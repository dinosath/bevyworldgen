//! `geo-import <lon> <lat> <size_m> <out_dir> [zoom=14] [cell_m=4]`
//!
//! Writes `region.wgdem`, `osm.json` and `ATTRIBUTION.txt` into `out_dir`. Uses the system `curl`
//! (no TLS stack is linked into this workspace). Data is fetched on demand and must not be
//! committed: see the attribution file and `docs/research/license-inventory.md`.

use std::path::{Path, PathBuf};
use std::process::Command;
use worldgen_geo::{Dem, LocalFrame};

const UA: &str = "bevy-worldgen-importer/0.1 (research prototype)";
const OVERPASS: &str = "https://overpass-api.de/api/interpreter";

fn curl(args: &[&str]) -> Result<(), String> {
    let out = Command::new("curl")
        .args(["-sS", "-f", "-A", UA])
        .args(args)
        .output()
        .map_err(|e| format!("curl: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

fn fetch_tile(cache: &Path, z: u32, x: u32, y: u32) -> Option<Vec<u8>> {
    let path = cache.join(format!("{z}_{x}_{y}.png"));
    if !path.exists() {
        let url =
            format!("https://s3.amazonaws.com/elevation-tiles-prod/terrarium/{z}/{x}/{y}.png");
        for attempt in 0..3 {
            match curl(&["-m", "60", "-o", path.to_str()?, &url]) {
                Ok(()) => break,
                Err(e) => {
                    eprintln!("tile {z}/{x}/{y} attempt {attempt}: {e}");
                    std::thread::sleep(std::time::Duration::from_secs(2));
                }
            }
        }
    }
    std::fs::read(path).ok()
}

fn overpass(bbox: (f64, f64, f64, f64)) -> Result<serde_json::Value, String> {
    let q = format!(
        "[out:json][timeout:90];(way[\"highway\"]({s},{w},{n},{e});way[\"building\"]({s},{w},{n},{e}););out geom;",
        s = bbox.0,
        w = bbox.1,
        n = bbox.2,
        e = bbox.3
    );
    let tmp = std::env::temp_dir().join(format!("wg-overpass-{}.json", std::process::id()));
    let mut last = String::new();
    for attempt in 0..4 {
        match curl(&[
            "-m",
            "150",
            "-H",
            "Accept: */*",
            "--data-urlencode",
            &format!("data={q}"),
            "-o",
            tmp.to_str().unwrap(),
            OVERPASS,
        ]) {
            Ok(()) => {
                let text = std::fs::read_to_string(&tmp).map_err(|e| e.to_string())?;
                if let Ok(v) = serde_json::from_str(&text) {
                    let _ = std::fs::remove_file(&tmp);
                    return Ok(v);
                }
                last = "response was not JSON".into();
            }
            Err(e) => last = e,
        }
        eprintln!("overpass attempt {attempt} failed: {last}");
        std::thread::sleep(std::time::Duration::from_secs(6 * (attempt + 1)));
    }
    Err(last)
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 5 {
        eprintln!("usage: geo-import <lon> <lat> <size_m> <out_dir> [zoom=14] [cell_m=4]");
        std::process::exit(2);
    }
    let (lon, lat, size): (f64, f64, f64) = (
        a[1].parse().unwrap(),
        a[2].parse().unwrap(),
        a[3].parse().unwrap(),
    );
    let out = PathBuf::from(&a[4]);
    let zoom: u32 = a.get(5).and_then(|s| s.parse().ok()).unwrap_or(14);
    let cell: f64 = a.get(6).and_then(|s| s.parse().ok()).unwrap_or(4.0);
    std::fs::create_dir_all(out.join("tiles")).unwrap();
    let frame = LocalFrame::new(lon, lat);

    // --- DEM
    let cache = out.join("tiles");
    let mut fetch = |z: u32, x: u32, y: u32| fetch_tile(&cache, z, x, y);
    let dem = Dem::from_terrarium(&frame, size, cell, zoom, &mut fetch).expect("build DEM");
    let (lo, hi) = dem
        .data
        .iter()
        .fold((f32::MAX, f32::MIN), |(l, h), v| (l.min(*v), h.max(*v)));
    dem.save(&mut std::fs::File::create(out.join("region.wgdem")).unwrap())
        .unwrap();
    println!(
        "DEM {}x{} @ {} m, elevation {:.0}..{:.0} m",
        dem.w, dem.h, cell, lo, hi
    );

    // --- OSM, in quadrants to stay within Overpass limits
    let half = size * 0.5;
    let mut elements: Vec<serde_json::Value> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for (qx, qz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
        let (x0, x1) = (-half + qx as f64 * half, -half + (qx + 1) as f64 * half);
        let (z0, z1) = (-half + qz as f64 * half, -half + (qz + 1) as f64 * half);
        let (lon_a, lat_a) = frame.from_local(x0, z1); // south-west
        let (lon_b, lat_b) = frame.from_local(x1, z0); // north-east
        let (s, n) = (lat_a.min(lat_b), lat_a.max(lat_b));
        let (w, e) = (lon_a.min(lon_b), lon_a.max(lon_b));
        match overpass((s, w, n, e)) {
            Ok(v) => {
                for el in v["elements"].as_array().cloned().unwrap_or_default() {
                    if seen.insert(el["id"].as_u64().unwrap_or(0)) {
                        elements.push(el);
                    }
                }
            }
            Err(e) => {
                eprintln!("quadrant ({qx},{qz}) failed, region will lack OSM data there: {e}")
            }
        }
    }
    let roads = elements
        .iter()
        .filter(|e| e["tags"]["highway"].is_string())
        .count();
    let buildings = elements
        .iter()
        .filter(|e| e["tags"]["building"].is_string())
        .count();
    std::fs::write(
        out.join("osm.json"),
        serde_json::json!({ "elements": elements }).to_string(),
    )
    .unwrap();
    println!("OSM: {roads} highways, {buildings} buildings");

    std::fs::write(
        out.join("ATTRIBUTION.txt"),
        "Elevation: AWS Open Data \"Terrain Tiles\" (Terrarium), a composite of SRTM, GMTED, NED and other sources;\nsee https://github.com/tilezen/joerd/blob/master/docs/attribution.md\n\nRoads and buildings: (c) OpenStreetMap contributors, ODbL 1.0 - https://www.openstreetmap.org/copyright\nThis directory is a derived database: do not redistribute it under other terms, and do not commit it.\n",
    )
    .unwrap();
    println!("wrote {}", out.display());
}
