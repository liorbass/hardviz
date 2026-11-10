//! A small software renderer for egui output: tessellated meshes are
//! rasterised on the CPU into an RGBA buffer, so screenshots need no window,
//! GPU or display server.
//!
//! Blending matches `egui_glow` on a non-sRGB framebuffer: premultiplied
//! alpha, in gamma space. Edges are anti-aliased by egui's own feathering;
//! rendering at a multiple of the output scale and box-filtering down
//! ([`Canvas::downsample`]) smooths what feathering leaves.

use eframe::egui::{self, epaint, ClippedPrimitive, Color32, ImageData, TextureId, TexturesDelta};
use std::collections::HashMap;

/// A texture as premultiplied RGBA in 0..1.
struct Texture {
    size: [usize; 2],
    texels: Vec<[f32; 4]>,
}

impl Texture {
    /// Bilinear sample at normalised `uv`, clamped to the edge.
    fn sample(&self, uv: egui::Pos2) -> [f32; 4] {
        let [w, h] = self.size;
        let x = (uv.x * w as f32 - 0.5).clamp(0.0, (w - 1) as f32);
        let y = (uv.y * h as f32 - 0.5).clamp(0.0, (h - 1) as f32);
        let (x0, y0) = (x.floor() as usize, y.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let at = |x: usize, y: usize| self.texels[y * w + x];
        let (a, b, c, d) = (at(x0, y0), at(x1, y0), at(x0, y1), at(x1, y1));
        std::array::from_fn(|i| {
            let top = a[i] + (b[i] - a[i]) * fx;
            let bottom = c[i] + (d[i] - c[i]) * fx;
            top + (bottom - top) * fy
        })
    }
}

fn unit(c: Color32) -> [f32; 4] {
    let [r, g, b, a] = c.to_array();
    [r, g, b, a].map(|v| f32::from(v) / 255.0)
}

/// The textures egui has uploaded so far, kept across frames like a GPU would.
#[derive(Default)]
pub(super) struct Textures(HashMap<TextureId, Texture>);

impl Textures {
    /// Apply a frame's texture updates (whole images and partial patches).
    pub(super) fn update(&mut self, delta: &TexturesDelta) {
        for (id, d) in &delta.set {
            let (size, texels): ([usize; 2], Vec<[f32; 4]>) = match &d.image {
                ImageData::Color(img) => (img.size, img.pixels.iter().map(|&c| unit(c)).collect()),
                ImageData::Font(img) => (img.size, img.srgba_pixels(None).map(unit).collect()),
            };
            match d.pos {
                None => {
                    self.0.insert(*id, Texture { size, texels });
                }
                Some([px, py]) => {
                    let Some(t) = self.0.get_mut(id) else { continue };
                    for y in 0..size[1] {
                        for x in 0..size[0] {
                            t.texels[(py + y) * t.size[0] + px + x] = texels[y * size[0] + x];
                        }
                    }
                }
            }
        }
        for id in &delta.free {
            self.0.remove(id);
        }
    }
}

/// An RGBA image in premultiplied 0..1 floats.
pub(super) struct Canvas {
    pub(super) width: usize,
    pub(super) height: usize,
    pixels: Vec<[f32; 4]>,
}

impl Canvas {
    pub(super) fn new(width: usize, height: usize) -> Self {
        Self { width, height, pixels: vec![[0.0, 0.0, 0.0, 1.0]; width * height] }
    }

    /// Draw tessellated primitives; positions are in points, scaled by `ppp`.
    pub(super) fn draw(&mut self, prims: &[ClippedPrimitive], textures: &Textures, ppp: f32) {
        for p in prims {
            let epaint::Primitive::Mesh(mesh) = &p.primitive else { continue };
            let Some(tex) = textures.0.get(&mesh.texture_id) else { continue };
            let clip = p.clip_rect;
            let clip = [
                (clip.min.x * ppp).round().max(0.0) as usize,
                (clip.min.y * ppp).round().max(0.0) as usize,
                ((clip.max.x * ppp).round().max(0.0) as usize).min(self.width),
                ((clip.max.y * ppp).round().max(0.0) as usize).min(self.height),
            ];
            let (triangles, _) = mesh.indices.as_chunks::<3>();
            for tri in triangles {
                let v = [0, 1, 2].map(|i| mesh.vertices[tri[i] as usize]);
                self.triangle(&v, tex, ppp, clip);
            }
        }
    }

    fn triangle(&mut self, v: &[epaint::Vertex; 3], tex: &Texture, ppp: f32, clip: [usize; 4]) {
        let p = v.map(|v| (v.pos.x * ppp, v.pos.y * ppp));
        let edge = |a: (f32, f32), b: (f32, f32), x: f32, y: f32| (b.0 - a.0) * (y - a.1) - (b.1 - a.1) * (x - a.0);
        let area = edge(p[0], p[1], p[2].0, p[2].1);
        if area.abs() < 1e-6 {
            return;
        }
        let min_x = p.iter().map(|q| q.0).fold(f32::INFINITY, f32::min).floor().max(clip[0] as f32) as usize;
        let min_y = p.iter().map(|q| q.1).fold(f32::INFINITY, f32::min).floor().max(clip[1] as f32) as usize;
        let max_x = (p.iter().map(|q| q.0).fold(f32::NEG_INFINITY, f32::max).ceil() as usize).min(clip[2]);
        let max_y = (p.iter().map(|q| q.1).fold(f32::NEG_INFINITY, f32::max).ceil() as usize).min(clip[3]);
        let colors = v.map(|v| unit(v.color));
        // Top-left fill rule, so pixels on an edge shared by two triangles
        // are drawn once (no double-blended seams).
        let owns = |a: (f32, f32), b: (f32, f32), w: f32| {
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            let (dx, dy) = if area < 0.0 { (-dx, -dy) } else { (dx, dy) };
            w > 0.0 || (w == 0.0 && (dy < 0.0 || (dy == 0.0 && dx > 0.0)))
        };
        for y in min_y..max_y {
            let cy = y as f32 + 0.5;
            for x in min_x..max_x {
                let cx = x as f32 + 0.5;
                let s = area.signum();
                let w0 = edge(p[1], p[2], cx, cy) * s;
                let w1 = edge(p[2], p[0], cx, cy) * s;
                let w2 = edge(p[0], p[1], cx, cy) * s;
                if !(owns(p[1], p[2], w0) && owns(p[2], p[0], w1) && owns(p[0], p[1], w2)) {
                    continue;
                }
                let a = area.abs();
                let (b0, b1, b2) = (w0 / a, w1 / a, w2 / a);
                let uv = egui::pos2(
                    v[0].uv.x * b0 + v[1].uv.x * b1 + v[2].uv.x * b2,
                    v[0].uv.y * b0 + v[1].uv.y * b1 + v[2].uv.y * b2,
                );
                let t = tex.sample(uv);
                let src: [f32; 4] =
                    std::array::from_fn(|i| (colors[0][i] * b0 + colors[1][i] * b1 + colors[2][i] * b2) * t[i]);
                let dst = &mut self.pixels[y * self.width + x];
                for i in 0..4 {
                    dst[i] = src[i] + dst[i] * (1.0 - src[3]);
                }
            }
        }
    }

    /// Box-filter down by `factor` in both directions.
    pub(super) fn downsample(&self, factor: usize) -> Canvas {
        let (w, h) = (self.width / factor, self.height / factor);
        let n = (factor * factor) as f32;
        let mut out = Canvas::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let mut acc = [0.0f32; 4];
                for dy in 0..factor {
                    for dx in 0..factor {
                        let p = self.pixels[(y * factor + dy) * self.width + x * factor + dx];
                        for i in 0..4 {
                            acc[i] += p[i];
                        }
                    }
                }
                out.pixels[y * w + x] = acc.map(|v| v / n);
            }
        }
        out
    }

    /// The smallest rectangle holding every pixel that differs from the
    /// top-left one, grown by `margin` (clamped to the canvas).
    pub(super) fn content_bounds(&self, margin: usize) -> [usize; 4] {
        let bg = self.pixels[0];
        let differs = |p: &[f32; 4]| p.iter().zip(bg).any(|(a, b)| (a - b).abs() > 0.03);
        let (mut x0, mut y0, mut x1, mut y1) = (self.width, self.height, 0, 0);
        for y in 0..self.height {
            for x in 0..self.width {
                if differs(&self.pixels[y * self.width + x]) {
                    (x0, y0, x1, y1) = (x0.min(x), y0.min(y), x1.max(x + 1), y1.max(y + 1));
                }
            }
        }
        if x0 >= x1 {
            return [0, 0, self.width, self.height];
        }
        [
            x0.saturating_sub(margin),
            y0.saturating_sub(margin),
            (x1 + margin).min(self.width),
            (y1 + margin).min(self.height),
        ]
    }

    /// Write `[x0, y0, x1, y1]` of the canvas as an opaque 8-bit RGB PNG.
    pub(super) fn save_png(&self, path: &std::path::Path, [x0, y0, x1, y1]: [usize; 4]) -> std::io::Result<()> {
        let mut rgb = Vec::with_capacity((x1 - x0) * (y1 - y0) * 3);
        for y in y0..y1 {
            for x in x0..x1 {
                let p = self.pixels[y * self.width + x];
                // Opaque output: composite over black (already the base).
                rgb.extend(p[..3].iter().map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8));
            }
        }
        let file = std::io::BufWriter::new(std::fs::File::create(path)?);
        let mut enc = png::Encoder::new(file, (x1 - x0) as u32, (y1 - y0) as u32);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::High);
        let mut writer = enc.write_header().map_err(std::io::Error::other)?;
        writer.write_image_data(&rgb).map_err(std::io::Error::other)?;
        Ok(())
    }
}
