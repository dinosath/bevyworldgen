// 8-layer terrain splat shading: weights arrive as vertex attributes
//   color.rgba = layers 0..3 (grass, forest floor, steppe, sand)
//   uv         = layers 4,5   (rock, snow)
//   uv_b       = layers 6,7   (mud, gravel)
// Top-down tiling at two scales (near/far) with a distance blend, triplanar rock on steep faces,
// macro colour variation and a derivative-based bump from the layer height (alpha channel).
// Texture coordinates use absolute world position (render position + origin) so recentring the
// floating origin never shifts the pattern.

import bevy_pbr::render::{
    pbr_fragment::pbr_input_from_standard_material,
    pbr_functions::{apply_pbr_lighting, main_pass_post_lighting_processing},
    forward_io::{VertexOutput, FragmentOutput},
};
import bevy_pbr::render::mesh_view_bindings::view;

struct TerrainUniforms {
    // metres covered by one near tile, one far tile
    near_tile_m: f32,
    far_tile_m: f32,
    wetness: f32,
    bump: f32,
    origin: vec2<f32>,
    _pad: vec2<f32>,
}

@group(constants::MATERIAL_BIND_GROUP) @binding(100) var albedo_array: texture_2d_array<f32>;
@group(constants::MATERIAL_BIND_GROUP) @binding(101) var albedo_sampler: sampler;
@group(constants::MATERIAL_BIND_GROUP) @binding(102) var<uniform> tu: TerrainUniforms;

fn hash21(p: vec2<f32>) -> f32 {
    var q = fract(p * vec2<f32>(0.1031, 0.1030));
    q = q + dot(q, q.yx + 33.33);
    return fract((q.x + q.y) * q.x);
}

fn vnoise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash21(i);
    let b = hash21(i + vec2<f32>(1.0, 0.0));
    let c = hash21(i + vec2<f32>(0.0, 1.0));
    let d = hash21(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// Gradient sampling lets us skip layers whose weight is ~0 without breaking derivative
// uniformity (only layers that actually contribute to the pixel are fetched).
fn layer_tex(layer: i32, p: vec2<f32>, far_mix: f32, gn: vec4<f32>, gf: vec4<f32>) -> vec4<f32> {
    var near_t = vec4<f32>(0.0);
    var far_t = vec4<f32>(0.0);
    if far_mix < 0.999 {
        near_t = textureSampleGrad(albedo_array, albedo_sampler, p / tu.near_tile_m, layer, gn.xy, gn.zw);
    }
    if far_mix > 0.001 {
        far_t = textureSampleGrad(albedo_array, albedo_sampler, p / tu.far_tile_m, layer, gf.xy, gf.zw);
    }
    if far_mix < 0.001 {
        return near_t;
    }
    if far_mix > 0.999 {
        return far_t;
    }
    return mix(near_t, mix(far_t, near_t * far_t * 2.0, 0.5), far_mix);
}

fn layer_tri(layer: i32, wp: vec3<f32>, n: vec3<f32>) -> vec4<f32> {
    var w = pow(abs(n), vec3<f32>(4.0));
    w = w / (w.x + w.y + w.z + 0.0001);
    let s = 1.0 / tu.near_tile_m;
    let cx = textureSampleLevel(albedo_array, albedo_sampler, wp.zy * s, layer, 0.0);
    let cy = textureSampleLevel(albedo_array, albedo_sampler, wp.xz * s, layer, 0.0);
    let cz = textureSampleLevel(albedo_array, albedo_sampler, wp.xy * s, layer, 0.0);
    return cx * w.x + cy * w.y + cz * w.z;
}

@fragment
fn fragment(in: VertexOutput, @builtin(front_facing) is_front: bool) -> FragmentOutput {
    var pbr_input = pbr_input_from_standard_material(in, is_front);

    let wpos = in.world_position.xyz;
    let wp = vec3<f32>(wpos.x + tu.origin.x, wpos.y, wpos.z + tu.origin.y);
    let dist = distance(view.world_position.xyz, wpos);
    let far_mix = smoothstep(40.0, 260.0, dist);
    let n = normalize(in.world_normal);

    var w = array<f32, 8>(in.color.x, in.color.y, in.color.z, in.color.w, in.uv.x, in.uv.y, in.uv_b.x, in.uv_b.y);

    // texture-space derivatives computed once, in uniform control flow
    let uvn = wp.xz / tu.near_tile_m;
    let uvf = wp.xz / tu.far_tile_m;
    let gn = vec4<f32>(dpdx(uvn), dpdy(uvn));
    let gf = vec4<f32>(dpdx(uvf), dpdy(uvf));

    var rgb = vec3<f32>(0.0);
    var hsum = 0.0;
    for (var l = 0; l < 8; l = l + 1) {
        let wl = w[l];
        if wl > 0.01 {
            var t = layer_tex(l, wp.xz, far_mix, gn, gf);
            if l == 4 && n.y < 0.93 {
                t = mix(layer_tri(l, wp, n), t, smoothstep(0.8, 0.93, n.y));
            }
            rgb = rgb + t.rgb * wl;
            hsum = hsum + t.a * wl;
        }
    }

    // macro variation
    let m1 = vnoise(wp.xz / 53.0);
    let m2 = vnoise(wp.xz / 9.0);
    rgb = rgb * (0.80 + 0.40 * m1) * (0.92 + 0.16 * m2);

    // wetness darkens albedo and lowers roughness
    rgb = rgb * (1.0 - 0.45 * tu.wetness);
    pbr_input.material.base_color = vec4<f32>(rgb, 1.0);
    let rough = clamp(0.95 - 0.22 * w[5] - 0.25 * w[6] + 0.0 * w[4], 0.3, 1.0);
    pbr_input.material.perceptual_roughness = mix(rough, 0.28, tu.wetness);

    // bump from layer height (Mikkelsen's surface-gradient method with screen derivatives)
    let dpx = dpdx(wpos);
    let dpy = dpdy(wpos);
    let dhx = dpdx(hsum);
    let dhy = dpdy(hsum);
    let r1 = cross(dpy, n);
    let r2 = cross(n, dpx);
    let det = dot(dpx, r1);
    let grad = sign(det) * (dhx * r1 + dhy * r2);
    let bump = tu.bump * (1.0 - far_mix * 0.8);
    pbr_input.N = normalize(abs(det) * n - bump * grad);
    pbr_input.world_normal = n;

    var out: FragmentOutput;
    out.color = apply_pbr_lighting(pbr_input);
    out.color = main_pass_post_lighting_processing(pbr_input, out.color);
    return out;
}
