//! Persistent terrain edits as sparse delta layers on the lod-0 vertex grid.
//!
//! Deltas are stored per 32x32-vertex block keyed by block coordinates and sampled bilinearly in
//! world space, so they are a pure function of world position: borders between chunks stay exact,
//! every LOD sees the same edit, and edits survive chunk eviction. Saved/loaded as a small binary
//! file.

use crate::chunk::{Bounds, HeightModifier, RegionModifier};
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

pub const BLOCK: i32 = 32;
const BLOCK_LEN: usize = (BLOCK * BLOCK) as usize;
const MAGIC: &[u8; 8] = b"WGEDIT01";

type Block = Arc<[f32; BLOCK_LEN]>;

/// Brush operation. Operations that need the current surface take a height-query closure.
pub enum BrushOp<'a> {
    Raise(f32),
    Lower(f32),
    /// Relax toward the local average by `strength` (0..1); `height_at` returns the current surface.
    Smooth {
        strength: f32,
        height_at: &'a dyn Fn(f64, f64) -> Option<f32>,
    },
}

/// Opaque snapshot of an [`EditLayer`].
#[derive(Clone)]
pub struct EditSnapshot(HashMap<(i32, i32), Block>);

pub struct EditLayer {
    cell_m: f64,
    blocks: RwLock<HashMap<(i32, i32), Block>>,
    revision: AtomicU64,
}

#[inline]
fn falloff(d: f64, r: f64) -> f32 {
    let t = (1.0 - d / r).clamp(0.0, 1.0);
    (t * t * (3.0 - 2.0 * t)) as f32
}

impl EditLayer {
    /// `cell_m` must equal the lod-0 vertex spacing of the world's chunk config.
    pub fn new(cell_m: f64) -> Arc<Self> {
        Arc::new(Self {
            cell_m,
            blocks: RwLock::new(HashMap::new()),
            revision: AtomicU64::new(1),
        })
    }

    pub fn revision_value(&self) -> u64 {
        self.revision.load(Ordering::Relaxed)
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.read().unwrap().is_empty()
    }

    pub fn block_count(&self) -> usize {
        self.blocks.read().unwrap().len()
    }

    /// Edit delta in metres at world `(x, z)`.
    pub fn delta_at(&self, x: f64, z: f64) -> f32 {
        let g = self.blocks.read().unwrap();
        sample(&g, self.cell_m, x, z)
    }

    /// Apply a circular brush; returns the affected world rectangle (for chunk invalidation).
    pub fn apply_brush(&self, cx: f64, cz: f64, radius: f64, op: &BrushOp) -> Bounds {
        let cell = self.cell_m;
        let (i0, i1) = (
            ((cx - radius) / cell).floor() as i64,
            ((cx + radius) / cell).ceil() as i64,
        );
        let (j0, j1) = (
            ((cz - radius) / cell).floor() as i64,
            ((cz + radius) / cell).ceil() as i64,
        );
        let mut g = self.blocks.write().unwrap();
        // snapshot of "before" values for read-modify-write ops that sample neighbours
        let mut writes: Vec<(i64, i64, f32)> = Vec::new();
        for j in j0..=j1 {
            for i in i0..=i1 {
                let (x, z) = (i as f64 * cell, j as f64 * cell);
                let d = ((x - cx).powi(2) + (z - cz).powi(2)).sqrt();
                if d >= radius {
                    continue;
                }
                let f = falloff(d, radius);
                let delta = match op {
                    BrushOp::Raise(a) => a * f,
                    BrushOp::Lower(a) => -a * f,
                    BrushOp::Smooth {
                        strength,
                        height_at,
                    } => {
                        let h = height_at(x, z);
                        let n = [
                            height_at(x - cell, z),
                            height_at(x + cell, z),
                            height_at(x, z - cell),
                            height_at(x, z + cell),
                        ];
                        match (h, n[0], n[1], n[2], n[3]) {
                            (Some(h), Some(a), Some(b), Some(c), Some(d4)) => {
                                ((a + b + c + d4) * 0.25 - h) * strength * f
                            }
                            _ => 0.0,
                        }
                    }
                };
                writes.push((i, j, delta));
            }
        }
        for (i, j, delta) in writes {
            if delta != 0.0 {
                add_vertex(&mut g, i, j, delta);
            }
        }
        drop(g);
        self.revision.fetch_add(1, Ordering::Relaxed);
        let m = cell * 2.0;
        Bounds {
            min_x: cx - radius - m,
            min_z: cz - radius - m,
            max_x: cx + radius + m,
            max_z: cz + radius + m,
        }
    }

    /// Flatten toward an absolute height. Needs the current surface, so it takes a query.
    pub fn flatten(
        &self,
        cx: f64,
        cz: f64,
        radius: f64,
        target: f32,
        strength: f32,
        height_at: &dyn Fn(f64, f64) -> Option<f32>,
    ) -> Bounds {
        let cell = self.cell_m;
        let (i0, i1) = (
            ((cx - radius) / cell).floor() as i64,
            ((cx + radius) / cell).ceil() as i64,
        );
        let (j0, j1) = (
            ((cz - radius) / cell).floor() as i64,
            ((cz + radius) / cell).ceil() as i64,
        );
        let mut g = self.blocks.write().unwrap();
        let mut writes = Vec::new();
        for j in j0..=j1 {
            for i in i0..=i1 {
                let (x, z) = (i as f64 * cell, j as f64 * cell);
                let d = ((x - cx).powi(2) + (z - cz).powi(2)).sqrt();
                if d >= radius {
                    continue;
                }
                if let Some(h) = height_at(x, z) {
                    writes.push((i, j, (target - h) * strength * falloff(d, radius)));
                }
            }
        }
        for (i, j, d) in writes {
            if d != 0.0 {
                add_vertex(&mut g, i, j, d);
            }
        }
        drop(g);
        self.revision.fetch_add(1, Ordering::Relaxed);
        let m = cell * 2.0;
        Bounds {
            min_x: cx - radius - m,
            min_z: cz - radius - m,
            max_x: cx + radius + m,
            max_z: cz + radius + m,
        }
    }

    /// Cheap copy-on-write snapshot (shares blocks) for undo/redo.
    pub fn snapshot(&self) -> EditSnapshot {
        EditSnapshot(self.blocks.read().unwrap().clone())
    }

    /// Restore a snapshot; returns the world rectangle covering every block that differs.
    pub fn restore(&self, snap: &EditSnapshot) -> Option<Bounds> {
        let bs = BLOCK as f64 * self.cell_m;
        let mut g = self.blocks.write().unwrap();
        let mut keys: Vec<(i32, i32)> = g.keys().chain(snap.0.keys()).copied().collect();
        keys.sort();
        keys.dedup();
        let mut hit: Option<Bounds> = None;
        for k in keys {
            let differs = match (g.get(&k), snap.0.get(&k)) {
                (Some(a), Some(b)) => !Arc::ptr_eq(a, b) && a[..] != b[..],
                (None, None) => false,
                _ => true,
            };
            if differs {
                let r = Bounds {
                    min_x: k.0 as f64 * bs - self.cell_m,
                    min_z: k.1 as f64 * bs - self.cell_m,
                    max_x: (k.0 + 1) as f64 * bs + self.cell_m,
                    max_z: (k.1 + 1) as f64 * bs + self.cell_m,
                };
                hit = Some(match hit {
                    None => r,
                    Some(h) => Bounds {
                        min_x: h.min_x.min(r.min_x),
                        min_z: h.min_z.min(r.min_z),
                        max_x: h.max_x.max(r.max_x),
                        max_z: h.max_z.max(r.max_z),
                    },
                });
            }
        }
        *g = snap.0.clone();
        drop(g);
        self.revision.fetch_add(1, Ordering::Relaxed);
        hit
    }

    pub fn clear(&self) {
        self.blocks.write().unwrap().clear();
        self.revision.fetch_add(1, Ordering::Relaxed);
    }

    pub fn save(&self, w: &mut impl Write) -> io::Result<()> {
        let g = self.blocks.read().unwrap();
        w.write_all(MAGIC)?;
        w.write_all(&(self.cell_m as f32).to_le_bytes())?;
        let mut keys: Vec<_> = g.keys().copied().collect();
        keys.sort();
        w.write_all(&(keys.len() as u32).to_le_bytes())?;
        for k in keys {
            w.write_all(&k.0.to_le_bytes())?;
            w.write_all(&k.1.to_le_bytes())?;
            for v in g[&k].iter() {
                w.write_all(&v.to_le_bytes())?;
            }
        }
        Ok(())
    }

    pub fn load(&self, r: &mut impl Read) -> io::Result<()> {
        let bad = |m: &str| io::Error::new(io::ErrorKind::InvalidData, m.to_string());
        let mut magic = [0u8; 8];
        r.read_exact(&mut magic)?;
        if &magic != MAGIC {
            return Err(bad("not a worldgen edit file"));
        }
        let mut b4 = [0u8; 4];
        r.read_exact(&mut b4)?;
        if (f32::from_le_bytes(b4) as f64 - self.cell_m).abs() > 1e-6 {
            return Err(bad("edit file cell size does not match this world"));
        }
        r.read_exact(&mut b4)?;
        let n = u32::from_le_bytes(b4);
        let mut map = HashMap::new();
        for _ in 0..n {
            r.read_exact(&mut b4)?;
            let bx = i32::from_le_bytes(b4);
            r.read_exact(&mut b4)?;
            let bz = i32::from_le_bytes(b4);
            let mut blk = [0f32; BLOCK_LEN];
            for v in blk.iter_mut() {
                r.read_exact(&mut b4)?;
                *v = f32::from_le_bytes(b4);
            }
            map.insert((bx, bz), Arc::new(blk));
        }
        *self.blocks.write().unwrap() = map;
        self.revision.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
}

fn split(i: i64) -> (i32, usize) {
    let b = i.div_euclid(BLOCK as i64) as i32;
    (b, i.rem_euclid(BLOCK as i64) as usize)
}

fn vertex(g: &HashMap<(i32, i32), Block>, i: i64, j: i64) -> f32 {
    let ((bx, li), (bz, lj)) = (split(i), split(j));
    g.get(&(bx, bz))
        .map_or(0.0, |b| b[lj * BLOCK as usize + li])
}

fn add_vertex(g: &mut HashMap<(i32, i32), Block>, i: i64, j: i64, d: f32) {
    let ((bx, li), (bz, lj)) = split2(i, j);
    let b = g
        .entry((bx, bz))
        .or_insert_with(|| Arc::new([0.0; BLOCK_LEN]));
    Arc::make_mut(b)[lj * BLOCK as usize + li] += d;
}

fn split2(i: i64, j: i64) -> ((i32, usize), (i32, usize)) {
    (split(i), split(j))
}

fn sample(g: &HashMap<(i32, i32), Block>, cell: f64, x: f64, z: f64) -> f32 {
    let (fx, fz) = (x / cell, z / cell);
    let (i, j) = (fx.floor() as i64, fz.floor() as i64);
    let (tx, tz) = ((fx - i as f64) as f32, (fz - j as f64) as f32);
    let v = |a, b| vertex(g, a, b);
    (v(i, j) * (1.0 - tx) + v(i + 1, j) * tx) * (1.0 - tz)
        + (v(i, j + 1) * (1.0 - tx) + v(i + 1, j + 1) * tx) * tz
}

struct Prepared {
    cell: f64,
    blocks: HashMap<(i32, i32), Block>,
}

impl RegionModifier for Prepared {
    fn apply(&self, x: f64, z: f64, h: f32) -> f32 {
        h + sample(&self.blocks, self.cell, x, z)
    }
}

impl HeightModifier for EditLayer {
    fn prepare(&self, b: &Bounds, spacing: f64) -> Option<Box<dyn RegionModifier>> {
        let g = self.blocks.read().unwrap();
        if g.is_empty() {
            return None;
        }
        let bs = BLOCK as f64 * self.cell_m;
        let m = spacing * 2.0 + self.cell_m;
        let (bx0, bx1) = (
            ((b.min_x - m) / bs).floor() as i32 - 1,
            ((b.max_x + m) / bs).floor() as i32 + 1,
        );
        let (bz0, bz1) = (
            ((b.min_z - m) / bs).floor() as i32 - 1,
            ((b.max_z + m) / bs).floor() as i32 + 1,
        );
        let sel: HashMap<_, _> = g
            .iter()
            .filter(|(k, _)| k.0 >= bx0 && k.0 <= bx1 && k.1 >= bz0 && k.1 <= bz1)
            .map(|(k, v)| (*k, v.clone()))
            .collect();
        if sel.is_empty() {
            None
        } else {
            Some(Box::new(Prepared {
                cell: self.cell_m,
                blocks: sel,
            }))
        }
    }

    fn revision(&self) -> u64 {
        self.revision_value()
    }

    fn stateful(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brush_is_local_and_symmetric_across_blocks() {
        let e = EditLayer::new(2.0);
        let b = e.apply_brush(63.0, -1.0, 20.0, &BrushOp::Raise(5.0));
        assert!(b.min_x < 63.0 && b.max_x > 63.0);
        assert!(e.delta_at(63.0, -1.0) > 4.5);
        assert_eq!(e.delta_at(200.0, 200.0), 0.0);
        // spans several blocks (block = 64 m)
        assert!(e.block_count() >= 2);
        let (a, c) = (e.delta_at(63.0 - 10.0, -1.0), e.delta_at(63.0 + 10.0, -1.0));
        assert!((a - c).abs() < 1e-5);
    }

    #[test]
    fn save_load_roundtrip() {
        let e = EditLayer::new(2.0);
        e.apply_brush(10.0, 10.0, 30.0, &BrushOp::Raise(3.0));
        e.apply_brush(-100.0, 40.0, 15.0, &BrushOp::Lower(2.0));
        let mut buf = vec![];
        e.save(&mut buf).unwrap();
        let f = EditLayer::new(2.0);
        f.load(&mut buf.as_slice()).unwrap();
        for p in [(10.0, 10.0), (-100.0, 40.0), (0.0, 0.0), (25.3, 11.1)] {
            assert_eq!(
                e.delta_at(p.0, p.1).to_bits(),
                f.delta_at(p.0, p.1).to_bits()
            );
        }
        assert!(
            EditLayer::new(4.0).load(&mut buf.as_slice()).is_err(),
            "cell size mismatch must be rejected"
        );
        assert!(f.load(&mut &b"garbage!"[..]).is_err());
    }

    #[test]
    fn undo_redo_via_snapshots_restores_exactly_and_reports_the_area() {
        let e = EditLayer::new(2.0);
        let s0 = e.snapshot();
        e.apply_brush(10.0, 10.0, 25.0, &BrushOp::Raise(4.0));
        let s1 = e.snapshot();
        e.apply_brush(300.0, -40.0, 20.0, &BrushOp::Lower(3.0));
        let after = e.delta_at(300.0, -40.0);
        assert!(after < -2.5);
        // undo the second edit only
        let b = e.restore(&s1).expect("something changed");
        assert!(
            b.min_x <= 300.0 && b.max_x >= 300.0 && b.min_z <= -40.0 && b.max_z >= -40.0,
            "dirty area must cover the undone brush: {b:?}"
        );
        assert_eq!(e.delta_at(300.0, -40.0), 0.0);
        assert!(e.delta_at(10.0, 10.0) > 3.5);
        // undo everything
        assert!(e.restore(&s0).is_some());
        assert!(e.is_empty());
        assert!(
            e.restore(&s0).is_none(),
            "restoring an identical state changes nothing"
        );
        // redo
        e.restore(&s1);
        assert!(e.delta_at(10.0, 10.0) > 3.5);
    }

    #[test]
    fn flatten_moves_toward_target() {
        let e = EditLayer::new(2.0);
        let surface = |_x: f64, _z: f64| Some(100.0f32);
        e.flatten(0.0, 0.0, 30.0, 110.0, 1.0, &surface);
        assert!((e.delta_at(0.0, 0.0) - 10.0).abs() < 1e-3);
    }
}
