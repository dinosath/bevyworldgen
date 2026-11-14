# Implementation roadmap

Phase 1 deliverable 10. Follows ADR-001 (integrate around the Symbios core in a new workspace) and the migration plan.
Reference machine for gates: **Ryzen 7 5700G + Radeon Vega iGPU (RADV)** — the hardware the baseline was measured on. Gates
there are deliberately modest; they are re-baselined on a discrete GPU before Phase 10. All gates are measured with the
in-repo harness and recorded under `docs/benchmarks/`.

## 0. Phase 1 completion gate (honest status)

| Gate item (prompt §10) | Status | Evidence / gap |
|---|---|---|
| Ground Lab inspected at source level | ✅ | audit; 0.20 branch **and** upstream `main` manifest |
| Fork compared against upstream | ✅ | same SHA `7358b00`, 0/0; local unpushed branch found |
| Independent Symbios crates investigated | ✅ | read + tests run (45/76/335 pass) + benchmarked |
| Other open-source projects evaluated | ✅ with depth tags | 25+ candidates; several at surface depth (Nerthus, bevy_arc_studio, Bevy-ecosystem crates) |
| Licensing verified for shortlisted deps | ✅ / ⚠ | license files read for all shortlisted *repos*; 629-package dependency scan; ⚠ items L-1…L-6 |
| Bevy compatibility of the foundation understood | ✅ | libs on 0.20 (published 2026-11-08), port branch builds |
| Architectural limitations documented | ✅ | audit §8, bevy audit |
| Baseline performance measured where feasible | ◐ | measured: algorithms, real app @512²/1025²/2049² on iGPU. **Not measured:** upstream-`main` runtime (host build blocker), `mate-h/bevy_terrain` runtime (tool permission denied, not circumvented), VRAM, per-system CPU, wasm, unattributed ≈37 ms at 2049² |
| Bevy best-practices audit | ✅ | 16 findings |
| Fork vs integration vs greenfield compared | ✅ | ADR §3 |
| Preferred architecture selected | ✅ | ADR §1 |
| Realistic implementation plan | ✅ | this document |

**Verdict:** the *foundation decision* does not depend on any missing measurement, so Phase 1 is complete for that decision.
Three **downstream choices** do depend on missing evidence and are explicitly gated as spikes: the terrain renderer (S-1), the
water renderer (S-8) and any adoption of Nerthus-derived algorithms (R-2). Phase 1 is therefore *not* a claim that the final
renderer is chosen.

## 0b. Implementation status (end of M10)

| M | Status | Notes (details and numbers: `docs/benchmarks/m2-m10-results.md`) |
|---|---|---|
| M0 | done | workspace, CI file, licences, pins (`cargo-deny` not installed locally; CI runs it) |
| M1 | done | G1, G1b, G3 pass |
| M2 | done, **G2 partial** | in-house chunk renderer (S-1 not run against `bevy_terrain`); G4 pass; G7 partial |
| M3 | done | edit layers + lossless disk cache; G9 pass |
| M4 | done | S-3 passed (order-independent erosion); hydrology is traced drainage, not flow accumulation |
| M5 | done | `bevy_grass` + baked trees; entities far below G8 |
| M6 | done except tunnels | highways, bridges, towns, drivable; S-5 reached via border-independent highway graph |
| M7 | done | CGA buildings, LOD bake; G6 not separately measured |
| M8 | done | quality presets, time of day, weather; custom water shader (no flow direction) |
| M9 | done, narrowed | DEM + OSM import, user roads, brush editor, undo/redo; no GeoTIFF/PBF/land cover, no egui Lab port |
| M10 | done, **gates partly met** | 30-min soak, full-route drive, preset table; no discrete-GPU re-baseline |

Gate status: G1, G1b, G3, G4 (mean), G8, G9 pass; G2 passes only at Low; G7 partial; G5/G6 not separately measured.

## 1. Spikes and research tasks (time-boxed; each ends with a written result and a decision)

| ID | Question | Method | Pass criterion | Blocks |
|---|---|---|---|---|
| **S-1** | Terrain LOD renderer: adopt `mate-h/bevy_terrain` or build in-house? | build and run it on stable Bevy 0.20.1 (dependency-line patch already shown to compile); feed it Symbios tile heights via `Heightfield`; fly at 100 m/s over 8 km² with occlusion + atmosphere; also prototype geomipmapped chunks with skirts (CPU or GPU) | meets **G2** below; no popping > 1 px at 1080p; clean on `OcclusionCulling`; license/vendor plan | M2 renderer |
| **S-2** | World-space seamless sampler | implement `h(x,z,seed)`; property tests over 10 k border cells | border heights **bitwise equal**; order independent; 257² chunk ≤ 15 ms | M1 |
| **S-3** | Chunk-correct erosion | fixed-point delta accumulation on base field + apron (target-architecture §5) | border bitwise equal; permutation-independent; visual parity review vs sequential erosion; ≤ 250 ms/257² chunk with apron | M4 |
| **S-4** | Tangent cost | tangents in worker vs analytic TBN in shader | no main-thread tangent work; visual equality of lit normal-mapped terrain | M1 |
| **S-5** | Cross-tile roads + highway layer | border ports + highway graph as fixed edges | adjacent tiles' road graphs join at ports (0 dangling edges across 100 tile pairs); tile ≤ 120 ms | M6 |
| **S-6** | wasm32 determinism + worker offload | golden hashes run under node/wasm32; vendor Overlands `gen-jobs`/worker pattern | native == wasm32 for all Layer-A goldens; main thread never blocked > 8 ms by a job | M2/M10 web |
| **S-7** | Building bake + HLOD | merge CGA terminals per material; `VisibilityRange` LODs | ≤ 20 entities/building at LOD0; 10 k buildings visible set ≤ 200 k entities total, derive+bake ≤ 3 ms/building off-thread | M7 |
| **S-8** | Water renderer | `bevy-aqua` vs custom vs `bevy_water` (after license fix) | depth fog, shoreline, flow on rivers; ≤ 1.5 ms GPU @1080p iGPU | M8 |
| **R-1** | What costs the unattributed ≈37 ms/frame at 2049² in the Lab? | CPU/GPU trace (tracing build) | attribution written | informs M2 design (does a CPU-side per-frame cost scale with vertex count?) |
| **R-2** | Nerthus: algorithms and IP | deep source audit of erosion/rivers/strata; legal review of "clean-room Gaea" provenance | go/no-go per algorithm | M4 hydrology/geology |
| **R-3** | Runtime check of `bevy_grass` and `bevy_erosion_filter` on this iGPU | run examples with the bench harness | frame costs recorded | M5 |

## 2. Benchmark gates (initial values; reference machine)

| Gate | Definition | Value |
|---|---|---|
| **G1 Determinism** | golden hash per `(chunk, subsystem)`; permutation of request order; native and wasm32 | 0 mismatches |
| **G1b Seams** | max height difference across shared chunk borders, all neighbour pairs | **0 (bitwise)** (baseline: 0.31 raw units, 59–117× interior step) |
| **G2 Streaming frame time** | free-camera flight at 100 m/s (360 km/h) over the demo world | p50 ≤ 12 ms, p99 ≤ 20 ms, **max ≤ 50 ms**, zero frames > 100 ms after warm-up (baseline: single frames of 5,937 ms / 29,073 ms) |
| **G3 Main-thread cost** | per chunk completion (`Collect + Realise + Upload`) | ≤ 1 ms median, ≤ 3 ms p99 |
| **G4 Worker cost** | one 257² terrain chunk incl. erosion + normals/TBN + splat | ≤ 150 ms (baseline ≈136 ms erosion + 14 ms FBM per 513² tile) |
| **G5 Road/city tile** | 512² tile roads + lots | ≤ 120 ms (baseline 86 ms) |
| **G6 Buildings** | CGA derive + bake per building | ≤ 3 ms (derive baseline 0.094 ms) |
| **G7 Memory** | resident world within `WorldBudget`; no unbounded growth during a 30-minute flight | peak RSS ≤ configured budget ±10 %; flat after warm-up |
| **G8 Entity budget** | visible entities in the showcase | ≤ 200 k |
| **G9 Cost of regeneration** | edit one chunk → visible update | ≤ 250 ms, no frame > 33 ms |

## 3. Milestones (map onto the prompt's phases 2–10)

| M | Phase | Deliverable | Acceptance | Depends on | Main risks |
|---|---|---|---|---|---|
| **M0** | 2a | Workspace, CI (fmt/clippy/test/wasm check/cargo-deny/goldens), license files + notices, dependency pins, port of `bench/phase1-algos` | CI green; baseline numbers reproduce ±5 % | owner confirms ADR | none significant |
| **M1** | 2b | **Smallest useful slice** (§4): `worldgen-core` + seamless sampler + chunk generator + job queue with versions/cancel + minimal Bevy plugin rendering a ring of chunks (single LOD, no erosion) with off-thread normals/tangents | G1, G1b, G3 pass; headless integration test of the state machine | S-2, S-4 | float determinism pitfalls |
| **M2** | 3 | Streamed mountain-region demo: LOD terrain, big-world origin, async jobs, caching, seamless transitions, terrain materials | **G2, G3, G4**; 10-minute flight without leaks (G7) | S-1, S-6 (web optional) | renderer integration (S-1), pre-release `big_space` |
| **M3** | 3 | Persistent edits (delta layers) + chunk cache on disk | G9; delta round-trip golden | M2 | cache invalidation |
| **M4** | 4 | Geological realism: chunk-correct erosion (S-3), climate (temperature, rain shadow, moisture), biomes, hydrology (rivers/lakes), richer splat | visual review checklist + G1/G1b with erosion on | S-3, R-2 | S-3 may fail → fallback coarse sim + erosion filter |
| **M5** | 5 | Vegetation: `bevy_grass` integration, ecology placement, tree instancing/impostors, wind | forest+grassland demo within G2; entities ≤ G8 | R-3, M4 | iGPU cost of grass/trees |
| **M6** | 6 | Roads and driving: highway layer, tile roads with ports, carving, bridges/tunnels, markings, colliders | S-5 pass; drivable continuous network over 4 km; G5 | S-5, M2 | cross-tile topology is research-grade; physics heightfield seams |
| **M7** | 7 | Cities: settlements, districts, parcels, baked/HLOD buildings, props | S-7 pass; G6, G8 | M6 | entity/draw budget |
| **M8** | 8 | Water, atmosphere, weather, time of day, quality presets | S-8 pass; G2 holds with all effects on "High" | M2, S-8 | iGPU budgets |
| **M9** | 9 | GIS import (DEM, OSM, land cover via `proj4rs`/`tiff`/`osmpbf`), editor (`apps/lab` port + terrain/road tools) | import a real 10 km² region deterministically; dataset license policy (L-5) done | M6, L-5 | data licensing, reprojection correctness |
| **M10** | 10 | Full-world showcase (mountains, forests, rivers, highways, villages, cities, coast, dynamic atmosphere); optimisation pass; discrete-GPU re-baseline | **all gates** on reference + a discrete GPU; documented frame-time distribution | all | integration scale |

Phases map: Phase 2 = M0–M1, Phase 3 = M2–M3, Phase 4 = M4, Phase 5 = M5, Phase 6 = M6, Phase 7 = M7, Phase 8 = M8,
Phase 9 = M9, Phase 10 = M10.

## 4. The smallest useful Phase 2 slice (M1)

**"Deterministic, seamless chunk pipeline, headless-first."** Scope:

1. Workspace + CI + pins (M0).
2. `worldgen-core`: `ChunkId`, `RegionRng`, `GenVersion`, `CancelToken`, `WorldBudget`.
3. `worldgen-terrain`: world-space seamless sampler (S-2), 257² chunk generation, normals + TBN computed in the worker (S-4), splat weights.
4. `worldgen-jobs` + `bevy_worldgen`: queue with priority/versions/cancellation, `Collect` via messages, chunk state machine, eviction.
5. Minimal renderer: one `Mesh` per chunk, one LOD, existing splat material ported from the Lab, free camera.
6. Tests: border bitwise equality over all neighbour pairs; order-permutation golden hashes; stale-result discard; headless state-machine test with
   `MinimalPlugins`; frame-time harness reusing the instrumented-Lab probe.

Exit: **G1, G1b, G3** pass and a recorded fly-over shows no frame > 100 ms (baseline: 5.9 s / 29 s freezes). Erosion, LOD, roads, vegetation are deliberately out
of M1 — they add research risk and none is needed to prove the data model.

## 5. Dependency and risk summary

* Critical path: S-2 → M1 → S-1 → M2 → S-3 → M4 → S-5 → M6 → M7 → M10.
* Highest technical risks: **S-3** (order-independent erosion), **S-5** (cross-tile roads), **S-1** (renderer adoption or build), cost of grass/trees on integrated GPUs.
* Highest external risks: single-maintainer dependencies, pre-release `big_space`, MPL-2.0 handling, dataset licenses.
* Cheap wins available immediately (before M1): move tangent generation off the main thread in the Lab (removes the 5.6 s / 27.7 s freeze, B1); track `Cargo.lock`; add CI.

## 6. Owner decisions needed

1. ~~Confirm ADR-001~~ **Done** — owner confirmed 2026-11-14.
2. ~~Push `bevy-0.20-veilbound` to the `dinosath` fork~~ **Done** 2026-11-14 (`dinosath/symbios-ground-lab`, branch `bevy-0.20-veilbound`).
3. Approve licensing posture (project `MIT OR Apache-2.0`; MPL-2.0 unmodified dependency allowed; `bevy_water` blocked until license files appear).
4. Permit building/running third-party examples for spikes S-1/R-3 — owner approved 2026-11-14; outcome recorded in `docs/benchmarks/s1-bevy-terrain.md` if it ran.
5. Decide hardware for gate re-baselining (discrete GPU).
