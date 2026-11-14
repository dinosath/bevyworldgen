# Feature comparison and weighted assessment

Phase 1 deliverable 4. Legend: **✔ verified** (read in source and/or run), **◐ partial**, **✘ absent**,
**? not verified** (never promoted to ✔). Evidence IDs refer to `symbios-ground-lab-audit.md` (A§), `baseline.md` (B§),
`opensource-landscape.md` (L§).

## 1. Capability matrix

| Capability | Symbios Ground Lab (app, 0.20 branch) | Symbios libraries (ground/tensor/shape + bevy_*) | Best alternative(s) found | Recommendation |
|---|---|---|---|---|
| Heightmap generation | ✔ FBM(value-noise)/Diamond-Square/Voronoi via lib, whole-map, tile-local coords (A§4) | ✔ same; **✘ seamless world-space sampling** (seams 59–117×, B§3.5) | `bevy_noised` (CPU+WGSL parity) ?; Nerthus node graph ? | **Extend**: add world-space seamless sampler in Layer A; keep FBM/DS/Voronoi |
| Erosion | ✔ hydraulic droplets + thermal, whole map, single thread, deterministic (B§3.2) | ✔ same; ✘ chunk apron/border handling; ✔ parallel across independent maps (11.9×) | `bevy_erosion_filter` ✔ (GPU analytic detail, MPL-2.0); Nerthus ErosionPro ? | **Extend + add**: border-correct chunk erosion; erosion-filter as render-time detail; evaluate Nerthus (R-2) |
| Terrain LOD | ✘ one full-res mesh (B10) | ✘ (`bevy_symbios_ground` builds a full mesh) | `mate-h/bevy_terrain` ◐ (design verified, compiles on stable with 2 edits, **runtime not measured**); `bevy-clipmap` ◐ (651 LOC, 0 tests) | **Spike S-1**, then Replace renderer layer |
| World streaming | ◐ `TiledHeightMap` primitives only (A§4) | ◐ `TiledHeightMap`, `CityStreamer`; no Bevy plugin, no budget/priority | `mate-h/bevy_terrain` streams *tile data* (quadtree) ◐; `bevytiles`/`bevy_3d_tiles` ? | **New integration layer** (job queue, versions, budgets) |
| GPU terrain rendering | ◐ splat via `ExtendedMaterial` (WESL) (A§6) | ✘ (CPU meshes) | `mate-h/bevy_terrain` ◐ | **Replace** with chunked/quadtree renderer; keep splat material as the shading hook |
| Road networks | ✔ tensor-field (A§5), **tile-bounded**, ≈O(n²) rationalize (B§3.3) | ✔ `symbios-tensor` | none found | **Reuse + extend**: tile ≤512 cells; coarse highway layer is new work |
| City blocks | ✔ planar-face extraction (1–43 ms) | ✔ | none found | **Reuse** |
| Building generation | ✔ CGA derive 0.094 ms/building; ✘ materialisation scale (A§5) | ✔ `symbios-shape` (335 tests) | none found in Rust/Bevy | **Reuse derivation; Replace realisation** (baked meshes + HLOD) |
| Terrain materials | ◐ 4 layers, height+slope weights | ◐ `SplatMapper` same | `plumesplat` ? (0.18) | **Extend** (more layers, climate-driven weights, triplanar) |
| Vegetation | ✘ (only L-system crates exist: `symbios`/`bevy_symbios` ?) | ✘ placement/rendering | `bevy_grass` ◐ (README/CI read; not run); `bevy_flora`, `feathrise_vegetation`, `bevy_map_scatter` ? | **Adopt `bevy_grass`**, spike trees/impostors |
| Hydrology | ◐ lakes from droplet pooling only | ◐ | Nerthus rivers/lakes ?; none in Bevy ecosystem | **New** (flow accumulation/rivers) — evaluate Nerthus algorithms first |
| Atmospheric rendering | ✘ (default light only) | ✘ | **Bevy built-in** atmosphere + volumetric fog ✔ (examples in tree); Overlands sky/clouds (Apache-2.0) ◐ | **Use Bevy built-ins** first |
| GIS integration | ✘ | ✘ | Overlands `geodata` (Berlin-only, pattern); `proj4rs`, `tiff`, `osmpbf`, `geo` ✔ (metadata) | **New** adapter layer on permissive Rust GIS crates |
| Bevy compatibility | ✔ 0.20 on a **local unpushed** branch; `main` = 0.18 (B§6) | ✔ 0.20 (published 2026-11-08) | `big_space` 0.14-rc (0.20.0-rc.2), 0.13 stable (0.19); `mate-h/bevy_terrain` pins git Bevy | **Pin stable 0.20.1**; adopt crates only with 0.20 builds |
| Test coverage | ✘ 11 tests; no CI gates (B11–B12) | ✔ 45 / 76 / 335 pass (run here); determinism tests; criterion benches | `bevy_grass` 308 tests + perf CI; `mate-h/bevy_terrain` 7 tests, no CI | **Reuse libs; build CI from day 1** |
| License compatibility | **PASS** (MIT; 629 deps all permissive, see license-inventory) | **PASS** (MIT) | **PASS** except `bevy-open-world` (GPL-3.0 ✘), `mate-h/bevy_erosion` (none ✘); `bevy_erosion_filter` MPL-2.0 (conditional); `bevy_water` (no license files) | Gate applied |

## 2. Weighted assessment

Weights: functional coverage 25 %, architecture/modularity 20 %, performance/scalability 20 %,
Bevy compatibility & migration 15 %, maintenance/ecosystem 10 %, code quality/testing 10 %.
Licensing is a separate pass/fail gate and is not weighted. Scores are 0–5 **judgements** tied to the evidence
below; the arithmetic is reproducible with `python3 -I docs/research/scores.py`.
*Functional coverage is scored within the candidate's own category* (a terrain renderer is not penalised for
lacking roads); totals are therefore comparable **within a category**, and across categories only as a
"is this a sound dependency" signal. Stars were not used.

| Candidate | Category | License gate | Func 25% | Arch 20% | Perf 20% | Bevy 15% | Maint 10% | Quality 10% | **Total /5** | Evidence confidence |
|---|---|---|--:|--:|--:|--:|--:|--:|--:|---|
| Symbios crates (symbios-ground/tensor/shape + bevy_symbios_*) | generation core | PASS | 3.0 | 4.5 | 2.5 | 5.0 | 2.5 | 4.0 | **3.55** | high (source read, tests run, benchmarked) |
| Symbios Ground Lab (app) as foundation | application | PASS | 2.5 | 1.5 | 1.0 | 4.0 | 1.5 | 1.5 | **2.02** | high (source read, app benchmarked) |
| Symbios Overlands (app) as foundation | application | PASS | 3.5 | 2.0 | 2.5 | 5.0 | 4.0 | 4.0 | **3.32** | medium (docs+structure read, not run) |
| Nerthus (Gaea-compatible terrain graph) | generation core | PASS* | 4.0 | 2.5 | 2.5 | 5.0 | 3.5 | 4.5 | **3.55** | low (surface inspection only) |
| bevy_arc_studio | application | PASS | 3.0 | 2.5 | 2.0 | 5.0 | 3.5 | 3.5 | **3.10** | low (surface inspection only) |
| mate-h/bevy_terrain | terrain renderer | PASS | 4.0 | 4.0 | 3.5 | 2.5 | 2.5 | 1.5 | **3.27** | medium (source read, compile-checked, not run) |
| kurtkuehnert/bevy_terrain (original) | terrain renderer | PASS | 3.5 | 4.0 | 3.5 | 0.5 | 0.5 | 1.0 | **2.60** | medium (README+tree; Bevy 0.14) |
| bevy-clipmap | terrain renderer | PASS | 2.0 | 3.0 | 3.0 | 3.0 | 2.0 | 0.5 | **2.40** | medium (source read; 651 LOC) |
| bevy_grass | vegetation | PASS | 3.0 | 4.0 | 4.0 | 5.0 | 3.5 | 4.5 | **3.90** | medium (README+CI read, not run) |
| bevy_erosion_filter | terrain detail | PASS (MPL-2.0) | 3.0 | 4.5 | 4.5 | 5.0 | 2.5 | 3.0 | **3.85** | medium (README+notices read) |
| big_space | large worlds | PASS | 4.0 | 4.0 | 4.0 | 3.0 | 4.0 | 4.0 | **3.85** | medium (manifest+README; rc for Bevy 0.20) |
| bevy_water | water | COND (no license files) | 2.5 | 3.0 | 2.5 | 5.0 | 3.0 | 1.0 | **2.88** | low |
| feldspar (voxel) | terrain (voxel) | PASS | 1.5 | 3.5 | 3.0 | 0.0 | 0.0 | 2.5 | **1.93** | medium |
| rust-procedural-world | 2D demo | PASS | 1.0 | 1.0 | 1.0 | 0.5 | 0.5 | 0.5 | **0.83** | high (814 LOC) |
| bevy-open-world | open-world crates | **FAIL (GPL-3.0)** | – | – | – | – | – | – | – | n/a - excluded by license gate |


### Score justification (one line per judgement; see source docs for evidence)

* **Symbios crates (3.55)** — Func 3: terrain/erosion/roads/blocks/lots/CGA/textures present; no climate, hydrology, vegetation, LOD, atmosphere, GIS. Arch 4.5: Bevy-free cores, small APIs, wrappers separate (A§4–5). Perf 2.5: fast at tile scale, but whole-map erosion, ≈O(n²) rationalize, seams (B§3). Bevy 5: published for 0.20. Maint 2.5: very active but single maintainer, low downloads. Quality 4: 456 passing tests across 3 crates, determinism tests, criterion benches, CI.
* **Ground Lab app (2.02)** — Func 2.5 (glue, UI, kits), Arch 1.5 (global singletons, A§3), Perf 1 (5.6 s / 27.7 s freezes, B§4), Bevy 4 (0.20 branch exists but unpushed; `main` stale), Maint 1.5 (stale since 2026-03, 0 PRs), Quality 1.5 (11 tests, no CI gates).
* **Overlands (3.32)** — Func 3.5 (much of the list, bounded), Arch 2 (large application, ATProto/P2P coupling; internal crates `gen-jobs`/`geodata` not published), Perf 2.5 (WASM worker offload; no LOD/far field by design), Bevy 5, Maint 4 (daily commits), Quality 4 (≈6.1 k test attrs, cargo-deny). *Not run by me.*
* **Nerthus (3.55, low confidence)** — surface inspection only. Strong on breadth/tests; architecture and algorithm quality unverified; **do not select on this number** (R-2).
* **bevy_arc_studio (3.10, low confidence)** — editor/game-studio; surface inspection.
* **mate-h/bevy_terrain (3.27)** — Func 4 (planar+spherical, streaming, attachments, shadows, GDAL preprocess, in-memory heightfields), Arch 4 (plugin, material hooks), Perf 3.5 (design-level evidence only, **not run**), Bevy 2.5 (manifest pins git Bevy/fork; compiles on stable after 2 edits), Maint 2.5 (active, one maintainer, not on crates.io), Quality 1.5 (7 tests, no CI).
* **kurtkuehnert/bevy_terrain (2.60)**, **bevy-clipmap (2.40)** — see L§3.
* **bevy_grass (3.90)** — Func 3 (grass/flowers, not trees), Arch 4, Perf 4 (GPU-built blades; perf-gate CI exists; not run here), Bevy 5 (0.20), Maint 3.5, Quality 4.5 (308 tests, CI).
* **bevy_erosion_filter (3.85)** — analytic, chunk-parallel by construction; small (1.1 k LOC, 15 commits).
* **big_space (3.85)** — Bevy score 3 because stable 0.13 targets 0.19 and the 0.20 build is a release candidate.
* **bevy_water (2.88)** — license gate conditional (declared in `Cargo.toml`, no license text in repo); 0 tests.

### What the numbers say

1. Inside the **generation core** category the Symbios crates are the only candidate I have both read *and* measured; they win on evidence, not only score.
2. As a **foundation application**, Ground Lab (2.02) loses clearly to every library-level alternative and also to Overlands (3.32), which is a different product.
3. The **terrain renderer** choice is genuinely open: `mate-h/bevy_terrain` (3.27) leads, but its runtime was not measured. The decision is therefore a *gated spike*, not a selection.
4. Vegetation (`bevy_grass`), detail erosion (`bevy_erosion_filter`) and floating origin (`big_space`) score ≥3.85 and fit the roadmap without duplication.
