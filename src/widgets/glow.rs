//! A soft, diffuse glow behind a stroke (a gauge's arc, a bar's fill).
//!
//! Drawn as one mesh whose opacity is set per vertex and blended by the GPU,
//! so it fades continuously. Stacking translucent strokes instead shows
//! bands: each layer's opacity rounds to a whole step out of 255, and a soft
//! glow needs steps smaller than that.

use crate::num::count_f32;
use egui::{Color32, Mesh, Pos2};

/// A stroke to glow around, in its own coordinates: `along` runs from 0 to
/// `length` down its middle, `across` from `-half` to `half` over its width.
pub struct Glow {
    pub length: f32,
    pub half: f32,
    /// How far the glow diffuses past the stroke's edges and ends, in points.
    pub reach: f32,
    /// Opacity right at the stroke's edge, 0-1.
    pub peak: f32,
}

impl Glow {
    /// The glow as a mesh. `place(along, across)` puts a point of the
    /// stroke's coordinates on screen (`along` may run `reach` past either
    /// end); `color(s)` is the glow's colour at `s` along it (0 start, 1 end),
    /// so it can follow a gradient.
    pub fn mesh(&self, place: impl Fn(f32, f32) -> Pos2, color: impl Fn(f32) -> Color32) -> Mesh {
        let mut mesh = Mesh::default();
        if self.length <= 0.0 || self.reach <= 0.0 {
            return mesh;
        }
        let outer = self.half + self.reach;
        let ext = self.reach;
        // About one column per 3 points along, and enough rows that the
        // fade is sampled finely; the GPU blends linearly in between.
        let cols = (((self.length + 2.0 * ext) / 3.0).ceil() as usize).clamp(8, 400);
        let rows = 24;
        for i in 0..=cols {
            let along = -ext + (self.length + 2.0 * ext) * count_f32(i) / count_f32(cols);
            let past_end = (-along).max(along - self.length).max(0.0);
            let base = color((along / self.length).clamp(0.0, 1.0));
            for j in 0..=rows {
                let across = -outer + 2.0 * outer * count_f32(j) / count_f32(rows);
                let outside = (across.abs() - self.half).max(0.0);
                let distance = outside.hypot(past_end);
                mesh.colored_vertex(
                    place(along, across),
                    base.gamma_multiply(self.peak * falloff(distance / self.reach)),
                );
            }
        }
        let stride = u32::try_from(rows + 1).unwrap_or(u32::MAX);
        for i in 0..u32::try_from(cols).unwrap_or(0) {
            for j in 0..u32::try_from(rows).unwrap_or(0) {
                let a = i * stride + j;
                let b = a + stride;
                mesh.add_triangle(a, a + 1, b);
                mesh.add_triangle(a + 1, b + 1, b);
            }
        }
        mesh
    }
}

/// Opacity at `x` reaches out (0 at the edge, 1 at the glow's limit): a
/// smooth bell, `(1 - x²)³`, flat at the edge and reaching zero with zero
/// slope, so neither end of the fade shows a line.
fn falloff(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    let k = 1.0 - x * x;
    k * k * k
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falloff_is_smooth_at_both_ends() {
        assert_eq!(falloff(0.0), 1.0);
        assert_eq!(falloff(1.0), 0.0);
        assert!(falloff(0.01) > 0.999, "flat at the edge");
        assert!(falloff(0.99) < 1e-5, "reaches zero gently");
        assert!((0..100).all(|i| falloff(i as f32 / 100.0) >= falloff((i + 1) as f32 / 100.0)));
    }

    #[test]
    fn glow_is_brightest_at_the_stroke_and_gone_at_its_reach() {
        let glow = Glow { length: 100.0, half: 5.0, reach: 10.0, peak: 0.5 };
        let mesh = glow.mesh(Pos2::new, |_| Color32::WHITE);
        assert!(mesh.is_valid());
        let alpha_at = |x: f32, y: f32| {
            mesh.vertices
                .iter()
                .min_by(|a, b| (a.pos - Pos2::new(x, y)).length().total_cmp(&(b.pos - Pos2::new(x, y)).length()))
                .map(|v| v.color.a())
                .unwrap_or(0)
        };
        assert!(alpha_at(50.0, 5.0) >= 120, "half opacity at the edge");
        assert_eq!(alpha_at(50.0, 15.0), 0, "nothing at the reach");
        assert_eq!(alpha_at(-10.0, 0.0), 0, "fades out past the ends too");
        assert!(alpha_at(-3.0, 0.0) > 0, "but diffuses a little beyond them");
    }
}
