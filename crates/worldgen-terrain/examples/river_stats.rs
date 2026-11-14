use worldgen_terrain::{TerrainParams, TerrainWorld};
fn main() {
    let w = TerrainWorld::new(TerrainParams::default());
    let mut lens = vec![];
    let mut ends = vec![];
    for set in w.rivers_around(0.0, 0.0).iter() {
        for r in set.rivers.iter() {
            lens.push(r.pts.len());
            let (x, z) = *r.pts.last().unwrap();
            ends.push(w.macro_height(x, z, 0.0));
        }
    }
    let ponds: usize = w
        .rivers_around(0.0, 0.0)
        .iter()
        .map(|s| s.ponds.len())
        .sum();
    println!("ponds {ponds}");
    lens.sort();
    println!(
        "rivers {} len min {} med {} max {}",
        lens.len(),
        lens[0],
        lens[lens.len() / 2],
        lens[lens.len() - 1]
    );
    ends.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!(
        "end heights: min {:.0} med {:.0} max {:.0}",
        ends[0],
        ends[ends.len() / 2],
        ends[ends.len() - 1]
    );
}
