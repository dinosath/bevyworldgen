use worldgen_roads::{RoadClass, RoadWorld};
use worldgen_terrain::{Bounds, TerrainParams, TerrainWorld};
fn main() {
    let rw = RoadWorld::new(
        TerrainWorld::new(TerrainParams::default()),
        TerrainParams::default().seed,
    );
    let (roads, _) = rw.roads_near(&Bounds {
        min_x: -20000.0,
        min_z: -30000.0,
        max_x: 50000.0,
        max_z: 30000.0,
    });
    for r in roads
        .iter()
        .filter(|r| matches!(r.class, RoadClass::Highway | RoadClass::Rural))
    {
        if let Some(i) = r.bridge.iter().position(|b| *b) {
            let j = r.bridge[i..]
                .iter()
                .position(|b| !*b)
                .map_or(r.bridge.len(), |k| i + k);
            let m = (i + j) / 2;
            println!(
                "{:?} bridge span {} segs at ({:.0}, {:.0}) y {:.1} dir ({:.2},{:.2})",
                r.class,
                j - i,
                r.pts[m][0],
                r.pts[m][1],
                r.y[m],
                r.pts[m + 1][0] - r.pts[m][0],
                r.pts[m + 1][1] - r.pts[m][1]
            );
        }
    }
    if let Some(r) = roads
        .iter()
        .find(|r| r.class == RoadClass::Highway && r.pts.len() > 200)
    {
        let m = r.pts.len() / 2;
        println!(
            "highway sample at ({:.0}, {:.0}) y {:.1} dir ({:.2},{:.2})",
            r.pts[m][0],
            r.pts[m][1],
            r.y[m],
            r.pts[m + 1][0] - r.pts[m][0],
            r.pts[m + 1][1] - r.pts[m][1]
        );
    }
}
