//! Terrain and water materials (`ExtendedMaterial` over `StandardMaterial`: public API only).

use bevy::{
    pbr::{ExtendedMaterial, MaterialExtension},
    prelude::*,
    render::render_resource::{AsBindGroup, ShaderType},
    shader::ShaderRef,
};

#[derive(Clone, Debug, ShaderType)]
pub struct TerrainUniforms {
    pub near_tile_m: f32,
    pub far_tile_m: f32,
    pub wetness: f32,
    pub bump: f32,
    pub origin: Vec2,
    pub _pad: Vec2,
}

impl Default for TerrainUniforms {
    fn default() -> Self {
        Self {
            near_tile_m: 8.0,
            far_tile_m: 128.0,
            wetness: 0.0,
            bump: 0.9,
            origin: Vec2::ZERO,
            _pad: Vec2::ZERO,
        }
    }
}

#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct TerrainExtension {
    #[texture(100, dimension = "2d_array")]
    #[sampler(101)]
    pub albedo: Handle<Image>,
    #[uniform(102)]
    pub uniforms: TerrainUniforms,
}

impl MaterialExtension for TerrainExtension {
    fn fragment_shader() -> ShaderRef {
        "embedded://bevy_worldgen_render/shaders/terrain.wesl".into()
    }
}

pub type TerrainMaterial = ExtendedMaterial<StandardMaterial, TerrainExtension>;

#[derive(Clone, Debug, ShaderType)]
pub struct WaterUniforms {
    pub shallow: Vec4,
    pub deep: Vec4,
    pub origin: Vec2,
    pub wave_strength: f32,
    pub foam_depth: f32,
}

impl Default for WaterUniforms {
    fn default() -> Self {
        Self {
            shallow: Vec4::new(0.10, 0.42, 0.45, 1.0),
            deep: Vec4::new(0.01, 0.06, 0.13, 1.0),
            origin: Vec2::ZERO,
            wave_strength: 0.35,
            foam_depth: 0.45,
        }
    }
}

#[derive(Asset, TypePath, AsBindGroup, Clone, Default)]
pub struct WaterExtension {
    #[uniform(100)]
    pub uniforms: WaterUniforms,
}

impl MaterialExtension for WaterExtension {
    fn fragment_shader() -> ShaderRef {
        "embedded://bevy_worldgen_render/shaders/water.wesl".into()
    }
}

pub type WaterMaterial = ExtendedMaterial<StandardMaterial, WaterExtension>;
