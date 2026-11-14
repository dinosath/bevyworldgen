//! Real-world data (Layer A): projection, DEM from Terrarium tiles, OSM roads/buildings, and the
//! overlay that puts them into a streamed world.
//!
//! Data sources and terms (see `docs/research/license-inventory.md`):
//! * Terrain: AWS "Terrain Tiles" (Terrarium) - composite of SRTM, GMTED, NED, etc.; attribution
//!   per <https://github.com/tilezen/joerd/blob/master/docs/attribution.md>.
//! * Roads/buildings: © OpenStreetMap contributors, ODbL (<https://www.openstreetmap.org/copyright>).
//!   Fetched on demand by the importer; derived data must not be redistributed under other terms.

pub mod dem;
pub mod frame;
pub mod osm;
pub mod overlay;

pub use dem::{Dem, decode_terrarium};
pub use frame::{LocalFrame, mercator_pixel};
pub use osm::{OsmData, footprint_rect, parse_overpass};
pub use overlay::GeoOverlay;
