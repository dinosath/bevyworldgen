# S-1 — `mate-h/bevy_terrain` runtime check (run 2026-11-14, after owner approval)

Question (roadmap S-1): adopt `bevy_terrain` or keep the in-house chunk renderer? The Phase-1 attempt was blocked by the
permission layer; the owner approved it afterwards and it was run as below.

## What was run
* `mate-h/bevy_terrain` commit `30ddc75` in a scratch clone (outside this repo), manifest patched to stable crates.io
  **Bevy 0.20.1** (explicit feature list, `wayland-sys` dlopen) and built with `--no-default-features` (no `big_space`).
  Patch: `raw/s1-bevy_terrain-Cargo.toml.diff`. Build: 3 m 31 s, no source changes needed for the library.
* Its own `heightfield` example (an 8,192 m planar map from a 2,048² in-memory heightfield, gradient material, raymarched
  atmosphere, shadows with contact shadows, bloom) copied to `s1_flight` with only a scripted flight and frame statistics added
  (`raw/s1-bevy_terrain-s1_flight.rs.txt`): 12 s hold, then 60 s at 100 m/s, 1280×720, vsync off, same iGPU as all other numbers.
* Method matches `showcase` `WG_BENCH`. A first run was **invalid** (assets not found because the binary was launched outside
  `cargo run`; terrain material never loaded) and is discarded; it is kept in the log for honesty.

## Result
| | frames | p50 | p99 | max | > 50 ms |
|---|---|---|---|---|---|
| bevy_terrain `heightfield` (run 2) | 2,395 | 24.5 ms | 30.3 | 31.6 | 0 |
| bevy_terrain `heightfield` (run 3, with screenshot) | 2,358 | 24.9 ms | 30.8 | 32.2 | 0 |
| this project, `High` preset (final run) | 2,105 | 17.9 ms | 20.2 | 22.6 | 0 |
| this project, `Medium` preset | 2,567 | 14.6 ms | 16.5 | 18.6 | 0 |

Screenshot of the valid run: `raw/s1-bevy_terrain.png` (mountains with aerial perspective; renders correctly).

## How to read it (this is **not** a like-for-like comparison)
* The upstream example turns on expensive atmosphere features (raymarched atmosphere, volumetric light, contact shadows)
  and has no vegetation, water, roads or buildings; my presets differ in other ways. I did not isolate how much of the
  24.5 ms is terrain versus atmosphere/shadows, so **no claim is made that either terrain renderer is faster**.
* What the run does show: `bevy_terrain` builds and runs correctly on stable Bevy 0.20.1 with a small manifest patch, produces
  no stalls (max 32 ms, 0 frames > 50 ms) when flying at 100 m/s over a 2,048² heightfield, and stays on this iGPU's budget only
  with that scene's effects.
* Not tested: streaming from a procedurally generated infinite world (its data model is a preprocessed tile pyramid / in-memory
  heightfield), live edits, per-chunk regeneration, road/building masks, the spherical modes, its `big_space` path.

## Decision
**Keep the in-house chunk renderer; do not adopt `bevy_terrain` now.** Reason is integration, not speed: the framework's
requirements (seamless deterministic chunks generated on demand, in-place regeneration after edits and road/overlay changes,
per-vertex masks and splat/water data, skirts/LOD tied to the chunk quadtree) are met by the current data path, while adopting
`bevy_terrain` would need an adapter that feeds it generated tiles and invalidates them on edits — unproven here. Revisit if a
discrete-GPU re-baseline shows the in-house terrain pass is a bottleneck. ADR-002's row for S-1 is updated accordingly.
