//! Streamed procedural world showcase.
//!
//! Env: `WG_SEED`, `WG_BENCH=<s>` (scripted 100 m/s flight + stats), `WG_SPEED`, `WG_SHOT=<png>@<frames>`,
//! `WG_START=x,z,y,yaw_deg,pitch_deg`, `WG_VSYNC=1`, `WG_THREADS`, `WG_NO_EROSION=1`.

mod bench;
mod drive;
mod editor;

use bevy::{
    anti_alias::fxaa::Fxaa,
    camera::Exposure,
    camera_controller::free_camera::{FreeCamera, FreeCameraPlugin},
    core_pipeline::tonemapping::Tonemapping,
    light::{
        Atmosphere, AtmosphereEnvironmentMapLight, CascadeShadowConfigBuilder, SunDisk,
        atmosphere::ScatteringMedium, light_consts::lux,
    },
    pbr::AtmosphereSettings,
    post_process::bloom::Bloom,
    prelude::*,
    render::view::screenshot::{Screenshot, save_to_disk},
};
use bevy_worldgen::{StreamFocus, StreamStats, WorldAnchored, WorldOrigin, WorldStreamingPlugin};
use bevy_worldgen_render::{Moon, Quality, TimeOfDay, Weather, WorldRenderPlugin};
use std::sync::Arc;
use worldgen_terrain::{ChunkConfig, EditLayer, TerrainParams};
use worldgen_world::World as GenWorld;

fn env_f32(k: &str) -> Option<f32> {
    std::env::var(k).ok()?.parse().ok()
}

/// The world's persistent terrain edit layer.
#[derive(Resource)]
pub struct EditLayerRes(pub Arc<EditLayer>);

#[derive(Resource)]
struct Shot {
    path: String,
    frame: u32,
    at: u32,
}

#[derive(Component)]
struct Hud;

/// `WG_START` with a negative altitude means "this many metres above the ground"; resolved once
/// terrain under the camera has loaded.
#[derive(Resource)]
struct AutoClearance(Option<f32>);

fn auto_place(
    mut ac: ResMut<AutoClearance>,
    origin: Res<WorldOrigin>,
    q: bevy_worldgen::TerrainQuery,
    mut cam: Single<&mut Transform, With<Camera3d>>,
) {
    let Some(c) = ac.0 else { return };
    let w = origin.to_world(cam.translation);
    if let Some(h) = q.height_at_fine(w.x, w.z, 1) {
        cam.translation.y = h.max(0.0) + c;
        ac.0 = None;
    }
}

fn main() {
    let seed = std::env::var("WG_SEED")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0x5EED_0001u64);
    let mut cfg = ChunkConfig::default();
    if std::env::var("WG_NO_EROSION").is_ok() {
        cfg.erosion.enabled = false;
    }
    // optional real-world region (see `geo-import`): DEM terrain, OSM roads and building footprints
    let geo = std::env::var("WG_GEO").ok().map(|d| {
        let o = worldgen_geo::GeoOverlay::load_dir(std::path::Path::new(&d), seed)
            .unwrap_or_else(|e| panic!("WG_GEO={d}: {e}"));
        println!(
            "geo region: {} roads, {} lots, DEM {}x{}, centre {:.4},{:.4}",
            o.roads.len(),
            o.lots.len(),
            o.dem.w,
            o.dem.h,
            o.lon0,
            o.lat0
        );
        std::sync::Arc::new(o)
    });
    let mut params = TerrainParams {
        seed,
        ..Default::default()
    };
    if let Some(g) = &geo {
        params.lat0_deg = g.lat0 as f32;
    }
    let mut world = GenWorld::new(params, cfg);
    if let Some(g) = &geo {
        let b = g.bounds();
        world.gen_world.add_modifier(g.clone());
        if let Some(rw) = &world.roads {
            rw.add_overlay(g.clone());
            rw.towns.add_exclusion((
                b.min_x - 400.0,
                b.min_z - 400.0,
                b.max_x + 400.0,
                b.max_z + 400.0,
            ));
        }
    }
    if let Ok(dir) = std::env::var("WG_CACHE") {
        world = world.with_cache(dir).expect("cache dir");
    }
    let edits = EditLayer::new(cfg.cell_m as f64);
    world.gen_world.add_modifier(edits.clone());
    let user_roads = worldgen_roads::UserRoads::new(world.gen_world.terrain.clone());
    if let Some(rw) = &world.roads {
        rw.add_overlay(user_roads.clone());
    }
    let edit_res = EditLayerRes(edits.clone());
    let road_res = editor::UserRoadsRes(user_roads);
    if let Ok(path) = std::env::var("WG_EDITS") {
        editor::load_all(&path, &edit_res, &road_res);
    }
    let drive_mode = std::env::var("WG_DRIVE").ok();
    let drive_route = drive_mode.as_ref().and_then(|_| {
        let start = std::env::var("WG_START")
            .ok()
            .map(|s| {
                s.split(',')
                    .filter_map(|v| v.parse::<f64>().ok())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        drive::find_route(
            &world,
            start.first().copied().unwrap_or(0.0),
            start.get(1).copied().unwrap_or(0.0),
        )
    });
    let mut plugin = WorldStreamingPlugin::new(world);
    plugin.threads = std::env::var("WG_THREADS")
        .ok()
        .and_then(|s| s.parse().ok());

    let present = if std::env::var("WG_VSYNC").is_ok() {
        bevy::window::PresentMode::AutoVsync
    } else {
        bevy::window::PresentMode::AutoNoVsync
    };
    let mut app = App::new();
    app.add_plugins((
        DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "bevy-worldgen showcase".into(),
                present_mode: present,
                resolution: (1280, 720).into(),
                ..default()
            }),
            ..default()
        }),
        FreeCameraPlugin,
        plugin,
        WorldRenderPlugin,
    ))
    .insert_resource(edit_res)
    .insert_resource(road_res)
    .init_resource::<editor::Editor>()
    .insert_resource(GlobalAmbientLight::NONE)
    .insert_resource(
        std::env::var("WG_QUALITY")
            .ok()
            .and_then(|q| Quality::parse(&q))
            .unwrap_or(Quality::High),
    )
    .insert_resource(TimeOfDay {
        hour: env_f32("WG_HOUR").unwrap_or(15.5),
        speed: env_f32("WG_TOD_SPEED").unwrap_or(0.0),
    })
    .insert_resource(
        std::env::var("WG_WEATHER")
            .ok()
            .and_then(|w| Weather::preset(&w))
            .unwrap_or_default(),
    )
    .insert_resource(ClearColor(Color::BLACK))
    .add_systems(Startup, setup)
    .add_systems(Update, (hud, auto_place, controls, editor::editor_input));
    if std::env::var("WG_EDIT_SCRIPT").is_ok() {
        app.add_systems(Update, editor::edit_script);
    }

    if let Some(mode) = drive_mode {
        app.insert_resource(drive::DriveMode {
            auto: mode == "auto",
        })
        .init_resource::<drive::DriveStats>();
        if let Some(r) = drive_route {
            app.insert_resource(r);
        }
        app.add_systems(PostStartup, (drive::spawn_car, drive::strip_camera_control))
            .add_systems(
                Update,
                drive::drive.before(bevy_worldgen::WorldSet::Observe),
            )
            .add_systems(
                Update,
                drive::chase_camera
                    .after(drive::drive)
                    .before(bevy_worldgen::WorldSet::Observe),
            );
    }
    if let Some(secs) = env_f32("WG_BENCH") {
        app.add_plugins((
            bevy::diagnostic::FrameTimeDiagnosticsPlugin::default(),
            bevy::render::diagnostic::RenderDiagnosticsPlugin,
        ))
        .insert_resource(bench::Bench::new(
            secs,
            env_f32("WG_SPEED").unwrap_or(100.0),
            0.3,
            std::env::var("WG_DRIVE").is_err(),
        ))
        .add_systems(PostStartup, bench::strip_free_camera)
        .add_systems(Update, bench::fly.after(bevy_worldgen::WorldSet::Evict));
        if std::env::var("WG_EDIT_BENCH").is_ok() {
            app.add_systems(
                Update,
                bench::edit_storm.before(bevy_worldgen::WorldSet::Observe),
            );
        }
    }
    if let Ok(s) = std::env::var("WG_SHOT") {
        let (path, at) = s
            .split_once('@')
            .map(|(p, a)| (p.to_string(), a.parse().unwrap_or(300)))
            .unwrap_or((s.clone(), 300));
        app.insert_resource(Shot { path, frame: 0, at })
            .add_systems(Update, shot);
    }
    app.insert_resource(AutoClearance(start_clearance()));
    app.run();
}

fn start_clearance() -> Option<f32> {
    let s = std::env::var("WG_START").ok()?;
    let v: Vec<f32> = s.split(',').filter_map(|x| x.parse().ok()).collect();
    v.get(2).copied().filter(|y| *y < 0.0).map(|y| -y)
}

fn setup(mut commands: Commands, mut media: ResMut<Assets<ScatteringMedium>>) {
    let earth = media.add(ScatteringMedium::earth(256, 256));
    commands.spawn(Atmosphere::earth(earth));

    let start = std::env::var("WG_START")
        .ok()
        .map(|s| {
            s.split(',')
                .filter_map(|v| v.parse::<f32>().ok())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let get = |i: usize, d: f32| start.get(i).copied().unwrap_or(d);
    let (x, z, y, yaw, pitch) = (
        get(0, 0.0),
        get(1, 0.0),
        get(2, 220.0),
        get(3, 20.0),
        get(4, -12.0),
    );
    let cam = Transform::from_xyz(x, y, z).with_rotation(Quat::from_euler(
        EulerRot::YXZ,
        (-yaw).to_radians(),
        pitch.to_radians(),
        0.0,
    ));
    let mut ec = commands.spawn((
        Camera3d::default(),
        Projection::from(PerspectiveProjection {
            near: 0.3,
            far: 90_000.0,
            ..default()
        }),
        cam,
        AtmosphereSettings::default(),
        Exposure { ev100: 13.0 },
        Tonemapping::AcesFitted,
        Bloom::NATURAL,
        AtmosphereEnvironmentMapLight::default(),
        Msaa::Off,
        Fxaa::default(),
        StreamFocus,
        WorldAnchored,
    ));
    ec.insert(FreeCamera {
        walk_speed: 60.0,
        run_speed: 600.0,
        ..default()
    });

    commands.spawn((
        DirectionalLight {
            shadow_maps_enabled: true,
            illuminance: lux::RAW_SUNLIGHT,
            ..default()
        },
        CascadeShadowConfigBuilder {
            num_cascades: 4,
            maximum_distance: 1500.0,
            first_cascade_far_bound: 40.0,
            ..default()
        }
        .build(),
        Transform::from_xyz(0.4, 0.55, 0.3).looking_at(Vec3::ZERO, Vec3::Y),
        SunDisk::EARTH,
    ));

    commands.spawn((
        Moon,
        DirectionalLight {
            illuminance: 0.0,
            color: Color::srgb(0.6, 0.7, 1.0),
            shadow_maps_enabled: false,
            ..default()
        },
        Transform::default(),
    ));

    commands.spawn((
        Hud,
        Text::new(""),
        TextFont::default(),
        Node {
            position_type: PositionType::Absolute,
            top: px(8),
            left: px(8),
            ..default()
        },
    ));
}

fn hud(
    time: Res<Time>,
    ed: Res<editor::Editor>,
    stats: Res<StreamStats>,
    origin: Res<WorldOrigin>,
    cam: Single<&Transform, With<Camera3d>>,
    mut text: Single<&mut Text, With<Hud>>,
    mut acc: Local<(f32, u32)>,
) {
    acc.0 += time.delta_secs();
    acc.1 += 1;
    if acc.0 < 0.25 {
        return;
    }
    let fps = acc.1 as f32 / acc.0;
    *acc = (0.0, 0);
    let p = origin.to_world(cam.translation);
    text.0 = format!(
        "{fps:.0} fps\npos {:.0}, {:.0}  alt {:.0} m\nchunks shown {} / resident {} / generating {}\ngen {:.1} ms (max {:.0})  collect max {:.2} ms\ncpu {:.0} MB",
        p.x,
        p.z,
        cam.translation.y,
        stats.shown,
        stats.resident,
        stats.generating,
        stats.gen_ms_ema,
        stats.gen_ms_max,
        stats.collect_ms_max,
        stats.cpu_mb
    );
    if ed.active {
        text.0 += &format!(
            "\n[EDIT] tool {:?}  radius {:.0} m  road points {}  undo {}\n{}",
            ed.tool,
            ed.radius,
            ed.road_clicks.len(),
            ed.can_undo() as u8,
            ed.status
        );
    }
}

fn shot(
    mut commands: Commands,
    mut s: ResMut<Shot>,
    stats: Res<StreamStats>,
    mut exit: MessageWriter<AppExit>,
) {
    s.frame += 1;
    // wait for the plan to complete, then at least `at` frames
    if s.frame == s.at && stats.generating == 0 {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(s.path.clone()));
    } else if s.frame == s.at {
        s.at += 30; // keep waiting until generation drains
    }
    if s.frame == s.at + 40 {
        exit.write(AppExit::Success);
    }
}

/// F1-F4 quality, T fast time of day, [ ] step the hour, 1-4 weather presets.
fn controls(
    keys: Res<ButtonInput<KeyCode>>,
    mut q: ResMut<Quality>,
    mut tod: ResMut<TimeOfDay>,
    mut w: ResMut<Weather>,
) {
    for (k, v) in [
        (KeyCode::F1, Quality::Low),
        (KeyCode::F2, Quality::Medium),
        (KeyCode::F3, Quality::High),
        (KeyCode::F4, Quality::Ultra),
    ] {
        if keys.just_pressed(k) {
            *q = v;
        }
    }
    if keys.just_pressed(KeyCode::KeyT) {
        tod.speed = if tod.speed == 0.0 { 0.25 } else { 0.0 };
    }
    if keys.just_pressed(KeyCode::BracketRight) {
        tod.hour = (tod.hour + 1.0) % 24.0;
    }
    if keys.just_pressed(KeyCode::BracketLeft) {
        tod.hour = (tod.hour + 23.0) % 24.0;
    }
    for (k, name) in [
        (KeyCode::Digit1, "clear"),
        (KeyCode::Digit2, "cloudy"),
        (KeyCode::Digit3, "rain"),
        (KeyCode::Digit4, "storm"),
    ] {
        if keys.just_pressed(k)
            && let Some(p) = Weather::preset(name)
        {
            *w = p;
        }
    }
}
