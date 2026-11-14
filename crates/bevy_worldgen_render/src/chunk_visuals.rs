//! Turns `ChunkData` into mesh entities (children of the chunk root).

use crate::materials::*;
use crate::textures::build_terrain_array;
use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy_worldgen::{ChunkData, ChunkRoot, WorldOrigin};
use worldgen_terrain::mesh::{TerrainMeshData, WaterMeshData};

#[derive(Resource)]
pub struct WorldMaterials {
    pub terrain: Handle<TerrainMaterial>,
    pub water: Handle<WaterMaterial>,
    pub vegetation: Handle<StandardMaterial>,
    pub roads: Handle<StandardMaterial>,
    pub buildings: Handle<StandardMaterial>,
    pub glass_dark: Handle<StandardMaterial>,
    pub glass_lit: Handle<StandardMaterial>,
}

/// Child mesh entities of a chunk root (so other systems can find/replace them).
#[derive(Component, Default)]
pub struct ChunkMeshes {
    pub terrain: Option<Entity>,
    pub water: Option<Entity>,
    pub vegetation: Option<Entity>,
    pub roads: Option<Entity>,
    pub buildings: Option<Entity>,
    pub windows: Vec<Entity>,
}

pub(crate) fn setup_materials(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut terrain: ResMut<Assets<TerrainMaterial>>,
    mut water: ResMut<Assets<WaterMaterial>>,
    mut std_mats: ResMut<Assets<StandardMaterial>>,
) {
    let albedo = images.add(build_terrain_array());
    let t = terrain.add(TerrainMaterial {
        base: StandardMaterial {
            base_color: Color::WHITE,
            perceptual_roughness: 0.92,
            reflectance: 0.25,
            ..default()
        },
        extension: TerrainExtension {
            albedo,
            uniforms: TerrainUniforms::default(),
        },
    });
    let w = water.add(WaterMaterial {
        base: StandardMaterial {
            base_color: Color::srgba(0.1, 0.3, 0.4, 0.7),
            perceptual_roughness: 0.06,
            reflectance: 0.5,
            alpha_mode: AlphaMode::Blend,
            cull_mode: None,
            ..default()
        },
        extension: WaterExtension::default(),
    });
    let v = std_mats.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.88,
        reflectance: 0.12,
        ..default()
    });
    let rd = std_mats.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.82,
        reflectance: 0.3,
        ..default()
    });
    let bd = std_mats.add(StandardMaterial {
        base_color: Color::WHITE,
        perceptual_roughness: 0.85,
        reflectance: 0.2,
        ..default()
    });
    let gd = std_mats.add(StandardMaterial {
        base_color: Color::srgb(0.04, 0.06, 0.09),
        perceptual_roughness: 0.06,
        reflectance: 0.7,
        metallic: 0.0,
        ..default()
    });
    let gl = std_mats.add(StandardMaterial {
        base_color: Color::srgb(0.04, 0.06, 0.09),
        perceptual_roughness: 0.06,
        reflectance: 0.7,
        emissive: LinearRgba::BLACK,
        ..default()
    });
    commands.insert_resource(WorldMaterials {
        terrain: t,
        water: w,
        vegetation: v,
        roads: rd,
        buildings: bd,
        glass_dark: gd,
        glass_lit: gl,
    });
}

/// Keep the shaders' absolute-world offset in sync with the floating origin.
pub(crate) fn sync_origin_uniforms(
    origin: Res<WorldOrigin>,
    mats: Option<Res<WorldMaterials>>,
    mut terrain: ResMut<Assets<TerrainMaterial>>,
    mut water: ResMut<Assets<WaterMaterial>>,
) {
    let Some(m) = mats else { return };
    if !origin.is_changed() {
        return;
    }
    let o = Vec2::new(origin.0.x as f32, origin.0.z as f32);
    if let Some(mut mat) = terrain.get_mut(&m.terrain) {
        mat.extension.uniforms.origin = o;
    }
    if let Some(mut mat) = water.get_mut(&m.water) {
        mat.extension.uniforms.origin = o;
    }
}

fn terrain_mesh(d: TerrainMeshData) -> Mesh {
    let mut m = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, d.positions);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, d.normals);
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, d.splat_a);
    m.insert_attribute(Mesh::ATTRIBUTE_UV_0, d.splat_b);
    m.insert_attribute(Mesh::ATTRIBUTE_UV_1, d.splat_c);
    m.insert_indices(Indices::U16(d.indices));
    m
}

fn glass_mesh(g: worldgen_city::GlassPart) -> Mesh {
    let mut m = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, g.positions);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, g.normals);
    m.insert_indices(Indices::U32(g.indices));
    m
}

fn building_mesh(d: worldgen_city::BuildingMeshData) -> Mesh {
    let mut m = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, d.positions);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, d.normals);
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, d.colors);
    m.insert_indices(Indices::U32(d.indices));
    m
}

fn road_mesh(d: worldgen_roads::RoadMeshData) -> Mesh {
    let mut m = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, d.positions);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, d.normals);
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, d.colors);
    m.insert_indices(Indices::U32(d.indices));
    m
}

fn veg_mesh(d: worldgen_veg::VegMeshData) -> Mesh {
    let mut m = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, d.positions);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, d.normals);
    m.insert_attribute(Mesh::ATTRIBUTE_COLOR, d.colors);
    m.insert_indices(Indices::U32(d.indices));
    m
}

fn water_mesh(d: WaterMeshData) -> Mesh {
    let n = d.positions.len();
    let mut m = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    );
    m.insert_attribute(Mesh::ATTRIBUTE_POSITION, d.positions);
    m.insert_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0f32, 1.0, 0.0]; n]);
    m.insert_attribute(
        Mesh::ATTRIBUTE_UV_0,
        d.depth.iter().map(|x| [*x, 0.0]).collect::<Vec<[f32; 2]>>(),
    );
    m.insert_indices(Indices::U16(d.indices));
    m
}

pub(crate) fn build_chunk_visuals(
    mut commands: Commands,
    mats: Option<Res<WorldMaterials>>,
    mut meshes: ResMut<Assets<Mesh>>,
    roots: Query<(Entity, &ChunkData), (Added<ChunkData>, With<ChunkRoot>)>,
    mut stats: ResMut<crate::RenderStats>,
) {
    let Some(mats) = mats else { return };
    if roots.is_empty() {
        stats.visuals_ms_last = 0.0;
        return;
    }
    let t0 = std::time::Instant::now();
    for (root, data) in &roots {
        let mut cm = ChunkMeshes::default();
        if let Some(tm) = data.0.take_terrain_mesh() {
            let e = commands
                .spawn((
                    Mesh3d(meshes.add(terrain_mesh(tm))),
                    MeshMaterial3d(mats.terrain.clone()),
                    Transform::default(),
                    ChildOf(root),
                ))
                .id();
            cm.terrain = Some(e);
        }
        if let Some(wm) = data.0.take_water_mesh() {
            let e = commands
                .spawn((
                    Mesh3d(meshes.add(water_mesh(wm))),
                    MeshMaterial3d(mats.water.clone()),
                    Transform::default(),
                    NotShadowCaster,
                    ChildOf(root),
                ))
                .id();
            cm.water = Some(e);
        }
        if let Some(mut bm) = data.0.take_building_mesh() {
            let lit = std::mem::take(&mut bm.lit);
            let dark = std::mem::take(&mut bm.dark);
            if !bm.indices.is_empty() {
                let mut e = commands.spawn((
                    Mesh3d(meshes.add(building_mesh(bm))),
                    MeshMaterial3d(mats.buildings.clone()),
                    Transform::default(),
                    ChildOf(root),
                ));
                if data.0.id.lod > 0 {
                    e.insert(NotShadowCaster);
                }
                cm.buildings = Some(e.id());
            }
            for (part, mat) in [(lit, &mats.glass_lit), (dark, &mats.glass_dark)] {
                if !part.is_empty() {
                    let id = commands
                        .spawn((
                            Mesh3d(meshes.add(glass_mesh(part))),
                            MeshMaterial3d(mat.clone()),
                            Transform::default(),
                            NotShadowCaster,
                            ChildOf(root),
                        ))
                        .id();
                    cm.windows.push(id);
                }
            }
        }
        if let Some(rm) = data.0.take_road_mesh() {
            let e = commands
                .spawn((
                    Mesh3d(meshes.add(road_mesh(rm))),
                    MeshMaterial3d(mats.roads.clone()),
                    Transform::default(),
                    NotShadowCaster,
                    ChildOf(root),
                ))
                .id();
            cm.roads = Some(e);
        }
        if let Some(vm) = data.0.take_veg_mesh() {
            let mut e = commands.spawn((
                Mesh3d(meshes.add(veg_mesh(vm))),
                MeshMaterial3d(mats.vegetation.clone()),
                Transform::default(),
                ChildOf(root),
            ));
            if data.0.id.lod > 0 {
                e.insert(NotShadowCaster);
            }
            cm.vegetation = Some(e.id());
        }
        commands.entity(root).insert(cm);
    }
    let ms = t0.elapsed().as_secs_f32() * 1000.0;
    stats.visuals_ms_last = ms;
    stats.visuals_ms_max = stats.visuals_ms_max.max(ms);
}
