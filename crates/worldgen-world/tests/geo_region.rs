use worldgen_core::{CancelToken, ChunkId};
use worldgen_geo::{Dem, GeoOverlay, OsmData};
use worldgen_terrain::{ChunkConfig, TerrainParams};
use worldgen_world::World;

fn world_with_region() -> World {
    let w = World::new(TerrainParams::default(), ChunkConfig::default());
    // flat 1234 m plateau, 3.2 km square centred on the origin
    let dem = Dem {
        x0: -1600.0,
        z0: -1600.0,
        cell: 16.0,
        w: 201,
        h: 201,
        data: vec![1234.0; 201 * 201],
        lon0: 7.9,
        lat0: 46.6,
    };
    let o = std::sync::Arc::new(GeoOverlay::build(dem, &OsmData::default(), 5));
    let b = o.bounds();
    w.gen_world.add_modifier(o.clone());
    let rw = w.roads.as_ref().unwrap();
    rw.add_overlay(o);
    rw.towns.add_exclusion((
        b.min_x - 400.0,
        b.min_z - 400.0,
        b.max_x + 400.0,
        b.max_z + 400.0,
    ));
    w
}

#[test]
fn chunks_inside_the_region_follow_the_dem() {
    let w = world_with_region();
    let c = w
        .generate(ChunkId::new(0, -3, 2), &CancelToken::new())
        .unwrap();
    let flat = c
        .terrain
        .heights
        .iter()
        .filter(|h| (**h - 1234.0).abs() < 1e-3)
        .count();
    // procedural highways may still be carved through the plateau; the bulk must be the DEM
    assert!(
        flat * 10 >= c.terrain.heights.len() * 9,
        "{flat}/{} vertices on the DEM",
        c.terrain.heights.len()
    );
    // outside the ramp the world is procedural again (not a 1234 m plateau)
    let far = w
        .generate(ChunkId::new(0, 200, 200), &CancelToken::new())
        .unwrap();
    assert!(
        far.terrain
            .heights
            .iter()
            .any(|h| (*h - 1234.0).abs() > 1.0)
    );
}

#[test]
fn borders_stay_bitwise_equal_across_the_region_boundary() {
    let w = world_with_region();
    let t = CancelToken::new();
    // x = 1600 m is a chunk border at LOD 0 (64 m chunks): chunks 24 | 25 straddle the DEM edge
    for lod_id in [(0u8, 24i32, 5i32, 25i32), (0, -26, -4, -25)] {
        let (lod, xa, z, xb) = lod_id;
        let a = w.generate(ChunkId::new(lod, xa, z), &t).unwrap().terrain;
        let b = w.generate(ChunkId::new(lod, xb, z), &t).unwrap().terrain;
        let n = a.n;
        for j in 0..n {
            assert_eq!(
                a.heights[j * n + n - 1].to_bits(),
                b.heights[j * n].to_bits(),
                "row {j}"
            );
        }
    }
    // vertical neighbours inside the region and on the ramp
    for (x, z0) in [(3i32, 4i32), (60, 60)] {
        let a = w.generate(ChunkId::new(0, x, z0), &t).unwrap().terrain;
        let b = w.generate(ChunkId::new(0, x, z0 + 1), &t).unwrap().terrain;
        let n = a.n;
        for i in 0..n {
            assert_eq!(
                a.heights[(n - 1) * n + i].to_bits(),
                b.heights[i].to_bits(),
                "col {i}"
            );
        }
    }
}

#[test]
fn region_chunks_are_deterministic_and_bypass_the_cache() {
    let dir = std::env::temp_dir().join(format!("wg-geo-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let w = world_with_region().with_cache(&dir).unwrap();
    let id = ChunkId::new(0, 5, 5);
    let a = w.generate(id, &CancelToken::new()).unwrap();
    let b = w.generate(id, &CancelToken::new()).unwrap();
    assert_eq!(a.terrain.fingerprint(), b.terrain.fingerprint());
    assert_eq!(
        w.cache()
            .unwrap()
            .hits
            .load(std::sync::atomic::Ordering::Relaxed),
        0,
        "stateful overlay output must not be served from the disk cache"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
