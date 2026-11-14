//! Digital elevation model on a regular local grid, built from Terrarium PNG tiles
//! (elevation = R*256 + G + B/256 - 32768 m).

use crate::frame::{LocalFrame, mercator_pixel};
use std::collections::HashMap;
use std::io::{self, Read, Write};

#[derive(Clone, Debug)]
pub struct Dem {
    pub x0: f64,
    pub z0: f64,
    pub cell: f64,
    pub w: usize,
    pub h: usize,
    pub data: Vec<f32>,
    pub lon0: f64,
    pub lat0: f64,
}

const MAGIC: &[u8; 8] = b"WGDEM001";

impl Dem {
    pub fn bounds(&self) -> (f64, f64, f64, f64) {
        (
            self.x0,
            self.z0,
            self.x0 + (self.w - 1) as f64 * self.cell,
            self.z0 + (self.h - 1) as f64 * self.cell,
        )
    }

    /// Bilinear elevation (clamped at the edges).
    pub fn sample(&self, x: f64, z: f64) -> f32 {
        let fx = ((x - self.x0) / self.cell).clamp(0.0, (self.w - 1) as f64 - 1e-9);
        let fz = ((z - self.z0) / self.cell).clamp(0.0, (self.h - 1) as f64 - 1e-9);
        let (i, j) = (fx as usize, fz as usize);
        let (tx, tz) = ((fx - i as f64) as f32, (fz - j as f64) as f32);
        let g = |a: usize, b: usize| self.data[b * self.w + a];
        (g(i, j) * (1.0 - tx) + g(i + 1, j) * tx) * (1.0 - tz)
            + (g(i, j + 1) * (1.0 - tx) + g(i + 1, j + 1) * tx) * tz
    }

    /// Build a DEM of `size_m` x `size_m` centred on the frame origin.
    ///
    /// `fetch(zoom, tile_x, tile_y)` returns the PNG bytes of a Terrarium tile.
    pub fn from_terrarium(
        frame: &LocalFrame,
        size_m: f64,
        cell: f64,
        zoom: u32,
        fetch: &mut dyn FnMut(u32, u32, u32) -> Option<Vec<u8>>,
    ) -> Result<Dem, String> {
        let n = (size_m / cell).round() as usize + 1;
        let half = size_m * 0.5;
        let mut tiles: HashMap<(u32, u32), Option<Vec<f32>>> = HashMap::new();
        let mut elev = |px: i64,
                        py: i64,
                        fetch: &mut dyn FnMut(u32, u32, u32) -> Option<Vec<u8>>|
         -> Option<f32> {
            let (tx, ty) = ((px.div_euclid(256)) as u32, (py.div_euclid(256)) as u32);
            let t = tiles
                .entry((tx, ty))
                .or_insert_with(|| fetch(zoom, tx, ty).and_then(|b| decode_terrarium(&b)));
            t.as_ref()
                .map(|v| v[(py.rem_euclid(256) * 256 + px.rem_euclid(256)) as usize])
        };
        let mut data = vec![0f32; n * n];
        for j in 0..n {
            for i in 0..n {
                let (x, z) = (-half + i as f64 * cell, -half + j as f64 * cell);
                let (lon, lat) = frame.from_local(x, z);
                let (px, py) = mercator_pixel(lon, lat, zoom);
                let (fx, fy) = (px - 0.5, py - 0.5);
                let (ix, iy) = (fx.floor() as i64, fy.floor() as i64);
                let (tx, ty) = ((fx - ix as f64) as f32, (fy - iy as f64) as f32);
                let mut s = [0f32; 4];
                for (k, (dx, dy)) in [(0, 0), (1, 0), (0, 1), (1, 1)].into_iter().enumerate() {
                    s[k] = elev(ix + dx, iy + dy, fetch)
                        .ok_or_else(|| format!("missing tile for pixel {} {}", ix + dx, iy + dy))?;
                }
                data[j * n + i] = (s[0] * (1.0 - tx) + s[1] * tx) * (1.0 - ty)
                    + (s[2] * (1.0 - tx) + s[3] * tx) * ty;
            }
        }
        Ok(Dem {
            x0: -half,
            z0: -half,
            cell,
            w: n,
            h: n,
            data,
            lon0: frame.lon0,
            lat0: frame.lat0,
        })
    }

    pub fn save(&self, w: &mut impl Write) -> io::Result<()> {
        w.write_all(MAGIC)?;
        for v in [self.x0, self.z0, self.cell, self.lon0, self.lat0] {
            w.write_all(&v.to_le_bytes())?;
        }
        w.write_all(&(self.w as u32).to_le_bytes())?;
        w.write_all(&(self.h as u32).to_le_bytes())?;
        for v in &self.data {
            w.write_all(&v.to_le_bytes())?;
        }
        Ok(())
    }

    pub fn load(r: &mut impl Read) -> io::Result<Dem> {
        let bad = |m: &str| io::Error::new(io::ErrorKind::InvalidData, m.to_string());
        let mut magic = [0u8; 8];
        r.read_exact(&mut magic)?;
        if &magic != MAGIC {
            return Err(bad("not a worldgen DEM"));
        }
        let mut f = [0f64; 5];
        let mut b8 = [0u8; 8];
        for v in f.iter_mut() {
            r.read_exact(&mut b8)?;
            *v = f64::from_le_bytes(b8);
        }
        let mut b4 = [0u8; 4];
        r.read_exact(&mut b4)?;
        let w = u32::from_le_bytes(b4) as usize;
        r.read_exact(&mut b4)?;
        let h = u32::from_le_bytes(b4) as usize;
        if w < 2 || h < 2 || w * h > 100_000_000 {
            return Err(bad("implausible DEM size"));
        }
        let mut data = Vec::with_capacity(w * h);
        for _ in 0..w * h {
            r.read_exact(&mut b4)?;
            data.push(f32::from_le_bytes(b4));
        }
        Ok(Dem {
            x0: f[0],
            z0: f[1],
            cell: f[2],
            lon0: f[3],
            lat0: f[4],
            w,
            h,
            data,
        })
    }
}

/// Decode a Terrarium PNG into 256x256 elevations (metres).
pub fn decode_terrarium(png_bytes: &[u8]) -> Option<Vec<f32>> {
    let dec = png::Decoder::new(std::io::Cursor::new(png_bytes));
    let mut reader = dec.read_info().ok()?;
    let mut buf = vec![0u8; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let ch = match info.color_type {
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        _ => return None,
    };
    if info.width != 256 || info.height != 256 || info.bit_depth != png::BitDepth::Eight {
        return None;
    }
    Some(
        (0..256 * 256)
            .map(|i| {
                buf[i * ch] as f32 * 256.0 + buf[i * ch + 1] as f32 + buf[i * ch + 2] as f32 / 256.0
                    - 32768.0
            })
            .collect(),
    )
}
