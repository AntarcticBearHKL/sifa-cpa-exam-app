//! Design tokens: colour, spacing, radius and type.
//!
//! The palette is mode-aware: a neutral dark palette and its light mirror,
//! sharing one muted slate accent. Surfaces are always translucent washes of
//! ink over the sheet (light-on-dark, or dark-on-light), so the blurred
//! desktop behind the window still shows through. Either scrim is translucent
//! too, or the backdrop it sits on would be invisible.

use std::collections::BTreeMap;

use egui::{Color32, FontFamily, FontId, TextStyle};

fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(r, g, b, a)
}

/// Which palette the app is running. The same value drives egui's
/// `Visuals`, the native window chrome and the DWM immersive-dark flag.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Mode {
    #[default]
    Dark,
    Light,
}

impl Mode {
    /// Persisted form (also used in `user.json`).
    pub fn id(self) -> &'static str {
        match self {
            Mode::Dark => "dark",
            Mode::Light => "light",
        }
    }

    /// Anything unrecognised — including the empty string a derived
    /// `UserStore::default()` leaves behind — falls back to Dark.
    pub fn from_id(id: &str) -> Self {
        match id.trim().to_ascii_lowercase().as_str() {
            "light" => Mode::Light,
            _ => Mode::Dark,
        }
    }
}

/// The backdrop. Kept slightly graduated so surfaces have something to sit on,
/// and only lightly opaque so the blurred desktop stays visible.
#[derive(Clone, Copy, Debug)]
pub struct Background {
    pub base_top: Color32,
    pub base_bottom: Color32,
    /// Soft washes painted over the gradient, in order.
    pub blobs: [Color32; 5],
    /// Flat translucent wash painted *over* the captured blurred desktop (or
    /// instead of the fallback gradient) — the desktop would otherwise be too
    /// loud for body text. Its alpha is the 透明度 setting at paint time.
    pub scrim: Color32,
}

/// Region surfaces. Dark keeps them as flat washes of light ink over the sheet
/// -- no gradient of their own, appearance varies with what is behind. Light
/// makes them opaque (the reference app's `surface` / `surface-2` / `well`), so
/// a dark desktop cannot tint the body.
#[derive(Clone, Copy, Debug)]
pub struct Glass {
    pub fill: Color32,
    pub fill_strong: Color32,
    pub fill_sunken: Color32,
    /// Opaque surface for floating layers (dropdown, tooltip): their backdrop is
    /// the app itself, so translucency would read as frosted glass.
    pub solid: Color32,
    /// Hairline edge where a surface meets the window frame. Deliberately below
    /// the fill alphas: an edge brighter than its surface reads as an outlined
    /// box, not as a sheet.
    pub stroke: Color32,
    /// Hairline between two flush regions (header / rail / content).
    pub divider: Color32,
}

/// Foreground text colours.
#[derive(Clone, Copy, Debug)]
pub struct Ink {
    pub primary: Color32,
    pub secondary: Color32,
    pub muted: Color32,
}

/// Accent and semantic colours, each with a low-alpha companion.
#[derive(Clone, Copy, Debug)]
pub struct Accent {
    pub base: Color32,
    /// Selection / active wash. Selection is expressed as this tinted fill —
    /// never as a full-strength accent hairline.
    pub soft: Color32,
    pub success: Color32,
    pub danger: Color32,
    pub warn: Color32,
}

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub background: Background,
    pub glass: Glass,
    pub ink: Ink,
    pub accent: Accent,
}

#[derive(Clone, Copy, Debug)]
pub struct Space {
    pub xs: f32,
    pub sm: f32,
    pub md: f32,
    pub lg: f32,
    pub xl: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct Radius {
    pub sm: u8,
    pub md: u8,
    pub lg: u8,
    pub pill: u8,
}

#[derive(Clone, Copy, Debug)]
pub struct Tokens {
    pub palette: Palette,
    pub space: Space,
    pub radius: Radius,
}

impl Tokens {
    /// Inner padding for a text input. egui's own default is
    /// `Margin::symmetric(4, 2)`, which leaves the text (and the hint text)
    /// almost touching the border; this uses the same spacing scale as the
    /// rows around it so an input reads as a box rather than as bare text.
    pub fn text_input_margin(&self) -> egui::Margin {
        egui::Margin::symmetric(self.space.sm as i8, self.space.xs as i8)
    }

    pub fn new(mode: Mode) -> Self {
        Self {
            palette: match mode {
                Mode::Dark => Palette::dark(),
                Mode::Light => Palette::light(),
            },
            space: Space {
                xs: 4.0,
                sm: 8.0,
                md: 12.0,
                lg: 16.0,
                xl: 24.0,
            },
            radius: Radius {
                sm: 6,
                md: 8,
                lg: 10,
                pill: 255,
            },
        }
    }

    /// Type scale. Every family stays `Proportional` so the CJK fallback
    /// registered at startup keeps applying.
    pub fn text_styles(&self) -> BTreeMap<TextStyle, FontId> {
        let prop = FontFamily::Proportional;
        let mono = FontFamily::Monospace;
        [
            (TextStyle::Heading, FontId::new(23.0, prop.clone())),
            (TextStyle::Body, FontId::new(14.5, prop.clone())),
            (TextStyle::Button, FontId::new(14.0, prop.clone())),
            (TextStyle::Small, FontId::new(12.0, prop.clone())),
            (TextStyle::Monospace, FontId::new(13.0, mono)),
            (TextStyle::Name("display".into()), FontId::new(29.0, prop.clone())),
            (TextStyle::Name("h1".into()), FontId::new(21.0, prop.clone())),
            (TextStyle::Name("h2".into()), FontId::new(17.5, prop.clone())),
            (TextStyle::Name("h3".into()), FontId::new(15.0, prop.clone())),
            (TextStyle::Name("label".into()), FontId::new(13.0, prop.clone())),
            (TextStyle::Name("caption".into()), FontId::new(11.5, prop.clone())),
            (TextStyle::Name("metric".into()), FontId::new(25.0, prop)),
        ]
        .into_iter()
        .collect()
    }
}

impl Palette {
    /// Neutral dark greys (not blue-black), ink is light.
    pub fn dark() -> Self {
        Palette {
            background: Background {
                base_top: rgb(30, 30, 33),
                base_bottom: rgb(16, 16, 18),
                blobs: [
                    rgba(255, 255, 255, 14),
                    rgba(0, 0, 0, 46),
                    rgba(255, 255, 255, 12),
                    rgba(0, 0, 0, 40),
                    rgba(255, 255, 255, 10),
                ],
                // Translucent so the window shows the desktop, but opaque enough
                // that body text keeps its contrast over a bright one. Measured
                // against `ink.primary` over a white desktop: α166 = 4.33:1
                // (fails WCAG AA), α190 = 5.96:1, α210 = 7.86:1, α228 = 10.03:1.
                // Guarded by
                // `scrim_keeps_body_text_at_aa_over_the_worst_case_desktop`.
                scrim: rgba(14, 14, 17, 228),
            },
            glass: Glass {
                fill: rgba(255, 255, 255, 18),
                fill_strong: rgba(255, 255, 255, 32),
                fill_sunken: rgba(0, 0, 0, 72),
                solid: rgb(30, 30, 33),
                // Below `fill`: edges recede under their surfaces instead of
                // reading as a wireframe.
                stroke: rgba(255, 255, 255, 14),
                divider: rgba(255, 255, 255, 24),
            },
            ink: Ink {
                primary: rgb(235, 235, 240),
                secondary: rgb(160, 160, 170),
                muted: rgb(114, 114, 124),
            },
            accent: Accent {
                base: rgb(110, 132, 168),
                soft: rgba(110, 132, 168, 64),
                success: rgb(122, 190, 152),
                danger: rgb(222, 132, 132),
                warn: rgb(214, 182, 122),
            },
        }
    }

    /// Neutral light greys, ink is dark. The mirror of `dark()`: surfaces are
    /// darkening washes, and the semantic colours are deepened so they keep
    /// their contrast on a bright sheet.
    pub fn light() -> Self {
        // Values copied from the reference app's light theme
        // (`D:\Dev\asset-library-desktop\ui\theme.slint`), including the
        // structural point that matters here: there, **only** `window-bg` is
        // translucent and every surface is opaque. That is why its light theme
        // is bright over any desktop while this one went grey -- our panels
        // were translucent washes over the sheet, so a dark desktop showed
        // through the whole body.
        //
        //   reference window-bg   #F4F5F7   -> our scrim's colour
        //   reference surface     #FFFFFF   -> our fill / solid
        //   reference surface-2   #F0F1F4   -> our fill_strong
        //   reference well        #F2F3F6   -> our fill_sunken
        //   reference divider     #E1E3E8   -> our divider
        //   reference border      #D5D8E0   -> our stroke
        //   reference text / text-dim / text-faint / accent / ok / warn / danger
        Palette {
            background: Background {
                base_top: rgb(247, 248, 250),
                base_bottom: rgb(238, 239, 243),
                blobs: [
                    rgba(255, 255, 255, 110),
                    rgba(0, 0, 0, 10),
                    rgba(255, 255, 255, 90),
                    rgba(0, 0, 0, 8),
                    rgba(255, 255, 255, 80),
                ],
                // The sheet behind the panels -- the only translucent layer, as
                // in the reference. Its alpha is the 透明度 setting at paint
                // time; the RGB is the reference's `window-bg`.
                scrim: rgba(244, 245, 247, 228),
            },
            glass: Glass {
                // Opaque, like the reference's surfaces: the desktop must not
                // tint the body. Opaque-white panels over the pale sheet are
                // what keep the light theme bright.
                fill: rgb(255, 255, 255),
                fill_strong: rgb(240, 241, 244),
                fill_sunken: rgb(242, 243, 246),
                solid: rgb(255, 255, 255),
                stroke: rgb(213, 216, 224),
                divider: rgb(225, 227, 232),
            },
            ink: Ink {
                primary: rgb(21, 22, 26),
                secondary: rgb(76, 78, 87),
                muted: rgb(99, 102, 111),
            },
            accent: Accent {
                base: rgb(10, 99, 194),
                soft: rgba(10, 99, 194, 56),
                success: rgb(12, 112, 50),
                danger: rgb(194, 47, 47),
                warn: rgb(154, 90, 10),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_is_dark_and_readable() {
        let t = Tokens::new(Mode::Dark);
        assert_eq!(t.text_styles().len(), 12);

        let base = t.palette.background.base_top;
        assert!(
            base.r() < 60 && base.g() < 60 && base.b() < 60,
            "the backdrop must be dark, got {base:?}"
        );
        let ink = t.palette.ink.primary;
        assert!(
            ink.r() > 200 && ink.g() > 200 && ink.b() > 200,
            "ink must be light on a dark backdrop, got {ink:?}"
        );

        let bg = t.palette.background;
        let span = (bg.base_top.r() as i32 - bg.base_bottom.r() as i32).abs();
        assert!(span >= 8, "the backdrop needs some depth, got {span}");
    }

    /// The inverse invariants, for the light half of the palette.
    #[test]
    fn light_palette_is_light_and_readable() {
        let t = Tokens::new(Mode::Light);

        let base = t.palette.background.base_top;
        let bottom = t.palette.background.base_bottom;
        assert!(
            base.r() > 200 && base.g() > 200 && base.b() > 200,
            "the light backdrop must be light, got {base:?}"
        );
        assert!(
            bottom.r() > 200 && bottom.g() > 200 && bottom.b() > 200,
            "and light end to end, got {bottom:?}"
        );
        let ink = t.palette.ink.primary;
        assert!(
            ink.r() < 80 && ink.g() < 80 && ink.b() < 80,
            "ink must be dark on a light backdrop, got {ink:?}"
        );

        let span = (base.r() as i32 - bottom.r() as i32).abs();
        assert!(span >= 8, "the backdrop needs some depth, got {span}");

        // The scrim keeps the transparent window legible over an arbitrary
        // desktop, and must stay translucent or the desktop stops showing
        // through. Read the channels un-premultiplied: `Color32` stores them
        // scaled by alpha, so a raw `.r()` drops with the alpha it is meant to
        // be checked against.
        let [r, g, b, a] = t.palette.background.scrim.to_srgba_unmultiplied();
        assert!(
            r > 200 && g > 200 && b > 200 && (150..=240).contains(&a),
            "the light scrim must be light and translucent, got {r},{g},{b},{a}"
        );

        // The dark scrim carries the same invariant upside-down.
        let [r, g, b, a] = Tokens::new(Mode::Dark)
            .palette
            .background
            .scrim
            .to_srgba_unmultiplied();
        assert!(
            r < 60 && g < 60 && b < 60 && (150..=240).contains(&a),
            "the dark scrim must stay dark and translucent, got {r},{g},{b},{a}"
        );
    }

    /// The scrim is the only thing between the app's text and an arbitrary
    /// desktop, so it has to hold body text at WCAG AA over the worst case for
    /// its mode: a white desktop behind dark mode, a black one behind light.
    #[test]
    fn scrim_keeps_body_text_at_aa_over_the_worst_case_desktop() {
        fn linear(c: f32) -> f32 {
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        }

        fn luminance(c: Color32) -> f32 {
            let [r, g, b, _] = c.to_srgba_unmultiplied();
            let channel = |v: u8| linear(v as f32 / 255.0);
            0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
        }

        fn contrast(a: Color32, b: Color32) -> f32 {
            let (x, y) = (luminance(a), luminance(b));
            let (hi, lo) = if x > y { (x, y) } else { (y, x) };
            (hi + 0.05) / (lo + 0.05)
        }

        // `src` composited over an opaque `dst`, both straight-alpha.
        fn over(src: Color32, dst: Color32) -> Color32 {
            let [sr, sg, sb, sa] = src.to_srgba_unmultiplied();
            let [dr, dg, db, _] = dst.to_srgba_unmultiplied();
            let a = sa as f32 / 255.0;
            let mix = |s: u8, d: u8| (s as f32 * a + d as f32 * (1.0 - a)).round() as u8;
            Color32::from_rgb(mix(sr, dr), mix(sg, dg), mix(sb, db))
        }

        for (mode, desktop, name) in [
            (Mode::Dark, Color32::WHITE, "white"),
            (Mode::Light, Color32::BLACK, "black"),
        ] {
            let t = Tokens::new(mode);
            let sheet = over(t.palette.background.scrim, desktop);
            let bg = over(t.palette.glass.fill, sheet);
            let ratio = contrast(t.palette.ink.primary, bg);
            assert!(
                ratio >= 4.5,
                "body text must stay at WCAG AA over a {name} desktop ({mode:?}), got {ratio:.2}:1"
            );
        }
    }

    #[test]
    fn accent_is_muted_not_saturated() {
        // Dark keeps the muted accent this app has always had. The light
        // accent is the reference app's real blue (`#0A63C2`), copied along
        // with the rest of its light palette -- a saturated accent is
        // deliberate there, so only dark is held to "near neutral".
        let dark = Tokens::new(Mode::Dark).palette.accent.base;
        let max = dark.r().max(dark.g()).max(dark.b()) as i32;
        let min = dark.r().min(dark.g()).min(dark.b()) as i32;
        assert!(
            max - min < 70,
            "the dark accent must stay close to neutral, got span {}",
            max - min
        );

        let light = Tokens::new(Mode::Light).palette.accent.base;
        assert!(
            light.b() > light.r() + 80,
            "the light accent is the reference's blue, got {light:?}"
        );
    }

    #[test]
    fn surfaces_are_translucent_but_visible() {
        // Dark keeps translucent washes over the sheet. Light copies the
        // reference app, where every surface is opaque and only the window
        // background is translucent -- that is what keeps a light theme bright
        // over a dark desktop instead of grey.
        let g = Tokens::new(Mode::Dark).palette.glass;
        assert!(g.fill.a() > 8, "a dark surface must be visible against the backdrop");
        assert!(g.fill.a() < 70, "but translucent, not an opaque slab");
        assert!(
            g.fill_strong.a() > g.fill.a(),
            "the raised surface is stronger"
        );
        assert!(g.divider.a() > 0 && g.divider.a() < 80, "a hairline, not a line");

        let l = Tokens::new(Mode::Light).palette.glass;
        assert_eq!(l.fill.a(), 255, "light surfaces are opaque");
        assert_eq!(l.solid.a(), 255);
        assert_eq!(l.fill_strong.a(), 255);
        assert_eq!(l.fill_sunken.a(), 255);
        assert_eq!(l.divider.a(), 255, "opaque hairline colours, as in the reference");
        assert_eq!(l.stroke.a(), 255);
        assert!(l.fill.r() > 240, "and light, got {l:?}");

        // Floating layers (popups / menus) are opaque in both modes -- a
        // translucent popup over the app reads as frosted glass, not a raised
        // surface.
        for mode in [Mode::Dark, Mode::Light] {
            let g = Tokens::new(mode).palette.glass;
            assert_eq!(g.solid.a(), 255, "floating layers must be opaque ({mode:?})");
            let in_family = match mode {
                Mode::Dark => g.solid.r() < 60 && g.solid.g() < 60 && g.solid.b() < 60,
                Mode::Light => g.solid.r() > 200 && g.solid.g() > 200 && g.solid.b() > 200,
            };
            assert!(
                in_family,
                "and must stay in the mode's surface family, got {:?} ({mode:?})",
                g.solid
            );
        }
    }

    /// The regression this rework exists for: a full-strength accent hairline
    /// on a near-black sheet read as a neon wireframe. Edges must recede below
    /// their surfaces, and selection washes must stay tints, never slabs.
    #[test]
    fn edges_recede_and_selection_is_a_tint() {
        for mode in [Mode::Dark, Mode::Light] {
            let p = Tokens::new(mode).palette;
            match mode {
                Mode::Dark => assert!(
                    p.glass.stroke.a() < p.glass.fill.a(),
                    "on dark surfaces the edge must recede below the fill ({mode:?})"
                ),
                Mode::Light => assert!(
                    p.glass.stroke.a() <= p.glass.fill_sunken.a(),
                    "on light surfaces the edge stays a soft grey ({mode:?})"
                ),
            }
            assert!(
                p.accent.soft.a() >= 40 && p.accent.soft.a() <= 120,
                "the selection wash must be visible but translucent ({mode:?}), got {}",
                p.accent.soft.a()
            );
        }
    }

    #[test]
    fn mode_ids_round_trip() {
        assert_eq!(Mode::default(), Mode::Dark);
        assert_eq!(Mode::Dark.id(), "dark");
        assert_eq!(Mode::Light.id(), "light");
        assert_eq!(Mode::from_id("light"), Mode::Light);
        assert_eq!(Mode::from_id(" LIGHT "), Mode::Light);
        assert_eq!(Mode::from_id("dark"), Mode::Dark);
        assert_eq!(Mode::from_id(""), Mode::Dark);
        assert_eq!(Mode::from_id("sepia"), Mode::Dark);
    }
}
