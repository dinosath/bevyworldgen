//! OpenStreetMap (Overpass JSON, `out geom`) -> drivable roads and building footprints in the
//! local frame. OSM data is © OpenStreetMap contributors, ODbL: it is fetched on demand and never
//! committed to this repository.

use crate::frame::LocalFrame;
use serde::Deserialize;
use worldgen_roads::RoadClass;

#[derive(Clone, Debug)]
pub struct OsmRoad {
    pub class: RoadClass,
    pub bridge: bool,
    pub pts: Vec<[f64; 2]>,
    pub id: u64,
}

#[derive(Clone, Debug)]
pub struct OsmBuilding {
    pub id: u64,
    pub pts: Vec<[f64; 2]>,
    pub levels: Option<f32>,
}

#[derive(Default, Debug)]
pub struct OsmData {
    pub roads: Vec<OsmRoad>,
    pub buildings: Vec<OsmBuilding>,
}

#[derive(Deserialize)]
struct Doc {
    elements: Vec<El>,
}

#[derive(Deserialize)]
struct El {
    #[serde(rename = "type")]
    ty: String,
    id: u64,
    #[serde(default)]
    tags: std::collections::HashMap<String, String>,
    #[serde(default)]
    geometry: Vec<Pt>,
}

#[derive(Deserialize)]
struct Pt {
    lat: f64,
    lon: f64,
}

fn road_class(tag: &str) -> Option<RoadClass> {
    match tag {
        "motorway" | "motorway_link" | "trunk" | "trunk_link" | "primary" | "primary_link" => {
            Some(RoadClass::Highway)
        }
        "secondary" | "secondary_link" | "tertiary" | "tertiary_link" => Some(RoadClass::Rural),
        "unclassified" | "residential" | "living_street" | "service" => Some(RoadClass::Street),
        _ => None,
    }
}

pub fn parse_overpass(json: &str, frame: &LocalFrame) -> Result<OsmData, String> {
    let doc: Doc = serde_json::from_str(json).map_err(|e| format!("bad Overpass JSON: {e}"))?;
    let mut out = OsmData::default();
    for e in doc
        .elements
        .iter()
        .filter(|e| e.ty == "way" && e.geometry.len() >= 2)
    {
        let pts: Vec<[f64; 2]> = e
            .geometry
            .iter()
            .map(|p| {
                let (x, z) = frame.to_local(p.lon, p.lat);
                [x, z]
            })
            .collect();
        if let Some(h) = e.tags.get("highway") {
            // tunnels cannot be represented in a heightfield; footpaths are not drivable
            if e.tags.get("tunnel").is_some_and(|t| t != "no") {
                continue;
            }
            if let Some(class) = road_class(h) {
                let bridge = e.tags.get("bridge").is_some_and(|b| b != "no");
                out.roads.push(OsmRoad {
                    class,
                    bridge,
                    pts,
                    id: e.id,
                });
            }
        } else if e.tags.contains_key("building") && pts.len() >= 4 {
            let levels = e.tags.get("building:levels").and_then(|l| l.parse().ok());
            out.buildings.push(OsmBuilding {
                id: e.id,
                pts,
                levels,
            });
        }
    }
    Ok(out)
}

/// Minimum-area bounding rectangle of a footprint: `(cx, cz, rot, w, d)` where the rectangle's
/// `w` axis points along `(cos rot, sin rot)` in the (x, z) plane.
pub fn footprint_rect(pts: &[[f64; 2]]) -> Option<(f64, f64, f32, f32, f32)> {
    if pts.len() < 3 {
        return None;
    }
    let mut best: Option<(f64, f64, f64, f64, f64, f64)> = None; // area, cx, cz, ang, w, d
    for k in 0..pts.len() - 1 {
        let (dx, dz) = (pts[k + 1][0] - pts[k][0], pts[k + 1][1] - pts[k][1]);
        let l = (dx * dx + dz * dz).sqrt();
        if l < 0.5 {
            continue;
        }
        let (ux, uz) = (dx / l, dz / l);
        let (mut lo_u, mut hi_u, mut lo_v, mut hi_v) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
        for p in pts {
            let (u, v) = (p[0] * ux + p[1] * uz, -p[0] * uz + p[1] * ux);
            lo_u = lo_u.min(u);
            hi_u = hi_u.max(u);
            lo_v = lo_v.min(v);
            hi_v = hi_v.max(v);
        }
        let (w, d) = (hi_u - lo_u, hi_v - lo_v);
        let area = w * d;
        if best.is_none_or(|b| area < b.0) {
            let (mu, mv) = ((lo_u + hi_u) * 0.5, (lo_v + hi_v) * 0.5);
            best = Some((
                area,
                mu * ux - mv * uz,
                mu * uz + mv * ux,
                uz.atan2(ux),
                w,
                d,
            ));
        }
    }
    best.map(|(_, cx, cz, a, w, d)| (cx, cz, a as f32, w as f32, d as f32))
}
