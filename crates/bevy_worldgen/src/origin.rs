//! Floating origin by recentring: world positions are `f64`; entities carry `f32` offsets from
//! [`WorldOrigin`]. When the focus drifts too far, the origin jumps and every anchored entity and
//! chunk root is shifted by the same amount, so nothing visibly moves.
//! (`big_space` was evaluated and is *not* used: it is a release candidate for Bevy 0.20 and the
//! single-threshold recentring is sufficient for a streamed world; see ADR-001 §4.)

use crate::stream::{ChunkRoot, StreamFocus};
use bevy::math::DVec3;
use bevy::prelude::*;

/// World position (metres, `f64`) of the render-space origin.
#[derive(Resource, Default, Debug, Clone, Copy)]
pub struct WorldOrigin(pub DVec3);

impl WorldOrigin {
    pub fn to_render(&self, world: DVec3) -> Vec3 {
        (world - self.0).as_vec3()
    }
    pub fn to_world(&self, render: Vec3) -> DVec3 {
        self.0 + render.as_dvec3()
    }
}

/// Entities whose `Transform` must follow origin shifts (cameras, vehicles, props).
#[derive(Component, Default)]
pub struct WorldAnchored;

pub const REBASE_DISTANCE: f32 = 2500.0;

pub(crate) fn rebase_origin(
    mut origin: ResMut<WorldOrigin>,
    mut anchored: Query<
        (&mut Transform, Has<StreamFocus>),
        (With<WorldAnchored>, Without<ChunkRoot>),
    >,
    mut roots: Query<&mut Transform, With<ChunkRoot>>,
) {
    let Some(focus) = anchored
        .iter()
        .find(|(_, f)| *f)
        .map(|(t, _)| t.translation)
    else {
        return;
    };
    let d = Vec3::new(focus.x, 0.0, focus.z);
    if d.length() < REBASE_DISTANCE {
        return;
    }
    // snap to a coarse grid so repeated shifts stay exact in f32
    let shift = Vec3::new(
        (d.x / 1024.0).round() * 1024.0,
        0.0,
        (d.z / 1024.0).round() * 1024.0,
    );
    origin.0 += shift.as_dvec3();
    for (mut tr, _) in &mut anchored {
        tr.translation -= shift;
    }
    for mut tr in &mut roots {
        tr.translation -= shift;
    }
}
