//! Layer C: rendering for `bevy_worldgen` chunks. Uses public Bevy APIs only (`ExtendedMaterial`,
//! embedded WESL shaders, standard PBR, built-in atmosphere is configured by the application).

mod chunk_visuals;
mod grass;
mod materials;
mod quality;
mod sky;
mod textures;
mod weather;

pub use chunk_visuals::{ChunkMeshes, WorldMaterials};
pub use grass::GrassConfig;
pub use quality::Quality;
pub use sky::{Moon, SkyState, TimeOfDay};
pub use weather::{CloudMaterial, Weather, WeatherState};

/// Main-thread cost of building chunk visuals (milliseconds), for budgets and benchmarks.
#[derive(Resource, Default, Debug, Clone)]
pub struct RenderStats {
    pub visuals_ms_last: f32,
    pub visuals_ms_max: f32,
    pub grass_ms_last: f32,
    pub grass_ms_max: f32,
}
pub use materials::{
    TerrainExtension, TerrainMaterial, TerrainUniforms, WaterExtension, WaterMaterial,
    WaterUniforms,
};

use bevy::asset::embedded_asset;
use bevy::prelude::*;

pub struct WorldRenderPlugin;

impl Plugin for WorldRenderPlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "shaders/terrain.wesl");
        embedded_asset!(app, "shaders/water.wesl");
        embedded_asset!(app, "shaders/clouds.wesl");
        app.add_plugins((
            MaterialPlugin::<TerrainMaterial>::default(),
            MaterialPlugin::<WaterMaterial>::default(),
        ))
        .add_plugins(bevy_grass::prelude::GrassPlugins)
        .init_resource::<GrassConfig>()
        .init_resource::<RenderStats>()
        .init_resource::<TimeOfDay>()
        .init_resource::<SkyState>()
        .add_plugins(weather::WeatherPlugin)
        .add_systems(Update, (quality::apply_quality, sky::update_sky))
        .add_systems(Update, grass::build_chunk_grass)
        .add_systems(Startup, chunk_visuals::setup_materials)
        .add_systems(
            Update,
            (
                chunk_visuals::build_chunk_visuals,
                chunk_visuals::sync_origin_uniforms,
            ),
        );
    }
}
