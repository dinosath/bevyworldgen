//! Settlements: deterministic site selection per world cell, and per-town street grids and lots
//! generated with `symbios-tensor` on a terrain patch.

use crate::net::{Pad, Road, RoadClass};
use std::collections::HashMap;
use std::sync::{Arc, OnceLock, RwLock};
use symbios_ground::HeightMap;
use symbios_tensor::{
    LotConfig, MathMode, RationalizeConfig, RoadType, TensorConfig, extract_blocks, extract_lots,
    generate_roads, prune_unused_roads, rationalize_graph,
};
use worldgen_core::{Subsystem, derive_seed, unit_f64};
use worldgen_terrain::TerrainWorld;

pub const CELL_M: f64 = 3200.0;
const PATCH_CELL: f32 = 2.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TownKind {
    Hamlet,
    Village,
    Town,
    City,
}

impl TownKind {
    pub fn patch_m(self) -> f64 {
        match self {
            TownKind::Hamlet => 256.0,
            TownKind::Village => 384.0,
            _ => 512.0,
        }
    }
    pub fn rank(self) -> u8 {
        self as u8
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Town {
    pub cell: (i32, i32),
    pub cx: f64,
    pub cz: f64,
    pub kind: TownKind,
}

impl Town {
    pub fn id(&self) -> u64 {
        ((self.cell.0 as u32 as u64) << 32) | self.cell.1 as u32 as u64
    }
    pub fn half_patch(&self) -> f64 {
        self.kind.patch_m() * 0.5
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Lot {
    pub cx: f64,
    pub cz: f64,
    pub rot: f32,
    pub w: f32,
    pub d: f32,
    pub y: f32,
    pub seed: u64,
}

pub struct TownContent {
    pub roads: Vec<Arc<Road>>,
    pub lots: Vec<Lot>,
    pub pads: Vec<Pad>,
    /// Street nodes (world x, z, elevation) usable as highway connection points.
    pub nodes: Vec<(f64, f64, f32)>,
}

type SiteCell = Arc<OnceLock<Option<Town>>>;
type ContentCell = Arc<OnceLock<Arc<TownContent>>>;

pub struct TownIndex {
    terrain: Arc<TerrainWorld>,
    seed: u64,
    sites: RwLock<HashMap<(i32, i32), SiteCell>>,
    content: RwLock<HashMap<(i32, i32), ContentCell>>,
    /// Rectangles (min_x, min_z, max_x, max_z) where procedural settlements are suppressed.
    exclusions: RwLock<Vec<(f64, f64, f64, f64)>>,
}

impl TownIndex {
    pub fn new(terrain: Arc<TerrainWorld>, seed: u64) -> Self {
        Self {
            terrain,
            seed,
            sites: Default::default(),
            content: Default::default(),
            exclusions: Default::default(),
        }
    }

    pub fn terrain(&self) -> &Arc<TerrainWorld> {
        &self.terrain
    }

    /// Suppress procedural towns whose patch would overlap the rectangle (set before generating).
    pub fn add_exclusion(&self, rect: (f64, f64, f64, f64)) {
        self.exclusions.write().unwrap().push(rect);
        self.sites.write().unwrap().clear();
        self.content.write().unwrap().clear();
    }

    fn compute_site(&self, cx: i32, cz: i32) -> Option<Town> {
        let h = |salt| derive_seed(self.seed, Subsystem::Towns, cx as i64, cz as i64, salt);
        if unit_f64(h(1)) > 0.62 {
            return None;
        }
        let x = (cx as f64 + 0.2 + 0.6 * unit_f64(h(2))) * CELL_M;
        let z = (cz as f64 + 0.2 + 0.6 * unit_f64(h(3))) * CELL_M;
        let r = unit_f64(h(4));
        let kind = if r < 0.40 {
            TownKind::Hamlet
        } else if r < 0.75 {
            TownKind::Village
        } else if r < 0.95 {
            TownKind::Town
        } else {
            TownKind::City
        };
        let half = kind.patch_m() * 0.5;
        if self
            .exclusions
            .read()
            .unwrap()
            .iter()
            .any(|r| x + half > r.0 && x - half < r.2 && z + half > r.1 && z - half < r.3)
        {
            return None;
        }
        // site suitability: dry, moderately flat land on the whole patch
        let ctx = self.terrain.ctx_for(
            x - half - 100.0,
            z - half - 100.0,
            x + half + 100.0,
            z + half + 100.0,
        );
        let n = 17;
        let (mut lo, mut hi, mut sum, mut wet) = (f32::MAX, f32::MIN, 0.0f32, 0);
        for j in 0..n {
            for i in 0..n {
                let (px, pz) = (
                    x - half + i as f64 / (n - 1) as f64 * 2.0 * half,
                    z - half + j as f64 / (n - 1) as f64 * 2.0 * half,
                );
                let s = self.terrain.sample(&ctx, px, pz, 16.0);
                lo = lo.min(s.height);
                hi = hi.max(s.height);
                sum += s.height;
                wet += usize::from(s.water > f32::NEG_INFINITY);
            }
        }
        let mean = sum / (n * n) as f32;
        if wet > 0 || hi - lo > 26.0 || !(6.0..700.0).contains(&mean) {
            return None;
        }
        Some(Town {
            cell: (cx, cz),
            cx: x,
            cz: z,
            kind,
        })
    }

    pub fn site(&self, cx: i32, cz: i32) -> Option<Town> {
        let cell = {
            if let Some(c) = self.sites.read().unwrap().get(&(cx, cz)) {
                c.clone()
            } else {
                self.sites
                    .write()
                    .unwrap()
                    .entry((cx, cz))
                    .or_default()
                    .clone()
            }
        };
        *cell.get_or_init(|| self.compute_site(cx, cz))
    }

    /// Towns whose cell lies within `rings` cells of the rectangle.
    pub fn towns_near(
        &self,
        min_x: f64,
        min_z: f64,
        max_x: f64,
        max_z: f64,
        rings: i32,
    ) -> Vec<Town> {
        let (c0x, c1x) = (
            (min_x / CELL_M).floor() as i32 - rings,
            (max_x / CELL_M).floor() as i32 + rings,
        );
        let (c0z, c1z) = (
            (min_z / CELL_M).floor() as i32 - rings,
            (max_z / CELL_M).floor() as i32 + rings,
        );
        let mut v = Vec::new();
        for cz in c0z..=c1z {
            for cx in c0x..=c1x {
                if let Some(t) = self.site(cx, cz) {
                    v.push(t);
                }
            }
        }
        v
    }

    pub fn content(&self, t: &Town) -> Arc<TownContent> {
        let cell = {
            if let Some(c) = self.content.read().unwrap().get(&t.cell) {
                c.clone()
            } else {
                self.content
                    .write()
                    .unwrap()
                    .entry(t.cell)
                    .or_default()
                    .clone()
            }
        };
        cell.get_or_init(|| Arc::new(self.build_content(t))).clone()
    }

    fn build_content(&self, t: &Town) -> TownContent {
        let patch = t.kind.patch_m();
        let n = (patch / PATCH_CELL as f64) as usize;
        let (ox, oz) = (t.cx - patch * 0.5, t.cz - patch * 0.5);
        let ctx = self
            .terrain
            .ctx_for(ox - 50.0, oz - 50.0, ox + patch + 50.0, oz + patch + 50.0);
        let mut hm = HeightMap::new(n, n, PATCH_CELL);
        for j in 0..n {
            for i in 0..n {
                hm.set(
                    i,
                    j,
                    self.terrain.height(
                        &ctx,
                        ox + i as f64 * PATCH_CELL as f64,
                        oz + j as f64 * PATCH_CELL as f64,
                        4.0,
                    ),
                );
            }
        }
        let tid = t.id();
        let (major, minor, max_lot) = match t.kind {
            TownKind::Hamlet => (95.0, 48.0, 600.0),
            TownKind::Village => (75.0, 38.0, 520.0),
            TownKind::Town => (60.0, 30.0, 460.0),
            TownKind::City => (48.0, 24.0, 380.0),
        };
        let tcfg = TensorConfig {
            seed: derive_seed(
                self.seed,
                Subsystem::Roads,
                t.cell.0 as i64,
                t.cell.1 as i64,
                1,
            ),
            major_road_dist: major,
            minor_road_dist: minor,
            math: MathMode::Portable,
            ..TensorConfig::default()
        };
        let mut empty = TownContent {
            roads: vec![],
            lots: vec![],
            pads: vec![],
            nodes: vec![],
        };
        let Ok(mut g) = generate_roads(&hm, &tcfg) else {
            return empty;
        };
        rationalize_graph(&mut g, &hm, &RationalizeConfig::default());
        extract_blocks(&mut g);
        let lcfg = LotConfig {
            max_lot_area: max_lot,
            min_lot_area: 110.0,
            front_setback: 3.0,
            side_setback: 1.2,
            rear_setback: 2.5,
            min_width: 8.0,
            min_depth: 8.0,
            ..LotConfig::default()
        };
        let lots = extract_lots(&g, &mut hm, &lcfg);
        prune_unused_roads(&mut g, &lots);

        // chain active edges into polylines (break at junctions and class changes)
        let class_of = |ty: RoadType| match ty {
            RoadType::Major => RoadClass::Avenue,
            RoadType::Minor => RoadClass::Street,
        };
        let deg: Vec<usize> = g
            .nodes
            .iter()
            .map(|nd| {
                nd.edges
                    .iter()
                    .filter(|e| g.edges[**e as usize].active)
                    .count()
            })
            .collect();
        let mut used = vec![false; g.edges.len()];
        let mut roads = Vec::new();
        let mut next_id = tid << 8;
        for e0 in 0..g.edges.len() {
            if used[e0] || !g.edges[e0].active {
                continue;
            }
            // walk to a chain start: backwards while the node is a pass-through of the same class
            let ty = g.edges[e0].road_type;
            let mut start_node = g.edges[e0].start;
            let mut cur_e = e0 as u32;
            let mut guard = 0;
            while deg[start_node as usize] == 2 && guard < 4000 {
                guard += 1;
                let other = g.nodes[start_node as usize]
                    .edges
                    .iter()
                    .copied()
                    .find(|e| *e != cur_e && g.edges[*e as usize].active);
                match other {
                    Some(o)
                        if g.edges[o as usize].road_type == ty
                            && !used[o as usize]
                            && o as usize != e0 =>
                    {
                        start_node = g.opposite(o, start_node);
                        cur_e = o;
                    }
                    _ => break,
                }
            }
            // now walk forward from start_node along cur_e
            let mut pts = vec![start_node];
            let mut node = start_node;
            let mut e = cur_e;
            loop {
                used[e as usize] = true;
                node = g.opposite(e, node);
                pts.push(node);
                if deg[node as usize] != 2 {
                    break;
                }
                let nxt = g.nodes[node as usize].edges.iter().copied().find(|x| {
                    *x != e
                        && g.edges[*x as usize].active
                        && !used[*x as usize]
                        && g.edges[*x as usize].road_type == ty
                });
                match nxt {
                    Some(x) => e = x,
                    None => break,
                }
            }
            if pts.len() >= 2 {
                let p: Vec<[f64; 2]> = pts
                    .iter()
                    .map(|&k| {
                        [
                            ox + g.nodes[k as usize].position.x as f64,
                            oz + g.nodes[k as usize].position.y as f64,
                        ]
                    })
                    .collect();
                let y: Vec<f32> = pts.iter().map(|&k| g.nodes[k as usize].elevation).collect();
                let segs = p.len() - 1;
                roads.push(Arc::new(Road {
                    id: next_id,
                    class: class_of(ty),
                    pts: p,
                    y,
                    bridge: vec![false; segs],
                }));
                next_id += 1;
            }
        }
        let mut pads = Vec::new();
        let mut out_lots = Vec::new();
        for (i, l) in lots.iter().enumerate() {
            let (wx, wz) = (ox + l.position.x as f64, oz + l.position.y as f64);
            let y = hm.get_height_at(l.position.x, l.position.y);
            out_lots.push(Lot {
                cx: wx,
                cz: wz,
                rot: l.rotation,
                w: l.width,
                d: l.depth,
                y,
                seed: derive_seed(
                    self.seed,
                    Subsystem::Buildings,
                    t.cell.0 as i64 * 4096 + t.cell.1 as i64,
                    i as i64,
                    7,
                ),
            });
            pads.push(Pad {
                cx: wx,
                cz: wz,
                rot: l.rotation,
                half_w: l.width * 0.5,
                half_d: l.depth * 0.5,
                y,
            });
        }
        empty.roads = roads;
        empty.lots = out_lots;
        empty.pads = pads;
        empty.nodes = g
            .nodes
            .iter()
            .zip(&deg)
            .filter(|(_, d)| **d > 0)
            .map(|(nd, _)| {
                (
                    ox + nd.position.x as f64,
                    oz + nd.position.y as f64,
                    nd.elevation,
                )
            })
            .collect();
        empty
    }
}
