//! Deterministic gradient noise in world coordinates. Uses only IEEE-exact operations
//! (`floor`, arithmetic, `abs`), so results are identical on every target.

use worldgen_core::splitmix64;

const GRADS: [(f64, f64); 16] = [
    (1.0, 0.0),
    (0.9238795325112867, 0.3826834323650898),
    (
        std::f64::consts::FRAC_1_SQRT_2,
        std::f64::consts::FRAC_1_SQRT_2,
    ),
    (0.3826834323650898, 0.9238795325112867),
    (0.0, 1.0),
    (-0.3826834323650898, 0.9238795325112867),
    (
        -std::f64::consts::FRAC_1_SQRT_2,
        std::f64::consts::FRAC_1_SQRT_2,
    ),
    (-0.9238795325112867, 0.3826834323650898),
    (-1.0, 0.0),
    (-0.9238795325112867, -0.3826834323650898),
    (
        -std::f64::consts::FRAC_1_SQRT_2,
        -std::f64::consts::FRAC_1_SQRT_2,
    ),
    (-0.3826834323650898, -0.9238795325112867),
    (0.0, -1.0),
    (0.3826834323650898, -0.9238795325112867),
    (
        std::f64::consts::FRAC_1_SQRT_2,
        -std::f64::consts::FRAC_1_SQRT_2,
    ),
    (0.9238795325112867, -0.3826834323650898),
];

#[inline]
fn lattice(seed: u64, ix: i64, iz: i64) -> (f64, f64) {
    let h = splitmix64(
        seed ^ (ix as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
            ^ (iz as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F),
    );
    GRADS[(h >> 60) as usize]
}

#[inline]
fn fade(t: f64) -> f64 {
    t * t * t * (t * (t * 6.0 - 15.0) + 10.0)
}

/// 2-D gradient noise, approximately in `[-1, 1]`.
pub fn gradient_noise(seed: u64, x: f64, z: f64) -> f64 {
    let x0 = x.floor();
    let z0 = z.floor();
    let (ix, iz) = (x0 as i64, z0 as i64);
    let (fx, fz) = (x - x0, z - z0);
    let dot = |cx: i64, cz: i64, dx: f64, dz: f64| {
        let (gx, gz) = lattice(seed, cx, cz);
        gx * dx + gz * dz
    };
    let n00 = dot(ix, iz, fx, fz);
    let n10 = dot(ix + 1, iz, fx - 1.0, fz);
    let n01 = dot(ix, iz + 1, fx, fz - 1.0);
    let n11 = dot(ix + 1, iz + 1, fx - 1.0, fz - 1.0);
    let (u, v) = (fade(fx), fade(fz));
    let a = n00 + (n10 - n00) * u;
    let b = n01 + (n11 - n01) * u;
    (a + (b - a) * v) * core::f64::consts::SQRT_2
}

/// Amplitude factor that fades octaves out as their wavelength approaches the sample spacing,
/// avoiding aliasing on coarse LODs. `spacing = 0` keeps every octave.
#[inline]
fn octave_gain(wavelength: f64, spacing: f64) -> f64 {
    if spacing <= 0.0 {
        return 1.0;
    }
    ((wavelength / spacing - 2.0) * 0.5).clamp(0.0, 1.0)
}

/// Fractal Brownian motion; `wavelength` is the first octave's wavelength in metres.
pub fn fbm(seed: u64, x: f64, z: f64, wavelength: f64, octaves: u32, spacing: f64) -> f64 {
    let mut sum = 0.0;
    let mut amp = 1.0;
    let mut norm = 0.0;
    let mut wl = wavelength;
    for k in 0..octaves {
        let g = octave_gain(wl, spacing);
        if g > 0.0 {
            let f = 1.0 / wl;
            let off = k as f64 * 37.719;
            sum += amp
                * g
                * gradient_noise(
                    seed.wrapping_add(k as u64 * 0x1F3D_5B79),
                    x * f + off,
                    z * f - off,
                );
        }
        norm += amp;
        amp *= 0.5;
        wl *= 0.5;
    }
    sum / norm
}

/// Ridged multifractal in `[0, 1]` (sharp crests).
pub fn ridged(seed: u64, x: f64, z: f64, wavelength: f64, octaves: u32, spacing: f64) -> f64 {
    let mut sum = 0.0;
    let mut amp = 0.5;
    let mut norm = 0.0;
    let mut weight = 1.0;
    let mut wl = wavelength;
    for k in 0..octaves {
        let g = octave_gain(wl, spacing);
        let f = 1.0 / wl;
        let off = k as f64 * 53.137;
        let n = gradient_noise(
            seed.wrapping_add(k as u64 * 0x2545_F491),
            x * f + off,
            z * f + off * 0.5,
        );
        let r = 1.0 - n.abs();
        let r = r * r * weight;
        weight = (r * 2.0).clamp(0.0, 1.0);
        sum += amp * g * r;
        norm += amp;
        amp *= 0.5;
        wl *= 0.5;
    }
    sum / norm
}

#[inline]
pub fn smoothstep(e0: f64, e1: f64, x: f64) -> f64 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[inline]
pub fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn noise_range_and_determinism() {
        let mut mn = f64::MAX;
        let mut mx = f64::MIN;
        for i in 0..20000 {
            let x = i as f64 * 0.137 - 900.0;
            let z = (i * 7 % 311) as f64 * 0.311;
            let a = gradient_noise(9, x, z);
            assert_eq!(a.to_bits(), gradient_noise(9, x, z).to_bits());
            mn = mn.min(a);
            mx = mx.max(a);
        }
        assert!(mn >= -1.05 && mx <= 1.05, "{mn} {mx}");
        assert!(mx - mn > 1.0);
    }

    #[test]
    fn noise_is_continuous_across_lattice() {
        for k in -3..3 {
            let a = gradient_noise(3, k as f64 - 1e-9, 0.37);
            let b = gradient_noise(3, k as f64 + 1e-9, 0.37);
            assert!((a - b).abs() < 1e-6);
        }
    }

    #[test]
    fn spacing_cutoff_removes_fine_octaves() {
        let fine = fbm(1, 10.3, 5.1, 100.0, 6, 0.0);
        let coarse = fbm(1, 10.3, 5.1, 100.0, 6, 200.0);
        assert!(fine != coarse);
    }
}
