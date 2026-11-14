# ADR-001 — Foundation selection

* **Status:** Proposed (Phase 1 outcome, awaiting owner confirmation)
* **Date:** 2026-11-14
* **Deciders:** project owner; analysis by Claude (Phase 1)
* **Supersedes:** none. **Evidence:** `docs/research/*`, `docs/benchmarks/baseline.md`.

## 1. Decision

**Build a new modular workspace (`bevy-worldgen`) that integrates specialised, permissively licensed libraries
around the Symbios generation crates as its engine-independent core — i.e. Option C, structured as Option B.
Do not extend Symbios Ground Lab as the foundation, and do not start from scratch.**

Concretely:

1. **Layer A (engine-free generation) reuses** `symbios-ground`, `symbios-tensor`, `symbios-shape`,
   `symbios-texture` (MIT, Bevy-free, tested, deterministic) behind our own thin adapter crates, **pinned exactly**
   (`=0.4.2`, `=0.7.0`, `=0.5.0`, `=0.8.1`) while they are 0.x.
2. **New code is written only where the evidence shows a gap** (§4): seamless world-space terrain sampling,
   chunk-border-correct erosion, chunk scheduling/jobs/budgets, cross-tile road topology and a highway layer,
   building baking/HLOD, climate/hydrology/vegetation ecology, GIS adapters.
3. **Layer C adopts existing renderers/libraries** rather than inventing them: Bevy built-ins (atmosphere,
   occlusion culling, visibility ranges), `bevy_grass`, `bevy_erosion_filter` (unmodified, MPL-2.0), `big_space`
   (pinned rc) — and a terrain LOD renderer chosen by **spike S-1** between `mate-h/bevy_terrain` and an in-house
   geomipmapped chunk renderer.
4. **Ground Lab becomes an editor/reference app** that later consumes the framework (Phase 9). Its `kit/`,
   splat material, export and egui panels are migrated; its data model is not.
5. **Upstream stance:** composition first, upstream contribution second, fork last (§7).

## 2. Context (what the evidence showed)

* The Lab is a **6.6 k-line application** over libraries that already exist independently. `core/` (788) + `logic/` (554) +
  `visuals/` (2,622) = 3,964 lines (≈60 %, an upper bound — some files in them, e.g. materials, do not touch the singletons) sit in modules built around the single-heightmap / single-mesh / one-task resources
  (`CurrentHeightMap`, `TerrainMesh`, `GenerationTask`, `DirtyFlags`), so adding streaming and LOD *in place* means reworking
  them. What is self-contained and portable: `kit/` (1,314), the splat/road/kit-building shaders and materials, export, and
  most UI widgets (see migration-plan §2). The portable part is not the hard part.
* The libraries it uses were **all republished for Bevy 0.20 on 2026-11-08/09**. The local `bevy-0.20-veilbound`
  branch proves the Bevy migration is a dependency bump plus shader/API fixes (WESL port), not a rewrite.
* **Measured failures are integration-level**, not algorithmic (baseline.md): 5.6 s / 27.7 s main-thread freeze
  from tangent generation (1025² / 2049²); seams 59–117× interior steps with per-tile generation; road
  rationalization ≈O(n²) (37.9 s at 2049²) so city generation must be tile-bounded; one entity per CGA terminal
  (≈385 per building).
* **Algorithms are fast at tile scale:** 513² terrain with default erosion ≈ 150 ms, whole 513² city tile ≈ 86 ms,
  10 k CGA buildings ≈ 1 s, and 16 independent erosion jobs run at 11.9× single-job throughput on 8 cores.
* **Determinism and WASM are already engineered into the libraries** (`libm`, seed streams) and Overlands shows a
  working Web-Worker offload pattern (Apache-2.0). Overlands is **not** a large-world engine (bounded rooms; far field removed on purpose).
* Ecosystem availability: no Rust/Bevy library supplies roads, CGA cities or climate; Bevy 0.20 supplies
  atmosphere/occlusion/HLOD; vegetation (`bevy_grass`), detail erosion (`bevy_erosion_filter`), floating origin
  (`big_space`) and a streaming terrain renderer (`mate-h/bevy_terrain`) exist with 0.20-era builds.

## 3. Options evaluated

| | A — extend `dinosath/symbios-ground-lab` | B — new app around Symbios crates | **C — integrate several libraries (chosen, built as B)** | D — from scratch |
|---|---|---|---|---|
| Reuses | Everything in the Lab | Layer A libs; selected Lab modules (kit, splat, export) | B + vegetation, detail erosion, floating origin, terrain renderer, Bevy built-ins | Nothing |
| Cost to reach streamed terrain | Rework ≈60 % of the Lab in place (`core`, `logic`, `visuals`), then add jobs/LOD | Write chunk layer + renderer adapter; no legacy to unwind | B's cost minus the renderer/vegetation/water work that exists | Everything incl. noise, erosion, roads, CGA (≈21 k LOC of tested terrain/road/CGA code re-done; ≈49 k with texture generators) |
| Bevy 0.20 | local unpushed branch; `main` is 0.18 | libs already 0.20 | libs 0.20; some deps pre-release (`big_space` rc) or pinned to git Bevy (`mate-h/bevy_terrain`) | free |
| Performance risk | inherits app-level stalls until rewritten | none inherited | integration risk on third-party renderers (mitigated by spike S-1) | highest (unproven) |
| Maintenance risk | stale upstream (last push 2026-03-21, 0 PRs ever, 0 releases) | single-maintainer libs (pin + vendor-ready MIT) | adds ≈4 more small-maintainer deps (each behind an adapter) | all on us |
| License | MIT ✔ | MIT ✔ | MIT/Apache ✔; MPL-2.0 (`bevy_erosion_filter`) conditional; GPL/unlicensed candidates excluded | n/a |
| Verdict | **Reject as foundation** (keep as editor/reference) | Right structure | **Chosen** | **Reject** (no evidence of unfixable limits) |

### Why not A
The Lab's limitations are in *its own* layer (`CurrentHeightMap(Option<_>)`, one `TerrainMesh`, one task, `DirtyFlags`
bools, despawn-all building rebuild). Fixing them means replacing nearly every system, after which the "fork" shares
little with its upstream. Upstream offers no review path to absorb the change (0 PRs ever; one author; 8 months idle).
Keeping the Lab as a thin editor consumer preserves its UI/kit value without letting its resource model constrain the
framework.

### Why not D
The prompt's own criteria for D are *unfixable* limits: incompatible data representation, licensing, unacceptable
performance, irreconcilable architecture, coupling, inability to support streaming/LOD. None holds: `HeightMap` is a flat
`Vec<f32>` (ideal), licenses are MIT, performance problems are in the app layer or are fixed by tiling, the
libraries are Bevy-free, and they already provide `TiledHeightMap`/`CityStreamer`/per-tile seeds. Starting fresh would
re-implement ≈21 k lines of tested terrain/road/CGA code (`symbios-ground` 2.9 k + `-tensor` 7.6 k + `-shape` 10.5 k; ≈49 k including
`symbios-texture`'s 28.3 k) for no measured advantage.

## 4. What is reused, extended, replaced, or new

| Subsystem | Disposition | Detail |
|---|---|---|
| Value-noise FBM, Diamond-Square, Voronoi terracing | **Reuse as options** | keep for presets/legacy parity |
| Seamless world-space heightfield | **New (Layer A)** | pure `f(x,z,seed)` so adjacent chunks match bitwise; propose upstreaming a `WorldNoise` sampler to `symbios-ground` |
| Hydraulic/thermal erosion | **Reuse + extend** | add chunk-border policy (S-3: order-independent delta accumulation on the base field with fixed-point accumulators); `bevy_erosion_filter` for render-time detail |
| `TiledHeightMap`, `derive_tile_seed` | **Reuse concepts** | seed derivation reused; storage replaced by our chunk store |
| Climate, biomes, hydrology (rivers/lakes) | **New** (algorithm survey R-2 incl. Nerthus) | |
| `symbios-tensor` roads/blocks/lots/carve/meshes | **Reuse** | tile-bounded (≤512 cells); adapter adds border *ports* and constraints from a coarse highway layer (**New**, S-5) |
| `symbios-shape` CGA | **Reuse** | derive off-thread; **new** baking to per-building merged meshes + HLOD (S-7) |
| `symbios-texture` procedural PBR | **Reuse** as fallback/LOD material generator; CC0 Poly Haven for hero materials |
| Terrain rendering/LOD | **Replace** Lab's single mesh | S-1: `mate-h/bevy_terrain` vs in-house chunk renderer |
| Vegetation | **Adopt** `bevy_grass`; **new** tree instancing/impostors + ecology placement | |
| Water | **New/adopt** (S-8): `bevy-aqua` or custom; drop Lab cuboid | `bevy_water` blocked by license-file gap |
| Atmosphere, fog, occlusion, HLOD | **Use Bevy built-ins** | |
| Large coordinates | **Adopt** `big_space` (pinned rc) behind a `WorldOrigin` adapter; in-house origin shift as fallback | |
| Async jobs | **New** job system; **adopt Overlands' `gen-jobs`/offload/worker pattern** (Apache-2.0, vendor with notice) for WASM | |
| GIS | **New** adapters on `proj4rs`, `tiff`, `osmpbf`, `geo`; pattern from Overlands `geodata` | |
| Editor | **Later**: Ground Lab UI + `jackdaw_editor_terrain`/`bevy_arc_studio` patterns | Phase 9 |

## 5. Integration / migration costs (estimates, ±50 %, to be revised at each milestone exit)

| Work | Estimate |
|---|---|
| Workspace, CI, license tooling, benches | 3–4 dev-days |
| Layer A core (ids, RNG streams, version/cancel, budgets) | 1 week |
| Seamless sampler + chunk generator + golden tests | 1–1.5 weeks |
| Chunk job/streaming plugin + minimal renderer (single LOD) | 2 weeks |
| Terrain LOD renderer (adopt vs build, after S-1) | 1–4 weeks |
| Chunk-correct erosion (S-3) | 1–2 weeks (research risk) |
| Cross-tile roads + highways (S-5) | 3–4 weeks (research risk) |
| Building bake + HLOD (S-7) | 2 weeks |

Estimates are order-of-magnitude planning numbers, not measurements.

## 6. Dependencies (initial pins)

`bevy = 0.20.1` (default features off; explicit list), `symbios-ground =0.4.2`, `symbios-tensor =0.7.0`,
`symbios-shape =0.5.0`, `symbios-texture =0.8.1`, `bevy_symbios_ground/_shape/_texture` at the versions the Lab branch locks,
`bevy_grass 0.20.x`, `bevy_erosion_filter 0.3`, `big_space =0.14.0-rc.0` (pre-release, adapter-isolated), `avian3d 0.8`,
`libm`, `rayon`/Bevy task pools, `proj4rs`/`tiff`/`osmpbf`/`geo` (Phase 9). `mate-h/bevy_terrain` only if S-1 passes
(vendored/patched, since it pins git Bevy).

## 7. Upstream and fork relationship

* Do **not** fork `symbios-ground-lab` for the foundation. Keep `dinosath/symbios-ground-lab` as a fork of
  `TheJanusStream/symbios-ground-lab` (currently 0 ahead / 0 behind, same SHA `7358b00`).
* The Bevy-0.20 port currently exists **only locally** (`bevy-0.20-veilbound`, `d391473`). *Owner action:* push it to the
  fork (not done by me). It is a plain dependency-bump port and could be offered upstream as a PR, though upstream has
  never merged one.
* Contribute to Symbios libraries only where a change is generically useful and small (world-space sampler,
  apron support, border-port hooks). If upstream is unresponsive, carry the change **as a layered crate over the public API**,
  not as a fork.
* Vendoring trigger: if `symbios-ground` or `symbios-tensor` publish nothing for 6 months **or** break semver
  contracts we depend on, vendor them (MIT) with notices.

## 8. Performance implications (all from baseline.md)

* Tile size policy: **terrain tiles 257–513 cells; road/city tiles ≤ 512 cells.** Rationalize cost is ≈O(n²), so bounded
  tiles turn a 37.9 s monolith into ≈86 ms units.
* Erosion budget: 50 k droplets ≈ 136 ms on one core ⇒ a 16-thread pool sustains ≈ 75 such tiles/s (measured 76.7 jobs/s).
  Flying at 100 m/s across 512 m tiles needs ≈ 0.2 tiles/s of *new* tiles — two orders of magnitude of headroom, so CPU
  generation is not the streaming bottleneck; **main-thread uploads and GPU cost are.**
* Main-thread rule: no per-chunk main-thread work > 1 ms; tangents/normals/meshes built in workers (removes the 5.6 s freeze).
* GPU: the Lab is GPU-bound at 8.7 ms in one pass with ≈0.5 M triangles on an iGPU ⇒ LOD + occlusion are mandatory, not optional.
* Memory: define `WorldBudget` (CPU MB, GPU MB, in-flight jobs, bytes uploaded/frame). Baseline peak 3.38 GB at 2049² is the cost
  of whole-world data.

## 9. Maintenance risks and mitigations

| Risk | Likelihood / impact | Mitigation |
|---|---|---|
| Single maintainer of Symbios libs (bus factor 1; 0.x semver churn) | medium / high | exact pins; adapters in our crates; MIT ⇒ vendorable; contribute small PRs; golden-hash tests detect behavioural drift |
| `big_space` 0.14 is a release candidate; stable targets Bevy 0.19 | medium / medium | `WorldOrigin` adapter; fallback in-house origin shifting; upstream tracking |
| `mate-h/bevy_terrain` pins git Bevy, no CI, 7 tests, one maintainer, not on crates.io | high / medium | spike-gated; if adopted, vendor + own CI; in-house chunk renderer fallback defined |
| MPL-2.0 dependency (`bevy_erosion_filter`) | low / low | unmodified dependency; or reimplement from the published description |
| Bevy 0.21 churn | high (annual) / medium | public APIs only; WESL; no private render-graph code; migration notes per release |
| WASM determinism unmeasured | medium / high for networked use | S-6 adds a wasm32 golden-hash CI job |

## 10. Long-term flexibility

Layer A has no Bevy dependency, so the engine, renderer, or even the host (server-side baking, other engines) can change
independently; each third-party library sits behind a trait defined in our crates (`TerrainRenderer`, `VegetationRenderer`,
`WorldOrigin`, `JobBackend`), so replacing any one is a local change.

## 11. Rejected alternatives (and reasons)

| Alternative | Reason |
|---|---|
| Extend the Lab in place | §3 — reworks ≈60 % of it; no upstream review path |
| Rewrite everything from scratch | §3 — no unfixable limitation found; ≈21 k LOC of tested core code discarded |
| Adopt Symbios Overlands as the base | application tied to ATProto/WebRTC; bounded worlds; its terrain pipeline is the same whole-room heightmap; crates not published. Used as a *pattern source* only |
| Adopt Nerthus as the generation core | surface inspection only; large app/editor; IP question over "clean-room Gaea" (L-2); revisit via R-2 for algorithms |
| `bevy-open-world` | **GPL-3.0** — license gate fail |
| `mate-h/bevy_erosion` | no license — gate fail |
| `feldspar` | Bevy 0.8-era, abandoned 2022, voxel representation |
| `rust-procedural-world` | 814-line 2D demo, Bevy 0.12 |
| Original `kurtkuehnert/bevy_terrain` | Bevy 0.14, dormant since 2024-07; superseded by the continuation |
| Treat the 20-system `.chain()` as the scale blocker | audit §9: access conflicts mean no parallelism gain; real blockers are blocking tangents and the data model |

## 12. Consequences and what would change this decision

* Positive: no legacy to unwind; tested algorithms preserved; engine-independent core; clear upgrade path.
* Negative: more adapter code; dependence on small projects; some work (S-1…S-8) is research.
* **Reopen this ADR if:** (a) S-3/S-5 show the Symbios erosion/road algorithms cannot be made chunk-correct without a rewrite
  (then reconsider D for those subsystems only); (b) S-1 shows neither `mate-h/bevy_terrain` nor an in-house renderer meets the
  streaming frame gate on the reference machine; (c) the maintainer stops publishing and vendoring proves heavier than estimated;
  (d) Nerthus (R-2) turns out to be a better generation core after a deeper audit.

## 13. Evidence gaps acknowledged (none changes the decision, several change later choices)

Not measured: runtime of `mate-h/bevy_terrain`, `bevy_grass`, `bevy_erosion_filter` here; VRAM; per-system CPU time; the
unattributed ≈37 ms/frame at 2049²; wasm32 builds/determinism; upstream `main` (Bevy 0.18) runtime (build blocked by a host
`libwayland` dev-package gap). See baseline.md §6.
