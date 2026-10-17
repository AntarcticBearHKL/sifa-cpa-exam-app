//! Backdrop and the frosted-glass surface.
//!
//! Layout style: the window is ONE continuous frosted sheet divided by
//! hairlines. Regions are flush — no outer gaps, no rounded cards, no shadows —
//! so nothing appears to float on top of a background.

pub mod paint;
pub mod widgets;

use egui::{Context, Frame, LayerId, Rect, Ui};

use crate::theme::Tokens;

/// Call once per frame, before any panel is drawn. `solid` selects the painted
/// gradient backdrop; when the OS supplies a real acrylic backdrop we paint only
/// a flat scrim so the desktop stays visible through the window.
pub fn backdrop(ctx: &Context, tokens: &Tokens, phase: f32, solid: bool) {
    let painter = ctx.layer_painter(LayerId::background());
    let rect = ctx.content_rect();
    if solid {
        paint::backdrop(&painter, rect, tokens, phase);
    } else {
        paint::scrim(&painter, rect, tokens);
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
