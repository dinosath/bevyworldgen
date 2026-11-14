//! Road networks (Layer A): settlements, highways, town streets, bridges, terrain carving, meshes.

pub mod highways;
pub mod mesh;
pub mod net;
pub mod towns;
pub mod user;
pub mod world;

pub use mesh::{RoadMeshData, build_road_mesh};
pub use net::{Pad, Road, RoadClass, RoadSampler, mask};
pub use towns::{Lot, Town, TownContent, TownIndex, TownKind};
pub use user::UserRoads;
pub use world::{RoadOverlay, RoadWorld};
