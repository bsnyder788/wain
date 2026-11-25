//! Small image buffers and the filters the detectors share.

use crate::Photo;

/// A colour image as floats in 0..=255, row by row from the top-left.
#[derive(Clone)]
pub(crate) struct Rgb {
    pub w: usize,
    pub h: usize,
    pub px: Vec<[f32; 3]>,
}

impl Rgb {
    /// The photo shrunk so its longer side is at most `max_side`, averaging
    /// the pixels each new pixel covers. A photo already that small is
    /// copied as it is.
    pub fn from_photo(photo: &Photo, max_side: usize) -> Rgb {
        let (sw, sh) = (photo.width, photo.height);
        let longest = sw.max(sh);
        let (w, h) = if longest <= max_side {
            (sw, sh)
        } else {
            let scale = max_side as f64 / longest as f64;
            (
                ((sw as f64 * scale).round() as usize).max(1),
                ((sh as f64 * scale).round() as usize).max(1),
            )
        };

        let mut px = Vec::with_capacity(w * h);
        for j in 0..h {
            let y0 = j * sh / h;
            let y1 = (((j + 1) * sh).div_ceil(h)).clamp(y0 + 1, sh);
            for i in 0..w {
                let x0 = i * sw / w;
                let x1 = (((i + 1) * sw).div_ceil(w)).clamp(x0 + 1, sw);
                let mut sum = [0u32; 3];
                for y in y0..y1 {
                    let row = &photo.rgba[(y * sw + x0) * 4..(y * sw + x1) * 4];
                    for pixel in row.as_chunks::<4>().0 {
                        sum[0] += pixel[0] as u32;
                        sum[1] += pixel[1] as u32;
                        sum[2] += pixel[2] as u32;
                    }
                }
                let count = ((y1 - y0) * (x1 - x0)) as f32;
                px.push(sum.map(|channel| channel as f32 / count));
            }
        }
        Rgb { w, h, px }
    }

    /// A Gaussian blur. Pixels past the edge repeat the edge.
    pub fn blurred(&self, sigma: f32) -> Rgb {
        let radius = (sigma * 2.5).ceil() as isize;
        let mut kernel: Vec<f32> = (-radius..=radius)
            .map(|k| (-(k * k) as f32 / (2.0 * sigma * sigma)).exp())
            .collect();
        let total: f32 = kernel.iter().sum();
        kernel.iter_mut().for_each(|value| *value /= total);

        let (w, h) = (self.w as isize, self.h as isize);
        let pass = |source: &[[f32; 3]], horizontal: bool| -> Vec<[f32; 3]> {
            let mut out = vec![[0.0; 3]; source.len()];
            for y in 0..h {
                for x in 0..w {
                    let mut sum = [0.0f32; 3];
                    for (index, weight) in kernel.iter().enumerate() {
                        let k = index as isize - radius;
                        let (sx, sy) = if horizontal {
                            ((x + k).clamp(0, w - 1), y)
                        } else {
                            (x, (y + k).clamp(0, h - 1))
                        };
                        let p = source[(sy * w + sx) as usize];
                        sum[0] += weight * p[0];
                        sum[1] += weight * p[1];
                        sum[2] += weight * p[2];
                    }
                    out[(y * w + x) as usize] = sum;
                }
            }
            out
        };
        let px = pass(&pass(&self.px, true), false);
        Rgb {
            w: self.w,
            h: self.h,
            px,
        }
    }
}

/// How fast the image changes at each pixel, and in which direction.
pub(crate) struct Gradient {
    pub w: usize,
    pub h: usize,
    /// Change per pixel going right and going down, in the colour channel
    /// that changes most at that pixel.
    pub gx: Vec<f32>,
    pub gy: Vec<f32>,
    pub mag: Vec<f32>,
}

impl Gradient {
    /// Sobel gradients. A wall and the ceiling above it can match in
    /// brightness and differ only in tint, so each channel is measured and
    /// the strongest wins.
    pub fn of(image: &Rgb) -> Gradient {
        let (w, h) = (image.w, image.h);
        let mut gx = vec![0.0; w * h];
        let mut gy = vec![0.0; w * h];
        let mut mag = vec![0.0; w * h];
        if w < 3 || h < 3 {
            return Gradient { w, h, gx, gy, mag };
        }
        for y in 1..h - 1 {
            for x in 1..w - 1 {
                let at = |dx: isize, dy: isize| {
                    image.px[(y as isize + dy) as usize * w + (x as isize + dx) as usize]
                };
                let (nw, n, ne) = (at(-1, -1), at(0, -1), at(1, -1));
                let (west, east) = (at(-1, 0), at(1, 0));
                let (sw, s, se) = (at(-1, 1), at(0, 1), at(1, 1));
                let mut best = (0.0f32, 0.0f32, 0.0f32);
                for c in 0..3 {
                    let dx = (ne[c] + 2.0 * east[c] + se[c] - nw[c] - 2.0 * west[c] - sw[c]) / 8.0;
                    let dy = (sw[c] + 2.0 * s[c] + se[c] - nw[c] - 2.0 * n[c] - ne[c]) / 8.0;
                    let m = dx * dx + dy * dy;
                    if m > best.2 {
                        best = (dx, dy, m);
                    }
                }
                let index = y * w + x;
                gx[index] = best.0;
                gy[index] = best.1;
                mag[index] = best.2.sqrt();
            }
        }
        Gradient { w, h, gx, gy, mag }
    }

    /// Keeps only the crest of each edge: pixels at least `threshold` strong
    /// that are no weaker than their two neighbours across the edge.
    pub fn thin(&self, threshold: f32) -> Vec<bool> {
        let (w, h) = (self.w, self.h);
        let mut keep = vec![false; w * h];
        if w < 3 || h < 3 {
            return keep;
        }
        for y in 1..h - 1 {
            for x in 1..w - 1 {
                let index = y * w + x;
                let m = self.mag[index];
                if m < threshold {
                    continue;
                }
                // Step one pixel along the gradient, to the nearest of the
                // eight neighbours.
                let (dx, dy) = (self.gx[index] / m, self.gy[index] / m);
                let (sx, sy) = (dx.round() as isize, dy.round() as isize);
                let ahead = self.mag[(y as isize + sy) as usize * w + (x as isize + sx) as usize];
                let behind = self.mag[(y as isize - sy) as usize * w + (x as isize - sx) as usize];
                keep[index] = m >= ahead && m > behind;
            }
        }
        keep
    }
}
