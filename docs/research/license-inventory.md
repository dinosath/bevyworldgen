# License inventory

Phase 1 deliverable 5. Licensing is a **pass/fail adoption gate**. This is an engineering inventory, not legal
advice; items marked ⚠ need a human/legal decision before adoption. "Verified" means I read the license file
and/or manifest in the cloned repository or crate metadata; assets and datasets are verified only as far as stated.

## 1. Project license policy (proposal)

* New code in this project: **`MIT OR Apache-2.0`** (matches Bevy and most of the shortlisted ecosystem).
* Third-party source keeps its own license and notices. MIT/BSD/Apache-2.0 code may be linked into or vendored in
  the dual-licensed project provided notices are preserved (`THIRD-PARTY-NOTICES.md`, per-file headers when files
  are copied). We do **not** relicense third-party code.
* Apache-2.0 sources (Symbios Overlands) may be vendored; keep the license text and any `NOTICE` (Overlands has no
  `NOTICE` file in its tree) and mark modified files.
* **MPL-2.0** (file-level copyleft) is acceptable only as an *unmodified dependency* (or with modified files kept
  under MPL-2.0 and published).
* **GPL/AGPL/LGPL-only, missing, or ambiguous licenses**: study only; no code incorporated.
* Code license ≠ asset license ≠ dataset license; each is tracked separately below.

## 2. Candidate repositories and crates

| Project | Declared (SPDX) | Files verified | Copyright holder(s) in files | Gate | Notes |
|---|---|---|---|---|---|
| `symbios-ground`, `-tensor`, `-shape`, `-texture` | MIT | `LICENSE` in each repo; `Cargo.toml license = "MIT"` | © 2026 TheJanusStream | **PASS** | single-license MIT |
| `bevy_symbios_ground/_shape/_texture`, `symbios`, `bevy_symbios` | MIT | same | © 2026 TheJanusStream | **PASS** | |
| `symbios-ground-lab` (upstream + `dinosath` fork) | MIT | `LICENSE` | © 2026 TheJanusStream | **PASS** | fork preserves upstream notice; local branch adds third-party CC0 assets (§4) |
| `symbios-overlands` | Apache-2.0 | `LICENSE` (Apache 2.0 text), `Cargo.toml` | none stated in `LICENSE` body | **PASS** | no `NOTICE` file; `deny.toml` present; contains non-code assets (§4) |
| `mate-h/bevy_terrain` | MIT OR Apache-2.0 | `LICENSE-MIT`, `LICENSE-APACHE` | © 2023 Kurt Kühnert; © 2023 Argeo (kept in files) | **PASS** | README: thesis PDF "when present" is CC BY 4.0; example datasets have separate terms (§5) |
| `kurtkuehnert/bevy_terrain` | MIT OR Apache-2.0 | same | same | **PASS** | |
| `bevy_grass` | MIT | `LICENSE` | © 2026 Gediminas Mikelenas | **PASS** | |
| `bevy_erosion_filter` | **MPL-2.0** (crate metadata) ; repo ships both `LICENSE-MIT` and `LICENSE-MPL-2.0` | both files + `THIRD-PARTY-NOTICES.md` | — | **PASS with condition** ⚠ | Port of Rune Skovbo Johansen's erosion filter whose released code is MPL-2.0 (per that author's blog, as quoted in the crate's notices). GitHub's license widget shows MIT, which is **misleading**; treat as MPL-2.0. Also includes Inigo Quilez noise code "under MIT-compatible" terms. Use unmodified. |
| `big_space` | MIT OR Apache-2.0 | `LICENSE-MIT`, `LICENSE-APACHE` | — | **PASS** | |
| `bevy-clipmap` | MIT | `LICENSE` | © 2025 Kirill Surkov | **PASS** | implements Hoppe's published algorithm |
| `Nerthus` | MIT | `LICENSE` | © 2026 Dane Madsen | **PASS (code)** ⚠ | Describes itself as a *clean-room reimplementation of Gaea 2.3.1* and reads/writes Gaea `.terrain` files. Code license is MIT; whether the reimplementation or its file-format work raises third-party IP concerns is **not assessed**. Obtain a review before importing any algorithm/format code. |
| `bevy_arc_studio` | MIT OR Apache-2.0 | `LICENSE-MIT`, `LICENSE-APACHE` | — | **PASS** | |
| `jackdaw` (+ `jackdaw_editor_terrain`) | MIT OR Apache-2.0 | both files | © 2025 jbuehler23 | **PASS** | |
| `bevy_water` | `MIT OR Apache-2.0` in `Cargo.toml` only | **no license files in repo** | author in `Cargo.toml` | **CONDITIONAL** ⚠ | License is declared in crate metadata but there is no license text/copyright notice in the repository. Prefer another water solution or ask upstream to add the files. |
| `bevy_feronia`, `bevy_eidolon`, `bevy_flora`, `feathrise_vegetation`, `bevy_map_scatter`, `bevy-aqua*`, `bevy_elements`, `bevy_weather`, `bevytiles`, `bevy_3d_tiles` | MIT OR Apache-2.0 (crates.io metadata) | metadata only | — | **PASS (unverified files)** | Re-verify license files when/if adopted |
| `avian3d` / `bevy_rapier3d` | MIT OR Apache-2.0 / Apache-2.0 | metadata only | — | **PASS** | |
| `feldspar` | MIT | `LICENSE` | © 2021 Duncan | PASS but **rejected** (engine/age) | |
| `rust-procedural-world` | MIT | `LICENSE` | © 2025 Bones | PASS but **rejected** (2D demo) | |
| `bevy-open-world` (evroon) | **GPL-3.0** | metadata only | — | **FAIL** | No code may be incorporated; architectural inspiration only |
| `mate-h/bevy_erosion` | **none** (no license file; no SPDX) | repo listing | — | **FAIL** | Default copyright applies; no rights granted |
| GIS crates `geo`, `geozero`, `rstar`, `spade`, `proj4rs`, `proj`, `osmpbf` | MIT OR Apache-2.0 | metadata | — | PASS | `proj` binds the C library PROJ (MIT, system dependency); `gdal` (MIT) binds GDAL (MIT/X) |
| `tiff`, `gdal`, `geotiff`, `libm`, `fastnoise-lite` | MIT | metadata | — | PASS | |

## 3. Dependency licenses of the Ground Lab Bevy-0.20 branch (computed from `cargo metadata`, 629 packages)

All 629 transitive dependencies have an SPDX `license` expression (none missing). Distribution (rows sum to 629):

| SPDX expression (grouped) | Packages |
|---|--:|
| `MIT OR Apache-2.0` (incl. `Apache-2.0 OR MIT`, `MIT/Apache-2.0`, `Apache-2.0/MIT`, `Apache-2.0 / MIT`) | 398 |
| `MIT` | 124 |
| `Apache-2.0` (alone, or `Apache-2.0 AND MIT`) | 12 |
| `Unicode-3.0` | 22 |
| `Zlib` variants (alone or in OR-expressions with MIT/Apache) | 27 |
| BSD-2/3-Clause (alone or dual) | 16 |
| `Unlicense OR MIT` / `Unlicense/MIT` | 7 |
| `Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT` | 7 |
| `MIT-0` (3), `BSL-1.0` (2), `ISC` (1), `0BSD OR MIT OR Apache-2.0` (1), `CC0-1.0 OR …` (3) | 10 |
| Compound with fonts: `(MIT OR Apache-2.0) AND OFL-1.1 AND Ubuntu-font-1.0` (Bevy's default font crate) | 1 |
| Compound: `(MIT OR Apache-2.0) AND NCSA`, `… AND Unicode-3.0` | 2 |
| Dual with copyleft alternative: `MIT OR Apache-2.0 OR LGPL-2.1-or-later` (×2), `Apache-2.0 OR GPL-2.0-only` (×1) | 3 |

* **No package is copyleft-only.** The three expressions offering a GPL/LGPL alternative are satisfied by choosing the
  MIT/Apache option. No MPL/EPL/CDDL packages in this graph.
* Obligations to carry: Unicode-3.0 data license notice, OFL-1.1/Ubuntu font notices (Bevy default font; disable
  `default_font` if shipping without these), BSD notices, NCSA notice.
* **Not yet done:** a `cargo-deny` policy in this project's CI (Overlands has a reference `deny.toml`) and a generated
  `THIRD-PARTY-NOTICES.md`. Roadmap M0.

## 4. Assets

| Asset set | License claimed | Verification | Gate |
|---|---|---|---|
| Ground Lab `assets/` (81 files: KTX2 PBR textures, glTF building kits and props, 71 MB) | **CC0-1.0**, source Poly Haven (authors named per file, e.g. Rob Tuytel), "modifications" recorded | `assets/ASSETS.toml` lists all 81 files, 0 unlisted, 0 missing; **claims taken from the provenance file, not re-checked against polyhaven.com** | **PASS (CC0, unverified at source)** — CC0 requires no attribution, but keep `ASSETS.toml` |
| Overlands `assets/fonts` (Noto Sans, Noto Sans CJK SC) | SIL OFL 1.1 | `assets/fonts/README.md` | PASS; embed license text if redistributed |
| Overlands `assets/shaders/*.wesl` | Apache-2.0 (repo license) | repo `LICENSE` | PASS if vendored (keep notice) |
| `mate-h/bevy_terrain` `assets/shaders`, `assets/textures` | repo license (MIT OR Apache-2.0) | not itemised | PASS (re-check textures if used) |

## 5. Datasets (Phase 9 ingestion policy — L-5)

Implemented by `apps/geo-import` (fetches at the user's request) and `crates/worldgen-geo` (reads). **No third-party data is
stored in this repository**: imported regions live in `data/` (git-ignored) and each import writes an `ATTRIBUTION.txt`.

| Dataset | Terms (from the providers' published statements; **not legal advice, not independently audited**) | Policy implemented |
|---|---|---|
| AWS Open Data "Terrain Tiles" (Terrarium PNG, `elevation-tiles-prod`) — composite of SRTM, GMTED, NED, national datasets | Attribution required; per-source terms listed at `github.com/tilezen/joerd/blob/master/docs/attribution.md`. Some constituent sources (e.g. 3DEP, national DEMs) carry their own conditions | Importer writes the attribution text into the region directory; derived `region.wgdem` stays local; **do not redistribute region directories without checking the constituent sources for the region** |
| OpenStreetMap roads/buildings via the public Overpass API | ODbL 1.0: attribution "© OpenStreetMap contributors"; share-alike applies to a *derived database* if publicly shared; Overpass public servers have a fair-use policy | Importer uses a generic User-Agent, sequential quadrant requests with back-off, caches nothing outside the region directory. `osm.json` is a database extract: not committed, not redistributed. **Rendering a derived world is a "produced work"; sharing the extract itself is what triggers ODbL** — owner should confirm before publishing any region data |
| GDI Berlin / Overlands layers, `bevy_terrain` demo datasets, SRTM / Copernicus / USGS 3DEP / ESA WorldCover as separate downloads | as listed in earlier revision | **Not ingested.** No importer exists for them; land-cover rasters (planned `tiff`/`osmpbf` adapters) were **not implemented** |

## 6. Code imported or vendored

**None vendored.** All third-party code is consumed as crates.io dependencies (545 resolved third-party packages;
every one satisfies the `deny.toml` allow-list — checked by a script over `cargo metadata`, because `cargo-deny` is not
installed on the build host; CI runs the real tool).
Direct runtime dependencies of note: Symbios crates `=0.4.2/0.7.0/0.5.0/0.8.1` (MIT), `bevy 0.20.1` (MIT/Apache-2.0),
`bevy_grass 0.20.6` (MIT), `proj4rs 0.3.0` (MIT/Apache-2.0), `png 0.18` (MIT/Apache-2.0).
Algorithms written here, with no code taken from other projects: terrain sampler, droplet erosion, drainage hydrology,
highway planner, town layout glue, vegetation placement, streaming/job layers. The Lab's default CGA grammar text
(MIT, © TheJanusStream) is embedded in `bench/phase1-algos` only.
**Not adopted** (contrary to the ADR's plan; see `docs/architecture/ADR-002-implementation-deviations.md`): `bevy_erosion_filter`
(so L-3, MPL-2.0 handling, is moot for now — MPL-2.0 remains allowed in `deny.toml`), `big_space`, `mate-h/bevy_terrain`.
Assets: the showcase ships **no** image/model assets — all textures (8-layer terrain array, road, glass) and meshes are
generated procedurally at start-up, so L-6 (Poly Haven re-verification) does not apply to this build.

## 7. Open licensing actions

| ID | Status |
|---|---|
| L-1 | **Done**: `LICENSE-MIT`, `LICENSE-APACHE`, `deny.toml`, CI job. `THIRD-PARTY-NOTICES.md` **not generated** (needs `cargo-about`/`cargo-deny`; not installed) |
| L-2 | N/A so far: no Nerthus-derived code or format used |
| L-3 | Moot (`bevy_erosion_filter` not used); revisit if adopted |
| L-4 | Moot: water is a custom shader (`bevy_water` not used) |
| L-5 | **Done** (policy above; owner must still confirm ODbL stance before publishing any extract) |
| L-6 | N/A (no Poly Haven assets shipped by the new workspace) |
