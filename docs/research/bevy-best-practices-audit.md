# Bevy best-practices audit — Symbios Ground Lab (Bevy 0.20 branch `bevy-0.20-veilbound`, `d391473`)

Phase 1 deliverable 3. Scope: the Lab application code (`src/`), measured against current Bevy (0.20.1)
conventions and the requirements in the master prompt §4. Locations are `file:line` in that branch.
API names recommended below were **checked to exist in `bevy_*-0.20.1` sources** (e.g. `bevy_tasks::futures::check_ready`,
`bevy_ecs::message::Messages`, `bevy_ecs::schedule::common_conditions::resource_changed`, `Mesh::generate_tangents(&mut self)`).
Severity = effect on the framework goals (not on the Lab as an editor): **H** blocks streaming/scale, **M** costs
correctness/perf/maintainability, **L** hygiene.

## 1. What is already good (keep)

| Practice | Where |
|---|---|
| Public Material API only: `ExtendedMaterial<StandardMaterial, _>` + `MaterialExtension` for splat and water; no private render-graph code | `visuals/splat_material.rs`, `water_material.rs` |
| Shaders as embedded assets (`embedded_asset!`) in WESL, ported to the 0.20 shader pipeline | `visuals/mod.rs:40–49` |
| Current messaging API (`MessageWriter<AppExit>`), `EguiPrimaryContextPass` schedule | `smoke.rs`, `main.rs:2,90` |
| Heavy work (generation, export) on `AsyncComputeTaskPool`, never in-frame | `generation.rs:36`, `export.rs:123,162,419` |
| Mesh asset overwritten in place on rebuild (no orphaned asset churn) | `terrain.rs:183–190` |
| Placeholder array textures so the bind group is valid from frame 0 | `terrain.rs` `spawn_terrain` |
| Generation is a pure function of config → testable; library crates carry the algorithms | `generation.rs:71` |
| `Changed`-style guards on building/prop rebuilds (`is_changed()`) | `buildings.rs:36`, `street_props.rs:46` |
| Rust 2024 edition, rustfmt+clippy components pinned in `rust-toolchain.toml` | repo root |

## 2. Deviations and recommended corrections

| # | Sev | Location | Deviation | Correction |
|---|:-:|---|---|---|
| B1 | **H** | `visuals/terrain.rs:176` (`mesh.generate_tangents()`) | Mikktspace over every vertex **on the main thread**, plus `.expect(...)` (panics on failure). **Measured: 5.64 s @1.05 M verts, 27.7 s @4.2 M verts** (baseline.md §4). | Generate tangents inside the worker that builds the chunk `Mesh` (`Mesh` is `Send`; `generate_tangents` takes `&mut self`) and handle the `Result`; better, derive the TBN from the height field in the fragment shader so no tangent attribute exists. |
| B2 | **H** | `logic/generation.rs:23–40` | One task generates heightmap + roads + lots + carving + erosion for the whole world; no cancellation, no generation id; `ErosionVizState.abandoned_init_tasks` (`core/config.rs:190`) exists only to count un-cancellable tasks. | Per-chunk jobs with a `GenerationVersion` carried through the task and checked on completion (stale results dropped), budgeted spawn queue, cooperative cancellation flag (`Arc<AtomicBool>`) checked at stage boundaries. |
| B3 | **H** | `core/config.rs:124–127,155` (`DirtyFlags`, `DirtyMesh(bool)`), `terrain.rs:97–102` (single `TerrainMesh` entity) | Bool-flag resources and `CurrentHeightMap(Option<HeightMap>)` as one global slot. Cannot represent N chunks, versions or partial completion. | Entities per chunk with `ChunkId` + `Handle`/component state machine (`Requested → Generating → Ready → Resident → Evicting`), `Message<ChunkGenerated>` on completion instead of flags. |
| B4 | **H** | `visuals/buildings.rs:36–52`, `street_props.rs:46–53`, `roads.rs:49` | Despawn **all** and re-derive CGA on the **main thread** on any change; `bevy_symbios_shape::spawn_shape` makes one entity per terminal (≈385 per building, measured). | Derive in tasks; bake terminals to per-building (or per-material) merged meshes; LOD via `VisibilityRange`; incremental per-lot rebuild keyed by lot id + seed. |
| B5 | M | `main.rs:103–127` | 20 systems under one `.chain()`: every system runs every frame and early-returns. Most take `ResMut<Assets<Mesh>>`/`ResMut<CurrentHeightMap>`, so removing the chain would **not** create parallelism (access conflicts). | Do not chase parallelism here (see audit §9). Replace flag-polling with `run_if(resource_changed::<T>)`/message-driven systems and named `SystemSet`s (`WorldGenSet::{Request, Collect, Realise, Upload}`) so *ordering* is explicit and testable. |
| B6 | M | `main.rs` (whole file) | No application plugin: ~17 `init_resource`s and every system are registered in `main()`, so nothing is reusable or testable headlessly. | `LabPlugin` per subsystem (`TerrainPlugin`, `UrbanPlugin`, `EditorUiPlugin`) with documented config resources; see target-architecture. |
| B7 | M | `visuals/buildings.rs:30` (`mut _dirty: ResMut<DirtyFlags>`) | Unused mutable resource parameter: claims exclusive access, forces ordering against every other `DirtyFlags` user. | Remove. |
| B8 | M | `smoke.rs:11`, `main.rs:99,132` | `std::env::var` parsed inside run-condition closures and systems **every frame**. | Parse once into a `Resource` at startup; gate with `run_if(resource_exists::<Smoke>)`. |
| B9 | M | `logic/generation.rs:50`, `erosion_viz.rs:73,78`, `export.rs:191` | `future::block_on(future::poll_once(t))` polling idiom. Works, but Bevy now ships `bevy_tasks::futures::check_ready`, which is the supported non-blocking poll. | Use `check_ready(&mut task)`. |
| B10 | M | `visuals/terrain.rs:157` + `bevy_symbios_ground::HeightMapMeshBuilder` | CPU mesh for every cell at full resolution (1.05 M verts at 1025²); no LOD, no skirts, no frustum granularity, no `VisibilityRange`. | Chunked meshes with 3–5 LOD levels and skirts (or the clipmap/quadtree renderer chosen in the ADR). |
| B11 | M | Repo: `.gitignore:5` (`Cargo.lock`) | Application with an untracked lockfile ⇒ non-reproducible builds, and the 0.20 port is not reproducible from git. | Track `Cargo.lock` for binaries/examples; pin library versions with `=x.y.z` for the Symbios crates while they are 0.x. |
| B12 | M | `.github/workflows/deploy.yml` (only workflow) | No `cargo fmt --check`, `clippy -D warnings`, `cargo test`, or wasm `check` on PRs. 11 tests in the app. | Add a CI workflow (fmt, clippy, test, `cargo check --target wasm32-unknown-unknown`, `cargo deny`). |
| B13 | M | `terrain.rs:176`, 11 other `unwrap/expect` outside `kit/` | Panics on recoverable asset/mesh errors. | Return/log errors; keep `expect` for invariants only. |
| B14 | L | `visuals/water_material.rs`, `terrain.rs` `spawn_terrain` (water cuboid) | Water is a scaled `Cuboid` + `cull_mode: None`; extension has no uniforms (no depth, flow, foam). | Replace with a proper water renderer; keep `ExtendedMaterial` pattern. |
| B15 | L | `Cargo.toml` | `bevy` with **default features** (audio, gamepad → `alsa`/`libudev` system deps) although the app plays no audio; this is what failed my first build on a clean host. | `default-features = false` + explicit feature list (the list that built here is in `baseline.md`/patch). Document system dependencies in the README. |
| B16 | L | `README.md` WASM note | CPU jobs "contain no `.await` yield points… block the tab". | Web Worker offload (Overlands pattern) rather than yielding, because generation is CPU-bound pure code. |

## 3. Assessment against prompt §4 requirements

| Requirement | Status in Ground Lab | Gap |
|---|---|---|
| 4.1 ECS: components/resources/systems/plugins, change detection, States | Resources + systems; no components for world content; no `States`; flags instead of change detection in places | B3, B5, B6 |
| 4.1 Dense data outside ECS | Yes: `HeightMap` is a flat `Vec<f32>` in a resource (**correct**), road graph likewise | keep |
| 4.2 Plugins with documented config | Two small plugins only | B6 |
| 4.3 Scheduling | One long chain | B5 / audit §9 |
| 4.4 Async: queues, budgets, cancellation, versions, prioritisation | single task, none of the rest | B2 |
| 4.5 Public render APIs, current wgpu/Bevy conventions | **Good** (ExtendedMaterial, embedded WESL) | – |
| 4.6 Asset lifecycle / budgets | in-place mesh overwrite good; no budgets; 3.4 GB peak at 2049² | needs a `WorldBudget` resource |
| 4.7 Determinism | Library-level yes (seeded, libm); app-level order-independent *only because* it is single-job | keep Layer A discipline; add per-chunk, order-permutation tests |
| 4.8 Testing/quality | 1 → 11 tests; no CI quality gates; lockfile ignored | B11–B13 |

## 4. Notes on dependent crates (spot checks, [read])

* `bevy_symbios_ground::async_gen` offers `AsyncTerrainPlugin` + `spawn_heightmap_task`/`poll_pending_heightmaps`:
  a sensible shape, but whole-map and without versioning. The Lab does not use it (it rolled its own), which is a
  duplication to remove when the framework adopts per-chunk jobs.
* `bevy_symbios_shape::ShapeMeshCache` is a **bounded LRU** keyed by bit-exact floats — a good lifecycle pattern to
  keep (prompt §4.6).
* All `bevy_symbios_*` crates depend on Bevy with `default-features = false` and explicit lists — the correct
  library practice; the application should do the same (B15).
