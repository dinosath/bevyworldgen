//! Procedural architecture (Layer A). A building is a pure function of its lot: the lot seed drives
//! the CGA derivation, the town and its climate pick the district kind and regional style.

mod mesher;
pub mod styles;

pub use mesher::{BuildingMeshData, GlassPart};
pub use styles::{Kind, Region};

use std::cell::RefCell;
use std::collections::HashMap;
use symbios_shape::grammar::parse_rule;
use symbios_shape::{Interpreter, Quat, Scope, ShapeModel, Vec3};
use worldgen_roads::{Lot, Town, TownKind};

/// A derived building, ready to bake at any LOD.
pub struct Building {
    pub cx: f64,
    pub cz: f64,
    pub y: f32,
    pub rot: f32,
    pub w: f32,
    pub d: f32,
    pub kind: Kind,
    pub region: Region,
    pub height: f32,
    pub seed: u64,
    model: ShapeModel,
}

impl Building {
    pub fn terminal_count(&self) -> usize {
        self.model.terminals.len()
    }
}

thread_local! {
    static CACHE: RefCell<HashMap<(Kind, Region, u32, u32), Interpreter>> = RefCell::new(HashMap::new());
}

/// District kind for a lot from its town, normalised distance from the centre and a random draw.
pub fn kind_for(town: TownKind, dist01: f32, area: f32, r: f32) -> (Kind, (u32, u32)) {
    match town {
        TownKind::City if dist01 < 0.28 && r < 0.8 => (Kind::Tower, (34, 78)),
        TownKind::City if dist01 < 0.55 && r < 0.55 => (Kind::Tower, (18, 40)),
        TownKind::City if dist01 < 0.75 && r < 0.6 => (Kind::Shop, (0, 0)),
        TownKind::Town if dist01 < 0.22 && r < 0.35 => (Kind::Tower, (14, 24)),
        TownKind::Town if dist01 < 0.5 && r < 0.65 => (Kind::Shop, (0, 0)),
        TownKind::Village if dist01 < 0.25 && r < 0.45 => (Kind::Shop, (0, 0)),
        TownKind::Hamlet | TownKind::Village if dist01 > 0.45 && r < 0.25 && area > 180.0 => {
            (Kind::Barn, (0, 0))
        }
        _ => (Kind::House, (0, 0)),
    }
}

/// Derive the building on a lot. `temp_c`/`precip_mm` are the climate at the town.
pub fn derive(lot: &Lot, town: &Town, temp_c: f32, precip_mm: f32) -> Option<Building> {
    let region = Region::from_climate(temp_c, precip_mm);
    let half = town.half_patch();
    let dist01 = ((((lot.cx - town.cx).powi(2) + (lot.cz - town.cz).powi(2)).sqrt()) / half) as f32;
    let r = (lot.seed >> 40) as f32 / (1u64 << 24) as f32;
    let (kind, tall) = kind_for(town.kind, dist01, lot.w * lot.d, r);
    let model = derive_model(kind, region, tall, lot).or_else(|| {
        (kind != Kind::House)
            .then(|| derive_model(Kind::House, region, (0, 0), lot))
            .flatten()
    })?;
    let kind = if model.terminals.is_empty() {
        return None;
    } else {
        kind
    };
    let mut height = 0.0f32;
    for t in &model.terminals {
        for c in mesher::corners(&t.scope) {
            height = height.max(c.y as f32);
        }
    }
    Some(Building {
        cx: lot.cx,
        cz: lot.cz,
        y: lot.y,
        rot: lot.rot,
        w: lot.w,
        d: lot.d,
        kind,
        region,
        height,
        seed: lot.seed,
        model,
    })
}

fn derive_model(kind: Kind, region: Region, tall: (u32, u32), lot: &Lot) -> Option<ShapeModel> {
    CACHE.with(|c| {
        let mut c = c.borrow_mut();
        let interp = c.entry((kind, region, tall.0, tall.1)).or_insert_with(|| {
            let mut i = Interpreter::new();
            for line in styles::grammar(kind, region, tall).lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with("//") {
                    continue;
                }
                if let Ok(rule) = parse_rule(line) {
                    let _ = i.add_grammar_rule(rule);
                }
            }
            i
        });
        interp.seed = lot.seed;
        let scope = Scope::new(
            Vec3::ZERO,
            Quat::IDENTITY,
            Vec3::new(lot.w as f64, 0.0, lot.d as f64),
        );
        interp.derive(scope, "Lot").ok()
    })
}

/// Bake the buildings that belong to one chunk into a merged mesh (positions relative to
/// `origin`). LOD 0: full detail; 1: massing; 2-3: tall buildings only; beyond: nothing.
pub fn bake(buildings: &[&Building], origin: (f64, f64), lod: u8) -> BuildingMeshData {
    let mut m = BuildingMeshData::default();
    for b in buildings {
        match lod {
            0 => mesher::bake_detailed(&mut m, b, origin),
            1 => mesher::bake_massing(&mut m, b, origin),
            2 | 3 if b.height > 14.0 => mesher::bake_massing(&mut m, b, origin),
            _ => {}
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lot(seed: u64) -> Lot {
        Lot {
            cx: 100.0,
            cz: 200.0,
            rot: 0.4,
            w: 16.0,
            d: 22.0,
            y: 30.0,
            seed,
        }
    }

    #[test]
    fn every_kind_and_region_derives_nonempty_models() {
        for kind in [Kind::House, Kind::Shop, Kind::Tower, Kind::Barn] {
            for region in [Region::Temperate, Region::Cold, Region::Dry] {
                for seed in 0..12u64 {
                    let m = derive_model(kind, region, (24, 50), &lot(seed * 7919 + 1))
                        .unwrap_or_else(|| {
                            panic!("{kind:?}/{region:?} seed {seed} failed to derive")
                        });
                    assert!(
                        m.terminals.len() >= 6,
                        "{kind:?}/{region:?}: only {} terminals",
                        m.terminals.len()
                    );
                    assert!(
                        m.terminals.len() < 1500,
                        "{kind:?}/{region:?}: {} terminals is too heavy",
                        m.terminals.len()
                    );
                }
            }
        }
    }

    #[test]
    fn derivation_is_deterministic_and_seed_sensitive() {
        let a = derive_model(Kind::House, Region::Temperate, (0, 0), &lot(5)).unwrap();
        let b = derive_model(Kind::House, Region::Temperate, (0, 0), &lot(5)).unwrap();
        assert_eq!(a.terminals.len(), b.terminals.len());
        let distinct: std::collections::HashSet<usize> = (0..30u64)
            .map(|s| {
                derive_model(Kind::House, Region::Temperate, (0, 0), &lot(s * 31 + 3))
                    .unwrap()
                    .terminals
                    .len()
            })
            .collect();
        assert!(
            distinct.len() > 3,
            "seeds should vary buildings: {distinct:?}"
        );
    }

    #[test]
    fn towers_are_tall_and_houses_are_low() {
        let t = derive_model(Kind::Tower, Region::Temperate, (40, 60), &lot(9)).unwrap();
        let h = derive_model(Kind::House, Region::Temperate, (0, 0), &lot(9)).unwrap();
        let top = |m: &ShapeModel| {
            m.terminals
                .iter()
                .flat_map(|t| mesher::corners(&t.scope))
                .map(|c| c.y)
                .fold(0.0, f64::max)
        };
        assert!(top(&t) >= 38.0, "tower height {}", top(&t));
        assert!(top(&h) < 16.0, "house height {}", top(&h));
    }
}

#[cfg(test)]
mod mesh_tests {
    use super::*;
    use worldgen_roads::{Town, TownKind};

    fn town(kind: TownKind) -> Town {
        Town {
            cell: (1, 1),
            cx: 100.0,
            cz: 200.0,
            kind,
        }
    }

    fn building(kind_town: TownKind, seed: u64, w: f32, d: f32, offset: f64) -> Building {
        let lot = Lot {
            cx: 100.0 + offset,
            cz: 200.0,
            rot: 0.9,
            w,
            d,
            y: 12.0,
            seed,
        };
        derive(&lot, &town(kind_town), 12.0, 800.0).expect("building")
    }

    fn tri_n(m: &BuildingMeshData, t: &[u32]) -> [f32; 3] {
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

    #[test]
    fn winding_matches_stored_normals_and_walls_face_outward() {
        for (tk, w, d) in [
            (TownKind::Village, 14.0, 20.0),
            (TownKind::City, 22.0, 26.0),
            (TownKind::Hamlet, 12.0, 18.0),
        ] {
            for seed in 1..8u64 {
                let b = building(tk, seed * 977, w, d, 0.0);
                for lod in [0u8, 1] {
                    let m = bake(&[&b], (0.0, 0.0), lod);
                    assert!(!m.is_empty());
                    assert!(m.indices.iter().all(|i| (*i as usize) < m.positions.len()));
                    let (mut bad, mut walls, mut walls_out) = (0, 0, 0);
                    for t in m.indices.chunks(3) {
                        let g = tri_n(&m, t);
                        let n = m.normals[t[0] as usize];
                        if g[0] * n[0] + g[1] * n[1] + g[2] * n[2] <= 0.0 {
                            bad += 1;
                        }
                        let area = 0.5 * (g[0] * g[0] + g[1] * g[1] + g[2] * g[2]).sqrt();
                        if n[1].abs() < 0.2 && area > 1.5 {
                            walls += 1;
                            let c = t.iter().fold([0.0f32; 2], |a, i| {
                                [
                                    a[0] + m.positions[*i as usize][0] / 3.0,
                                    a[1] + m.positions[*i as usize][2] / 3.0,
                                ]
                            });
                            let (dx, dz) = (c[0] - 100.0, c[1] - 200.0);
                            if dx * n[0] + dz * n[2] > -0.5 {
                                walls_out += 1;
                            }
                        }
                    }
                    assert_eq!(
                        bad, 0,
                        "{bad} triangles are wound against their normal (kind {:?}, lod {lod})",
                        b.kind
                    );
                    // massing gables are double-sided triangles, so half of those face inward by design
                    let need = if lod == 0 { 90 } else { 70 };
                    assert!(
                        walls > 0 && walls_out * 100 >= walls * need,
                        "only {walls_out}/{walls} wall triangles face outward ({:?} lod {lod})",
                        b.kind
                    );
                }
            }
        }
    }

    #[test]
    fn lod_detail_decreases_and_far_lods_keep_only_tall_buildings() {
        let tower = building(TownKind::City, 3, 24.0, 28.0, 0.0);
        let house = building(TownKind::Village, 4, 14.0, 18.0, 40.0);
        let set = [&tower, &house];
        let (l0, l1, l2, l4) = (
            bake(&set, (0.0, 0.0), 0),
            bake(&set, (0.0, 0.0), 1),
            bake(&set, (0.0, 0.0), 2),
            bake(&set, (0.0, 0.0), 4),
        );
        let (t0, t1, t2) = (
            l0.triangle_count(),
            l1.triangle_count(),
            l2.triangle_count(),
        );
        println!(
            "tower {:?} h {:.1}, house {:?} h {:.1}: tris lod0 {t0} lod1 {t1} lod2 {t2}",
            tower.kind, tower.height, house.kind, house.height
        );
        assert!(t0 > t1, "lod0 {t0} must exceed lod1 {t1}");
        assert!(t1 >= t2, "lod1 {t1} must not be below lod2 {t2}");
        assert!(l4.is_empty());
    }

    #[test]
    fn detailed_building_is_light_enough_to_stream() {
        let b = building(TownKind::Town, 11, 15.0, 20.0, 0.0);
        let m = bake(&[&b], (0.0, 0.0), 0);
        assert!(
            m.triangle_count() < 2500,
            "{} triangles per building is too heavy",
            m.triangle_count()
        );
        println!(
            "detailed {:?}: {} terminals, {} tris, height {:.1} m",
            b.kind,
            b.terminal_count(),
            m.triangle_count(),
            b.height
        );
    }
}
