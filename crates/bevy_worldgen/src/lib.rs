//! Layer B: Bevy integration for streamed procedural worlds.
//!
//! [`WorldStreamingPlugin`] runs the pipeline `Observe -> Plan -> Dispatch -> Collect -> Realise ->
//! Evict` every frame. Worker threads (see `worldgen-jobs`) never touch the ECS; completed chunks
//! arrive as plain data and are turned into entities only inside the budgeted `Collect`/`Realise`
//! systems. A renderer crate reacts to `Added<ChunkData>` and builds meshes/materials.

mod origin;
mod query;
mod stream;

pub use origin::{WorldAnchored, WorldOrigin};
pub use query::TerrainQuery;
pub use stream::{
    ChunkData, ChunkRoot, ChunkState, ChunkStore, InvalidateRegion, StreamConfig, StreamFocus,
    StreamStats, WorldRes,
};

use bevy::prelude::*;
use worldgen_core::WorldBudget;
use worldgen_world::World as GenWorld;
use worldgen_world::streaming::StreamParams;

/// System ordering for the streaming pipeline.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum WorldSet {
    Observe,
    Plan,
    Dispatch,
    Collect,
    Realise,
    Evict,
}

pub struct WorldStreamingPlugin {
    pub world: GenWorld,
    pub stream: StreamParams,
    pub budget: WorldBudget,
    /// Worker threads (default: cores - 2, at least 2).
    pub threads: Option<usize>,
}

impl WorldStreamingPlugin {
    pub fn new(world: GenWorld) -> Self {
        Self {
            world,
            stream: StreamParams::default(),
            budget: WorldBudget::default(),
            threads: None,
        }
    }
}

impl Plugin for WorldStreamingPlugin {
    fn build(&self, app: &mut App) {
        let threads = self.threads.unwrap_or_else(|| {
            std::thread::available_parallelism().map_or(4, |n| n.get().saturating_sub(2).max(2))
        });
        app.insert_resource(WorldRes {
            world: self.world.clone(),
        })
        .insert_resource(StreamConfig {
            params: self.stream,
            budget: self.budget,
            lookahead_s: 2.0,
            cancel_after_frames: 120,
        })
        .insert_resource(ChunkStore::new(threads))
        .init_resource::<StreamStats>()
        .init_resource::<WorldOrigin>()
        .init_resource::<stream::Focus>()
        .init_resource::<stream::PlanState>()
        .add_message::<InvalidateRegion>()
        .configure_sets(
            Update,
            (
                WorldSet::Observe,
                WorldSet::Plan,
                WorldSet::Dispatch,
                WorldSet::Collect,
                WorldSet::Realise,
                WorldSet::Evict,
            )
                .chain(),
        )
        .add_systems(Update, origin::rebase_origin.before(WorldSet::Observe))
        .add_systems(
            Update,
            stream::apply_invalidations.in_set(WorldSet::Observe),
        )
        .add_systems(Update, stream::observe.in_set(WorldSet::Observe))
        .add_systems(Update, stream::plan.in_set(WorldSet::Plan))
        .add_systems(Update, stream::dispatch.in_set(WorldSet::Dispatch))
        .add_systems(Update, stream::collect.in_set(WorldSet::Collect))
        .add_systems(Update, stream::realise.in_set(WorldSet::Realise))
        .add_systems(Update, stream::evict.in_set(WorldSet::Evict));
    }
}
