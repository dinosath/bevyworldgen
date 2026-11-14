//! Seamless, deterministic world-space terrain generation (Layer A, no Bevy dependency).
//!
//! * [`TerrainWorld::sample`] - pure pointwise terrain (continents, ranges, rivers, lakes).
//! * [`WorldGen::generate_chunk`] - chunk pipeline with order-independent erosion.
//! * [`climate`] - temperature/precipitation/biomes/splat weights.
//! * [`mesh`] - engine-free mesh buffers.

pub mod chunk;
pub mod climate;
pub mod edits;
pub mod erosion;
pub mod height;
pub mod hydro;
pub mod mesh;
pub mod noise;

pub use chunk::{Bounds, ChunkConfig, HeightModifier, RegionModifier, TerrainChunk, WorldGen};
pub use climate::{Biome, Climate, ClimateGrid, layer};
pub use edits::{BrushOp, EditLayer, EditSnapshot};
pub use erosion::ErosionParams;
pub use height::{Lake, SampleCtx, TerrainParams, TerrainSample, TerrainWorld};
