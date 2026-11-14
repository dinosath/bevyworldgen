//! Exported for the wasm32 determinism check: `golden(lod, x, z)` generates one chunk
//! (erosion on for lod <= 2) and returns its bitwise fingerprint.
use std::sync::{Arc, OnceLock};
use worldgen_core::{CancelToken, ChunkId};
use worldgen_terrain::{ChunkConfig, TerrainParams, WorldGen};

fn world() -> &'static Arc<WorldGen> {
    static W: OnceLock<Arc<WorldGen>> = OnceLock::new();
    W.get_or_init(|| WorldGen::new(TerrainParams::default(), ChunkConfig::default()))
}

pub fn golden_native(lod: i32, x: i32, z: i32) -> u64 {
    world()
        .generate_chunk(ChunkId::new(lod as u8, x, z), &CancelToken::new())
        .unwrap()
        .fingerprint()
}

/// Fingerprints are split because wasm32 `extern "C"` cannot return u64 to JS without BigInt;
/// Node supports BigInt, so a plain u64 return is fine.
#[unsafe(no_mangle)]
pub extern "C" fn golden(lod: i32, x: i32, z: i32) -> u64 {
    golden_native(lod, x, z)
}

pub const PROBE: [(i32, i32, i32); 8] = [
    (0, 0, 0),
    (0, -7, 3),
    (1, 12, -9),
    (2, 5, 5),
    (5, -1, -1),
    (7, 2, 2),
    (0, 415, 741),
    (0, 673, 601),
];
