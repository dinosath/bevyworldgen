//! Vegetation ecology (Layer A). Placement is a pure function of world position and climate:
//! candidates come from a world-aligned jittered grid, so every chunk (and every LOD) sees the
//! same plants and never double-places or drops one on a border.

mod bake;

pub use bake::{VegMeshData, bake};

use worldgen_core::{ChunkId, Subsystem, derive_seed, unit_f32, unit_f64};
use worldgen_terrain::climate::Climate;
use worldgen_terrain::noise::{fbm, smoothstep};
use worldgen_terrain::{Biome, TerrainChunk};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Species {
    Oak = 0,
    Spruce,
    Pine,
    Birch,
    Shrub,
    Boulder,
}

#[derive(Clone, Copy, Debug)]
pub struct Instance {
    /// Position relative to the chunk origin (x east, z south), y absolute.
    pub pos: [f32; 3],
    pub yaw: f32,
    pub scale: f32,
    pub species: Species,
    /// Per-instance random value in `[0,1)` for colour/shape variation.
    pub variant: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct VegConfig {
    pub seed: u64,
    /// 0 = summer green, 1 = full autumn colours (broadleaf trees and shrubs).
    pub autumn: f32,
    /// Multiplier on tree/shrub density.
    pub density: f32,
    /// Plants are generated for chunks up to this LOD.
    pub max_lod: u8,
}

impl Default for VegConfig {
    fn default() -> Self {
        Self {
            seed: 0x5EED_0001,
            autumn: 0.35,
            density: 1.0,
            max_lod: 2,
        }
    }
}

const TREE_CELL: f64 = 5.5;
const SHRUB_CELL: f64 = 3.6;
const ROCK_CELL: f64 = 11.0;

/// Something that keeps plants away (roads, building lots, water works...).
pub trait Exclusion {
    fn excludes(&self, x: f64, z: f64) -> bool;
}

fn nearest(c: &TerrainChunk, x: f64, z: f64) -> usize {
    let i =
        (((x - c.origin_x) / c.spacing as f64).round() as i64).clamp(0, c.n as i64 - 1) as usize;
    let j =
        (((z - c.origin_z) / c.spacing as f64).round() as i64).clamp(0, c.n as i64 - 1) as usize;
    j * c.n + i
}

/// All plants whose world position lies in this chunk (half-open rectangle).
pub fn place(c: &TerrainChunk, cfg: &VegConfig, excl: &[&dyn Exclusion]) -> Vec<Instance> {
    if c.id.lod > cfg.max_lod {
        return Vec::new();
    }
    let size = c.spacing as f64 * (c.n - 1) as f64;
    let (x0, z0, x1, z1) = (c.origin_x, c.origin_z, c.origin_x + size, c.origin_z + size);
    let mut out = Vec::new();
    for (layer, cell) in [(0u64, TREE_CELL), (1, SHRUB_CELL), (2, ROCK_CELL)] {
        let cell = if layer == 2 {
            cell
        } else {
            cell / (cfg.density as f64).sqrt().clamp(0.5, 2.0)
        };
        let (i0, i1) = ((x0 / cell).floor() as i64, (x1 / cell).ceil() as i64);
        let (j0, j1) = ((z0 / cell).floor() as i64, (z1 / cell).ceil() as i64);
        for j in j0..=j1 {
            for i in i0..=i1 {
                let h = derive_seed(cfg.seed, Subsystem::Vegetation, i, j, layer);
                let x = (i as f64 + unit_f64(h)) * cell;
                let z = (j as f64 + unit_f64(h.rotate_left(21))) * cell;
                if x < x0 || x >= x1 || z < z0 || z >= z1 {
                    continue;
                }
                if let Some(inst) = candidate(c, cfg, layer, h, x, z) {
                    if excl.iter().any(|e| e.excludes(x, z)) {
                        continue;
                    }
                    out.push(inst);
                }
            }
        }
    }
    out
}

fn candidate(
    c: &TerrainChunk,
    cfg: &VegConfig,
    layer: u64,
    h: u64,
    x: f64,
    z: f64,
) -> Option<Instance> {
    let k = nearest(c, x, z);
    if c.water[k] > f32::NEG_INFINITY {
        return None;
    }
    let ny = c.normals[k][1];
    let biome = c.biome_at_vertex(k % c.n, k / c.n);
    let y = c.height_at(x, z);
    let cl: Climate = c.climate.sample(x, z);
    let (t, p) = (cl.temp_c as f64, cl.precip_mm as f64);
    if matches!(biome, Biome::Ocean | Biome::Beach | Biome::Snow) {
        return None;
    }
    // keep a dry margin around water
    let n = c.n;
    let (ci, cj) = (k % n, k / n);
    for (di, dj) in [(-2i64, 0i64), (2, 0), (0, -2), (0, 2)] {
        let (a, b) = (
            (ci as i64 + di).clamp(0, n as i64 - 1) as usize,
            (cj as i64 + dj).clamp(0, n as i64 - 1) as usize,
        );
        if c.water[b * n + a] > f32::NEG_INFINITY && layer != 2 {
            return None;
        }
    }
    let pick = unit_f64(h.rotate_left(42));
    let variant = unit_f32(h.rotate_left(7));
    let yaw = unit_f32(h.rotate_left(33)) * std::f32::consts::TAU;
    let pos = [(x - c.origin_x) as f32, y, (z - c.origin_z) as f32];

    // forest patches: low-frequency noise biased by rainfall
    let fnoise = 0.5 + 0.5 * fbm(cfg.seed ^ 0xF04E, x, z, 800.0, 3, 0.0);
    let wet = smoothstep(420.0, 1100.0, p);
    let forestness = smoothstep(0.55 - 0.30 * wet, 0.70 - 0.30 * wet, fnoise);
    let warm = smoothstep(0.5, 6.0, t);

    match layer {
        0 => {
            if ny < 0.80 || matches!(biome, Biome::Desert | Biome::Alpine | Biome::Tundra) {
                return None;
            }
            let conifer_w = smoothstep(10.0, 3.0, t) + if y > 800.0 { 0.5 } else { 0.0 };
            let broad_w = smoothstep(3.0, 9.0, t) * smoothstep(480.0, 720.0, p);
            let birch_w = 0.18 * warm;
            let dens = forestness * warm * smoothstep(300.0, 560.0, p);
            if (unit_f64(h.rotate_left(14))) > dens * cfg.density.min(1.5) as f64 {
                return None;
            }
            let total = conifer_w + broad_w + birch_w + 1e-6;
            let r = pick * total;
            let species = if r < broad_w {
                Species::Oak
            } else if r < broad_w + birch_w {
                Species::Birch
            } else if t < 7.0 || y > 700.0 {
                Species::Spruce
            } else {
                Species::Pine
            };
            // smaller near the treeline
            let stunt = (0.45 + 0.55 * smoothstep(-1.0, 6.0, t)) as f32;
            let base = match species {
                Species::Oak => 10.0 + 9.0 * variant,
                Species::Birch => 8.0 + 6.0 * variant,
                Species::Spruce => 13.0 + 14.0 * variant,
                _ => 11.0 + 10.0 * variant,
            };
            Some(Instance {
                pos,
                yaw,
                scale: base * stunt,
                species,
                variant,
            })
        }
        1 => {
            if ny < 0.72 || matches!(biome, Biome::Alpine) {
                return None;
            }
            // understory in forests, scrub on dry steppe, dwarf shrubs on tundra
            let scrub = smoothstep(180.0, 320.0, p) * (1.0 - smoothstep(520.0, 760.0, p)) * 0.45;
            let under = forestness * 0.55 * warm;
            let tundra = if matches!(biome, Biome::Tundra) {
                0.35
            } else {
                0.0
            };
            let d = (scrub + under + tundra) * cfg.density as f64;
            if unit_f64(h.rotate_left(14)) > d {
                return None;
            }
            Some(Instance {
                pos,
                yaw,
                scale: 0.9 + 1.6 * variant,
                species: Species::Shrub,
                variant,
            })
        }
        _ => {
            // boulders: more on steep ground and in the mountains, rare elsewhere
            let steep = smoothstep(0.62, 0.92, ny as f64);
            let mountain = smoothstep(500.0, 1300.0, y as f64);
            let d = 0.015
                + 0.12 * (1.0 - steep) * 0.5
                + 0.10 * mountain
                + if matches!(biome, Biome::Alpine) {
                    0.2
                } else {
                    0.0
                };
            if unit_f64(h.rotate_left(14)) > d {
                return None;
            }
            Some(Instance {
                pos,
                yaw,
                scale: 0.6 + 2.6 * variant * variant,
                species: Species::Boulder,
                variant,
            })
        }
    }
}

/// Number of instances by species (diagnostics).
pub fn census(v: &[Instance]) -> [usize; 6] {
    let mut n = [0; 6];
    for i in v {
        n[i.species as usize] += 1;
    }
    n
}

pub fn chunk_label(id: ChunkId) -> String {
    format!("{}:{}:{}", id.lod, id.x, id.z)
}
