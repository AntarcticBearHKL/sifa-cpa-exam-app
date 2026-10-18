//! Low-level glass painting: the backdrop, the frosted surface and progress
//! rings.
//!
//! The backdrop is the captured, Gaussian-blurred desktop (see
//! [`crate::glass::blur`]) with a scrim over it, or — when the capture is
//! unavailable — the painted gradient fallback. A pane is built from three
//! things and nothing more: a milky, near-colourless body, a faint contact
//! shadow, and a hairline rim. No grain, no glow — real glass has neither.

use std::f32::consts::TAU;

use egui::epaint::{Mesh, Vertex};
use egui::{Color32, CornerRadius, Painter, Pos2, Rect, Shape};

use crate::theme::Tokens;

const BLOB_ANCHORS: [(f32, f32); 5] = [
    (0.16, 0.20),
    (0.84, 0.14),
    (0.72, 0.74),
    (0.28, 0.86),
    (0.50, 0.44),
];

const BLOB_RADII: [f32; 5] = [0.72, 0.68, 0.80, 0.66, 0.58];

const RING_RINGS: usize = 4;
const RING_SEGMENTS: usize = 56;

fn push_vertex(mesh: &mut Mesh, pos: Pos2, color: Color32) -> u32 {
    mesh.vertices.push(Vertex::untextured(pos, color));
    (mesh.vertices.len() - 1) as u32
}

/// `Color32` channels are premultiplied, so scaling all four by `alpha` equals
/// multiplying its opacity.
pub(crate) fn with_alpha(color: Color32, alpha: f32) -> Color32 {
    let scale = |c: u8| (c as f32 * alpha.clamp(0.0, 1.0)).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgba_premultiplied(
        scale(color.r()),
        scale(color.g()),
        scale(color.b()),
        scale(color.a()),
    )
}

pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round().clamp(0.0, 255.0) as u8;
    Color32::from_rgba_premultiplied(
        f(a.r(), b.r()),
        f(a.g(), b.g()),
        f(a.b(), b.b()),
        f(a.a(), b.a()),
    )
}

/// Drifting centre of wash `i`, expressed relative to `rect`.
pub(crate) fn blob_center(rect: Rect, i: usize, phase: f32) -> Pos2 {
    let i_f = i as f32;
    let (ax, ay) = BLOB_ANCHORS[i % BLOB_ANCHORS.len()];
    let dx = (phase * 0.09 + i_f * 1.7).sin() * 0.045;
    let dy = (phase * 0.11 + i_f * 2.3).cos() * 0.055;
    Pos2::new(
        rect.left() + (ax + dx) * rect.width(),
        rect.top() + (ay + dy) * rect.height(),
    )
}

/// Smooth radial falloff built as a triangle fan over concentric rings.
pub(crate) fn radial_mesh(
    center: Pos2,
    radius: f32,
    color: Color32,
    rings: usize,
    segments: usize,
) -> Mesh {
    let mut mesh = Mesh::default();
    if radius <= 0.0 || rings == 0 || segments < 3 {
        return mesh;
    }

    mesh.reserve_vertices(1 + rings * segments);
    mesh.reserve_triangles(rings * segments * 2);

    let center_idx = push_vertex(&mut mesh, center, color);
    let mut prev_ring = vec![center_idx; segments];

    for ring in 1..=rings {
        let t = ring as f32 / rings as f32;
        let ring_radius = radius * t;
        let ring_color = with_alpha(color, (1.0 - t).powf(1.05));

        let mut cur_ring = Vec::with_capacity(segments);
        for seg in 0..segments {
            let angle = (seg as f32 / segments as f32) * TAU;
            let pos = Pos2::new(
                center.x + angle.cos() * ring_radius,
                center.y + angle.sin() * ring_radius,
            );
            cur_ring.push(push_vertex(&mut mesh, pos, ring_color));
        }

        for seg in 0..segments {
            let next = (seg + 1) % segments;
            let (a, b) = (prev_ring[seg], prev_ring[next]);
            let (c, d) = (cur_ring[seg], cur_ring[next]);
            mesh.add_triangle(a, c, d);
            if ring > 1 {
                mesh.add_triangle(a, d, b);
            }
        }

        prev_ring = cur_ring;
    }

    mesh
}

/// Flat frosted fill for a flush region — no rounding, no shadow.
pub fn fill(painter: &Painter, rect: Rect, tokens: &Tokens, strong: bool) {
    let g = &tokens.palette.glass;
    let color = if strong { g.fill_strong } else { g.fill };
    painter.rect_filled(rect, CornerRadius::ZERO, color);
}

/// Draw the captured, Gaussian-blurred desktop across `rect`.
///
/// `uv` is the content rect's sub-rectangle of the capture texture, derived
/// from the window's screen position relative to the capture origin — so while
/// the window moves, the blurred desktop stays registered with the real
/// desktop instead of sliding along with the window. `Color32::WHITE` tints
/// nothing: the texture is drawn opaque.
pub fn desktop(painter: &Painter, rect: Rect, texture: &egui::TextureHandle, uv: Rect) {
    painter.image(texture.id(), rect, uv, Color32::WHITE);
}

/// Scrim alpha from the 透明度 setting: `0` → 255 (opaque), `100` → 0 (fully
/// clear), clamped and monotonically decreasing in between.
pub fn scrim_alpha(transparency: u8) -> u8 {
    let t = transparency.min(100) as f32 / 100.0;
    (255.0 * (1.0 - t)).round() as u8
}

/// Flat wash painted over the blurred desktop. Deliberately featureless: the
/// blurred desktop behind IS the background, and this only exists to keep text
/// legible over arbitrary content. Its opacity is the 透明度 setting.
pub fn scrim(painter: &Painter, rect: Rect, tokens: &Tokens, alpha: u8) {
    let [r, g, b, _] = tokens.palette.background.scrim.to_srgba_unmultiplied();
    painter.rect_filled(
        rect,
        CornerRadius::ZERO,
        Color32::from_rgba_unmultiplied(r, g, b, alpha),
    );
}

/// Fallback backdrop, used only when the desktop capture is unavailable: a
/// pale base plus a few very soft, almost achromatic washes. Kept
/// low-frequency because that is what reads as "blurred" behind the panes.
pub fn backdrop(painter: &Painter, rect: Rect, tokens: &Tokens, phase: f32) {
    let bg = &tokens.palette.background;
    painter.add(Shape::gradient_rect(
        rect,
        egui::epaint::Direction::TopDown,
        [bg.base_top, bg.base_bottom],
    ));

    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }

    let big = rect.width().max(rect.height());
    for (i, color) in bg.blobs.iter().enumerate() {
        let radius = big * BLOB_RADII[i % BLOB_RADII.len()];
        let mesh = radial_mesh(
            blob_center(rect, i, phase),
            radius,
            *color,
            RING_RINGS,
            RING_SEGMENTS,
        );
        painter.add(Shape::mesh(mesh));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_rect() -> Rect {
        Rect::from_min_size(Pos2::new(0.0, 0.0), egui::vec2(800.0, 600.0))
    }

    #[test]
    fn radial_mesh_has_expected_geometry() {
        let mesh = radial_mesh(Pos2::ZERO, 100.0, Color32::from_rgba_unmultiplied(10, 20, 30, 90), 4, 56);
        assert_eq!(mesh.vertices.len(), 1 + 4 * 56);
        assert_eq!(mesh.indices.len() % 3, 0);
        for index in &mesh.indices {
            assert!((*index as usize) < mesh.vertices.len());
        }
    }

    #[test]
    fn radial_mesh_fades_to_transparent_at_the_rim() {
        let mesh = radial_mesh(Pos2::ZERO, 100.0, Color32::from_rgba_unmultiplied(255, 255, 255, 96), 4, 48);
        assert_eq!(mesh.vertices[0].color.a(), 96);
        let rim_alpha = mesh.vertices[1..].iter().map(|v| v.color.a()).min().unwrap();
        assert_eq!(rim_alpha, 0, "outermost ring must be fully transparent");
    }

    #[test]
    fn degenerate_inputs_do_not_panic() {
        assert!(radial_mesh(Pos2::ZERO, 0.0, Color32::WHITE, 4, 48).vertices.is_empty());
        assert!(radial_mesh(Pos2::ZERO, -5.0, Color32::WHITE, 4, 48).vertices.is_empty());
        assert!(radial_mesh(Pos2::ZERO, 10.0, Color32::WHITE, 0, 2).vertices.is_empty());
        assert!(radial_mesh(Pos2::ZERO, 10.0, Color32::WHITE, 4, 2).vertices.is_empty());
    }

    #[test]
    fn blob_centers_stay_inside_the_rect() {
        let rect = sample_rect();
        for i in 0..5 {
            for step in 0..40 {
                let c = blob_center(rect, i, step as f32 * 0.25);
                assert!(c.x >= rect.left() && c.x <= rect.right());
                assert!(c.y >= rect.top() && c.y <= rect.bottom());
            }
        }
    }

    #[test]
    fn with_alpha_and_mix_are_well_behaved() {
        let c = with_alpha(Color32::from_rgba_unmultiplied(255, 255, 255, 100), 0.5);
        assert!(c.a() < 100 && c.a() > 0);
        assert_eq!(with_alpha(Color32::WHITE, 0.0).a(), 0);

        let black = Color32::from_rgb(0, 0, 0);
        let white = Color32::from_rgb(255, 255, 255);
        assert_eq!(mix(black, white, -1.0), black);
        assert_eq!(mix(black, white, 2.0), white);
        assert!((mix(black, white, 0.5).r() as i32 - 128).abs() <= 1);
    }

    #[test]
    fn scrim_alpha_maps_transparency_to_a_clamped_monotonic_alpha() {
        assert_eq!(scrim_alpha(0), 255, "0% transparent is an opaque scrim");
        assert_eq!(scrim_alpha(100), 0, "100% transparent paints nothing");
        assert_eq!(scrim_alpha(200), 0, "above 100 is clamped");
        assert_eq!(
            scrim_alpha(12),
            224,
            "the default ≈ the scrim the app shipped with"
        );

        let mut prev = 255;
        for t in 0..=100u8 {
            let a = scrim_alpha(t);
            assert!(a <= prev, "alpha must not rise with transparency at {t}");
            prev = a;
        }
    }

    /// The scrim keeps body text at WCAG AA over the worst-case desktop for
    /// its mode — the same invariant `theme/tokens.rs` guards for the token,
    /// now that the alpha is user-controlled: 0% transparency must not be
    /// weaker than the default it replaces.
    #[test]
    fn the_default_scrim_alpha_is_not_weaker_than_the_token() {
        let token_alpha = Tokens::new(crate::theme::Mode::Dark)
            .palette
            .background
            .scrim
            .a();
        assert!(
            scrim_alpha(12) >= token_alpha.saturating_sub(8),
            "the default (12%) must stay near the shipped scrim alpha, got {} vs {token_alpha}",
            scrim_alpha(12)
        );
    }
}
