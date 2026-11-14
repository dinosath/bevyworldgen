# Migration plan — adopting, upgrading, extracting, replacing, removing

Phase 1 deliverable 9. Implements ADR-001. Baseline for "existing code": Symbios Ground Lab at
`bevy-0.20-veilbound` (`d391473`, local, unpushed) — the Bevy-0.20 port of upstream `main` (`7358b00`).
Dispositions: **Adopt** (move with attribution), **Adapt**, **Reference** (read, re-implement), **Replace**, **Retire**.
Sizes are current LOC.

## 1. Order of operations

| Step | What | Done when |
|---|---|---|
| 0 | Owner pushes `bevy-0.20-veilbound` to the `dinosath` fork (preserves the port; not done by me) | branch visible on the remote |
| 1 | New workspace `bevy-worldgen` (already `git init`ed with `docs/` and `bench/`): `LICENSE-MIT`, `LICENSE-APACHE`, `THIRD-PARTY-NOTICES.md`, `deny.toml`, `rust-toolchain.toml`, tracked `Cargo.lock`, CI | CI green on an empty workspace |
| 2 | Pin dependencies (below); `cargo deny` clean; wasm32 `cargo check` clean | M0 exit |
| 3 | Create Layer A crates and port the **benchmark crate** into the workspace as `apps/bench` | baseline numbers reproduce within noise |
| 4 | Extract Lab modules per §2 into the new crates, commit-by-commit with provenance | each import logged in `license-inventory.md` §6 |
| 5 | Build the chunk pipeline (M1–M2) and **retire** Lab resources/systems per §2 | Lab-derived code paths no longer on the hot path |
| 6 | Rebuild the Lab UI as `apps/lab` on top of the framework (M9) | feature parity list in §4 |

## 2. Module-by-module disposition

| Lab path (LOC) | Disposition | Destination / action | Reason (evidence) |
|---|---|---|---|
| `core/config.rs` (248) `TerrainConfig` etc. | **Adapt** | split into per-plugin config resources (`TerrainConfig`, `ErosionConfig`, …) with `serde` | config shape is fine; the *global dirty flags* and `CurrentHeightMap` are not (audit B3) |
| `core/config.rs` `DirtyFlags`, `DirtyMesh`, `GenerationTask`, `ErosionVizState` | **Retire** | replaced by chunk entities + `GenVersion` + messages | single-slot, un-cancellable (B2, B3) |
| `core/urban_config.rs` (128), `architecture_config.rs` (147), `material_config.rs` (243) | **Adapt** | config types for city/architecture/material plugins; default CGA grammar moves to a data file | reusable parameter sets; defaults used in baseline |
| `logic/generation.rs` (197) | **Reference** | stage order (generate → roads → lots → carve → erode) informs `worldgen-terrain` chunk pipeline; the monolithic task is retired | stage order is the Lab's value; granularity is wrong (B2) |
| `logic/erosion_viz.rs` (346) | **Adopt (later)** | `apps/lab` educational visualisation | useful editor feature; not on the framework path |
| `visuals/terrain.rs` (196) | **Replace** | chunk renderer (S-1) | single mesh; main-thread tangents (B1, B10) |
| `visuals/splat_material.rs` (98) + `shaders/splat.wesl` (242) | **Adopt + extend** | `bevy_worldgen_render` terrain material: more layers, climate weights, triplanar, wetness | already correct against 0.20 WESL/`ExtendedMaterial` |
| `visuals/water_material.rs` (~40) + `water.wesl` (49) | **Retire** | water renderer (S-8) | cuboid + sine normals only |
| `visuals/roads.rs` (135), `road_materials.rs` (137) | **Adapt** | road ribbon realisation per tile; asphalt material | geometry from `symbios-tensor` reused; lifecycle (despawn-all) replaced |
| `visuals/buildings.rs` (127), `building_materials.rs` (317) | **Replace** | baked per-building meshes + HLOD (S-7) | one entity per terminal; main-thread derivation (B4) |
| `visuals/kit_buildings.rs` (198), `visuals/street_props.rs` (130), `visuals/polyhaven.rs` (65), `kit/*` (1,314) | **Adopt** | `worldgen-city` kit assembler + props (MIT, © TheJanusStream — keep notices) | not tied to the singleton model; carries CC0 asset conventions |
| `visuals/export.rs` (451) | **Adopt** | `worldgen-export` (PNG/OBJ/JSON/glTF later) | pure functions + tasks; adapt to chunk sets |
| `visuals/urban_gizmos.rs`, `droplets.rs` | **Adopt (later)** | `apps/lab` debug overlays | editor-only |
| `ui/*` (1,104) | **Adopt (later)** | `apps/lab` panels rewired to framework resources | UI is largely independent of data model |
| `smoke.rs` (58), `main.rs` (135) | **Retire / replace** | bench harness (`apps/bench`), app plugins | env-var polling per frame (B8); no plugin boundary (B6) |
| `assets/` (81 files, 71 MB, CC0 Poly Haven) | **Adopt** selectively | `assets/` with `ASSETS.toml` provenance; large binaries via Git LFS or fetch script | CC0; size would bloat a code repo |
| `rust-toolchain.toml`, `.cargo/config.toml` (getrandom wasm flag) | **Adopt** | workspace root | needed for wasm builds |
| `.github/workflows/deploy.yml` | **Adapt** | Pages demo workflow; add real CI (B12) | |
| `.gitignore` (`Cargo.lock`) | **Replace** | track the lockfile (B11) | reproducibility |

Totals (from per-file LOC): ≈ 1.1 k lines retired/replaced (`generation.rs`, `terrain.rs`, water, `buildings.rs`, `building_materials.rs`,
`smoke.rs`, `main.rs`), ≈ 3.3 k adopted/adapted (`kit/`, kit buildings, props, polyhaven, splat material, export, roads,
road materials, config types), ≈ 1.6 k editor-only deferred (`ui/`, `erosion_viz.rs`, gizmos, droplets); the remainder is module glue.

**Reconciliation with ADR-001 §2.** The ADR's "≈60 % of the Lab (3,964 of 6,618 lines in `core`/`logic`/`visuals`) reads or writes the
single-heightmap singletons" is the cost of *staying in the Lab* (Option A): all of it must be reworked in place. Here, most "adopted/adapted"
files still need rewiring from `CurrentHeightMap`/flags to chunk queries and messages — the difference is that it happens in new, tested
crates with a different data model, rather than inside an application whose resources and schedule assume one world.

## 3. Dependency migration

| Dependency | Lab branch | Workspace pin | Notes |
|---|---|---|---|
| `bevy` | 0.20 (default features) | `0.20.1`, `default-features = false`, explicit list | removes `alsa`/`libudev` build requirement; document system deps if audio is added later |
| `symbios-ground` | 0.4 | `=0.4.2` | exact pin while 0.x |
| `symbios-tensor` | 0.7 | `=0.7.0` | |
| `symbios-shape` | 0.5 | `=0.5.0` | |
| `symbios-texture` | (via `bevy_symbios_texture`) | `=0.8.1` | |
| `bevy_symbios_ground/_shape/_texture` | 0.8 / 0.11 / 0.14 | same pins, **only where still needed** | `bevy_symbios_ground`'s whole-map mesher and async helper are superseded by the chunk pipeline; keep the Avian collider helper if it fits |
| `bevy_egui` / `bevy_panorbit_camera` | 0.43.1 / 0.36 | editor app only | |
| new | – | `bevy_grass 0.20.x`, `bevy_erosion_filter 0.3`, `big_space =0.14.0-rc.0`, `avian3d 0.8` | each isolated behind an adapter trait |
| conditional | – | `mate-h/bevy_terrain` (vendored/patched) | only if S-1 passes; it pins git Bevy and a `big_space` fork |

Bevy migration practice (per release): read the official migration guide, upgrade on a branch with a frozen golden-hash
corpus, keep shaders in WESL, avoid private render-graph APIs, run the frame-time bench before/after.

## 4. Feature parity checklist for the Lab port (Phase 9)

Generators (FBM/DS/Voronoi) ✔ via Layer A options · erosion controls · thermal controls · erosion visualisation · urban panel ·
architecture grammar editor · material panels · export (PNG/OBJ/JSON) · web build · smoke/screenshot harness.

## 5. Risks in the migration itself

| Risk | Mitigation |
|---|---|
| Provenance loss when copying MIT code | per-file header + `license-inventory.md` §6 entry per import; keep upstream commit SHA in commit message |
| Behavioural drift of pinned libraries | golden hashes (bench crate already fingerprints heightfields) |
| Large assets in git | LFS/fetch script; `ASSETS.toml` stays in git |
| Divergence from upstream Lab | the Lab remains a fork with an unchanged upstream relationship; framework changes live in the new workspace |
| Unverified assumptions about third-party renderers | spikes S-1/S-8 precede any import |

## 6. What this plan deliberately does not do

* It does not rewrite roads or CGA because the visuals are weak (algorithm quality ≠ rendering quality).
* It does not refactor the Lab's `.chain()` for parallelism (audit §9: access conflicts, no measured gain).
* It does not touch the `dinosath` fork or upstream; every action touching those is an owner decision (ADR §7).
