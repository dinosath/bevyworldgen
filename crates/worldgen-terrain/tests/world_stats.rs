//! Statistical sanity checks of the generated world (geology, climate, biomes).
use std::collections::HashSet;
use worldgen_core::{CancelToken, ChunkId};
use worldgen_terrain::{ChunkConfig, SampleCtx, TerrainParams, TerrainWorld, WorldGen};

#[test]
fn temperature_falls_with_altitude() {
    let w = TerrainWorld::new(TerrainParams::default());
    let ctx = SampleCtx::default();
    let step = 1000.0;
    let (mut hi, mut lo) = ((f64::MIN, 0.0, 0.0), (f64::MAX, 0.0, 0.0));
    for j in -40..40 {
        for i in -40..40 {
            let (x, z) = (i as f64 * step, j as f64 * step);
            let h = w.height(&ctx, x, z, step) as f64;
            if h > hi.0 {
                hi = (h, x, z);
            }
            if h > 5.0 && h < lo.0 {
                lo = (h, x, z);
            }
        }
    }
    assert!(hi.0 - lo.0 > 900.0, "need relief, got {} m", hi.0 - lo.0);
    let (t_hi, t_lo) = (
        w.climate_node((hi.1 / 64.0) as i64, (hi.2 / 64.0) as i64, 64.0)
            .temp_c,
        w.climate_node((lo.1 / 64.0) as i64, (lo.2 / 64.0) as i64, 64.0)
            .temp_c,
    );
    assert!(
        t_hi < t_lo - 3.0,
        "summit {t_hi:.1} C should be much colder than lowland {t_lo:.1} C"
    );
}

#[test]
fn rain_shadow_exists_leeward_of_ranges() {
    // Westerlies blow toward +x: sample precipitation on a west->east line across the mountains
    // and require that somewhere the air is markedly drier right after a high crest than before it.
    let w = TerrainWorld::new(TerrainParams::default());
    let mut found = false;
    'outer: for z in (-60..60).step_by(4) {
        let zz = z as f64 * 1000.0;
        let line: Vec<(f64, f64, f64)> = (-60..60)
            .map(|i| {
                let x = i as f64 * 1000.0;
                let c = w.climate_node((x / 64.0) as i64, (zz / 64.0) as i64, 64.0);
                (w.macro_height(x, zz, 64.0), c.precip_mm as f64, x)
            })
            .collect();
        for k in 6..line.len() - 6 {
            let crest = line[k].0;
            if crest > 700.0 && line[k - 3].0 < crest - 300.0 && line[k + 3].0 < crest - 300.0 {
                let windward = line[k - 2].1;
                let leeward = line[k + 3].1;
                if leeward < windward * 0.8 {
                    found = true;
                    break 'outer;
                }
            }
        }
    }
    assert!(
        found,
        "no windward/leeward precipitation contrast found across crests"
    );
}

#[test]
fn world_has_diverse_biomes_and_water() {
    let w = WorldGen::new(TerrainParams::default(), ChunkConfig::default());
    let c = CancelToken::new();
    let mut biomes = HashSet::new();
    let mut wet = 0usize;
    // sample lod-5 chunks across a 150 km window (cheap, no erosion)
    for z in -8..8 {
        for x in -8..8 {
            let ch = w.generate_chunk(ChunkId::new(5, x, z), &c).unwrap();
            for (k, b) in ch.biome.iter().enumerate() {
                biomes.insert(*b);
                wet += usize::from(ch.water[k] > f32::NEG_INFINITY);
            }
        }
    }
    assert!(
        biomes.len() >= 6,
        "only {} biomes: {:?}",
        biomes.len(),
        biomes
    );
    assert!(wet > 1000, "expected oceans/lakes, got {wet} wet vertices");
}
