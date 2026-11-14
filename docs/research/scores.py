#!/usr/bin/env python3
"""Weighted candidate scores for docs/research/feature-comparison.md.

Per-criterion scores are 0-5 *judgements* backed by the evidence cited in that document; this
script only does the arithmetic so the totals are reproducible.
Run: python3 -I docs/research/scores.py
"""
W = dict(functional=0.25, architecture=0.20, performance=0.20, bevy=0.15, maintenance=0.10, quality=0.10)
# name: (category, license_gate, functional, architecture, performance, bevy, maintenance, quality, confidence)
C = {
 "Symbios crates (symbios-ground/tensor/shape + bevy_symbios_*)": ("generation core", "PASS", 3.0, 4.5, 2.5, 5.0, 2.5, 4.0, "high (source read, tests run, benchmarked)"),
 "Symbios Ground Lab (app) as foundation":                       ("application",     "PASS", 2.5, 1.5, 1.0, 4.0, 1.5, 1.5, "high (source read, app benchmarked)"),
 "Symbios Overlands (app) as foundation":                        ("application",     "PASS", 3.5, 2.0, 2.5, 5.0, 4.0, 4.0, "medium (docs+structure read, not run)"),
 "Nerthus (Gaea-compatible terrain graph)":                      ("generation core", "PASS*", 4.0, 2.5, 2.5, 5.0, 3.5, 4.5, "low (surface inspection only)"),
 "bevy_arc_studio":                                              ("application",     "PASS", 3.0, 2.5, 2.0, 5.0, 3.5, 3.5, "low (surface inspection only)"),
 "mate-h/bevy_terrain":                                          ("terrain renderer","PASS", 4.0, 4.0, 3.5, 2.5, 2.5, 1.5, "medium (source read, compile-checked, not run)"),
 "kurtkuehnert/bevy_terrain (original)":                         ("terrain renderer","PASS", 3.5, 4.0, 3.5, 0.5, 0.5, 1.0, "medium (README+tree; Bevy 0.14)"),
 "bevy-clipmap":                                                 ("terrain renderer","PASS", 2.0, 3.0, 3.0, 3.0, 2.0, 0.5, "medium (source read; 651 LOC)"),
 "bevy_grass":                                                   ("vegetation",      "PASS", 3.0, 4.0, 4.0, 5.0, 3.5, 4.5, "medium (README+CI read, not run)"),
 "bevy_erosion_filter":                                          ("terrain detail",  "PASS (MPL-2.0)", 3.0, 4.5, 4.5, 5.0, 2.5, 3.0, "medium (README+notices read)"),
 "big_space":                                                    ("large worlds",    "PASS", 4.0, 4.0, 4.0, 3.0, 4.0, 4.0, "medium (manifest+README; rc for Bevy 0.20)"),
 "bevy_water":                                                   ("water",           "COND (no license files)", 2.5, 3.0, 2.5, 5.0, 3.0, 1.0, "low"),
 "feldspar (voxel)":                                             ("terrain (voxel)", "PASS", 1.5, 3.5, 3.0, 0.0, 0.0, 2.5, "medium"),
 "rust-procedural-world":                                        ("2D demo",         "PASS", 1.0, 1.0, 1.0, 0.5, 0.5, 0.5, "high (814 LOC)"),
 "bevy-open-world":                                              ("open-world crates","FAIL (GPL-3.0)", None, None, None, None, None, None, "n/a - excluded by license gate"),
}
print("| Candidate | Category | License gate | Func 25% | Arch 20% | Perf 20% | Bevy 15% | Maint 10% | Quality 10% | **Total /5** | Evidence confidence |")
print("|---|---|---|--:|--:|--:|--:|--:|--:|--:|---|")
for n,(cat,gate,f,a,p,b,m,q,conf) in C.items():
    if f is None:
        print(f"| {n} | {cat} | **{gate}** | – | – | – | – | – | – | – | {conf} |"); continue
    t = f*W['functional']+a*W['architecture']+p*W['performance']+b*W['bevy']+m*W['maintenance']+q*W['quality']
    print(f"| {n} | {cat} | {gate} | {f} | {a} | {p} | {b} | {m} | {q} | **{t:.2f}** | {conf} |")
