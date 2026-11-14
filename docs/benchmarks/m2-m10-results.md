# Results M2–M10 — gates, measurements, and what was *not* measured

Reference machine: **Ryzen 7 5700G (16 threads) + Radeon Vega integrated GPU (RADV)**, 1280×720 window, Linux/Wayland,
release build (`cargo build --release -p showcase`), Bevy 0.20.1. The GPU is an iGPU: every GPU-bound number below is
pessimistic for the "AAA" target and **no discrete-GPU measurement exists**. Raw logs: `docs/benchmarks/raw/m*.log`.
Frame times are wall-clock frame deltas with vsync off (`WG_VSYNC` unset), first 2 s after the initial plan excluded.

## 1. Gate summary

| Gate | Target | Result | Verdict |
|---|---|---|---|
| **G1** determinism | 0 mismatches: goldens, order/thread permutations, native vs wasm32 | 6 golden chunk hashes (`GOLDEN`), request-order and thread-count independence, debug == release, **native == wasm32 for 8 probe chunks** (2 with rivers/ponds) | **PASS** (8 probe chunks, not the whole world) |
| **G1b** seams | 0 (bitwise) across shared borders | Bitwise-equal borders, including erosion, rivers, vertical and horizontal neighbours, and across the edge of an imported GIS region | **PASS** |
| **G2** frame time at 100 m/s | p50 ≤ 12, p99 ≤ 20, max ≤ 50 ms, none > 100 ms | table §2 | **PARTIAL**: Low passes all; Medium p99/max pass, p50 miss; High/Ultra miss p50 and (marginally/clearly) p99; max ≤ 50 ms and 0 frames > 50 ms at every preset in the final runs |
| **G3** main-thread cost / chunk | ≤ 1 ms median, ≤ 3 ms p99 | collect p99 0.005–0.009 ms, max ≤ 0.38 ms; realise max ≈ 0.45–0.9 ms; terrain visuals max 0.52 ms | **PASS** |
| **G4** worker cost / chunk | ≤ 150 ms | ≈ 15–23 ms mean per 32-cell chunk including erosion, roads, buildings and vegetation (EMA in logs); 400–634 chunks/s on 16 threads; worst single job ≈ 205–480 ms (cold, first chunks / contention) | **PASS on mean**; worst-case outliers exceed 150 ms |
| **G5** road/city tile ≤ 120 ms | | Not measured as a separate 512² tile; roads/lots are part of the chunk job above | **NOT SEPARATELY MEASURED** |
| **G6** building derive+bake ≤ 3 ms | | Not measured per building. Whole-town derivation is amortised in the chunk job (G4) | **NOT SEPARATELY MEASURED** |
| **G7** memory | within budget; flat over 30 min | see §3 | **PARTIAL** (bounded and plateauing over 700 s / ≈ 210 km; no 30-min RSS series) |
| **G8** entities ≤ 200 k | | **max 5,194 total, 1,234 visible** (Ultra, 40 s flight). Vegetation is baked into chunk meshes; grass is GPU-instanced | **PASS** |
| **G9** edit → visible | ≤ 250 ms, no frame > 33 ms | worst edit latency 50 ms (edit storm), worst frame 21 ms; warm disk cache chunk 0.2 ms vs 25 ms cold | **PASS** |

## 2. G2: frame time per quality preset (40 s flight, 100 m/s, 720p, iGPU)

Source: `raw/m10-presets.log` (final, after M9) and `raw/m8-presets.log` (earlier, M8).

| Preset | p50 | p99 | max | frames > 50 ms | G2 |
|---|---|---|---|---|---|
| Low | 6.8 | 7.8 | 11.5 | 0 | pass |
| Medium | 14.6 | 16.5 | 18.6 | 0 | p50 miss (14.6 > 12) |
| High | 17.9 | 20.2 | 22.6 | 0 | p50 miss; p99 miss by 0.2 ms |
| Ultra | 19.8 | 22.2 | 28.5 | 0 | p50 and p99 miss |

The M8 run of the same flight showed worse tails (Low p99 26.7, Medium max 70 ms, Ultra 11 frames > 50 ms). I did **not**
isolate why the final run is cleaner; the code changed between runs (M9/M10) but so did what else was running on the machine,
so treat the tails as "typically ≤ 30 ms on this machine, not guaranteed".
GPU pass timings from the logs (High, ms): main opaque pass ≈ 11–12, shadow passes ≈ 10, atmosphere LUTs ≈ 1.6,
light probes ≈ 2.5, bloom ≈ 0.9, FXAA ≈ 0.6. Shadows and the opaque pass dominate; this is why G2's p50 target is out of reach
on this GPU at High without further LOD/shadow work.

## 3. G7: memory

* **First measurement was misleading, and is corrected.** `StreamStats.cpu_mb` summed *generation-time* bytes, which kept counting
  meshes after they had been uploaded and taken. The 30-minute soak (`raw/m10-soak-30min.log`, High, 100 m/s, 1800 s, 98,476 frames)
  therefore reported 190 → 743 MB growth in the last 10 minutes; that was content-dependent mesh size, not retained memory.
  The same soak: **p50 18.0, p99 24.7, max 36.9 ms, 0 frames > 50 ms, collect max 0.38 ms; resident chunks capped ≤ 1367**.
  It did **not** log process RSS, so it cannot show a flat RSS.
* **Fix:** `GeneratedChunk::live_bytes()` (shrinks as meshes are taken), `WorldBudget::max_resident_bytes` (768 MiB default)
  with LRU eviction of hidden chunks, and RSS in the bench output.
* **Corrected measurement** (`raw/m10-memory-300ms-b.log`: Medium, 300 m/s for 700 s ≈ 210 km of ground, ≈ 62,000 chunk jobs):
  live chunk bytes 35–388 MB (budget never reached), resident chunks ≤ 1316, **RSS floor rises 306 → ≈ 500 MB over the first
  ≈ 400 s and then stays 493–535 MB for the remaining ≈ 300 s; transient peaks up to 954 MB** while many chunks are in flight.
* **Not shown:** a 30-minute corrected-RSS run, GPU memory (VRAM was never measured), or a bound on the per-region
  caches (river regions, town sites/content, lake cache, highway edges, town buildings are `HashMap`s without eviction; they
  grow with explored area, slowly: the RSS floor plateau over the last ≈ 80 km suggests small growth, but this is evidence, not a proof).

## 4. Other measured results

| Item | Result | Log |
|---|---|---|
| Autopilot drive, full highway | Highway #0, route 8,108 m, **reached 8,087 m**, max lateral deviation 0.66 m, off-road 0.2 s (spawn), no-ground 0.1 s, **0 s on a bridge** (this route has none), 1 frame at 50.15 ms (max), p50 23.1 / p99 30.0 ms at High, 624 s | `raw/m10-drive-full.log` |
| Earlier drive (rural route) | 3.1 km of a 10.2 km route in 260 s; deviation 0.69 m | `raw/m6-drive.log` |
| Network test world | 91 roads / 486 km, 3 bridges, max grade 7.0 %, road ends join town street nodes | `crates/worldgen-roads/tests/network.rs` |
| Bridge driving | Bridges exist and the query returns deck heights (unit-tested); **no bridge was driven in the full-route run** | |
| GIS import | Lauterbrunnen (lon 7.9087, lat 46.5934), 3.2 km, DEM 801×801 @ 4 m (747–1859 m), 660 OSM highways + 1,536 buildings fetched; overlay builds 247 roads (footpaths/tunnels skipped) + 1,430 lots; imported in 2 m 22 s (network-bound) | `geo-import` stdout |
| Edit session (scripted) | raise/flatten/smooth/road/save/undo-restore: hill centre +9.60 m in the *rendered* terrain, road surface found by `surface_at`; edit file 24.6 kB + 81 B roads | `WG_EDIT_SCRIPT=1` |
| Test suite | `cargo test --workspace --release`: all pass; `clippy -D warnings`, `fmt --check` clean; wasm32 `cargo check` of Layer A clean | |

## 5. Things that were not measured or verified

* `mate-h/bevy_terrain` (S-1): run after owner approval, see `s1-bevy-terrain.md` (24.5 ms p50 in its own example; not like-for-like). The in-house renderer is **not** shown to be faster or slower; it was kept for integration reasons.
* Any discrete GPU; VRAM; GPU time per feature on Medium/Low.
* Browser/WebGPU build and a Web-Worker job backend (only native-vs-wasm32 determinism of Layer A was checked, with node).
* A human using the editor with a mouse (picking, brush feel); only the scripted session and unit tests ran.
* The unattributed CPU cost of the Lab baseline (R-1) and the Lab's `main` runtime (host build blocker).
* Visual quality judgements are mine from screenshots (no user review); no pop-in/morph-artifact measurement at 1080p (S-1's "≤ 1 px popping" criterion was not measured).
* G5 and G6 as separately defined.
* Real-region correctness beyond one region: no check of OSM road/DEM alignment against ground truth; Terrarium tile alignment is unit-tested against an independent computation only.

## 6. Known functional gaps

Tunnels (heightfield terrain), water flow direction and refraction, full flow-accumulation hydrology, GeoTIFF/PBF/land-cover import,
a port of the Lab's egui editor, per-building interiors/impostors, roadside props beyond lamps and delineators, `THIRD-PARTY-NOTICES.md`.
See `docs/architecture/ADR-002-implementation-deviations.md`.
