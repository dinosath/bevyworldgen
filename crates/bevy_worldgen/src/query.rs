use crate::WorldRes;
use crate::stream::ChunkStore;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;

/// CPU height queries against resident chunks (finest available LOD wins).
#[derive(SystemParam)]
pub struct TerrainQuery<'w> {
    store: Res<'w, ChunkStore>,
    world: Res<'w, WorldRes>,
}

impl TerrainQuery<'_> {
    /// Terrain height at world `(x, z)` or `None` if no chunk covering it is loaded.
    pub fn height_at(&self, x: f64, z: f64) -> Option<f32> {
        let cfg = self.world.world.cfg();
        for lod in 0..=cfg.max_lod {
            let size = cfg.size_m(lod);
            let id = worldgen_core::ChunkId::new(
                lod,
                (x / size).floor() as i32,
                (z / size).floor() as i32,
            );
            if let Some(d) = self.store.data(id) {
                return Some(d.terrain.height_at(x, z));
            }
        }
        None
    }

    /// Height from chunks of LOD `<= max_lod` only (use to wait for detailed terrain).
    pub fn height_at_fine(&self, x: f64, z: f64, max_lod: u8) -> Option<f32> {
        let cfg = self.world.world.cfg();
        for lod in 0..=max_lod.min(cfg.max_lod) {
            let size = cfg.size_m(lod);
            let id = worldgen_core::ChunkId::new(
                lod,
                (x / size).floor() as i32,
                (z / size).floor() as i32,
            );
            if let Some(d) = self.store.data(id) {
                return Some(d.terrain.height_at(x, z));
            }
        }
        None
    }

    /// Drivable surface at `(x, z)`: the road deck if the caller is on a road (within 2 m of
    /// `ref_y`; this keeps cars on bridges and lets them pass underneath), else the terrain.
    /// Returns `(height, on_road)`.
    pub fn surface_at(&self, x: f64, z: f64, ref_y: f32) -> Option<(f32, bool)> {
        let cfg = self.world.world.cfg();
        for lod in 0..=cfg.max_lod {
            let size = cfg.size_m(lod);
            let id = worldgen_core::ChunkId::new(
                lod,
                (x / size).floor() as i32,
                (z / size).floor() as i32,
            );
            if let Some(d) = self.store.data(id) {
                let ground = d.terrain.height_at(x, z);
                if let Some(y) = d.roads.as_ref().and_then(|r| r.surface_y(x, z))
                    && ((y - ref_y).abs() < 2.0 || (y - ground).abs() < 0.5)
                {
                    return Some((y + 0.03, true));
                }
                return Some((ground, false));
            }
        }
        None
    }

    /// Surface normal from central differences of [`Self::height_at`].
    pub fn normal_at(&self, x: f64, z: f64) -> Option<Vec3> {
        let e = 1.5;
        let (l, r) = (self.height_at(x - e, z)?, self.height_at(x + e, z)?);
        let (d, u) = (self.height_at(x, z - e)?, self.height_at(x, z + e)?);
        Some(Vec3::new(l - r, 2.0 * e as f32, d - u).normalize())
    }
}
