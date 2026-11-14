//! Network-level properties over a large region.
use std::collections::HashSet;
use std::sync::Arc;
use worldgen_roads::{Road, RoadClass, RoadSampler, RoadWorld};
use worldgen_terrain::{Bounds, TerrainParams, TerrainWorld};

fn world() -> Arc<RoadWorld> {
    RoadWorld::new(
        TerrainWorld::new(TerrainParams::default()),
        TerrainParams::default().seed,
    )
}

fn all_highways(rw: &RoadWorld) -> Vec<Arc<Road>> {
    let b = Bounds {
        min_x: -20000.0,
        min_z: -30000.0,
        max_x: 50000.0,
        max_z: 30000.0,
    };
    let (roads, _) = rw.roads_near(&b);
    roads
        .into_iter()
        .filter(|r| matches!(r.class, RoadClass::Highway | RoadClass::Rural))
        .collect()
}

#[test]
fn highway_network_is_connected_graded_and_bridged() {
    let rw = world();
    let t0 = std::time::Instant::now();
    let hw = all_highways(&rw);
    println!(
        "{} highway/rural roads planned in {:.1} s",
        hw.len(),
        t0.elapsed().as_secs_f32()
    );
    assert!(hw.len() >= 20, "expected a network, got {}", hw.len());
    let (mut total, mut bridge_segs, mut bridges, mut max_grade) = (0.0, 0usize, 0usize, 0.0f32);
    for r in &hw {
        total += r.length();
        assert!(r.pts.len() == r.y.len() && r.bridge.len() == r.pts.len() - 1);
        for (w, p) in r.y.windows(2).zip(r.pts.windows(2)) {
            let l = ((p[1][0] - p[0][0]).powi(2) + (p[1][1] - p[0][1]).powi(2)).sqrt() as f32;
            max_grade = max_grade.max((w[1] - w[0]).abs() / l.max(0.1));
        }
        bridge_segs += r.bridge.iter().filter(|b| **b).count();
        bridges += r.bridge.windows(2).filter(|w| !w[0] && w[1]).count() + usize::from(r.bridge[0]);
    }
    println!(
        "total {:.0} km, bridges {bridges} ({bridge_segs} segments), max grade {:.1} %",
        total / 1000.0,
        max_grade * 100.0
    );
    assert!(max_grade <= 0.075, "grade limit violated: {max_grade}");
    // every highway starts and ends on a street node of a town (continuity into the town network)
    let towns = rw.towns.towns_near(-30000.0, -40000.0, 60000.0, 40000.0, 1);
    let mut nodes: Vec<(f64, f64)> = vec![];
    for t in &towns {
        nodes.extend(rw.towns.content(t).nodes.iter().map(|n| (n.0, n.1)));
    }
    let near = |p: [f64; 2]| {
        nodes
            .iter()
            .any(|n| (n.0 - p[0]).abs() < 1.0 && (n.1 - p[1]).abs() < 1.0)
    };
    let connected = hw
        .iter()
        .filter(|r| near(r.pts[0]) && near(*r.pts.last().unwrap()))
        .count();
    assert!(
        connected * 10 >= hw.len() * 9,
        "only {connected}/{} highways end on town street nodes",
        hw.len()
    );
}

#[test]
fn sampler_is_independent_of_the_asking_region_and_carves_to_profile() {
    let rw = world();
    let hw = all_highways(&rw);
    let r = hw
        .iter()
        .find(|r| r.bridge.iter().all(|b| !b) && r.pts.len() > 30)
        .expect("a plain road");
    let k = r.pts.len() / 2;
    let (x, z) = (r.pts[k][0], r.pts[k][1]);
    let mk = |pad: f64| {
        let b = Bounds {
            min_x: x - pad,
            min_z: z - pad,
            max_x: x + pad,
            max_z: z + pad,
        };
        let (roads, pads) = rw.roads_near(&b);
        RoadSampler::build(roads, pads, (b.min_x, b.min_z, b.max_x, b.max_z))
    };
    let (a, b) = (mk(300.0), mk(900.0));
    for (dx, dz) in [
        (0.0, 0.0),
        (6.0, 2.0),
        (-12.0, 9.0),
        (20.0, -15.0),
        (40.0, 33.0),
    ] {
        let (px, pz) = (x + dx, z + dz);
        assert_eq!(
            a.modify(px, pz, 100.0).to_bits(),
            b.modify(px, pz, 100.0).to_bits(),
            "carving depends on region at ({dx},{dz})"
        );
        assert_eq!(a.mask(px, pz), b.mask(px, pz));
    }
    // on the centreline the surface equals the road profile
    let h = a.modify(x, z, 500.0);
    assert!(
        (h - r.y[k]).abs() < 0.05,
        "centreline {h} vs profile {}",
        r.y[k]
    );
    assert!(a.mask(x, z) & worldgen_roads::mask::ROAD != 0);
    assert!(a.surface_y(x, z).is_some());
}

#[test]
fn towns_have_streets_lots_and_pads_and_are_deterministic() {
    let rw = world();
    let towns = rw.towns.towns_near(0.0, -20000.0, 50000.0, 20000.0, 0);
    assert!(towns.len() >= 15);
    let kinds: HashSet<u8> = towns.iter().map(|t| t.kind as u8).collect();
    assert!(kinds.len() >= 3, "town kinds {kinds:?}");
    let village = towns.iter().find(|t| t.kind as u8 >= 1).unwrap();
    let c = rw.towns.content(village);
    assert!(c.roads.len() > 5 && c.lots.len() > 10 && c.lots.len() == c.pads.len());
    let again = world().towns.content(village);
    assert_eq!(c.lots.len(), again.lots.len());
    assert!(
        c.lots
            .iter()
            .zip(&again.lots)
            .all(|(a, b)| a.cx.to_bits() == b.cx.to_bits() && a.rot.to_bits() == b.rot.to_bits())
    );
    // lots do not collide with streets
    let s = RoadSampler::build(
        c.roads.clone(),
        vec![],
        (
            village.cx - 300.0,
            village.cz - 300.0,
            village.cx + 300.0,
            village.cz + 300.0,
        ),
    );
    let bad = c
        .lots
        .iter()
        .filter(|l| s.clearance(l.cx, l.cz) < 1.0)
        .count();
    assert!(
        bad * 10 <= c.lots.len(),
        "{bad}/{} lot centres lie on a street",
        c.lots.len()
    );
}
