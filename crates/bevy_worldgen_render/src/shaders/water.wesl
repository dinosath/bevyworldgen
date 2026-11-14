// Water: depth-tinted translucent surface with animated wave normals, shoreline foam and
// fresnel handled by the PBR pipeline (low roughness). `uv.x` carries water depth in metres.

import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
};
import bevy_pbr::render::mesh_view_bindings::{globals, view};

struct WaterUniforms {
    shallow: vec4<f32>,
    deep: vec4<f32>,
    origin: vec2<f32>,
    wave_strength: f32,
    foam_depth: f32,
}
@group(constants::MATERIAL_BIND_GROUP) @binding(100) var<uniform> wu: WaterUniforms;

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

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);
    let t = globals.time;
    let wp = in.world_position.xz + wu.origin;
    let depth = max(in.uv.x, 0.0);

    // two drifting octaves of value-noise gradient as wave normals
    let far = smoothstep(60.0, 600.0, distance(view.world_position.xyz, in.world_position.xyz));
    let p1 = wp * 0.12 + vec2<f32>(t * 0.06, t * 0.04);
    let p2 = wp * 0.33 + vec2<f32>(-t * 0.09, t * 0.07);
    let e = 0.2;
    let h1 = vnoise(p1);
    let g1 = vec2<f32>(vnoise(p1 + vec2<f32>(e, 0.0)) - h1, vnoise(p1 + vec2<f32>(0.0, e)) - h1) / e;
    let h2 = vnoise(p2);
    let g2 = vec2<f32>(vnoise(p2 + vec2<f32>(e, 0.0)) - h2, vnoise(p2 + vec2<f32>(0.0, e)) - h2) / e;
    let calm = clamp(depth / 0.8, 0.15, 1.0); // shallow water ripples less
    let g = (g1 * 0.6 + g2 * 0.4) * wu.wave_strength * calm * (1.0 - 0.85 * far);
    pbr_input.N = normalize(vec3<f32>(-g.x, 1.0, -g.y));

    let d01 = clamp(depth / 7.0, 0.0, 1.0);
    var col = mix(wu.shallow.rgb, wu.deep.rgb, sqrt(d01));
    var alpha = mix(0.30, 0.94, clamp(depth / 2.5, 0.0, 1.0));

    // shoreline foam
    let foam_n = vnoise(wp * 1.7 + vec2<f32>(t * 0.4, -t * 0.3));
    let foam = (1.0 - smoothstep(0.0, wu.foam_depth, depth)) * smoothstep(0.25, 0.8, foam_n + 0.35 * (1.0 - depth / wu.foam_depth));
    col = mix(col, vec3<f32>(0.95), foam);
    alpha = max(alpha, foam * 0.9);

    pbr_input.material.base_color = vec4<f32>(col, alpha);
    pbr_input.material.perceptual_roughness = mix(0.06, 0.5, foam);

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
