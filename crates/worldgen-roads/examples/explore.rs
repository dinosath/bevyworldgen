use std::time::Instant;
use worldgen_roads::RoadWorld;
use worldgen_terrain::{Bounds, TerrainParams, TerrainWorld};
fn main() {
    let t = TerrainWorld::new(TerrainParams::default());
    let rw = RoadWorld::new(t.clone(), 0x5EED_0001);
    let t0 = Instant::now();
    let towns = rw.towns.towns_near(-30000.0, -30000.0, 60000.0, 60000.0, 0);
    println!(
        "towns in 90km square: {} ({} ms)",
        towns.len(),
        t0.elapsed().as_millis()
    );
    let mut kinds = [0; 4];
    for tw in &towns {
        kinds[tw.kind as usize] += 1;
    }
    println!("kinds hamlet/village/town/city: {kinds:?}");
    for tw in towns.iter().take(5) {
        let t1 = Instant::now();
        let c = rw.towns.content(tw);
        println!(
            "{:?} at ({:.0},{:.0}): roads {} lots {} nodes {} in {} ms",
            tw.kind,
            tw.cx,
            tw.cz,
            c.roads.len(),
            c.lots.len(),
            c.nodes.len(),
            t1.elapsed().as_millis()
        );
    }
    if let Some(tw) = towns.first() {
        let b = Bounds {
            min_x: tw.cx - 2000.0,
            min_z: tw.cz - 2000.0,
            max_x: tw.cx + 2000.0,
            max_z: tw.cz + 2000.0,
        };
        let t2 = Instant::now();
        let (roads, pads) = rw.roads_near(&b);
        let hw = roads
            .iter()
            .filter(|r| {
                matches!(
                    r.class,
                    worldgen_roads::RoadClass::Highway | worldgen_roads::RoadClass::Rural
                )
            })
            .count();
        let bridges: usize = roads
            .iter()
            .map(|r| r.bridge.iter().filter(|b| **b).count())
            .sum();
        println!(
            "roads_near 4km: {} roads ({} highway/rural) bridge segs {} pads {} in {} ms",
            roads.len(),
            hw,
            bridges,
            pads.len(),
            t2.elapsed().as_millis()
        );
    }
}
