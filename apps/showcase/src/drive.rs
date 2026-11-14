//! Drivable car (kinematic bicycle model with surface following) and a road-following autopilot.
//! `WG_DRIVE=manual` (WASD/arrows) or `WG_DRIVE=auto` (drives the longest highway/rural road near
//! the start and records how well the network holds up).

use crate::bench::Bench;
use bevy::camera_controller::free_camera::FreeCamera;
use bevy::math::DVec3;
use bevy::prelude::*;
use bevy_worldgen::{StreamFocus, TerrainQuery, WorldOrigin, WorldRes};
use std::sync::Arc;
use worldgen_roads::{Road, RoadClass};
use worldgen_terrain::Bounds;

#[derive(Component)]
pub struct Car {
    pub world: DVec3,
    pub heading: f32,
    pub speed: f32,
    pub steer: f32,
}

#[derive(Resource)]
pub struct DriveMode {
    pub auto: bool,
}

#[derive(Resource, Default, Debug, Clone)]
pub struct DriveStats {
    pub distance_m: f64,
    pub max_dev_m: f32,
    pub offroad_s: f32,
    pub no_ground_s: f32,
    pub on_bridge_s: f32,
    pub road_name: String,
    pub route_len_m: f64,
    pub progress: f64,
    pub done: bool,
    pub max_speed: f32,
}

#[derive(Resource)]
pub struct Route {
    pub road: Arc<Road>,
    cum: Vec<f64>,
}

impl Route {
    fn new(road: Arc<Road>) -> Self {
        let mut cum = vec![0.0];
        for w in road.pts.windows(2) {
            let l = ((w[1][0] - w[0][0]).powi(2) + (w[1][1] - w[0][1]).powi(2)).sqrt();
            cum.push(cum.last().unwrap() + l);
        }
        Self { road, cum }
    }

    fn at(&self, s: f64) -> ([f64; 2], [f64; 2]) {
        let s = s.clamp(0.0, *self.cum.last().unwrap());
        let k = self
            .cum
            .partition_point(|c| *c <= s)
            .saturating_sub(1)
            .min(self.road.pts.len() - 2);
        let (a, b) = (self.road.pts[k], self.road.pts[k + 1]);
        let l = (self.cum[k + 1] - self.cum[k]).max(1e-9);
        let t = (s - self.cum[k]) / l;
        (
            [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t],
            [(b[0] - a[0]) / l, (b[1] - a[1]) / l],
        )
    }

    /// (arc length of the closest point, distance to the route) searching near `hint`.
    fn project(&self, p: [f64; 2], hint: f64) -> (f64, f64) {
        let (mut best_s, mut best_d) = (hint, f64::MAX);
        let k0 = self
            .cum
            .partition_point(|c| *c < hint - 60.0)
            .saturating_sub(1);
        let k1 = self
            .cum
            .partition_point(|c| *c < hint + 120.0)
            .min(self.road.pts.len() - 1);
        for k in k0..k1 {
            let (a, b) = (self.road.pts[k], self.road.pts[k + 1]);
            let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
            let l2 = (dx * dx + dz * dz).max(1e-9);
            let t = (((p[0] - a[0]) * dx + (p[1] - a[1]) * dz) / l2).clamp(0.0, 1.0);
            let d = ((p[0] - a[0] - dx * t).powi(2) + (p[1] - a[1] - dz * t).powi(2)).sqrt();
            if d < best_d {
                best_d = d;
                best_s = self.cum[k] + t * l2.sqrt();
            }
        }
        (best_s, best_d)
    }
}

/// Pick the longest highway/rural road in a 12 km window around `(x, z)`.
pub fn find_route(world: &worldgen_world::World, x: f64, z: f64) -> Option<Route> {
    let rw = world.roads.as_ref()?;
    let b = Bounds {
        min_x: x - 6000.0,
        min_z: z - 6000.0,
        max_x: x + 6000.0,
        max_z: z + 6000.0,
    };
    let (roads, _) = rw.roads_near(&b);
    roads
        .into_iter()
        .filter(|r| matches!(r.class, RoadClass::Highway | RoadClass::Rural))
        .max_by(|a, b| a.length().total_cmp(&b.length()))
        .map(Route::new)
}

pub fn spawn_car(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut mats: ResMut<Assets<StandardMaterial>>,
    route: Option<Res<Route>>,
    origin: Res<WorldOrigin>,
    mut stats: ResMut<DriveStats>,
) {
    let (p0, dir) = match &route {
        Some(r) => {
            stats.route_len_m = *r.cum.last().unwrap();
            stats.road_name = format!("{:?} #{:x}", r.road.class, r.road.id & 0xffff);
            r.at(0.0)
        }
        None => ([0.0, 0.0], [1.0, 0.0]),
    };
    let heading = dir[1].atan2(dir[0]) as f32;
    let body = mats.add(StandardMaterial {
        base_color: Color::srgb(0.75, 0.05, 0.04),
        metallic: 0.6,
        perceptual_roughness: 0.3,
        ..default()
    });
    let glass = mats.add(StandardMaterial {
        base_color: Color::srgb(0.02, 0.03, 0.04),
        metallic: 0.9,
        perceptual_roughness: 0.1,
        ..default()
    });
    let tyre = mats.add(StandardMaterial {
        base_color: Color::srgb(0.02, 0.02, 0.02),
        perceptual_roughness: 0.9,
        ..default()
    });
    let car = commands
        .spawn((
            Car {
                world: DVec3::new(p0[0], 0.0, p0[1]),
                heading,
                speed: 0.0,
                steer: 0.0,
            },
            Transform::from_translation(origin.to_render(DVec3::new(p0[0], 200.0, p0[1]))),
            Visibility::Inherited,
            bevy_worldgen::WorldAnchored,
        ))
        .id();
    let b = meshes.add(Cuboid::new(1.9, 0.62, 4.3));
    let c = meshes.add(Cuboid::new(1.6, 0.5, 2.1));
    let w = meshes.add(Cylinder::new(0.34, 0.26));
    commands.spawn((
        Mesh3d(b),
        MeshMaterial3d(body.clone()),
        Transform::from_xyz(0.0, 0.62, 0.0),
        ChildOf(car),
    ));
    commands.spawn((
        Mesh3d(c),
        MeshMaterial3d(glass),
        Transform::from_xyz(0.0, 1.12, 0.3),
        ChildOf(car),
    ));
    for (x, z) in [(-0.98, -1.4), (0.98, -1.4), (-0.98, 1.4), (0.98, 1.4)] {
        commands.spawn((
            Mesh3d(w.clone()),
            MeshMaterial3d(tyre.clone()),
            Transform::from_xyz(x, 0.34, z)
                .with_rotation(Quat::from_rotation_z(std::f32::consts::FRAC_PI_2)),
            ChildOf(car),
        ));
    }
}

#[allow(clippy::too_many_arguments)]
pub fn drive(
    time: Res<Time<Real>>,
    keys: Res<ButtonInput<KeyCode>>,
    mode: Res<DriveMode>,
    route: Option<Res<Route>>,
    origin: Res<WorldOrigin>,
    q: TerrainQuery,
    mut stats: ResMut<DriveStats>,
    mut car: Single<(&mut Car, &mut Transform)>,
) {
    let dt = time.delta_secs().min(0.05);
    let (c, tr) = &mut *car;
    // ---- control
    let (mut throttle, mut steer_in) = (0.0f32, 0.0f32);
    if mode.auto {
        if let Some(r) = &route {
            let (s, dev) = r.project([c.world.x, c.world.z], stats.progress);
            stats.progress = s.max(stats.progress.min(s + 5.0));
            stats.max_dev_m = stats.max_dev_m.max(dev as f32);
            let look = 6.0 + c.speed as f64 * 0.9;
            let (target, _) = r.at(s + look);
            // curvature ahead -> speed limit
            let (_, d0) = r.at(s + 10.0);
            let (_, d1) = r.at(s + 55.0);
            let turn = (d0[0] * d1[1] - d0[1] * d1[0]).abs().min(1.0) as f32;
            let v_target = (24.0 - 55.0 * turn).clamp(7.0, 24.0);
            if s >= r.cum.last().unwrap() - 12.0 {
                stats.done = true;
            }
            let want = (target[1] - c.world.z).atan2(target[0] - c.world.x) as f32;
            let mut d = want - c.heading;
            while d > std::f32::consts::PI {
                d -= std::f32::consts::TAU;
            }
            while d < -std::f32::consts::PI {
                d += std::f32::consts::TAU;
            }
            steer_in = (d * 1.6).clamp(-1.0, 1.0);
            throttle = ((v_target - c.speed) * 0.5).clamp(-1.0, 1.0);
        }
    } else {
        if keys.pressed(KeyCode::KeyW) || keys.pressed(KeyCode::ArrowUp) {
            throttle += 1.0;
        }
        if keys.pressed(KeyCode::KeyS) || keys.pressed(KeyCode::ArrowDown) {
            throttle -= 1.0;
        }
        if keys.pressed(KeyCode::KeyA) || keys.pressed(KeyCode::ArrowLeft) {
            steer_in -= 1.0;
        }
        if keys.pressed(KeyCode::KeyD) || keys.pressed(KeyCode::ArrowRight) {
            steer_in += 1.0;
        }
    }
    // ---- dynamics
    let accel = if throttle >= 0.0 {
        throttle * 9.0
    } else {
        throttle * 18.0
    };
    c.speed = (c.speed + (accel - 0.015 * c.speed * c.speed.abs() - 0.25 * c.speed.signum()) * dt)
        .clamp(-8.0, 55.0);
    if c.speed.abs() < 0.2 && throttle == 0.0 {
        c.speed = 0.0;
    }
    let max_steer = 0.55 / (1.0 + c.speed.abs() / 11.0);
    c.steer += (steer_in * max_steer - c.steer) * (1.0 - (-8.0 * dt).exp());
    // bicycle model, wheelbase 2.8 m (heading is measured in the x/z plane: forward = (cos h, sin h))
    c.heading += c.speed / 2.8 * c.steer.tan() * dt;
    let (sh, ch) = c.heading.sin_cos();
    c.world.x += (ch * c.speed * dt) as f64;
    c.world.z += (sh * c.speed * dt) as f64;
    stats.distance_m += (c.speed.abs() * dt) as f64;
    stats.max_speed = stats.max_speed.max(c.speed);
    // ---- surface following
    let ref_y = c.world.y as f32;
    let hit = q.surface_at(c.world.x, c.world.z, ref_y);
    match hit {
        Some((y, on_road)) => {
            if c.world.y == 0.0 {
                c.world.y = y as f64;
            }
            c.world.y += (y as f64 - c.world.y) * (1.0 - (-18.0 * dt as f64).exp());
            if mode.auto {
                if !on_road {
                    stats.offroad_s += dt;
                }
                if on_road && y > q.height_at(c.world.x, c.world.z).unwrap_or(y) + 1.0 {
                    stats.on_bridge_s += dt;
                }
            }
        }
        None => {
            stats.no_ground_s += dt;
        }
    }
    // attitude from four surface samples
    let probe = |dx: f32, dz: f32| {
        let (fx, fz) = (ch * dx - sh * dz, sh * dx + ch * dz);
        q.surface_at(
            c.world.x + fx as f64,
            c.world.z + fz as f64,
            c.world.y as f32,
        )
        .map(|v| v.0)
        .unwrap_or(c.world.y as f32)
    };
    let (yf, yb) = (probe(1.8, 0.0), probe(-1.8, 0.0));
    let (yr, yl) = (probe(0.0, 0.95), probe(0.0, -0.95));
    let pitch = ((yf - yb) / 3.6).atan();
    let roll = ((yr - yl) / 1.9).atan();
    let phi = -c.heading - std::f32::consts::FRAC_PI_2;
    tr.rotation =
        Quat::from_rotation_y(phi) * Quat::from_rotation_x(pitch) * Quat::from_rotation_z(roll);
    tr.translation = origin.to_render(c.world);
}

pub fn chase_camera(
    time: Res<Time<Real>>,
    car: Single<(&Car, &Transform), Without<Camera3d>>,
    mut cam: Single<&mut Transform, With<Camera3d>>,
) {
    let (c, ct) = *car;
    let dt = time.delta_secs().min(0.05);
    let (sh, ch) = c.heading.sin_cos();
    let back = Vec3::new(-ch, 0.0, -sh);
    let want = ct.translation + back * (7.5 + c.speed.max(0.0) * 0.08) + Vec3::Y * 2.8;
    let k = 1.0 - (-6.0 * dt).exp();
    cam.translation = cam.translation.lerp(want, k);
    let look_at = ct.translation + Vec3::new(ch, 0.0, sh) * 8.0 + Vec3::Y * 1.0;
    let target = Transform::from_translation(cam.translation).looking_at(look_at, Vec3::Y);
    cam.rotation = cam.rotation.slerp(target.rotation, k.max(0.25));
}

pub fn strip_camera_control(
    mut commands: Commands,
    q: Query<Entity, (With<Camera3d>, With<FreeCamera>)>,
) {
    for e in &q {
        commands.entity(e).remove::<FreeCamera>();
    }
}

#[allow(dead_code)]
pub fn mark_unused(_: &WorldRes, _: &Bench, _: &StreamFocus) {}
