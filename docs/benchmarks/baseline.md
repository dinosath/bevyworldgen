# Baseline benchmark — Symbios Ground Lab and its generation crates

Phase 1 deliverable 6. All numbers are **measured** on 2026-11-13/14 unless marked otherwise. Raw output:
`docs/benchmarks/raw/`. Reproduction commands are at the bottom.

## 1. Environment

| Item | Value |
|---|---|
| CPU | AMD Ryzen 7 5700G (8 cores / 16 threads) |
| GPU | AMD Radeon Graphics (RADV RENOIR, **integrated Vega**), Vulkan 1.4, Mesa 26.2.6 |
| RAM | 58 GiB |
| OS | Fedora Atomic-derived, Linux 7.2.15, Wayland session |
| Rust | rustc / cargo 1.99.1 |
| Bevy | 0.20.1 (lab branch); upstream `main` would use 0.18.1 (could not build, §6) |
| Load during runs | loadavg ≈ 0.7–0.9 at start of headless runs |
| Not measured | VRAM (no tooling available), cross-target determinism, browser/WASM, discrete-GPU numbers |

**Caveat that matters:** this GPU is an iGPU. GPU-bound results here are *pessimistic* for a discrete card;
CPU-side results (generation, main-thread stalls) are hardware-independent in shape.

## 2. What was measured and how

1. **Headless algorithm benchmark** (`bench/phase1-algos`): the exact crate versions the Lab branch locks
   (`symbios-ground =0.4.2`, `symbios-tensor =0.7.0`, `symbios-shape =0.5.0`), the Lab's default
   `TerrainConfig` / `UrbanConfig` / default CGA grammar (41 rules), release profile
   (`opt-level 3`, thin LTO). One scenario per process so peak RSS is attributable.
2. **Instrumented Lab application**, run for real on the iGPU. Instrumentation lives only in a scratch
   copy (patches archived as `raw/lab-bench-patch-*`); the user's branch was not modified. Added:
   `FrameTimeDiagnosticsPlugin`, `RenderDiagnosticsPlugin` (GPU timestamps), per-2-s frame-time
   mean/p50/p99/max, entity count, `VmRSS/VmHWM`, generation-latency probe, timers around mesh build and
   tangent generation, `PresentMode::AutoNoVsync`, env overrides `BENCH_GRID` / `BENCH_DROPS`.
   Urban layer enabled (smoke mode, `VEIL_LAB_SMOKE`). Window 1280×720.

### Deviations from the repo's own build

* Repo release profile is `opt-level="s"`, fat LTO, `panic="abort"` (size-oriented, for wasm). I built
  `opt-level=3`, no LTO, 16 codegen units, `panic="unwind"`, no strip, to measure speed, not size.
  **Absolute CPU numbers for the app are therefore not those of the shipped profile.**
* Bevy features: audio and gamepad **disabled** (`default-features = false` with an explicit list) because
  the host and the `gpuikit-build` distrobox lack `alsa`/`libudev` development packages. Neither feature
  is on any generation/terrain/city path. I did not install packages on your system.

## 3. Headless algorithm results (CPU, single process each)

### 3.1 Base generators (release; FBM = 6 octaves, base freq 4)

| Grid | FBM | Diamond-Square | Voronoi (64 seeds, 8 terraces) | Peak RSS |
|---|--:|--:|--:|--:|
| 257² | 3.7 ms | 0.70 ms | 2.7 ms | 3.7 MB |
| 513² | 14.1 ms | 2.7 ms | 10.9 ms | 5.9 MB |
| 1025² | 55.5 ms | 11.0 ms | 42.8 ms | 15.1 MB |
| 2049² | 219 ms | 44.2 ms | 172.8 ms | 50.9 MB |

Linear in cell count (≈52 ns/cell for FBM).

### 3.2 Erosion

| Scenario | Hydraulic | Thermal | Notes |
|---|--:|--:|---|
| 513², 50 k drops (Lab default) ×3 | 136.1 / 133.7 / 135.3 ms | 5.4–5.5 ms (1 iter) | run-to-run spread 1.8 % |
| 513², 100 thermal iterations | 134.4 ms | **286.8 ms** | thermal is O(iters × cells) |
| 1025², 50 k drops | 148.9 ms | 18.2 ms | same drop count as 513² ⇒ only +10 % |
| 1025², 200 k drops | 585.2 ms | 21.5 ms | |
| 2049², 50 k drops | 172.8 ms | 57.4 ms | |
| 2049², 800 k drops (same drops/cell as 513² default) | **2,762 ms** | 76.7 ms | ≈290 drops/ms |

Hydraulic erosion cost is **∝ droplets, not cells** (≈290–370 drops/ms single-threaded). Holding erosion
*density* constant (0.19 drops/cell, the Lab default) therefore makes erosion cost grow linearly with area:
136 ms @513² → 585 ms @1025² → 2.76 s @2049². With a fixed drop budget the map is simply under-eroded.

Parallel chunk model (independent 513² maps, 50 k drops each, rayon on 16 threads):

| Jobs | Wall time | Throughput |
|--:|--:|--:|
| 1 | 155 ms | 6.5 jobs/s |
| 16 | 208.7 ms | 76.7 jobs/s (**11.9×**) |
| 32 | 434 ms | 73.7 jobs/s |

Chunked erosion parallelises well (8 physical cores). It is *not* yet border-correct (see §3.5).

### 3.3 Road/city pipeline (Lab defaults, carve + meshes included)

| Grid (1 m/cell) | generate_roads | rationalize | blocks | lots | prune | carve (roads+lots) | road meshes | **Total** | nodes / edges / blocks / lots | Peak RSS |
|---|--:|--:|--:|--:|--:|--:|--:|--:|---|--:|
| 513² | 7.0 ms | 64 ms (63–80 over 3 runs) | 1.2 | 1.1 | 7.0 | 4.9 | 0.2 | **≈ 86 ms** | 8,925 / 10,123 / 298 / 40 | 8.6 MB |
| 1025² | 37 ms | **1,794 ms** | 7.1 | 5.6 | 221 | 23.7 | 2.0 | **≈ 2.1 s** | 40,840 / 46,894 / 1,990 / 221 | 27.5 MB |
| 2049² | 161 ms | **37,856 ms** | 42.7 | 24.4 | **6,259** | 113 | 11.5 | **≈ 44.5 s** | 181,359 / 209,121 / 11,943 / 953 | 102.8 MB |

* `rationalize_graph` grows ×28 for ×4.6 nodes, then ×21 for ×4.4 nodes ⇒ **≈ O(n²)** in node count.
* Lots stay sparse at the Lab's lot config (40 lots on 512 m, 953 on 2 km): the default produces a *village*, not a city.
* Both larger runs logged `tensor field blending toward axis-aligned fallback on near-flat terrain`.

### 3.4 CGA building derivation (default 41-rule house grammar, per-building seed)

| Buildings | Total | Mean | p50 | p99 | Terminals | Peak RSS |
|--:|--:|--:|--:|--:|--:|--:|
| 1,000 | 106.6 ms | 0.098 ms | 0.092 | 0.259 | 381,685 | 4.0 MB |
| 10,000 | 1,014.6 ms | 0.094 ms | 0.093 | 0.168 | 3,852,020 | 4.0 MB |

≈385 terminals per building. Derivation is cheap (≈10 k buildings/s/thread). In the app, each terminal becomes
an ECS entity — see §4.

### 3.5 Tile streaming primitive (`TiledHeightMap`, FBM)

| Tile size | 25 tiles | Seam mean Δ | Interior adjacent-cell mean Δ | Seam / interior | Seam max Δ | Order-independent |
|--:|--:|--:|--:|--:|--:|:-:|
| 128² | 20.9 ms | 0.1769 | 0.0030 | **58.9×** | 0.313 | yes |
| 256² | 83.9 ms | 0.1766 | 0.0015 | **117.2×** | 0.313 | yes |

(Heights are raw FBM units, range ≈ 0–1.) Confirms the crate's own documentation: independently generated
tiles do not meet. Tile generation order does not change content.

### 3.6 Determinism (same machine)

FBM repeat = equal; FBM from 8 concurrent rayon workers = equal; hydraulic erosion repeat = equal (bitwise FNV
fingerprint of `f32` bits). **Not tested:** other CPUs/OSes, wasm32.

## 4. Instrumented Lab application (real GPU)

| Scenario | Generation latency | Mesh build | **`generate_tangents` (main thread)** | Worst frame | Steady-state frame | GPU main-opaque / transparent | Entities | Peak RSS |
|---|--:|--:|--:|--:|--:|--:|--:|--:|
| 512², 50 k drops, urban on — run 1 | 334 ms | n/a | n/a | startup 1,560 ms; publish 225 ms | mean 10.79, p50 10.5, p99 13.3, max 13.4 ms (**≈93 fps**) | 8.73 / 0.40 ms | 2,831 | 823 MB |
| 512² — run 2 | 303 ms | n/a | n/a | startup 1,535 ms; publish 228 ms | mean 10.79, p99 13.4 ms | 8.71 / 0.40 ms | 2,831 | 818 MB |
| 1025², 200 k drops | 3,596 / 3,998 ms (2 runs) | 49 ms (1,050,625 verts) | **5,642 ms** | **5,937 / 5,968 ms** (single frame) | mean 14.1, p99 16.6–16.9 ms | 10.6 / 1.2 ms | 5,694 | 1,073–1,221 MB |
| 2049², 800 k drops | **82,063 ms** | 206 ms (4,198,401 verts) | **27,679 ms** | **29,073 ms** (single frame) | mean ≈ 59–62 ms, p99 ≈ 80 ms | 16.8 / 4.9 ms | 6,527 | 1.6 GB steady, **3.38 GB peak** |

Findings:

1. **Steady state at the default scale is GPU-bound** (8.7 of 10.8 ms in one pass) on this iGPU with only
   2.8 k entities and a ~0.5 M-triangle terrain. No LOD, no occlusion.
2. **The main-thread freeze is `generate_tangents()`**, not meshing: ≈95 % of the freeze at both 1025²
   (5,642 of 5,937 ms) and 2049² (27,679 of 29,073 ms). Scales ≈ linearly with vertices (5.6 s per 1.05 M ⇒
   27.7 s per 4.2 M). Avoidable (compute in the worker, or derive TBN from the heightfield in the shader).
3. The remainder of the freeze (≈0.25 s at 1025², ≈1.4 s at 2049²) is unattributed (mesh build, buildings,
   roads and texture publish land in the same frame).
4. After the 2049² publish the app stays at ≈59 ms/frame. The GPU passes above account for ≈22 ms; the rest
   (≈37 ms) is **not attributed** (needs CPU trace / more GPU passes; open item, not asserted).
5. 3.38 GB peak RSS at 2049²: world-sized `HeightMap` + clone for the task + mesh CPU copy + tangent buffers.
   No memory budget exists.
6. Frame-time variability: p99/p50 ≈ 1.27 at 512² in steady state, but every publish produces one frame far
   outside it: 225 ms (≈22× the 10.5 ms median) at 512², 5.9 s at 1025², 29 s at 2049². The distribution is
   "smooth, then catastrophic".

Screenshot of the 512² smoke run (urban enabled): gizmo-line road graph (green), lot gizmos (orange) and
centroids (magenta), brown splat terrain, flat blue lake quads, brick-textured box buildings. Saved by the
smoke harness at `.local/city_lab.png` in the scratch copy (not archived; 1.0 MB).

## 5. Interpretation

* The **algorithms are fast enough at tile scale** (≤512² per tile): whole city tile ≈ 86 ms, whole terrain
  tile with default erosion ≈ 160 ms, 10 k building derivations ≈ 1 s. A streamed world is feasible *if* work
  is chunked into ≲512-cell tiles, run off the main thread, and the ≈quadratic rationalize/prune stages are
  never fed a large graph.
* **The application-level design, not the algorithms, is what breaks at scale.** One mesh, one global heightmap, blocking tangents,
  one entity per terminal.
* **Quality gaps to fix at integration level:** tile seams (58–117×), cross-tile roads, erosion borders.
* Targets for the next phase (benchmark gates, see roadmap): no frame > 2× median during streaming;
  all per-chunk CPU work off the main thread; main-thread work per chunk upload ≤ 1 ms.

## 6. Blockers and things not done

| Item | Status |
|---|---|
| Build of upstream `main` (Bevy 0.18.1 + symbios-ground 0.2.1 …) | **Failed on this host**: `wayland-sys 0.31.11` needs `libwayland-client` pkg-config; persisted with the x11-only feature set because the crate is pulled transitively. Log: `raw/build-upstream-main-bevy018-wayland-failure.log`. Dependency resolution itself succeeded. No runtime baseline for the 0.18 build; given that `main` is the same app on superseded libraries, the 0.20 branch is the relevant baseline. |
| First attempt to build the 0.20 branch | Failed (`alsa-sys` needs `alsa.pc`); succeeded after disabling audio/gamepad features (2 m 08 s, 629 dependencies, cold 16-thread build). Log: `raw/build-lab-first-attempt-alsa-failure.log` |
| `mate-h/bevy_terrain` runtime (frame time, LOD behaviour) | **Not measured.** I verified only that `cargo check --lib` passes against stable Bevy 0.20.1 after replacing two git dependency lines. Building and running its modified example was blocked by the tool permission layer and I did not work around it. Phase 2 spike S-1 covers it. |
| Per-system CPU timings | Not collected (no tracing build) |
| VRAM | Not collected |
| `cargo test` on the Lab branch | Not run (11 tests in the app; library crates' own suites were run: 45 / 76 / 335 pass) |
| Browser/WASM | Not run |

## 7. Reproduction

```bash
# Headless algorithms (one process per scenario)
cd bench/phase1-algos && cargo build --release
B=target/release/phase1-algos
$B generators 513        # 257 | 513 | 1025 | 2049
$B erosion 513 50000 1   # size drops thermal_iters
$B roads 513             # 513 | 1025 | 2049
$B buildings 10000
$B tiles 128             # tile edge in cells
$B determinism
$B parallel-erosion 513 50000 16

# Instrumented Lab (scratch copy with raw/lab-bench-patch-* applied; audio/gamepad features off)
VEIL_LAB_SMOKE=900   cargo run --release                                   # 512²
VEIL_LAB_SMOKE=2500  BENCH_GRID=1025 BENCH_DROPS=200000 cargo run --release # 1025²
VEIL_LAB_SMOKE=80000 BENCH_GRID=2049 BENCH_DROPS=800000 cargo run --release # 2049² (≈4 min)
```

Host-dependent numbers should be re-collected on the target hardware before being used as acceptance gates.
