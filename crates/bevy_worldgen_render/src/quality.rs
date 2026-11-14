//! Quality presets: one resource drives view distance, shadows, post effects, atmosphere LUT
//! sizes and grass reach. Change `Quality` at runtime (the showcase binds F1-F4).

use bevy::light::{CascadeShadowConfigBuilder, DirectionalLight, GlobalAmbientLight};
use bevy::pbr::AtmosphereSettings;
use bevy::post_process::bloom::Bloom;
use bevy::prelude::*;
use bevy_grass::prelude::{GrassQuality, GrassSettings};
use bevy_worldgen::StreamConfig;

#[derive(Resource, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Quality {
    Low,
    Medium,
    High,
    Ultra,
}

impl Quality {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "low" => Some(Self::Low),
            "medium" | "med" => Some(Self::Medium),
            "high" => Some(Self::High),
            "ultra" => Some(Self::Ultra),
            _ => None,
        }
    }

    /// Quadtree split distance as a multiple of chunk size (view distance / LOD pressure).
    pub fn split_factor(self) -> f64 {
        match self {
            Self::Low => 1.25,
            Self::Medium => 1.6,
            Self::High => 2.0,
            Self::Ultra => 2.6,
        }
    }

    fn grass(self) -> GrassQuality {
        match self {
            Self::Low => GrassQuality::Competitive,
            Self::Medium => GrassQuality::Low,
            Self::High => GrassQuality::Medium,
            Self::Ultra => GrassQuality::High,
        }
    }

    /// (cascades, max distance, first cascade bound); `None` = shadows off.
    fn shadows(self) -> Option<(usize, f32, f32)> {
        match self {
            Self::Low => None,
            Self::Medium => Some((2, 260.0, 30.0)),
            Self::High => Some((3, 800.0, 25.0)),
            Self::Ultra => Some((4, 1500.0, 20.0)),
        }
    }

    fn lut_scale(self) -> f32 {
        match self {
            Self::Low => 0.5,
            Self::Medium => 0.75,
            _ => 1.0,
        }
    }

    fn bloom(self) -> bool {
        matches!(self, Self::High | Self::Ultra)
    }

    fn env_light(self) -> bool {
        !matches!(self, Self::Low)
    }
}

pub(crate) fn apply_quality(
    mut commands: Commands,
    q: Res<Quality>,
    mut stream: Option<ResMut<StreamConfig>>,
    mut grass: Option<ResMut<GrassSettings>>,
    mut lights: Query<(Entity, &mut DirectionalLight), Without<crate::sky::Moon>>,
    cams: Query<Entity, With<Camera3d>>,
    mut atmos: Query<&mut AtmosphereSettings>,
    mut ambient: ResMut<GlobalAmbientLight>,
    mut applied: Local<Option<Quality>>,
) {
    if *applied == Some(*q) && !q.is_changed() {
        return;
    }
    // wait until the scene exists
    if cams.is_empty() || lights.is_empty() {
        return;
    }
    *applied = Some(*q);
    if let Some(s) = stream.as_mut() {
        s.params.split_factor = q.split_factor();
    }
    if let Some(g) = grass.as_mut() {
        **g = GrassSettings::from(q.grass());
    }
    for (e, mut l) in &mut lights {
        match q.shadows() {
            None => l.shadow_maps_enabled = false,
            Some((n, dist, first)) => {
                l.shadow_maps_enabled = true;
                commands.entity(e).insert(
                    CascadeShadowConfigBuilder {
                        num_cascades: n,
                        maximum_distance: dist,
                        first_cascade_far_bound: first,
                        ..default()
                    }
                    .build(),
                );
            }
        }
    }
    for e in &cams {
        if q.bloom() {
            commands.entity(e).insert(Bloom::NATURAL);
        } else {
            commands.entity(e).remove::<Bloom>();
        }
        if q.env_light() {
            commands
                .entity(e)
                .insert(bevy::light::AtmosphereEnvironmentMapLight::default());
        } else {
            commands
                .entity(e)
                .remove::<bevy::light::AtmosphereEnvironmentMapLight>();
        }
    }
    let k = q.lut_scale();
    let d = AtmosphereSettings::default();
    for mut a in &mut atmos {
        let sc = |v: UVec2| {
            UVec2::new(
                ((v.x as f32 * k) as u32).max(16),
                ((v.y as f32 * k) as u32).max(16),
            )
        };
        a.transmittance_lut_size = sc(d.transmittance_lut_size);
        a.multiscattering_lut_size = sc(d.multiscattering_lut_size).max(UVec2::splat(8));
        a.sky_view_lut_size = sc(d.sky_view_lut_size);
        a.aerial_view_lut_size = UVec3::new(
            ((d.aerial_view_lut_size.x as f32 * k) as u32).max(8),
            ((d.aerial_view_lut_size.y as f32 * k) as u32).max(8),
            d.aerial_view_lut_size.z,
        );
    }
    // without the atmosphere environment map a constant sky-ish ambient keeps shadows readable
    ambient.brightness = if q.env_light() { 0.0 } else { 900.0 };
    ambient.color = Color::srgb(0.62, 0.72, 0.9);
}
