//! `cargo run --release -p worldgen-terrain --example survey [seed]` - find scenic spots.
use worldgen_terrain::{SampleCtx, TerrainParams, TerrainWorld};

fn main() {
    let seed: u64 = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0x5EED_0001);
    let w = TerrainWorld::new(TerrainParams {
        seed,
        ..Default::default()
    });
    let half = 60_000.0;
    let ctx = SampleCtx {
        lakes: w.lakes_near(-half, -half, half, half),
        rivers: Some(w.river_sampler(-half, -half, half, half)),
    };
    println!(
        "lakes in 120 km square: {}, river segments: {}",
        ctx.lakes.len(),
        ctx.rivers.as_ref().map_or(0, |r| r.segment_count())
    );
    let step = 400.0;
    let n = (2.0 * half / step) as usize;
    let mut grid = vec![(0f32, false); n * n];
    for j in 0..n {
        for i in 0..n {
            let (x, z) = (-half + i as f64 * step, -half + j as f64 * step);
            let s = w.sample(&ctx, x, z, step);
            grid[j * n + i] = (s.height, s.water > f32::NEG_INFINITY && s.height > 0.5);
        }
    }
    // score 6 km windows: land, relief, inland water
    let win = 15usize;
    let mut best: Vec<(f32, f64, f64, f32, f32, usize)> = vec![];
    for j in (0..n - win).step_by(5) {
        for i in (0..n - win).step_by(5) {
            let (mut lo, mut hi, mut land, mut water) = (f32::MAX, f32::MIN, 0usize, 0usize);
            for dj in 0..win {
                for di in 0..win {
                    let (h, wt) = grid[(j + dj) * n + i + di];
                    lo = lo.min(h);
                    hi = hi.max(h);
                    land += usize::from(h > 2.0);
                    water += usize::from(wt);
                }
            }
            let frac = land as f32 / (win * win) as f32;
            if frac > 0.75 && water >= 2 {
                let score = (hi - lo).min(900.0) + water.min(60) as f32 * 4.0;
                best.push((
                    score,
                    -half + (i + win / 2) as f64 * step,
                    -half + (j + win / 2) as f64 * step,
                    lo,
                    hi,
                    water,
                ));
            }
        }
    }
    best.sort_by(|a, b| b.0.total_cmp(&a.0));
    for b in best.iter().take(8) {
        println!(
            "score {:.0} at x={:.0} z={:.0} relief {:.0}..{:.0} m inland-water-cells {}",
            b.0, b.1, b.2, b.3, b.4, b.5
        );
    }
}
