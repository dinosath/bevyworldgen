//! Chunk generation: base field -> (erosion deltas) -> modifiers -> normals -> climate/splat/water.

use crate::climate::{Biome, Climate, ClimateGrid, classify, splat_weights};
use crate::erosion::{ErosionParams, blur_deltas, delta_m, erosion_deltas};
use crate::height::{TerrainParams, TerrainWorld};
use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};
use worldgen_core::{CancelToken, ChunkId, Fingerprint};

/// World rectangle (metres).
#[derive(Clone, Copy, Debug)]
pub struct Bounds {
    pub min_x: f64,
    pub min_z: f64,
    pub max_x: f64,
    pub max_z: f64,
}

impl Bounds {
    pub fn intersects(&self, o: &Bounds) -> bool {
        self.min_x <= o.max_x
            && self.max_x >= o.min_x
            && self.min_z <= o.max_z
            && self.max_z >= o.min_z
    }
    pub fn expand(&self, m: f64) -> Bounds {
        Bounds {
            min_x: self.min_x - m,
            min_z: self.min_z - m,
            max_x: self.max_x + m,
            max_z: self.max_z + m,
        }
    }
}

/// A prepared, lock-free height modifier for one region (roads, town pads, user edits).
pub trait RegionModifier: Send {
    fn apply(&self, x: f64, z: f64, h: f32) -> f32;
    /// Surface flags (road, verge, lot) at a point; see `worldgen_roads::mask`.
    fn mask(&self, _x: f64, _z: f64) -> u8 {
        0
    }
    /// True where this modifier fully replaces the procedural terrain (real-world DEM data):
    /// procedural lakes/rivers are suppressed there.
    fn replaces_terrain(&self, _x: f64, _z: f64) -> bool {
        false
    }
}

/// A world-level height modifier. `prepare` is called once per chunk with the region bounds.
pub trait HeightModifier: Send + Sync {
    fn prepare(&self, bounds: &Bounds, spacing: f64) -> Option<Box<dyn RegionModifier>>;
    /// Stable identity mixed into cache keys (e.g. edit revision).
    fn revision(&self) -> u64 {
        0
    }
    /// True if output depends on mutable state (e.g. user edits): such chunks bypass the disk cache.
    fn stateful(&self) -> bool {
        false
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct ChunkConfig {
    /// Quads per chunk edge (vertices = cells + 1). Same at every LOD.
    pub cells: u32,
    /// Lod-0 vertex spacing in metres; lod L uses `cell_m * 2^L`.
    pub cell_m: f32,
    pub max_lod: u8,
    pub erosion: ErosionParams,
}

impl Default for ChunkConfig {
    fn default() -> Self {
        Self {
            cells: 32,
            cell_m: 2.0,
            max_lod: 8,
            erosion: ErosionParams::default(),
        }
    }
}

impl ChunkConfig {
    pub fn spacing(&self, lod: u8) -> f64 {
        self.cell_m as f64 * (1u64 << lod) as f64
    }
    pub fn size_m(&self, lod: u8) -> f64 {
        self.cells as f64 * self.spacing(lod)
    }
    pub fn bounds(&self, id: ChunkId) -> Bounds {
        let s = self.size_m(id.lod);
        Bounds {
            min_x: id.x as f64 * s,
            min_z: id.z as f64 * s,
            max_x: (id.x + 1) as f64 * s,
            max_z: (id.z + 1) as f64 * s,
        }
    }
    pub fn n(&self) -> usize {
        self.cells as usize + 1
    }
}

/// Shared, immutable world description handed to worker jobs.
pub struct WorldGen {
    pub terrain: Arc<TerrainWorld>,
    pub cfg: ChunkConfig,
    modifiers: RwLock<Vec<Arc<dyn HeightModifier>>>,
}

impl WorldGen {
    pub fn new(params: TerrainParams, cfg: ChunkConfig) -> Arc<Self> {
        Arc::new(Self {
            terrain: TerrainWorld::new(params),
            cfg,
            modifiers: RwLock::new(Vec::new()),
        })
    }

    pub fn add_modifier(&self, m: Arc<dyn HeightModifier>) {
        self.modifiers.write().unwrap().push(m);
    }

    pub fn modifier_revision(&self) -> u64 {
        self.modifiers
            .read()
            .unwrap()
            .iter()
            .fold(0u64, |a, m| a.wrapping_mul(31).wrapping_add(m.revision()))
    }

    /// True if a stateful modifier (edits) influences `bounds`.
    pub fn stateful_touch(&self, b: &Bounds, spacing: f64) -> bool {
        self.modifiers
            .read()
            .unwrap()
            .iter()
            .any(|m| m.stateful() && m.prepare(b, spacing).is_some())
    }

    fn prepare_modifiers(&self, b: &Bounds, spacing: f64) -> Vec<Box<dyn RegionModifier>> {
        self.modifiers
            .read()
            .unwrap()
            .iter()
            .filter_map(|m| m.prepare(b, spacing))
            .collect()
    }

    /// Generate one terrain chunk. Returns `None` if cancelled.
    pub fn generate_chunk(&self, id: ChunkId, cancel: &CancelToken) -> Option<TerrainChunk> {
        let cfg = &self.cfg;
        let n = cfg.n();
        let c = cfg.cells as i64;
        let spacing = cfg.spacing(id.lod);
        let eros = cfg.erosion;
        let do_erosion = eros.enabled && id.lod <= eros.max_lod;
        let apron: usize = if do_erosion { eros.apron() } else { 2 };
        let gw = n + 2 * apron;
        // world vertex index of grid cell (0,0)
        let gx0 = id.x as i64 * c - apron as i64;
        let gz0 = id.z as i64 * c - apron as i64;
        let bounds = cfg.bounds(id);
        let wb = bounds.expand(apron as f64 * spacing);
        let ctx = self.terrain.ctx_for(wb.min_x, wb.min_z, wb.max_x, wb.max_z);

        // 1. base field on the (extended) grid
        let mut base = vec![0.0f32; gw * gw];
        let mut water_grid = vec![f32::NEG_INFINITY; gw * gw];
        for gj in 0..gw {
            if gj % 16 == 0 && cancel.is_cancelled() {
                return None;
            }
            let z = (gz0 + gj as i64) as f64 * spacing;
            for gi in 0..gw {
                let x = (gx0 + gi as i64) as f64 * spacing;
                let s = self.terrain.sample(&ctx, x, z, spacing);
                base[gj * gw + gi] = s.height;
                water_grid[gj * gw + gi] = s.water;
            }
        }

        // 2. erosion deltas (fixed-point, order independent)
        let inner = 4usize; // valid margin around the interior (blur needs +1, normals need +1)
        let mut height = base.clone();
        if do_erosion {
            let steps = eros.steps as usize;
            let lo = apron - steps - inner;
            let hi = apron + n + steps + inner;
            let deltas = erosion_deltas(
                self.terrain.params.seed,
                &eros,
                &base,
                gw,
                gx0,
                gz0,
                spacing as f32,
                lo,
                hi,
                cancel,
            )?;
            // droplet deposits are point-like; smooth them (exact integer blur) to avoid spikes
            let deltas = blur_deltas(&deltas, gw);
            let limit = eros.max_delta_m.min(1.5 * spacing as f32);
            let strength = 1.0 / (1u32 << id.lod) as f32; // coarse LODs: gentler (they are far away)
            for gj in (apron - inner)..(apron + n + inner) {
                for gi in (apron - inner)..(apron + n + inner) {
                    let i = gj * gw + gi;
                    height[i] += delta_m(deltas[i], limit) * strength.max(0.25);
                }
            }
        }

        // 3. modifiers (roads, towns, edits) on interior + inner ring
        let mods = self.prepare_modifiers(&wb, spacing);
        if !mods.is_empty() {
            for gj in (apron - inner)..(apron + n + inner) {
                let z = (gz0 + gj as i64) as f64 * spacing;
                for gi in (apron - inner)..(apron + n + inner) {
                    let x = (gx0 + gi as i64) as f64 * spacing;
                    let mut h = height[gj * gw + gi];
                    for m in &mods {
                        h = m.apply(x, z, h);
                    }
                    height[gj * gw + gi] = h;
                }
            }
        }
        if !mods.is_empty() {
            let sea = self.terrain.params.sea_level;
            for gj in (apron - inner)..(apron + n + inner) {
                let z = (gz0 + gj as i64) as f64 * spacing;
                for gi in (apron - inner)..(apron + n + inner) {
                    let x = (gx0 + gi as i64) as f64 * spacing;
                    let k = gj * gw + gi;
                    if mods.iter().any(|m| m.replaces_terrain(x, z)) {
                        // real terrain: no synthetic rivers/lakes; open water is whatever lies below the sea
                        water_grid[k] = if height[k] < sea {
                            sea
                        } else {
                            f32::NEG_INFINITY
                        };
                    }
                }
            }
        }
        if cancel.is_cancelled() {
            return None;
        }

        // 4. crop interior, normals
        let at = |i: usize, j: usize| height[(j + apron) * gw + (i + apron)];
        let hat = |i: i64, j: i64| {
            height[((j + apron as i64) as usize) * gw + (i + apron as i64) as usize]
        };
        let mut heights = Vec::with_capacity(n * n);
        let mut normals = Vec::with_capacity(n * n);
        let mut water = Vec::with_capacity(n * n);
        let (mut min_h, mut max_h) = (f32::MAX, f32::MIN);
        for j in 0..n {
            for i in 0..n {
                let h = at(i, j);
                min_h = min_h.min(h);
                max_h = max_h.max(h);
                heights.push(h);
                let (ii, jj) = (i as i64, j as i64);
                let nx = hat(ii - 1, jj) - hat(ii + 1, jj);
                let nz = hat(ii, jj - 1) - hat(ii, jj + 1);
                let ny = 2.0 * spacing as f32;
                let l = (nx * nx + ny * ny + nz * nz).sqrt();
                normals.push([nx / l, ny / l, nz / l]);
                let w = water_grid[(j + apron) * gw + (i + apron)];
                water.push(if w > h { w } else { f32::NEG_INFINITY });
            }
        }

        // surface flags from modifiers (roads, pads)
        let mut mask = vec![0u8; n * n];
        if !mods.is_empty() {
            for j in 0..n {
                let z = (id.z as i64 * c + j as i64) as f64 * spacing;
                for i in 0..n {
                    let x = (id.x as i64 * c + i as i64) as f64 * spacing;
                    mask[j * n + i] = mods.iter().fold(0u8, |a, m| a | m.mask(x, z));
                }
            }
        }

        // 5. climate, splat, biome
        let climate = ClimateGrid::build(
            &self.terrain,
            bounds.min_x,
            bounds.min_z,
            bounds.max_x,
            bounds.max_z,
            spacing,
        );
        let sea = self.terrain.params.sea_level;
        let mut splat = Vec::with_capacity(n * n);
        let mut biome = Vec::with_capacity(n * n);
        for j in 0..n {
            for i in 0..n {
                let k = j * n + i;
                let x = (id.x as i64 * c + i as i64) as f64 * spacing;
                let z = (id.z as i64 * c + j as i64) as f64 * spacing;
                let cl: Climate = climate.sample(x, z);
                // temperature at the actual (detailed) height, not the macro height
                let cl = Climate {
                    temp_c: cl.temp_c,
                    precip_mm: cl.precip_mm,
                };
                let wet = {
                    // read the apron so the result does not depend on the chunk edge
                    let gw_at = |ii: i64, jj: i64| {
                        let gi = (ii + apron as i64) as usize;
                        let gj = (jj + apron as i64) as usize;
                        water_grid[gj * gw + gi] > height[gj * gw + gi]
                    };
                    let (ii, jj) = (i as i64, j as i64);
                    if gw_at(ii, jj) {
                        1.0
                    } else if gw_at(ii - 1, jj)
                        || gw_at(ii + 1, jj)
                        || gw_at(ii, jj - 1)
                        || gw_at(ii, jj + 1)
                    {
                        0.8
                    } else {
                        0.0
                    }
                };
                let hs = heights[k] - sea;
                let ny = normals[k][1];
                let mut w = splat_weights(cl, hs, ny, wet);
                let mk = mask[k];
                if mk & 3 != 0 || mk & 4 != 0 {
                    // road / verge -> gravel; building pad -> trampled earth
                    let (target, amt) = if mk & 3 != 0 {
                        (crate::climate::layer::GRAVEL, 0.75)
                    } else {
                        (crate::climate::layer::MUD, 0.35)
                    };
                    for v in w.iter_mut() {
                        *v *= 1.0 - amt;
                    }
                    w[target] += amt;
                }
                let mut q = [0u8; 8];
                let mut tot = 0i32;
                for (o, v) in q.iter_mut().zip(w.iter()) {
                    *o = (v * 255.0 + 0.5) as u8;
                    tot += *o as i32;
                }
                // renormalise rounding error into the dominant layer so weights sum to 255
                let dom = (0..8).max_by_key(|&l| q[l]).unwrap();
                q[dom] = (q[dom] as i32 + 255 - tot).clamp(0, 255) as u8;
                splat.push(q);
                biome.push(classify(cl, hs, 1.0 - ny) as u8);
            }
        }

        Some(TerrainChunk {
            id,
            origin_x: bounds.min_x,
            origin_z: bounds.min_z,
            spacing: spacing as f32,
            n,
            heights,
            normals,
            splat,
            water,
            biome,
            mask,
            climate,
            min_h,
            max_h,
        })
    }
}

#[derive(Clone, Debug)]
pub struct TerrainChunk {
    pub id: ChunkId,
    pub origin_x: f64,
    pub origin_z: f64,
    pub spacing: f32,
    /// Vertices per side.
    pub n: usize,
    pub heights: Vec<f32>,
    pub normals: Vec<[f32; 3]>,
    pub splat: Vec<[u8; 8]>,
    /// Water surface per vertex (`NEG_INFINITY` = dry).
    pub water: Vec<f32>,
    pub biome: Vec<u8>,
    /// Surface flags per vertex from modifiers (road / verge / building pad).
    pub mask: Vec<u8>,
    pub climate: ClimateGrid,
    pub min_h: f32,
    pub max_h: f32,
}

impl TerrainChunk {
    pub fn height_at_vertex(&self, i: usize, j: usize) -> f32 {
        self.heights[j * self.n + i]
    }

    /// Bilinear height at world coordinates inside this chunk (clamped).
    pub fn height_at(&self, x: f64, z: f64) -> f32 {
        let fx = ((x - self.origin_x) / self.spacing as f64).clamp(0.0, (self.n - 1) as f64 - 1e-9);
        let fz = ((z - self.origin_z) / self.spacing as f64).clamp(0.0, (self.n - 1) as f64 - 1e-9);
        let (i, j) = (fx as usize, fz as usize);
        let (tx, tz) = ((fx - i as f64) as f32, (fz - j as f64) as f32);
        let h = |a: usize, b: usize| self.heights[b * self.n + a];
        let a = h(i, j) * (1.0 - tx) + h(i + 1, j) * tx;
        let b = h(i, j + 1) * (1.0 - tx) + h(i + 1, j + 1) * tx;
        a * (1.0 - tz) + b * tz
    }

    pub fn biome_at_vertex(&self, i: usize, j: usize) -> Biome {
        // SAFETY-free conversion via match to keep this crate `unsafe`-free
        match self.biome[j * self.n + i] {
            0 => Biome::Ocean,
            1 => Biome::Beach,
            2 => Biome::Desert,
            3 => Biome::Steppe,
            4 => Biome::Grassland,
            5 => Biome::TemperateForest,
            6 => Biome::BorealForest,
            7 => Biome::Tundra,
            8 => Biome::Alpine,
            _ => Biome::Snow,
        }
    }

    /// Bitwise fingerprint of everything generated.
    pub fn fingerprint(&self) -> u64 {
        let mut f = Fingerprint::new();
        f.f32s(&self.heights);
        for n in &self.normals {
            f.f32s(n);
        }
        for s in &self.splat {
            f.bytes(s);
        }
        f.f32s(&self.water);
        f.bytes(&self.biome);
        f.bytes(&self.mask);
        f.finish()
    }

    pub fn approx_bytes(&self) -> usize {
        self.n * self.n * (4 + 12 + 8 + 4 + 2)
    }
}

// ---------------------------------------------------------------------------------------------
// Lossless binary (de)serialisation for the disk cache.
// ---------------------------------------------------------------------------------------------

/// Bump when any terrain algorithm changes output (also invalidates disk caches).
pub const TERRAIN_GEN_VERSION: u32 = 2;
const CHUNK_MAGIC: &[u8; 8] = b"WGTC0001";

struct W(Vec<u8>);
impl W {
    fn u8(&mut self, v: u8) {
        self.0.push(v);
    }
    fn u32(&mut self, v: u32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn i32(&mut self, v: i32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn i64(&mut self, v: i64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f32(&mut self, v: f32) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
    fn f64(&mut self, v: f64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }
}

struct R<'a>(&'a [u8], usize);
impl R<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let s = self.0.get(self.1..self.1 + n)?;
        self.1 += n;
        Some(s)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn i32(&mut self) -> Option<i32> {
        Some(i32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn i64(&mut self) -> Option<i64> {
        Some(i64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
    fn f32(&mut self) -> Option<f32> {
        Some(f32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn f64(&mut self) -> Option<f64> {
        Some(f64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
}

impl TerrainChunk {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut w = W(Vec::with_capacity(self.approx_bytes() + 256));
        w.0.extend_from_slice(CHUNK_MAGIC);
        w.u32(TERRAIN_GEN_VERSION);
        w.u8(self.id.lod);
        w.i32(self.id.x);
        w.i32(self.id.z);
        w.f64(self.origin_x);
        w.f64(self.origin_z);
        w.f32(self.spacing);
        w.u32(self.n as u32);
        w.f32(self.min_h);
        w.f32(self.max_h);
        for v in &self.heights {
            w.f32(*v);
        }
        for n in &self.normals {
            for c in n {
                w.f32(*c);
            }
        }
        for s in &self.splat {
            w.0.extend_from_slice(s);
        }
        for v in &self.water {
            w.f32(*v);
        }
        w.0.extend_from_slice(&self.biome);
        w.0.extend_from_slice(&self.mask);
        let c = &self.climate;
        w.i64(c.ix0);
        w.i64(c.iz0);
        w.f64(c.step);
        w.u32(c.n as u32);
        for v in c.temp.iter().chain(c.precip.iter()) {
            w.f32(*v);
        }
        w.0
    }

    pub fn from_bytes(b: &[u8]) -> Option<TerrainChunk> {
        let mut r = R(b, 0);
        if r.take(8)? != CHUNK_MAGIC || r.u32()? != TERRAIN_GEN_VERSION {
            return None;
        }
        let id = ChunkId::new(r.u8()?, r.i32()?, r.i32()?);
        let (origin_x, origin_z, spacing) = (r.f64()?, r.f64()?, r.f32()?);
        let n = r.u32()? as usize;
        if n == 0 || n > 4096 {
            return None;
        }
        let (min_h, max_h) = (r.f32()?, r.f32()?);
        let nn = n * n;
        let heights = (0..nn).map(|_| r.f32()).collect::<Option<Vec<_>>>()?;
        let mut normals = Vec::with_capacity(nn);
        for _ in 0..nn {
            normals.push([r.f32()?, r.f32()?, r.f32()?]);
        }
        let mut splat = Vec::with_capacity(nn);
        for _ in 0..nn {
            splat.push(<[u8; 8]>::try_from(r.take(8)?).ok()?);
        }
        let water = (0..nn).map(|_| r.f32()).collect::<Option<Vec<_>>>()?;
        let biome = r.take(nn)?.to_vec();
        let mask = r.take(nn)?.to_vec();
        let (ix0, iz0, step, cn) = (r.i64()?, r.i64()?, r.f64()?, r.u32()? as usize);
        if cn == 0 || cn > 4096 {
            return None;
        }
        let temp = (0..cn * cn).map(|_| r.f32()).collect::<Option<Vec<_>>>()?;
        let precip = (0..cn * cn).map(|_| r.f32()).collect::<Option<Vec<_>>>()?;
        if r.1 != b.len() {
            return None;
        }
        Some(TerrainChunk {
            id,
            origin_x,
            origin_z,
            spacing,
            n,
            heights,
            normals,
            splat,
            water,
            biome,
            mask,
            climate: ClimateGrid {
                ix0,
                iz0,
                step,
                n: cn,
                temp,
                precip,
            },
            min_h,
            max_h,
        })
    }
}
