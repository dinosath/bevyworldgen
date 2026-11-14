use std::sync::Arc;
use worldgen_geo::*;
use worldgen_roads::{RoadClass, RoadOverlay};
use worldgen_terrain::Bounds;

#[test]
fn local_frame_round_trips_and_has_metric_scale() {
    let f = LocalFrame::new(7.9087, 46.5934); // Lauterbrunnen
    let (x, z) = f.to_local(7.9087, 46.5934);
    assert!(x.abs() < 1e-6 && z.abs() < 1e-6);
    for (dx, dz) in [(1000.0, -2000.0), (-3500.0, 4200.0), (12.5, 0.5)] {
        let (lon, lat) = f.from_local(dx, dz);
        let (x2, z2) = f.to_local(lon, lat);
        assert!(
            (x2 - dx).abs() < 1e-5 && (z2 - dz).abs() < 1e-5,
            "round trip off by {} {}",
            x2 - dx,
            z2 - dz
        );
    }
    // 1 km south of the origin is ~0.009 deg lower latitude; 1 km east ~0.013 deg at 46.6 N
    let (lon, lat) = f.from_local(0.0, 1000.0);
    assert!((f.lat0 - lat - 0.00899).abs() < 3e-4, "lat {lat}");
    // grid south is not true south away from the UTM central meridian (convergence ~0.8 deg here)
    assert!((lon - f.lon0).abs() < 4e-4, "lon drift {}", lon - f.lon0);
    let (lon, _) = f.from_local(1000.0, 0.0);
    assert!((lon - f.lon0 - 0.0131).abs() < 6e-4, "lon {lon}");
}

fn encode(elev: impl Fn(u32, u32) -> f32) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(256 * 256 * 3);
    for y in 0..256 {
        for x in 0..256 {
            let v = elev(x, y) + 32768.0;
            rgb.extend_from_slice(&[
                (v / 256.0).floor() as u8,
                (v.floor() % 256.0) as u8,
                ((v - v.floor()) * 256.0) as u8,
            ]);
        }
    }
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, 256, 256);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header().unwrap().write_image_data(&rgb).unwrap();
    }
    out
}

#[test]
fn terrarium_decode_and_dem_resampling_use_the_right_tiles() {
    let png = encode(|x, y| 500.0 + x as f32 * 0.5 + y as f32 * 0.25);
    let dec = decode_terrarium(&png).expect("decode");
    assert!(
        (dec[0] - 500.0).abs() < 0.01
            && (dec[255] - (500.0 + 127.5)).abs() < 0.02
            && (dec[255 * 256] - (500.0 + 63.75)).abs() < 0.02
    );
    assert!(
        decode_terrarium(&png[..40]).is_none(),
        "truncated PNG must be rejected"
    );

    // every tile encodes a plane in its *own* pixel coordinates, so the DEM value at a point depends on
    // which tile/pixel the resampler picked; compare against an independent computation
    let frame = LocalFrame::new(7.9087, 46.5934);
    let mut requested = Vec::new();
    let mut fetch = |z: u32, x: u32, y: u32| {
        requested.push((z, x, y));
        Some(png.clone())
    };
    let dem = Dem::from_terrarium(&frame, 800.0, 8.0, 14, &mut fetch).expect("dem");
    assert_eq!((dem.w, dem.h), (101, 101));
    let (px, py) = mercator_pixel(frame.lon0, frame.lat0, 14);
    let centre_tile = ((px / 256.0).floor() as u32, (py / 256.0).floor() as u32);
    assert!(
        requested.contains(&(14, centre_tile.0, centre_tile.1)),
        "centre tile {centre_tile:?} must be fetched: {requested:?}"
    );
    let want = 500.0
        + ((px - 0.5).rem_euclid(256.0)) as f32 * 0.5
        + ((py - 0.5).rem_euclid(256.0)) as f32 * 0.25;
    let got = dem.sample(0.0, 0.0);
    assert!(
        (got - want).abs() < 1.5,
        "centre elevation {got} vs expected {want}"
    );
    // save/load round trip
    let mut buf = vec![];
    dem.save(&mut buf).unwrap();
    let back = Dem::load(&mut buf.as_slice()).unwrap();
    assert_eq!(back.data, dem.data);
    assert!(Dem::load(&mut &b"nonsense_______"[..]).is_err());
}

const OSM: &str = r#"{"elements":[
 {"type":"way","id":11,"tags":{"highway":"primary","bridge":"yes"},"geometry":[{"lat":46.5934,"lon":7.9060},{"lat":46.5934,"lon":7.9087},{"lat":46.5934,"lon":7.9114}]},
 {"type":"way","id":12,"tags":{"highway":"residential"},"geometry":[{"lat":46.5930,"lon":7.9070},{"lat":46.5936,"lon":7.9072}]},
 {"type":"way","id":13,"tags":{"highway":"footway"},"geometry":[{"lat":46.5930,"lon":7.9070},{"lat":46.5936,"lon":7.9072}]},
 {"type":"way","id":14,"tags":{"highway":"primary","tunnel":"yes"},"geometry":[{"lat":46.5940,"lon":7.9070},{"lat":46.5940,"lon":7.9080}]},
 {"type":"way","id":21,"tags":{"building":"yes","building:levels":"3"},"geometry":[{"lat":46.5938,"lon":7.9090},{"lat":46.5938,"lon":7.90915},{"lat":46.59365,"lon":7.90915},{"lat":46.59365,"lon":7.9090},{"lat":46.5938,"lon":7.9090}]},
 {"type":"node","id":99,"lat":46.59,"lon":7.9}
]}"#;

#[test]
fn osm_parsing_classifies_roads_bridges_and_buildings() {
    let f = LocalFrame::new(7.9087, 46.5934);
    let d = parse_overpass(OSM, &f).unwrap();
    assert_eq!(
        d.roads.len(),
        2,
        "primary + residential only (footway skipped, tunnel skipped)"
    );
    let primary = d.roads.iter().find(|r| r.id == 11).unwrap();
    assert_eq!(primary.class, RoadClass::Highway);
    assert!(primary.bridge);
    assert_eq!(
        d.roads.iter().find(|r| r.id == 12).unwrap().class,
        RoadClass::Street
    );
    assert_eq!(d.buildings.len(), 1);
    assert_eq!(d.buildings[0].levels, Some(3.0));
    assert!(parse_overpass("not json", &f).is_err());
}

#[test]
fn footprint_rectangle_recovers_size_and_orientation() {
    // 12 x 8 m rectangle rotated by 30 degrees, centred at (100, -50)
    let (c, s) = (30f64.to_radians().cos(), 30f64.to_radians().sin());
    let p = |u: f64, v: f64| [100.0 + u * c - v * s, -50.0 + u * s + v * c];
    let pts = vec![
        p(-6.0, -4.0),
        p(6.0, -4.0),
        p(6.0, 4.0),
        p(-6.0, 4.0),
        p(-6.0, -4.0),
    ];
    let (cx, cz, rot, w, d) = footprint_rect(&pts).unwrap();
    assert!((cx - 100.0).abs() < 1e-6 && (cz + 50.0).abs() < 1e-6);
    let (long, short) = (w.max(d), w.min(d));
    assert!(
        (long - 12.0).abs() < 1e-3 && (short - 8.0).abs() < 1e-3,
        "{w} x {d}"
    );
    // axis direction is 30 deg or 120 deg depending on which side is `w`
    let deg = (rot.to_degrees() % 90.0 + 90.0) % 90.0;
    assert!((deg - 30.0).abs() < 0.01, "rotation {deg}");
}

fn flat_dem(h: f32) -> Dem {
    Dem {
        x0: -500.0,
        z0: -500.0,
        cell: 10.0,
        w: 101,
        h: 101,
        data: vec![h; 101 * 101],
        lon0: 7.9087,
        lat0: 46.5934,
    }
}

#[test]
fn overlay_builds_graded_roads_bridges_and_lots() {
    let f = LocalFrame::new(7.9087, 46.5934);
    let mut dem = flat_dem(300.0);
    // a valley across x in [-60, 60]: 40 m deep
    for j in 0..101 {
        for i in 0..101 {
            let x = -500.0 + i as f64 * 10.0;
            if x.abs() < 60.0 {
                dem.data[j * 101 + i] -= 40.0 * (1.0 - (x.abs() / 60.0) as f32);
            }
        }
    }
    let osm = parse_overpass(OSM, &f).unwrap();
    let o = GeoOverlay::build(dem, &osm, 7);
    assert_eq!(o.roads.len(), 2);
    let primary = o.roads.iter().find(|r| (r.id & 0xffff) == 11).unwrap();
    assert!(primary.bridge.iter().all(|b| *b));
    // bridge deck spans the valley instead of dipping into it
    let mid = primary.pts.len() / 2;
    assert!(
        primary.y[mid] > 285.0,
        "deck follows the rims, not the valley floor: {}",
        primary.y[mid]
    );
    for (w, p) in primary.y.windows(2).zip(primary.pts.windows(2)) {
        let l = ((p[1][0] - p[0][0]).powi(2) + (p[1][1] - p[0][1]).powi(2)).sqrt() as f32;
        assert!((w[1] - w[0]).abs() <= 0.12 * l + 1e-3);
    }
    assert_eq!(o.lots.len(), 1);
    assert!(o.lots[0].w >= 4.0 && o.lots[0].d >= 4.0);
    let b = Bounds {
        min_x: -100.0,
        min_z: -100.0,
        max_x: 100.0,
        max_z: 100.0,
    };
    assert_eq!(o.lots_near(&b).len(), 1);
    assert_eq!(
        o.lots_near(&Bounds {
            min_x: 300.0,
            min_z: 300.0,
            max_x: 400.0,
            max_z: 400.0
        })
        .len(),
        0
    );
    let _ = Arc::strong_count(&o.dem);
}

#[test]
fn overlay_replaces_terrain_inside_and_eases_into_procedural_outside() {
    use worldgen_terrain::HeightModifier;
    let o = GeoOverlay::build(flat_dem(777.0), &OsmData::default(), 1);
    let m = o
        .prepare(
            &Bounds {
                min_x: -50.0,
                min_z: -50.0,
                max_x: 50.0,
                max_z: 50.0,
            },
            2.0,
        )
        .expect("inside region");
    assert!(
        (m.apply(0.0, 0.0, 10.0) - 777.0).abs() < 1e-3,
        "interior fully replaced"
    );
    assert!(
        (m.apply(500.0, 0.0, 10.0) - 777.0).abs() < 1e-3,
        "no cliff at the border: the DEM edge elevation continues"
    );
    assert!(m.replaces_terrain(0.0, 0.0));
    // outside, the blend toward the procedural height is monotone and complete at the ramp end
    let mut last = 777.0;
    for k in 0..=30 {
        let h = m.apply(500.0 + k as f64 * 100.0, 0.0, 10.0);
        assert!(h <= last + 1e-3, "monotone ramp at {k}: {h} > {last}");
        last = h;
    }
    assert!(
        (last - 10.0).abs() < 1e-3,
        "procedural terrain is untouched beyond the ramp: {last}"
    );
    assert!(
        o.prepare(
            &Bounds {
                min_x: 9000.0,
                min_z: 9000.0,
                max_x: 9100.0,
                max_z: 9100.0
            },
            2.0
        )
        .is_none(),
        "far chunks are untouched"
    );
}
