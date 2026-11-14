# bevy-worldgen

A modular, deterministic, open-source world-generation framework for Bevy (0.20), aimed at open-world driving
environments: streamed mountain terrain with erosion, rivers, biomes, vegetation, highways and bridges, towns and
buildings, dynamic sky/weather, real-world GIS import and a small in-engine editor.

**Status: milestones M0–M10 are implemented; several gates are only partly met on the reference iGPU.** Read
[`docs/benchmarks/m2-m10-results.md`](docs/benchmarks/m2-m10-results.md) for what was measured, what was missed and what was
never measured, and [`docs/architecture/ADR-002-implementation-deviations.md`](docs/architecture/ADR-002-implementation-deviations.md)
for where the build departs from the plan.

## Layout
| Crate | Layer | Role |
|---|---|---|
| `worldgen-core` | A (engine-free) | `ChunkId`, seeded RNG streams, `GenVersion`, `CancelToken`, `WorldBudget`, fingerprints |
| `worldgen-terrain` | A | world-space seamless height sampler, order-independent erosion, hydrology, climate/biomes, chunk generation, edit layers |
| `worldgen-jobs` | A | prioritised, versioned, cancellable job pool |
| `worldgen-veg`, `worldgen-roads`, `worldgen-city` | A | vegetation placement/baking, towns + highways + bridges + user roads, CGA buildings (via `symbios-*`) |
| `worldgen-geo` | A | `proj4rs` UTM frame, Terrarium DEM, Overpass OSM parsing, region overlay |
| `worldgen-world` | A | assembles chunks (terrain, water, vegetation, roads, buildings), quadtree planner, disk cache |
| `bevy_worldgen` | B | streaming plugin, chunk state machine, origin shifting, terrain queries |
| `bevy_worldgen_render` | C | terrain/water materials, grass (`bevy_grass`), sky/weather/quality presets |
| `apps/showcase` | – | flyable/drivable world, benchmark harness, editor |
| `apps/geo-import` | – | downloads a real region (DEM + OSM) into `data/` |
| `apps/wasm-golden`, `bench/phase1-algos` | – | native-vs-wasm32 determinism check; Phase-1 baseline algorithms |

## Run
```bash
cargo run --release -p showcase                       # fly (WASD + mouse; F1-F4 quality, T/[ ] time, 1-4 weather)
WG_DRIVE=manual cargo run --release -p showcase       # drive; WG_DRIVE=auto for the autopilot
WG_BENCH=40 WG_QUALITY=high cargo run --release -p showcase   # scripted 100 m/s flight, prints frame statistics

# real-world region (network; data stays in data/, not committed - read the attribution file it writes)
cargo run --release -p geo-import -- 7.9087 46.5934 3200 data/lauterbrunnen 14 4
WG_GEO=data/lauterbrunnen WG_START=-300,1100,-260,-170,-14 cargo run --release -p showcase
```
Editor: `E` toggles edit mode; `Z/X/C/V` raise/lower/smooth/flatten, `B` road tool (LMB points, Enter builds), `-`/`=` radius,
`Ctrl+Z/Y` undo/redo, `Ctrl+S` save (`WG_EDITS=<file>` loads/saves). All env switches are listed at the top of `apps/showcase/src/main.rs`.

## Verify
```bash
cargo test --workspace --release       # unit, property, determinism, integration
cargo clippy --workspace --release --all-targets -- -D warnings
./scripts/wasm-golden.sh               # native == wasm32 golden fingerprints (needs node + wasm32 target)
./scripts/update-golden.sh             # only when a generator change is intended; bump the version constants
```
Requires Rust >= 1.97 (built with 1.99.1). No audio/gamepad/libudev dependencies are used; on Linux `wayland-sys` is loaded with
`dlopen`, so `libwayland-dev` is not needed to build.

## Phase-1 research (still valid)
| Deliverable | File |
|---|---|
| Foundation decision | [`docs/architecture/ADR-001-foundation-selection.md`](docs/architecture/ADR-001-foundation-selection.md) |
| Departures from it | [`docs/architecture/ADR-002-implementation-deviations.md`](docs/architecture/ADR-002-implementation-deviations.md) |
| Target architecture / migration plan | [`docs/architecture/`](docs/architecture/) |
| Roadmap, gates, spikes | [`docs/roadmap.md`](docs/roadmap.md) |
| Open-source landscape, Lab audit, Bevy audit, feature comparison | [`docs/research/`](docs/research/) |
| License inventory (incl. dataset policy) | [`docs/research/license-inventory.md`](docs/research/license-inventory.md) |
| Baseline and M2-M10 benchmarks | [`docs/benchmarks/baseline.md`](docs/benchmarks/baseline.md), [`docs/benchmarks/m2-m10-results.md`](docs/benchmarks/m2-m10-results.md) |

## Licensing and data
Own code: `MIT OR Apache-2.0`. Third-party dependencies were checked against `deny.toml` (545 packages, all allowed). The
showcase ships no image/model assets (textures are procedural). Imported OSM/DEM data is **ODbL / attribution-bound**: it is
git-ignored, `geo-import` writes an `ATTRIBUTION.txt`, and the owner should confirm a policy before publishing any extract.
