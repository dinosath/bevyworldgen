| Candidate | Category | License gate | Func 25% | Arch 20% | Perf 20% | Bevy 15% | Maint 10% | Quality 10% | **Total /5** | Evidence confidence |
|---|---|---|--:|--:|--:|--:|--:|--:|--:|---|
| Symbios crates (symbios-ground/tensor/shape + bevy_symbios_*) | generation core | PASS | 3.0 | 4.5 | 2.5 | 5.0 | 2.5 | 4.0 | **3.55** | high (source read, tests run, benchmarked) |
| Symbios Ground Lab (app) as foundation | application | PASS | 2.5 | 1.5 | 1.0 | 4.0 | 1.5 | 1.5 | **2.02** | high (source read, app benchmarked) |
| Symbios Overlands (app) as foundation | application | PASS | 3.5 | 2.0 | 2.5 | 5.0 | 4.0 | 4.0 | **3.32** | medium (docs+structure read, not run) |
| Nerthus (Gaea-compatible terrain graph) | generation core | PASS* | 4.0 | 2.5 | 2.5 | 5.0 | 3.5 | 4.5 | **3.55** | low (surface inspection only) |
| bevy_arc_studio | application | PASS | 3.0 | 2.5 | 2.0 | 5.0 | 3.5 | 3.5 | **3.10** | low (surface inspection only) |
| mate-h/bevy_terrain | terrain renderer | PASS | 4.0 | 4.0 | 3.5 | 2.5 | 2.5 | 1.5 | **3.27** | medium (source read, compile-checked, not run) |
| kurtkuehnert/bevy_terrain (original) | terrain renderer | PASS | 3.5 | 4.0 | 3.5 | 0.5 | 0.5 | 1.0 | **2.60** | medium (README+tree; Bevy 0.14) |
| bevy-clipmap | terrain renderer | PASS | 2.0 | 3.0 | 3.0 | 3.0 | 2.0 | 0.5 | **2.40** | medium (source read; 651 LOC) |
| bevy_grass | vegetation | PASS | 3.0 | 4.0 | 4.0 | 5.0 | 3.5 | 4.5 | **3.90** | medium (README+CI read, not run) |
| bevy_erosion_filter | terrain detail | PASS (MPL-2.0) | 3.0 | 4.5 | 4.5 | 5.0 | 2.5 | 3.0 | **3.85** | medium (README+notices read) |
| big_space | large worlds | PASS | 4.0 | 4.0 | 4.0 | 3.0 | 4.0 | 4.0 | **3.85** | medium (manifest+README; rc for Bevy 0.20) |
| bevy_water | water | COND (no license files) | 2.5 | 3.0 | 2.5 | 5.0 | 3.0 | 1.0 | **2.88** | low |
| feldspar (voxel) | terrain (voxel) | PASS | 1.5 | 3.5 | 3.0 | 0.0 | 0.0 | 2.5 | **1.93** | medium |
| rust-procedural-world | 2D demo | PASS | 1.0 | 1.0 | 1.0 | 0.5 | 0.5 | 0.5 | **0.83** | high (814 LOC) |
| bevy-open-world | open-world crates | **FAIL (GPL-3.0)** | – | – | – | – | – | – | – | n/a - excluded by license gate |
