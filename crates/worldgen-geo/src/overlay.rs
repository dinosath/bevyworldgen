//! Puts real-world data into the streamed world: the DEM replaces procedural terrain inside the
//! region (feathered at the edge), OSM roads get DEM-following profiles with bridges, and OSM
//! building footprints become lots for the CGA grammars.

use crate::dem::Dem;
use crate::frame::LocalFrame;
use crate::osm::{OsmData, footprint_rect, parse_overpass};
use std::path::Path;
use std::sync::Arc;
use worldgen_core::{Fingerprint, Subsystem, derive_seed};
use worldgen_roads::{Lot, Pad, Road, RoadOverlay};
use worldgen_terrain::{Bounds, HeightModifier, RegionModifier};

/// Outside the region the DEM's edge elevation is extended and eased into the procedural terrain
/// over this distance, so the region does not end in a cliff.
const RAMP_M: f64 = 3000.0;
const MAX_GRADE: f32 = 0.12;

pub struct GeoOverlay {
    pub dem: Arc<Dem>,
    pub roads: Vec<Arc<Road>>,
    pub lots: Vec<Lot>,
    pub pads: Vec<Pad>,
    pub lat0: f64,
    pub lon0: f64,
}

fn smooth(y: &[f32], half: usize) -> Vec<f32> {
    (0..y.len())
        .map(|i| {
            let (a, b) = (i.saturating_sub(half), (i + half).min(y.len() - 1));
            y[a..=b].iter().sum::<f32>() / (b - a + 1) as f32
        })
        .collect()
}

fn resample(p: &[[f64; 2]], step: f64) -> Vec<[f64; 2]> {
    let mut out = vec![p[0]];
    for w in p.windows(2) {
        let (dx, dz) = (w[1][0] - w[0][0], w[1][1] - w[0][1]);
        let len = (dx * dx + dz * dz).sqrt();
        let n = (len / step).ceil().max(1.0) as usize;
        for k in 1..=n {
            let t = k as f64 / n as f64;
            out.push([w[0][0] + dx * t, w[0][1] + dz * t]);
        }
    }
    out
}

/// Vertical profile of an OSM road over the DEM: smoothed terrain with a grade limit; bridges
/// interpolate linearly between their ends so the deck spans valleys.
fn road_from_osm(
    dem: &Dem,
    id: u64,
    class: worldgen_roads::RoadClass,
    bridge: bool,
    pts: &[[f64; 2]],
) -> Option<Road> {
    if pts.len() < 2 {
        return None;
    }
    let pts = resample(pts, 6.0);
    let n = pts.len();
    let mut y: Vec<f32> = pts.iter().map(|p| dem.sample(p[0], p[1])).collect();
    if bridge && n > 2 {
        let (a, b) = (y[0], y[n - 1]);
        for (i, v) in y.iter_mut().enumerate() {
            *v = a + (b - a) * (i as f32 / (n - 1) as f32);
        }
    } else {
        y = smooth(&smooth(&y, 2), 2);
    }
    let seg: Vec<f32> = pts
        .windows(2)
        .map(|w| ((w[1][0] - w[0][0]).powi(2) + (w[1][1] - w[0][1]).powi(2)).sqrt() as f32)
        .collect();
    for _ in 0..3 {
        for i in 1..n {
            let m = MAX_GRADE * seg[i - 1];
            y[i] = y[i].clamp(y[i - 1] - m, y[i - 1] + m);
        }
        for i in (0..n - 1).rev() {
            let m = MAX_GRADE * seg[i];
            y[i] = y[i].clamp(y[i + 1] - m, y[i + 1] + m);
        }
    }
    Some(Road {
        id: id | (1 << 62),
        class,
        pts,
        y,
        bridge: vec![bridge; n - 1],
    })
}

impl GeoOverlay {
    pub fn build(dem: Dem, osm: &OsmData, seed: u64) -> Self {
        let roads: Vec<Arc<Road>> = osm
            .roads
            .iter()
            .filter_map(|r| road_from_osm(&dem, r.id, r.class, r.bridge, &r.pts))
            .map(Arc::new)
            .collect();
        let mut lots = Vec::new();
        let mut pads = Vec::new();
        for b in &osm.buildings {
            let Some((cx, cz, rot, w, d)) = footprint_rect(&b.pts) else {
                continue;
            };
            if w * d < 14.0 || w * d > 3500.0 || w.min(d) < 2.5 {
                continue;
            }
            let (w, d) = (w.clamp(4.0, 70.0), d.clamp(4.0, 70.0));
            let y = dem.sample(cx, cz);
            lots.push(Lot {
                cx,
                cz,
                rot,
                w,
                d,
                y,
                seed: derive_seed(seed, Subsystem::Buildings, b.id as i64, 0, 99),
            });
            pads.push(Pad {
                cx,
                cz,
                rot,
                half_w: w * 0.5,
                half_d: d * 0.5,
                y,
            });
        }
        let (lon0, lat0) = (dem.lon0, dem.lat0);
        Self {
            dem: Arc::new(dem),
            roads,
            lots,
            pads,
            lat0,
            lon0,
        }
    }

    /// Load `region.wgdem` + `osm.json` written by the importer.
    pub fn load_dir(dir: &Path, seed: u64) -> Result<Self, String> {
        let mut f = std::fs::File::open(dir.join("region.wgdem"))
            .map_err(|e| format!("region.wgdem: {e}"))?;
        let dem = Dem::load(&mut f).map_err(|e| e.to_string())?;
        let frame = LocalFrame::new(dem.lon0, dem.lat0);
        let json =
            std::fs::read_to_string(dir.join("osm.json")).map_err(|e| format!("osm.json: {e}"))?;
        let osm = parse_overpass(&json, &frame)?;
        Ok(Self::build(dem, &osm, seed))
    }

    pub fn bounds(&self) -> Bounds {
        let (a, b, c, d) = self.dem.bounds();
        Bounds {
            min_x: a,
            min_z: b,
            max_x: c,
            max_z: d,
        }
    }

    pub fn fingerprint(&self) -> u64 {
        let mut f = Fingerprint::new();
        f.f32s(&self.dem.data);
        for r in &self.roads {
            f.u64(r.id);
            f.f32s(&r.y);
        }
        f.finish()
    }
}

struct GeoMod {
    dem: Arc<Dem>,
    bounds: (f64, f64, f64, f64),
}

impl GeoMod {
    /// 1 inside the region; outside, 1 at the border easing to 0 over [`RAMP_M`].
    fn w(&self, x: f64, z: f64) -> f64 {
        let (x0, z0, x1, z1) = self.bounds;
        let dx = (x0 - x).max(0.0).max(x - x1);
        let dz = (z0 - z).max(0.0).max(z - z1);
        let d = (dx * dx + dz * dz).sqrt();
        let t = (d / RAMP_M).clamp(0.0, 1.0);
        1.0 - t * t * (3.0 - 2.0 * t)
    }
}

impl RegionModifier for GeoMod {
    fn apply(&self, x: f64, z: f64, h: f32) -> f32 {
        let w = self.w(x, z);
        if w <= 0.0 {
            h
        } else {
            h + (self.dem.sample(x, z) - h) * w as f32
        }
    }
    fn replaces_terrain(&self, x: f64, z: f64) -> bool {
        self.w(x, z) > 0.5
    }
}

impl HeightModifier for GeoOverlay {
    /// Region data is not part of the cache key, so terrain it touches bypasses the disk cache.
    fn stateful(&self) -> bool {
        true
    }

    fn prepare(&self, b: &Bounds, spacing: f64) -> Option<Box<dyn RegionModifier>> {
        let m = RAMP_M + spacing * 4.0 + 8.0;
        let r = self.bounds();
        if b.max_x + m < r.min_x
            || b.min_x - m > r.max_x
            || b.max_z + m < r.min_z
            || b.min_z - m > r.max_z
        {
            return None;
        }
        Some(Box::new(GeoMod {
            dem: self.dem.clone(),
            bounds: self.dem.bounds(),
        }))
    }
}

impl RoadOverlay for GeoOverlay {
    fn stateful(&self) -> bool {
        true
    }

    fn roads_near(&self, b: &Bounds) -> (Vec<Arc<Road>>, Vec<Pad>) {
        let roads = self
            .roads
            .iter()
            .filter(|r| {
                let (lo, hi) = r.bbox();
                hi[0] >= b.min_x - 60.0
                    && lo[0] <= b.max_x + 60.0
                    && hi[1] >= b.min_z - 60.0
                    && lo[1] <= b.max_z + 60.0
            })
            .cloned()
            .collect();
        let pads = self
            .pads
            .iter()
            .filter(|p| {
                p.cx >= b.min_x - 80.0
                    && p.cx <= b.max_x + 80.0
                    && p.cz >= b.min_z - 80.0
                    && p.cz <= b.max_z + 80.0
            })
            .copied()
            .collect();
        (roads, pads)
    }

    fn lots_near(&self, b: &Bounds) -> Vec<Lot> {
        self.lots
            .iter()
            .filter(|l| l.cx >= b.min_x && l.cx < b.max_x && l.cz >= b.min_z && l.cz < b.max_z)
            .copied()
            .collect()
    }
}
