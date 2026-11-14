use worldgen_terrain::{ChunkConfig, TerrainParams, TerrainWorld};
fn main() {
    let w = TerrainWorld::new(TerrainParams::default());
    let cfg = ChunkConfig::default();
    let s = cfg.size_m(0);
    for set in w.rivers_around(0.0, 0.0).iter().take(25) {
        for r in set.rivers.iter().take(2) {
            let m = r.pts.len() / 2;
            let (x, z) = r.pts[m];
            println!(
                "river len {} mid chunk lod0 ({}, {}) half_w {:.1} level {:.1}",
                r.pts.len(),
                (x / s).floor(),
                (z / s).floor(),
                r.half_w[m],
                r.level[m]
            );
        }
    }
}
