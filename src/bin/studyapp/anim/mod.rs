//! Animation primitives: the app's one motion vocabulary ([`motion`]), the
//! pure easing math the shake uses ([`easing`]), and the wrong-answer shake
//! itself — it carries feedback; everything else was decoration.

pub mod easing;
pub mod motion;

/// Seconds a shake stays alive.
pub const SHAKE_DURATION: f32 = 0.3;
/// Peak horizontal offset of the shake, in points.
pub const SHAKE_AMPLITUDE: f32 = 5.0;

/// Per-frame animation state, driven by a monotonic `now` in seconds.
#[derive(Default)]
pub struct Animator {
    /// `(key, t0)` for each running shake.
    shakes: Vec<(u64, f64)>,
}

impl Animator {
    pub fn new() -> Self {
        Self::default()
    }

    /// (Re)start the shake identified by `key` at `now`.
    pub fn trigger_shake(&mut self, key: u64, now: f64) {
        match self.shakes.iter_mut().find(|(k, _)| *k == key) {
            Some(entry) => entry.1 = now,
            None => self.shakes.push((key, now)),
        }
    }

    /// Signed horizontal offset multiplier in `[-1, 1]`; `0.0` when not shaking
    /// or once finished.
    pub fn shake(&self, key: u64, now: f64) -> f32 {
        match self.shakes.iter().find(|(k, _)| *k == key) {
            Some((_, t0)) => {
                let p = easing::clamp01(((now - t0) / SHAKE_DURATION as f64) as f32);
                easing::shake_decay(p)
            }
            None => 0.0,
        }
    }

    /// True while a shake is still running at `now`.
    pub fn is_live(&self, now: f64) -> bool {
        self.shakes
            .iter()
            .any(|(_, t0)| now - *t0 < SHAKE_DURATION as f64)
    }

    /// Drop finished shakes. Call once per frame.
    pub fn prune(&mut self, now: f64) {
        self.shakes
            .retain(|(_, t0)| now - *t0 < SHAKE_DURATION as f64);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shake_restarts_and_finishes() {
        let mut a = Animator::new();
        a.trigger_shake(3, 0.0);
        a.trigger_shake(3, 0.1);
        assert_eq!(a.shakes.len(), 1, "re-triggering must not add a second entry");
        assert_eq!(a.shake(3, 0.1), 0.0, "starts at zero offset");
        assert!(a.shake(3, 0.22).abs() > 0.0, "mid-shake is non-zero");
        assert_eq!(a.shake(3, 100.0), 0.0, "finished");
        assert_eq!(a.shake(999, 0.22), 0.0, "unknown key");
    }

    #[test]
    fn is_live_and_prune() {
        let mut a = Animator::new();
        assert!(!a.is_live(0.0));
        a.trigger_shake(1, 0.0);
        assert!(a.is_live(0.0));
        assert!(!a.is_live(100.0));
        a.prune(100.0);
        assert!(!a.is_live(100.0));
    }
}
