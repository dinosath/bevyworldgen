//! Road geometry for one chunk: asphalt ribbons, lane markings, bridge decks, rails and piers.
//! A road segment belongs to the chunk containing its midpoint, so chunks never duplicate or drop
//! geometry; shared end vertices are identical on both sides of a border.

use crate::net::{Road, RoadClass, RoadSampler};
use worldgen_terrain::Bounds;

#[derive(Default)]
pub struct RoadMeshData {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
    /// Index of the first prop triangle: everything before it is road surface, markings and bridges.
    pub props_from: usize,
}

impl RoadMeshData {
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }
    fn v(&mut self, p: [f32; 3], n: [f32; 3], c: [f32; 3]) -> u32 {
        self.positions.push(p);
        self.normals.push(n);
        self.colors.push([c[0], c[1], c[2], 1.0]);
        (self.positions.len() - 1) as u32
    }
    fn quad(&mut self, a: u32, b: u32, c: u32, d: u32) {
        // a,b along one side; c,d the other side (a-c, b-d connected): two triangles facing +normal order
        self.indices.extend_from_slice(&[a, b, c, b, d, c]);
    }
}

const ASPHALT: [f32; 3] = [0.040, 0.042, 0.046];
const WHITE: [f32; 3] = [0.62, 0.62, 0.60];
const YELLOW: [f32; 3] = [0.62, 0.45, 0.05];
const CONCRETE: [f32; 3] = [0.30, 0.30, 0.29];
const UP: [f32; 3] = [0.0, 1.0, 0.0];

fn lift_for(spacing: f64) -> f32 {
    0.04 + 0.07 * (spacing - 2.0).max(0.0) as f32
}

struct Run<'a> {
    r: &'a Road,
    i0: usize,
    i1: usize, // inclusive point indices
}

fn tangent(r: &Road, k: usize) -> [f64; 2] {
    let (a, b) = (
        r.pts[k.saturating_sub(1)],
        r.pts[(k + 1).min(r.pts.len() - 1)],
    );
    let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
    let l = (dx * dx + dz * dz).sqrt().max(1e-9);
    [dx / l, dz / l]
}

/// Build the road mesh for a chunk. `ground(x, z)` is the terrain height (for bridge piers).
pub fn build_road_mesh(
    s: &RoadSampler,
    b: &Bounds,
    origin: (f64, f64),
    spacing: f64,
    ground: &dyn Fn(f64, f64) -> f32,
) -> RoadMeshData {
    let mut m = RoadMeshData::default();
    if s.roads.is_empty() || spacing > 32.0 {
        return m;
    }
    let lift = lift_for(spacing);
    let detail = spacing <= 4.0;
    let rel = |x: f64, z: f64| ((x - origin.0) as f32, (z - origin.1) as f32);
    let mut prop_runs: Vec<(std::sync::Arc<Road>, usize, usize)> = Vec::new();
    for r in &s.roads {
        let n = r.pts.len();
        if n < 2 {
            continue;
        }
        let owned: Vec<bool> = (0..n - 1)
            .map(|i| {
                let (mx, mz) = (
                    (r.pts[i][0] + r.pts[i + 1][0]) * 0.5,
                    (r.pts[i][1] + r.pts[i + 1][1]) * 0.5,
                );
                mx >= b.min_x && mx < b.max_x && mz >= b.min_z && mz < b.max_z
            })
            .collect();
        if !owned.iter().any(|o| *o) {
            continue;
        }
        // runs of owned segments split by bridge flag
        let mut i = 0;
        while i < n - 1 {
            if !owned[i] {
                i += 1;
                continue;
            }
            let bridge = r.bridge[i];
            let i0 = i;
            while i < n - 1 && owned[i] && r.bridge[i] == bridge {
                i += 1;
            }
            let run = Run { r, i0, i1: i };
            if !bridge && spacing <= 4.0 {
                prop_runs.push((r.clone(), i0, i));
            }
            if bridge {
                bridge_run(&mut m, &run, lift, &rel, ground);
            } else {
                ribbon_run(&mut m, &run, lift, detail, &rel);
            }
        }
    }
    // second pass: props, so everything before `props_from` is the drivable surface
    m.props_from = m.indices.len();
    for (r, i0, i1) in &prop_runs {
        props_run(
            &mut m,
            &Run {
                r,
                i0: *i0,
                i1: *i1,
            },
            lift,
            &rel,
        );
    }
    m
}

fn ribbon_run(
    m: &mut RoadMeshData,
    run: &Run,
    lift: f32,
    detail: bool,
    rel: &dyn Fn(f64, f64) -> (f32, f32),
) {
    let r = run.r;
    let hw = r.class.half_width();
    let mut left = vec![];
    let mut right = vec![];
    for k in run.i0..=run.i1 {
        let t = tangent(r, k);
        let (nx, nz) = (-t[1], t[0]);
        let (cx, cz) = rel(r.pts[k][0], r.pts[k][1]);
        let y = r.y[k] + lift;
        left.push(m.v([cx + nx as f32 * hw, y, cz + nz as f32 * hw], UP, ASPHALT));
        right.push(m.v([cx - nx as f32 * hw, y, cz - nz as f32 * hw], UP, ASPHALT));
    }
    for k in 0..left.len() - 1 {
        m.quad(left[k], left[k + 1], right[k], right[k + 1]);
    }
    if detail {
        markings(m, run, lift + 0.025, rel);
    }
}

/// Position, tangent and y at arc length `s` along the run.
fn at(r: &Road, i0: usize, i1: usize, mut s: f64) -> Option<([f64; 2], [f64; 2], f32)> {
    for k in i0..i1 {
        let (a, b) = (r.pts[k], r.pts[k + 1]);
        let l = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2)).sqrt();
        if s <= l || k + 1 == i1 {
            let t = if l > 0.0 {
                (s / l).clamp(0.0, 1.0)
            } else {
                0.0
            };
            return Some((
                [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t],
                [(b[0] - a[0]) / l.max(1e-9), (b[1] - a[1]) / l.max(1e-9)],
                r.y[k] + (r.y[k + 1] - r.y[k]) * t as f32,
            ));
        }
        s -= l;
    }
    None
}

fn strip(
    m: &mut RoadMeshData,
    run: &Run,
    from: f64,
    to: f64,
    off: f32,
    width: f32,
    y: f32,
    color: [f32; 3],
    rel: &dyn Fn(f64, f64) -> (f32, f32),
) {
    let (Some((p0, t0, y0)), Some((p1, t1, y1))) = (
        at(run.r, run.i0, run.i1, from),
        at(run.r, run.i0, run.i1, to),
    ) else {
        return;
    };
    let mk = |p: [f64; 2], t: [f64; 2], yy: f32, m: &mut RoadMeshData| {
        let (nx, nz) = (-t[1] as f32, t[0] as f32);
        let (cx, cz) = rel(p[0], p[1]);
        let a = m.v(
            [cx + nx * (off + width), yy + y, cz + nz * (off + width)],
            UP,
            color,
        );
        let b = m.v(
            [cx + nx * (off - width), yy + y, cz + nz * (off - width)],
            UP,
            color,
        );
        (a, b)
    };
    let (a0, b0) = mk(p0, t0, y0, m);
    let (a1, b1) = mk(p1, t1, y1, m);
    m.quad(a0, a1, b0, b1);
}

fn markings(m: &mut RoadMeshData, run: &Run, y: f32, rel: &dyn Fn(f64, f64) -> (f32, f32)) {
    let r = run.r;
    let len: f64 = (run.i0..run.i1)
        .map(|k| {
            ((r.pts[k + 1][0] - r.pts[k][0]).powi(2) + (r.pts[k + 1][1] - r.pts[k][1]).powi(2))
                .sqrt()
        })
        .sum();
    if len < 2.0 {
        return;
    }
    let hw = r.class.half_width();
    // centre line: dashes (streets: only on avenues), yellow on highways
    let centre = !matches!(r.class, RoadClass::Street);
    if centre {
        let col = if r.class == RoadClass::Highway {
            YELLOW
        } else {
            WHITE
        };
        let mut s = 1.0;
        while s + 3.0 < len {
            strip(m, run, s, s + 3.0, 0.0, 0.09, y, col, rel);
            s += 9.0;
        }
    }
    if r.class.has_edge_lines() {
        for side in [-1.0f32, 1.0] {
            let mut s = 0.0;
            while s < len {
                let e = (s + 6.0).min(len);
                strip(m, run, s, e, side * (hw - 0.35), 0.075, y, WHITE, rel);
                s = e;
            }
        }
    }
}

fn boxm(m: &mut RoadMeshData, c: [f32; 3], axes: [[f32; 3]; 3], half: [f32; 3], color: [f32; 3]) {
    // axes: three orthonormal vectors; half: half extents along each
    let corner = |sx: f32, sy: f32, sz: f32| {
        let mut p = c;
        for k in 0..3 {
            p[k] +=
                axes[0][k] * half[0] * sx + axes[1][k] * half[1] * sy + axes[2][k] * half[2] * sz;
        }
        p
    };
    let faces: [([f32; 3], [(f32, f32, f32); 4]); 6] = [
        (
            axes[1],
            [
                (-1.0, 1.0, -1.0),
                (1.0, 1.0, -1.0),
                (-1.0, 1.0, 1.0),
                (1.0, 1.0, 1.0),
            ],
        ),
        (
            [-axes[1][0], -axes[1][1], -axes[1][2]],
            [
                (-1.0, -1.0, 1.0),
                (1.0, -1.0, 1.0),
                (-1.0, -1.0, -1.0),
                (1.0, -1.0, -1.0),
            ],
        ),
        (
            axes[0],
            [
                (1.0, -1.0, -1.0),
                (1.0, -1.0, 1.0),
                (1.0, 1.0, -1.0),
                (1.0, 1.0, 1.0),
            ],
        ),
        (
            [-axes[0][0], -axes[0][1], -axes[0][2]],
            [
                (-1.0, -1.0, 1.0),
                (-1.0, -1.0, -1.0),
                (-1.0, 1.0, 1.0),
                (-1.0, 1.0, -1.0),
            ],
        ),
        (
            axes[2],
            [
                (1.0, -1.0, 1.0),
                (-1.0, -1.0, 1.0),
                (1.0, 1.0, 1.0),
                (-1.0, 1.0, 1.0),
            ],
        ),
        (
            [-axes[2][0], -axes[2][1], -axes[2][2]],
            [
                (-1.0, -1.0, -1.0),
                (1.0, -1.0, -1.0),
                (-1.0, 1.0, -1.0),
                (1.0, 1.0, -1.0),
            ],
        ),
    ];
    for (n, cs) in faces {
        let v: Vec<u32> = cs
            .iter()
            .map(|&(a, b, d)| m.v(corner(a, b, d), n, color))
            .collect();
        m.indices
            .extend_from_slice(&[v[0], v[2], v[1], v[1], v[2], v[3]]);
    }
}

fn bridge_run(
    m: &mut RoadMeshData,
    run: &Run,
    lift: f32,
    rel: &dyn Fn(f64, f64) -> (f32, f32),
    ground: &dyn Fn(f64, f64) -> f32,
) {
    let r = run.r;
    let hw = r.class.half_width() + 0.5;
    // deck: one slab per segment (top + sides + underside via boxes oriented to the segment)
    let mut acc = 0.0f64;
    let mut next_pier = 12.0f64;
    for k in run.i0..run.i1 {
        let (a, b) = (r.pts[k], r.pts[k + 1]);
        let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
        let l = (dx * dx + dz * dz).sqrt();
        if l < 1e-6 {
            continue;
        }
        let (ux, uz) = ((dx / l) as f32, (dz / l) as f32);
        let (mx, mz) = rel((a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5);
        let ym = (r.y[k] + r.y[k + 1]) * 0.5 + lift;
        let dy = r.y[k + 1] - r.y[k];
        let dl = (l as f32).hypot(dy);
        // axes: along (tilted by grade), up, across
        let along = [ux * l as f32 / dl, dy / dl, uz * l as f32 / dl];
        let across = [-uz, 0.0, ux];
        let up = [
            -along[1] * ux,
            (along[0] * ux + along[2] * uz),
            -along[1] * uz,
        ];
        let ul = (up[0] * up[0] + up[1] * up[1] + up[2] * up[2]).sqrt();
        let up = [up[0] / ul, up[1] / ul, up[2] / ul];
        boxm(
            m,
            [mx, ym - 0.45, mz],
            [along, up, across],
            [dl * 0.5 + 0.02, 0.45, hw],
            CONCRETE,
        );
        // road surface on top of the deck
        let rw = r.class.half_width();
        boxm(
            m,
            [mx, ym + 0.01, mz],
            [along, up, across],
            [dl * 0.5 + 0.01, 0.015, rw],
            ASPHALT,
        );
        // rails
        for side in [-1.0f32, 1.0] {
            let off = side * (hw - 0.14);
            boxm(
                m,
                [mx + across[0] * off, ym + 0.5, mz + across[2] * off],
                [along, up, across],
                [dl * 0.5 + 0.01, 0.5, 0.13],
                CONCRETE,
            );
        }
        // piers
        let seg_end = acc + l;
        while next_pier <= seg_end {
            let t = ((next_pier - acc) / l).clamp(0.0, 1.0);
            let (px, pz) = (a[0] + dx * t, a[1] + dz * t);
            let py = r.y[k] + dy * t as f32;
            let g = ground(px, pz);
            let h = py - 0.9 - g;
            if h > 0.5 {
                let (rx, rz) = rel(px, pz);
                boxm(
                    m,
                    [rx, g + h * 0.5 - 0.5, rz],
                    [[ux, 0.0, uz], [0.0, 1.0, 0.0], across],
                    [0.7, h * 0.5 + 0.5, hw * 0.7],
                    CONCRETE,
                );
            }
            next_pier += 28.0;
        }
        acc = seg_end;
    }
}

const POLE: [f32; 3] = [0.10, 0.10, 0.11];
const LAMP: [f32; 3] = [0.95, 0.88, 0.62];
const POST: [f32; 3] = [0.70, 0.70, 0.68];

/// Street lamps along town streets, delineator posts along highways and rural roads.
fn props_run(m: &mut RoadMeshData, run: &Run, lift: f32, rel: &dyn Fn(f64, f64) -> (f32, f32)) {
    let r = run.r;
    let len: f64 = (run.i0..run.i1)
        .map(|k| {
            ((r.pts[k + 1][0] - r.pts[k][0]).powi(2) + (r.pts[k + 1][1] - r.pts[k][1]).powi(2))
                .sqrt()
        })
        .sum();
    let hw = r.class.half_width() + r.class.verge() * 0.5;
    let (spacing, lamp) = match r.class {
        RoadClass::Street | RoadClass::Avenue => (30.0, true),
        _ => (48.0, false),
    };
    // phase from the road id so neighbouring roads do not line up exactly
    let mut s = 6.0 + (r.id % 7) as f64;
    let mut side = if r.id & 1 == 0 { 1.0f32 } else { -1.0 };
    while s < len - 2.0 {
        if let Some((p, t, y)) = at(r, run.i0, run.i1, s) {
            let (nx, nz) = (-t[1] as f32, t[0] as f32);
            let (cx, cz) = rel(p[0], p[1]);
            let axes = [
                [t[0] as f32, 0.0, t[1] as f32],
                [0.0, 1.0, 0.0],
                [nx, 0.0, nz],
            ];
            if lamp {
                let off = side * (hw + 0.5);
                let (bx, bz) = (cx + nx * off, cz + nz * off);
                let by = y + lift;
                boxm(m, [bx, by + 2.5, bz], axes, [0.07, 2.5, 0.07], POLE);
                // arm reaching over the road, lamp head at its end
                let (ax, az) = (bx - nx * side * 0.9, bz - nz * side * 0.9);
                boxm(m, [ax, by + 5.0, az], axes, [0.05, 0.05, 0.95], POLE);
                boxm(
                    m,
                    [bx - nx * side * 1.7, by + 4.95, bz - nz * side * 1.7],
                    axes,
                    [0.28, 0.07, 0.14],
                    LAMP,
                );
            } else {
                for sd in [-1.0f32, 1.0] {
                    let off = sd * (hw + 0.25);
                    boxm(
                        m,
                        [cx + nx * off, y + lift + 0.45, cz + nz * off],
                        axes,
                        [0.045, 0.45, 0.045],
                        POST,
                    );
                }
            }
        }
        s += if lamp { spacing * 0.5 } else { spacing };
        side = -side;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn tri_normal(m: &RoadMeshData, t: &[u32]) -> [f32; 3] {
        let p = |i: u32| m.positions[i as usize];
        let (a, b, c) = (p(t[0]), p(t[1]), p(t[2]));
        let (e1, e2) = (
            [b[0] - a[0], b[1] - a[1], b[2] - a[2]],
            [c[0] - a[0], c[1] - a[1], c[2] - a[2]],
        );
        [
            e1[1] * e2[2] - e1[2] * e2[1],
            e1[2] * e2[0] - e1[0] * e2[2],
            e1[0] * e2[1] - e1[1] * e2[0],
        ]
    }

    /// Every triangle must face the same way as the normal stored on its vertices (else it is culled).
    fn assert_outward(m: &RoadMeshData) {
        for t in m.indices.chunks(3) {
            let n = tri_normal(m, t);
            let d = &m.normals[t[0] as usize];
            assert!(
                n[0] * d[0] + n[1] * d[1] + n[2] * d[2] > 0.0,
                "triangle {t:?} faces away from its normal {d:?} (geometric {n:?})"
            );
        }
    }

    #[test]
    fn ribbons_and_markings_face_up_for_any_direction() {
        for ang in [0.0f64, 0.7, 1.9, 3.3, 5.0] {
            let (c, s) = (ang.cos(), ang.sin());
            let pts: Vec<[f64; 2]> = (0..12)
                .map(|i| [100.0 + c * i as f64 * 8.0, 100.0 + s * i as f64 * 8.0])
                .collect();
            let road = Arc::new(Road {
                id: 1,
                class: RoadClass::Highway,
                pts,
                y: vec![50.0; 12],
                bridge: vec![false; 11],
            });
            let b = Bounds {
                min_x: 0.0,
                min_z: 0.0,
                max_x: 400.0,
                max_z: 400.0,
            };
            let s = RoadSampler::build(vec![road], vec![], (0.0, 0.0, 400.0, 400.0));
            let m = build_road_mesh(&s, &b, (0.0, 0.0), 2.0, &|_, _| 40.0);
            assert!(!m.is_empty());
            assert!(
                m.props_from > 0 && m.props_from < m.indices.len(),
                "highways get delineator props after the surface"
            );
            for t in m.indices[..m.props_from].chunks(3) {
                let n = tri_normal(&m, t);
                assert!(
                    n[1] > 0.0,
                    "angle {ang}: ribbon/marking triangle faces down {n:?}"
                );
            }
            // props are boxes whose faces all point away from their centres (outward)
            for t in m.indices[m.props_from..].chunks(3) {
                let n = tri_normal(&m, t);
                let d = &m.normals[t[0] as usize];
                assert!(
                    n[0] * d[0] + n[1] * d[1] + n[2] * d[2] > 0.0,
                    "prop triangle wound against its normal"
                );
            }
        }
    }

    #[test]
    fn bridges_have_outward_faces() {
        let pts: Vec<[f64; 2]> = (0..10).map(|i| [100.0 + i as f64 * 8.0, 100.0]).collect();
        let road = Arc::new(Road {
            id: 2,
            class: RoadClass::Rural,
            pts,
            y: (0..10).map(|i| 50.0 + i as f32 * 0.2).collect(),
            bridge: vec![true; 9],
        });
        let b = Bounds {
            min_x: 0.0,
            min_z: 0.0,
            max_x: 400.0,
            max_z: 400.0,
        };
        let s = RoadSampler::build(vec![road], vec![], (0.0, 0.0, 400.0, 400.0));
        let m = build_road_mesh(&s, &b, (0.0, 0.0), 2.0, &|_, _| 30.0);
        assert!(
            m.triangle_count() > 100,
            "bridge deck, rails and piers expected"
        );
        assert_outward(&m);
        // piers reach the ground
        let lowest = m.positions.iter().map(|p| p[1]).fold(f32::MAX, f32::min);
        assert!(
            lowest < 31.0,
            "piers should reach ground, lowest y {lowest}"
        );
    }

    #[test]
    fn chunk_ownership_never_duplicates_segments() {
        let pts: Vec<[f64; 2]> = (0..40).map(|i| [10.0 + i as f64 * 8.0, 64.0]).collect();
        let road = Arc::new(Road {
            id: 3,
            class: RoadClass::Street,
            pts,
            y: vec![10.0; 40],
            bridge: vec![false; 39],
        });
        let s = RoadSampler::build(vec![road], vec![], (0.0, 0.0, 400.0, 200.0));
        let whole = build_road_mesh(
            &s,
            &Bounds {
                min_x: 0.0,
                min_z: 0.0,
                max_x: 400.0,
                max_z: 200.0,
            },
            (0.0, 0.0),
            2.0,
            &|_, _| 0.0,
        );
        let halves_surface: usize = [(0.0, 160.0), (160.0, 400.0)]
            .iter()
            .map(|&(a, b)| {
                build_road_mesh(
                    &s,
                    &Bounds {
                        min_x: a,
                        min_z: 0.0,
                        max_x: b,
                        max_z: 200.0,
                    },
                    (a, 0.0),
                    2.0,
                    &|_, _| 0.0,
                )
                .props_from
                    / 3
            })
            .sum();
        // streets have no markings: asphalt triangles are 2 per segment, so splitting must conserve them
        // (compared on the road surface: lamp props are placed per run and legitimately differ)
        let surf = |m: &RoadMeshData| m.props_from / 3;
        assert_eq!(surf(&whole), halves_surface);
    }
}
