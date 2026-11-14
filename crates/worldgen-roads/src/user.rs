//! User-drawn roads (editor). Stored as the click polylines; profiles are rebuilt deterministically
//! from the terrain, so saved files stay valid and tiny.

use crate::highways::{chaikin, profile_road, resample};
use crate::net::{Pad, Road, RoadClass};
use crate::world::RoadOverlay;
use std::io::{self, Read, Write};
use std::sync::{Arc, RwLock};
use worldgen_terrain::{Bounds, TerrainWorld};

const MAGIC: &[u8; 8] = b"WGROAD01";

struct Entry {
    class: RoadClass,
    clicks: Vec<[f64; 2]>,
    road: Arc<Road>,
}

pub struct UserRoads {
    terrain: Arc<TerrainWorld>,
    entries: RwLock<Vec<Entry>>,
    next_id: std::sync::atomic::AtomicU64,
}

fn class_from(b: u8) -> RoadClass {
    match b {
        0 => RoadClass::Highway,
        1 => RoadClass::Rural,
        2 => RoadClass::Avenue,
        _ => RoadClass::Street,
    }
}

impl UserRoads {
    pub fn new(terrain: Arc<TerrainWorld>) -> Arc<Self> {
        Arc::new(Self {
            terrain,
            entries: RwLock::new(Vec::new()),
            next_id: std::sync::atomic::AtomicU64::new(1),
        })
    }

    pub fn len(&self) -> usize {
        self.entries.read().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn build(&self, clicks: &[[f64; 2]], class: RoadClass, id: u64) -> Option<Road> {
        if clicks.len() < 2 {
            return None;
        }
        let pts = resample(&chaikin(clicks, 2), 8.0);
        let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
        for p in &pts {
            for k in 0..2 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        let ctx = self
            .terrain
            .ctx_for(lo[0] - 60.0, lo[1] - 60.0, hi[0] + 60.0, hi[1] + 60.0);
        profile_road(&self.terrain, &ctx, pts, None, class, id | (3 << 61))
    }

    /// Add a road through the clicked points. Returns the area to regenerate.
    pub fn add(&self, clicks: &[(f64, f64)], class: RoadClass) -> Option<Bounds> {
        let clicks: Vec<[f64; 2]> = clicks.iter().map(|c| [c.0, c.1]).collect();
        let id = self
            .next_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let road = Arc::new(self.build(&clicks, class, id)?);
        let (lo, hi) = road.bbox();
        self.entries.write().unwrap().push(Entry {
            class,
            clicks,
            road,
        });
        Some(Bounds {
            min_x: lo[0] - 70.0,
            min_z: lo[1] - 70.0,
            max_x: hi[0] + 70.0,
            max_z: hi[1] + 70.0,
        })
    }

    /// Remove the most recently added road; returns its area.
    pub fn undo(&self) -> Option<Bounds> {
        let e = self.entries.write().unwrap().pop()?;
        let (lo, hi) = e.road.bbox();
        Some(Bounds {
            min_x: lo[0] - 70.0,
            min_z: lo[1] - 70.0,
            max_x: hi[0] + 70.0,
            max_z: hi[1] + 70.0,
        })
    }

    pub fn save(&self, w: &mut impl Write) -> io::Result<()> {
        let g = self.entries.read().unwrap();
        w.write_all(MAGIC)?;
        w.write_all(&(g.len() as u32).to_le_bytes())?;
        for e in g.iter() {
            w.write_all(&[e.class as u8])?;
            w.write_all(&(e.clicks.len() as u32).to_le_bytes())?;
            for c in &e.clicks {
                w.write_all(&c[0].to_le_bytes())?;
                w.write_all(&c[1].to_le_bytes())?;
            }
        }
        Ok(())
    }

    pub fn load(&self, r: &mut impl Read) -> io::Result<()> {
        let bad = |m: &str| io::Error::new(io::ErrorKind::InvalidData, m.to_string());
        let mut magic = [0u8; 8];
        r.read_exact(&mut magic)?;
        if &magic != MAGIC {
            return Err(bad("not a worldgen road file"));
        }
        let mut b4 = [0u8; 4];
        r.read_exact(&mut b4)?;
        let n = u32::from_le_bytes(b4);
        if n > 100_000 {
            return Err(bad("implausible road count"));
        }
        let mut entries = Vec::new();
        for i in 0..n {
            let mut c = [0u8; 1];
            r.read_exact(&mut c)?;
            r.read_exact(&mut b4)?;
            let k = u32::from_le_bytes(b4) as usize;
            if !(2..=100_000).contains(&k) {
                return Err(bad("implausible polyline"));
            }
            let mut clicks = Vec::with_capacity(k);
            let mut b8 = [0u8; 8];
            for _ in 0..k {
                r.read_exact(&mut b8)?;
                let x = f64::from_le_bytes(b8);
                r.read_exact(&mut b8)?;
                clicks.push([x, f64::from_le_bytes(b8)]);
            }
            let class = class_from(c[0]);
            let road = Arc::new(
                self.build(&clicks, class, i as u64 + 1)
                    .ok_or_else(|| bad("road could not be rebuilt"))?,
            );
            entries.push(Entry {
                class,
                clicks,
                road,
            });
        }
        self.next_id
            .store(n as u64 + 1, std::sync::atomic::Ordering::Relaxed);
        *self.entries.write().unwrap() = entries;
        Ok(())
    }
}

impl RoadOverlay for UserRoads {
    fn roads_near(&self, b: &Bounds) -> (Vec<Arc<Road>>, Vec<Pad>) {
        let roads = self
            .entries
            .read()
            .unwrap()
            .iter()
            .filter(|e| {
                let (lo, hi) = e.road.bbox();
                hi[0] >= b.min_x - 60.0
                    && lo[0] <= b.max_x + 60.0
                    && hi[1] >= b.min_z - 60.0
                    && lo[1] <= b.max_z + 60.0
            })
            .map(|e| e.road.clone())
            .collect();
        (roads, Vec::new())
    }

    fn stateful(&self) -> bool {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use worldgen_terrain::TerrainParams;

    #[test]
    fn add_undo_save_load_round_trip() {
        let t = TerrainWorld::new(TerrainParams::default());
        let u = UserRoads::new(t.clone());
        let b = u
            .add(
                &[(29000.0, 14000.0), (29200.0, 14100.0), (29500.0, 14050.0)],
                RoadClass::Rural,
            )
            .expect("road");
        assert!(b.max_x > 29500.0 && b.min_x < 29000.0);
        let near = Bounds {
            min_x: 29100.0,
            min_z: 14000.0,
            max_x: 29300.0,
            max_z: 14200.0,
        };
        let (roads, _) = u.roads_near(&near);
        assert_eq!(roads.len(), 1);
        let r = &roads[0];
        assert!(r.pts.len() > 20 && r.y.len() == r.pts.len());
        let max_g = r
            .y
            .windows(2)
            .zip(r.pts.windows(2))
            .map(|(y, p)| {
                (y[1] - y[0]).abs()
                    / (((p[1][0] - p[0][0]).powi(2) + (p[1][1] - p[0][1]).powi(2)).sqrt() as f32)
                        .max(0.1)
            })
            .fold(0.0f32, f32::max);
        assert!(max_g <= 0.075, "grade {max_g}");
        let far = Bounds {
            min_x: -9000.0,
            min_z: -9000.0,
            max_x: -8000.0,
            max_z: -8000.0,
        };
        assert!(u.roads_near(&far).0.is_empty());

        let mut buf = vec![];
        u.save(&mut buf).unwrap();
        let v = UserRoads::new(t);
        v.load(&mut buf.as_slice()).unwrap();
        let (again, _) = v.roads_near(&near);
        assert_eq!(again.len(), 1);
        assert!(
            again[0]
                .y
                .iter()
                .zip(&r.y)
                .all(|(a, b)| a.to_bits() == b.to_bits()),
            "rebuilt profile must be identical"
        );
        assert!(v.load(&mut &b"garbage_"[..]).is_err());
        assert!(u.undo().is_some());
        assert!(u.is_empty() && u.undo().is_none());
        assert!(
            u.add(&[(0.0, 0.0)], RoadClass::Street).is_none(),
            "a single click is not a road"
        );
    }
}
