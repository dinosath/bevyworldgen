//! Local metric frame around a real-world point: x east, z south (matching the world's axes).

use proj4rs::Proj;
use proj4rs::transform::transform;

pub struct LocalFrame {
    wgs: Proj,
    utm: Proj,
    e0: f64,
    n0: f64,
    pub lon0: f64,
    pub lat0: f64,
}

impl LocalFrame {
    pub fn new(lon0: f64, lat0: f64) -> Self {
        let zone = (((lon0 + 180.0) / 6.0).floor() as i32 + 1).clamp(1, 60);
        let south = if lat0 < 0.0 { " +south" } else { "" };
        let utm = Proj::from_proj_string(&format!(
            "+proj=utm +zone={zone}{south} +datum=WGS84 +units=m +no_defs"
        ))
        .expect("utm");
        let wgs = Proj::from_proj_string("+proj=longlat +datum=WGS84 +no_defs").expect("wgs84");
        let mut p = (lon0.to_radians(), lat0.to_radians());
        transform(&wgs, &utm, &mut p).expect("project origin");
        Self {
            wgs,
            utm,
            e0: p.0,
            n0: p.1,
            lon0,
            lat0,
        }
    }

    /// (lon, lat) in degrees -> (x east, z south) metres from the frame origin.
    pub fn to_local(&self, lon: f64, lat: f64) -> (f64, f64) {
        let mut p = (lon.to_radians(), lat.to_radians());
        transform(&self.wgs, &self.utm, &mut p).expect("project");
        (p.0 - self.e0, -(p.1 - self.n0))
    }

    /// (x east, z south) metres -> (lon, lat) degrees.
    pub fn from_local(&self, x: f64, z: f64) -> (f64, f64) {
        let mut p = (self.e0 + x, self.n0 - z);
        transform(&self.utm, &self.wgs, &mut p).expect("unproject");
        (p.0.to_degrees(), p.1.to_degrees())
    }
}

/// Slippy-map (web mercator) pixel coordinates at `zoom` for a lon/lat, 256-px tiles.
pub fn mercator_pixel(lon: f64, lat: f64, zoom: u32) -> (f64, f64) {
    let n = (1u64 << zoom) as f64 * 256.0;
    let lat_r = lat.to_radians();
    let x = (lon + 180.0) / 360.0 * n;
    let y = (1.0 - (lat_r.tan() + 1.0 / lat_r.cos()).ln() / std::f64::consts::PI) / 2.0 * n;
    (x, y)
}
