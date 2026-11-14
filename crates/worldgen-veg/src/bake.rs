//! Bake plant instances into one merged mesh per chunk (engine-free buffers).

use crate::{Instance, Species};
use std::sync::OnceLock;

/// Merged vegetation mesh. Positions are relative to the chunk origin.
#[derive(Default)]
pub struct VegMeshData {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub colors: Vec<[f32; 4]>,
    pub indices: Vec<u32>,
}

impl VegMeshData {
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }
}

type Ico = (Vec<[f32; 3]>, Vec<[u32; 3]>);

fn ico(level: usize) -> &'static Ico {
    static CACHE: OnceLock<[Ico; 2]> = OnceLock::new();
    &CACHE.get_or_init(|| [make_ico(0), make_ico(1)])[level.min(1)]
}

fn make_ico(subdiv: usize) -> Ico {
    let t = (1.0 + 5f32.sqrt()) / 2.0;
    let mut v: Vec<[f32; 3]> = vec![
        [-1.0, t, 0.0],
        [1.0, t, 0.0],
        [-1.0, -t, 0.0],
        [1.0, -t, 0.0],
        [0.0, -1.0, t],
        [0.0, 1.0, t],
        [0.0, -1.0, -t],
        [0.0, 1.0, -t],
        [t, 0.0, -1.0],
        [t, 0.0, 1.0],
        [-t, 0.0, -1.0],
        [-t, 0.0, 1.0],
    ];
    let mut f: Vec<[u32; 3]> = vec![
        [0, 11, 5],
        [0, 5, 1],
        [0, 1, 7],
        [0, 7, 10],
        [0, 10, 11],
        [1, 5, 9],
        [5, 11, 4],
        [11, 10, 2],
        [10, 7, 6],
        [7, 1, 8],
        [3, 9, 4],
        [3, 4, 2],
        [3, 2, 6],
        [3, 6, 8],
        [3, 8, 9],
        [4, 9, 5],
        [2, 4, 11],
        [6, 2, 10],
        [8, 6, 7],
        [9, 8, 1],
    ];
    let norm = |p: [f32; 3]| {
        let l = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
        [p[0] / l, p[1] / l, p[2] / l]
    };
    for p in v.iter_mut() {
        *p = norm(*p);
    }
    for _ in 0..subdiv {
        let mut mid = std::collections::HashMap::new();
        let mut nf = Vec::new();
        for tri in &f {
            let mut m = [0u32; 3];
            for k in 0..3 {
                let (a, b) = (tri[k], tri[(k + 1) % 3]);
                let key = (a.min(b), a.max(b));
                m[k] = *mid.entry(key).or_insert_with(|| {
                    let (pa, pb) = (v[a as usize], v[b as usize]);
                    v.push(norm([
                        (pa[0] + pb[0]) / 2.0,
                        (pa[1] + pb[1]) / 2.0,
                        (pa[2] + pb[2]) / 2.0,
                    ]));
                    (v.len() - 1) as u32
                });
            }
            nf.extend_from_slice(&[
                [tri[0], m[0], m[2]],
                [tri[1], m[1], m[0]],
                [tri[2], m[2], m[1]],
                [m[0], m[1], m[2]],
            ]);
        }
        f = nf;
    }
    (v, f)
}

struct Xf {
    ox: f32,
    oy: f32,
    oz: f32,
    c: f32,
    s: f32,
}

impl Xf {
    fn p(&self, p: [f32; 3]) -> [f32; 3] {
        [
            self.ox + p[0] * self.c + p[2] * self.s,
            self.oy + p[1],
            self.oz - p[0] * self.s + p[2] * self.c,
        ]
    }
    fn n(&self, n: [f32; 3]) -> [f32; 3] {
        [
            n[0] * self.c + n[2] * self.s,
            n[1],
            -n[0] * self.s + n[2] * self.c,
        ]
    }
}

struct B<'a> {
    m: &'a mut VegMeshData,
    xf: Xf,
}

fn hash3(p: [f32; 3], seed: f32) -> f32 {
    let v = (p[0] * 12.9898 + p[1] * 78.233 + p[2] * 37.719 + seed * 4.1414).to_bits();
    let mut h = v.wrapping_mul(0x9E37_79B1);
    h ^= h >> 15;
    h = h.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 13;
    (h >> 8) as f32 / (1u32 << 24) as f32
}

impl B<'_> {
    fn vert(&mut self, p: [f32; 3], n: [f32; 3], c: [f32; 3], shade: f32) -> u32 {
        self.m.positions.push(self.xf.p(p));
        self.m.normals.push(self.xf.n(n));
        // colours are authored in sRGB; Bevy vertex colours are linear
        let lin = |v: f32| libm::powf((v * shade).clamp(0.0, 1.0), 2.2);
        self.m.colors.push([lin(c[0]), lin(c[1]), lin(c[2]), 1.0]);
        (self.m.positions.len() - 1) as u32
    }

    /// Lumpy ellipsoid; `ao` darkens the underside.
    fn blob(
        &mut self,
        ctr: [f32; 3],
        r: [f32; 3],
        lod_level: usize,
        col: [f32; 3],
        lump: f32,
        seed: f32,
        ao: f32,
    ) {
        let (v, f) = ico(lod_level);
        let base = self.m.positions.len() as u32;
        for d in v {
            let k = 1.0 + lump * (hash3(*d, seed) - 0.5);
            let p = [
                ctr[0] + d[0] * r[0] * k,
                ctr[1] + d[1] * r[1] * k,
                ctr[2] + d[2] * r[2] * k,
            ];
            let shade = 1.0 - ao * (0.5 - 0.5 * d[1]);
            self.vert(p, *d, col, shade);
        }
        for t in f {
            self.m
                .indices
                .extend_from_slice(&[base + t[0], base + t[1], base + t[2]]);
        }
    }

    fn prism(&mut self, y0: f32, y1: f32, r0: f32, r1: f32, sides: usize, col: [f32; 3]) {
        let base = self.m.positions.len() as u32;
        for ring in 0..2 {
            let (y, r, sh) = if ring == 0 {
                (y0, r0, 0.7)
            } else {
                (y1, r1, 1.0)
            };
            for k in 0..sides {
                let a = k as f32 / sides as f32 * std::f32::consts::TAU;
                let (s, c) = (libm::sinf(a), libm::cosf(a));
                self.vert([c * r, y, s * r], [c, 0.0, s], col, sh);
            }
        }
        for k in 0..sides {
            let (a, b) = (k as u32, ((k + 1) % sides) as u32);
            let s = sides as u32;
            self.m.indices.extend_from_slice(&[
                base + a,
                base + b,
                base + s + a,
                base + b,
                base + s + b,
                base + s + a,
            ]);
        }
    }

    fn cone(&mut self, y0: f32, h: f32, r: f32, sides: usize, col: [f32; 3], seed: f32) {
        let base = self.m.positions.len() as u32;
        let slope = r / h;
        for k in 0..sides {
            let a = k as f32 / sides as f32 * std::f32::consts::TAU;
            let (s, c) = (libm::sinf(a), libm::cosf(a));
            let l = (1.0 + slope * slope).sqrt();
            self.vert(
                [c * r, y0, s * r],
                [c / l, slope / l, s / l],
                col,
                0.62 + 0.1 * hash3([c, s, y0], seed),
            );
        }
        let apex = self.vert([0.0, y0 + h, 0.0], [0.0, 1.0, 0.0], col, 1.0);
        for k in 0..sides {
            let (a, b) = (k as u32, ((k + 1) % sides) as u32);
            self.m
                .indices
                .extend_from_slice(&[base + a, base + b, apex]);
        }
        // underside is hidden by the next cone / the camera rarely sees it; omit for triangle budget
    }
}

fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn leaf_colour(sp: Species, variant: f32, autumn: f32) -> [f32; 3] {
    let v = 0.85 + 0.3 * variant;
    let mul = |c: [f32; 3]| [c[0] * v, c[1] * v, c[2] * v];
    match sp {
        Species::Oak => {
            let fall = lerp3([0.70, 0.28, 0.05], [0.78, 0.55, 0.10], variant);
            mul(lerp3(
                [0.12, 0.28, 0.06],
                fall,
                autumn * (0.55 + 0.45 * variant),
            ))
        }
        Species::Birch => mul(lerp3([0.26, 0.40, 0.10], [0.88, 0.72, 0.14], autumn)),
        Species::Spruce => mul([0.05, 0.16, 0.08]),
        Species::Pine => mul([0.09, 0.22, 0.09]),
        Species::Shrub => mul(lerp3(
            [0.17, 0.30, 0.08],
            [0.60, 0.30, 0.07],
            autumn * variant,
        )),
        Species::Boulder => {
            let g = 0.30 + 0.16 * variant;
            [g, g * 0.98, g * 0.94]
        }
    }
}

/// Bake instances. `lod` is the chunk LOD: 0 = detailed, 1 = low poly, 2 = very low poly.
pub fn bake(instances: &[Instance], lod: u8, autumn: f32) -> VegMeshData {
    let mut m = VegMeshData::default();
    let (ico_l, sides) = match lod {
        0 => (1usize, 6usize),
        1 => (0, 5),
        _ => (0, 4),
    };
    for inst in instances {
        let a = inst.yaw;
        let xf = Xf {
            ox: inst.pos[0],
            oy: inst.pos[1],
            oz: inst.pos[2],
            c: libm::cosf(a),
            s: libm::sinf(a),
        };
        let mut b = B { m: &mut m, xf };
        let h = inst.scale;
        let seed = inst.variant * 17.0;
        let leaf = leaf_colour(inst.species, inst.variant, autumn);
        let bark = [0.20, 0.14, 0.09];
        match inst.species {
            Species::Oak => {
                if lod <= 1 {
                    b.prism(-0.2, 0.46 * h, 0.040 * h, 0.022 * h, sides, bark);
                }
                b.blob(
                    [0.0, 0.64 * h, 0.0],
                    [0.34 * h, 0.30 * h, 0.34 * h],
                    ico_l,
                    leaf,
                    0.35,
                    seed,
                    0.45,
                );
                if lod == 0 {
                    b.blob(
                        [0.17 * h, 0.55 * h, 0.10 * h],
                        [0.20 * h, 0.17 * h, 0.20 * h],
                        ico_l,
                        leaf,
                        0.35,
                        seed + 1.0,
                        0.45,
                    );
                    b.blob(
                        [-0.15 * h, 0.58 * h, -0.12 * h],
                        [0.20 * h, 0.17 * h, 0.20 * h],
                        ico_l,
                        leaf,
                        0.35,
                        seed + 2.0,
                        0.45,
                    );
                }
            }
            Species::Birch => {
                if lod <= 1 {
                    b.prism(
                        -0.2,
                        0.62 * h,
                        0.022 * h,
                        0.012 * h,
                        sides.min(5),
                        [0.86, 0.85, 0.80],
                    );
                }
                b.blob(
                    [0.0, 0.74 * h, 0.0],
                    [0.20 * h, 0.26 * h, 0.20 * h],
                    ico_l,
                    leaf,
                    0.3,
                    seed,
                    0.4,
                );
            }
            Species::Spruce => {
                if lod <= 1 {
                    b.prism(-0.2, 0.2 * h, 0.03 * h, 0.02 * h, 4, bark);
                }
                let tiers = if lod == 0 { 5 } else { 3 };
                for i in 0..tiers {
                    let f = i as f32 / tiers as f32;
                    b.cone(
                        (0.10 + 0.74 * f * (1.0 - 0.05)) * h,
                        (0.30 - 0.12 * f) * h * (5.0 / tiers as f32).min(1.6),
                        (0.30 - 0.22 * f) * h * 0.9,
                        sides + 2,
                        leaf,
                        seed + i as f32,
                    );
                }
            }
            Species::Pine => {
                if lod <= 1 {
                    b.prism(
                        -0.2,
                        0.74 * h,
                        0.028 * h,
                        0.016 * h,
                        sides.min(5),
                        [0.30, 0.18, 0.10],
                    );
                }
                b.blob(
                    [0.0, 0.80 * h, 0.0],
                    [0.24 * h, 0.15 * h, 0.24 * h],
                    ico_l,
                    leaf,
                    0.4,
                    seed,
                    0.4,
                );
                if lod == 0 {
                    b.blob(
                        [0.10 * h, 0.66 * h, 0.06 * h],
                        [0.15 * h, 0.1 * h, 0.15 * h],
                        ico_l,
                        leaf,
                        0.4,
                        seed + 1.0,
                        0.4,
                    );
                }
            }
            Species::Shrub => {
                b.blob(
                    [0.0, 0.32 * h, 0.0],
                    [0.62 * h, 0.42 * h, 0.62 * h],
                    ico_l,
                    leaf,
                    0.5,
                    seed,
                    0.4,
                );
            }
            Species::Boulder => {
                b.blob(
                    [0.0, 0.15 * h, 0.0],
                    [1.0 * h, 0.7 * h, 0.85 * h],
                    ico_l.max(1),
                    leaf,
                    0.55,
                    seed,
                    0.25,
                );
            }
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inst(sp: Species) -> Instance {
        Instance {
            pos: [10.0, 5.0, 20.0],
            yaw: 0.7,
            scale: 12.0,
            species: sp,
            variant: 0.4,
        }
    }

    #[test]
    fn every_species_bakes_valid_meshes_at_every_lod() {
        for sp in [
            Species::Oak,
            Species::Spruce,
            Species::Pine,
            Species::Birch,
            Species::Shrub,
            Species::Boulder,
        ] {
            let mut last = usize::MAX;
            for lod in 0..3u8 {
                let m = bake(&[inst(sp)], lod, 0.3);
                assert!(!m.is_empty());
                assert_eq!(m.positions.len(), m.normals.len());
                assert_eq!(m.positions.len(), m.colors.len());
                assert!(m.indices.iter().all(|i| (*i as usize) < m.positions.len()));
                assert_eq!(m.indices.len() % 3, 0);
                assert!(
                    m.triangle_count() <= last,
                    "{sp:?} lod {lod} must not be heavier than the previous lod"
                );
                last = m.triangle_count();
                // instance placed near (10, 5, 20): all vertices within a tree-sized radius
                for p in &m.positions {
                    assert!(
                        (p[0] - 10.0).abs() < 30.0
                            && (p[2] - 20.0).abs() < 30.0
                            && p[1] > 5.0 - 15.0
                            && p[1] < 5.0 + 30.0
                    );
                }
            }
        }
    }

    #[test]
    fn baking_is_deterministic() {
        let v = [inst(Species::Oak), inst(Species::Spruce)];
        let (a, b) = (bake(&v, 0, 0.5), bake(&v, 0, 0.5));
        assert_eq!(a.positions, b.positions);
        assert_eq!(a.indices, b.indices);
    }
}
