//! Weather: clouds, rain, wetness, fog and wind. Targets are set on [`Weather`]; [`WeatherState`] is
//! the smoothed state the effects read, so changes fade in over seconds.

use crate::materials::{TerrainMaterial, WaterMaterial};
use crate::sky::SkyState;
use bevy::pbr::{DistanceFog, FogFalloff, Material, MaterialPlugin};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, ShaderType};
use bevy::shader::ShaderRef;
use bevy_grass::prelude::GrassWind;
use bevy_worldgen::WorldOrigin;

#[derive(Resource, Clone, Copy, Debug)]
pub struct Weather {
    /// Target rain intensity 0..1.
    pub rain: f32,
    /// Target cloud coverage 0..1.
    pub cloud: f32,
    /// Wind (x, z) m/s.
    pub wind: Vec2,
}

impl Default for Weather {
    fn default() -> Self {
        Self {
            rain: 0.0,
            cloud: 0.25,
            wind: Vec2::new(3.0, 1.0),
        }
    }
}

impl Weather {
    pub fn preset(name: &str) -> Option<Self> {
        Some(match name {
            "clear" => Self {
                rain: 0.0,
                cloud: 0.12,
                wind: Vec2::new(2.0, 1.0),
            },
            "cloudy" => Self {
                rain: 0.0,
                cloud: 0.65,
                wind: Vec2::new(5.0, 2.0),
            },
            "rain" => Self {
                rain: 0.65,
                cloud: 0.9,
                wind: Vec2::new(7.0, 3.0),
            },
            "storm" => Self {
                rain: 1.0,
                cloud: 1.0,
                wind: Vec2::new(14.0, 6.0),
            },
            _ => return None,
        })
    }
}

#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct WeatherState {
    pub rain: f32,
    pub cloud: f32,
    /// Surface wetness 0..1 (rises quickly in rain, dries slowly).
    pub wet: f32,
    pub wind: Vec2,
}

#[derive(Clone, Debug, ShaderType)]
pub struct CloudUniforms {
    pub sun: Vec4,
    pub tint: Vec4,
    pub origin: Vec2,
    pub wind: Vec2,
    pub coverage: f32,
    pub darkness: f32,
    pub _pad: Vec2,
}

#[derive(Asset, TypePath, AsBindGroup, Clone)]
pub struct CloudMaterial {
    #[uniform(0)]
    pub u: CloudUniforms,
}

impl Material for CloudMaterial {
    fn fragment_shader() -> ShaderRef {
        "embedded://bevy_worldgen_render/shaders/clouds.wesl".into()
    }
    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }
}

#[derive(Component)]
pub struct CloudLayer;

#[derive(Component)]
pub struct RainDrop {
    offset: Vec3,
    speed: f32,
}

#[derive(Resource)]
pub(crate) struct CloudHandle(Handle<CloudMaterial>);

pub(crate) struct WeatherPlugin;

const CLOUD_ALTITUDE: f32 = 2800.0;
const RAIN_DROPS: usize = 900;

impl Plugin for WeatherPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(MaterialPlugin::<CloudMaterial>::default())
            .init_resource::<Weather>()
            .init_resource::<WeatherState>()
            .add_systems(Startup, spawn_effects)
            .add_systems(
                Update,
                (
                    blend_weather,
                    update_clouds,
                    update_wetness,
                    update_rain,
                    update_fog_and_wind,
                )
                    .chain(),
            );
    }
}

fn spawn_effects(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut clouds: ResMut<Assets<CloudMaterial>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
) {
    let h = clouds.add(CloudMaterial {
        u: CloudUniforms {
            sun: Vec4::new(0.0, 1.0, 0.0, 1.0),
            tint: Vec4::ONE,
            origin: Vec2::ZERO,
            wind: Vec2::new(3.0, 1.0),
            coverage: 0.3,
            darkness: 0.6,
            _pad: Vec2::ZERO,
        },
    });
    commands.insert_resource(CloudHandle(h.clone()));
    // the plane faces down so it is visible from below
    commands.spawn((
        CloudLayer,
        Mesh3d(meshes.add(Plane3d::default().mesh().size(90_000.0, 90_000.0))),
        MeshMaterial3d(h),
        Transform::from_xyz(0.0, CLOUD_ALTITUDE, 0.0)
            .with_rotation(Quat::from_rotation_x(std::f32::consts::PI)),
        bevy::light::NotShadowCaster,
    ));
    let drop_mesh = meshes.add(Cuboid::new(0.012, 0.55, 0.012));
    let drop_mat = mats.add(StandardMaterial {
        base_color: Color::srgba(0.75, 0.82, 0.95, 0.35),
        unlit: true,
        alpha_mode: AlphaMode::Blend,
        ..default()
    });
    let mut seed = 0x1234_5678u32;
    let mut rnd = move || {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        (seed >> 8) as f32 / (1u32 << 24) as f32
    };
    for _ in 0..RAIN_DROPS {
        let offset = Vec3::new(
            (rnd() - 0.5) * 36.0,
            rnd() * 22.0 - 4.0,
            (rnd() - 0.5) * 36.0,
        );
        commands.spawn((
            RainDrop {
                offset,
                speed: 8.0 + rnd() * 3.0,
            },
            Mesh3d(drop_mesh.clone()),
            MeshMaterial3d(drop_mat.clone()),
            Transform::from_translation(offset),
            Visibility::Hidden,
            bevy::light::NotShadowCaster,
        ));
    }
}

fn blend_weather(time: Res<Time<Real>>, w: Res<Weather>, mut s: ResMut<WeatherState>) {
    let dt = time.delta_secs();
    let k = |rate: f32| 1.0 - (-rate * dt).exp();
    s.rain += (w.rain - s.rain) * k(0.35);
    s.cloud += (w.cloud - s.cloud) * k(0.12);
    let wd = (w.wind - s.wind) * k(0.2);
    s.wind += wd;
    let target = if s.rain > 0.05 { 1.0 } else { 0.0 };
    let rate = if target > s.wet {
        0.25 * s.rain.max(0.2)
    } else {
        0.02
    };
    s.wet += (target - s.wet) * k(rate);
}

fn update_clouds(
    sky: Res<SkyState>,
    st: Res<WeatherState>,
    origin: Res<WorldOrigin>,
    handle: Option<Res<CloudHandle>>,
    mut mats: ResMut<Assets<CloudMaterial>>,
    cam: Query<&Transform, (With<Camera3d>, Without<CloudLayer>)>,
    mut layer: Query<&mut Transform, With<CloudLayer>>,
) {
    let (Some(h), Ok(c)) = (handle, cam.single()) else {
        return;
    };
    for mut t in &mut layer {
        t.translation.x = c.translation.x;
        t.translation.z = c.translation.z;
    }
    if let Some(mut m) = mats.get_mut(&h.0) {
        let day = (1.0 - sky.night).clamp(0.0, 1.0);
        let warm = Vec3::new(1.0, 1.0 - 0.38 * sky.dusk, 1.0 - 0.7 * sky.dusk);
        let tint = warm * (0.04 + 1.0 * day) * (1.0 - 0.35 * st.cloud);
        m.u.sun = sky.sun_dir.extend(sky.elevation);
        m.u.tint = tint.extend(1.0);
        m.u.origin = Vec2::new(origin.0.x as f32, origin.0.z as f32);
        m.u.wind = st.wind;
        m.u.coverage = st.cloud;
        m.u.darkness = 0.55 - 0.25 * st.rain;
    }
}

fn update_wetness(
    st: Res<WeatherState>,
    mats: Option<Res<crate::WorldMaterials>>,
    mut terrain: ResMut<Assets<TerrainMaterial>>,
    mut water: ResMut<Assets<WaterMaterial>>,
    mut std_mats: ResMut<Assets<StandardMaterial>>,
) {
    let Some(m) = mats else { return };
    if let Some(mut t) = terrain.get_mut(&m.terrain) {
        t.extension.uniforms.wetness = st.wet;
    }
    if let Some(mut w) = water.get_mut(&m.water) {
        w.extension.uniforms.wave_strength =
            0.35 + 0.9 * st.rain + 0.0004 * st.wind.length_squared();
    }
    if let Some(mut r) = std_mats.get_mut(&m.roads) {
        r.perceptual_roughness = 0.82 - 0.55 * st.wet;
    }
    if let Some(mut b) = std_mats.get_mut(&m.buildings) {
        b.perceptual_roughness = 0.85 - 0.35 * st.wet;
    }
}

fn update_rain(
    time: Res<Time<Real>>,
    st: Res<WeatherState>,
    cam: Query<&Transform, (With<Camera3d>, Without<RainDrop>)>,
    mut drops: Query<(&mut RainDrop, &mut Transform, &mut Visibility)>,
) {
    let Ok(c) = cam.single() else { return };
    let dt = time.delta_secs();
    let active = (st.rain * RAIN_DROPS as f32) as usize;
    let slant = Vec3::new(st.wind.x, 0.0, st.wind.y) * 0.18;
    for (i, (mut d, mut t, mut v)) in drops.iter_mut().enumerate() {
        if i >= active {
            *v = Visibility::Hidden;
            continue;
        }
        *v = Visibility::Inherited;
        d.offset.y -= d.speed * dt;
        d.offset += slant * dt;
        if d.offset.y < -4.0 {
            d.offset.y += 22.0;
        }
        for k in [0usize, 2] {
            if d.offset[k] > 18.0 {
                d.offset[k] -= 36.0;
            } else if d.offset[k] < -18.0 {
                d.offset[k] += 36.0;
            }
        }
        t.translation = c.translation + d.offset;
        let fall = Vec3::new(slant.x, -d.speed, slant.z).normalize();
        t.rotation = Quat::from_rotation_arc(Vec3::Y, -fall);
    }
}

fn update_fog_and_wind(
    mut commands: Commands,
    st: Res<WeatherState>,
    sky: Res<SkyState>,
    cams: Query<Entity, With<Camera3d>>,
    mut wind: Option<ResMut<GrassWind>>,
) {
    for e in &cams {
        let vis = 18000.0 - 16000.0 * st.rain.max(st.cloud * 0.35);
        if st.rain > 0.02 || st.cloud > 0.5 {
            let day = 1.0 - sky.night;
            let grey = Color::srgba(
                0.55 * day + 0.03,
                0.6 * day + 0.035,
                0.66 * day + 0.045,
                1.0,
            );
            commands.entity(e).insert(DistanceFog {
                color: grey,
                falloff: FogFalloff::from_visibility(vis.max(900.0)),
                ..default()
            });
        } else {
            commands.entity(e).remove::<DistanceFog>();
        }
    }
    if let Some(w) = wind.as_mut() {
        w.direction = st.wind.normalize_or(Vec2::X);
        w.strength = (st.wind.length() / 16.0).clamp(0.05, 1.0);
    }
}
