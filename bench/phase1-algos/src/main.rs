//! Phase-1 headless baseline for the Symbios generation crates (the algorithms that Symbios
//! Ground Lab drives). No Bevy, no GPU: this measures the CPU generation paths only, using
//! the same defaults as `symbios-ground-lab` (`TerrainConfig::default`, `UrbanConfig::default`,
//! the lab's default CGA grammar).
//!
//! Usage: `phase1-algos <scenario> [args]`; each scenario prints `key=value` lines.
//! Run one scenario per process so peak RSS (`VmHWM`) is attributable.

use std::time::Instant;

use symbios_ground::{
    DiamondSquare, FbmNoise, HeightMap, HydraulicErosion, TerrainGenerator, ThermalErosion,
    TiledHeightMap, VoronoiTerracing,
};

fn rss_mb() -> f64 {
    let s = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let get = |k: &str| {
        s.lines()
            .find(|l| l.starts_with(k))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.0)
            / 1024.0
    };
    get("VmHWM:")
}

/// FNV-1a over the raw f32 bit patterns: a bitwise fingerprint of a heightmap.
fn fingerprint(hm: &HeightMap) -> u64 {
    let mut h = 0xcbf29ce484222325u64;
    for v in hm.data() {
        for b in v.to_bits().to_le_bytes() {
            h ^= b as u64;
            h = h.wrapping_mul(0x100000001b3);
        }
    }
    h
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn base_fbm(size: usize, scale: f32, seed: u64) -> HeightMap {
    let mut hm = HeightMap::new(size, size, scale);
    FbmNoise {
        seed,
        octaves: 6,
        persistence: 0.5,
        lacunarity: 2.0,
        base_frequency: 4.0,
    }
    .generate(&mut hm);
    hm.normalize();
    for v in hm.data_mut() {
        *v *= 40.0;
    }
    hm
}

fn scenario_generators(size: usize) {
    let t = Instant::now();
    let hm = base_fbm(size, 1.0, 42);
    println!("fbm_ms={:.2} fp={:016x}", ms(t), fingerprint(&hm));

    let mut hm = HeightMap::new(size, size, 1.0);
    let t = Instant::now();
    DiamondSquare::new(42, 0.5).generate(&mut hm);
    println!("diamond_square_ms={:.2}", ms(t));

    let mut hm = HeightMap::new(size, size, 1.0);
    let t = Instant::now();
    VoronoiTerracing::new(42, 64, 8).generate(&mut hm);
    println!("voronoi_ms={:.2}", ms(t));
}

fn scenario_erosion(size: usize, drops: u32, thermal_iters: u32) {
    let mut hm = base_fbm(size, 1.0, 42);
    let before = fingerprint(&hm);
    let t = Instant::now();
    HydraulicErosion {
        num_drops: drops,
        water_level: 0.2 * 40.0,
        ..HydraulicErosion::new(42)
    }
    .erode(&mut hm);
    println!(
        "hydraulic_ms={:.2} drops={} cells={} lakes={}",
        ms(t),
        drops,
        size * size,
        hm.lakes().len()
    );
    let t = Instant::now();
    ThermalErosion::new()
        .with_iterations(thermal_iters)
        .with_talus_angle(0.05)
        .with_water_level(0.2 * 40.0)
        .with_underwater_talus_angle(0.01)
        .erode(&mut hm);
    println!("thermal_ms={:.2} iters={}", ms(t), thermal_iters);
    println!("changed={}", before != fingerprint(&hm));
}

fn scenario_roads(size: usize) {
    use symbios_tensor::{
        LotConfig, RationalizeConfig, RoadMeshConfig, SkirtConfig, TensorConfig, carve_lots,
        carve_roads, extract_blocks, extract_lots, generate_road_meshes, generate_roads,
        prune_unused_roads, rationalize_graph,
    };
    let mut hm = base_fbm(size, 1.0, 42);
    let wl = 0.2 * 40.0;
    let tcfg = TensorConfig {
        water_level: wl,
        ..TensorConfig::default()
    };
    let lcfg = LotConfig {
        max_lot_area: 650.0,
        min_lot_area: 130.0,
        front_setback: 2.5,
        side_setback: 1.0,
        rear_setback: 2.0,
        min_width: 9.0,
        min_depth: 9.0,
        water_level: wl,
        ..LotConfig::default()
    };
    let t = Instant::now();
    let mut g = generate_roads(&hm, &tcfg).expect("roads");
    println!(
        "generate_roads_ms={:.2} nodes={} edges={}",
        ms(t),
        g.nodes.len(),
        g.edges.len()
    );
    let t = Instant::now();
    rationalize_graph(&mut g, &hm, &RationalizeConfig::default());
    println!("rationalize_ms={:.2}", ms(t));
    let t = Instant::now();
    extract_blocks(&mut g);
    println!("extract_blocks_ms={:.2} blocks={}", ms(t), g.blocks.len());
    let t = Instant::now();
    let lots = extract_lots(&g, &mut hm, &lcfg);
    println!("extract_lots_ms={:.2} lots={}", ms(t), lots.len());
    let t = Instant::now();
    prune_unused_roads(&mut g, &lots);
    println!("prune_ms={:.2} edges_after={}", ms(t), g.edges.len());
    let rmc = RoadMeshConfig {
        major_half_width: 3.0,
        minor_half_width: 2.0,
        hub_sides: 8,
        depth_bias: 0.05,
        texture_scale: 0.1,
        spline_subdivisions: 8,
        curb_radius: 2.0,
        skirt: SkirtConfig {
            width: 3.0,
            bury_depth: 0.5,
        },
    };
    let t = Instant::now();
    let surface = carve_roads(&g, &mut hm, &rmc, 10.0);
    println!("carve_roads_ms={:.2}", ms(t));
    let t = Instant::now();
    carve_lots(&lots, &mut hm, 10.0, Some(&surface));
    println!("carve_lots_ms={:.2}", ms(t));
    let t = Instant::now();
    let _m = generate_road_meshes(&g, &hm, &rmc);
    println!("road_meshes_ms={:.2}", ms(t));
}

fn scenario_buildings(count: usize) {
    use symbios_shape::grammar::parse_rule;
    use symbios_shape::{Interpreter, Quat, Scope, Vec3};
    let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/lab_default.cga"))
        .expect("grammar");
    let mut interp = Interpreter::new();
    let mut parsed = 0;
    let t = Instant::now();
    for line in src.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("//") {
            continue;
        }
        match parse_rule(line) {
            Ok(rule) => {
                interp.add_grammar_rule(rule).ok();
                parsed += 1;
            }
            Err(e) => eprintln!("parse fail: {line}: {e:?}"),
        }
    }
    println!("grammar_parse_ms={:.2} rules={}", ms(t), parsed);
    let mut terminals = 0usize;
    let mut per = Vec::with_capacity(count);
    let t = Instant::now();
    for i in 0..count {
        interp.seed = (i as u64) * 12345 + 99;
        let scope = Scope::new(Vec3::ZERO, Quat::IDENTITY, Vec3::new(20.0, 0.0, 28.0));
        let t1 = Instant::now();
        let model = interp.derive(scope, "Lot").expect("derive");
        per.push(ms(t1));
        terminals += model.len();
    }
    per.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!(
        "derive_total_ms={:.2} buildings={} terminals={} mean_ms={:.3} p50_ms={:.3} p99_ms={:.3}",
        ms(t),
        count,
        terminals,
        per.iter().sum::<f64>() / count as f64,
        per[count / 2],
        per[(count * 99) / 100]
    );
}

fn scenario_tiles(tile: usize) {
    // Per-tile generation, as `TiledHeightMap` is documented to work.
    let mut map = TiledHeightMap::new(tile, 1.0, 42, |seed| Box::new(FbmNoise::new(seed)));
    let t = Instant::now();
    map.ensure_radius((0, 0), 2);
    println!("tiles_25_ms={:.2} loaded={}", ms(t), map.loaded_count());

    // Seam: max |height difference| across the shared border of tile (0,0) and (1,0), vs the
    // mean adjacent-cell difference inside tile (0,0).
    let a = map.tile((0, 0)).heightmap.clone();
    let b = map.tile((1, 0)).heightmap.clone();
    let n = tile;
    let mut seam_max = 0.0f32;
    let mut seam_sum = 0.0f32;
    for z in 0..n {
        let d = (a.get(n - 1, z) - b.get(0, z)).abs();
        seam_max = seam_max.max(d);
        seam_sum += d;
    }
    let mut in_sum = 0.0f32;
    let mut in_cnt = 0;
    for z in 0..n {
        for x in 0..n - 1 {
            in_sum += (a.get(x, z) - a.get(x + 1, z)).abs();
            in_cnt += 1;
        }
    }
    println!(
        "seam_mean={:.5} seam_max={:.5} interior_adjacent_mean={:.5} seam_to_interior_ratio={:.1}",
        seam_sum / n as f32,
        seam_max,
        in_sum / in_cnt as f32,
        (seam_sum / n as f32) / (in_sum / in_cnt as f32)
    );

    // Determinism vs request order.
    let order_a = [(0, 0), (1, 0), (-1, 3), (2, 2)];
    let order_b = [(2, 2), (-1, 3), (1, 0), (0, 0)];
    let mut m1 = TiledHeightMap::new(tile, 1.0, 42, |s| Box::new(FbmNoise::new(s)));
    let mut m2 = TiledHeightMap::new(tile, 1.0, 42, |s| Box::new(FbmNoise::new(s)));
    let f1: Vec<u64> = order_a
        .iter()
        .map(|c| fingerprint(&m1.tile(*c).heightmap))
        .collect();
    let mut f2 = vec![0u64; 4];
    for c in order_b {
        let idx = order_a.iter().position(|x| *x == c).unwrap();
        f2[idx] = fingerprint(&m2.tile(c).heightmap);
    }
    println!("order_independent={}", f1 == f2);
}

fn scenario_determinism() {
    let a = fingerprint(&base_fbm(513, 1.0, 42));
    let b = fingerprint(&base_fbm(513, 1.0, 42));
    // Same job from rayon worker threads, concurrently.
    use rayon::prelude::*;
    let all: Vec<u64> = (0..8)
        .into_par_iter()
        .map(|_| fingerprint(&base_fbm(513, 1.0, 42)))
        .collect();
    let mut e1 = base_fbm(513, 1.0, 42);
    HydraulicErosion::new(42).erode(&mut e1);
    let mut e2 = base_fbm(513, 1.0, 42);
    HydraulicErosion::new(42).erode(&mut e2);
    println!(
        "fbm_repeat_equal={} fbm_parallel_equal={} erosion_repeat_equal={} fp_fbm={:016x} fp_eroded={:016x}",
        a == b,
        all.iter().all(|x| *x == a),
        fingerprint(&e1) == fingerprint(&e2),
        a,
        fingerprint(&e1)
    );
}

/// Parallel scaling of independent whole-map erosions (what a chunked pipeline could do).
fn scenario_parallel_erosion(size: usize, drops: u32, jobs: usize) {
    use rayon::prelude::*;
    let t = Instant::now();
    (0..jobs).into_par_iter().for_each(|i| {
        let mut hm = base_fbm(size, 1.0, 42 + i as u64);
        HydraulicErosion {
            num_drops: drops,
            ..HydraulicErosion::new(42 + i as u64)
        }
        .erode(&mut hm);
    });
    println!(
        "parallel_jobs={} size={} drops={} wall_ms={:.2}",
        jobs,
        size,
        drops,
        ms(t)
    );
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let arg = |i: usize, d: usize| args.get(i).and_then(|s| s.parse().ok()).unwrap_or(d);
    match args.get(1).map(String::as_str) {
        Some("generators") => scenario_generators(arg(2, 513)),
        Some("erosion") => scenario_erosion(arg(2, 513), arg(3, 50_000) as u32, arg(4, 1) as u32),
        Some("roads") => scenario_roads(arg(2, 513)),
        Some("buildings") => scenario_buildings(arg(2, 1000)),
        Some("tiles") => scenario_tiles(arg(2, 128)),
        Some("determinism") => scenario_determinism(),
        Some("parallel-erosion") => {
            scenario_parallel_erosion(arg(2, 513), arg(3, 50_000) as u32, arg(4, 16))
        }
        _ => {
            eprintln!(
                "scenarios: generators|erosion|roads|buildings|tiles|determinism|parallel-erosion"
            );
            std::process::exit(2);
        }
    }
    println!("peak_rss_mb={:.1}", rss_mb());
}
