//! Engine-free mesh data for terrain and water chunks (built on worker threads).

use crate::chunk::TerrainChunk;

/// Terrain mesh with skirts. Positions are relative to the chunk origin (x east, z south).
/// Splat weights are exposed as `splat_a` (layers 0-3), `splat_b` (4-5) and `splat_c` (6-7).
pub struct TerrainMeshData {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub splat_a: Vec<[f32; 4]>,
    pub splat_b: Vec<[f32; 2]>,
    pub splat_c: Vec<[f32; 2]>,
    pub indices: Vec<u16>,
}

impl TerrainMeshData {
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }
}

pub fn build_terrain_mesh(c: &TerrainChunk) -> TerrainMeshData {
    let n = c.n;
    let s = c.spacing;
    let skirt = s * 2.0 + 3.0;
    let cap = n * n + 4 * n;
    let mut m = TerrainMeshData {
        positions: Vec::with_capacity(cap),
        normals: Vec::with_capacity(cap),
        splat_a: Vec::with_capacity(cap),
        splat_b: Vec::with_capacity(cap),
        splat_c: Vec::with_capacity(cap),
        indices: Vec::with_capacity((n - 1) * (n - 1) * 6 + 4 * (n - 1) * 6),
    };
    let push = |m: &mut TerrainMeshData, i: usize, j: usize, drop: f32| {
        let k = j * n + i;
        m.positions
            .push([i as f32 * s, c.heights[k] - drop, j as f32 * s]);
        m.normals.push(c.normals[k]);
        let w = c.splat[k];
        let f = |v: u8| v as f32 / 255.0;
        m.splat_a.push([f(w[0]), f(w[1]), f(w[2]), f(w[3])]);
        m.splat_b.push([f(w[4]), f(w[5])]);
        m.splat_c.push([f(w[6]), f(w[7])]);
    };
    for j in 0..n {
        for i in 0..n {
            push(&mut m, i, j, 0.0);
        }
    }
    let idx = |i: usize, j: usize| (j * n + i) as u16;
    for j in 0..n - 1 {
        for i in 0..n - 1 {
            let (a, b, cc, d) = (idx(i, j), idx(i, j + 1), idx(i + 1, j), idx(i + 1, j + 1));
            m.indices.extend_from_slice(&[a, b, cc, cc, b, d]);
        }
    }
    // skirts: four edges, outward-facing
    let edges: [(Vec<(usize, usize)>, (f32, f32)); 4] = [
        ((0..n).map(|i| (i, 0)).collect(), (0.0, -1.0)),
        ((0..n).map(|i| (i, n - 1)).collect(), (0.0, 1.0)),
        ((0..n).map(|j| (0, j)).collect(), (-1.0, 0.0)),
        ((0..n).map(|j| (n - 1, j)).collect(), (1.0, 0.0)),
    ];
    for (verts, out) in edges.iter() {
        let base = m.positions.len() as u16;
        for &(i, j) in verts {
            push(&mut m, i, j, skirt);
        }
        for e in 0..n - 1 {
            let (t0, t1) = (
                idx(verts[e].0, verts[e].1),
                idx(verts[e + 1].0, verts[e + 1].1),
            );
            let (s0, s1) = (base + e as u16, base + e as u16 + 1);
            for tri in [[t0, t1, s0], [t1, s1, s0]] {
                let p = |k: u16| m.positions[k as usize];
                let (a, b, cc) = (p(tri[0]), p(tri[1]), p(tri[2]));
                let (e1, e2) = (
                    [b[0] - a[0], b[1] - a[1], b[2] - a[2]],
                    [cc[0] - a[0], cc[1] - a[1], cc[2] - a[2]],
                );
                let nrm = [
                    e1[1] * e2[2] - e1[2] * e2[1],
                    e1[2] * e2[0] - e1[0] * e2[2],
                    e1[0] * e2[1] - e1[1] * e2[0],
                ];
                if nrm[0] * out.0 + nrm[2] * out.1 >= 0.0 {
                    m.indices.extend_from_slice(&tri);
                } else {
                    m.indices.extend_from_slice(&[tri[0], tri[2], tri[1]]);
                }
            }
        }
    }
    m
}

/// Water surface mesh; `depth` is stored per vertex (metres of water above the ground).
pub struct WaterMeshData {
    pub positions: Vec<[f32; 3]>,
    pub depth: Vec<f32>,
    pub indices: Vec<u16>,
}

pub fn build_water_mesh(c: &TerrainChunk) -> Option<WaterMeshData> {
    let n = c.n;
    if !c.water.iter().any(|w| *w > f32::NEG_INFINITY) {
        return None;
    }
    let s = c.spacing;
    let mut positions = Vec::new();
    let mut depth = Vec::new();
    let mut remap = vec![u16::MAX; n * n];
    let mut indices = Vec::new();
    let wet = |i: usize, j: usize| c.water[j * n + i] > f32::NEG_INFINITY;
    let vert = |i: usize,
                j: usize,
                positions: &mut Vec<[f32; 3]>,
                depth: &mut Vec<f32>,
                remap: &mut Vec<u16>|
     -> u16 {
        let k = j * n + i;
        if remap[k] == u16::MAX {
            let w = c.water[k];
            let (y, d) = if w > f32::NEG_INFINITY {
                (w, w - c.heights[k])
            } else {
                (c.heights[k], 0.0)
            };
            remap[k] = positions.len() as u16;
            positions.push([i as f32 * s, y, j as f32 * s]);
            depth.push(d);
        }
        remap[k]
    };
    for j in 0..n - 1 {
        for i in 0..n - 1 {
            if wet(i, j) || wet(i + 1, j) || wet(i, j + 1) || wet(i + 1, j + 1) {
                let a = vert(i, j, &mut positions, &mut depth, &mut remap);
                let b = vert(i, j + 1, &mut positions, &mut depth, &mut remap);
                let cc = vert(i + 1, j, &mut positions, &mut depth, &mut remap);
                let d = vert(i + 1, j + 1, &mut positions, &mut depth, &mut remap);
                indices.extend_from_slice(&[a, b, cc, cc, b, d]);
            }
        }
    }
    Some(WaterMeshData {
        positions,
        depth,
        indices,
    })
}
