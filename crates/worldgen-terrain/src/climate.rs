//! Climate (temperature, precipitation with orographic effects) and biome classification.
//! Climate is evaluated on world-aligned grid nodes and interpolated, so chunks agree at borders.

use crate::height::TerrainWorld;
use crate::noise::{fbm, smoothstep};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Climate {
    pub temp_c: f32,
    pub precip_mm: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Biome {
    Ocean = 0,
    Beach,
    Desert,
    Steppe,
    Grassland,
    TemperateForest,
    BorealForest,
    Tundra,
    Alpine,
    Snow,
}

/// Terrain splat layer indices (8 layers packed in two RGBA groups).
pub mod layer {
    pub const GRASS: usize = 0;
    pub const FOREST_FLOOR: usize = 1;
    pub const STEPPE: usize = 2;
    pub const SAND: usize = 3;
    pub const ROCK: usize = 4;
    pub const SNOW: usize = 5;
    pub const MUD: usize = 6;
    pub const GRAVEL: usize = 7;
}

impl TerrainWorld {
    /// Climate at a world-aligned node `(ix, iz) * step`.
    pub fn climate_node(&self, ix: i64, iz: i64, step: f64) -> Climate {
        let p = &self.params;
        let (x, z) = (ix as f64 * step, iz as f64 * step);
        let sp = step;
        let seed = p.seed;
        let h = self.macro_height(x, z, sp);
        let lat = p.lat0_deg as f64 - z / 111_000.0;
        let t_sea = 27.0 - 0.0078 * lat * lat + 3.5 * fbm(seed ^ 0xC11, x, z, 30_000.0, 2, sp);
        let temp = t_sea - 6.5 * h.max(0.0) / 1000.0;

        // precipitation: wet westerlies (+x) with orographic lift and rain shadow
        let base = 900.0 + 650.0 * fbm(seed ^ 0xC12, x, z, 50_000.0, 2, sp);
        let h1 = self.macro_height(x - 1500.0, z, sp);
        let h2 = self.macro_height(x - 3500.0, z, sp);
        let h3 = self.macro_height(x - 7000.0, z, sp);
        let lift = ((h - h1) / 1500.0).max(0.0) * 1400.0;
        let up_max = h1.max(h2).max(h3);
        let shadow = ((up_max - h - 300.0) / 900.0).clamp(0.0, 1.0);
        let precip = (base + lift) * (1.0 - 0.7 * shadow);
        Climate {
            temp_c: temp as f32,
            precip_mm: precip.max(30.0) as f32,
        }
    }
}

/// A rectangular grid of climate nodes with bilinear sampling.
#[derive(Clone, Debug)]
pub struct ClimateGrid {
    pub ix0: i64,
    pub iz0: i64,
    pub step: f64,
    pub n: usize,
    pub temp: Vec<f32>,
    pub precip: Vec<f32>,
}

impl ClimateGrid {
    pub fn build(
        world: &TerrainWorld,
        min_x: f64,
        min_z: f64,
        max_x: f64,
        max_z: f64,
        spacing: f64,
    ) -> Self {
        let mut step = 64.0;
        while step < spacing * 4.0 {
            step *= 2.0;
        }
        let ix0 = (min_x / step).floor() as i64;
        let iz0 = (min_z / step).floor() as i64;
        let nx = ((max_x / step).floor() as i64 - ix0 + 2) as usize;
        let nz = ((max_z / step).floor() as i64 - iz0 + 2) as usize;
        let n = nx.max(nz);
        let mut temp = Vec::with_capacity(n * n);
        let mut precip = Vec::with_capacity(n * n);
        for j in 0..n {
            for i in 0..n {
                let c = world.climate_node(ix0 + i as i64, iz0 + j as i64, step);
                temp.push(c.temp_c);
                precip.push(c.precip_mm);
            }
        }
        Self {
            ix0,
            iz0,
            step,
            n,
            temp,
            precip,
        }
    }

    pub fn sample(&self, x: f64, z: f64) -> Climate {
        let fx = (x / self.step - self.ix0 as f64).clamp(0.0, (self.n - 1) as f64 - 1e-6);
        let fz = (z / self.step - self.iz0 as f64).clamp(0.0, (self.n - 1) as f64 - 1e-6);
        let (i, j) = (fx as usize, fz as usize);
        let (tx, tz) = ((fx - i as f64) as f32, (fz - j as f64) as f32);
        let at = |v: &Vec<f32>, a: usize, b: usize| v[b * self.n + a];
        let bil = |v: &Vec<f32>| {
            let a = at(v, i, j) * (1.0 - tx) + at(v, i + 1, j) * tx;
            let b = at(v, i, j + 1) * (1.0 - tx) + at(v, i + 1, j + 1) * tx;
            a * (1.0 - tz) + b * tz
        };
        Climate {
            temp_c: bil(&self.temp),
            precip_mm: bil(&self.precip),
        }
    }
}

/// Dominant biome from climate, height above sea level and steepness (`1 - normal.y`).
pub fn classify(c: Climate, h_above_sea: f32, steep: f32) -> Biome {
    let (t, p, h) = (c.temp_c, c.precip_mm, h_above_sea);
    if h < 0.0 {
        Biome::Ocean
    } else if h < 3.5 {
        Biome::Beach
    } else if t < -2.5 {
        Biome::Snow
    } else if steep > 0.42 || (t < 1.0 && h > 900.0) {
        Biome::Alpine
    } else if t < 1.5 {
        Biome::Tundra
    } else if p < 260.0 && t > 6.0 {
        Biome::Desert
    } else if p < 520.0 {
        Biome::Steppe
    } else if t < 6.5 {
        Biome::BorealForest
    } else if p > 760.0 {
        Biome::TemperateForest
    } else {
        Biome::Grassland
    }
}

/// Eight splat weights (sum = 1) for a terrain vertex.
pub fn splat_weights(c: Climate, h_above_sea: f32, ny: f32, wet: f32) -> [f32; 8] {
    let (t, p, h) = (c.temp_c as f64, c.precip_mm as f64, h_above_sea as f64);
    let steep = (1.0 - ny as f64).max(0.0);
    let steep_s = smoothstep(0.16, 0.46, steep);
    let snow = smoothstep(1.0, -3.0, t) * (1.0 - smoothstep(0.30, 0.65, steep));
    let rock = steep_s * (1.0 - 0.85 * snow);
    let sand_coast = 1.0 - smoothstep(1.5, 9.0, h);
    let desert = smoothstep(380.0, 160.0, p) * smoothstep(8.0, 16.0, t);
    let sand = sand_coast.max(desert * 0.9) * (1.0 - steep_s);
    let alpine = smoothstep(900.0, 1500.0, h);
    let gravel =
        alpine * (0.25 + 0.75 * smoothstep(0.07, 0.24, steep)) * (1.0 - snow) * (1.0 - steep_s);
    let forest = smoothstep(450.0, 800.0, p) * smoothstep(1.0, 6.0, t);
    let steppe = smoothstep(620.0, 300.0, p) * (1.0 - desert);
    let mud = (wet as f64).clamp(0.0, 1.0) * 0.9;

    let cover = (1.0 - snow) * (1.0 - rock) * (1.0 - sand) * (1.0 - gravel) * (1.0 - mud);
    let mut w = [0.0f64; 8];
    w[layer::FOREST_FLOOR] = forest * (1.0 - steppe) * cover;
    w[layer::STEPPE] = steppe * (1.0 - forest) * cover;
    w[layer::GRASS] = ((1.0 - forest) * (1.0 - steppe)).max(0.0) * cover + 0.02;
    w[layer::SAND] = sand * (1.0 - snow) * (1.0 - rock);
    w[layer::ROCK] = rock;
    w[layer::SNOW] = snow * (1.0 - rock);
    w[layer::MUD] = mud * (1.0 - snow);
    w[layer::GRAVEL] = gravel;
    let sum: f64 = w.iter().sum();
    let mut out = [0.0f32; 8];
    for i in 0..8 {
        out[i] = (w[i] / sum) as f32;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weights_sum_to_one() {
        for t in [-10.0, 0.0, 10.0, 25.0] {
            for p in [100.0, 500.0, 1500.0] {
                for h in [0.0, 100.0, 1500.0] {
                    for ny in [0.3, 0.8, 1.0] {
                        let w = splat_weights(
                            Climate {
                                temp_c: t,
                                precip_mm: p,
                            },
                            h,
                            ny,
                            0.0,
                        );
                        let s: f32 = w.iter().sum();
                        assert!((s - 1.0).abs() < 1e-4);
                    }
                }
            }
        }
    }

    #[test]
    fn cold_high_steep_is_alpine_or_snow() {
        let b = classify(
            Climate {
                temp_c: -5.0,
                precip_mm: 1200.0,
            },
            1800.0,
            0.1,
        );
        assert_eq!(b, Biome::Snow);
        let b = classify(
            Climate {
                temp_c: 8.0,
                precip_mm: 1200.0,
            },
            300.0,
            0.6,
        );
        assert_eq!(b, Biome::Alpine);
    }
}
