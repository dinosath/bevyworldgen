//! Grass via `bevy_grass` (GPU-built blades): one field per nearest (lod-0) chunk, with ground
//! height and density baked once from that chunk's terrain data.

use bevy::prelude::*;
use bevy_grass::prelude::*;
use bevy_worldgen::{ChunkData, ChunkRoot};
use std::sync::Arc;
use worldgen_terrain::{Biome, TerrainChunk};

/// Grass quality / reach; the presets map to `bevy_grass` quality tiers.
#[derive(Resource, Clone, Copy, Debug)]
pub struct GrassConfig {
    pub enabled: bool,
}

impl Default for GrassConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}

fn splat_at(c: &TerrainChunk, x: f64, z: f64) -> ([u8; 8], usize) {
    let i =
        (((x - c.origin_x) / c.spacing as f64).round() as i64).clamp(0, c.n as i64 - 1) as usize;
    let j =
        (((z - c.origin_z) / c.spacing as f64).round() as i64).clamp(0, c.n as i64 - 1) as usize;
    (c.splat[j * c.n + i], j * c.n + i)
}

fn density(c: &TerrainChunk, x: f64, z: f64) -> f32 {
    let (w, k) = splat_at(c, x, z);
    if c.water[k] > f32::NEG_INFINITY || c.normals[k][1] < 0.74 || c.mask[k] != 0 {
        return 0.0;
    }
    if matches!(
        c.biome_at_vertex(k % c.n, k / c.n),
        Biome::Desert | Biome::Alpine | Biome::Snow | Biome::Ocean | Biome::Beach
    ) {
        return 0.0;
    }
    use worldgen_terrain::layer::*;
    let f = |l: usize| w[l] as f32 / 255.0;
    (f(GRASS) + 0.8 * f(STEPPE) + 0.45 * f(FOREST_FLOOR)).clamp(0.0, 1.0)
}

/// Chunks waiting for a grass field. Field creation evaluates ~32k closure calls on the main
/// thread, so it is rate limited to `FIELDS_PER_FRAME` to protect frame time.
#[derive(Default)]
pub(crate) struct GrassQueue(std::collections::VecDeque<(Entity, Arc<TerrainChunk>)>);

const FIELDS_PER_FRAME: usize = 1;

pub(crate) fn build_chunk_grass(
    mut commands: Commands,
    cfg: Option<Res<GrassConfig>>,
    mut styles: ResMut<Assets<GrassStyle>>,
    mut images: ResMut<Assets<Image>>,
    roots: Query<(Entity, &ChunkData), (Added<ChunkData>, With<ChunkRoot>)>,
    alive: Query<(), With<ChunkRoot>>,
    mut queue: Local<GrassQueue>,
    mut stats: ResMut<crate::RenderStats>,
) {
    if !cfg.is_none_or(|c| c.enabled) {
        return;
    }
    for (root, data) in &roots {
        if data.0.terrain.id.lod == 0 {
            queue.0.push_back((root, data.0.terrain.clone()));
        }
    }
    if queue.0.is_empty() {
        stats.grass_ms_last = 0.0;
        return;
    }
    let t0 = std::time::Instant::now();
    let mut made = 0;
    while made < FIELDS_PER_FRAME {
        let Some((root, t)) = queue.0.pop_front() else {
            break;
        };
        if alive.get(root).is_err() {
            continue; // chunk was evicted/replaced while queued
        }
        let size = t.spacing * (t.n - 1) as f32;
        let (cx, cz) = (
            t.origin_x + size as f64 * 0.5,
            t.origin_z + size as f64 * 0.5,
        );
        // skip chunks that cannot grow grass at all (cheap early-out)
        if !(0..t.n * t.n).any(|k| t.water[k] == f32::NEG_INFINITY && t.splat[k][0] > 40) {
            continue;
        }
        let cl = t.climate.sample(cx, cz);
        let style = if cl.precip_mm < 520.0 {
            GrassStyle::DRY_SAVANNA
        } else if cl.precip_mm > 950.0 && cl.temp_c < 9.0 {
            GrassStyle::AUTUMN_WILD
        } else if ((t.id.x * 31 + t.id.z * 17) & 3) == 0 {
            GrassStyle::WILDFLOWER_MEADOW
        } else {
            GrassStyle::MEADOW
        };
        let (t_h, t_d) = (t.clone(), t.clone());
        let field = GrassField::new(Vec2::splat(size), style, &mut styles)
            .with_ground_from_fn(&mut images, 0.5, move |p| {
                t_h.height_at(cx + p.x as f64, cz + p.y as f64)
            })
            .with_density_from_fn(&mut images, 0.5, move |p| {
                density(&t_d, cx + p.x as f64, cz + p.y as f64)
            });
        commands.spawn((
            field,
            Transform::from_xyz(size * 0.5, 0.0, size * 0.5),
            ChildOf(root),
        ));
        made += 1;
    }
    let ms = t0.elapsed().as_secs_f32() * 1000.0;
    stats.grass_ms_last = ms;
    stats.grass_ms_max = stats.grass_ms_max.max(ms);
}
