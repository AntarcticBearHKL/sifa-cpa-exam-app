//! Pure `f32` easing helpers used by the animator.

use std::f32::consts::TAU;

pub fn clamp01(t: f32) -> f32 {
    t.clamp(0.0, 1.0)
}

/// Signed horizontal shake offset in `[-1, 1]`. A decaying envelope `(1 - t)`
/// scales a fast sine, and both endpoints are forced to exactly `0.0`.
pub fn shake_decay(t: f32) -> f32 {
    if t <= 0.0 || t >= 1.0 {
        return 0.0;
    }
    (1.0 - t) * (t * TAU * 3.0).sin()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_bounds() {
        assert_eq!(clamp01(-2.0), 0.0);
        assert_eq!(clamp01(2.0), 1.0);
        assert_eq!(clamp01(0.25), 0.25);
    }

    #[test]
    fn shake_decay_endpoints_and_bounds() {
        assert_eq!(shake_decay(0.0), 0.0);
        assert_eq!(shake_decay(1.0), 0.0);
        let mut i = 0;
        while i <= 200 {
            let t = i as f32 / 200.0;
            assert!(shake_decay(t).abs() <= 1.0, "out of range at {t}");
            i += 1;
        }
    }
}
