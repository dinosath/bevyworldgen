use worldgen_terrain::{TerrainParams, TerrainWorld};
fn main() {
    let w = TerrainWorld::new(TerrainParams::default());
    let (x0, z0) = (-12400.0, 33400.0);
    let ctx = w.ctx_for(x0 - 600.0, z0 - 600.0, x0 + 2600.0, z0 + 2600.0);
    println!(
        "lakes {} river segs {}",
        ctx.lakes.len(),
        ctx.rivers.as_ref().unwrap().segment_count()
    );
    let mut worst = (0.0f32, 0.0, 0.0);
    for j in 0..200 {
        let row: Vec<f32> = (0..200)
            .map(|i| w.height(&ctx, x0 + i as f64 * 10.0, z0 + j as f64 * 10.0, 10.0))
            .collect();
        for i in 1..200 {
            let s = (row[i] - row[i - 1]).abs() / 10.0;
            if s > worst.0 {
                worst = (s, x0 + i as f64 * 10.0, z0 + j as f64 * 10.0);
            }
        }
    }
    println!(
        "steepest slope {:.2} at {:.0},{:.0}",
        worst.0, worst.1, worst.2
    );
    let at = |x: f64, z: f64| {
        let s = w.sample(&ctx, x, z, 2.0);
        (s.height, s.water)
    };
    for k in -4..=4 {
        let x = worst.1 + k as f64 * 6.0;
        println!("x={:.0} {:?}", x, at(x, worst.2));
    }
    let no_river = TerrainParams {
        rivers: false,
        lakes: false,
        ..Default::default()
    };
    let w2 = TerrainWorld::new(no_river);
    let c2 = w2.ctx_for(0.0, 0.0, 1.0, 1.0);
    println!(
        "without rivers/lakes at worst: {:.1}  with: {:.1}",
        w2.height(&c2, worst.1, worst.2, 2.0),
        w.height(&ctx, worst.1, worst.2, 2.0)
    );
}
