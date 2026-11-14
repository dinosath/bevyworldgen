// Procedural cloud layer on a large plane above the camera: fbm coverage, fake self-shadowing
// toward the sun, sun/sky tint, distance fade. Unlit, alpha-blended.

import bevy_pbr::render::forward_io::VertexOutput;
import bevy_pbr::render::mesh_view_bindings::{globals, view};

struct CloudUniforms {
    sun: vec4<f32>,   // xyz = direction toward the sun, w = elevation (sin)
    tint: vec4<f32>,  // rgb = light colour at the clouds
    origin: vec2<f32>,
    wind: vec2<f32>,
    coverage: f32,
    darkness: f32,
    _pad: vec2<f32>,
}
@group(constants::MATERIAL_BIND_GROUP) @binding(0) var<uniform> cu: CloudUniforms;

fn hash21(p: vec2<f32>) -> f32 {
    var q = fract(p * vec2<f32>(0.1031, 0.1030));
    q = q + dot(q, q.yx + 33.33);
    return fract((q.x + q.y) * q.x);
}
fn vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return mix(mix(hash21(i), hash21(i + vec2<f32>(1.0, 0.0)), u.x),
               mix(hash21(i + vec2<f32>(0.0, 1.0)), hash21(i + vec2<f32>(1.0, 1.0)), u.x), u.y);
}
fn fbm(p0: vec2<f32>) -> f32 {
    var p = p0;
    var a = 0.5;
    var s = 0.0;
    for (var i = 0; i < 5; i = i + 1) {
        s = s + a * vnoise(p);
        p = p * 2.03 + vec2<f32>(17.1, 9.2);
        a = a * 0.5;
    }
    return s;
}

@fragment
fn fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let dist = distance(view.world_position.xyz, in.world_position.xyz);
    let t = globals.time;
    let p = (in.world_position.xz + cu.origin) / 4200.0 + cu.wind * t * 0.00018;
    let f = fbm(p);
    let thresh = mix(0.80, 0.34, clamp(cu.coverage, 0.0, 1.0));
    let d = smoothstep(thresh, thresh + 0.20, f);
    // fake self-shadowing: compare density a step toward the sun
    let fs = fbm(p + cu.sun.xz * 0.05);
    let shade = clamp(1.0 - (fs - f) * 3.2, 0.3, 1.0);
    var col = cu.tint.rgb * mix(0.5, 1.05, shade);
    col = col * mix(1.0, cu.darkness, clamp(d * cu.coverage * 1.2, 0.0, 1.0));
    let fade = 1.0 - smoothstep(14000.0, 32000.0, dist);
    let a = d * fade * 0.92;
    return vec4<f32>(col, a);
}
