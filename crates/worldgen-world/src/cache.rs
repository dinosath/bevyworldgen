//! On-disk terrain chunk cache. Keys include a fingerprint of everything that influences terrain
//! (world parameters, chunk config, generator version). Chunks touched by stateful modifiers
//! (user edits) bypass the cache, so it can never serve stale edited terrain.

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use worldgen_core::{ChunkId, Fingerprint};
use worldgen_terrain::chunk::TERRAIN_GEN_VERSION;
use worldgen_terrain::{ChunkConfig, TerrainChunk, TerrainParams};

pub struct ChunkCache {
    dir: PathBuf,
    pub hits: AtomicU64,
    pub misses: AtomicU64,
    pub stores: AtomicU64,
}

/// Fingerprint of all inputs that determine terrain output.
pub fn world_fingerprint(p: &TerrainParams, c: &ChunkConfig) -> u64 {
    let mut f = Fingerprint::new();
    f.bytes(format!("{p:?}|{c:?}|{TERRAIN_GEN_VERSION}").as_bytes());
    f.finish()
}

impl ChunkCache {
    pub fn new(root: impl Into<PathBuf>, fingerprint: u64) -> std::io::Result<Self> {
        let dir = root.into().join(format!("{fingerprint:016x}"));
        fs::create_dir_all(&dir)?;
        Ok(Self {
            dir,
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
            stores: AtomicU64::new(0),
        })
    }

    fn path(&self, id: ChunkId) -> PathBuf {
        self.dir.join(format!("{}_{}_{}.wgc", id.lod, id.x, id.z))
    }

    pub fn load(&self, id: ChunkId) -> Option<TerrainChunk> {
        let bytes = fs::read(self.path(id)).ok();
        let c = bytes
            .as_deref()
            .and_then(TerrainChunk::from_bytes)
            .filter(|c| c.id == id);
        match &c {
            Some(_) => self.hits.fetch_add(1, Ordering::Relaxed),
            None => self.misses.fetch_add(1, Ordering::Relaxed),
        };
        c
    }

    pub fn store(&self, c: &TerrainChunk) {
        let tmp = self.path(c.id).with_extension("tmp");
        let ok = fs::File::create(&tmp)
            .and_then(|mut f| f.write_all(&c.to_bytes()))
            .is_ok();
        if ok && fs::rename(&tmp, self.path(c.id)).is_ok() {
            self.stores.fetch_add(1, Ordering::Relaxed);
        } else {
            let _ = fs::remove_file(&tmp);
        }
    }
}
