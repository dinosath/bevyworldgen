//! `cargo run --release -p worldgen-terrain --example chunk_bench`
use std::time::Instant;
use worldgen_core::{CancelToken, ChunkId};
use worldgen_terrain::{ChunkConfig, TerrainParams, WorldGen};

fn main() {
    let w = WorldGen::new(TerrainParams::default(), ChunkConfig::default());
    let c = CancelToken::new();
    for lod in 0..=7u8 {
        let mut ts = vec![];
        for k in 0..12 {
            let t = Instant::now();
            let ch = w
                .generate_chunk(ChunkId::new(lod, 100 + k, 50 - k), &c)
                .unwrap();
            ts.push(t.elapsed().as_secs_f64() * 1000.0);
            std::hint::black_box(ch.fingerprint());
        }
        ts.sort_by(|a, b| a.partial_cmp(b).unwrap());
        println!(
            "lod {lod}: median {:.1} ms  p90 {:.1} ms  max {:.1} ms (erosion {})",
            ts[6],
            ts[10],
            ts[11],
            lod <= w.cfg.erosion.max_lod
        );
    }
    // parallel throughput at lod 0
    let n = 96;
    let t = Instant::now();
    std::thread::scope(|s| {
        let threads = 16;
        for th in 0..threads {
            let w = &w;
            s.spawn(move || {
                for k in (th..n).step_by(threads) {
                    let _ = w.generate_chunk(ChunkId::new(0, k as i32, 7), &CancelToken::new());
                }
            });
        }
    });
    let el = t.elapsed().as_secs_f64();
    println!(
        "parallel lod0: {n} chunks in {:.0} ms = {:.0} chunks/s",
        el * 1000.0,
        n as f64 / el
    );
}
