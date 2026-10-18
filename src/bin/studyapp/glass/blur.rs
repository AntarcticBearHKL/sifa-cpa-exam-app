//! Capture the desktop behind the window and Gaussian-blur it in-app.
//!
//! The OS blur the app used to rely on (`SetWindowCompositionAttribute`'s accent
//! policy) has **no radius parameter**, so it can never be made user
//! controllable. It was removed. Instead the app now screenshots the primary
//! monitor itself, blurs the shot and draws it under the scrim:
//!
//! 1. [`capture`] — `BitBlt` the screen DC into a top-down 32-bit DIB. The
//!    window is kept out of its own screenshot for the duration of the blit,
//!    by one of two paths (see [`Hide`]):
//!    * `SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE)` — Windows 10
//!      2004+. The compositor leaves the window out of every capture while the
//!      call is in effect, and the user still sees it normally, so **nothing
//!      ever changes on screen** and there is no transparent frame for DWM to
//!      present. Cleared back to `WDA_NONE` right after the blit.
//!    * Fallback, when that call fails (older Windows): the window is made
//!      fully transparent (`LWA_ALPHA`, alpha 0) for the blit and restored to
//!      alpha 255 immediately afterwards — hide → blit → restore stays well
//!      under one vsync, so DWM normally never presents the transparent state.
//! 2. [`downscale`] — repeated 2x2 box halvings. This is both a cheap pre-blur
//!    and the reason the Gaussian stays affordable: it runs on 1/2^h of the
//!    pixels.
//! 3. [`gaussian`] — a real separable Gaussian (horizontal then vertical
//!    convolution, clamped at the edges).
//! 4. [`to_color_image`] — back to an `egui::ColorImage`, uploaded with
//!    [`egui::TextureOptions::LINEAR`] so the small image is bilinearly
//!    upscaled when it is drawn.
//!
//! The capture is the **primary monitor** only (`GetDC(NULL)`'s DC covers the
//! virtual screen, but the shot is taken at its origin, which is the primary
//! monitor's top-left). A window dragged to a secondary monitor therefore
//! clamps to the edge of the captured image; the painted fallback is never
//! used for that, and the next capture re-registers as soon as it comes back.

use egui::{Color32, ColorImage};

/// One screenshot of the primary monitor, top-down 32-bit BGRA.
pub struct Shot {
    /// Top-left of the capture in physical screen pixels.
    pub origin: (i32, i32),
    pub width: usize,
    pub height: usize,
    /// `width * height * 4` bytes, BGRA order, rows top-down.
    pub bgra: Vec<u8>,
}

/// Map the 模糊度 setting (`0..=100`) onto the blur pipeline: how many 2x2
/// halvings the capture gets first, and the Gaussian sigma applied afterwards
/// (in the halved image's pixels).
///
/// One documented place, so the slider, the painter and the tests can never
/// disagree. `0` means "no blur at all" — the capture is shown sharp. Larger
/// values raise both the number of halvings (`0..=3`, each one a box blur and
/// an 2x upscale later) and the full-resolution-equivalent sigma (`0.6..6.0`
/// px), so the result gets visibly softer from every direction at once.
pub fn params(blur: u8) -> (u8, f32) {
    let b = blur.min(100);
    if b == 0 {
        return (0, 0.0);
    }
    let halvings = ((b as u32 - 1) / 25).min(3) as u8;
    let sigma_full = 0.6 + (b as f32 / 100.0) * 5.4;
    (halvings, sigma_full / (1u32 << halvings) as f32)
}

// ---------------------------------------------------------------------------
// Capture (Win32)
// ---------------------------------------------------------------------------

#[repr(C)]
struct BitmapInfoHeader {
    size: u32,
    width: i32,
    height: i32,
    planes: u16,
    bit_count: u16,
    compression: u32,
    size_image: u32,
    x_pels_per_meter: i32,
    y_pels_per_meter: i32,
    clr_used: u32,
    clr_important: u32,
}

#[repr(C)]
struct BitmapInfo {
    header: BitmapInfoHeader,
    colors: [u32; 3],
}

#[link(name = "gdi32")]
extern "system" {
    fn BitBlt(
        dst: isize,
        x: i32,
        y: i32,
        w: i32,
        h: i32,
        src: isize,
        src_x: i32,
        src_y: i32,
        rop: u32,
    ) -> i32;
    fn CreateCompatibleDC(dc: isize) -> isize;
    fn CreateDIBSection(
        dc: isize,
        bmi: *const BitmapInfo,
        usage: u32,
        bits: *mut *mut core::ffi::c_void,
        section: isize,
        offset: u32,
    ) -> isize;
    fn SelectObject(dc: isize, obj: isize) -> isize;
    fn DeleteObject(obj: isize) -> i32;
    fn DeleteDC(dc: isize) -> i32;
    fn GetDC(hwnd: isize) -> isize;
    fn ReleaseDC(hwnd: isize, dc: isize) -> i32;
}

#[link(name = "user32")]
extern "system" {
    fn GetSystemMetrics(index: i32) -> i32;
    fn SetLayeredWindowAttributes(hwnd: isize, key: u32, alpha: u8, flags: u32) -> i32;
    fn GetWindowLongPtrW(hwnd: isize, index: i32) -> isize;
    fn SetWindowLongPtrW(hwnd: isize, index: i32, value: isize) -> isize;
    fn SetWindowDisplayAffinity(hwnd: isize, affinity: u32) -> i32;
}

const SM_CXSCREEN: i32 = 0;
const SM_CYSCREEN: i32 = 1;
const SRCCOPY: u32 = 0x00CC_0020;
const DIB_RGB_COLORS: u32 = 0;
const BI_RGB: u32 = 0;
const LWA_ALPHA: u32 = 0x0000_0002;
const GWL_EXSTYLE: i32 = -20;
const WS_EX_LAYERED: isize = 0x0008_0000;

/// `WDA_NONE` — the window is captured like any other again.
const WDA_NONE: u32 = 0x0000_0000;
/// `WDA_EXCLUDEFROMCAPTURE` (Windows 10 2004+): the compositor draws the window
/// normally for the user but leaves it out of screen captures.
const WDA_EXCLUDEFROMCAPTURE: u32 = 0x0000_0011;

/// Which mechanism kept the window out of its own screenshot.
///
/// Both variants carry the raw return value of the
/// `SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE)` call they were
/// chosen from, so `log.txt` / `--probe` report what the OS actually answered
/// (non-zero = accepted) instead of only the path that was taken.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hide {
    /// `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)` was accepted
    /// (`ret` non-zero): the window was never altered on screen, so DWM had no
    /// transparent frame to present — the path that cannot flash.
    DisplayAffinity { ret: i32 },
    /// The call was refused (`ret` 0, pre-2004 Windows): the window was made
    /// fully transparent for the blit and restored right after.
    LayeredAlpha { ret: i32 },
}

impl Hide {
    /// Stable id for the log / probe readback.
    pub fn id(self) -> &'static str {
        match self {
            Hide::DisplayAffinity { .. } => "display-affinity",
            Hide::LayeredAlpha { .. } => "layered-alpha",
        }
    }

    /// The raw `SetWindowDisplayAffinity` return value this path came from.
    /// Non-zero means the compositor accepted `WDA_EXCLUDEFROMCAPTURE`, so the
    /// window was never hidden on screen.
    pub fn affinity_ret(self) -> i32 {
        match self {
            Hide::DisplayAffinity { ret } | Hide::LayeredAlpha { ret } => ret,
        }
    }
}

/// `SetLayeredWindowAttributes` needs `WS_EX_LAYERED`; winit's transparent
/// windows do not carry it, so the fallback capture path adds it once. The
/// `WDA_EXCLUDEFROMCAPTURE` path never touches the window's extended style.
fn ensure_layered(hwnd: isize) {
    unsafe {
        let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        if ex & WS_EX_LAYERED == 0 {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex | WS_EX_LAYERED);
        }
    }
}

/// Screenshot the primary monitor into a top-down BGRA buffer.
///
/// The window is kept out of its own screenshot (see [`Hide`]) for the blit
/// only, so the shot shows the real desktop instead of our own UI. Returns
/// `None` on any Win32 failure — the caller then keeps its previous capture,
/// or falls back to the painted gradient.
pub fn capture(hwnd: isize) -> Option<(Shot, Hide)> {
    let width = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let height = unsafe { GetSystemMetrics(SM_CYSCREEN) };
    if width <= 0 || height <= 0 {
        return None;
    }
    let (w, h) = (width as usize, height as usize);

    unsafe {
        let screen = GetDC(0);
        if screen == 0 {
            return None;
        }
        let mem = CreateCompatibleDC(screen);
        if mem == 0 {
            ReleaseDC(0, screen);
            return None;
        }

        let bmi = BitmapInfo {
            header: BitmapInfoHeader {
                size: std::mem::size_of::<BitmapInfoHeader>() as u32,
                width,
                // Negative height: a top-down DIB, i.e. `bgra` in screen order.
                height: -height,
                planes: 1,
                bit_count: 32,
                compression: BI_RGB,
                size_image: 0,
                x_pels_per_meter: 0,
                y_pels_per_meter: 0,
                clr_used: 0,
                clr_important: 0,
            },
            colors: [0; 3],
        };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let dib = CreateDIBSection(mem, &bmi, DIB_RGB_COLORS, &mut bits, 0, 0);
        if dib == 0 || bits.is_null() {
            DeleteDC(mem);
            ReleaseDC(0, screen);
            return None;
        }
        let old = SelectObject(mem, dib);

        // Hide → blit → unhide, back to back.
        //
        // Preferred path: `WDA_EXCLUDEFROMCAPTURE`. The compositor omits the
        // window from the blit while the affinity is set and keeps drawing it
        // for the user, so no frame ever changes on screen — unlike the
        // transparent-for-a-few-ms trick below, whose alpha-0 frame DWM
        // sometimes presents (the flash this app used to show).
        // Fallback: the affinity call is not available (pre-2004 Windows), so
        // the window goes fully transparent for the blit and back to 255 right
        // after it — well under one vsync.
        let affinity_ret = SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE);
        let hidden = if affinity_ret != 0 {
            true
        } else {
            ensure_layered(hwnd);
            SetLayeredWindowAttributes(hwnd, 0, 0, LWA_ALPHA) != 0
        };
        let blt = BitBlt(mem, 0, 0, width, height, screen, 0, 0, SRCCOPY);
        if affinity_ret != 0 {
            SetWindowDisplayAffinity(hwnd, WDA_NONE);
        } else {
            SetLayeredWindowAttributes(hwnd, 0, 255, LWA_ALPHA);
        }
        let hide = if affinity_ret != 0 {
            Hide::DisplayAffinity {
                ret: affinity_ret,
            }
        } else {
            Hide::LayeredAlpha {
                ret: affinity_ret,
            }
        };

        let shot = if hidden && blt != 0 {
            let bytes =
                std::slice::from_raw_parts(bits as *const u8, w * h * 4).to_vec();
            Some((
                Shot {
                    origin: (0, 0),
                    width: w,
                    height: h,
                    bgra: bytes,
                },
                hide,
            ))
        } else {
            None
        };

        // Deselect the DIB before deleting it, or `DeleteObject` would fail
        // and leak the section.
        if old != 0 {
            SelectObject(mem, old);
        }
        DeleteObject(dib);
        DeleteDC(mem);
        ReleaseDC(0, screen);
        shot
    }
}
// ---------------------------------------------------------------------------
// Downscale + Gaussian
// ---------------------------------------------------------------------------

/// Repeated 2x2 box halving, converting BGRA to `[f32; 3]` RGB in `0.0..=1.0`.
///
/// Each halving averages every 2x2 block (edges of an odd-sized image average
/// the pixels that exist, so `w x h` becomes `ceil(w/2) x ceil(h/2)`). The
/// floats are treated as approximately linear; no gamma math is done.
pub fn downscale(shot: &Shot, halvings: u8) -> (usize, usize, Vec<[f32; 3]>) {
    let mut w = shot.width;
    let mut h = shot.height;
    let mut rgb = to_rgb(&shot.bgra, w, h);
    for _ in 0..halvings {
        if w <= 1 && h <= 1 {
            break;
        }
        let (nw, nh) = ((w + 1) / 2, (h + 1) / 2);
        rgb = halve(&rgb, w, h, nw, nh);
        w = nw;
        h = nh;
    }
    (w, h, rgb)
}

fn to_rgb(bgra: &[u8], w: usize, h: usize) -> Vec<[f32; 3]> {
    let n = w * h;
    let mut out: Vec<[f32; 3]> = bgra
        .chunks_exact(4)
        .take(n)
        .map(|p| {
            [
                p[2] as f32 / 255.0, // R
                p[1] as f32 / 255.0, // G
                p[0] as f32 / 255.0, // B
            ]
        })
        .collect();
    // A short buffer (should not happen for a DIB) stays black instead of
    // silently changing the image's dimensions.
    out.resize(n, [0.0; 3]);
    out
}

fn halve(src: &[[f32; 3]], w: usize, h: usize, nw: usize, nh: usize) -> Vec<[f32; 3]> {
    let mut out = vec![[0.0f32; 3]; nw * nh];
    for y in 0..nh {
        for x in 0..nw {
            let mut acc = [0.0f32; 3];
            let mut n = 0.0f32;
            for (dx, dy) in [(0usize, 0usize), (1, 0), (0, 1), (1, 1)] {
                let sx = x * 2 + dx;
                let sy = y * 2 + dy;
                if sx < w && sy < h {
                    let p = src[sy * w + sx];
                    acc[0] += p[0];
                    acc[1] += p[1];
                    acc[2] += p[2];
                    n += 1.0;
                }
            }
            if n > 0.0 {
                out[y * nw + x] = [acc[0] / n, acc[1] / n, acc[2] / n];
            }
        }
    }
    out
}

/// A real separable Gaussian: a 1-D kernel of radius `ceil(3*sigma)`, applied
/// horizontally and then vertically, with samples clamped at the edges.
///
/// `sigma <= 0.0` is an exact copy (no-op) — the 模糊度 = 0 case. A `w x h`
/// of 0, or a buffer shorter than `w * h`, yields an empty image instead of a
/// panic.
pub fn gaussian(rgb: &[[f32; 3]], w: usize, h: usize, sigma: f32) -> Vec<[f32; 3]> {
    let n = w * h;
    if n == 0 || rgb.len() < n {
        return Vec::new();
    }
    let src = &rgb[..n];
    if sigma <= 0.0 {
        return src.to_vec();
    }

    let radius = (3.0 * sigma).ceil() as usize;
    let mut kernel = Vec::with_capacity(radius * 2 + 1);
    let mut sum = 0.0f32;
    for i in 0..=2 * radius {
        let x = i as f32 - radius as f32;
        let weight = (-x * x / (2.0 * sigma * sigma)).exp();
        kernel.push(weight);
        sum += weight;
    }
    for k in &mut kernel {
        *k /= sum;
    }

    let mut tmp = vec![[0.0f32; 3]; n];
    for y in 0..h {
        let row = &src[y * w..y * w + w];
        for x in 0..w {
            let mut acc = [0.0f32; 3];
            for (k, weight) in kernel.iter().enumerate() {
                // x + k - radius, clamped to the row.
                let sx = (x + k).saturating_sub(radius).min(w - 1);
                let p = row[sx];
                acc[0] += p[0] * weight;
                acc[1] += p[1] * weight;
                acc[2] += p[2] * weight;
            }
            tmp[y * w + x] = acc;
        }
    }

    let mut out = vec![[0.0f32; 3]; n];
    for y in 0..h {
        for x in 0..w {
            let mut acc = [0.0f32; 3];
            for (k, weight) in kernel.iter().enumerate() {
                let sy = (y + k).saturating_sub(radius).min(h - 1);
                let p = tmp[sy * w + x];
                acc[0] += p[0] * weight;
                acc[1] += p[1] * weight;
                acc[2] += p[2] * weight;
            }
            out[y * w + x] = acc;
        }
    }
    out
}

/// `[f32; 3]` in `0.0..=1.0` back to an opaque `egui::ColorImage`.
pub fn to_color_image(rgb: &[[f32; 3]], w: usize, h: usize) -> ColorImage {
    let n = (w * h).min(rgb.len());
    let channel = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let mut pixels: Vec<Color32> = rgb[..n]
        .iter()
        .map(|p| Color32::from_rgb(channel(p[0]), channel(p[1]), channel(p[2])))
        .collect();
    pixels.resize(w * h, Color32::BLACK);
    ColorImage::new([w, h], pixels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gaussian_of_a_flat_image_preserves_the_mean() {
        let (w, h) = (16, 9);
        let flat = vec![[0.4f32, 0.5, 0.6]; w * h];
        let out = gaussian(&flat, w, h, 2.0);
        assert_eq!(out.len(), w * h);
        for p in &out {
            assert!(
                (p[0] - 0.4).abs() < 1e-5
                    && (p[1] - 0.5).abs() < 1e-5
                    && (p[2] - 0.6).abs() < 1e-5,
                "a flat image must stay flat, got {p:?}"
            );
        }
    }

    #[test]
    fn gaussian_of_a_step_edge_is_monotonic_and_smooth() {
        let (w, h) = (32, 4);
        let mut src = vec![[0.0f32; 3]; w * h];
        for y in 0..h {
            for x in w / 2..w {
                src[y * w + x] = [1.0; 3];
            }
        }
        let out = gaussian(&src, w, h, 3.0);
        let row = &out[..w];
        for x in 1..w {
            assert!(
                row[x][0] >= row[x - 1][0] - 1e-6,
                "the edge must not wobble at x={x}: {:?} -> {:?}",
                row[x - 1],
                row[x]
            );
        }
        assert!(row[0][0] < 0.01, "the black side stays black");
        assert!(row[w - 1][0] > 0.99, "the white side stays white");
        assert!(
            row.iter().any(|p| p[0] > 0.05 && p[0] < 0.95),
            "a blurred edge has to have intermediate values"
        );
    }

    #[test]
    fn gaussian_with_zero_sigma_is_an_exact_copy() {
        let src: Vec<[f32; 3]> = (0..12)
            .map(|i| [i as f32 / 12.0, 0.25, 0.75])
            .collect();
        assert_eq!(gaussian(&src, 4, 3, 0.0), src);
        assert_eq!(gaussian(&src, 4, 3, -1.0), src, "negative is a no-op too");
    }

    #[test]
    fn gaussian_handles_tiny_and_empty_images() {
        assert_eq!(gaussian(&[], 0, 0, 2.0), Vec::<[f32; 3]>::new());
        assert_eq!(gaussian(&[], 4, 0, 2.0), Vec::<[f32; 3]>::new());

        let one = [[0.3f32, 0.4, 0.5]];
        let out = gaussian(&one, 1, 1, 5.0);
        assert_eq!(out.len(), 1);
        assert!((out[0][0] - 0.3).abs() < 1e-6);
        assert!((out[0][1] - 0.4).abs() < 1e-6);
    }

    #[test]
    fn downscale_halves_with_ceil_and_keeps_flat_images_flat() {
        // BGRA (B=255, G=128, R=0) -> RGB (0.0, 128/255, 1.0).
        let shot = Shot {
            origin: (0, 0),
            width: 5,
            height: 3,
            bgra: vec![255u8, 128, 0, 255].repeat(15),
        };

        let (w0, h0, rgb0) = downscale(&shot, 0);
        assert_eq!((w0, h0), (5, 3), "zero halvings is the raw size");
        assert_eq!(rgb0.len(), 15);

        let (w1, h1, rgb1) = downscale(&shot, 1);
        assert_eq!((w1, h1), (3, 2), "ceil(5/2) x ceil(3/2)");
        assert_eq!(rgb1.len(), 6);

        let (w2, h2, rgb2) = downscale(&shot, 2);
        assert_eq!((w2, h2), (2, 1));
        assert_eq!(rgb2.len(), 2);

        for p in rgb0.iter().chain(&rgb1).chain(&rgb2) {
            assert!(
                p[0].abs() < 1e-6
                    && (p[1] - 128.0 / 255.0).abs() < 1e-6
                    && (p[2] - 1.0).abs() < 1e-6,
                "a flat image stays flat through every halving, got {p:?}"
            );
        }
    }

    #[test]
    fn params_are_sharp_at_zero_and_grow_with_the_slider() {
        assert_eq!(params(0), (0, 0.0), "0 means an unblurred capture");

        // The effective sigma in full-resolution pixels is monotonic in the
        // setting, so the slider only ever gets blurrier.
        let full = |b: u8| {
            let (h, s) = params(b);
            s * (1u32 << h) as f32
        };
        let mut prev = 0.0;
        for b in 1..=100u8 {
            let (h, s) = params(b);
            assert!(s > 0.0, "every non-zero setting blurs");
            assert!(h <= 3, "the halvings stay within 0..=3");
            let now = full(b);
            assert!(now >= prev, "blur must not decrease at {b}");
            prev = now;
        }
    }
}
