# ADR-002 — Where the implementation departs from ADR-001 / the roadmap

Status: **informational record** (written at the end of M10). ADR-001's *decision* (new workspace reusing the Bevy-free Symbios
crates as Layer A, plus a new chunk/streaming/determinism layer) was followed. The items below are the places where a plan
item was replaced, narrowed or not done, with the reason. Nothing here reopens ADR-001; the owner should still confirm it.

## Replaced or not adopted

| Plan | What happened | Why / evidence |
|---|---|---|
| `mate-h/bevy_terrain` as terrain renderer (spike S-1) | **Not evaluated at runtime.** An in-house geomipmapped quadtree chunk renderer (`bevy_worldgen_render`) was built instead | The single build+run of a modified third-party example was denied by the permission layer and was *not* worked around, so S-1's comparison was never run. The in-house renderer is measured (below), but "it beats bevy_terrain" is **not** a claim this project can make |
| `big_space` for large worlds | **Not adopted.** `WorldOrigin` re-centres the render origin (threshold 2.5 km, shift snapped to 1024 m), all generation uses `f64` world metres | Pre-release dependency; the shift is ~150 lines, covered by tests, and keeps Layer B free of a load-bearing RC crate. Not measured against `big_space` |
| `bevy_erosion_filter` for detail erosion | **Not adopted.** Chunk-correct erosion is in-house (S-3: order-independent fixed-point droplets on an immutable base field, `erosion.rs`) | S-3 *passed* (borders bitwise equal, request-order independent), so the fallback was unnecessary. Visual parity with sequential erosion was reviewed by eye on screenshots only |
| `bevy_grass` | **Adopted** (0.20.6, one `GrassField` per LOD-0 chunk, rate-limited creation) | Works; cost is the largest single GPU item in High/Ultra |
| Hydrology = regional flow accumulation | **Narrowed**: rivers are *traced drainage paths* from sources per 16 km region on a two-scale gradient, with basin ponds that spill, plus lakes; water level = smoothed terrain − 2.2 m | Deterministic and seamless (tested), but it is not a full flow-accumulation simulation: no tributary-order widths from catchment area, no floodplains |
| Tunnels (S-5 / M6) | **Not implemented** | The terrain is a heightfield; a tunnel needs either a cut mesh or volumetric terrain. Highways use grade limits (7 %) and bridges instead |
| Roads: tensor-field city streets + highway layer | Implemented (towns via `symbios-tensor`; highways via own A* with grade-limited profile and bridges) | Highways are not from Symbios; they are new code with tests (91 roads / 486 km, 3 bridges, max grade 7.0 % in the test world) |
| Buildings: CGA via `symbios-shape` | Implemented; baked per building by LOD (detailed / massing / tall-only), lit and dark window glass as separate parts | Entity budget G8 met; no impostors, no interior |
| Water renderer (S-8) | Custom WESL shader (depth tint, foam). **No flow direction, no refraction/reflection pass** | `bevy-aqua`/`bevy_water` were not tried; S-8's ≤1.5 ms GPU criterion was not individually measured |
| WASM Web-Worker job backend (S-6) | **Only the determinism half**: Layer A native output equals wasm32 output (node), 8 probe chunks incl. river chunks. No Web-Worker pool, no browser build of the renderer | Time; the determinism check is the part that gates correctness |
| GIS: DEM + OSM + land cover via `tiff`/`osmpbf` | DEM (Terrarium PNG tiles) and OSM (Overpass JSON) done; **GeoTIFF, `.osm.pbf`, land cover not implemented** | `proj4rs` projection used for UTM local frame; one real region (Lauterbrunnen, 3.2 km, 4 m cells) imported and rendered |
| Editor as port of `apps/lab` | New small in-showcase editor (raise/lower/smooth/flatten brushes, user-drawn roads, undo/redo, save/load). **Not** a port of the Lab's egui UI; erosion-visualisation panels etc. are not carried over | Mouse interaction cannot be exercised by a human in this environment; terrain tools, road tool, undo and save/load are covered by unit tests and a scripted session (`WG_EDIT_SCRIPT=1`) |
| `THIRD-PARTY-NOTICES.md` | Not generated | `cargo-deny`/`cargo-about` not installed on the host; license policy checked by script instead (see `license-inventory.md`) |

## Gates (reference machine: Ryzen 7 5700G + Radeon Vega iGPU) — see `docs/benchmarks/m2-m10-results.md`

Gate outcomes, including misses, are reported in that file; this ADR deliberately does not restate numbers so there is one place to
keep correct.

## Decisions this leaves to the owner

1. Confirm ADR-001 (never explicitly confirmed during implementation).
2. Whether to spend effort on S-1 (run `bevy_terrain`) — needs permission for building/running a modified third-party example.
3. GPU hardware for a re-baseline: every GPU-bound gate miss below is on a ~2 TFLOP integrated GPU.
4. Publishing policy for any imported OSM/DEM region (ODbL share-alike; see `license-inventory.md` §5).
5. Push of the local `bevy-0.20-veilbound` branch of the Ground Lab fork (still local only; not touched).
