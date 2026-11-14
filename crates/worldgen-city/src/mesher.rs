//! Terminal meshing and per-building baking (engine-free vertex buffers).

use crate::{Building, Kind};
use symbios_shape::Vec3 as DVec3;
use symbios_shape::{FaceProfile, Scope, Terminal};

/// Window panes, kept apart from the solid mesh so they can use an emissive material at night.
#[derive(Default)]
pub struct GlassPart {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub indices: Vec<u32>,
}

impl GlassPart {
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }
}

#[derive(Default)]
pub struct BuildingMeshData {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
    /// Panes that are lit at night.
    pub lit: GlassPart,
    /// Panes that stay dark at night.
    pub dark: GlassPart,
}

impl BuildingMeshData {
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty() && self.lit.is_empty() && self.dark.is_empty()
    }
    pub fn triangle_count(&self) -> usize {
        (self.indices.len() + self.lit.indices.len() + self.dark.indices.len()) / 3
    }
}

/// The 8 corners of a scope in its parent frame.
pub(crate) fn corners(s: &Scope) -> [DVec3; 8] {
    let mut out = [DVec3::ZERO; 8];
    for (k, o) in out.iter_mut().enumerate() {
        *o = s.world_point((k & 1) as f64, ((k >> 1) & 1) as f64, ((k >> 2) & 1) as f64);
    }
    out
}

fn linear(c: f32) -> f32 {
    c.clamp(0.0, 1.0).powf(2.2)
}

fn material_colour(id: &str) -> [f32; 3] {
    match id {
        "Brick" => [0.50, 0.22, 0.16],
        "Stucco" => [0.80, 0.76, 0.66],
        "Wood" => [0.42, 0.29, 0.17],
        "WoodDark" => [0.22, 0.15, 0.10],
        "Adobe" => [0.74, 0.58, 0.40],
        "Concrete" => [0.56, 0.56, 0.54],
        "Tile" => [0.55, 0.25, 0.18],
        "Slate" => [0.22, 0.24, 0.27],
        "Shingle" => [0.26, 0.20, 0.17],
        "Terracotta" => [0.70, 0.34, 0.18],
        "Glass" => [0.09, 0.17, 0.24],
        "Door" => [0.20, 0.12, 0.08],
        "Trim" => [0.75, 0.74, 0.70],
        _ => [0.6, 0.6, 0.6],
    }
}

struct Place {
    cx: f64,
    cz: f64,
    y: f64,
    hw: f64,
    hd: f64,
    c: f64,
    s: f64,
    ox: f64,
    oz: f64,
    tint: f32,
}

impl Place {
    fn new(b: &Building, origin: (f64, f64)) -> Self {
        let (s, c) = (b.rot as f64).sin_cos();
        let h = (b.seed >> 8) as u32;
        let tint = 0.92 + 0.16 * ((h.wrapping_mul(2654435761) >> 8) as f32 / (1u32 << 24) as f32);
        Self {
            cx: b.cx,
            cz: b.cz,
            y: b.y as f64,
            hw: b.w as f64 * 0.5,
            hd: b.d as f64 * 0.5,
            c,
            s,
            ox: origin.0,
            oz: origin.1,
            tint,
        }
    }
    /// building-local point -> chunk-relative f32
    fn p(&self, p: DVec3) -> [f32; 3] {
        let (x, z) = (p.x - self.hw, p.z - self.hd);
        [
            (self.cx + x * self.c - z * self.s - self.ox) as f32,
            (self.y + p.y) as f32,
            (self.cz + x * self.s + z * self.c - self.oz) as f32,
        ]
    }
    fn n(&self, n: DVec3) -> [f32; 3] {
        let l = n.length().max(1e-12);
        let n = n / l;
        [
            (n.x * self.c - n.z * self.s) as f32,
            n.y as f32,
            (n.x * self.s + n.z * self.c) as f32,
        ]
    }
}

impl BuildingMeshData {
    fn quad(&mut self, pl: &Place, q: [DVec3; 4], nominal: DVec3, col: [f32; 3]) {
        let g = (q[1] - q[0]).cross(q[3] - q[0]);
        let n = pl.n(nominal);
        let base = self.positions.len() as u32;
        for p in q {
            self.positions.push(pl.p(p));
            self.normals.push(n);
            self.colors.push([
                linear(col[0] * pl.tint),
                linear(col[1] * pl.tint),
                linear(col[2] * pl.tint),
                1.0,
            ]);
        }
        if g.dot(nominal) >= 0.0 {
            self.indices
                .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        } else {
            self.indices
                .extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
        }
    }

    fn tri_double(&mut self, pl: &Place, t: [DVec3; 3], nominal: DVec3, col: [f32; 3]) {
        for flip in [false, true] {
            let nom = if flip { -nominal } else { nominal };
            let g = (t[1] - t[0]).cross(t[2] - t[0]);
            let n = pl.n(nom);
            let base = self.positions.len() as u32;
            for p in t {
                self.positions.push(pl.p(p));
                self.normals.push(n);
                self.colors.push([
                    linear(col[0] * pl.tint),
                    linear(col[1] * pl.tint),
                    linear(col[2] * pl.tint),
                    1.0,
                ]);
            }
            if g.dot(nom) >= 0.0 {
                self.indices.extend_from_slice(&[base, base + 1, base + 2]);
            } else {
                self.indices.extend_from_slice(&[base, base + 2, base + 1]);
            }
        }
    }
}

const FACES: [([f64; 3], [f64; 3], [f64; 3]); 5] = [
    ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
    ([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
    ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
    ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
    ([0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]),
];

fn terminal_colour(t: &Terminal) -> [f32; 3] {
    match (&t.material, t.mesh_id.as_str()) {
        (Some(m), _) => material_colour(&m.id),
        (None, "Pane") => material_colour("Glass"),
        (None, "Door") => material_colour("Door"),
        _ => [0.62, 0.6, 0.58],
    }
}

fn emit_terminal(m: &mut BuildingMeshData, pl: &Place, t: &Terminal) {
    if t.mesh_id == "Pane" {
        emit_pane(m, pl, t);
        return;
    }
    let s = &t.scope;
    let col = terminal_colour(t);
    let sgn = DVec3::new(s.size.x.signum(), s.size.y.signum(), s.size.z.signum());
    let dims = [s.size.x.abs(), s.size.y.abs(), s.size.z.abs()];
    let thin = dims.iter().filter(|d| **d < 1e-6).count();
    if thin >= 2 {
        return;
    }
    let pt = |u: f64, v: f64, w: f64| s.world_point(u + 0.5, v + 0.5, w + 0.5);
    match &t.face_profile {
        FaceProfile::Rectangle | FaceProfile::Taper(_) => {
            let taper = if let FaceProfile::Taper(k) = &t.face_profile {
                *k
            } else {
                0.0
            };
            // CGA extrudes thin slabs outward along +Z of the face scope, so the -Z face of a thin
            // slab is the hidden inner side: skip it (halves wall geometry)
            let thin_z = dims[2] < 0.5 && dims[2] <= dims[0].min(dims[1]);
            for (n, u, v) in FACES {
                if thin_z && n[2] < 0.0 {
                    continue;
                }
                let c = |su: f64, sv: f64| {
                    let mut q = [
                        n[0] * 0.5 + u[0] * su * 0.5 + v[0] * sv * 0.5,
                        n[1] * 0.5 + u[1] * su * 0.5 + v[1] * sv * 0.5,
                        n[2] * 0.5 + u[2] * su * 0.5 + v[2] * sv * 0.5,
                    ];
                    if q[1] > 0.0 && taper > 0.0 {
                        q[0] *= 1.0 - taper;
                        q[2] *= 1.0 - taper;
                    }
                    pt(q[0], q[1], q[2])
                };
                // skip faces that have no area (flat scopes)
                let quad = [c(-1.0, -1.0), c(1.0, -1.0), c(1.0, 1.0), c(-1.0, 1.0)];
                if (quad[1] - quad[0]).cross(quad[3] - quad[0]).length() < 1e-9 {
                    continue;
                }
                let nominal = s.rotation * DVec3::new(n[0] * sgn.x, n[1] * sgn.y, n[2] * sgn.z);
                m.quad(pl, quad, nominal, col);
            }
        }
        FaceProfile::Triangle { peak_offset } => {
            let po = peak_offset.clamp(0.0, 1.0);
            let nominal = s.rotation * DVec3::new(0.0, 0.0, sgn.z);
            m.tri_double(
                pl,
                [
                    pt(-0.5, -0.5, 0.0),
                    pt(0.5, -0.5, 0.0),
                    pt(po - 0.5, 0.5, 0.0),
                ],
                nominal,
                col,
            );
        }
        FaceProfile::Trapezoid {
            top_width,
            offset_x,
        } => {
            let tw = top_width.clamp(0.0, 1.0);
            let ox = offset_x.clamp(0.0, (1.0 - tw).max(0.0));
            let nominal = s.rotation * DVec3::new(0.0, 0.0, sgn.z);
            let q = [
                pt(-0.5, -0.5, 0.0),
                pt(0.5, -0.5, 0.0),
                pt(ox + tw - 0.5, 0.5, 0.0),
                pt(ox - 0.5, 0.5, 0.0),
            ];
            m.tri_double(pl, [q[0], q[1], q[2]], nominal, col);
            m.tri_double(pl, [q[0], q[2], q[3]], nominal, col);
        }
        FaceProfile::Polygon(_) => {
            let nominal = s.rotation * DVec3::new(0.0, 0.0, sgn.z);
            let q = [
                pt(-0.5, -0.5, 0.0),
                pt(0.5, -0.5, 0.0),
                pt(0.5, 0.5, 0.0),
                pt(-0.5, 0.5, 0.0),
            ];
            m.quad(pl, q, nominal, col);
            m.quad(pl, [q[3], q[2], q[1], q[0]], -nominal, col);
        }
    }
}

pub(crate) fn bake_detailed(m: &mut BuildingMeshData, b: &Building, origin: (f64, f64)) {
    let pl = Place::new(b, origin);
    for t in &b.model.terminals {
        emit_terminal(m, &pl, t);
    }
}

fn first_colour(b: &Building, mesh_id: &str, fallback: [f32; 3]) -> [f32; 3] {
    b.model
        .terminals
        .iter()
        .find(|t| t.mesh_id == mesh_id && t.material.is_some())
        .map(terminal_colour)
        .unwrap_or(fallback)
}

/// Simple massing: walls box + gable roof (houses/shops/barns) or a glassy box (towers).
pub(crate) fn bake_massing(m: &mut BuildingMeshData, b: &Building, origin: (f64, f64)) {
    let pl = Place::new(b, origin);
    let (w, d) = (b.w as f64, b.d as f64);
    let h = b.height as f64;
    let wall = first_colour(b, "Wall", [0.7, 0.68, 0.62]);
    let local = |x: f64, y: f64, z: f64| DVec3::new(x, y, z);
    let boxq = |m: &mut BuildingMeshData, y0: f64, y1: f64, col: [f32; 3]| {
        // 4 side faces + top
        let (c0, c1, c2, c3) = ((0.0, 0.0), (w, 0.0), (w, d), (0.0, d));
        let sides = [
            (c0, c1, DVec3::new(0.0, 0.0, -1.0)),
            (c1, c2, DVec3::new(1.0, 0.0, 0.0)),
            (c2, c3, DVec3::new(0.0, 0.0, 1.0)),
            (c3, c0, DVec3::new(-1.0, 0.0, 0.0)),
        ];
        for (a, bb, n) in sides {
            m.quad(
                &pl,
                [
                    local(a.0, y0, a.1),
                    local(bb.0, y0, bb.1),
                    local(bb.0, y1, bb.1),
                    local(a.0, y1, a.1),
                ],
                n,
                col,
            );
        }
        m.quad(
            &pl,
            [
                local(0.0, y1, 0.0),
                local(0.0, y1, d),
                local(w, y1, d),
                local(w, y1, 0.0),
            ],
            DVec3::Y,
            col,
        );
    };
    if b.kind == Kind::Tower {
        boxq(
            m,
            0.0,
            h,
            [
                wall[0] * 0.55 + 0.1,
                wall[1] * 0.55 + 0.14,
                wall[2] * 0.55 + 0.17,
            ],
        );
        return;
    }
    let roof = first_colour(b, "RoofTile", [0.4, 0.22, 0.18]);
    let eave = (h * 0.76).max(2.5);
    boxq(m, 0.0, eave, wall);
    // gable roof along the longer axis
    let along_x = w >= d;
    let (ridge_a, ridge_b, s0, s1) = if along_x {
        (local(0.0, h, d * 0.5), local(w, h, d * 0.5), (0.0, d), 0.0)
    } else {
        (local(w * 0.5, h, 0.0), local(w * 0.5, h, d), (0.0, w), 0.0)
    };
    let _ = (s0, s1);
    if along_x {
        m.quad(
            &pl,
            [local(0.0, eave, 0.0), local(w, eave, 0.0), ridge_b, ridge_a],
            DVec3::new(0.0, 1.0, -1.0),
            roof,
        );
        m.quad(
            &pl,
            [local(w, eave, d), local(0.0, eave, d), ridge_a, ridge_b],
            DVec3::new(0.0, 1.0, 1.0),
            roof,
        );
        m.tri_double(
            &pl,
            [local(0.0, eave, 0.0), local(0.0, eave, d), ridge_a],
            DVec3::new(-1.0, 0.0, 0.0),
            wall,
        );
        m.tri_double(
            &pl,
            [local(w, eave, d), local(w, eave, 0.0), ridge_b],
            DVec3::new(1.0, 0.0, 0.0),
            wall,
        );
    } else {
        m.quad(
            &pl,
            [local(0.0, eave, d), local(0.0, eave, 0.0), ridge_a, ridge_b],
            DVec3::new(-1.0, 1.0, 0.0),
            roof,
        );
        m.quad(
            &pl,
            [local(w, eave, 0.0), local(w, eave, d), ridge_b, ridge_a],
            DVec3::new(1.0, 1.0, 0.0),
            roof,
        );
        m.tri_double(
            &pl,
            [local(0.0, eave, 0.0), local(w, eave, 0.0), ridge_a],
            DVec3::new(0.0, 0.0, -1.0),
            wall,
        );
        m.tri_double(
            &pl,
            [local(w, eave, d), local(0.0, eave, d), ridge_b],
            DVec3::new(0.0, 0.0, 1.0),
            wall,
        );
    }
}

/// A window pane: only its outward (front) face, into the lit or dark glass buffer.
fn emit_pane(m: &mut BuildingMeshData, pl: &Place, t: &Terminal) {
    let s = &t.scope;
    let pt = |u: f64, v: f64| s.world_point(u, v, 1.0);
    let q = [pt(0.0, 0.0), pt(1.0, 0.0), pt(1.0, 1.0), pt(0.0, 1.0)];
    let sgn = s.size.z.signum();
    let nominal = s.rotation * DVec3::new(0.0, 0.0, sgn);
    let g = (q[1] - q[0]).cross(q[3] - q[0]);
    // deterministic lit/dark choice per window
    let h = ((q[0].x * 31.7 + q[0].y * 17.3 + q[0].z * 53.1 + pl.cx * 0.37 + pl.cz * 0.11).abs()
        * 1000.0) as u64;
    let h = (h.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 40) as f32 / (1u64 << 24) as f32;
    let part = if h < 0.45 { &mut m.lit } else { &mut m.dark };
    let n = pl.n(nominal);
    let base = part.positions.len() as u32;
    for p in q {
        part.positions.push(pl.p(p));
        part.normals.push(n);
    }
    if g.dot(nominal) >= 0.0 {
        part.indices
            .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    } else {
        part.indices
            .extend_from_slice(&[base, base + 2, base + 1, base, base + 3, base + 2]);
    }
}
