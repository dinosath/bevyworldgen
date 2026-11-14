//! Highway planning: least-cost path over a coarse cost surface (slope, water), smoothed and
//! resampled, with a grade-limited vertical profile and bridge detection.

use crate::net::{Road, RoadClass};
use std::cmp::Ordering;
use std::collections::BinaryHeap;
use worldgen_terrain::TerrainWorld;

const G: f64 = 40.0;
const RES: f64 = 8.0;
const MAX_GRADE: f32 = 0.07;
const MAX_CELLS: usize = 220_000;

struct Node {
    f: f64,
    idx: u32,
}
impl PartialEq for Node {
    fn eq(&self, o: &Self) -> bool {
        self.idx == o.idx && self.f.to_bits() == o.f.to_bits()
    }
}
impl Eq for Node {}
impl PartialOrd for Node {
    fn partial_cmp(&self, o: &Self) -> Option<Ordering> {
        Some(self.cmp(o))
    }
}
impl Ord for Node {
    fn cmp(&self, o: &Self) -> Ordering {
        o.f.total_cmp(&self.f).then(o.idx.cmp(&self.idx))
    }
}

pub(crate) fn chaikin(p: &[[f64; 2]], iters: usize) -> Vec<[f64; 2]> {
    let mut cur = p.to_vec();
    for _ in 0..iters {
        if cur.len() < 3 {
            break;
        }
        let mut n = vec![cur[0]];
        for w in cur.windows(2) {
            n.push([
                0.75 * w[0][0] + 0.25 * w[1][0],
                0.75 * w[0][1] + 0.25 * w[1][1],
            ]);
            n.push([
                0.25 * w[0][0] + 0.75 * w[1][0],
                0.25 * w[0][1] + 0.75 * w[1][1],
            ]);
        }
        n.push(*cur.last().unwrap());
        cur = n;
    }
    cur
}

pub(crate) fn resample(p: &[[f64; 2]], step: f64) -> Vec<[f64; 2]> {
    let mut out = vec![p[0]];
    let mut carry = 0.0;
    for w in p.windows(2) {
        let (dx, dz) = (w[1][0] - w[0][0], w[1][1] - w[0][1]);
        let len = (dx * dx + dz * dz).sqrt();
        if len < 1e-9 {
            continue;
        }
        let mut s = step - carry;
        while s <= len {
            out.push([w[0][0] + dx * s / len, w[0][1] + dz * s / len]);
            s += step;
        }
        carry = len - (s - step);
    }
    let last = *p.last().unwrap();
    if out
        .last()
        .is_none_or(|l| (l[0] - last[0]).abs() + (l[1] - last[1]).abs() > 1.0)
    {
        out.push(last);
    }
    out
}

fn smooth(y: &[f32], half: usize) -> Vec<f32> {
    (0..y.len())
        .map(|i| {
            let (a, b) = (i.saturating_sub(half), (i + half).min(y.len() - 1));
            y[a..=b].iter().sum::<f32>() / (b - a + 1) as f32
        })
        .collect()
}

/// Plan a road between two world points with fixed end elevations.
pub fn plan_highway(
    terrain: &TerrainWorld,
    a: (f64, f64, f32),
    b: (f64, f64, f32),
    class: RoadClass,
    id: u64,
) -> Option<Road> {
    let (min_x, min_z) = (a.0.min(b.0) - 700.0, a.1.min(b.1) - 700.0);
    let (max_x, max_z) = (a.0.max(b.0) + 700.0, a.1.max(b.1) + 700.0);
    let (nx, nz) = (
        ((max_x - min_x) / G).ceil() as usize + 1,
        ((max_z - min_z) / G).ceil() as usize + 1,
    );
    if nx * nz > MAX_CELLS {
        return None;
    }
    let ctx = terrain.ctx_for(min_x, min_z, max_x, max_z);
    let mut h = vec![0f32; nx * nz];
    let mut wet = vec![false; nx * nz];
    for j in 0..nz {
        for i in 0..nx {
            let s = terrain.sample(&ctx, min_x + i as f64 * G, min_z + j as f64 * G, G);
            h[j * nx + i] = s.height;
            wet[j * nx + i] = s.water > f32::NEG_INFINITY;
        }
    }
    let cell = |p: (f64, f64)| {
        (
            ((p.0 - min_x) / G).round().clamp(0.0, (nx - 1) as f64) as usize,
            ((p.1 - min_z) / G).round().clamp(0.0, (nz - 1) as f64) as usize,
        )
    };
    let (s, e) = (cell((a.0, a.1)), cell((b.0, b.1)));
    let (si, ei) = ((s.1 * nx + s.0) as u32, (e.1 * nx + e.0) as u32);
    let mut best = vec![f64::MAX; nx * nz];
    let mut prev = vec![u32::MAX; nx * nz];
    let mut heap = BinaryHeap::new();
    let heur = |i: usize, j: usize| {
        (((i as f64 - e.0 as f64).powi(2) + (j as f64 - e.1 as f64).powi(2)).sqrt()) * G
    };
    best[si as usize] = 0.0;
    heap.push(Node {
        f: heur(s.0, s.1),
        idx: si,
    });
    const N8: [(i32, i32); 8] = [
        (1, 0),
        (-1, 0),
        (0, 1),
        (0, -1),
        (1, 1),
        (1, -1),
        (-1, 1),
        (-1, -1),
    ];
    let mut found = false;
    while let Some(Node { idx, .. }) = heap.pop() {
        if idx == ei {
            found = true;
            break;
        }
        let (ci, cj) = ((idx as usize) % nx, (idx as usize) / nx);
        let g0 = best[idx as usize];
        for (di, dj) in N8 {
            let (ni, nj) = (ci as i32 + di, cj as i32 + dj);
            if ni < 0 || nj < 0 || ni >= nx as i32 || nj >= nz as i32 {
                continue;
            }
            let (ni, nj) = (ni as usize, nj as usize);
            let k = nj * nx + ni;
            let dist = G * if di != 0 && dj != 0 {
                std::f64::consts::SQRT_2
            } else {
                1.0
            };
            let slope = ((h[k] - h[idx as usize]).abs() as f64) / dist;
            let pen = ((slope / 0.06).powi(2) * 3.0).min(80.0);
            let mut c = dist * (1.0 + pen);
            if wet[k] || h[k] < 0.5 {
                c += 70.0 * dist / G;
            }
            let g = g0 + c;
            if g < best[k] {
                best[k] = g;
                prev[k] = idx;
                heap.push(Node {
                    f: g + heur(ni, nj),
                    idx: k as u32,
                });
            }
        }
    }
    if !found {
        return None;
    }
    let mut path = vec![];
    let mut cur = ei;
    while cur != u32::MAX {
        path.push([
            min_x + (cur as usize % nx) as f64 * G,
            min_z + (cur as usize / nx) as f64 * G,
        ]);
        cur = prev[cur as usize];
    }
    path.reverse();
    if path.len() < 2 {
        return None;
    }
    path[0] = [a.0, a.1];
    *path.last_mut().unwrap() = [b.0, b.1];
    let pts = resample(&chaikin(&path, 3), RES);
    if pts.len() < 6 {
        return None;
    }

    profile_road(terrain, &ctx, pts, Some((a.2, b.2)), class, id)
}

/// Vertical profile for a polyline: raw terrain, bridged over water, smoothed, anchored (optional)
/// and grade-limited; bridges are marked where the road crosses water or a deep valley.
pub fn profile_road(
    terrain: &TerrainWorld,
    ctx: &worldgen_terrain::SampleCtx,
    pts: Vec<[f64; 2]>,
    anchor: Option<(f32, f32)>,
    class: RoadClass,
    id: u64,
) -> Option<Road> {
    if pts.len() < 3 {
        return None;
    }
    let n = pts.len();
    let raw: Vec<f32> = pts
        .iter()
        .map(|p| terrain.sample(ctx, p[0], p[1], RES).height)
        .collect();
    let wetp: Vec<bool> = pts
        .iter()
        .map(|p| terrain.sample(ctx, p[0], p[1], RES).water > f32::NEG_INFINITY)
        .collect();
    // mark water runs (dilated for abutments) and bridge them by interpolating across
    let mut over = wetp.clone();
    for i in 0..n {
        if wetp[i] {
            for k in i.saturating_sub(2)..=(i + 2).min(n - 1) {
                over[k] = true;
            }
        }
    }
    let mut base = raw.clone();
    let mut i = 0;
    while i < n {
        if over[i] {
            let j0 = i;
            while i < n && over[i] {
                i += 1;
            }
            let (l, r) = (j0.saturating_sub(1), i.min(n - 1));
            let (yl, yr) = (raw[l], raw[r]);
            for (k, v) in base.iter_mut().enumerate().take(i).skip(j0) {
                let t = (k - l) as f32 / (r - l).max(1) as f32;
                *v = yl + (yr - yl) * t;
            }
        } else {
            i += 1;
        }
    }
    let mut y = smooth(&smooth(&base, 12), 12);
    // anchor to the town street elevations at both ends, blending over ~300 m
    let (d0, d1) = anchor.map_or((0.0, 0.0), |(a, b)| (a - y[0], b - y[n - 1]));
    for (i, v) in y.iter_mut().enumerate() {
        let k0 = (1.0 - i as f32 / 40.0).max(0.0);
        let k1 = (1.0 - (n - 1 - i) as f32 / 40.0).max(0.0);
        *v += d0 * k0 * k0 + d1 * k1 * k1;
    }
    // grade limit: forward then backward, then anchors again
    let seg_len: Vec<f32> = pts
        .windows(2)
        .map(|w| ((w[1][0] - w[0][0]).powi(2) + (w[1][1] - w[0][1]).powi(2)).sqrt() as f32)
        .collect();
    for _ in 0..4 {
        for i in 1..n {
            let m = MAX_GRADE * seg_len[i - 1];
            y[i] = y[i].clamp(y[i - 1] - m, y[i - 1] + m);
        }
        for i in (0..n - 1).rev() {
            let m = MAX_GRADE * seg_len[i];
            y[i] = y[i].clamp(y[i + 1] - m, y[i + 1] + m);
        }
    }
    // bridges: over water, or high above a deep valley (cut would be absurd)
    let mut bridge = vec![false; n - 1];
    for i in 0..n - 1 {
        let deep = raw[i] < y[i] - 7.0 && raw[i + 1] < y[i + 1] - 7.0;
        bridge[i] = over[i] || over[i + 1] || deep;
    }
    // close one-segment gaps between bridge spans
    for i in 1..n.saturating_sub(2) {
        if !bridge[i] && bridge[i - 1] && bridge[i + 1] {
            bridge[i] = true;
        }
    }
    Some(Road {
        id,
        class,
        pts,
        y,
        bridge,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use worldgen_terrain::{TerrainParams, TerrainWorld};

    #[test]
    fn plan_respects_grade_and_endpoints() {
        let w = TerrainWorld::new(TerrainParams::default());
        let ctx = w.ctx_for(28500.0, 13500.0, 32300.0, 15700.0);
        let ya = w.height(&ctx, 29000.0, 14000.0, 8.0);
        let yb = w.height(&ctx, 31800.0, 15200.0, 8.0);
        let (a, b) = ((29000.0, 14000.0, ya), (31800.0, 15200.0, yb));
        let r = plan_highway(&w, a, b, RoadClass::Highway, 1).expect("route");
        assert!(r.length() >= ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt());
        assert!(
            (r.y[0] - a.2).abs() < 3.0 && (r.y[r.y.len() - 1] - b.2).abs() < 3.0,
            "ends {} {} vs {} {}",
            r.y[0],
            r.y[r.y.len() - 1],
            a.2,
            b.2
        );
        let max = r
            .y
            .windows(2)
            .zip(r.pts.windows(2))
            .map(|(y, p)| {
                (y[1] - y[0]).abs()
                    / (((p[1][0] - p[0][0]).powi(2) + (p[1][1] - p[0][1]).powi(2)).sqrt() as f32)
                        .max(0.1)
            })
            .fold(0.0f32, f32::max);
        assert!(max <= MAX_GRADE + 0.003, "grade {max}");
        assert_eq!(r.pts.len(), r.y.len());
        assert_eq!(r.bridge.len(), r.pts.len() - 1);
        // deterministic
        let r2 = plan_highway(&w, a, b, RoadClass::Highway, 1).unwrap();
        assert_eq!(r.pts.len(), r2.pts.len());
        assert!(
            r.y.iter()
                .zip(&r2.y)
                .all(|(p, q)| p.to_bits() == q.to_bits())
        );
    }
}
