//! Theme installation: maps [`Tokens`] onto egui's `Style`/`Visuals`.

pub mod tokens;

pub use tokens::{Mode, Tokens};

use egui::style::{WidgetVisuals, Widgets};
use egui::{Color32, CornerRadius, Shadow, Stroke, Style, Visuals};

pub fn install(ctx: &egui::Context, mode: Mode) -> Tokens {
    let tokens = Tokens::new(mode);
    apply(ctx, &tokens, mode);
    tokens
}

/// `all_styles_mut` covers both style slots, so app colours never depend on
/// which slot the OS preference happens to select. The preference is still set
/// explicitly: egui syncs it to the native window, and the mode drives the
/// whole palette (dark ink-on-dark, or the light mirror).
pub fn apply(ctx: &egui::Context, tokens: &Tokens, mode: Mode) {
    ctx.set_theme(match mode {
        Mode::Dark => egui::ThemePreference::Dark,
        Mode::Light => egui::ThemePreference::Light,
    });
    ctx.all_styles_mut(|style| apply_style(style, tokens, mode));
}

fn apply_style(style: &mut Style, t: &Tokens, mode: Mode) {
    style.visuals = visuals(t, mode);
    style.text_styles = t.text_styles();
    // The few native egui surfaces that still self-time (menu / tooltip
    // fade-in, area age) read this. It is the fastest tier of the shared
    // vocabulary, so every surface answers at the same speed.
    style.animation_time = crate::anim::motion::duration::FAST;

    let sp = &mut style.spacing;
    sp.item_spacing = egui::vec2(t.space.sm, t.space.sm);
    sp.button_padding = egui::vec2(t.space.md, t.space.sm);
    sp.interact_size.y = 30.0;
    sp.combo_width = 200.0;
    sp.indent = t.space.lg;
    sp.slider_width = 140.0;
    sp.text_edit_width = 220.0;

    // GLOBAL scrollbar hide. egui 0.36.2's `ScrollStyle` exposes no colour
    // fields (no `handle_color` / `bar_background_color` / hovered / dragged),
    // only width and opacity knobs — so a transparent-but-still-draggable bar
    // is not expressible here. Fall back to zero widths: the bar paints
    // nothing and allocates no space. Every opacity is zeroed too, so no state
    // (dormant / active / interact) can ever paint a handle or track. Wheel
    // scrolling keeps working; click-drag on the bar does not (a zero-width
    // bar has no grab area). The values look arbitrary without this note.
    let sc = &mut sp.scroll;
    sc.floating = true;
    sc.bar_width = 0.0;
    sc.floating_width = 0.0;
    sc.handle_min_length = 30.0;
    sc.bar_inner_margin = 3.0;
    sc.bar_outer_margin = 2.0;
    sc.dormant_background_opacity = 0.0;
    sc.active_background_opacity = 0.0;
    sc.interact_background_opacity = 0.0;
    sc.dormant_handle_opacity = 0.0;
    sc.active_handle_opacity = 0.0;
    sc.interact_handle_opacity = 0.0;
}

/// `(fill, stroke)` for the inactive / hovered / active button states —
/// exactly the values [`visuals`] assigns to egui's own widgets. Painted
/// widgets that ease between these states read them from here, so the eased
/// buttons can never drift from the native ones.
pub fn button_state_colors(t: &Tokens) -> [(Color32, Color32); 3] {
    let p = &t.palette;
    [
        (p.glass.fill, p.glass.stroke),
        (p.glass.fill_strong, p.accent.soft),
        (p.accent.soft, p.accent.soft),
    ]
}

/// `panel_fill` is transparent so the aurora layer painted behind every panel
/// stays visible through the glass.
fn visuals(t: &Tokens, mode: Mode) -> Visuals {
    let p = &t.palette;
    let r = &t.radius;
    let mut v = match mode {
        Mode::Dark => Visuals::dark(),
        Mode::Light => Visuals::light(),
    };
    v.dark_mode = mode == Mode::Dark;
    v.panel_fill = egui::Color32::TRANSPARENT;
    // Floating layers (dropdown, tooltips) join the flush design: opaque fill,
    // hairline edge, no shadow — nothing in this UI floats.
    v.window_fill = p.glass.solid;
    v.window_stroke = Stroke::new(1.0, p.glass.stroke);
    v.window_corner_radius = CornerRadius::same(r.md);
    v.window_shadow = Shadow::NONE;
    v.window_highlight_topmost = false;
    v.menu_corner_radius = CornerRadius::same(r.md);
    v.popup_shadow = Shadow::NONE;
    v.faint_bg_color = p.glass.fill;
    v.extreme_bg_color = p.glass.fill_sunken;
    v.text_edit_bg_color = Some(p.glass.fill_sunken);
    v.code_bg_color = p.glass.fill_sunken;
    v.hyperlink_color = p.accent.base;
    v.warn_fg_color = p.accent.warn;
    v.error_fg_color = p.accent.danger;
    v.weak_text_color = Some(p.ink.secondary);
    v.override_text_color = None;
    v.striped = false;
    v.button_frame = true;
    v.collapsing_header_frame = false;
    v.indent_has_left_vline = false;
    v.handle_shape = egui::style::HandleShape::Circle;

    v.selection = egui::style::Selection {
        bg_fill: p.accent.soft,
        stroke: Stroke::new(1.0, p.accent.base),
    };

    let ink_fg = Stroke::new(1.0, p.ink.primary);
    let soft_fg = Stroke::new(1.0, p.ink.secondary);

    let [(rest_fill, rest_stroke), (hover_fill, hover_stroke), (active_fill, active_stroke)] =
        button_state_colors(t);

    v.widgets = Widgets {
        noninteractive: widget(
            egui::Color32::TRANSPARENT,
            egui::Color32::TRANSPARENT,
            Stroke::new(1.0, p.glass.stroke),
            soft_fg,
            r.sm,
            0.0,
        ),
        inactive: widget(rest_fill, rest_fill, Stroke::new(1.0, rest_stroke), ink_fg, r.md, 0.0),
        hovered: widget(
            hover_fill,
            hover_fill,
            Stroke::new(1.0, hover_stroke),
            ink_fg,
            r.md,
            0.0,
        ),
        // Pressed (and focus-held) widgets: a tinted fill, with the accent only
        // as a low-alpha edge — never a full-strength outline.
        active: widget(
            active_fill,
            active_fill,
            Stroke::new(1.0, active_stroke),
            ink_fg,
            r.md,
            0.0,
        ),
        open: widget(
            p.glass.fill_strong,
            p.glass.fill_strong,
            Stroke::new(1.0, p.glass.stroke),
            ink_fg,
            r.md,
            0.0,
        ),
    };

    v
}

fn widget(
    bg_fill: egui::Color32,
    weak_bg_fill: egui::Color32,
    bg_stroke: Stroke,
    fg_stroke: Stroke,
    radius: u8,
    expansion: f32,
) -> WidgetVisuals {
    WidgetVisuals {
        bg_fill,
        weak_bg_fill,
        bg_stroke,
        fg_stroke,
        corner_radius: CornerRadius::same(radius),
        expansion,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `Frame::popup` / `Frame::menu` — the frame behind every dropdown and
    /// tooltip — read exactly these fields, so this is what keeps them flush.
    /// True in both modes: the popup surface follows the palette.
    #[test]
    fn popups_are_flush_not_floating_glass() {
        let ctx = egui::Context::default();

        for mode in [Mode::Dark, Mode::Light] {
            let t = install(&ctx, mode);
            let dark = ctx.style_of(egui::Theme::Dark);
            let light = ctx.style_of(egui::Theme::Light);

            for v in [&dark.visuals, &light.visuals] {
                assert_eq!(
                    v.window_fill, t.palette.glass.solid,
                    "the popup surface must follow the active palette ({mode:?})"
                );
                assert_eq!(
                    v.window_fill.a(),
                    255,
                    "a popup's backdrop is the app itself, not the desktop ({mode:?})"
                );
                assert_eq!(
                    v.popup_shadow,
                    Shadow::NONE,
                    "nothing in this UI floats ({mode:?})"
                );
                assert_eq!(v.window_shadow, Shadow::NONE, "{mode:?}");
                assert!(
                    !v.window_highlight_topmost,
                    "the design has no top light band ({mode:?})"
                );
                assert_eq!(
                    v.menu_corner_radius,
                    CornerRadius::same(t.radius.md),
                    "{mode:?}"
                );
            }
        }
    }

    /// The active slot must never paint a full-strength accent hairline.
    #[test]
    fn pressed_widgets_get_a_tint_not_a_neon_outline() {
        for mode in [Mode::Dark, Mode::Light] {
            let ctx = egui::Context::default();
            let t = install(&ctx, mode);
            let v = ctx.style_of(match mode {
                Mode::Dark => egui::Theme::Dark,
                Mode::Light => egui::Theme::Light,
            });

            assert_eq!(v.visuals.widgets.active.bg_fill, t.palette.accent.soft);
            assert_eq!(v.visuals.widgets.active.bg_stroke.color, t.palette.accent.soft);
            assert!(
                v.visuals.widgets.active.bg_stroke.color.a() < 120,
                "the active edge must stay a low-alpha tint ({mode:?})"
            );
            assert_eq!(v.visuals.dark_mode, mode == Mode::Dark);
        }
    }

    /// Scrollbars are hidden globally — in every scrollable area, both theme
    /// slots: zero width (nothing painted, no space allocated) and every
    /// handle/track opacity zeroed, so no dormant/active/interact state can
    /// ever paint a bar.
    #[test]
    fn scrollbars_are_hidden_everywhere() {
        let ctx = egui::Context::default();

        for mode in [Mode::Dark, Mode::Light] {
            install(&ctx, mode);
            let dark = ctx.style_of(egui::Theme::Dark);
            let light = ctx.style_of(egui::Theme::Light);

            for style in [&dark, &light] {
                let sc = &style.spacing.scroll;
                assert_eq!(sc.bar_width, 0.0, "{mode:?}");
                assert_eq!(sc.floating_width, 0.0, "{mode:?}");
                assert_eq!(sc.dormant_handle_opacity, 0.0, "{mode:?}");
                assert_eq!(sc.active_handle_opacity, 0.0, "{mode:?}");
                assert_eq!(sc.interact_handle_opacity, 0.0, "{mode:?}");
                assert_eq!(sc.dormant_background_opacity, 0.0, "{mode:?}");
                assert_eq!(sc.active_background_opacity, 0.0, "{mode:?}");
                assert_eq!(sc.interact_background_opacity, 0.0, "{mode:?}");
            }
        }
    }
}
