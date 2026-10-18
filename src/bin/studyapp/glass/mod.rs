//! Backdrop and the frosted-glass surface.
//!
//! Layout style: the window is ONE continuous frosted sheet divided by
//! hairlines. Regions are flush — no outer gaps, no rounded cards, no shadows —
//! so nothing appears to float on top of a background.

pub mod blur;
pub mod paint;
pub mod widgets;

use egui::{Context, Frame, LayerId, Rect, TextureHandle, Ui};

use crate::theme::Tokens;

/// The blurred desktop texture plus the UV rect of the window's content area
/// inside it. Both are prepared by `App`, which owns the capture and the
/// window's screen rect.
pub struct DesktopGlass<'a> {
    pub texture: &'a TextureHandle,
    /// Where the content rect falls inside the capture, in UV coordinates.
    /// Derived from the window's screen position, so the blurred desktop stays
    /// registered with the real desktop while the window moves.
    pub uv: Rect,
}

/// Call once per frame, before any panel is drawn. With `desktop` the backdrop
/// is the captured, Gaussian-blurred desktop under a scrim whose alpha is the
/// 透明度 setting; without it (capture unavailable) the painted gradient
/// fallback is used instead, so the window is never unreadable.
pub fn backdrop(
    ctx: &Context,
    tokens: &Tokens,
    phase: f32,
    desktop: Option<DesktopGlass<'_>>,
    transparency: u8,
) {
    let painter = ctx.layer_painter(LayerId::background());
    let rect = ctx.content_rect();
    match desktop {
        Some(d) => {
            paint::desktop(&painter, rect, d.texture, d.uv);
            paint::scrim(&painter, rect, tokens, paint::scrim_alpha(transparency));
        }
        None => paint::backdrop(&painter, rect, tokens, phase),
    }
}

/// Panel padding only. Deliberately no outer margin: the regions are flush.
pub fn frame(tokens: &Tokens) -> Frame {
    Frame::new().inner_margin(egui::Margin::symmetric(
        tokens.space.lg as i8,
        tokens.space.md as i8,
    ))
}

/// A region's own rect sits one inner margin outside `ui.max_rect()`.
fn region_rect(ui: &Ui, tokens: &Tokens) -> Rect {
    ui.max_rect()
        .expand2(egui::vec2(tokens.space.lg, tokens.space.md))
}

/// Flat frosted fill for a flush region — no rounding, no shadow.
pub fn panel_fill(ui: &Ui, tokens: &Tokens, strong: bool) {
    paint::fill(ui.painter(), region_rect(ui, tokens), tokens, strong);
}

/// Hairline along the bottom edge of a region (under a header).
pub fn divider_bottom(ui: &Ui, tokens: &Tokens) {
    let r = region_rect(ui, tokens);
    ui.painter().line_segment(
        [egui::pos2(r.left(), r.bottom()), egui::pos2(r.right(), r.bottom())],
        egui::Stroke::new(1.0, tokens.palette.glass.divider),
    );
}

/// Hairline along the top edge of a region (above a footer).
pub fn divider_top(ui: &Ui, tokens: &Tokens) {
    let r = region_rect(ui, tokens);
    ui.painter().line_segment(
        [egui::pos2(r.left(), r.top()), egui::pos2(r.right(), r.top())],
        egui::Stroke::new(1.0, tokens.palette.glass.divider),
    );
}

/// Hairline along the right edge of a region (beside the rail).
pub fn divider_right(ui: &Ui, tokens: &Tokens) {
    let r = region_rect(ui, tokens);
    ui.painter().line_segment(
        [egui::pos2(r.right(), r.top()), egui::pos2(r.right(), r.bottom())],
        egui::Stroke::new(1.0, tokens.palette.glass.divider),
    );
}
