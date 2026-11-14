//! Procedural tileable terrain texture array (8 layers, RGBA8 sRGB, alpha = height) with a CPU
//! generated mip chain. Layer order matches `worldgen_terrain::layer`.

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

const SIZE: usize = 256;

fn hash(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x27d4_eb2d)
        ^ (y as u32).wrapping_mul(0x1656_67b1)
        ^ seed.wrapping_mul(0x9e37_79b9);
    h ^= h >> 15;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2_ae35);
    h ^= h >> 16;
    (h >> 8) as f32 / (1u32 << 24) as f32
}

/// Tileable value noise with integer `period` cells across the texture.
fn tnoise(u: f32, v: f32, period: i32, seed: u32) -> f32 {
    let (x, y) = (u * period as f32, v * period as f32);
    let (xi, yi) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (x - xi as f32, y - yi as f32);
    let (sx, sy) = (fx * fx * (3.0 - 2.0 * fx), fy * fy * (3.0 - 2.0 * fy));
    let w = |a: i32, b: i32| hash(a.rem_euclid(period), b.rem_euclid(period), seed);
    let (a, b, c, d) = (w(xi, yi), w(xi + 1, yi), w(xi, yi + 1), w(xi + 1, yi + 1));
    (a + (b - a) * sx) * (1.0 - sy) + (c + (d - c) * sx) * sy
}

fn fbm(u: f32, v: f32, base: i32, oct: u32, seed: u32) -> f32 {
    let (mut s, mut a, mut n, mut p) = (0.0, 0.5, 0.0, base);
    for o in 0..oct {
        s += a * tnoise(u, v, p, seed.wrapping_add(o * 17));
        n += a;
        a *= 0.5;
        p *= 2;
    }
    s / n
}

fn srgb(c: [f32; 3]) -> [f32; 3] {
    c
}

/// (colour, per-layer generator) for layer `l` at texel `(u, v)` in `[0,1)`; returns (rgb 0..1, height 0..1).
fn layer_texel(l: usize, u: f32, v: f32) -> ([f32; 3], f32) {
    let seed = 100 + l as u32 * 31;
    let n1 = fbm(u, v, 8, 4, seed);
    let n2 = fbm(u, v, 32, 3, seed + 7);
    let fine = fbm(u, v, 64, 2, seed + 13);
    let (base, var, h): ([f32; 3], f32, f32) = match l {
        0 => ([0.26, 0.40, 0.12], 0.55, 0.55 * n2 + 0.45 * fine), // grass
        1 => ([0.17, 0.13, 0.07], 0.7, 0.6 * n1 + 0.4 * fine),    // forest floor
        2 => ([0.52, 0.46, 0.25], 0.5, 0.5 * n2 + 0.5 * fine),    // steppe
        3 => (
            [0.74, 0.67, 0.47],
            0.25,
            (n1 * 12.0).fract() * 0.2 + 0.3 * fine,
        ), // sand ripples
        4 => {
            // rock: strata + cracks
            let strata = ((v * 14.0 + n1 * 3.0).sin() * 0.5 + 0.5) * 0.35;
            let crack = (1.0 - ((n2 - 0.5).abs() * 6.0).min(1.0)) * 0.5;
            (
                [0.40, 0.38, 0.35],
                0.6,
                (0.35 * n1 + strata + crack * 0.5).min(1.0),
            )
        }
        5 => ([0.93, 0.95, 0.98], 0.08, 0.5 * n2 + 0.2 * fine), // snow
        6 => ([0.20, 0.14, 0.09], 0.5, 0.7 * n2 + 0.3 * fine),  // mud
        _ => ([0.47, 0.45, 0.41], 0.7, 0.5 * fine + 0.5 * n2),  // gravel
    };
    let k = 1.0 + (n1 - 0.5) * var * 0.45 + (n2 - 0.5) * var * 0.5 + (fine - 0.5) * var * 0.6;
    let c = srgb([
        base[0] * k,
        base[1] * k,
        base[2] * k * (1.0 + (n2 - 0.5) * var * 0.3),
    ]);
    (c, h.clamp(0.0, 1.0))
}

/// Build the 8-layer albedo(+height) array with mips.
pub fn build_terrain_array() -> Image {
    let mips = (SIZE as f32).log2() as usize + 1;
    let mut data: Vec<u8> = Vec::with_capacity(8 * SIZE * SIZE * 4 * 4 / 3);
    for l in 0..8 {
        let mut level: Vec<[f32; 4]> = Vec::with_capacity(SIZE * SIZE);
        for y in 0..SIZE {
            for x in 0..SIZE {
                let (c, h) = layer_texel(l, x as f32 / SIZE as f32, y as f32 / SIZE as f32);
                level.push([c[0], c[1], c[2], h]);
            }
        }
        let mut size = SIZE;
        for m in 0..mips {
            for t in &level {
                for ch in t {
                    data.push((ch.clamp(0.0, 1.0) * 255.0 + 0.5) as u8);
                }
            }
            if m + 1 < mips {
                let ns = size / 2;
                let mut next = Vec::with_capacity(ns * ns);
                for y in 0..ns {
                    for x in 0..ns {
                        let mut acc = [0.0f32; 4];
                        for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                            let t = level[(2 * y + dy) * size + 2 * x + dx];
                            for k in 0..4 {
                                acc[k] += t[k] * 0.25;
                            }
                        }
                        next.push(acc);
                    }
                }
                level = next;
                size = ns;
            }
        }
    }
    let mut img = Image::new(
        Extent3d {
            width: SIZE as u32,
            height: SIZE as u32,
            depth_or_array_layers: 8,
        },
        TextureDimension::D2,
        data,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    img.texture_descriptor.mip_level_count = mips as u32;
    img.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        anisotropy_clamp: 8,
        ..default()
    });
    img
}
