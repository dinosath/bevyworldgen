//! Road data, spatial sampling, terrain carving and masks.

use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum RoadClass {
    Highway = 0,
    Rural,
    Avenue,
    Street,
}

impl RoadClass {
    pub fn half_width(self) -> f32 {
        match self {
            RoadClass::Highway => 5.6,
            RoadClass::Rural => 3.9,
            RoadClass::Avenue => 4.6,
            RoadClass::Street => 3.3,
        }
    }
    /// Flat verge beyond the asphalt.
    pub fn verge(self) -> f32 {
        match self {
            RoadClass::Highway => 1.6,
            RoadClass::Rural => 1.2,
            _ => 0.9,
        }
    }
    pub fn has_edge_lines(self) -> bool {
        matches!(
            self,
            RoadClass::Highway | RoadClass::Rural | RoadClass::Avenue
        )
    }
}

/// Bit flags returned by [`RoadSampler::mask`].
pub mod mask {
    pub const ROAD: u8 = 1;
    pub const VERGE: u8 = 2;
    pub const LOT: u8 = 4;
}

/// One road polyline with its vertical profile.
#[derive(Clone, Debug)]
pub struct Road {
    pub id: u64,
    pub class: RoadClass,
    pub pts: Vec<[f64; 2]>,
    /// Road surface elevation at each point.
    pub y: Vec<f32>,
    /// Per segment: carried on a bridge (terrain is not modified there).
    pub bridge: Vec<bool>,
}

impl Road {
    pub fn length(&self) -> f64 {
        self.pts
            .windows(2)
            .map(|w| ((w[1][0] - w[0][0]).powi(2) + (w[1][1] - w[0][1]).powi(2)).sqrt())
            .sum()
    }
    pub fn bbox(&self) -> ([f64; 2], [f64; 2]) {
        let mut lo = [f64::MAX; 2];
        let mut hi = [f64::MIN; 2];
        for p in &self.pts {
            for k in 0..2 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        (lo, hi)
    }
}

/// A building pad: terrain is levelled under it.
#[derive(Clone, Copy, Debug)]
pub struct Pad {
    pub cx: f64,
    pub cz: f64,
    pub rot: f32,
    pub half_w: f32,
    pub half_d: f32,
    pub y: f32,
}

#[derive(Clone, Copy)]
pub(crate) struct Seg {
    pub a: [f64; 2],
    pub b: [f64; 2],
    pub ya: f32,
    pub yb: f32,
    pub hw: f32,
    pub verge: f32,
    pub bridge: bool,
}

const BUCKET: f64 = 64.0;
/// Maximum embankment length (m) beyond the verge.
const MAX_BANK: f64 = 46.0;

/// Spatial index over road segments (and building pads) relevant to a region.
#[derive(Default)]
pub struct RoadSampler {
    pub roads: Vec<Arc<Road>>,
    pub(crate) segs: Vec<Seg>,
    grid: HashMap<(i32, i32), Vec<u32>>,
    pads: Vec<Pad>,
    pad_grid: HashMap<(i32, i32), Vec<u32>>,
}

impl RoadSampler {
    /// Approximate bytes owned by this index (segments, pads and grids; the shared `Road`s are not counted).
    pub fn approx_bytes(&self) -> usize {
        let cells =
            |g: &HashMap<(i32, i32), Vec<u32>>| g.values().map(|v| 48 + v.len() * 4).sum::<usize>();
        self.segs.len() * std::mem::size_of::<Seg>()
            + self.pads.len() * std::mem::size_of::<Pad>()
            + cells(&self.grid)
            + cells(&self.pad_grid)
    }

    pub fn is_empty(&self) -> bool {
        self.segs.is_empty() && self.pads.is_empty()
    }

    pub fn build(roads: Vec<Arc<Road>>, pads: Vec<Pad>, region: (f64, f64, f64, f64)) -> Self {
        let margin = MAX_BANK + 16.0;
        let mut s = RoadSampler {
            roads,
            ..Default::default()
        };
        for r in s.roads.clone().iter() {
            for i in 0..r.pts.len().saturating_sub(1) {
                let (a, b) = (r.pts[i], r.pts[i + 1]);
                let (lo_x, hi_x, lo_z, hi_z) = (
                    a[0].min(b[0]),
                    a[0].max(b[0]),
                    a[1].min(b[1]),
                    a[1].max(b[1]),
                );
                if hi_x < region.0 - margin
                    || lo_x > region.2 + margin
                    || hi_z < region.1 - margin
                    || lo_z > region.3 + margin
                {
                    continue;
                }
                let id = s.segs.len() as u32;
                s.segs.push(Seg {
                    a,
                    b,
                    ya: r.y[i],
                    yb: r.y[i + 1],
                    hw: r.class.half_width(),
                    verge: r.class.verge(),
                    bridge: r.bridge[i],
                });
                let inf = (r.class.half_width() + r.class.verge()) as f64 + MAX_BANK;
                for cz in
                    ((lo_z - inf) / BUCKET).floor() as i32..=((hi_z + inf) / BUCKET).floor() as i32
                {
                    for cx in ((lo_x - inf) / BUCKET).floor() as i32
                        ..=((hi_x + inf) / BUCKET).floor() as i32
                    {
                        s.grid.entry((cx, cz)).or_default().push(id);
                    }
                }
            }
        }
        for p in pads {
            let r = (p.half_w.max(p.half_d) as f64) * 1.5 + 14.0;
            if p.cx + r < region.0 - margin
                || p.cx - r > region.2 + margin
                || p.cz + r < region.1 - margin
                || p.cz - r > region.3 + margin
            {
                continue;
            }
            let id = s.pads.len() as u32;
            s.pads.push(p);
            for cz in ((p.cz - r) / BUCKET).floor() as i32..=((p.cz + r) / BUCKET).floor() as i32 {
                for cx in
                    ((p.cx - r) / BUCKET).floor() as i32..=((p.cx + r) / BUCKET).floor() as i32
                {
                    s.pad_grid.entry((cx, cz)).or_default().push(id);
                }
            }
        }
        s
    }

    /// Closest approach to a segment: (distance, interpolated profile y).
    #[inline]
    fn closest(s: &Seg, x: f64, z: f64) -> (f64, f32) {
        let (dx, dz) = (s.b[0] - s.a[0], s.b[1] - s.a[1]);
        let l2 = dx * dx + dz * dz;
        let t = if l2 > 0.0 {
            (((x - s.a[0]) * dx + (z - s.a[1]) * dz) / l2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let (px, pz) = (s.a[0] + dx * t, s.a[1] + dz * t);
        (
            ((x - px).powi(2) + (z - pz).powi(2)).sqrt(),
            s.ya + (s.yb - s.ya) * t as f32,
        )
    }

    /// Carve the terrain height `h` at `(x, z)` for roads and pads.
    pub fn modify(&self, x: f64, z: f64, h: f32) -> f32 {
        let key = ((x / BUCKET).floor() as i32, (z / BUCKET).floor() as i32);
        let mut cands: [(f64, f32, f32, f32); 8] = [(0.0, 0.0, 0.0, 0.0); 8];
        let mut n = 0;
        if let Some(ids) = self.grid.get(&key) {
            for &i in ids {
                let s = &self.segs[i as usize];
                if s.bridge {
                    continue;
                }
                let (d, y) = Self::closest(s, x, z);
                let flat = (s.hw + s.verge) as f64;
                if d < flat + MAX_BANK && n < 8 {
                    cands[n] = (d, y, s.hw + s.verge, 0.0);
                    n += 1;
                } else if d < flat + MAX_BANK {
                    // keep the 8 closest
                    if let Some(w) = (0..8).max_by(|&a, &b| cands[a].0.total_cmp(&cands[b].0))
                        && d < cands[w].0
                    {
                        cands[w] = (d, y, s.hw + s.verge, 0.0);
                    }
                }
            }
        }
        let mut h = h;
        if n > 0 {
            // farthest first so the nearest road dominates at junctions
            let c = &mut cands[..n];
            c.sort_by(|a, b| b.0.total_cmp(&a.0));
            for &(d, y, flat, _) in c.iter() {
                let flat = flat as f64;
                if d <= flat {
                    h = y;
                } else {
                    let l = (2.2 * (h - y).abs() as f64).clamp(3.0, MAX_BANK);
                    let t = ((d - flat) / l).clamp(0.0, 1.0);
                    let t = t * t * (3.0 - 2.0 * t);
                    h = y + (h - y) * t as f32;
                }
            }
        }
        // building pads
        if let Some(ids) = self.pad_grid.get(&key) {
            for &i in ids {
                let p = &self.pads[i as usize];
                let (dx, dz) = (x - p.cx, z - p.cz);
                let (s, c) = (libm_sin(p.rot), libm_cos(p.rot));
                let (lx, lz) = (
                    (dx * c as f64 + dz * s as f64).abs(),
                    (-dx * s as f64 + dz * c as f64).abs(),
                );
                let (ex, ez) = (
                    (lx - p.half_w as f64).max(0.0),
                    (lz - p.half_d as f64).max(0.0),
                );
                let d = (ex * ex + ez * ez).sqrt();
                if d < 10.0 {
                    let t = (d / 10.0).clamp(0.0, 1.0);
                    let t = t * t * (3.0 - 2.0 * t);
                    h = p.y + (h - p.y) * t as f32;
                }
            }
        }
        h
    }

    /// Surface flags at `(x, z)` (road asphalt, verge, building pad).
    pub fn mask(&self, x: f64, z: f64) -> u8 {
        let key = ((x / BUCKET).floor() as i32, (z / BUCKET).floor() as i32);
        let mut m = 0u8;
        if let Some(ids) = self.grid.get(&key) {
            for &i in ids {
                let s = &self.segs[i as usize];
                let (d, _) = Self::closest(s, x, z);
                if d <= s.hw as f64 {
                    m |= mask::ROAD;
                } else if d <= (s.hw + s.verge) as f64 + 2.5 {
                    m |= mask::VERGE;
                }
            }
        }
        if let Some(ids) = self.pad_grid.get(&key) {
            for &i in ids {
                let p = &self.pads[i as usize];
                let (dx, dz) = (x - p.cx, z - p.cz);
                let (s, c) = (libm_sin(p.rot), libm_cos(p.rot));
                let (lx, lz) = (
                    (dx * c as f64 + dz * s as f64).abs(),
                    (-dx * s as f64 + dz * c as f64).abs(),
                );
                if lx <= p.half_w as f64 + 1.5 && lz <= p.half_d as f64 + 1.5 {
                    m |= mask::LOT;
                }
            }
        }
        m
    }

    /// True if a building pad lies within `r` metres of `(x, z)` (keeps gardens clear of trees).
    pub fn near_pad(&self, x: f64, z: f64, r: f64) -> bool {
        let (c0x, c1x) = (
            ((x - r) / BUCKET).floor() as i32,
            ((x + r) / BUCKET).floor() as i32,
        );
        let (c0z, c1z) = (
            ((z - r) / BUCKET).floor() as i32,
            ((z + r) / BUCKET).floor() as i32,
        );
        for cz in c0z..=c1z {
            for cx in c0x..=c1x {
                if let Some(ids) = self.pad_grid.get(&(cx, cz)) {
                    for &i in ids {
                        let p = &self.pads[i as usize];
                        let reach = p.half_w.hypot(p.half_d) as f64 + r;
                        if (x - p.cx).powi(2) + (z - p.cz).powi(2) < reach * reach {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    /// Distance to the nearest road centreline minus its half width (negative on the asphalt).
    pub fn clearance(&self, x: f64, z: f64) -> f64 {
        let key = ((x / BUCKET).floor() as i32, (z / BUCKET).floor() as i32);
        let mut best = f64::MAX;
        if let Some(ids) = self.grid.get(&key) {
            for &i in ids {
                let s = &self.segs[i as usize];
                best = best.min(Self::closest(s, x, z).0 - s.hw as f64);
            }
        }
        best
    }

    /// Elevation of the road surface under `(x, z)` if it lies on a road (incl. bridges).
    pub fn surface_y(&self, x: f64, z: f64) -> Option<f32> {
        let key = ((x / BUCKET).floor() as i32, (z / BUCKET).floor() as i32);
        let ids = self.grid.get(&key)?;
        let mut best: Option<(f64, f32)> = None;
        for &i in ids {
            let s = &self.segs[i as usize];
            let (d, y) = Self::closest(s, x, z);
            if d <= s.hw as f64 && best.is_none_or(|b| d < b.0) {
                best = Some((d, y));
            }
        }
        best.map(|b| b.1)
    }
}

// The roads layer needs only sin/cos of lot rotations, via the portable libm used by tensor.
fn libm_sin(a: f32) -> f32 {
    symbios_tensor::MathMode::Portable.sin(a)
}
fn libm_cos(a: f32) -> f32 {
    symbios_tensor::MathMode::Portable.cos(a)
}
