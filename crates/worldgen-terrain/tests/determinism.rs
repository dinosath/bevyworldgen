use worldgen_core::{CancelToken, ChunkId};
use worldgen_terrain::{ChunkConfig, TerrainChunk, TerrainParams, WorldGen};

fn world() -> std::sync::Arc<WorldGen> {
    WorldGen::new(TerrainParams::default(), ChunkConfig::default())
}

fn make(w: &WorldGen, id: ChunkId) -> TerrainChunk {
    w.generate_chunk(id, &CancelToken::new())
        .expect("not cancelled")
}

fn assert_x_border(a: &TerrainChunk, b: &TerrainChunk) {
    let n = a.n;
    for j in 0..n {
        let (ka, kb) = (j * n + n - 1, j * n);
        assert_eq!(
            a.heights[ka].to_bits(),
            b.heights[kb].to_bits(),
            "height j={j}"
        );
        for c in 0..3 {
            assert_eq!(
                a.normals[ka][c].to_bits(),
                b.normals[kb][c].to_bits(),
                "normal j={j}"
            );
        }
        assert_eq!(a.splat[ka], b.splat[kb], "splat j={j}");
        assert_eq!(a.water[ka].to_bits(), b.water[kb].to_bits(), "water j={j}");
        assert_eq!(a.biome[ka], b.biome[kb], "biome j={j}");
    }
}

fn assert_z_border(a: &TerrainChunk, b: &TerrainChunk) {
    let n = a.n;
    for i in 0..n {
        let (ka, kb) = ((n - 1) * n + i, i);
        assert_eq!(
            a.heights[ka].to_bits(),
            b.heights[kb].to_bits(),
            "height i={i}"
        );
        for c in 0..3 {
            assert_eq!(
                a.normals[ka][c].to_bits(),
                b.normals[kb][c].to_bits(),
                "normal i={i}"
            );
        }
        assert_eq!(a.splat[ka], b.splat[kb], "splat i={i}");
    }
}

/// G1b: borders between same-LOD neighbours are bitwise equal, with erosion on, for every
/// quadrant (negative coordinates included).
#[test]
fn borders_are_bitwise_equal_with_erosion() {
    let w = world();
    for lod in [0u8, 1, 2] {
        for (x, z) in [(0, 0), (-1, 0), (-3, 5), (40, -17)] {
            let a = make(&w, ChunkId::new(lod, x, z));
            let r = make(&w, ChunkId::new(lod, x + 1, z));
            let d = make(&w, ChunkId::new(lod, x, z + 1));
            assert_x_border(&a, &r);
            assert_z_border(&a, &d);
        }
    }
}

#[test]
fn borders_equal_without_erosion_at_coarse_lods() {
    let w = world();
    for lod in [3u8, 5, 7] {
        let a = make(&w, ChunkId::new(lod, -2, 1));
        let r = make(&w, ChunkId::new(lod, -1, 1));
        let d = make(&w, ChunkId::new(lod, -2, 2));
        assert_x_border(&a, &r);
        assert_z_border(&a, &d);
    }
}

/// G1: request order and thread count never change content.
#[test]
fn request_order_independent_and_parallel() {
    let ids: Vec<ChunkId> = (-2..2)
        .flat_map(|x| (-2..2).map(move |z| ChunkId::new(0, x, z)))
        .collect();
    let w1 = world();
    let f1: Vec<u64> = ids.iter().map(|&i| make(&w1, i).fingerprint()).collect();
    let w2 = world();
    let mut order: Vec<usize> = (0..ids.len()).collect();
    order.reverse();
    order.rotate_left(5);
    let mut f2 = vec![0u64; ids.len()];
    for &k in &order {
        f2[k] = make(&w2, ids[k]).fingerprint();
    }
    assert_eq!(f1, f2);

    let w3 = world();
    let mut f3 = vec![0u64; ids.len()];
    std::thread::scope(|s| {
        let hs: Vec<_> = ids
            .iter()
            .enumerate()
            .map(|(k, &id)| {
                let w = &w3;
                s.spawn(move || (k, make(w, id).fingerprint()))
            })
            .collect();
        for h in hs {
            let (k, f) = h.join().unwrap();
            f3[k] = f;
        }
    });
    assert_eq!(f1, f3);
}

#[test]
fn erosion_changes_terrain_but_stays_bounded() {
    let mut cfg = ChunkConfig::default();
    cfg.erosion.enabled = false;
    let off = WorldGen::new(TerrainParams::default(), cfg);
    let on = world();
    let id = ChunkId::new(0, 30, 12);
    let (a, b) = (make(&off, id), make(&on, id));
    let mut maxd = 0.0f32;
    let mut changed = 0;
    for (x, y) in a.heights.iter().zip(&b.heights) {
        let d = (x - y).abs();
        maxd = maxd.max(d);
        if d > 1e-4 {
            changed += 1;
        }
    }
    assert!(
        changed > a.heights.len() / 20,
        "erosion had no visible effect: {changed}"
    );
    assert!(maxd <= on.cfg.erosion.max_delta_m + 1e-3, "delta {maxd}");
}

#[test]
fn cancellation_returns_none() {
    let w = world();
    let t = CancelToken::new();
    t.cancel();
    assert!(w.generate_chunk(ChunkId::new(0, 0, 0), &t).is_none());
}

#[test]
fn splat_sums_to_255_and_normals_unit() {
    let w = world();
    let c = make(&w, ChunkId::new(1, 3, -4));
    for s in &c.splat {
        assert_eq!(s.iter().map(|v| *v as u32).sum::<u32>(), 255);
    }
    for n in &c.normals {
        let l = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        assert!((l - 1.0).abs() < 1e-4);
        assert!(n[1] > 0.0);
    }
}

/// Golden hashes: any change to generator output (including via dependency or compiler changes)
/// must be a conscious decision. Regenerate with `GOLDEN_PRINT=1 cargo test -- golden --nocapture`.
#[test]
fn golden_hashes() {
    let w = world();
    // the last two lod-0 chunks contain river channels (found with the `find_river` example)
    let ids = [
        ChunkId::new(0, 0, 0),
        ChunkId::new(0, -7, 3),
        ChunkId::new(2, 5, 5),
        ChunkId::new(5, -1, -1),
        ChunkId::new(0, 415, 741),
        ChunkId::new(0, 673, 601),
    ];
    let got: Vec<u64> = ids.iter().map(|&i| make(&w, i).fingerprint()).collect();
    if std::env::var("GOLDEN_PRINT").is_ok() {
        println!("GOLDEN {got:016x?}");
    }
    let want: [u64; 6] = GOLDEN;
    assert_eq!(got, want, "generator output changed");
}

const GOLDEN: [u64; 6] = [
    0xf62b21370111a455,
    0xd343c90ebed35305,
    0x5a8dbfa14b48040c,
    0x509d0d27845d912d,
    0x30b5dbb2ea413c3f,
    0x8ade5998f116f4e9,
];

/// Rivers cross chunk borders: heights, normals, water and splat must still match bitwise.
#[test]
fn borders_are_exact_across_river_channels() {
    let w = world();
    for (x, z) in [(415, 741), (673, 601)] {
        let a = make(&w, ChunkId::new(0, x, z));
        let wet = a.water.iter().filter(|v| **v > f32::NEG_INFINITY).count();
        let r = make(&w, ChunkId::new(0, x + 1, z));
        let d = make(&w, ChunkId::new(0, x, z + 1));
        assert_x_border(&a, &r);
        assert_z_border(&a, &d);
        println!("river chunk ({x},{z}) wet vertices {wet}");
    }
}
