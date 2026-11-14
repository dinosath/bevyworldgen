//! Drainage-based rivers. Sources are placed deterministically per 16 km region on high ground and
//! traced *downhill* over the smooth macro terrain (steepest descent with inertia), so rivers
//! follow the landscape, merge naturally where traces coincide, widen downstream and have a
//! monotonically non-increasing water surface. Everything is a pure function of the seed, so any
//! chunk can rebuild the rivers that cross it.

use crate::height::{Lake, TerrainWorld};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};
use worldgen_core::{Subsystem, derive_seed, unit_f64};

pub const REGION_M: f64 = 16_000.0;
const STEP_M: f64 = 60.0;
const MAX_STEPS: usize = 450;
const SOURCES_PER_REGION: u64 = 10;
/// Rivers from regions within this many rings can reach a chunk.
pub const REGION_RINGS: i32 = 2;
const BUCKET_M: f64 = 256.0;
/// Influence radius multiple of the half width (carve + banks).
pub const INFLUENCE: f64 = 9.0;

#[derive(Clone, Debug)]
pub struct River {
    pub pts: Vec<(f64, f64)>,
    /// Water surface elevation at each point (non-increasing downstream).
    pub level: Vec<f64>,
    pub half_w: Vec<f64>,
    pub depth: Vec<f64>,
}

#[derive(Clone, Copy)]
struct Seg {
    a: (f64, f64),
    b: (f64, f64),
    ia: (f32, f32, f32), // level, half_w, depth at a
    ib: (f32, f32, f32),
}

/// Spatial index over the river segments that may influence one chunk region.
#[derive(Default)]
pub struct RiverSampler {
    segs: Vec<Seg>,
    grid: HashMap<(i32, i32), Vec<u32>>,
}

impl RiverSampler {
    pub fn is_empty(&self) -> bool {
        self.segs.is_empty()
    }

    pub fn segment_count(&self) -> usize {
        self.segs.len()
    }

    fn build(rivers: &[Arc<RegionData>], min_x: f64, min_z: f64, max_x: f64, max_z: f64) -> Self {
        let margin = 24.0 * INFLUENCE + BUCKET_M;
        let mut s = RiverSampler::default();
        for set in rivers {
            for r in set.rivers.iter() {
                for i in 0..r.pts.len().saturating_sub(1) {
                    let (a, b) = (r.pts[i], r.pts[i + 1]);
                    let (lo_x, hi_x) = (a.0.min(b.0), a.0.max(b.0));
                    let (lo_z, hi_z) = (a.1.min(b.1), a.1.max(b.1));
                    if hi_x < min_x - margin
                        || lo_x > max_x + margin
                        || hi_z < min_z - margin
                        || lo_z > max_z + margin
                    {
                        continue;
                    }
                    let hw = r.half_w[i].max(r.half_w[i + 1]);
                    let inf = hw * INFLUENCE;
                    let id = s.segs.len() as u32;
                    s.segs.push(Seg {
                        a,
                        b,
                        ia: (r.level[i] as f32, r.half_w[i] as f32, r.depth[i] as f32),
                        ib: (
                            r.level[i + 1] as f32,
                            r.half_w[i + 1] as f32,
                            r.depth[i + 1] as f32,
                        ),
                    });
                    let (cx0, cx1) = (
                        ((lo_x - inf) / BUCKET_M).floor() as i32,
                        ((hi_x + inf) / BUCKET_M).floor() as i32,
                    );
                    let (cz0, cz1) = (
                        ((lo_z - inf) / BUCKET_M).floor() as i32,
                        ((hi_z + inf) / BUCKET_M).floor() as i32,
                    );
                    for cz in cz0..=cz1 {
                        for cx in cx0..=cx1 {
                            s.grid.entry((cx, cz)).or_default().push(id);
                        }
                    }
                }
            }
        }
        s
    }

    /// Apply every river channel in range to terrain height `h`; channels combine with `min`, so
    /// overlapping rivers stay continuous. Returns `(new height, bank 0..1, water level or -inf)`.
    pub fn carve(&self, x: f64, z: f64, h: f64) -> (f64, f64, f64) {
        let cell = ((x / BUCKET_M).floor() as i32, (z / BUCKET_M).floor() as i32);
        let Some(ids) = self.grid.get(&cell) else {
            return (h, 0.0, f64::NEG_INFINITY);
        };
        let (mut out, mut bank, mut water) = (h, 0.0f64, f64::NEG_INFINITY);
        let mut any = false;
        for &i in ids {
            let s = &self.segs[i as usize];
            let (dx, dz) = (s.b.0 - s.a.0, s.b.1 - s.a.1);
            let l2 = dx * dx + dz * dz;
            let t = if l2 > 0.0 {
                (((x - s.a.0) * dx + (z - s.a.1) * dz) / l2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let (px, pz) = (s.a.0 + dx * t, s.a.1 + dz * t);
            let d = ((x - px).powi(2) + (z - pz).powi(2)).sqrt();
            let tf = t as f32;
            let lerp = |a: f32, b: f32| (a + (b - a) * tf) as f64;
            let hw = lerp(s.ia.1, s.ib.1);
            if d >= hw * INFLUENCE {
                continue;
            }
            let (lvl, depth) = (lerp(s.ia.0, s.ib.0), lerp(s.ia.2, s.ib.2));
            let bed = if d < hw {
                let u = d / hw;
                lvl - depth * (1.0 - u * u)
            } else {
                lvl + 0.30 * (d - hw)
            };
            let w = (d - hw) / (hw * (INFLUENCE - 1.0));
            let t2 = w.clamp(0.0, 1.0);
            let t2 = t2 * t2 * (3.0 - 2.0 * t2);
            let nh = bed + (h - bed) * t2;
            // gentle levees close to the channel, otherwise only ever lower the terrain
            let cand = if d < hw * 2.0 {
                nh.min(h + 3.0)
            } else {
                nh.min(h)
            };
            out = if any { out.min(cand) } else { cand };
            any = true;
            let b = 1.0 - ((d - hw) / (hw * 5.0)).clamp(0.0, 1.0);
            bank = bank.max(b * b * (3.0 - 2.0 * b));
            if d < hw {
                water = water.max(lvl);
            }
        }
        (out, bank, water)
    }
}

/// Rivers and the ponds/lakes created where they filled basins, for one 16 km source region.
#[derive(Default)]
pub struct RegionData {
    pub rivers: Vec<River>,
    pub ponds: Vec<Lake>,
}

type RegionRivers = Arc<OnceLock<Arc<RegionData>>>;

#[derive(Default)]
pub(crate) struct RiverCache {
    regions: RwLock<HashMap<(i32, i32), RegionRivers>>,
}

impl TerrainWorld {
    fn trace_river(&self, sx: f64, sz: f64) -> Option<(River, Vec<Lake>)> {
        let sea = self.params.sea_level as f64;
        let mut pts = vec![(sx, sz)];
        let mut hs = vec![self.macro_raw(sx, sz, 0.0).h];
        let (mut x, mut z) = (sx, sz);
        let (mut dirx, mut dirz) = (0.0f64, 0.0f64);
        let e = 30.0;
        let mut ponds: Vec<Lake> = Vec::new();
        for _ in 0..MAX_STEPS {
            // two-scale gradient: the wide stencil sees through small basins so streams
            // keep flowing the way rivers overflow minor depressions
            let grad = |e: f64| {
                (
                    self.macro_raw(x + e, z, 0.0).h - self.macro_raw(x - e, z, 0.0).h,
                    self.macro_raw(x, z + e, 0.0).h - self.macro_raw(x, z - e, 0.0).h,
                )
            };
            let (a, b) = (grad(e), grad(e * 9.0));
            let (hx, hz) = (
                a.0 / (2.0 * e) * 0.35 + b.0 / (18.0 * e) * 0.65,
                a.1 / (2.0 * e) * 0.35 + b.1 / (18.0 * e) * 0.65,
            );
            let g = (hx * hx + hz * hz).sqrt();
            let (mut nx, mut nz) = if g > 1e-7 {
                (-hx / g, -hz / g)
            } else {
                (dirx, dirz)
            };
            if dirx != 0.0 || dirz != 0.0 {
                nx = nx * 0.5 + dirx * 0.5;
                nz = nz * 0.5 + dirz * 0.5;
            }
            let l = (nx * nx + nz * nz).sqrt();
            if l < 1e-9 {
                break;
            }
            (dirx, dirz) = (nx / l, nz / l);
            x += dirx * STEP_M;
            z += dirz * STEP_M;
            let h = self.macro_raw(x, z, 0.0).h;
            pts.push((x, z));
            hs.push(h);
            if h < sea + 0.3 {
                break;
            }
            // trapped in a basin (the path turns back on itself): fill it into a pond and spill
            // over the lowest rim, as water does
            let n = pts.len();
            if n > 10
                && pts[..n - 10]
                    .iter()
                    .any(|p| (p.0 - x).powi(2) + (p.1 - z).powi(2) < (STEP_M * 1.1).powi(2))
            {
                if ponds.len() >= 8 {
                    break;
                }
                let m = (n.saturating_sub(40)..n)
                    .min_by(|a, b| hs[*a].total_cmp(&hs[*b]))
                    .unwrap();
                let (cx, cz, hc) = (pts[m].0, pts[m].1, hs[m]);
                // lowest point on rings around the basin bottom
                let mut best: Option<(f64, f64, f64, f64)> = None; // (h, x, z, radius)
                for &rad in &[140.0, 260.0, 480.0] {
                    for k in 0..16 {
                        let (s_, c_) = ring_dir(k);
                        let (px, pz) = (cx + c_ * rad, cz + s_ * rad);
                        let hh = self.macro_raw(px, pz, 0.0).h;
                        if best.is_none_or(|b| hh < b.0) {
                            best = Some((hh, px, pz, rad));
                        }
                    }
                }
                let Some((rim, rx, rz, rad)) = best else {
                    break;
                };
                if rim < sea + 0.5 {
                    break;
                }
                let level = rim - 0.4;
                ponds.push(Lake {
                    cx,
                    cz,
                    radius: rad * 0.85,
                    level,
                    depth: (level - hc + 1.5).clamp(2.0, 14.0),
                });
                // continue downstream from the spill point
                let (dx, dz) = (rx - cx, rz - cz);
                let l = (dx * dx + dz * dz).sqrt().max(1.0);
                (dirx, dirz) = (dx / l, dz / l);
                x = rx;
                z = rz;
                pts.push((x, z));
                hs.push(self.macro_raw(x, z, 0.0).h);
            }
        }
        // a stream must actually descend (reject traces that stalled near their source)
        if pts.len() < 20 || hs[0] - hs[hs.len() - 1] < 4.0 {
            return None;
        }
        // Water surface = smoothed terrain minus a constant: a pure function of position, so
        // rivers that overlap (merging traces) agree on the level and never form cliffs.
        let n = pts.len();
        let mut level = Vec::with_capacity(n);
        let mut half_w = Vec::with_capacity(n);
        let mut depth = Vec::with_capacity(n);
        for (i, &(px, pz)) in pts.iter().enumerate() {
            let hw = (2.6 + 0.00052 * (i as f64 * STEP_M)).min(21.0);
            level.push(self.smooth_level(px, pz) - 2.2);
            half_w.push(hw);
            depth.push(1.4 + hw * 0.24);
        }
        Some((
            River {
                pts,
                level,
                half_w,
                depth,
            },
            ponds,
        ))
    }

    /// Terrain averaged over a ~400 m disc: smooth enough to serve as a river water surface.
    fn smooth_level(&self, x: f64, z: f64) -> f64 {
        const R: f64 = 400.0;
        const D: f64 = std::f64::consts::FRAC_1_SQRT_2 * R;
        let pts = [
            (0.0, 0.0),
            (R, 0.0),
            (-R, 0.0),
            (0.0, R),
            (0.0, -R),
            (D, D),
            (-D, D),
            (D, -D),
            (-D, -D),
        ];
        pts.iter()
            .map(|(dx, dz)| self.macro_raw(x + dx, z + dz, 0.0).h)
            .sum::<f64>()
            / 9.0
    }

    fn build_region_rivers(&self, rx: i32, rz: i32) -> Arc<RegionData> {
        let sd = self.params.seed;
        let mut out = RegionData::default();
        for k in 0..SOURCES_PER_REGION {
            let h = |salt| {
                derive_seed(
                    sd,
                    Subsystem::Hydrology,
                    rx as i64,
                    rz as i64,
                    0x1000 + k * 8 + salt,
                )
            };
            let x = (rx as f64 + unit_f64(h(1))) * REGION_M;
            let z = (rz as f64 + unit_f64(h(2))) * REGION_M;
            let m = self.macro_raw(x, z, 0.0);
            if m.land < 0.95 || m.h < 120.0 || m.h > 1150.0 {
                continue;
            }
            if let Some((r, p)) = self.trace_river(x, z) {
                out.rivers.push(r);
                out.ponds.extend(p);
            }
        }
        Arc::new(out)
    }

    fn region_rivers(&self, rx: i32, rz: i32) -> Arc<RegionData> {
        let cell = {
            if let Some(c) = self.rivers.regions.read().unwrap().get(&(rx, rz)) {
                c.clone()
            } else {
                self.rivers
                    .regions
                    .write()
                    .unwrap()
                    .entry((rx, rz))
                    .or_default()
                    .clone()
            }
        };
        cell.get_or_init(|| self.build_region_rivers(rx, rz))
            .clone()
    }

    /// River index for chunk bounds (empty if rivers are disabled).
    pub fn river_sampler(
        &self,
        min_x: f64,
        min_z: f64,
        max_x: f64,
        max_z: f64,
    ) -> Arc<RiverSampler> {
        if !self.params.rivers {
            return Arc::new(RiverSampler::default());
        }
        let (r0x, r1x) = (
            (min_x / REGION_M).floor() as i32 - REGION_RINGS,
            (max_x / REGION_M).floor() as i32 + REGION_RINGS,
        );
        let (r0z, r1z) = (
            (min_z / REGION_M).floor() as i32 - REGION_RINGS,
            (max_z / REGION_M).floor() as i32 + REGION_RINGS,
        );
        let mut sets = Vec::new();
        for rz in r0z..=r1z {
            for rx in r0x..=r1x {
                sets.push(self.region_rivers(rx, rz));
            }
        }
        Arc::new(RiverSampler::build(&sets, min_x, min_z, max_x, max_z))
    }

    /// Ponds created by river tracing whose influence may touch a world rectangle.
    pub fn ponds_near(&self, min_x: f64, min_z: f64, max_x: f64, max_z: f64) -> Vec<Lake> {
        if !self.params.rivers {
            return Vec::new();
        }
        let (r0x, r1x) = (
            (min_x / REGION_M).floor() as i32 - REGION_RINGS,
            (max_x / REGION_M).floor() as i32 + REGION_RINGS,
        );
        let (r0z, r1z) = (
            (min_z / REGION_M).floor() as i32 - REGION_RINGS,
            (max_z / REGION_M).floor() as i32 + REGION_RINGS,
        );
        let mut out = Vec::new();
        for rz in r0z..=r1z {
            for rx in r0x..=r1x {
                for l in self.region_rivers(rx, rz).ponds.iter() {
                    let m = l.radius * 2.5;
                    if l.cx + m >= min_x
                        && l.cx - m <= max_x
                        && l.cz + m >= min_z
                        && l.cz - m <= max_z
                    {
                        out.push(*l);
                    }
                }
            }
        }
        out
    }

    /// All rivers from regions around a point (for overview maps / tests).
    pub fn rivers_around(&self, x: f64, z: f64) -> Vec<Arc<RegionData>> {
        let (rx, rz) = ((x / REGION_M).floor() as i32, (z / REGION_M).floor() as i32);
        let mut v = vec![];
        for dz in -REGION_RINGS..=REGION_RINGS {
            for dx in -REGION_RINGS..=REGION_RINGS {
                v.push(self.region_rivers(rx + dx, rz + dz));
            }
        }
        v
    }
}

#[inline]
fn ring_dir(k: usize) -> (f64, f64) {
    // 16 evenly spaced unit directions (sin, cos) from exact constants (no libm needed)
    const T: [(f64, f64); 16] = [
        (0.0, 1.0),
        (0.3826834323650898, 0.9238795325112867),
        (
            std::f64::consts::FRAC_1_SQRT_2,
            std::f64::consts::FRAC_1_SQRT_2,
        ),
        (0.9238795325112867, 0.3826834323650898),
        (1.0, 0.0),
        (0.9238795325112867, -0.3826834323650898),
        (
            std::f64::consts::FRAC_1_SQRT_2,
            -std::f64::consts::FRAC_1_SQRT_2,
        ),
        (0.3826834323650898, -0.9238795325112867),
        (0.0, -1.0),
        (-0.3826834323650898, -0.9238795325112867),
        (
            -std::f64::consts::FRAC_1_SQRT_2,
            -std::f64::consts::FRAC_1_SQRT_2,
        ),
        (-0.9238795325112867, -0.3826834323650898),
        (-1.0, 0.0),
        (-0.9238795325112867, 0.3826834323650898),
        (
            -std::f64::consts::FRAC_1_SQRT_2,
            std::f64::consts::FRAC_1_SQRT_2,
        ),
        (-0.3826834323650898, 0.9238795325112867),
    ];
    T[k]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::height::TerrainParams;

    #[test]
    fn rivers_widen_and_follow_the_landscape() {
        let w = TerrainWorld::new(TerrainParams::default());
        let mut found = 0;
        for rz in -3..3 {
            for rx in -3..4 {
                for r in w.region_rivers(rx, rz).rivers.iter() {
                    found += 1;
                    assert!(r.level.iter().all(|l| l.is_finite()));
                    assert!(
                        r.half_w.windows(2).all(|p| p[1] >= p[0] - 1e-9),
                        "rivers widen downstream"
                    );
                    assert!(r.pts.len() >= 20);
                }
            }
        }
        assert!(
            found > 5,
            "expected rivers in a 7x6 region window, found {found}"
        );
    }

    #[test]
    fn sampler_is_pure_and_finds_channels() {
        let w = TerrainWorld::new(TerrainParams::default());
        let r = w
            .rivers_around(0.0, 0.0)
            .into_iter()
            .flat_map(|s| s.rivers.to_vec())
            .next()
            .expect("some river near origin");
        let mid = r.pts.len() / 2;
        let (x, z) = r.pts[mid];
        let s1 = w.river_sampler(x - 500.0, z - 500.0, x + 500.0, z + 500.0);
        let s2 = w.river_sampler(x - 300.0, z - 700.0, x + 900.0, z + 100.0);
        let (a, b) = (s1.carve(x, z, 300.0), s2.carve(x, z, 300.0));
        assert_eq!(
            a.0.to_bits(),
            b.0.to_bits(),
            "result must not depend on which chunk asked"
        );
        assert_eq!(a.2.to_bits(), b.2.to_bits());
        assert!(a.2 > f64::NEG_INFINITY, "river centre must be wet");
    }
}
