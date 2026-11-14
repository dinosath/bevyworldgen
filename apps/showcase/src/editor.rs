//! World editor: terrain brushes and a road tool with undo/redo and save/load.
//!
//! `E` toggles edit mode. Tools: `Z` raise, `X` lower, `C` smooth, `V` flatten, `B` road.
//! LMB applies (RMB = opposite), `-`/`=` radius, `Ctrl+Z`/`Ctrl+Y` undo/redo, `Ctrl+S` save.
//! Road tool: LMB adds points, `Enter` builds, `Esc` cancels, `Backspace` removes a point.
//! `WG_EDIT_SCRIPT=1` runs the same operations from a script (no mouse needed).

use crate::EditLayerRes;
use bevy::camera_controller::free_camera::FreeCamera;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;
use bevy_worldgen::{InvalidateRegion, TerrainQuery, WorldOrigin};
use std::sync::Arc;
use worldgen_roads::{RoadClass, UserRoads};
use worldgen_terrain::{Bounds, BrushOp, EditSnapshot};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Raise,
    Lower,
    Smooth,
    Flatten,
    Road,
}

enum Undo {
    Terrain(EditSnapshot),
    Road {
        clicks: Vec<(f64, f64)>,
        class: RoadClass,
    },
}

#[derive(Resource)]
pub struct UserRoadsRes(pub Arc<UserRoads>);

#[derive(Resource)]
pub struct Editor {
    pub active: bool,
    pub tool: Tool,
    pub radius: f32,
    pub road_clicks: Vec<(f64, f64)>,
    flatten_target: Option<f32>,
    stroke: bool,
    cooldown: f32,
    undo: Vec<Undo>,
    redo: Vec<Undo>,
    pub status: String,
    pub path: String,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            active: false,
            tool: Tool::Raise,
            radius: 18.0,
            road_clicks: vec![],
            flatten_target: None,
            stroke: false,
            cooldown: 0.0,
            undo: vec![],
            redo: vec![],
            status: String::new(),
            path: std::env::var("WG_EDITS").unwrap_or_else(|_| "edits.wgedit".into()),
        }
    }
}

impl Editor {
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
}

/// Camera ray -> point on the terrain (world coordinates).
fn pick(q: &TerrainQuery, origin: &WorldOrigin, ray: Ray3d) -> Option<(f64, f64, f32)> {
    let o = origin.to_world(ray.origin);
    let d = ray.direction.as_vec3();
    let (mut t_prev, mut t) = (0.0f64, 0.0f64);
    let mut step = 3.0;
    while t < 4000.0 {
        t += step;
        let (x, y, z) = (
            o.x + d.x as f64 * t,
            o.y + d.y as f64 * t,
            o.z + d.z as f64 * t,
        );
        if let Some(h) = q.height_at(x, z)
            && (y as f32) < h
        {
            // bisect between the last two samples
            let (mut lo, mut hi) = (t_prev, t);
            for _ in 0..12 {
                let m = 0.5 * (lo + hi);
                let (mx, my, mz) = (
                    o.x + d.x as f64 * m,
                    o.y + d.y as f64 * m,
                    o.z + d.z as f64 * m,
                );
                if q.height_at(mx, mz).is_some_and(|hh| (my as f32) < hh) {
                    hi = m;
                } else {
                    lo = m;
                }
            }
            let (px, pz) = (o.x + d.x as f64 * hi, o.z + d.z as f64 * hi);
            return q.height_at(px, pz).map(|h| (px, pz, h));
        }
        t_prev = t;
        step = (step * 1.012).min(25.0);
    }
    None
}

fn invalidate(out: &mut MessageWriter<InvalidateRegion>, b: Bounds) {
    out.write(InvalidateRegion(b));
}

/// Apply the active terrain tool at a world point. Returns the dirty area.
fn apply_terrain(
    ed: &mut Editor,
    layer: &EditLayerRes,
    q: &TerrainQuery,
    x: f64,
    z: f64,
    y: f32,
    alt: bool,
) -> Option<Bounds> {
    let r = ed.radius as f64;
    let height = |px: f64, pz: f64| q.height_at(px, pz);
    Some(match (ed.tool, alt) {
        (Tool::Raise, false) | (Tool::Lower, true) => {
            layer.0.apply_brush(x, z, r, &BrushOp::Raise(0.7))
        }
        (Tool::Lower, false) | (Tool::Raise, true) => {
            layer.0.apply_brush(x, z, r, &BrushOp::Lower(0.7))
        }
        (Tool::Smooth, _) => layer.0.apply_brush(
            x,
            z,
            r,
            &BrushOp::Smooth {
                strength: 0.6,
                height_at: &height,
            },
        ),
        (Tool::Flatten, _) => {
            let target = *ed.flatten_target.get_or_insert(y);
            layer.0.flatten(x, z, r, target, 0.35, &height)
        }
        (Tool::Road, _) => return None,
    })
}

#[allow(clippy::too_many_arguments)]
pub fn editor_input(
    time: Res<Time<Real>>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut ed: ResMut<Editor>,
    layer: Res<EditLayerRes>,
    roads: Res<UserRoadsRes>,
    q: TerrainQuery,
    origin: Res<WorldOrigin>,
    window: Single<&Window, With<PrimaryWindow>>,
    cam: Single<(&Camera, &GlobalTransform, Option<&mut FreeCamera>)>,
    mut out: MessageWriter<InvalidateRegion>,
    mut gizmos: Gizmos,
) {
    let ctrl = keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight);
    let (camera, cgt, free) = cam.into_inner();
    if keys.just_pressed(KeyCode::KeyE) && !ctrl {
        ed.active = !ed.active;
        ed.status = if ed.active {
            "edit mode on".into()
        } else {
            "edit mode off".into()
        };
        if let Some(mut f) = free {
            // free the left mouse button for the brush while editing
            f.mouse_key_cursor_grab = if ed.active {
                MouseButton::Middle
            } else {
                MouseButton::Left
            };
        }
    }
    if !ed.active {
        return;
    }
    for (k, t) in [
        (KeyCode::KeyZ, Tool::Raise),
        (KeyCode::KeyX, Tool::Lower),
        (KeyCode::KeyC, Tool::Smooth),
        (KeyCode::KeyV, Tool::Flatten),
        (KeyCode::KeyB, Tool::Road),
    ] {
        if keys.just_pressed(k) && !ctrl {
            ed.tool = t;
            ed.flatten_target = None;
            ed.road_clicks.clear();
        }
    }
    if keys.just_pressed(KeyCode::Minus) {
        ed.radius = (ed.radius * 0.8).max(3.0);
    }
    if keys.just_pressed(KeyCode::Equal) {
        ed.radius = (ed.radius * 1.25).min(160.0);
    }
    // ---- undo / redo / save
    if ctrl
        && keys.just_pressed(KeyCode::KeyZ)
        && let Some(u) = ed.undo.pop()
    {
        match u {
            Undo::Terrain(snap) => {
                let cur = layer.0.snapshot();
                if let Some(b) = layer.0.restore(&snap) {
                    invalidate(&mut out, b);
                }
                ed.redo.push(Undo::Terrain(cur));
            }
            Undo::Road { clicks, class } => {
                if let Some(b) = roads.0.undo() {
                    invalidate(&mut out, b);
                }
                ed.redo.push(Undo::Road { clicks, class });
            }
        }
        ed.status = "undo".into();
    }
    if ctrl
        && keys.just_pressed(KeyCode::KeyY)
        && let Some(u) = ed.redo.pop()
    {
        match u {
            Undo::Terrain(snap) => {
                let cur = layer.0.snapshot();
                if let Some(b) = layer.0.restore(&snap) {
                    invalidate(&mut out, b);
                }
                ed.undo.push(Undo::Terrain(cur));
            }
            Undo::Road { clicks, class } => {
                if let Some(b) = roads.0.add(&clicks, class) {
                    invalidate(&mut out, b);
                }
                ed.undo.push(Undo::Road { clicks, class });
            }
        }
        ed.status = "redo".into();
    }
    if ctrl && keys.just_pressed(KeyCode::KeyS) {
        ed.status = match save_all(&ed.path, &layer, &roads) {
            Ok(()) => format!(
                "saved {} ({} edit blocks, {} roads)",
                ed.path,
                layer.0.block_count(),
                roads.0.len()
            ),
            Err(e) => format!("save failed: {e}"),
        };
    }
    // ---- cursor
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    let Ok(ray) = camera.viewport_to_world(cgt, cursor) else {
        return;
    };
    let Some((x, z, y)) = pick(&q, &origin, ray) else {
        return;
    };
    let render = origin.to_render(bevy::math::DVec3::new(x, y as f64, z));
    let col = if ed.tool == Tool::Road {
        Color::srgb(1.0, 0.8, 0.1)
    } else {
        Color::srgb(0.2, 0.9, 1.0)
    };
    gizmos.circle(
        Isometry3d::new(
            render + Vec3::Y * 0.4,
            Quat::from_rotation_arc(Vec3::Z, Vec3::Y),
        ),
        ed.radius,
        col,
    );
    // ---- tools
    if ed.tool == Tool::Road {
        if mouse.just_pressed(MouseButton::Left) {
            ed.road_clicks.push((x, z));
        }
        if keys.just_pressed(KeyCode::Backspace)
            && ed.road_clicks.pop().is_none()
            && let Some(b) = roads.0.undo()
        {
            invalidate(&mut out, b);
        }
        if keys.just_pressed(KeyCode::Escape) {
            ed.road_clicks.clear();
        }
        if keys.just_pressed(KeyCode::Enter) && ed.road_clicks.len() >= 2 {
            let clicks = std::mem::take(&mut ed.road_clicks);
            match roads.0.add(&clicks, RoadClass::Rural) {
                Some(b) => {
                    invalidate(&mut out, b);
                    ed.undo.push(Undo::Road {
                        clicks,
                        class: RoadClass::Rural,
                    });
                    ed.redo.clear();
                    ed.status = "road built".into();
                }
                None => ed.status = "road could not be built".into(),
            }
        }
        for w in ed.road_clicks.windows(2) {
            let (a, b) = (
                origin.to_render(bevy::math::DVec3::new(w[0].0, y as f64 + 2.0, w[0].1)),
                origin.to_render(bevy::math::DVec3::new(w[1].0, y as f64 + 2.0, w[1].1)),
            );
            gizmos.line(a, b, Color::srgb(1.0, 0.8, 0.1));
        }
        return;
    }
    let (down, alt) = (
        mouse.pressed(MouseButton::Left),
        mouse.pressed(MouseButton::Right),
    );
    if !(down || alt) {
        ed.stroke = false;
        ed.flatten_target = None;
        return;
    }
    if !ed.stroke {
        ed.undo.push(Undo::Terrain(layer.0.snapshot()));
        ed.redo.clear();
        ed.stroke = true;
    }
    ed.cooldown -= time.delta_secs();
    if ed.cooldown <= 0.0 {
        ed.cooldown = 0.1;
        if let Some(b) = apply_terrain(&mut ed, &layer, &q, x, z, y, alt) {
            invalidate(&mut out, b);
        }
    }
}

pub fn save_all(path: &str, layer: &EditLayerRes, roads: &UserRoadsRes) -> std::io::Result<()> {
    layer.0.save(&mut std::fs::File::create(path)?)?;
    roads
        .0
        .save(&mut std::fs::File::create(format!("{path}.roads"))?)
}

pub fn load_all(path: &str, layer: &EditLayerRes, roads: &UserRoadsRes) {
    if let Ok(mut f) = std::fs::File::open(path) {
        match layer.0.load(&mut f) {
            Ok(()) => println!("loaded {} edit blocks from {path}", layer.0.block_count()),
            Err(e) => eprintln!("could not load edits: {e}"),
        }
    }
    if let Ok(mut f) = std::fs::File::open(format!("{path}.roads")) {
        match roads.0.load(&mut f) {
            Ok(()) => println!("loaded {} user roads", roads.0.len()),
            Err(e) => eprintln!("could not load roads: {e}"),
        }
    }
}

/// Scripted editing session (`WG_EDIT_SCRIPT=1`): raise, flatten, smooth, road, undo/redo, save.
pub fn edit_script(
    time: Res<Time<Real>>,
    layer: Res<EditLayerRes>,
    roads: Res<UserRoadsRes>,
    q: TerrainQuery,
    origin: Res<WorldOrigin>,
    cam: Single<&Transform, With<Camera3d>>,
    mut ed: ResMut<Editor>,
    mut out: MessageWriter<InvalidateRegion>,
    mut step: Local<u32>,
    mut at: Local<Option<(f64, f64)>>,
    mut h_start: Local<f32>,
) {
    let t = time.elapsed_secs();
    let schedule = [3.0, 4.5, 6.0, 8.0, 10.0, 12.0, 14.0];
    if *step as usize >= schedule.len() || t < schedule[*step as usize] {
        return;
    }
    let f = cam.forward().as_vec3();
    let base = at.get_or_insert_with(|| {
        let p =
            origin.to_world(cam.translation + Vec3::new(f.x, 0.0, f.z).normalize_or_zero() * 90.0);
        (p.x, p.z)
    });
    let (bx, bz) = *base;
    let height = |px: f64, pz: f64| q.height_at(px, pz);
    let Some(h0) = q.height_at(bx, bz) else {
        return;
    };
    match *step {
        0 => {
            *h_start = h0;
            ed.undo.push(Undo::Terrain(layer.0.snapshot()));
            for k in 0..8 {
                invalidate(
                    &mut out,
                    layer
                        .0
                        .apply_brush(bx, bz, 30.0, &BrushOp::Raise(1.2 + k as f32 * 0.0)),
                );
            }
            ed.status = "script: raised a hill".into();
        }
        1 => {
            ed.undo.push(Undo::Terrain(layer.0.snapshot()));
            invalidate(
                &mut out,
                layer.0.flatten(bx + 70.0, bz, 28.0, h0 + 3.0, 0.9, &height),
            );
            ed.status = "script: flattened a pad".into();
        }
        2 => {
            ed.undo.push(Undo::Terrain(layer.0.snapshot()));
            for _ in 0..3 {
                invalidate(
                    &mut out,
                    layer.0.apply_brush(
                        bx,
                        bz,
                        40.0,
                        &BrushOp::Smooth {
                            strength: 0.6,
                            height_at: &height,
                        },
                    ),
                );
            }
            ed.status = "script: smoothed".into();
        }
        3 => {
            let clicks = [
                (bx - 150.0, bz + 40.0),
                (bx - 50.0, bz + 70.0),
                (bx + 60.0, bz + 55.0),
                (bx + 170.0, bz + 95.0),
            ];
            if let Some(b) = roads.0.add(&clicks, RoadClass::Rural) {
                invalidate(&mut out, b);
                ed.undo.push(Undo::Road {
                    clicks: clicks.to_vec(),
                    class: RoadClass::Rural,
                });
            }
            ed.status = "script: road built".into();
        }
        4 => {
            let saved = save_all(&ed.path, &layer, &roads);
            ed.status = format!(
                "script: saved ({:?}), {} blocks, {} roads",
                saved.is_ok(),
                layer.0.block_count(),
                roads.0.len()
            );
        }
        5 => {
            if let Some(Undo::Terrain(s)) =
                ed.undo.iter().rev().find(|u| matches!(u, Undo::Terrain(_)))
            {
                if let Some(b) = layer.0.restore(&s.clone()) {
                    invalidate(&mut out, b);
                }
                ed.status = "script: undo (restore) then redo".into();
            }
        }
        _ => {
            ed.status = "script: done".into();
            let road_y = q.surface_at(bx, bz + 62.0, 1.0e4);
            println!(
                "EDIT_SCRIPT done: {} blocks, {} roads, undo depth {}; rendered terrain at hill centre {:+.2} m vs start; surface near road {:?}",
                layer.0.block_count(),
                roads.0.len(),
                ed.undo.len(),
                h0 - *h_start,
                road_y
            );
        }
    }
    println!("EDIT_SCRIPT step {} at t={:.1}: {}", *step, t, ed.status);
    *step += 1;
}
