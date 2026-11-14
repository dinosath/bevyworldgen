//! `hillshade x z size_m px out.pgm` : writes a hillshaded, height-tinted PGM of the terrain function.
use std::io::Write;
use worldgen_terrain::{TerrainParams, TerrainWorld};
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let (x0, z0, size, px): (f64, f64, f64, usize) = (
        a[1].parse().unwrap(),
        a[2].parse().unwrap(),
        a[3].parse().unwrap(),
        a[4].parse().unwrap(),
    );
    let w = TerrainWorld::new(TerrainParams::default());
    let step = size / px as f64;
    let ctx = w.ctx_for(x0, z0, x0 + size, z0 + size);
    let mut h = vec![0f32; (px + 2) * (px + 2)];
    let mut wet = vec![false; (px + 2) * (px + 2)];
    for j in 0..px + 2 {
        for i in 0..px + 2 {
            let s = w.sample(
                &ctx,
                x0 + (i as f64 - 1.0) * step,
                z0 + (j as f64 - 1.0) * step,
                step,
            );
            h[j * (px + 2) + i] = s.height;
            wet[j * (px + 2) + i] = s.water > f32::NEG_INFINITY;
        }
    }
    let mut out = std::fs::File::create(&a[5]).unwrap();
    write!(out, "P6\n{px} {px}\n255\n").unwrap();
    let (mut lo, mut hi) = (f32::MAX, f32::MIN);
    for v in &h {
        lo = lo.min(*v);
        hi = hi.max(*v);
    }
    for j in 1..=px {
        for i in 1..=px {
            let k = j * (px + 2) + i;
            let dx = (h[k + 1] - h[k - 1]) as f64 / (2.0 * step);
            let dz = (h[k + px + 2] - h[k - px - 2]) as f64 / (2.0 * step);
            let n = (-dx, 1.0, -dz);
            let l = (n.0 * n.0 + n.1 * n.1 + n.2 * n.2).sqrt();
            let lam = ((n.0 * -0.5 + n.1 * 0.7 + n.2 * -0.5) / l / 1.0).clamp(0.0, 1.0);
            let t = ((h[k] - lo) / (hi - lo)) as f64;
            let (r, g, b) = if wet[k] {
                (30.0, 90.0, 200.0)
            } else {
                (
                    60.0 + 190.0 * t,
                    130.0 + 90.0 * (1.0 - (t - 0.5).abs() * 2.0).max(0.0),
                    50.0 + 180.0 * t * t,
                )
            };
            let f = 0.25 + 0.85 * lam;
            out.write_all(&[
                (r * f).min(255.0) as u8,
                (g * f).min(255.0) as u8,
                (b * f).min(255.0) as u8,
            ])
            .unwrap();
        }
    }
    println!("height range {lo:.0}..{hi:.0}");
}
