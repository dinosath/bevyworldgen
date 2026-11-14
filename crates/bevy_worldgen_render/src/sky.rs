//! Time of day: sun and moon motion, exposure, light colour. The sky itself is Bevy's built-in
//! atmosphere.

use bevy::camera::Exposure;
use bevy::light::{DirectionalLight, light_consts::lux};
use bevy::prelude::*;

#[derive(Resource, Clone, Copy, Debug)]
pub struct TimeOfDay {
    /// 0..24.
    pub hour: f32,
    /// Hours per real second (0 = frozen). 0.01 = a day in 40 minutes.
    pub speed: f32,
}

impl Default for TimeOfDay {
    fn default() -> Self {
        Self {
            hour: 15.5,
            speed: 0.0,
        }
    }
}

/// Weak blue directional light that takes over at night.
#[derive(Component)]
pub struct Moon;

/// Derived sky state other systems (clouds, windows, lamps) read.
#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct SkyState {
    /// Unit vector toward the sun.
    pub sun_dir: Vec3,
    /// sin(elevation): >0 day.
    pub elevation: f32,
    /// 0 day .. 1 night.
    pub night: f32,
    /// 0..1 how orange the light is (low sun).
    pub dusk: f32,
}

fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub(crate) fn update_sky(
    time: Res<Time<Real>>,
    mut tod: ResMut<TimeOfDay>,
    weather: Option<Res<crate::weather::WeatherState>>,
    mut sky: ResMut<SkyState>,
    mut sun: Query<(&mut Transform, &mut DirectionalLight), Without<Moon>>,
    mut moon: Query<(&mut Transform, &mut DirectionalLight), With<Moon>>,
    mut exposure: Query<&mut Exposure>,
    mats: Option<Res<crate::WorldMaterials>>,
    mut std_mats: ResMut<Assets<StandardMaterial>>,
) {
    tod.hour = (tod.hour + tod.speed * time.delta_secs()).rem_euclid(24.0);
    let e = std::f32::consts::PI * (tod.hour - 6.0) / 12.0;
    let p = Vec3::new(e.cos(), e.sin(), 0.32).normalize();
    let elev = p.y;
    let night = smooth(0.06, -0.20, elev);
    let dusk = smooth(0.35, 0.0, elev) * (1.0 - night);
    *sky = SkyState {
        sun_dir: p,
        elevation: elev,
        night,
        dusk,
    };
    let overcast = weather
        .as_ref()
        .map_or(0.0, |w| w.cloud * 0.55 + w.rain * 0.25);
    for (mut t, mut l) in &mut sun {
        *t = Transform::IDENTITY.looking_to(-p, Vec3::Y);
        l.illuminance = lux::RAW_SUNLIGHT * smooth(-0.04, 0.10, elev) * (1.0 - overcast);
        l.color = Color::srgb(1.0, 1.0 - 0.28 * dusk, 1.0 - 0.6 * dusk);
    }
    for (mut t, mut l) in &mut moon {
        *t = Transform::IDENTITY.looking_to(p, Vec3::Y);
        // a full moon is ~0.25 lux; boosted so the night is playable
        l.illuminance = 2.2 * night * (1.0 - overcast * 0.6);
    }
    if let Some(m) = mats
        && let Some(mut g) = std_mats.get_mut(&m.glass_lit)
    {
        let k = 1.0 * smooth(0.25, 0.95, night);
        g.emissive = LinearRgba::rgb(1.0 * k, 0.68 * k, 0.32 * k);
    }
    // physically based: sunlight needs EV100 ~13, moonlight ~ -1
    let ev = 13.0 - 3.5 * smooth(0.30, 0.0, elev) - 11.3 * night;
    for mut x in &mut exposure {
        x.ev100 += (ev - x.ev100) * 0.1;
    }
}
