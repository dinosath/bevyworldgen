use std::collections::HashSet;
use worldgen_core::{CancelToken, ChunkId};
use worldgen_terrain::{ChunkConfig, TerrainParams, WorldGen};
use worldgen_veg::{Species, VegConfig, census, place};

fn world() -> std::sync::Arc<WorldGen> {
    WorldGen::new(TerrainParams::default(), ChunkConfig::default())
}

fn world_pos(c: &worldgen_terrain::TerrainChunk, p: [f32; 3]) -> (i64, i64) {
    // exact: instance world coords reconstructed as origin + local; quantise to 1 mm
    (
        ((c.origin_x + p[0] as f64) * 1000.0).round() as i64,
        ((c.origin_z + p[2] as f64) * 1000.0).round() as i64,
    )
}

/// Find lod-0 chunks that contain forest (scan a few candidates).
fn forested(w: &WorldGen, cfg: &VegConfig) -> Vec<ChunkId> {
    let mut out = vec![];
    for z in (-200..200).step_by(7) {
        for x in (-200..200).step_by(7) {
            let id = ChunkId::new(0, x + 300, z + 300);
            let c = w.generate_chunk(id, &CancelToken::new()).unwrap();
            if place(&c, cfg, &[]).len() > 60 {
                out.push(id);
                if out.len() >= 3 {
                    return out;
                }
            }
        }
    }
    out
}

#[test]
fn neighbours_never_share_or_drop_plants_and_placement_is_deterministic() {
    let w = world();
    let cfg = VegConfig::default();
    let ids = forested(&w, &cfg);
    assert!(!ids.is_empty(), "no forested chunk found");
    for id in ids {
        let mut seen = HashSet::new();
        let mut total = 0;
        for (dx, dz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let nid = ChunkId::new(0, id.x + dx, id.z + dz);
            let c = w.generate_chunk(nid, &CancelToken::new()).unwrap();
            let a = place(&c, &cfg, &[]);
            let b = place(&c, &cfg, &[]);
            assert_eq!(a.len(), b.len());
            for (p, q) in a.iter().zip(&b) {
                assert_eq!(p.pos.map(f32::to_bits), q.pos.map(f32::to_bits));
            }
            for i in &a {
                let (x, z) = world_pos(&c, i.pos);
                assert!(
                    seen.insert((x, z)),
                    "plant duplicated across chunks at {x},{z}"
                );
                // inside this chunk's half-open bounds
                let size = c.spacing as f64 * (c.n - 1) as f64;
                assert!(
                    i.pos[0] >= 0.0
                        && (i.pos[0] as f64) < size + 1e-3
                        && i.pos[2] >= 0.0
                        && (i.pos[2] as f64) < size + 1e-3
                );
            }
            total += a.len();
        }
        assert!(total > 100, "forest too sparse: {total}");
    }
}

#[test]
fn ecology_follows_climate() {
    let w = world();
    let cfg = VegConfig::default();
    let mut c = [0usize; 6];
    let (mut hi_trees, mut lo_trees) = (0usize, 0usize);
    for z in -12..12 {
        for x in -12..12 {
            let ch = w
                .generate_chunk(
                    ChunkId::new(1, x * 5 - 27, z * 5 + 238),
                    &CancelToken::new(),
                )
                .unwrap();
            let v = place(&ch, &cfg, &[]);
            let n = census(&v);
            for k in 0..6 {
                c[k] += n[k];
            }
            for i in &v {
                if matches!(
                    i.species,
                    Species::Oak | Species::Spruce | Species::Pine | Species::Birch
                ) {
                    if i.pos[1] > 1300.0 {
                        hi_trees += 1;
                    } else if i.pos[1] < 400.0 {
                        lo_trees += 1;
                    }
                }
            }
        }
    }
    let trees = c[0] + c[1] + c[2] + c[3];
    assert!(trees > 500, "expected trees, census {c:?}");
    assert!(
        c[4] > 50 && c[5] > 20,
        "expected shrubs and boulders: {c:?}"
    );
    assert!(
        c.iter().filter(|n| **n > 0).count() >= 4,
        "species diversity too low: {c:?}"
    );
    assert!(
        hi_trees * 20 < lo_trees.max(1),
        "trees above the treeline: high {hi_trees} vs low {lo_trees}"
    );
    assert!(
        c[1] + c[2] > 0 && c[0] > 0,
        "need both conifers and broadleaf: {c:?}"
    );
}

#[test]
fn nothing_grows_in_water_or_on_cliffs() {
    let w = world();
    let cfg = VegConfig::default();
    for id in forested(&w, &cfg) {
        let ch = w.generate_chunk(id, &CancelToken::new()).unwrap();
        for i in place(&ch, &cfg, &[]) {
            let k = {
                let ii = (i.pos[0] / ch.spacing).round() as usize;
                let jj = (i.pos[2] / ch.spacing).round() as usize;
                jj.min(ch.n - 1) * ch.n + ii.min(ch.n - 1)
            };
            assert!(ch.water[k] == f32::NEG_INFINITY, "plant in water");
            if !matches!(i.species, Species::Boulder) {
                assert!(ch.normals[k][1] > 0.7, "tree on a cliff");
            }
        }
    }
}
