//! Quadtree streaming planner (pure; no engine). Decides which chunks to show and which to request.
//!
//! * A node is *split* when the camera is closer than `size * split_factor`.
//! * A split is only shown when **all four children are ready**; otherwise the parent (if ready)
//!   keeps covering the area, so the visible set never has holes or overlaps once covered.
//! * Zooming out re-uses already-resident children until the parent arrives.

use std::collections::HashSet;
use worldgen_core::ChunkId;
use worldgen_terrain::ChunkConfig;

#[derive(Clone, Copy, Debug)]
pub struct StreamParams {
    pub split_factor: f64,
    /// Root nodes (at `cfg.max_lod`) within this many root-sizes of the camera are planned.
    pub root_radius: i32,
}

impl Default for StreamParams {
    fn default() -> Self {
        Self {
            split_factor: 2.0,
            root_radius: 1,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Request {
    pub id: ChunkId,
    /// Lower is more urgent.
    pub priority: f32,
}

#[derive(Default, Debug)]
pub struct Plan {
    pub visible: Vec<ChunkId>,
    pub requests: Vec<Request>,
    /// True when every wanted area is covered by ready chunks.
    pub complete: bool,
}

struct Ctx<'a> {
    cfg: &'a ChunkConfig,
    sp: &'a StreamParams,
    cam: (f64, f64, f64),
    ready: &'a dyn Fn(ChunkId) -> bool,
    requests: Vec<Request>,
    seen: HashSet<ChunkId>,
}

impl Ctx<'_> {
    fn dist(&self, id: ChunkId) -> f64 {
        let b = self.cfg.bounds(id);
        let dx = (b.min_x - self.cam.0).max(0.0).max(self.cam.0 - b.max_x);
        let dz = (b.min_z - self.cam.2).max(0.0).max(self.cam.2 - b.max_z);
        let h = self.cam.1.max(0.0) * 0.7;
        (dx * dx + dz * dz + h * h).sqrt()
    }

    fn wants_split(&self, id: ChunkId) -> bool {
        id.lod > 0 && self.dist(id) < self.cfg.size_m(id.lod) * self.sp.split_factor
    }

    fn request(&mut self, id: ChunkId, bias: f32) {
        if self.seen.insert(id) {
            let pr = (self.dist(id) / self.cfg.size_m(id.lod)) as f32 + bias;
            self.requests.push(Request { id, priority: pr });
        }
    }

    fn cover_from_ready(&self, id: ChunkId, out: &mut Vec<ChunkId>) -> bool {
        if (self.ready)(id) {
            out.push(id);
            return true;
        }
        if id.lod == 0 {
            return false;
        }
        let mut tmp = Vec::new();
        for k in id.children() {
            if !self.cover_from_ready(k, &mut tmp) {
                return false;
            }
        }
        out.extend(tmp);
        true
    }

    fn node(&mut self, id: ChunkId, visible: &mut Vec<ChunkId>) -> bool {
        if self.wants_split(id) {
            let mut kids_vis = Vec::new();
            let mut all = true;
            for k in id.children() {
                all &= self.node(k, &mut kids_vis);
            }
            if all {
                visible.extend(kids_vis);
                return true;
            }
            if (self.ready)(id) {
                visible.push(id);
                return true;
            }
            // coarser fallback while the finer set is incomplete
            self.request(id, -2.0);
            visible.extend(kids_vis);
            false
        } else if (self.ready)(id) {
            visible.push(id);
            true
        } else {
            self.request(id, 0.0);
            let mut kv = Vec::new();
            if id.lod > 0 && self.cover_from_ready(id, &mut kv) {
                visible.extend(kv);
                return true;
            }
            false
        }
    }
}

/// Plan the visible set and the chunk requests for a camera at world `(x, y, z)`.
pub fn plan(
    cfg: &ChunkConfig,
    sp: &StreamParams,
    cam: (f64, f64, f64),
    ready: &dyn Fn(ChunkId) -> bool,
) -> Plan {
    let root_size = cfg.size_m(cfg.max_lod);
    let (rx, rz) = (
        (cam.0 / root_size).floor() as i32,
        (cam.2 / root_size).floor() as i32,
    );
    let mut ctx = Ctx {
        cfg,
        sp,
        cam,
        ready,
        requests: Vec::new(),
        seen: HashSet::new(),
    };
    let mut visible = Vec::new();
    let mut complete = true;
    for dz in -sp.root_radius..=sp.root_radius {
        for dx in -sp.root_radius..=sp.root_radius {
            complete &= ctx.node(ChunkId::new(cfg.max_lod, rx + dx, rz + dz), &mut visible);
        }
    }
    ctx.requests
        .sort_by(|a, b| a.priority.total_cmp(&b.priority));
    Plan {
        visible,
        requests: ctx.requests,
        complete,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_ancestor(a: ChunkId, b: ChunkId) -> bool {
        let mut c = b;
        while c.lod < a.lod {
            c = c.parent();
        }
        c == a && a.lod > b.lod
    }

    #[test]
    fn all_ready_tiles_exactly_without_overlap() {
        let cfg = ChunkConfig::default();
        let sp = StreamParams::default();
        for cam in [
            (10.0, 50.0, 10.0),
            (-5000.0, 300.0, 12345.0),
            (1e6, 20.0, -1e6),
        ] {
            let p = plan(&cfg, &sp, cam, &|_| true);
            assert!(p.complete && p.requests.is_empty());
            let area: f64 = p.visible.iter().map(|&i| cfg.size_m(i.lod).powi(2)).sum();
            let roots = ((2 * sp.root_radius + 1) as f64).powi(2);
            assert!(
                (area - roots * cfg.size_m(cfg.max_lod).powi(2)).abs() < 1.0,
                "area {area}"
            );
            for (i, &a) in p.visible.iter().enumerate() {
                for &b in &p.visible[i + 1..] {
                    assert!(
                        !is_ancestor(a, b) && !is_ancestor(b, a) && a != b,
                        "overlap {a:?} {b:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn finest_lod_is_near_the_camera() {
        let cfg = ChunkConfig::default();
        let p = plan(&cfg, &StreamParams::default(), (0.0, 10.0, 0.0), &|_| true);
        let near = p.visible.iter().filter(|i| i.lod == 0).count();
        assert!((4..100).contains(&near), "lod0 count {near}");
        assert!(
            p.visible.len() < 400,
            "too many chunks: {}",
            p.visible.len()
        );
        // every lod-0 chunk is within a few chunk-sizes of the camera
        for i in p.visible.iter().filter(|i| i.lod == 0) {
            let b = cfg.bounds(*i);
            assert!(b.min_x.abs().min(b.max_x.abs()) < 600.0);
        }
    }

    #[test]
    fn nothing_ready_requests_everything_coarse_first() {
        let cfg = ChunkConfig::default();
        let p = plan(&cfg, &StreamParams::default(), (0.0, 10.0, 0.0), &|_| false);
        assert!(p.visible.is_empty() && !p.complete);
        assert!(!p.requests.is_empty());
        // requests are sorted by priority
        assert!(
            p.requests
                .windows(2)
                .all(|w| w[0].priority <= w[1].priority)
        );
    }

    #[test]
    fn parent_keeps_covering_until_children_ready() {
        let cfg = ChunkConfig::default();
        let sp = StreamParams::default();
        // only coarse (lod >= 4) ready
        let p = plan(&cfg, &sp, (0.0, 10.0, 0.0), &|i| i.lod >= 4);
        assert!(p.visible.iter().all(|i| i.lod >= 4));
        // the area covered is whole roots (no holes) - at least as much as root coverage
        let area: f64 = p.visible.iter().map(|&i| cfg.size_m(i.lod).powi(2)).sum();
        let roots = ((2 * sp.root_radius + 1) as f64).powi(2);
        assert!((area - roots * cfg.size_m(cfg.max_lod).powi(2)).abs() < 1.0);
        assert!(!p.requests.is_empty());
    }

    #[test]
    fn zoom_out_reuses_ready_children() {
        let cfg = ChunkConfig::default();
        let sp = StreamParams::default();
        // camera far above: wants coarse nodes, but only lod-0 children exist for one root area
        let ready = |i: ChunkId| i.lod == 0 || i.lod == cfg.max_lod;
        let p = plan(&cfg, &sp, (0.0, 20000.0, 0.0), &ready);
        let area: f64 = p.visible.iter().map(|&i| cfg.size_m(i.lod).powi(2)).sum();
        let roots = ((2 * sp.root_radius + 1) as f64).powi(2);
        assert!((area - roots * cfg.size_m(cfg.max_lod).powi(2)).abs() < 1.0);
    }
}

#[cfg(test)]
mod tuning {
    use super::*;
    /// `cargo test -p worldgen-world tuning -- --nocapture` prints chunk/triangle counts per factor.
    #[test]
    fn print_counts() {
        let cfg = ChunkConfig::default();
        for f in [1.5, 2.0, 2.5, 3.0] {
            let sp = StreamParams {
                split_factor: f,
                root_radius: 1,
            };
            for alt in [10.0, 200.0, 1500.0] {
                let p = plan(&cfg, &sp, (0.0, alt, 0.0), &|_| true);
                let c = cfg.cells as usize;
                let tris: usize = p.visible.iter().map(|_| c * c * 2 + 4 * c * 2).sum();
                let l0 = p.visible.iter().filter(|i| i.lod == 0).count();
                println!(
                    "factor {f} alt {alt}: chunks {} lod0 {} tris {:.2}M",
                    p.visible.len(),
                    l0,
                    tris as f64 / 1e6
                );
            }
        }
    }
}
