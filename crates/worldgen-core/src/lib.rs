//! Engine-independent core types (Layer A). No Bevy dependency.
//!
//! * [`ChunkId`] - quadtree chunk address `(lod, x, z)`.
//! * [`derive_seed`] / [`RegionRng`] - order-independent, partitioned random streams.
//! * [`GenVersion`], [`CancelToken`] - stale-result detection and cooperative cancellation.
//! * [`WorldBudget`] - explicit CPU/GPU/job/upload budgets.
//! * [`Fingerprint`] - bitwise FNV-1a fingerprints used by the golden-hash tests.

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Quadtree chunk address. At `lod = L` a chunk spans `2^L` times the lod-0 chunk edge.
/// `x`/`z` are chunk indices at that lod (negative allowed: the world is unbounded).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ChunkId {
    pub lod: u8,
    pub x: i32,
    pub z: i32,
}

impl ChunkId {
    pub const fn new(lod: u8, x: i32, z: i32) -> Self {
        Self { lod, x, z }
    }

    /// Parent at `lod + 1`.
    pub fn parent(self) -> Self {
        Self::new(self.lod + 1, self.x.div_euclid(2), self.z.div_euclid(2))
    }

    /// The four children at `lod - 1`. Panics at lod 0.
    pub fn children(self) -> [ChunkId; 4] {
        assert!(self.lod > 0, "lod-0 chunks have no children");
        let (l, x, z) = (self.lod - 1, self.x * 2, self.z * 2);
        [
            Self::new(l, x, z),
            Self::new(l, x + 1, z),
            Self::new(l, x, z + 1),
            Self::new(l, x + 1, z + 1),
        ]
    }
}

/// Generation version: bumped whenever config or edits change; results carrying an older
/// version are discarded on completion.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
pub struct GenVersion(pub u64);

impl GenVersion {
    pub fn next(self) -> Self {
        Self(self.0 + 1)
    }
}

/// Cooperative cancellation flag shared between the scheduler and a running job.
#[derive(Clone, Debug, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Explicit resource budgets (CPU/GPU memory, concurrency, upload rate).
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct WorldBudget {
    pub cpu_mb: u32,
    pub gpu_mb: u32,
    pub max_inflight_jobs: u32,
    pub upload_bytes_per_frame: u64,
    pub max_resident_chunks: u32,
    /// Evict least-recently-needed hidden chunks while the CPU bytes they still hold exceed this.
    pub max_resident_bytes: u64,
}

impl Default for WorldBudget {
    fn default() -> Self {
        Self {
            cpu_mb: 2048,
            gpu_mb: 2048,
            max_inflight_jobs: 8,
            upload_bytes_per_frame: 4 * 1024 * 1024,
            max_resident_chunks: 1400,
            max_resident_bytes: 768 * 1024 * 1024,
        }
    }
}

/// Subsystem identifiers: each subsystem draws from its own random stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum Subsystem {
    Terrain = 1,
    Erosion = 2,
    Climate = 3,
    Hydrology = 4,
    Roads = 5,
    Towns = 6,
    Buildings = 7,
    Vegetation = 8,
    Edits = 9,
}

/// SplitMix64 finaliser.
#[inline]
pub fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Derive a seed for `(world seed, subsystem, two integer coordinates, salt)`.
/// A pure function: independent of call order, thread, or neighbouring chunks.
#[inline]
pub fn derive_seed(seed: u64, sub: Subsystem, a: i64, b: i64, salt: u64) -> u64 {
    let mut h = splitmix64(seed ^ ((sub as u64) << 48));
    h = splitmix64(h ^ (a as u64));
    h = splitmix64(h ^ (b as u64).rotate_left(21));
    splitmix64(h ^ salt.rotate_left(42))
}

/// Uniform `[0,1)` from a hash.
#[inline]
pub fn unit_f32(h: u64) -> f32 {
    ((h >> 40) as f32) * (1.0 / (1u64 << 24) as f32)
}

/// Uniform `[0,1)` f64 from a hash.
#[inline]
pub fn unit_f64(h: u64) -> f64 {
    ((h >> 11) as f64) * (1.0 / (1u64 << 53) as f64)
}

/// Small deterministic PRNG (SplitMix64 stream) for one region/subsystem.
#[derive(Clone, Debug)]
pub struct RegionRng(u64);

impl RegionRng {
    pub fn new(seed: u64, sub: Subsystem, a: i64, b: i64) -> Self {
        Self(derive_seed(seed, sub, a, b, 0))
    }
    pub fn from_raw(state: u64) -> Self {
        Self(state)
    }
    #[inline]
    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    #[inline]
    pub fn next_f32(&mut self) -> f32 {
        unit_f32(self.next_u64())
    }
    #[inline]
    pub fn next_f64(&mut self) -> f64 {
        unit_f64(self.next_u64())
    }
    pub fn range_f32(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next_f32()
    }
}

/// Bitwise FNV-1a fingerprint accumulator (golden-hash tests).
#[derive(Clone, Copy, Debug)]
pub struct Fingerprint(u64);

impl Default for Fingerprint {
    fn default() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
}

impl Fingerprint {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn bytes(&mut self, b: &[u8]) -> &mut Self {
        for &x in b {
            self.0 ^= x as u64;
            self.0 = self.0.wrapping_mul(0x100_0000_01b3);
        }
        self
    }
    pub fn f32s(&mut self, v: &[f32]) -> &mut Self {
        for x in v {
            self.bytes(&x.to_bits().to_le_bytes());
        }
        self
    }
    pub fn u32s(&mut self, v: &[u32]) -> &mut Self {
        for x in v {
            self.bytes(&x.to_le_bytes());
        }
        self
    }
    pub fn u64(&mut self, x: u64) -> &mut Self {
        self.bytes(&x.to_le_bytes())
    }
    pub fn finish(&self) -> u64 {
        self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derive_seed_is_pure_and_partitioned() {
        let a = derive_seed(7, Subsystem::Terrain, 3, -4, 0);
        assert_eq!(a, derive_seed(7, Subsystem::Terrain, 3, -4, 0));
        assert_ne!(a, derive_seed(7, Subsystem::Erosion, 3, -4, 0));
        assert_ne!(a, derive_seed(7, Subsystem::Terrain, -4, 3, 0));
        assert_ne!(a, derive_seed(8, Subsystem::Terrain, 3, -4, 0));
    }

    #[test]
    fn rng_streams_are_independent_of_creation_order() {
        let mk = |x, z| RegionRng::new(1, Subsystem::Vegetation, x, z).next_u64();
        let first: Vec<u64> = (0..5).map(|i| mk(i, 0)).collect();
        let second: Vec<u64> = (0..5)
            .rev()
            .map(|i| mk(i, 0))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        assert_eq!(first, second);
    }

    #[test]
    fn unit_floats_in_range() {
        let mut r = RegionRng::from_raw(1);
        for _ in 0..10_000 {
            let f = r.next_f32();
            assert!((0.0..1.0).contains(&f));
        }
    }

    #[test]
    fn chunk_tree_roundtrip() {
        let c = ChunkId::new(3, -5, 7);
        for ch in c.children() {
            assert_eq!(ch.parent(), c);
        }
    }

    #[test]
    fn cancel_token_is_shared() {
        let t = CancelToken::new();
        let u = t.clone();
        assert!(!u.is_cancelled());
        t.cancel();
        assert!(u.is_cancelled());
    }

    #[test]
    fn fingerprint_distinguishes_bits() {
        let a = Fingerprint::new().f32s(&[0.0]).finish();
        let b = Fingerprint::new().f32s(&[-0.0]).finish();
        assert_ne!(a, b);
    }
}
