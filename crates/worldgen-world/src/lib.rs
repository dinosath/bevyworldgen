//! World composition (Layer A): one call produces everything a chunk needs.

pub mod cache;
pub mod streaming;

use cache::ChunkCache;
use std::sync::{Arc, Mutex};
use worldgen_city::{Building, BuildingMeshData};
use worldgen_core::{CancelToken, ChunkId};
use worldgen_roads::{RoadMeshData, RoadSampler, RoadWorld, TownKind, build_road_mesh};
use worldgen_terrain::mesh::{
    TerrainMeshData, WaterMeshData, build_terrain_mesh, build_water_mesh,
};
use worldgen_terrain::{ChunkConfig, TerrainChunk, TerrainParams, WorldGen};
use worldgen_veg::{Exclusion, VegConfig, VegMeshData};

/// Everything generated for one chunk on a worker thread.
///
/// Mesh buffers are take-once (`take_*`): the renderer moves them into GPU assets so the CPU copy
/// is freed; the terrain grid stays for CPU queries (height, biome, instances).
pub struct GeneratedChunk {
    pub id: ChunkId,
    pub terrain: Arc<TerrainChunk>,
    terrain_mesh: Mutex<Option<TerrainMeshData>>,
    water_mesh: Mutex<Option<WaterMeshData>>,
    veg_mesh: Mutex<Option<VegMeshData>>,
    road_mesh: Mutex<Option<RoadMeshData>>,
    building_mesh: Mutex<Option<BuildingMeshData>>,
    /// Buildings baked into this chunk's mesh.
    pub building_count: usize,
    /// Number of plants baked into the vegetation mesh.
    pub plant_count: usize,
    /// Road index for this chunk (surface heights incl. bridges), if any road is nearby.
    pub roads: Option<Arc<RoadSampler>>,
    /// Worker time spent, milliseconds (diagnostics).
    pub gen_ms: f32,
    bytes: usize,
    /// Bytes still held: shrinks as take-once meshes are handed to the renderer.
    live: std::sync::atomic::AtomicUsize,
    mesh_bytes: [usize; 5],
}

impl GeneratedChunk {
    pub fn take_terrain_mesh(&self) -> Option<TerrainMeshData> {
        let m = self.terrain_mesh.lock().unwrap().take();
        if m.is_some() {
            self.release(0);
        }
        m
    }
    pub fn take_water_mesh(&self) -> Option<WaterMeshData> {
        let m = self.water_mesh.lock().unwrap().take();
        if m.is_some() {
            self.release(1);
        }
        m
    }
    pub fn take_building_mesh(&self) -> Option<BuildingMeshData> {
        let m = self.building_mesh.lock().unwrap().take();
        if m.is_some() {
            self.release(2);
        }
        m
    }
    pub fn take_road_mesh(&self) -> Option<RoadMeshData> {
        let m = self.road_mesh.lock().unwrap().take();
        if m.is_some() {
            self.release(3);
        }
        m
    }
    pub fn take_veg_mesh(&self) -> Option<VegMeshData> {
        let m = self.veg_mesh.lock().unwrap().take();
        if m.is_some() {
            self.release(4);
        }
        m
    }
    fn release(&self, part: usize) {
        self.live
            .fetch_sub(self.mesh_bytes[part], std::sync::atomic::Ordering::Relaxed);
    }

    /// Bytes this chunk still holds on the CPU (terrain grid, road index and any mesh not yet taken).
    pub fn live_bytes(&self) -> usize {
        self.live.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Approximate size of the data generated (used for upload budgeting and memory stats).
    pub fn approx_bytes(&self) -> usize {
        self.bytes
    }
}

/// The world: parameters + generator. Cheap to clone (`Arc`).
#[derive(Clone)]
pub struct World {
    pub gen_world: Arc<WorldGen>,
    cache: Option<Arc<ChunkCache>>,
    pub veg: VegConfig,
    pub roads: Option<Arc<RoadWorld>>,
    towns_built: Arc<
        std::sync::RwLock<
            std::collections::HashMap<(i32, i32), Arc<std::sync::OnceLock<Arc<Vec<Building>>>>>,
        >,
    >,
}

/// Keeps plants off roads, verges and building pads.
struct RoadExclusion(Arc<RoadSampler>);

impl Exclusion for RoadExclusion {
    fn excludes(&self, x: f64, z: f64) -> bool {
        self.0.clearance(x, z) < 5.0 || self.0.near_pad(x, z, 7.0)
    }
}

impl World {
    pub fn new(params: TerrainParams, cfg: ChunkConfig) -> Self {
        let veg = VegConfig {
            seed: params.seed,
            ..Default::default()
        };
        let gen_world = WorldGen::new(params, cfg);
        let roads = RoadWorld::new(gen_world.terrain.clone(), gen_world.terrain.params.seed);
        gen_world.add_modifier(roads.clone());
        Self {
            gen_world,
            cache: None,
            veg,
            roads: Some(roads),
            towns_built: Default::default(),
        }
    }

    /// Enable the on-disk terrain cache under `root`.
    pub fn with_cache(mut self, root: impl Into<std::path::PathBuf>) -> std::io::Result<Self> {
        let fp = cache::world_fingerprint(&self.gen_world.terrain.params, &self.gen_world.cfg);
        self.cache = Some(Arc::new(ChunkCache::new(root, fp)?));
        Ok(self)
    }

    pub fn with_vegetation(mut self, v: VegConfig) -> Self {
        self.veg = v;
        self
    }

    /// Derived buildings of one town (cached; derivation of a whole town costs a few ms).
    fn town_buildings(&self, t: &worldgen_roads::Town) -> Arc<Vec<Building>> {
        let rw = self.roads.as_ref().expect("roads enabled");
        let cell = {
            if let Some(c) = self.towns_built.read().unwrap().get(&t.cell) {
                c.clone()
            } else {
                self.towns_built
                    .write()
                    .unwrap()
                    .entry(t.cell)
                    .or_default()
                    .clone()
            }
        };
        cell.get_or_init(|| {
            let content = rw.towns.content(t);
            let cl = self.gen_world.terrain.climate_node(
                (t.cx / 64.0) as i64,
                (t.cz / 64.0) as i64,
                64.0,
            );
            Arc::new(
                content
                    .lots
                    .iter()
                    .filter_map(|l| worldgen_city::derive(l, t, cl.temp_c, cl.precip_mm))
                    .collect(),
            )
        })
        .clone()
    }

    /// Number of buildings derived for a town (diagnostics/tests).
    pub fn town_building_count(&self, t: &worldgen_roads::Town) -> usize {
        self.town_buildings(t).len()
    }

    /// Buildings whose lot centre lies in the chunk (half-open), for chunks that show buildings.
    fn buildings_in(&self, b: &worldgen_terrain::Bounds, lod: u8) -> Vec<Arc<Vec<Building>>> {
        let Some(rw) = &self.roads else { return vec![] };
        if lod > 3 {
            return vec![];
        }
        let mut out: Vec<Arc<Vec<Building>>> = Vec::new();
        for (k, o) in rw.overlays().iter().enumerate() {
            let lots = o.lots_near(b);
            if lots.is_empty() {
                continue;
            }
            // overlay lots are derived on demand (cached per lot seed set) as a pseudo village
            let stub = worldgen_roads::Town {
                cell: (i32::MIN + k as i32, 0),
                cx: (b.min_x + b.max_x) * 0.5,
                cz: (b.min_z + b.max_z) * 0.5,
                kind: TownKind::Village,
            };
            let cl = self.gen_world.terrain.climate_node(
                (stub.cx / 64.0) as i64,
                (stub.cz / 64.0) as i64,
                64.0,
            );
            let v: Vec<Building> = lots
                .iter()
                .filter_map(|l| worldgen_city::derive(l, &stub, cl.temp_c, cl.precip_mm))
                .collect();
            out.push(Arc::new(v));
        }
        out.extend(
            rw.towns
                .towns_near(b.min_x, b.min_z, b.max_x, b.max_z, 1)
                .into_iter()
                .filter(|t| {
                    let h = t.half_patch() + 40.0;
                    t.cx + h >= b.min_x
                        && t.cx - h <= b.max_x
                        && t.cz + h >= b.min_z
                        && t.cz - h <= b.max_z
                })
                .map(|t| self.town_buildings(&t)),
        );
        out
    }

    pub fn cache(&self) -> Option<&Arc<ChunkCache>> {
        self.cache.as_ref()
    }

    pub fn cfg(&self) -> &ChunkConfig {
        &self.gen_world.cfg
    }

    /// Terrain stage with cache: edited chunks always regenerate.
    fn terrain_stage(&self, id: ChunkId, cancel: &CancelToken) -> Option<Arc<TerrainChunk>> {
        let cfg = &self.gen_world.cfg;
        let cacheable = self.cache.is_some()
            && !self.gen_world.stateful_touch(
                &cfg.bounds(id).expand(cfg.spacing(id.lod) * 4.0),
                cfg.spacing(id.lod),
            );
        if cacheable && let Some(t) = self.cache.as_ref().and_then(|c| c.load(id)) {
            return Some(Arc::new(t));
        }
        let t = self.gen_world.generate_chunk(id, cancel)?;
        if cacheable && let Some(c) = &self.cache {
            c.store(&t);
        }
        Some(Arc::new(t))
    }

    pub fn generate(&self, id: ChunkId, cancel: &CancelToken) -> Option<GeneratedChunk> {
        let t = std::time::Instant::now();
        let terrain = self.terrain_stage(id, cancel)?;
        let terrain_mesh = build_terrain_mesh(&terrain);
        let water_mesh = build_water_mesh(&terrain);
        let cfg = &self.gen_world.cfg;
        let spacing = cfg.spacing(id.lod);
        let bounds = cfg.bounds(id);
        let sampler = self
            .roads
            .as_ref()
            .map(|r| r.sampler(&bounds.expand(40.0), spacing));
        let road_mesh = sampler.as_ref().and_then(|s| {
            let m = build_road_mesh(
                s,
                &bounds,
                (bounds.min_x, bounds.min_z),
                spacing,
                &|x, z| terrain.height_at(x, z),
            );
            (!m.is_empty()).then_some(m)
        });
        let town_sets = self.buildings_in(&bounds, id.lod);
        let chunk_buildings: Vec<&Building> = town_sets
            .iter()
            .flat_map(|v| v.iter())
            .filter(|b| {
                b.cx >= bounds.min_x
                    && b.cx < bounds.max_x
                    && b.cz >= bounds.min_z
                    && b.cz < bounds.max_z
            })
            .collect();
        let building_count = chunk_buildings.len();
        let building_mesh = (!chunk_buildings.is_empty())
            .then(|| worldgen_city::bake(&chunk_buildings, (bounds.min_x, bounds.min_z), id.lod))
            .filter(|m| !m.is_empty());
        let excl = sampler.clone().map(RoadExclusion);
        let excls: Vec<&dyn Exclusion> = excl.iter().map(|e| e as &dyn Exclusion).collect();
        let instances = worldgen_veg::place(&terrain, &self.veg, &excls);
        let plant_count = instances.len();
        let veg_mesh =
            (plant_count > 0).then(|| worldgen_veg::bake(&instances, id.lod, self.veg.autumn));
        let veg_b = veg_mesh.as_ref().map_or(0, |v| {
            v.positions.len() * (12 + 12 + 16) + v.indices.len() * 4
        });
        let road_b = road_mesh.as_ref().map_or(0, |v| {
            v.positions.len() * (12 + 12 + 16) + v.indices.len() * 4
        });
        let bld_b = building_mesh.as_ref().map_or(0, |v| {
            v.positions.len() * (12 + 12 + 16)
                + v.indices.len() * 4
                + (v.lit.positions.len() + v.dark.positions.len()) * 24
        });
        let terr_b =
            terrain_mesh.positions.len() * (12 + 12 + 16 + 8 + 8) + terrain_mesh.indices.len() * 2;
        let water_b = water_mesh
            .as_ref()
            .map_or(0, |w| w.positions.len() * 16 + w.indices.len() * 2);
        let mesh_bytes = [terr_b, water_b, bld_b, road_b, veg_b];
        let sampler_b = sampler.as_ref().map_or(0, |s| s.approx_bytes());
        let retained = terrain.approx_bytes() + sampler_b;
        let bytes = retained + mesh_bytes.iter().sum::<usize>();
        Some(GeneratedChunk {
            id,
            terrain,
            terrain_mesh: Mutex::new(Some(terrain_mesh)),
            water_mesh: Mutex::new(water_mesh),
            veg_mesh: Mutex::new(veg_mesh),
            road_mesh: Mutex::new(road_mesh),
            building_mesh: Mutex::new(building_mesh),
            building_count,
            plant_count,
            roads: sampler.filter(|s| !s.is_empty()),
            gen_ms: t.elapsed().as_secs_f32() * 1000.0,
            bytes,
            live: std::sync::atomic::AtomicUsize::new(bytes),
            mesh_bytes,
        })
    }
}
