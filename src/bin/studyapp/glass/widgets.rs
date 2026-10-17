//! Custom glass widgets: segmented tabs with a sliding indicator, badges and
//! chips.

use egui::{
    Align2, Color32, CornerRadius, FontId, Id, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui,
    Vec2,
};

use crate::anim::motion;
use crate::theme::Tokens;

const TRACK_HEIGHT: f32 = 32.0;

pub use crate::glass::paint::mix;

/// `offsets[i]` is the left edge of item `i`; returns the interpolated
/// `(left, width)` of the indicator at fractional position `slide`, so the pill
/// stretches smoothly as it moves between differently sized items.
pub(crate) fn indicator_span(offsets: &[f32], widths: &[f32], slide: f32) -> (f32, f32) {
    let n = widths.len();
    if n == 0 || offsets.len() != n {
        return (0.0, 0.0);
    }
    let s = slide.clamp(0.0, (n - 1) as f32);
    let i0 = s.floor() as usize;
    let i1 = (i0 + 1).min(n - 1);
    let frac = s - i0 as f32;
    let left = offsets[i0] + (offsets[i1] - offsets[i0]) * frac;
    let width = widths[i0] + (widths[i1] - widths[i0]) * frac;
    (left, width)
}

fn label_font() -> FontId {
    FontId::proportional(14.0)
}

/// Segmented control with a pill indicator that slides and stretches between
/// items. Returns the index the user clicked, if any.
pub fn segmented(
    ui: &mut Ui,
    tokens: &Tokens,
    id_salt: &str,
    items: &[&str],
    selected: usize,
) -> Option<usize> {
    if items.is_empty() {
        return None;
    }

    let font = label_font();
    let pad_x = tokens.space.md;
    let mut widths = Vec::with_capacity(items.len());
    for item in items {
        let galley = ui
            .painter()
            .layout_no_wrap((*item).to_owned(), font.clone(), Color32::WHITE);
        widths.push(galley.size().x + pad_x * 2.0);
    }
    let mut offsets = Vec::with_capacity(items.len());
    let mut acc = 0.0;
    for w in &widths {
        offsets.push(acc);
        acc += w;
    }
    let total_width = acc;

    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(total_width, TRACK_HEIGHT),
        Sense::hover(),
    );
    let base_id = ui.id().with(id_salt);
    let slide = motion::ease_value(
        ui.ctx(),
        base_id.with("slide"),
        selected as f32,
        motion::duration::SLOW,
        motion::curve::STATE,
    );

    let radius = CornerRadius::same(tokens.radius.pill);
    let painter = ui.painter();

    painter.rect_filled(rect, radius, tokens.palette.glass.fill_sunken);
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, tokens.palette.glass.stroke),
        StrokeKind::Inside,
    );

    let (ind_left, ind_width) = indicator_span(&offsets, &widths, slide);
    let indicator = Rect::from_min_size(
        Pos2::new(rect.left() + ind_left + 3.0, rect.top() + 3.0),
        Vec2::new((ind_width - 6.0).max(0.0), TRACK_HEIGHT - 6.0),
    );
    // Selection is a tinted fill. A full-strength accent stroke here is what
    // made the indicator read as a neon wireframe on the dark sheet.
    painter.rect_filled(
        indicator,
        radius,
        mix(tokens.palette.accent.soft, tokens.palette.accent.base, 0.35),
    );

    let mut clicked = None;
    for (i, item) in items.iter().enumerate() {
        let item_rect = Rect::from_min_size(
            Pos2::new(rect.left() + offsets[i], rect.top()),
            Vec2::new(widths[i], TRACK_HEIGHT),
        );
        let response = ui.interact(item_rect, base_id.with(("item", i)), Sense::CLICK);
        let sel_amount = (1.0 - (slide - i as f32).abs()).clamp(0.0, 1.0);
        let hover = motion::ease_bool(
            ui.ctx(),
            base_id.with(("hover", i)),
            response.hovered(),
            motion::duration::FAST,
            motion::curve::ENTER,
        );
        let base = mix(tokens.palette.ink.secondary, tokens.palette.ink.primary, hover);
        painter.text(
            item_rect.center(),
            Align2::CENTER_CENTER,
            *item,
            font.clone(),
            mix(base, tokens.palette.ink.primary, sel_amount),
        );
        if response.clicked() {
            clicked = Some(i);
        }
    }

    clicked
}

/// Like [`chip`], but with an explicit on/off state; hover eases toward the
/// on-state so the control still feels alive when off.
pub fn toggle_chip(ui: &mut Ui, tokens: &Tokens, label: &str, on: bool) -> Response {
    let font = FontId::proportional(12.5);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font.clone(), Color32::WHITE);
    let size = Vec2::new(
        galley.size().x + tokens.space.md * 2.0,
        galley.size().y + tokens.space.xs * 2.0,
    );
    let (rect, response) = ui.allocate_exact_size(size, Sense::CLICK);

    let hover = motion::ease_bool(
        ui.ctx(),
        response.id.with("hover"),
        response.hovered(),
        motion::duration::FAST,
        motion::curve::ENTER,
    );
    let on_amt = motion::ease_bool(
        ui.ctx(),
        response.id.with("on"),
        on,
        motion::duration::BASE,
        motion::curve::STATE,
    );
    let t = on_amt.max(hover);
    let radius = CornerRadius::same(tokens.radius.pill);
    let fill = mix(tokens.palette.glass.fill, tokens.palette.accent.soft, t);
    // The hairline stays neutral at every strength: the tinted fill already
    // carries the on-state, so an accent stroke would only add a neon edge.
    let stroke = tokens.palette.glass.stroke;
    let text = mix(tokens.palette.ink.secondary, tokens.palette.ink.primary, t);

    let painter = ui.painter();
    painter.rect_filled(rect, radius, fill);
    painter.rect_stroke(rect, radius, Stroke::new(1.0, stroke), StrokeKind::Inside);
    painter.text(rect.center(), Align2::CENTER_CENTER, label, font, text);
    response
}

/// Full-width selectable row: painted square checkbox, label, right-aligned
/// note. Drawn rather than text-based because the CJK font has no ✔glyph.
pub fn check_row(
    ui: &mut Ui,
    tokens: &Tokens,
    id: Id,
    label: &str,
    trailing: &str,
    checked: bool,
    width: f32,
) -> Response {
    const HEIGHT: f32 = 26.0;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, HEIGHT), Sense::CLICK);

    let hover = motion::ease_bool(
        ui.ctx(),
        id.with("hover"),
        response.hovered(),
        motion::duration::FAST,
        motion::curve::ENTER,
    );
    let sel = motion::ease_bool(
        ui.ctx(),
        id.with("checked"),
        checked,
        motion::duration::BASE,
        motion::curve::STATE,
    );
    let strength = hover.max(sel);

    let painter = ui.painter();
    if strength > 0.001 {
        let pill = rect.shrink2(Vec2::new(0.0, 1.0));
        let target = mix(
            tokens.palette.glass.fill_strong,
            tokens.palette.accent.soft,
            sel,
        );
        painter.rect_filled(
            pill,
            CornerRadius::same(tokens.radius.sm),
            mix(Color32::TRANSPARENT, target, strength),
        );
    }

    let box_size = 14.0;
    let box_rect = Rect::from_min_size(
        Pos2::new(rect.left() + tokens.space.sm, rect.center().y - box_size * 0.5),
        Vec2::splat(box_size),
    );
    let box_radius = CornerRadius::same(tokens.radius.sm);
    if sel > 0.001 {
        // Tinted fill carries the checked state; the edge stays neutral.
        painter.rect_filled(
            box_rect,
            box_radius,
            mix(
                Color32::TRANSPARENT,
                mix(tokens.palette.accent.soft, tokens.palette.accent.base, 0.4),
                sel,
            ),
        );
    }
    painter.rect_stroke(
        box_rect,
        box_radius,
        Stroke::new(1.0, tokens.palette.glass.stroke),
        StrokeKind::Inside,
    );
    if sel > 0.001 {
        let a = Pos2::new(box_rect.left() + box_size * 0.24, box_rect.center().y + box_size * 0.02);
        let b = Pos2::new(box_rect.left() + box_size * 0.43, box_rect.center().y + box_size * 0.21);
        let c = Pos2::new(box_rect.left() + box_size * 0.76, box_rect.center().y - box_size * 0.20);
        let stroke = Stroke::new(1.5, tokens.palette.ink.primary.gamma_multiply(sel));
        painter.line_segment([a, b], stroke);
        painter.line_segment([b, c], stroke);
    }

    painter.text(
        Pos2::new(box_rect.right() + tokens.space.sm, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(13.0),
        tokens.palette.ink.primary,
    );
    painter.text(
        Pos2::new(rect.right() - tokens.space.sm, rect.center().y),
        Align2::RIGHT_CENTER,
        trailing,
        FontId::proportional(11.5),
        tokens.palette.ink.muted,
    );

    response
}

/// The one prominent action on a screen (e.g. 开始练习. Everything else uses
/// the default button so the primary action actually stands out.
pub fn primary_button(ui: &mut Ui, tokens: &Tokens, label: &str, enabled: bool) -> Response {
    let font = FontId::proportional(15.0);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font.clone(), Color32::WHITE);
    let size = Vec2::new(
        (galley.size().x + tokens.space.xl * 2.0).max(132.0),
        38.0,
    );
    let (rect, resp) = ui.allocate_exact_size(size, Sense::CLICK);
    let hover = motion::ease_bool(
        ui.ctx(),
        resp.id.with("hover"),
        resp.hovered() && enabled,
        motion::duration::FAST,
        motion::curve::ENTER,
    );

    let painter = ui.painter();
    let radius = CornerRadius::same(tokens.radius.md);
    let accent = tokens.palette.accent;
    // The primary action reads through its tinted slate fill, not through an
    // outline: a full-strength accent stroke reads as a neon wire.
    let (fill, stroke, text) = if enabled {
        (
            mix(accent.soft, accent.base, 0.5 + hover * 0.25),
            tokens.palette.glass.stroke,
            tokens.palette.ink.primary,
        )
    } else {
        (
            tokens.palette.glass.fill,
            tokens.palette.glass.stroke,
            tokens.palette.ink.muted,
        )
    };

    painter.rect_filled(rect, radius, fill);
    painter.rect_stroke(rect, radius, Stroke::new(1.0, stroke), StrokeKind::Inside);
    painter.text(rect.center(), Align2::CENTER_CENTER, label, font, text);

    if enabled {
        resp
    } else {
        resp.on_hover_text("先选好范围")
    }
}

/// Small tinted pill, used for chapter labels and related-knowledge chips.
pub fn chip(ui: &mut Ui, tokens: &Tokens, label: &str, interactive: bool) -> Response {
    let font = FontId::proportional(12.5);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font.clone(), Color32::WHITE);
    let size = Vec2::new(
        galley.size().x + tokens.space.md * 2.0,
        galley.size().y + tokens.space.xs * 2.0,
    );
    let sense = if interactive {
        Sense::CLICK
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(size, sense);

    let hover = motion::ease_bool(
        ui.ctx(),
        response.id.with("hover"),
        response.hovered(),
        motion::duration::FAST,
        motion::curve::ENTER,
    );
    let radius = CornerRadius::same(tokens.radius.pill);
    let fill = mix(tokens.palette.glass.fill, tokens.palette.accent.soft, hover);
    let text_color = mix(tokens.palette.ink.secondary, tokens.palette.ink.primary, hover);

    let painter = ui.painter();
    painter.rect_filled(rect, radius, fill);
    painter.rect_stroke(
        rect,
        radius,
        Stroke::new(1.0, tokens.palette.glass.stroke),
        StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        font,
        text_color,
    );
    response
}

/// Eased `(fill, stroke)` for a standard button: inactive →hovered →active,
/// interpolated across exactly the states [`crate::theme::button_state_colors`]
/// assigns to egui's own buttons.
pub fn button_colors(tokens: &Tokens, hover: f32, press: f32) -> (Color32, Stroke) {
    let [(rest_fill, rest_stroke), (hover_fill, hover_stroke), (active_fill, active_stroke)] =
        crate::theme::button_state_colors(tokens);
    let fill = mix(mix(rest_fill, hover_fill, hover), active_fill, press);
    let stroke = mix(mix(rest_stroke, hover_stroke, hover), active_stroke, press);
    (fill, Stroke::new(1.0, stroke))
}

/// egui resolves a button's visual state only while painting it, so the ease
/// is driven by the previous frame's response (one frame ≈16 ms —invisible
/// at these durations). A repaint is requested whenever that state flips, so
/// the ease still starts even if the pointer event was the last input.
fn eased_button(
    ui: &mut Ui,
    tokens: &Tokens,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    enabled: bool,
    add: impl FnOnce(&mut Ui, Color32, Stroke) -> Response,
) -> Response {
    let id = ui.id().with(("button", id_salt));
    let (prev_hover, prev_press) = ui
        .ctx()
        .data(|d| d.get_temp::<(bool, bool)>(id).unwrap_or((false, false)));
    let hover = motion::ease_bool(
        ui.ctx(),
        id.with("hover"),
        prev_hover && enabled,
        motion::duration::FAST,
        motion::curve::ENTER,
    );
    let press = motion::ease_bool(
        ui.ctx(),
        id.with("press"),
        prev_press && enabled,
        motion::duration::FAST,
        motion::curve::ENTER,
    );
    let (fill, stroke) = button_colors(tokens, hover, press);
    let response = add(ui, fill, stroke);
    let now = (
        response.hovered() && enabled,
        response.is_pointer_button_down_on() && enabled,
    );
    if now != (prev_hover, prev_press) {
        ui.ctx().request_repaint();
    }
    ui.ctx().data_mut(|d| d.insert_temp(id, now));
    response
}

/// Standard [`egui::Button`] with eased hover / press feedback, using the
/// same palette values as the native widgets.
pub fn button(
    ui: &mut Ui,
    tokens: &Tokens,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    text: impl Into<egui::WidgetText>,
    min_size: Vec2,
    enabled: bool,
) -> Response {
    eased_button(ui, tokens, id_salt, enabled, |ui, fill, stroke| {
        ui.add_enabled(
            enabled,
            egui::Button::new(text)
                .min_size(min_size)
                .fill(fill)
                .stroke(stroke),
        )
    })
}

/// [`button`] kept inside an exact-size cell (the `ui.add_sized` layout the
/// small header buttons use).
pub fn button_sized(
    ui: &mut Ui,
    tokens: &Tokens,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    text: impl Into<egui::WidgetText>,
    size: Vec2,
) -> Response {
    eased_button(ui, tokens, id_salt, true, |ui, fill, stroke| {
        ui.add_sized(
            size,
            egui::Button::new(text).fill(fill).stroke(stroke),
        )
    })
}

/// `limit` is the block's bottom-right corner, so only the origin shifts with `dx`.
pub fn shifted_block(next: Pos2, limit: Pos2, dx: f32) -> Rect {
    Rect::from_min_max(next + Vec2::new(dx, 0.0), limit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indicator_slides_and_stretches() {
        let widths = [40.0, 80.0, 60.0];
        let offsets = [0.0, 40.0, 120.0];

        assert_eq!(indicator_span(&offsets, &widths, 0.0), (0.0, 40.0));
        assert_eq!(indicator_span(&offsets, &widths, 1.0), (40.0, 80.0));
        assert_eq!(indicator_span(&offsets, &widths, 2.0), (120.0, 60.0));

        let (mid_left, mid_width) = indicator_span(&offsets, &widths, 0.5);
        assert!((mid_left - 20.0).abs() < 0.01);
        assert!((mid_width - 60.0).abs() < 0.01);
    }

    #[test]
    fn indicator_clamps_out_of_range_slides() {
        let widths = [40.0, 80.0];
        let offsets = [0.0, 40.0];
        assert_eq!(indicator_span(&offsets, &widths, -3.0), (0.0, 40.0));
        assert_eq!(indicator_span(&offsets, &widths, 9.0), (40.0, 80.0));
        assert_eq!(indicator_span(&[], &[], 1.0), (0.0, 0.0));
    }

    #[test]
    fn mix_interpolates_channels() {
        let black = Color32::from_rgb(0, 0, 0);
        let white = Color32::from_rgb(255, 255, 255);
        assert_eq!(mix(black, white, 0.0), black);
        assert_eq!(mix(black, white, 1.0), white);
        let mid = mix(black, white, 0.5);
        assert!((mid.r() as i32 - 128).abs() <= 1);
        assert_eq!(mix(black, white, 5.0), white);
        assert_eq!(mix(black, white, -5.0), black);
    }

    #[test]
    fn shifted_block_moves_only_x() {
        let next = Pos2::new(100.0, 50.0);
        let limit = Pos2::new(800.0, 600.0);

        let right = shifted_block(next, limit, 9.0);
        assert_eq!(right.min, Pos2::new(109.0, 50.0));
        assert_eq!(right.max, limit, "bottom-right corner must not move");
        assert_eq!(right.height(), 550.0);

        let left = shifted_block(next, limit, -9.0);
        assert_eq!(left.min, Pos2::new(91.0, 50.0));
        assert_eq!(left.max, limit);

        assert_eq!(shifted_block(next, limit, 0.0).min, next);
    }

    #[test]
    fn scope_builder_places_content_at_the_shifted_rect() {
        let ctx = egui::Context::default();
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
        let mut observed: Option<(Pos2, Pos2)> = None;

        let _ = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ui| {
                let base = ui.next_widget_position();
                let rect = shifted_block(base, ui.max_rect().max, 9.0);
                ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::new(120.0, 20.0), Sense::hover());
                    observed = Some((base, r.min));
                });
            },
        );

        let (base, placed) = observed.expect("the shifted block must run");
        assert!(
            (placed.x - (base.x + 9.0)).abs() < 0.5,
            "content must land 9px right: base={base:?} placed={placed:?}"
        );
        assert!(
            (placed.y - base.y).abs() < 0.5,
            "the shake is horizontal only: base={base:?} placed={placed:?}"
        );
    }
}
