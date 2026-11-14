//! The pure, pointwise terrain function: continents, mountain ranges, rivers, lakes, detail.
//! Every output is a function of world coordinates and the seed only, so adjacent chunks agree
//! bitwise on shared borders and results never depend on request order.

use crate::noise::{fbm, gradient_noise, lerp, ridged, smoothstep};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use worldgen_core::{Subsystem, derive_seed, unit_f64};

/// World-level terrain parameters. Part of the world file; changing any value changes the world.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TerrainParams {
    pub seed: u64,
    pub sea_level: f32,
    pub max_mountain_m: f32,
    pub continent_wavelength_m: f64,
    pub mountain_wavelength_m: f64,
    pub warp_amp_m: f64,
    pub rivers: bool,
    pub lakes: bool,
    /// Latitude (degrees) at world z = 0; z increases southwards.
    pub lat0_deg: f32,
}

impl Default for TerrainParams {
    fn default() -> Self {
        Self {
            seed: 0x5EED_0001,
            sea_level: 0.0,
            max_mountain_m: 1900.0,
            continent_wavelength_m: 60_000.0,
            mountain_wavelength_m: 7_500.0,
            warp_amp_m: 700.0,
            rivers: true,
            lakes: true,
            lat0_deg: 46.0,
        }
    }
}

/// Pointwise terrain sample.
#[derive(Clone, Copy, Debug)]
pub struct TerrainSample {
    pub height: f32,
    /// Water surface elevation, `f32::NEG_INFINITY` where dry.
    pub water: f32,
}

/// A lake: deterministic per feature cell.
#[derive(Clone, Copy, Debug)]
pub struct Lake {
    pub cx: f64,
    pub cz: f64,
    pub radius: f64,
    pub level: f64,
    pub depth: f64,
}

/// Per-chunk sampling context (nearby lakes), built once so hot loops take no locks.
#[derive(Clone, Default)]
pub struct SampleCtx {
    pub lakes: Vec<Lake>,
    pub rivers: Option<Arc<crate::hydro::RiverSampler>>,
}

const LAKE_CELL_M: f64 = 5000.0;

pub(crate) struct MacroS {
    pub h: f64,
    pub land: f64,
    pub mmask: f64,
}

/// Shared, thread-safe world generator state (immutable parameters + lake cache).
pub struct TerrainWorld {
    pub params: TerrainParams,
    lake_cache: RwLock<HashMap<(i32, i32), Option<Lake>>>,
    pub(crate) rivers: crate::hydro::RiverCache,
}

impl TerrainWorld {
    pub fn new(params: TerrainParams) -> Arc<Self> {
        Arc::new(Self {
            params,
            lake_cache: RwLock::new(HashMap::new()),
            rivers: Default::default(),
        })
    }

    fn s(&self, salt: u64) -> u64 {
        self.params
            .seed
            .wrapping_add(salt.wrapping_mul(0x9E37_79B9_7F4A_7C15))
    }

    /// Smooth large-scale terrain (no rivers/lakes/detail).
    pub(crate) fn macro_raw(&self, x: f64, z: f64, spacing: f64) -> MacroS {
        let p = &self.params;
        let wx = x + p.warp_amp_m * fbm(self.s(1), x, z, 9000.0, 2, spacing);
        let wz = z + p.warp_amp_m * fbm(self.s(2), x + 5000.0, z - 3000.0, 9000.0, 2, spacing);
        let c = fbm(self.s(3), wx, wz, p.continent_wavelength_m, 4, spacing);
        let land = smoothstep(-0.10, 0.20, c);
        let cluster = smoothstep(
            -0.05,
            0.45,
            fbm(self.s(4), wx, wz, p.mountain_wavelength_m * 3.5, 2, spacing),
        );
        let mmask = cluster * land;
        let ridge = ridged(self.s(5), wx, wz, p.mountain_wavelength_m, 7, spacing);
        let hills = 0.5 + 0.5 * fbm(self.s(6), wx, wz, 3200.0, 3, spacing);
        let rolling = 0.5 + 0.5 * fbm(self.s(9), wx, wz, 1100.0, 3, spacing);
        let plains = 20.0 + 170.0 * hills + 55.0 * rolling * (1.0 - cluster);
        let ocean = -260.0 + 420.0 * c;
        let base = lerp(ocean, plains, land);
        let h = base + mmask * ridge * p.max_mountain_m as f64;
        MacroS { h, land, mmask }
    }

    /// Smooth macro height only (used by climate and as the river/lake reference).
    pub fn macro_height(&self, x: f64, z: f64, spacing: f64) -> f64 {
        self.macro_raw(x, z, spacing).h
    }

    /// Sampling context (lakes + river index) for a world rectangle.
    pub fn ctx_for(&self, min_x: f64, min_z: f64, max_x: f64, max_z: f64) -> SampleCtx {
        let mut lakes = self.lakes_near(min_x, min_z, max_x, max_z);
        lakes.extend(self.ponds_near(min_x, min_z, max_x, max_z));
        SampleCtx {
            lakes,
            rivers: Some(self.river_sampler(min_x, min_z, max_x, max_z)),
        }
    }

    /// Lakes whose influence may touch the given world rectangle.
    pub fn lakes_near(&self, min_x: f64, min_z: f64, max_x: f64, max_z: f64) -> Vec<Lake> {
        if !self.params.lakes {
            return Vec::new();
        }
        let m = 900.0;
        let (c0x, c1x) = (
            ((min_x - m) / LAKE_CELL_M).floor() as i32,
            ((max_x + m) / LAKE_CELL_M).floor() as i32,
        );
        let (c0z, c1z) = (
            ((min_z - m) / LAKE_CELL_M).floor() as i32,
            ((max_z + m) / LAKE_CELL_M).floor() as i32,
        );
        let mut out = Vec::new();
        for cz in c0z..=c1z {
            for cx in c0x..=c1x {
                if let Some(l) = self.lake_for_cell(cx, cz) {
                    out.push(l);
                }
            }
        }
        out
    }

    fn lake_for_cell(&self, cx: i32, cz: i32) -> Option<Lake> {
        if let Some(v) = self.lake_cache.read().unwrap().get(&(cx, cz)) {
            return *v;
        }
        let v = self.compute_lake(cx, cz);
        self.lake_cache.write().unwrap().insert((cx, cz), v);
        v
    }

    fn compute_lake(&self, cx: i32, cz: i32) -> Option<Lake> {
        let sd = self.params.seed;
        let h = |salt| derive_seed(sd, Subsystem::Hydrology, cx as i64, cz as i64, salt);
        if unit_f64(h(1)) > 0.45 {
            return None;
        }
        let x = (cx as f64 + 0.2 + 0.6 * unit_f64(h(2))) * LAKE_CELL_M;
        let z = (cz as f64 + 0.2 + 0.6 * unit_f64(h(3))) * LAKE_CELL_M;
        let r = 110.0 + 260.0 * unit_f64(h(4));
        let m = self.macro_raw(x, z, 0.0);
        if m.h < 12.0 || m.h > 650.0 || m.land < 0.95 {
            return None;
        }
        let (mut lo, mut hi) = (m.h, m.h);
        for k in 0..8 {
            let (s, c) = octant(k);
            let e = self.macro_raw(x + c * r * 1.5, z + s * r * 1.5, 0.0).h;
            lo = lo.min(e);
            hi = hi.max(e);
        }
        if hi - lo > 22.0 {
            return None;
        }
        Some(Lake {
            cx: x,
            cz: z,
            radius: r,
            level: m.h - 1.2,
            depth: 3.0 + 4.0 * unit_f64(h(5)),
        })
    }

    /// Full pointwise terrain sample at world `(x, z)`; `spacing` (m) drives anti-aliasing.
    pub fn sample(&self, ctx: &SampleCtx, x: f64, z: f64, spacing: f64) -> TerrainSample {
        let p = &self.params;
        let m = self.macro_raw(x, z, spacing);
        let mut h = m.h;
        let mut water = f64::NEG_INFINITY;
        let mut bank = 0.0f64; // 0 = open terrain, 1 = river bank/channel (damps detail)

        if let Some(rs) = ctx.rivers.as_ref().filter(|_| p.rivers && m.land > 0.3) {
            let (nh, b, w) = rs.carve(x, z, h);
            h = nh;
            bank = bank.max(b);
            water = water.max(w);
        }

        for l in &ctx.lakes {
            let dx = x - l.cx;
            let dz = z - l.cz;
            let dist = (dx * dx + dz * dz).sqrt();
            if dist < l.radius * 2.4 {
                let wob = 1.0 + 0.22 * gradient_noise(self.s(21), x / 70.0, z / 70.0);
                let dd = dist / (l.radius * wob);
                let prof = if dd < 1.0 {
                    l.level - l.depth * (1.0 - dd * dd)
                } else {
                    l.level + 0.22 * l.radius * (dd - 1.0)
                };
                let t = smoothstep(1.0, 2.3, dd);
                let nh = lerp(prof, h, t);
                if dd < 2.3 {
                    h = nh;
                    bank = bank.max(1.0 - smoothstep(1.0, 2.0, dd));
                }
                if dd < 1.0 {
                    water = water.max(l.level);
                }
            }
        }

        // detail, damped in channels so it does not poke above water
        let rough = 3.0 + 10.0 * m.mmask;
        let detail = fbm(self.s(7), x, z, 90.0, 4, spacing) * rough
            + fbm(self.s(8), x, z, 420.0, 3, spacing) * (14.0 + 34.0 * m.mmask);
        h += detail * (1.0 - bank);

        let sea = p.sea_level as f64;
        if h < sea {
            water = water.max(sea);
        }
        TerrainSample {
            height: h as f32,
            water: if water > h {
                water as f32
            } else {
                f32::NEG_INFINITY
            },
        }
    }

    /// Height only.
    pub fn height(&self, ctx: &SampleCtx, x: f64, z: f64, spacing: f64) -> f32 {
        self.sample(ctx, x, z, spacing).height
    }
}

#[inline]
fn octant(k: usize) -> (f64, f64) {
    const R: f64 = std::f64::consts::FRAC_1_SQRT_2;
    [
        (0.0, 1.0),
        (R, R),
        (1.0, 0.0),
        (R, -R),
        (0.0, -1.0),
        (-R, -R),
        (-1.0, 0.0),
        (-R, R),
    ][k]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn height_is_pure() {
        let w = TerrainWorld::new(TerrainParams::default());
        let ctx = SampleCtx::default();
        let a = w.height(&ctx, 1234.5, -987.25, 2.0);
        let b = w.height(&ctx, 1234.5, -987.25, 2.0);
        assert_eq!(a.to_bits(), b.to_bits());
    }

    #[test]
    fn terrain_has_relief_and_ocean() {
        let w = TerrainWorld::new(TerrainParams::default());
        let ctx = SampleCtx::default();
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for i in 0..200 {
            for j in 0..200 {
                let h = w.height(
                    &ctx,
                    i as f64 * 400.0 - 40000.0,
                    j as f64 * 400.0 - 40000.0,
                    400.0,
                );
                lo = lo.min(h);
                hi = hi.max(h);
            }
        }
        assert!(lo < -20.0, "no ocean: {lo}");
        assert!(hi > 400.0, "no mountains: {hi}");
    }
}
