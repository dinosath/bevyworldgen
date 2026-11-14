//! Scripted flight + frame-time statistics (gates G2/G3). Enabled with `WG_BENCH=<seconds>`.

use crate::EditLayerRes;
use bevy::camera_controller::free_camera::FreeCamera;
use bevy::diagnostic::DiagnosticsStore;
use bevy::prelude::*;
use bevy_worldgen::{InvalidateRegion, StreamStats, TerrainQuery, WorldOrigin};
use bevy_worldgen_render::RenderStats;
use std::time::Instant;
use worldgen_terrain::BrushOp;

#[derive(Resource)]
pub struct Bench {
    pub seconds: f32,
    pub speed: f32,
    pub yaw: f32,
    /// Move the camera along the scripted flight (off when driving).
    pub scripted: bool,
    elapsed: f32,
    frames: Vec<(f32, f32, f32)>, // (t, frame_ms, collect_ms)
    window: Vec<f32>,
    last_print: f32,
    warm_at: Option<f32>,
    started: Instant,
}

impl Bench {
    pub fn new(seconds: f32, speed: f32, yaw: f32, scripted: bool) -> Self {
        Self {
            seconds,
            speed,
            yaw,
            scripted,
            elapsed: 0.0,
            frames: vec![],
            window: vec![],
            last_print: 0.0,
            warm_at: None,
            started: Instant::now(),
        }
    }
}

fn pct(v: &[f32], p: f32) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    v[(((v.len() - 1) as f32) * p) as usize]
}

pub fn fly(
    mut bench: ResMut<Bench>,
    time: Res<Time<Real>>,
    stats: Res<StreamStats>,
    rstats: Res<RenderStats>,
    drive: Option<Res<crate::drive::DriveStats>>,
    origin: Res<WorldOrigin>,
    q: TerrainQuery,
    store_diag: Res<DiagnosticsStore>,
    mut cam: Single<&mut Transform, With<Camera3d>>,
    mut exit: MessageWriter<AppExit>,
    all_entities: Query<()>,
    visible: Query<&ViewVisibility>,
) {
    let dt = time.delta_secs();
    bench.elapsed += dt;
    if bench.warm_at.is_none() && stats.plan_complete {
        bench.warm_at = Some(bench.elapsed + 2.0);
    }
    // fly only once the first plan is complete so the start is not a cold-start measurement
    let moving = bench.warm_at.is_some() && bench.scripted;
    if moving {
        bench.yaw += (bench.elapsed * 0.11).sin() * 0.25 * dt;
        let fwd = Vec3::new(bench.yaw.cos(), 0.0, bench.yaw.sin());
        cam.translation += fwd * bench.speed * dt;
        let w = origin.to_world(cam.translation);
        let ground = q.height_at(w.x, w.z).unwrap_or(0.0).max(0.0);
        let target = ground + 70.0 + 25.0 * (bench.elapsed * 0.4).sin();
        cam.translation.y += (target - cam.translation.y) * (1.0 - (-dt * 1.5).exp());
        let look = fwd * 60.0 + Vec3::Y * (-12.0);
        cam.look_to(look, Vec3::Y);
    }
    // statistics
    let ms = dt * 1000.0;
    bench.window.push(ms);
    if let Some(w) = bench.warm_at
        && bench.elapsed > w
    {
        let t = bench.elapsed;
        bench.frames.push((t, ms, stats.collect_ms_last));
    }
    if bench.elapsed - bench.last_print >= 2.0 {
        let mut v = std::mem::take(&mut bench.window);
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mut gpu = String::new();
        for d in store_diag.iter() {
            let p = d.path().as_str();
            if p.starts_with("render/")
                && p.ends_with("elapsed_gpu")
                && let Some(x) = d.smoothed()
                && x > 0.3
            {
                gpu += &format!(
                    " {}={:.2}",
                    p.trim_start_matches("render/")
                        .trim_end_matches("/elapsed_gpu"),
                    x
                );
            }
        }
        println!(
            "BENCH t={:.0}s fps={:.0} p50={:.1} p99={:.1} max={:.1} | chunks shown={} resident={} gen={} done={} | gen_ms ema={:.1} max={:.0} | collect max={:.2} realise max={:.2} visuals max={:.2} grass max={:.2} | entities={} visible={} | cpu_mb={:.0} rss_mb={:.0} | gpu_ms[{} ]",
            bench.elapsed,
            v.len() as f32 / 2.0,
            pct(&v, 0.5),
            pct(&v, 0.99),
            v.last().copied().unwrap_or(0.0),
            stats.shown,
            stats.resident,
            stats.generating,
            stats.jobs_done,
            stats.gen_ms_ema,
            stats.gen_ms_max,
            stats.collect_ms_max,
            stats.realise_ms_max,
            rstats.visuals_ms_max,
            rstats.grass_ms_max,
            all_entities.iter().count(),
            visible.iter().filter(|v| v.get()).count(),
            stats.cpu_mb,
            rss_mb(),
            gpu
        );
        if let Some(d) = &drive {
            println!(
                "DRIVE {} route={:.0} m progress={:.0} m dist={:.0} m speed_max={:.1} m/s max_dev={:.2} m offroad={:.1}s no_ground={:.1}s on_bridge={:.1}s done={}",
                d.road_name,
                d.route_len_m,
                d.progress,
                d.distance_m,
                d.max_speed,
                d.max_dev_m,
                d.offroad_s,
                d.no_ground_s,
                d.on_bridge_s,
                d.done
            );
        }
        bench.last_print = bench.elapsed;
    }
    if drive.as_ref().is_some_and(|d| d.done) && bench.elapsed < bench.seconds {
        bench.seconds = bench.elapsed + 0.01;
    }
    if bench.elapsed >= bench.seconds {
        let mut ft: Vec<f32> = bench.frames.iter().map(|f| f.1).collect();
        ft.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let mut ct: Vec<f32> = bench.frames.iter().map(|f| f.2).collect();
        ct.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let over = |th: f32| ft.iter().filter(|x| **x > th).count();
        println!(
            "BENCH_SUMMARY regen_done={} regen_latency_ms_last={:.0} regen_latency_ms_max={:.0} regen_pending={} frames={} warmup_s={:.1} p50={:.2} p99={:.2} max={:.2} over50ms={} over100ms={} collect_p50={:.3} collect_p99={:.3} collect_max={:.3} jobs_done={} gen_ms_ema={:.1} wall_s={:.0}",
            stats.regen_done,
            stats.regen_latency_ms_last,
            stats.regen_latency_ms_max,
            stats.regen_pending,
            ft.len(),
            bench.warm_at.unwrap_or(0.0),
            pct(&ft, 0.5),
            pct(&ft, 0.99),
            ft.last().copied().unwrap_or(0.0),
            over(50.0),
            over(100.0),
            pct(&ct, 0.5),
            pct(&ct, 0.99),
            ct.last().copied().unwrap_or(0.0),
            stats.jobs_done,
            stats.gen_ms_ema,
            bench.started.elapsed().as_secs_f32()
        );
        exit.write(AppExit::Success);
    }
}

/// Remove the free-camera controller while benchmarking so it cannot fight the script.
pub fn strip_free_camera(mut commands: Commands, q: Query<Entity, With<FreeCamera>>) {
    for e in &q {
        commands.entity(e).remove::<FreeCamera>();
    }
}

/// G9: apply a brush ahead of the camera every 2 s and let the streamer regenerate in place.
pub fn edit_storm(
    time: Res<Time<Real>>,
    origin: Res<WorldOrigin>,
    layer: Res<EditLayerRes>,
    q: TerrainQuery,
    cam: Single<&Transform, With<Camera3d>>,
    mut out: MessageWriter<InvalidateRegion>,
    mut last: Local<f32>,
) {
    let t = time.elapsed_secs();
    if t < 8.0 || t - *last < 2.0 {
        return;
    }
    *last = t;
    let fwd = cam.forward().as_vec3();
    let p =
        origin.to_world(cam.translation + Vec3::new(fwd.x, 0.0, fwd.z).normalize_or_zero() * 150.0);
    if q.height_at(p.x, p.z).is_some() {
        let b = layer.0.apply_brush(p.x, p.z, 22.0, &BrushOp::Raise(2.5));
        out.write(InvalidateRegion(b));
    }
}

/// Resident set size of this process in MiB (Linux; 0 elsewhere).
fn rss_mb() -> f32 {
    std::fs::read_to_string("/proc/self/statm")
        .ok()
        .and_then(|s| {
            s.split_whitespace()
                .nth(1)
                .and_then(|v| v.parse::<f32>().ok())
        })
        .map_or(0.0, |pages| pages * 4096.0 / (1024.0 * 1024.0))
}
