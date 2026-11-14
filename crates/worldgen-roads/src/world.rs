//! The road network of a world: settlements, highways between them, and region queries.

use crate::highways::plan_highway;
use crate::net::{Pad, Road, RoadClass, RoadSampler};
use crate::towns::{CELL_M, Town, TownContent, TownIndex};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};
use worldgen_terrain::{Bounds, HeightModifier, RegionModifier, TerrainWorld};

/// Extra roads/lots from another source (real-world OSM data, user-drawn roads).
pub trait RoadOverlay: Send + Sync {
    fn roads_near(&self, b: &Bounds) -> (Vec<Arc<Road>>, Vec<Pad>);
    /// Building lots whose centres lie in the rectangle (all of them; the caller filters ownership).
    fn lots_near(&self, _b: &Bounds) -> Vec<crate::towns::Lot> {
        Vec::new()
    }
    /// True if the overlay's output can change (or differs between runs): terrain near it must
    /// bypass the disk cache.
    fn stateful(&self) -> bool {
        false
    }
}

type EdgeCell = Arc<OnceLock<Option<Arc<Road>>>>;

pub struct RoadWorld {
    pub towns: TownIndex,
    terrain: Arc<TerrainWorld>,
    edges: RwLock<HashMap<(u64, u64), EdgeCell>>,
    knn: RwLock<HashMap<(i32, i32), Arc<Vec<Town>>>>,
    overlays: RwLock<Vec<Arc<dyn RoadOverlay>>>,
}

/// Roads only matter for chunks whose vertex spacing is at most this (metres).
pub const MAX_SPACING_M: f64 = 32.0;

impl RoadWorld {
    pub fn new(terrain: Arc<TerrainWorld>, seed: u64) -> Arc<Self> {
        Arc::new(Self {
            towns: TownIndex::new(terrain.clone(), seed),
            terrain,
            edges: Default::default(),
            knn: Default::default(),
            overlays: Default::default(),
        })
    }

    pub fn add_overlay(&self, o: Arc<dyn RoadOverlay>) {
        self.overlays.write().unwrap().push(o);
    }

    pub fn overlays(&self) -> Vec<Arc<dyn RoadOverlay>> {
        self.overlays.read().unwrap().clone()
    }

    fn neighbours(&self, t: &Town) -> Arc<Vec<Town>> {
        if let Some(v) = self.knn.read().unwrap().get(&t.cell) {
            return v.clone();
        }
        let mut c: Vec<Town> = self
            .towns
            .towns_near(t.cx, t.cz, t.cx, t.cz, 2)
            .into_iter()
            .filter(|u| u.cell != t.cell)
            .collect();
        let d = |u: &Town| (u.cx - t.cx).powi(2) + (u.cz - t.cz).powi(2);
        c.sort_by(|a, b| d(a).total_cmp(&d(b)).then(a.id().cmp(&b.id())));
        c.truncate(3);
        let v = Arc::new(c);
        self.knn.write().unwrap().insert(t.cell, v.clone());
        v
    }

    fn connected(&self, a: &Town, b: &Town) -> bool {
        self.neighbours(a).iter().any(|u| u.cell == b.cell)
            || self.neighbours(b).iter().any(|u| u.cell == a.cell)
    }

    fn entry(&self, t: &Town, c: &TownContent, toward: &Town) -> (f64, f64, f32) {
        let (dx, dz) = (toward.cx - t.cx, toward.cz - t.cz);
        let l = (dx * dx + dz * dz).sqrt().max(1.0);
        let (ux, uz) = (dx / l, dz / l);
        let lim = t.half_patch() * 0.92;
        c.nodes
            .iter()
            .filter(|n| (n.0 - t.cx).abs() < lim && (n.1 - t.cz).abs() < lim)
            .max_by(|p, q| {
                let sc = |n: &&(f64, f64, f32)| {
                    (n.0 - t.cx) * ux + (n.1 - t.cz) * uz
                        - 0.3 * ((n.0 - t.cx) * -uz + (n.1 - t.cz) * ux).abs()
                };
                sc(p).total_cmp(&sc(q))
            })
            .copied()
            .unwrap_or_else(|| {
                let ctx = self
                    .terrain
                    .ctx_for(t.cx - 50.0, t.cz - 50.0, t.cx + 50.0, t.cz + 50.0);
                (t.cx, t.cz, self.terrain.height(&ctx, t.cx, t.cz, 8.0))
            })
    }

    /// Highway between two towns (cached; `None` if no route could be planned).
    pub fn highway(&self, a: &Town, b: &Town) -> Option<Arc<Road>> {
        let key = if a.id() < b.id() {
            (a.id(), b.id())
        } else {
            (b.id(), a.id())
        };
        let cell = {
            if let Some(c) = self.edges.read().unwrap().get(&key) {
                c.clone()
            } else {
                self.edges.write().unwrap().entry(key).or_default().clone()
            }
        };
        cell.get_or_init(|| {
            let (lo, hi) = if a.id() < b.id() { (a, b) } else { (b, a) };
            let (ca, cb) = (self.towns.content(lo), self.towns.content(hi));
            let (ea, eb) = (self.entry(lo, &ca, hi), self.entry(hi, &cb, lo));
            let class = if lo.kind.rank().max(hi.kind.rank()) >= 2 {
                RoadClass::Highway
            } else {
                RoadClass::Rural
            };
            plan_highway(&self.terrain, ea, eb, class, key.0 ^ key.1.rotate_left(17)).map(Arc::new)
        })
        .clone()
    }

    /// Roads and building pads that may touch the rectangle.
    pub fn roads_near(&self, b: &Bounds) -> (Vec<Arc<Road>>, Vec<Pad>) {
        let towns = self.towns.towns_near(b.min_x, b.min_z, b.max_x, b.max_z, 3);
        let (mut roads, mut pads) = (Vec::new(), Vec::new());
        for t in &towns {
            let h = t.half_patch() + 80.0;
            if t.cx + h >= b.min_x
                && t.cx - h <= b.max_x
                && t.cz + h >= b.min_z
                && t.cz - h <= b.max_z
            {
                let c = self.towns.content(t);
                roads.extend(c.roads.iter().cloned());
                pads.extend(c.pads.iter().copied());
            }
        }
        let mut seen = std::collections::HashSet::new();
        for t in &towns {
            for (di, dj) in (-2..=2).flat_map(|i| (-2..=2).map(move |j| (i, j))) {
                let Some(u) = self.towns.site(t.cell.0 + di, t.cell.1 + dj) else {
                    continue;
                };
                if u.cell == t.cell {
                    continue;
                }
                let key = if t.id() < u.id() {
                    (t.id(), u.id())
                } else {
                    (u.id(), t.id())
                };
                if seen.contains(&key) {
                    continue;
                }
                let (lo_x, hi_x) = (t.cx.min(u.cx) - 900.0, t.cx.max(u.cx) + 900.0);
                let (lo_z, hi_z) = (t.cz.min(u.cz) - 900.0, t.cz.max(u.cz) + 900.0);
                if hi_x < b.min_x || lo_x > b.max_x || hi_z < b.min_z || lo_z > b.max_z {
                    continue;
                }
                seen.insert(key);
                if self.connected(t, &u)
                    && let Some(r) = self.highway(t, &u)
                {
                    roads.push(r);
                }
            }
        }
        for o in self.overlays.read().unwrap().iter() {
            let (r, p) = o.roads_near(b);
            roads.extend(r);
            pads.extend(p);
        }
        (roads, pads)
    }

    /// Spatial index for a region; empty for coarse LODs.
    pub fn sampler(&self, b: &Bounds, spacing: f64) -> Arc<RoadSampler> {
        if spacing > MAX_SPACING_M {
            return Arc::new(RoadSampler::default());
        }
        let (roads, pads) = self.roads_near(&b.expand(60.0));
        Arc::new(RoadSampler::build(
            roads,
            pads,
            (b.min_x, b.min_z, b.max_x, b.max_z),
        ))
    }

    pub fn cell_size() -> f64 {
        CELL_M
    }
}

struct Mod(Arc<RoadSampler>);

impl RegionModifier for Mod {
    fn apply(&self, x: f64, z: f64, h: f32) -> f32 {
        self.0.modify(x, z, h)
    }
    fn mask(&self, x: f64, z: f64) -> u8 {
        self.0.mask(x, z)
    }
}

impl HeightModifier for RoadWorld {
    fn stateful(&self) -> bool {
        self.overlays.read().unwrap().iter().any(|o| o.stateful())
    }

    fn prepare(&self, b: &Bounds, spacing: f64) -> Option<Box<dyn RegionModifier>> {
        let s = self.sampler(b, spacing);
        if s.is_empty() {
            None
        } else {
            Some(Box::new(Mod(s)))
        }
    }
}
