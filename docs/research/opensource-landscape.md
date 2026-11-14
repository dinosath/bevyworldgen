# Open-source landscape for a Bevy procedural-world framework

Phase 1 deliverable 1. Research date: 2026-11-13/14. Sources: crates.io API, GitHub API/clones, repository
files, docs.rs not used (crate metadata read directly). **Depth tags**: **[deep]** source cloned, read and/or
run; **[mid]** cloned, README + structure + manifest read; **[shallow]** metadata/README only.
Stars are listed only for context and are **not** used in scoring.

Current engine baseline: **Bevy 0.20.1** (crates.io, 2026-10-28). Local `bevy/` checkout is `0.20.0-dev`
(`41a1282e3`, 2026-11-13). Rust 1.99.1.

## 1. The Symbios ecosystem (primary candidate) — org `TheJanusStream`, one maintainer ("Janus"), all MIT

| Crate | Version (crates.io) | Source LOC / tests | Bevy | Role | Depth |
|---|---|---|---|---|---|
| `symbios-ground` | 0.4.2 | 2.9 k / 45 pass | none | heightmap, FBM/DS/Voronoi, hydraulic+thermal erosion, splat, `TiledHeightMap` | **deep** |
| `symbios-tensor` | 0.7.0 | 7.6 k (+bench) / 76 pass | none | tensor-field roads, rationalize, blocks, lots, carve, road meshes, `CityStreamer` | **deep** |
| `symbios-shape` | 0.5.0 | 10.5 k / 335 pass | none | CGA shape grammar derivation | **deep** |
| `symbios-texture` | 0.8.1 | 28.3 k / ≈323 test attrs | none | procedural PBR texture generators | mid |
| `bevy_symbios_ground` | 0.8.0 | 2.2 k / 41 test attrs | 0.20 | mesh builder, splat texture, `TerrainQuery`, Avian heightfield collider (`physics` feature), async task helper | **deep** |
| `bevy_symbios_shape` | 0.11.0 | 6.0 k / 78 | 0.20 | `spawn_shape` (entity per terminal + mesh cache) | mid |
| `bevy_symbios_texture` | 0.14.0 | 5.6 k / 45 | 0.20 | texture assets/plugin/egui | mid |
| `symbios` / `bevy_symbios` | 1.5.2 / 0.12.0 | 11.9 k / 4.2 k | 0.20 | L-systems (trees/plants) | shallow |
| `symbios-genetics`, `-audio`, `-avatar`, … | – | – | – | out of scope | n/a |

Facts that drive the decision:

* Every library was republished **2026-11-08/09** for Bevy 0.20 — the foundation Ground Lab `main` (0.18,
  symbios-ground 0.2.1) is two to five minor versions behind the libraries it depends on.
* Crates are **Bevy-free where it matters** (Layer A of the prompt's architecture already exists): `symbios-ground`,
  `-tensor`, `-shape`, `-texture` have no Bevy dependency; `bevy_*` wrappers are thin.
* Determinism has been engineered deliberately (`libm` for transcendentals, per-tile and per-shape seed streams)
  and is covered by tests (`tests/determinism.rs` in symbios-ground).
* **Risk:** single maintainer; low crates.io downloads (symbios-ground 2.1 k, symbios-tensor 0.7 k); lockstep
  semver churn (0.2 → 0.4 in eight months for ground, 0.1 → 0.7 for tensor). Mitigation: pin exact versions,
  vendor-ready MIT code, contribute upstream (see ADR-001 §Upstream).
* Not provided: climate/biome simulation, river networks/drainage, vegetation placement/rendering, terrain LOD,
  atmosphere, GIS import, chunk-aware erosion, cross-tile roads, highways/bridges/tunnels, road markings.

## 2. Symbios Overlands — `TheJanusStream/symbios-overlands` [mid]

Apache-2.0, Bevy **0.20.1**, active (last push 2026-11-11), 1,424 Rust files, ≈6.1 k test attributes, `cargo-deny`
policy, CI + deploy. A peer-to-peer social "world of recipes" on ATProto/WebRTC. **Not a library and not a
large-world engine** — its own geodata doc records a deliberate removal of the far field/horizon ("a Kiez at full
detail to its edge… with no horizon") and worlds are bounded (250 m–2 km).

Reusable *solutions* it contains (answering the prompt's checklist):

| Need | What exists | Reusable? |
|---|---|---|
| Async generation, native + browser | `crates/gen-jobs` (Bevy-free, serialisable `GenJob`/`GenResult`, pure `run()`), `src/offload.rs` (native `AsyncComputeTaskPool` / wasm pooled Web Workers, two-lane priority), `crates/gen-worker` (gloo-worker, MessagePack codec) | **Yes, as a pattern** (Apache-2.0; crates are workspace-internal and **not on crates.io** ⇒ vendor with NOTICE or re-implement) |
| Cross-target determinism | `libm` routing; documented honestly as "same arithmetic, not yet measured on wasm" | **Yes** — adopt the discipline and add a wasm32 test runner |
| Geographic data | `crates/geodata`: GDI Berlin WMS/WFS (EPSG:25833), legend-decoding of heights, dl-de/zero-2.0 data; Berlin-only | **Pattern only**; replace dataset layer (see GIS below) |
| Terrain integration | Same symbios-ground pipeline (FBM/DS/Voronoi + erosion) + splat; whole-room heightmap | No new capability over Ground Lab |
| Roads | `src/urban/*` (ribbon/hubs/bends/levelling/truncation) draped over terrain from `symbios-tensor` topology | **Study** for road geometry quality |
| Vegetation | `src/grass/*` (tiles, horizon), `ground_cover/*` | **Study**; prefer `bevy_grass` first |
| Atmosphere/weather | `sky`, `clouds`, `weather`, `time_of_day`, WESL shaders (`cloud*.wesl`, `sky*.wesl`, `water.wesl`) | **Study/vendor** (Apache-2.0) as an alternative to Bevy's atmosphere |
| Engine upgrades | pins `bevy 0.20.1`, `bevy_egui 0.43.1`; lockfile deliberately untracked; has diagnostics/alloc tracking | Process reference |
| World streaming / large worlds | **None.** Bounded rooms; no chunks, no LOD, no floating origin | — |

**Answer to the key question:** newer Symbios *libraries* already solve the Bevy-version problem (the 0.20 port is
a dependency bump, which the local branch demonstrates). Overlands adds the browser/offload and determinism
patterns, not large-world capability.

## 3. Terrain renderers

| Project | License | Bevy | State | Findings | Depth | Verdict |
|---|---|---|---|---|---|---|
| **`mate-h/bevy_terrain`** (continuation of the thesis project) | MIT OR Apache-2.0 | API for 0.20 | 98 commits, last 2026-11-02, one active maintainer; 16 k LOC, 7 tests, **no CI**; **not on crates.io** | Quadtree tile streaming with morphing/blending, planar *and* WGS84, custom materials + attachments, deferred, motion vectors, heightfield terrain shadows, Bevy `Atmosphere` integration, optional `big_space`, GDAL preprocess crate, `TerrainConfig::from_heightfield` (CPU tiling ≈50 ms for 2048² per README). **Contradiction found:** README says `main` uses Bevy 0.20 from crates.io; `Cargo.toml` on `main` pins `bevy` and `big_space` to **git `main`/a fork**. I verified `cargo check --lib` **passes on stable 0.20.1 + `big_space =0.14.0-rc.0`** after changing those two lines. Runtime not run here. | mid | **Leading candidate for the LOD layer, gated by spike S-1** (vendor/patch, runtime benchmark) |
| `kurtkuehnert/bevy_terrain` (original) | MIT OR Apache-2.0 | 0.14 | last commit 2024-07, last push 2025-04, 8 open issues | Same design lineage; superseded by the continuation above | mid | Reference only |
| `bevy-clipmap` 1.0.4 | MIT | crates.io: 0.18 (repo: 0.19) | 651 LOC, **0 tests**, 30 commits | GPU geometry clipmaps (Hoppe); heightmaps as KTX via a Python script | mid | Algorithm reference only |
| `bevy_symbios_ground` | MIT | 0.20 | active | single full-res mesh builder, no LOD | deep | Keep for editing/colliders; not the streamed renderer |
| `jackdaw_editor_terrain` 0.20.2 | MIT OR Apache-2.0 | 0.20 | active (2026-11-10) | terrain sculpt/paint/scatter for the Jackdaw editor | shallow | Phase 9 editor candidate |
| `plumesplat` 0.1.1 | MIT OR Apache-2.0 | 0.18 | 2026-01 | 256+ material splatting | shallow | Watch (no 0.20 build) |
| `bevy_mesh_terrain`, `degen_toon_terrain` | – | ≤0.16 | stale | simple heightmap terrain | shallow | Exclude (stale) |
| `bevy_noised` 0.3.1 | MIT OR Apache-2.0 | none | 2026-10 | matching CPU+WGSL 2D noise | shallow | Candidate for world-space noise parity (CPU gen = GPU shading) |
| `bevy-open-world` (evroon) | **GPL-3.0** | 0.18 | active | open-world rendering crates | shallow | **Excluded by license gate** — architecture inspiration only |

## 4. Terrain generation, erosion, geology

| Project | License | Bevy | Findings | Depth | Verdict |
|---|---|---|---|---|---|
| **`bevy_erosion_filter`** 0.3.0 | **MPL-2.0** (crate metadata; repo also has an MIT file; algorithm is Rune Skovbo Johansen's MPL-2.0 work) | 0.20 | Analytic per-fragment gully detail: no neighbour reads, no simulation, chunk-parallel, has a Bevy-free CPU module. Ideal *detail layer* on top of coarse simulated terrain; not a drainage simulator. | mid | **Adopt as a rendering detail layer**; MPL-2.0 is file-level copyleft — use unmodified as a dependency |
| **Nerthus** (`danemadsen/nerthus`) | MIT | 0.20 | "Clean-room reimplementation of Gaea 2.3.1": node graph with Volcano/MountainRange/Canyon/Island/Dunes, ErosionPro, Lichtenberg, rivers, lakes, snow, glaciers, strata, colour maps; 462 k LOC, 1,188 tests, 3 CI workflows | **shallow** | **Promising source of erosion/hydrology/strata algorithms**; deep audit (algorithm quality, determinism, GPU/CPU split, and an IP review of "reimplementation of a commercial product") is task R-2 in the roadmap. Not scored with confidence. |
| `bevy_arc_studio` | MIT OR Apache-2.0 | 0.20 | AI-first environment studio: domain-warped noise, droplet erosion, 6-layer splat, Poisson-disk scatter, atmosphere, editor/undo, BRP control | shallow | Editor-architecture reference (Phase 9) |
| `mate-h/bevy_erosion` | **no license file** | 0.20 | compute-shader hydraulic erosion | shallow | **Excluded by license gate** until a license is added |
| `julianbieber/bevy_terrain_erosion` | MIT | – | tiny | shallow | Not evaluated further |
| `feldspar` | MIT | **0.8-era**, last push 2022-08 | voxel SDF terrain, LOD by chunk downsampling | mid | **Reject** (incompatible engine, voxel representation); concepts only |
| `rust-procedural-world` | MIT | 0.12 | 814 LOC, 2D perlin tile map | deep | **Reject** (2D demo); nothing reusable for 3D terrain |

Not found (searched, nothing suitable): a maintained Rust **GPU hydraulic/stream-power erosion library** for
Bevy 0.20 with a permissive license and tests; a Bevy **procedural-city** crate other than Symbios;
a Bevy **CGA** alternative; a Bevy **virtual texturing** crate.

## 5. Vegetation, water, atmosphere, large worlds, GIS

| Area | Candidate | License | Bevy | Notes | Depth |
|---|---|---|---|---|---|
| Grass/foliage | **`bevy_grass`** 0.20.6 | MIT | 0.20 | GPU compute builds blades each frame near the camera ("no blades stored"; 4 km field ≈ 200 m field cost), wind, trampling, Avian/Rapier, 36 presets, 308 tests, a 240 Hz perf-gate CI (self-hosted), WebGPU | mid |
| Foliage scatter | `bevy_feronia` 0.8.4 / `bevy_eidolon` | MIT OR Apache-2.0 | 0.18 / 0.19 | scattering + instanced material; behind current Bevy | shallow |
| Trees | `bevy_flora` 0.47.0, `feathrise_vegetation` 0.2 | MIT OR Apache-2.0 | 0.20 | parametric trees / deterministic distribution + competition | shallow |
| Scatter core | `bevy_map_scatter` 0.5.0 | MIT OR Apache-2.0 | 0.19 | map-driven object scattering | shallow |
| Water | `bevy_water` 0.20.0 | declared MIT OR Apache-2.0, **no license files in repo** | 0.20 | ocean material | mid |
| Water | `bevy-aqua` 0.1.3 (+ `-query`, `-shore`) | MIT OR Apache-2.0 | 0.19 | camera-centred Gerstner + FFT ocean, GPU-sampled probes, shoreline domain | shallow |
| Sky/weather | `bevy_elements` 0.8.0, `bevy_weather`, `fk-sky` | MIT OR Apache-2.0 | 0.20/0.19 | sky, climate-driven weather, clouds | shallow |
| Sky/atmosphere | **Bevy built-in** `bevy_pbr::atmosphere` (LUT-based aerial view, sky view, multiscattering), volumetric fog, `OcclusionCulling`, `VisibilityRange` (HLOD), contact shadows, SSAO, SSR, Solari RT lighting, **meshlet (experimental)** | MIT OR Apache-2.0 | 0.20 | checked in the local `bevy/` checkout (`examples/3d/*`) | mid |
| Floating origin | **`big_space`** | MIT OR Apache-2.0 | stable 0.13.0 → Bevy **0.19**; 0.14.0-rc.0 → **0.20.0-rc.2** | i32/i64 grid cells + f32 local; used by `mate-h/bevy_terrain` | mid |
| Physics | `avian3d` 0.8.0, `bevy_rapier3d` 0.37.0 | MIT OR Apache-2.0 / Apache-2.0 | 0.20 | heightfield colliders; Symbios has an Avian wrapper | shallow |
| Tile streaming (GIS) | `bevytiles` 0.1.0, `bevy_3d_tiles` 0.5.1 (OGC 3D Tiles), `bevy_geo_tiles`, `bevy_slippy_tiles` | MIT OR Apache-2.0 (mostly) | 0.19 | geospatial raster/3D tile streaming | shallow |
| GIS libraries | `geo` 0.33, `proj4rs` 0.3 (pure Rust), `proj` 0.32 (C), `tiff` 0.11, `geotiff` 0.1, `gdal` 0.19, `osmpbf` 0.3.8, `geozero` 0.15, `rstar` 0.14, `spade` 2.15 | MIT or MIT OR Apache-2.0 | none | no Bevy coupling; `proj4rs` avoids a C dependency (wasm-friendly) | shallow |

## 6. Techniques from the prompt vs. Bevy 0.20 (rendering research, abbreviated)

| Technique | Bevy status (checked in `bevy/` tree) | Plugin or upstream? |
|---|---|---|
| Physically based atmosphere / aerial perspective | **Built in** (`atmosphere/*.wesl`, `examples/3d/atmosphere.rs`) | use; contribute fixes upstream |
| GPU occlusion culling (HZB) | **Built in** (`OcclusionCulling`, example) | use; verify it works with custom terrain materials |
| HLOD | **Built in** (`VisibilityRange`) | use for buildings/props |
| Meshlet / virtual geometry | **Experimental** (`meshlet` feature) | evaluate for hero static meshes only |
| Volumetric fog, SSAO, SSR, contact shadows, TAA, bloom | built in | use |
| Ray-traced GI | **Solari** (`bevy_solari`) | optional ultra preset; hardware-gated |
| Terrain clipmaps / quadtree streaming | **not built in** | plugin (`bevy_terrain` candidate) |
| Virtual texturing | **not built in**, no crate found | long-term; upstream design discussion |
| Vegetation instancing/impostors | **not built in** | `bevy_grass` + own impostors |
| Water | **not built in** | plugin |
| Indirect/GPU-driven draw for custom materials | Bevy has GPU-driven mesh preprocessing; custom-material indirect draws need verification | spike |

Upstream-contribution candidates (no private renderer fork): (1) fixes found while integrating terrain with
`OcclusionCulling`/`Atmosphere`; (2) a `Mesh`-tangent job helper or `generate_tangents` off-thread guidance;
(3) docs for custom material extensions on large worlds. None is promised until the spike work shows a real gap.

## 7. Excluded / shortlisted summary

* **Shortlisted for adoption:** Symbios generation crates; `bevy_grass`; `bevy_erosion_filter` (MPL, unmodified);
  `big_space` (rc, pinned); `mate-h/bevy_terrain` (spike-gated); Bevy built-ins; `avian3d`.
* **Study / pattern reuse:** Symbios Overlands (offload, determinism, urban ribbons, sky), `bevy_arc_studio`,
  `jackdaw_editor_terrain`, Nerthus (algorithms, pending R-2).
* **Excluded:** `bevy-open-world` (GPL-3.0), `mate-h/bevy_erosion` (no license), `feldspar` (engine/age),
  `rust-procedural-world` (2D), original `kurtkuehnert/bevy_terrain` (Bevy 0.14), `bevy_mesh_terrain`/`degen_toon_terrain` (stale).
* **Archived-but-valuable research:** the thesis behind `bevy_terrain` (CC BY 4.0 per its README), Hoppe's GPU
  geometry clipmaps (cited by `bevy-clipmap`), Rune Skovbo Johansen's erosion-filter write-up (MPL-2.0 code).

## 8. Reference video

`https://www.youtube.com/watch?v=fqYKAAcktWs` — only metadata was accessible (no frames, transcript or
description): title **"some roads remember you | rainy autumn touge | Toyota AE86 | 4K"**, channel *thsunami*
(YouTube oEmbed). Nothing about its contents beyond the title is asserted here. It reads as a
mountain-pass (touge) driving mood reference: wet asphalt, autumn foliage, tight road geometry — which
supports prioritising roads-on-terrain, wetness/weather and vegetation density, but that is an inference from the
title, not an analysis of the video. Whether it is game footage (let alone which game) is unverified.
