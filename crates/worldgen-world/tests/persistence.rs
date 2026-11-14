use std::sync::Arc;
use worldgen_core::{CancelToken, ChunkId};
use worldgen_terrain::{BrushOp, ChunkConfig, EditLayer, TerrainParams};
use worldgen_world::World;

fn tmpdir(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("wg-test-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn world() -> World {
    World::new(TerrainParams::default(), ChunkConfig::default())
}

#[test]
fn serialisation_is_lossless() {
    let w = world();
    let c = w
        .generate(ChunkId::new(0, 9, -3), &CancelToken::new())
        .unwrap();
    let bytes = c.terrain.to_bytes();
    let back = worldgen_terrain::TerrainChunk::from_bytes(&bytes).expect("decode");
    assert_eq!(back.fingerprint(), c.terrain.fingerprint());
    assert!(worldgen_terrain::TerrainChunk::from_bytes(&bytes[..bytes.len() - 1]).is_none());
}

#[test]
fn cache_hit_equals_miss_and_is_faster() {
    let dir = tmpdir("cache");
    let w = world().with_cache(&dir).unwrap();
    let id = ChunkId::new(1, 4, 4);
    let t0 = std::time::Instant::now();
    let a = w.generate(id, &CancelToken::new()).unwrap();
    let miss = t0.elapsed();
    let t1 = std::time::Instant::now();
    let b = w.generate(id, &CancelToken::new()).unwrap();
    let hit = t1.elapsed();
    assert_eq!(a.terrain.fingerprint(), b.terrain.fingerprint());
    let c = w.cache().unwrap();
    assert_eq!(c.hits.load(std::sync::atomic::Ordering::Relaxed), 1);
    assert!(hit < miss, "hit {hit:?} should beat miss {miss:?}");
    // a different world must not see this cache
    let other = World::new(
        TerrainParams {
            seed: 99,
            ..Default::default()
        },
        ChunkConfig::default(),
    )
    .with_cache(&dir)
    .unwrap();
    let o = other.generate(id, &CancelToken::new()).unwrap();
    assert_ne!(o.terrain.fingerprint(), a.terrain.fingerprint());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn edits_change_terrain_bypass_cache_and_keep_borders_exact() {
    let dir = tmpdir("edit");
    let w = world().with_cache(&dir).unwrap();
    let layer = EditLayer::new(w.cfg().cell_m as f64);
    w.gen_world.add_modifier(layer.clone());
    let a_id = ChunkId::new(0, 10, 10);
    let b_id = ChunkId::new(0, 11, 10);
    let before = w.generate(a_id, &CancelToken::new()).unwrap();
    // populate the cache for a chunk, then edit across the A|B border
    let border_x = w.cfg().bounds(b_id).min_x;
    let z = (w.cfg().bounds(a_id).min_z + w.cfg().bounds(a_id).max_z) * 0.5;
    layer.apply_brush(border_x, z, 25.0, &BrushOp::Raise(6.0));
    let after = w.generate(a_id, &CancelToken::new()).unwrap();
    let after_b = w.generate(b_id, &CancelToken::new()).unwrap();
    assert_ne!(
        before.terrain.fingerprint(),
        after.terrain.fingerprint(),
        "edit must change the chunk (cache must not mask it)"
    );
    let n = after.terrain.n;
    for j in 0..n {
        let (ka, kb) = (j * n + n - 1, j * n);
        assert_eq!(
            after.terrain.heights[ka].to_bits(),
            after_b.terrain.heights[kb].to_bits(),
            "border j={j}"
        );
        for c in 0..3 {
            assert_eq!(
                after.terrain.normals[ka][c].to_bits(),
                after_b.terrain.normals[kb][c].to_bits()
            );
        }
    }
    // the raise is visible: border height rose by about 6 m at the brush centre
    let jc = ((z - w.cfg().bounds(a_id).min_z) / after.terrain.spacing as f64).round() as usize;
    let rise = after.terrain.heights[jc * n + n - 1] - before.terrain.heights[jc * n + n - 1];
    assert!((rise - 6.0).abs() < 0.2, "rise {rise}");
    // persistence: save, load into a fresh layer, same result
    let mut buf = vec![];
    layer.save(&mut buf).unwrap();
    let w2 = world();
    let l2 = EditLayer::new(w2.cfg().cell_m as f64);
    l2.load(&mut buf.as_slice()).unwrap();
    w2.gen_world.add_modifier(l2);
    let again = w2.generate(a_id, &CancelToken::new()).unwrap();
    assert_eq!(again.terrain.fingerprint(), after.terrain.fingerprint());
    let _ = Arc::strong_count(&layer);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Every building belongs to exactly one chunk: summing over all chunks covering a town gives the
/// number of derived buildings (nothing dropped or duplicated on chunk borders).
#[test]
fn buildings_are_owned_by_exactly_one_chunk() {
    let w = world();
    let rw = w.roads.as_ref().unwrap().clone();
    let towns = rw.towns.towns_near(20000.0, -15000.0, 45000.0, 5000.0, 0);
    let t = towns.iter().find(|t| t.kind as u8 == 1).expect("a village");
    let total = w.town_building_count(t);
    assert!(total > 8, "village should have buildings, got {total}");
    let cfg = w.cfg();
    let size = cfg.size_m(0);
    let h = t.half_patch() + 80.0;
    let (x0, x1) = (
        ((t.cx - h) / size).floor() as i32,
        ((t.cx + h) / size).ceil() as i32,
    );
    let (z0, z1) = (
        ((t.cz - h) / size).floor() as i32,
        ((t.cz + h) / size).ceil() as i32,
    );
    let mut sum = 0;
    let mut meshes = 0;
    for z in z0..=z1 {
        for x in x0..=x1 {
            let c = w
                .generate(ChunkId::new(0, x, z), &CancelToken::new())
                .unwrap();
            sum += c.building_count;
            meshes += usize::from(c.take_building_mesh().is_some());
        }
    }
    assert_eq!(
        sum, total,
        "buildings must be counted once across all chunks"
    );
    assert!(meshes > 3, "building meshes expected in several chunks");
}

#[test]
fn live_bytes_shrink_when_meshes_are_taken() {
    let w = world();
    let c = w
        .generate(ChunkId::new(0, 9, -3), &CancelToken::new())
        .unwrap();
    let before = c.live_bytes();
    assert!(
        before > c.terrain.approx_bytes(),
        "meshes count while untaken"
    );
    assert!(c.take_terrain_mesh().is_some());
    let mid = c.live_bytes();
    assert!(mid < before);
    assert!(c.take_terrain_mesh().is_none(), "take-once");
    assert_eq!(c.live_bytes(), mid, "a second take must not release twice");
    let _ = (
        c.take_water_mesh(),
        c.take_road_mesh(),
        c.take_veg_mesh(),
        c.take_building_mesh(),
    );
    assert!(
        c.live_bytes() >= c.terrain.approx_bytes()
            && c.live_bytes() < before / 2 + c.terrain.approx_bytes()
    );
}
