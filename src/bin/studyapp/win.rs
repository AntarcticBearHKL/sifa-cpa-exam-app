//! Windows window chrome for a borderless, transparent window: a real DWM
//! acrylic backdrop plus the drag / resize / minimize / close affordances that
//! the missing title bar would otherwise provide.

use egui::{CornerRadius, Pos2, Rect, ResizeDirection, Response, Sense, Ui};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use crate::anim::motion;
use crate::theme::{Mode, Tokens};

const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
const DWMWA_SYSTEMBACKDROP_TYPE: u32 = 38;

const DWMWCP_ROUND: i32 = 2;
/// Acrylic — blurs whatever is behind the window.
const DWMSBT_TRANSIENTWINDOW: i32 = 3;

#[link(name = "dwmapi")]
extern "system" {
    fn DwmSetWindowAttribute(hwnd: isize, attr: u32, value: *const u8, size: u32) -> i32;
    fn DwmGetWindowAttribute(hwnd: isize, attr: u32, value: *mut u8, size: u32) -> i32;
}

fn hwnd_of(handle: impl HasWindowHandle) -> Option<isize> {
    match handle.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(h.hwnd.get()),
        _ => None,
    }
}

fn set_attr(hwnd: isize, attr: u32, value: i32) -> i32 {
    unsafe { DwmSetWindowAttribute(hwnd, attr, &value as *const i32 as *const u8, 4) }
}

fn get_attr(hwnd: isize, attr: u32) -> Option<i32> {
    let mut out: i32 = 0;
    let hr = unsafe { DwmGetWindowAttribute(hwnd, attr, &mut out as *mut i32 as *mut u8, 4) };
    (hr >= 0).then_some(out)
}

/// The raw Win32 handle of the host window, when the platform is Win32. Kept
/// so runtime code can flip DWM attributes without a `CreationContext`.
pub fn window_hwnd(handle: impl HasWindowHandle) -> Option<isize> {
    hwnd_of(handle)
}

/// DWM's immersive-dark flag for the non-client frame: `1` for Dark, `0` for
/// Light, so the system-drawn parts of the window follow the app theme.
pub fn set_immersive_dark(hwnd: isize, dark: bool) {
    let _ = set_attr(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, i32::from(dark));
}

/// Ask DWM for an acrylic backdrop. `Ok(())` means the compositor accepted it,
/// i.e. the window really shows the desktop behind it, blurred.
pub fn apply_acrylic(cc: &eframe::CreationContext<'_>, mode: Mode) -> Result<(), String> {
    let hwnd = hwnd_of(cc).ok_or_else(|| "no Win32 window handle".to_string())?;

    set_immersive_dark(hwnd, mode == Mode::Dark);
    let _ = set_attr(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND);

    let hr = set_attr(hwnd, DWMWA_SYSTEMBACKDROP_TYPE, DWMSBT_TRANSIENTWINDOW);
    if hr < 0 {
        return Err(format!("backdrop HRESULT 0x{:08X}", hr as u32));
    }
    Ok(())
}

/// Human-readable readback, used by `--probe` to prove the backdrop took effect
/// without anyone touching the window.
pub fn probe(cc: &eframe::CreationContext<'_>) -> String {
    let Some(hwnd) = hwnd_of(cc) else {
        return "no Win32 window handle".to_string();
    };
    let show = |v: Option<i32>| v.map_or_else(|| "read-failed".to_string(), |n| n.to_string());
    format!(
        "backdrop={} (3=acrylic) dark={} corner={} (2=round)",
        show(get_attr(hwnd, DWMWA_SYSTEMBACKDROP_TYPE)),
        show(get_attr(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE)),
        show(get_attr(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE)),
    )
}

/// Which window edge (if any) `pos` is pressing on.
fn edge_direction(r: Rect, pos: Pos2) -> Option<ResizeDirection> {
    const EDGE: f32 = 8.0;
    const CORNER: f32 = 16.0;
    if !r.contains(pos) {
        return None;
    }
    let left = pos.x <= r.left() + EDGE;
    let right = pos.x >= r.right() - EDGE;
    let top = pos.y <= r.top() + EDGE;
    let bottom = pos.y >= r.bottom() - EDGE;

    if left && pos.y <= r.top() + CORNER {
        Some(ResizeDirection::NorthWest)
    } else if right && pos.y <= r.top() + CORNER {
        Some(ResizeDirection::NorthEast)
    } else if left && pos.y >= r.bottom() - CORNER {
        Some(ResizeDirection::SouthWest)
    } else if right && pos.y >= r.bottom() - CORNER {
        Some(ResizeDirection::SouthEast)
    } else if left {
        Some(ResizeDirection::West)
    } else if right {
        Some(ResizeDirection::East)
    } else if top {
        Some(ResizeDirection::North)
    } else if bottom {
        Some(ResizeDirection::South)
    } else {
        None
    }
}

fn resize_cursor(dir: ResizeDirection) -> egui::CursorIcon {
    use egui::CursorIcon as C;
    match dir {
        ResizeDirection::North | ResizeDirection::South => C::ResizeVertical,
        ResizeDirection::East | ResizeDirection::West => C::ResizeHorizontal,
        ResizeDirection::NorthWest | ResizeDirection::SouthEast => C::ResizeNwSe,
        ResizeDirection::NorthEast | ResizeDirection::SouthWest => C::ResizeNeSw,
    }
}

/// Window-level mouse behaviour, called once per frame.
///
/// * Hovering a window edge shows the matching resize cursor. A borderless
///   window gets no cursor affordance from the OS, so without this there is no
///   way to tell where the resize zone is.
/// * Pressing an edge starts a native resize.
/// * Pressing anywhere egui is NOT using the pointer drags the whole window —
///   that is how the app moves without a title bar, and it works over any
///   non-interactive area, not just a header strip.
///
/// Must NOT use an `egui::Area`: a full-window foreground area sits above every
/// panel in egui's hit-testing order and swallows every click in the app.
pub fn chrome(ctx: &egui::Context) {
    let Some(pos) = ctx.input(|i| i.pointer.latest_pos()) else {
        return;
    };
    let dir = edge_direction(ctx.content_rect(), pos);
    if let Some(dir) = dir {
        ctx.set_cursor_icon(resize_cursor(dir));
    }

    if !ctx.input(|i| i.pointer.primary_pressed()) {
        return;
    }
    if let Some(dir) = dir {
        ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(dir));
        return;
    }
    if !ctx.egui_wants_pointer_input() {
        ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ChromeButton {
    Minimize,
    Close,
}

/// Minimize / close drawn with lines, so no glyph is needed (the CJK font has
/// no window-control glyphs and would render tofu).
pub fn chrome_button(ui: &mut Ui, tokens: &Tokens, kind: ChromeButton) -> Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(34.0, 26.0), Sense::CLICK);
    let hover = motion::ease_bool(
        ui.ctx(),
        resp.id.with("hover"),
        resp.hovered(),
        motion::duration::FAST,
        motion::curve::ENTER,
    );
    let painter = ui.painter();
    if hover > 0.001 {
        let bg = match kind {
            ChromeButton::Close => tokens.palette.accent.danger,
            ChromeButton::Minimize => tokens.palette.glass.fill_strong,
        };
        painter.rect_filled(
            rect,
            CornerRadius::same(tokens.radius.sm),
            bg.gamma_multiply(hover),
        );
    }
    let c = rect.center();
    let stroke = egui::Stroke::new(1.4, tokens.palette.ink.secondary);
    match kind {
        ChromeButton::Minimize => {
            painter.line_segment(
                [egui::pos2(c.x - 5.0, c.y), egui::pos2(c.x + 5.0, c.y)],
                stroke,
            );
        }
        ChromeButton::Close => {
            for (sx, sy) in [(-1.0, -1.0), (-1.0, 1.0)] {
                painter.line_segment(
                    [
                        egui::pos2(c.x + 4.5 * sx, c.y + 4.5 * sy),
                        egui::pos2(c.x - 4.5 * sx, c.y - 4.5 * sy),
                    ],
                    stroke,
                );
            }
        }
    }
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    const SCREEN: [f32; 2] = [800.0, 600.0];
    fn screen() -> Rect {
        Rect::from_min_size(Pos2::ZERO, egui::vec2(SCREEN[0], SCREEN[1]))
    }

    /// Press the primary button at `pos`, optionally with a clickable widget
    /// under it, and report whether a window drag was requested.
    fn press_starts_drag(pos: Pos2, with_widget: bool) -> bool {
        let ctx = egui::Context::default();
        let raw = |events| egui::RawInput {
            screen_rect: Some(screen()),
            events,
            ..Default::default()
        };

        // Frame 1 establishes the pointer position, frame 2 is the press.
        let _ = ctx.run_ui(raw(vec![egui::Event::PointerMoved(pos)]), |ui| {
            if with_widget {
                let rect = Rect::from_center_size(pos, egui::vec2(120.0, 120.0));
                ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                    ui.allocate_exact_size(egui::vec2(120.0, 120.0), Sense::CLICK);
                });
            }
            chrome(ui.ctx());
        });

        let out = ctx.run_ui(
            raw(vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::default(),
            }]),
            |ui| {
                if with_widget {
                    let rect = Rect::from_center_size(pos, egui::vec2(120.0, 120.0));
                    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                        ui.allocate_exact_size(egui::vec2(120.0, 120.0), Sense::CLICK);
                    });
                }
                chrome(ui.ctx());
            },
        );

        out.viewport_output
            .values()
            .flat_map(|v| v.commands.iter())
            .any(|c| matches!(c, egui::ViewportCommand::StartDrag))
    }

    #[test]
    fn pressing_a_widget_does_not_drag_the_window() {
        assert!(
            !press_starts_drag(egui::pos2(400.0, 300.0), true),
            "pressing an interactive widget must be handled by the widget"
        );
    }

    #[test]
    fn hovering_a_window_edge_shows_the_matching_resize_cursor() {
        let cursor_at = |pos| {
            let ctx = egui::Context::default();
            let out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(screen()),
                    events: vec![egui::Event::PointerMoved(pos)],
                    ..Default::default()
                },
                |ui| chrome(ui.ctx()),
            );
            out.platform_output.cursor_icon
        };

        assert_eq!(
            cursor_at(egui::pos2(3.0, 300.0)),
            egui::CursorIcon::ResizeHorizontal,
            "left edge"
        );
        assert_eq!(
            cursor_at(egui::pos2(797.0, 300.0)),
            egui::CursorIcon::ResizeHorizontal,
            "right edge"
        );
        assert_eq!(
            cursor_at(egui::pos2(400.0, 3.0)),
            egui::CursorIcon::ResizeVertical,
            "top edge"
        );
        assert_eq!(
            cursor_at(egui::pos2(3.0, 3.0)),
            egui::CursorIcon::ResizeNwSe,
            "top-left corner"
        );
        assert_eq!(
            cursor_at(egui::pos2(797.0, 597.0)),
            egui::CursorIcon::ResizeNwSe,
            "bottom-right corner"
        );
        assert_eq!(
            cursor_at(egui::pos2(400.0, 300.0)),
            egui::CursorIcon::Default,
            "the middle is not a resize zone"
        );
    }

    /// Press the primary button at `pos` and report the viewport commands that
    /// were requested that frame.
    fn press_commands(pos: Pos2) -> Vec<egui::ViewportCommand> {
        let ctx = egui::Context::default();
        let raw = |events| egui::RawInput {
            screen_rect: Some(screen()),
            events,
            ..Default::default()
        };
        let _ = ctx.run_ui(raw(vec![egui::Event::PointerMoved(pos)]), |ui| {
            chrome(ui.ctx());
        });
        let out = ctx.run_ui(
            raw(vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::default(),
            }]),
            |ui| chrome(ui.ctx()),
        );
        out.viewport_output
            .values()
            .flat_map(|v| v.commands.iter().cloned())
            .collect()
    }

    fn resizes(pos: Pos2, want: ResizeDirection) -> bool {
        press_commands(pos).iter().any(|c| match c {
            egui::ViewportCommand::BeginResize(d) => *d == want,
            _ => false,
        })
    }

    fn drags(pos: Pos2) -> bool {
        press_commands(pos)
            .iter()
            .any(|c| matches!(c, egui::ViewportCommand::StartDrag))
    }

    #[test]
    fn pressing_empty_space_starts_a_window_drag() {
        assert!(
            drags(egui::pos2(400.0, 300.0)),
            "pressing a non-interactive area must drag the whole window"
        );
    }

    #[test]
    fn every_edge_and_corner_resizes_in_its_own_direction() {
        assert!(resizes(egui::pos2(3.0, 300.0), ResizeDirection::West), "left");
        assert!(resizes(egui::pos2(797.0, 300.0), ResizeDirection::East), "right");
        assert!(resizes(egui::pos2(400.0, 3.0), ResizeDirection::North), "top");
        assert!(resizes(egui::pos2(400.0, 597.0), ResizeDirection::South), "bottom");
        assert!(resizes(egui::pos2(3.0, 3.0), ResizeDirection::NorthWest), "top-left");
        assert!(resizes(egui::pos2(797.0, 3.0), ResizeDirection::NorthEast), "top-right");
        assert!(resizes(egui::pos2(3.0, 597.0), ResizeDirection::SouthWest), "bottom-left");
        assert!(resizes(egui::pos2(797.0, 597.0), ResizeDirection::SouthEast), "bottom-right");
    }

    #[test]
    fn pressing_a_window_edge_resizes_instead_of_dragging() {
        let commands = press_commands(egui::pos2(3.0, 300.0));
        assert!(
            commands
                .iter()
                .any(|c| matches!(c, egui::ViewportCommand::BeginResize(_))),
            "an edge press must start a resize"
        );
        assert!(
            !commands
                .iter()
                .any(|c| matches!(c, egui::ViewportCommand::StartDrag)),
            "an edge press must not also drag"
        );
    }
}
