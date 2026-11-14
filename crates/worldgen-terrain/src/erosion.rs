//! Order-independent, chunk-border-exact hydraulic erosion (spike S-3).
//!
//! Classic droplet erosion mutates the terrain it reads, so its result depends on droplet order
//! and cannot be reproduced chunk-by-chunk. Here every droplet is evaluated against the
//! *immutable base field* and its height changes are accumulated in **fixed-point integers**
//! (addition is associative). Droplets are seeded by their spawn *world cell*, so any chunk can
//! regenerate exactly the droplets that reach it. A chunk simulates all droplets spawned within
//! `steps` cells of itself on a grid extended by `2 * steps`, hence every droplet that can touch
//! the chunk is simulated to completion with identical inputs in every chunk containing it.

use serde::{Deserialize, Serialize};
use worldgen_core::{CancelToken, Subsystem, derive_seed, unit_f32};

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct ErosionParams {
    pub enabled: bool,
    pub steps: u32,
    pub drops_per_cell: f32,
    pub inertia: f32,
    pub capacity_k: f32,
    pub erode_rate: f32,
    pub deposit_rate: f32,
    pub evaporation: f32,
    pub min_slope: f32,
    pub gravity: f32,
    /// Clamp on the accumulated per-cell change, in metres (also limited to 1.5 x vertex spacing).
    pub max_delta_m: f32,
    /// Largest LOD that is eroded (higher LODs skip erosion).
    pub max_lod: u8,
}

impl Default for ErosionParams {
    fn default() -> Self {
        Self {
            enabled: true,
            steps: 24,
            drops_per_cell: 0.45,
            inertia: 0.30,
            capacity_k: 5.0,
            erode_rate: 0.30,
            deposit_rate: 0.30,
            evaporation: 0.02,
            min_slope: 0.02,
            gravity: 4.0,
            max_delta_m: 4.0,
            max_lod: 2,
        }
    }
}

impl ErosionParams {
    /// Apron (in cells) of base-field samples needed around the chunk interior.
    pub fn apron(&self) -> usize {
        2 * self.steps as usize + 3
    }
}

const FIXED: f32 = 65536.0;

/// Fixed-point deltas (`i64`, 1/65536 m) for a `gw x gw` grid of base heights whose grid cell
/// `(0,0)` has world cell index `(gx0, gz0)`. Droplets spawn in `[spawn_lo, spawn_hi)` (grid cells).
#[allow(clippy::too_many_arguments)]
pub fn erosion_deltas(
    seed: u64,
    p: &ErosionParams,
    grid: &[f32],
    gw: usize,
    gx0: i64,
    gz0: i64,
    spacing: f32,
    spawn_lo: usize,
    spawn_hi: usize,
    cancel: &CancelToken,
) -> Option<Vec<i64>> {
    let mut acc = vec![0i64; gw * gw];
    let whole = p.drops_per_cell.floor() as u32;
    let frac = p.drops_per_cell - whole as f32;
    for cz in spawn_lo..spawn_hi {
        if cz % 8 == 0 && cancel.is_cancelled() {
            return None;
        }
        for cx in spawn_lo..spawn_hi {
            let (wx, wz) = (gx0 + cx as i64, gz0 + cz as i64);
            let count = whole
                + u32::from(unit_f32(derive_seed(seed, Subsystem::Erosion, wx, wz, 0xFFFF)) < frac);
            for k in 0..count {
                let h = derive_seed(seed, Subsystem::Erosion, wx, wz, k as u64);
                let jx = unit_f32(h);
                let jz = unit_f32(h.rotate_left(29));
                droplet(p, grid, gw, spacing, (cx, cz), (jx, jz), h, &mut acc);
            }
        }
    }
    Some(acc)
}

/// Bilinear height + gradient at integer cell `(ix, iz)` with fraction `(fx, fz)`.
#[inline]
fn bil(grid: &[f32], gw: usize, ix: usize, iz: usize, fx: f32, fz: f32) -> (f32, f32, f32) {
    let i = iz * gw + ix;
    let (h00, h10, h01, h11) = (grid[i], grid[i + 1], grid[i + gw], grid[i + gw + 1]);
    let h = h00 * (1.0 - fx) * (1.0 - fz)
        + h10 * fx * (1.0 - fz)
        + h01 * (1.0 - fx) * fz
        + h11 * fx * fz;
    let gx = (h10 - h00) * (1.0 - fz) + (h11 - h01) * fz;
    let gz = (h01 - h00) * (1.0 - fx) + (h11 - h10) * fx;
    (h, gx, gz)
}

#[inline]
fn add(acc: &mut [i64], gw: usize, ix: usize, iz: usize, fx: f32, fz: f32, amount: f32) {
    let i = iz * gw + ix;
    let q = |w: f32| (w * amount * FIXED).round() as i64;
    acc[i] += q((1.0 - fx) * (1.0 - fz));
    acc[i + 1] += q(fx * (1.0 - fz));
    acc[i + gw] += q((1.0 - fx) * fz);
    acc[i + gw + 1] += q(fx * fz);
}

/// Position = integer cell + fraction. All arithmetic on fractions is independent of the grid
/// origin, so the same droplet behaves bit-identically in every chunk that simulates it.
fn droplet(
    p: &ErosionParams,
    grid: &[f32],
    gw: usize,
    spacing: f32,
    cell: (usize, usize),
    frac: (f32, f32),
    hash: u64,
    acc: &mut [i64],
) {
    let lim = gw as i64 - 2; // last valid base cell index (needs ix+1)
    let (mut ix, mut iz) = (cell.0 as i64, cell.1 as i64);
    let (mut fx, mut fz) = frac;
    let (mut dx, mut dz) = (0.0f32, 0.0f32);
    let (mut speed, mut water, mut sediment) = (1.0f32, 1.0f32, 0.0f32);
    for step in 0..p.steps {
        if ix < 0 || iz < 0 || ix >= lim || iz >= lim {
            return;
        }
        let (h, gx, gz) = bil(grid, gw, ix as usize, iz as usize, fx, fz);
        dx = dx * p.inertia - gx * (1.0 - p.inertia);
        dz = dz * p.inertia - gz * (1.0 - p.inertia);
        let len = (dx * dx + dz * dz).sqrt();
        if len < 1e-6 {
            dx = if unit_f32(hash.rotate_left(step + 7)) < 0.5 {
                1.0
            } else {
                -1.0
            };
            dz = if unit_f32(hash.rotate_left(step + 31)) < 0.5 {
                1.0
            } else {
                -1.0
            };
            let l = (dx * dx + dz * dz).sqrt();
            dx /= l;
            dz /= l;
        } else {
            dx /= len;
            dz /= len;
        }
        let (mut nfx, mut nfz, mut nix, mut niz) = (fx + dx, fz + dz, ix, iz);
        while nfx >= 1.0 {
            nfx -= 1.0;
            nix += 1;
        }
        while nfx < 0.0 {
            nfx += 1.0;
            nix -= 1;
        }
        while nfz >= 1.0 {
            nfz -= 1.0;
            niz += 1;
        }
        while nfz < 0.0 {
            nfz += 1.0;
            niz -= 1;
        }
        if nix < 0 || niz < 0 || nix >= lim || niz >= lim {
            return;
        }
        let (hn, _, _) = bil(grid, gw, nix as usize, niz as usize, nfx, nfz);
        let dh = hn - h;
        let capacity = (-dh).max(p.min_slope * spacing) * speed * water * p.capacity_k;
        if sediment > capacity || dh > 0.0 {
            let amount = if dh > 0.0 {
                dh.min(sediment)
            } else {
                (sediment - capacity) * p.deposit_rate
            };
            sediment -= amount;
            add(acc, gw, ix as usize, iz as usize, fx, fz, amount);
        } else {
            let amount = ((capacity - sediment) * p.erode_rate).min(-dh);
            sediment += amount;
            add(acc, gw, ix as usize, iz as usize, fx, fz, -amount);
        }
        speed = (speed * speed - dh / spacing * p.gravity).max(0.0).sqrt();
        water *= 1.0 - p.evaporation;
        (ix, iz, fx, fz) = (nix, niz, nfx, nfz);
    }
    if sediment > 0.0 && ix >= 0 && iz >= 0 && ix < lim && iz < lim {
        add(acc, gw, ix as usize, iz as usize, fx, fz, sediment);
    }
}

/// One pass of a [1 2 1]x[1 2 1]/16 binomial blur on the fixed-point delta grid.
///
/// Integer arithmetic keeps it exactly reproducible. Output is valid wherever the input and all
/// 8 neighbours are valid, so callers must keep one extra cell of valid margin.
pub fn blur_deltas(acc: &[i64], gw: usize) -> Vec<i64> {
    let mut out = acc.to_vec();
    for j in 1..gw - 1 {
        for i in 1..gw - 1 {
            let k = j * gw + i;
            let s = acc[k - gw - 1]
                + 2 * acc[k - gw]
                + acc[k - gw + 1]
                + 2 * acc[k - 1]
                + 4 * acc[k]
                + 2 * acc[k + 1]
                + acc[k + gw - 1]
                + 2 * acc[k + gw]
                + acc[k + gw + 1];
            out[k] = s.div_euclid(16);
        }
    }
    out
}

/// Convert an accumulated fixed-point delta to metres, clamped.
#[inline]
pub fn delta_m(v: i64, max: f32) -> f32 {
    ((v as f32) / FIXED).clamp(-max, max)
}
