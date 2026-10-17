//! The app's one motion vocabulary: three duration tiers and two curves,
//! shared by every animated surface.
//!
//! The design language is quiet, so every transition is short — nothing in
//! this module runs longer than ~220 ms, and the whole vocabulary fits on one
//! screen:
//!
//! * [`duration::FAST`] (~100 ms) — cursor-level feedback: hover and press
//!   tints on rail rows, chips, check boxes, buttons and window controls.
//! * [`duration::BASE`] (~160 ms) — everyday state changes: selection,
//!   checked, the answer reveal, the progress fill, the question swap.
//! * [`duration::SLOW`] (~220 ms) — deliberate movement: the segmented
//!   indicator travelling between items, the settings modal fading in.
//!
//! * [`curve::ENTER`] (ease-out) — for anything **arriving**: appearing
//!   fills, fades, growing bars. Starts quickly, settles softly.
//! * [`curve::STATE`] (ease-in-out) — for A→B **state changes** (slides,
//!   toggles) where the eye should follow the whole path.
//!
//! Everything is driven by a monotonic wall clock (`InputState::time`), and
//! all state lives in egui's temp data or plain structs — never in `App` — so
//! no animation can mark the document dirty. Repaints are requested only
//! while a transition is actually running; an idle app stays asleep.

use egui::{Context, Id};

/// Easing curve: linear `0..=1` progress in, eased `0..=1` progress out.
/// Endpoints are exact (`f(0) == 0`, `f(1) == 1`) and the curve never
/// goes backwards.
pub type Curve = fn(f32) -> f32;

/// Ease-out — starts fast, settles softly. For things arriving.
pub fn ease_out(t: f32) -> f32 {
    let u = 1.0 - t.clamp(0.0, 1.0);
    1.0 - u * u * u
}

/// Ease-in-out — accelerates, then decelerates. For state changes.
pub fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        2.0 * t * t
    } else {
        1.0 - 2.0 * (1.0 - t) * (1.0 - t)
    }
}

pub mod curve {
    use super::{ease_in_out, ease_out, Curve};

    /// Ease-out: use for anything appearing / arriving.
    pub const ENTER: Curve = ease_out;
    /// Ease-in-out: use for state changes that travel between two states.
    pub const STATE: Curve = ease_in_out;
}

pub mod duration {
    /// Cursor-level feedback — hover / press tints (~100 ms).
    pub const FAST: f32 = 0.10;
    /// Everyday state changes — selection, checked, reveal, question swap (~160 ms).
    pub const BASE: f32 = 0.16;
    /// Deliberate movement — segmented indicator travel, modal entrance (~220 ms).
    pub const SLOW: f32 = 0.22;
}

/// A one-track eased transition between two float values, driven by a
/// monotonic `now` in seconds. Pure math, unit-tested below; the egui-facing
/// helpers wrap it in temp-data storage.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Transition {
    from: f32,
    to: f32,
    t0: f64,
    dur: f32,
}

impl Transition {
    /// A transition that starts settled at `value` — the first time a widget
    /// is seen there is nothing to animate.
    fn settled(value: f32, now: f64) -> Self {
        Self {
            from: value,
            to: value,
            t0: now,
            dur: 0.0,
        }
    }

    /// Aim at `target`. When it differs from where we were already heading,
    /// the transition continues from its current (mid-flight) position so the
    /// value never jumps.
    fn retarget(&mut self, target: f32, dur: f32, now: f64, curve: Curve) {
        if (self.to - target).abs() <= 1e-6 {
            return;
        }
        self.from = self.sample(now, curve);
        self.to = target;
        self.t0 = now;
        self.dur = dur;
    }

    /// Current eased value; exactly `to` once the duration has run out.
    fn sample(&self, now: f64, curve: Curve) -> f32 {
        let r = raw_progress(self.t0, now, self.dur);
        self.from + (self.to - self.from) * curve(r)
    }

    /// True once this transition has nothing left to animate.
    fn is_settled(&self, now: f64) -> bool {
        self.from == self.to || raw_progress(self.t0, now, self.dur) >= 1.0
    }
}

/// Linear `0..=1` progress of a `dur`-long transition started at `t0`.
fn raw_progress(t0: f64, now: f64, dur: f32) -> f32 {
    if dur <= 0.0 {
        return 1.0;
    }
    (((now - t0) / dur as f64) as f32).clamp(0.0, 1.0)
}

/// Eased `0→1` progress since `t0`; reaches exactly `1.0` after `dur`
/// seconds, and clamps (never overshoots or goes negative).
pub fn progress_since(t0: f64, now: f64, dur: f32, curve: Curve) -> f32 {
    curve(raw_progress(t0, now, dur))
}

/// Shared core of the egui-facing helpers: loads the transition stored under
/// `id`, retargets it, requests a repaint while it is still running, and
/// returns the eased value for this frame.
fn advance(ctx: &Context, id: Id, target: f32, dur: f32, curve: Curve) -> f32 {
    let now = ctx.input(|i| i.time);
    let mut live = false;
    let value = ctx.data_mut(|d| {
        let mut t = d
            .get_temp::<Transition>(id)
            .unwrap_or_else(|| Transition::settled(target, now));
        t.retarget(target, dur, now, curve);
        live = !t.is_settled(now);
        let v = t.sample(now, curve);
        d.insert_temp(id, t);
        v
    });
    if live {
        ctx.request_repaint();
    }
    value
}

/// Eased `0..=1` state for a boolean (hover, checked, selected, on/off).
///
/// The first time an `id` is seen it returns the target immediately — widgets
/// do not animate in just because they were first drawn.
pub fn ease_bool(ctx: &Context, id: Id, target: bool, dur: f32, curve: Curve) -> f32 {
    advance(ctx, id, if target { 1.0 } else { 0.0 }, dur, curve)
}

/// Eased float for a value state (e.g. the segmented indicator position, or
/// the session progress fill).
pub fn ease_value(ctx: &Context, id: Id, target: f32, dur: f32, curve: Curve) -> f32 {
    advance(ctx, id, target, dur, curve)
}

/// Eased `0→1` progress for a one-shot event keyed by `key`: a question
/// change, an answer reveal, a modal opening. Whenever `key` changes the
/// clock re-arms from now, so every event gets its own entrance.
pub fn change_progress(ctx: &Context, id: Id, key: u64, dur: f32, curve: Curve) -> f32 {
    let now = ctx.input(|i| i.time);
    let t0 = ctx.data_mut(|d| match d.get_temp::<(u64, f64)>(id) {
        Some((k, t0)) if k == key => t0,
        _ => {
            d.insert_temp(id, (key, now));
            now
        }
    });
    let p = progress_since(t0, now, dur, curve);
    if p < 1.0 {
        ctx.request_repaint();
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn curves_are_exact_at_the_endpoints_and_never_go_backwards() {
        for (name, curve) in [("enter", curve::ENTER), ("state", curve::STATE)] {
            assert_eq!(curve(0.0), 0.0, "{name} at t=0");
            assert_eq!(curve(1.0), 1.0, "{name} at t=1");
            assert_eq!(curve(-1.0), 0.0, "{name} clamps below");
            assert_eq!(curve(2.0), 1.0, "{name} clamps above");
            let mut prev = curve(0.0);
            let mut i = 1;
            while i <= 100 {
                let v = curve(i as f32 / 100.0);
                assert!(v >= prev, "{name} must not go backwards at {i}");
                prev = v;
                i += 1;
            }
        }
        // Ease-out front-loads its progress; ease-in-out does not.
        assert!(curve::ENTER(0.25) > curve::STATE(0.25));
    }

    #[test]
    fn durations_are_ordered_fast_base_slow_and_all_short() {
        assert!(duration::FAST < duration::BASE, "fast tier must be shortest");
        assert!(duration::BASE < duration::SLOW, "slow tier must be longest");
        assert!(duration::FAST >= 0.09, "hover feedback must stay perceptible");
        assert!(duration::SLOW <= 0.30, "nothing in this app may feel slow");
    }

    #[test]
    fn transition_reaches_its_target_and_settles() {
        let mut t = Transition::settled(0.0, 0.0);
        assert!(t.is_settled(0.0), "a fresh transition is already settled");
        t.retarget(1.0, duration::BASE, 0.0, curve::ENTER);
        assert!(!t.is_settled(0.0), "a retargeted transition is live");
        assert_eq!(t.sample(0.0, curve::ENTER), 0.0);
        assert!(t.sample(0.08, curve::ENTER) > 0.0, "moves mid-flight");
        assert_eq!(
            t.sample(1.0, curve::ENTER),
            1.0,
            "lands exactly on the target"
        );
        assert!(t.is_settled(1.0), "and then reports itself done");
    }

    #[test]
    fn retarget_mid_flight_continues_from_the_current_position() {
        let mut t = Transition::settled(0.0, 0.0);
        t.retarget(1.0, duration::BASE, 0.0, curve::ENTER);
        let mid = t.sample(0.08, curve::ENTER);
        assert!(mid > 0.2 && mid < 0.95, "mid-flight sample: {mid}");
        t.retarget(0.0, duration::BASE, 0.08, curve::ENTER);
        assert_eq!(t.sample(0.08, curve::ENTER), mid, "no jump on retarget");
        assert_eq!(t.sample(1.0, curve::ENTER), 0.0);
    }

    #[test]
    fn retargeting_to_the_same_target_does_not_restart_the_clock() {
        let mut t = Transition::settled(1.0, 0.0);
        t.retarget(1.0, duration::BASE, 0.5, curve::ENTER); // must be a no-op
        assert_eq!(t.sample(0.5, curve::ENTER), 1.0);
    }

    #[test]
    fn progress_since_hits_one_and_never_overshoots() {
        let (t0, dur) = (10.0, duration::BASE);
        assert_eq!(progress_since(t0, 10.0, dur, curve::ENTER), 0.0);
        assert_eq!(
            progress_since(t0, 9.0, dur, curve::ENTER),
            0.0,
            "no negative progress"
        );
        assert_eq!(progress_since(t0, 99.0, dur, curve::ENTER), 1.0);
        let mid = progress_since(t0, 10.08, dur, curve::ENTER);
        assert!(mid > 0.0 && mid < 1.0);
    }

    /// The repaint gate: a helper whose transition finished must stop asking
    /// for frames. `Duration::MAX` means "never repaint". egui pairs every
    /// immediate request with one follow-up frame, so the idle frame is the
    /// third one.
    #[test]
    fn a_finished_transition_stops_requesting_repaints() {
        let ctx = egui::Context::default();
        let run = |time: f64| {
            let mut input = egui::RawInput::default();
            input.time = Some(time);
            let out = ctx.run_ui(input, |ui| {
                change_progress(
                    ui.ctx(),
                    Id::new("done_clock"),
                    1,
                    duration::BASE,
                    curve::ENTER,
                );
            });
            out.viewport_output
                .values()
                .map(|v| v.repaint_delay)
                .min()
                .unwrap_or(Duration::MAX)
        };
        let _ = run(0.0);
        let _ = run(0.2);
        assert_eq!(
            run(5.0),
            Duration::MAX,
            "nothing animates → the app must be allowed to sleep"
        );
    }

    /// …and an unfinished one must: while a transition is live, frames are
    /// requested.
    #[test]
    fn a_running_transition_requests_repaints() {
        let ctx = egui::Context::default();
        let run = |time: f64| {
            let mut input = egui::RawInput::default();
            input.time = Some(time);
            let out = ctx.run_ui(input, |ui| {
                change_progress(
                    ui.ctx(),
                    Id::new("live_clock"),
                    7,
                    duration::BASE,
                    curve::ENTER,
                );
            });
            out.viewport_output
                .values()
                .map(|v| v.repaint_delay)
                .min()
                .unwrap_or(Duration::MAX)
        };
        assert_eq!(run(0.0), Duration::ZERO, "a fresh transition wants frames");
        assert_eq!(run(0.05), Duration::ZERO, "still running");
        let _ = run(1.0);
        assert_eq!(run(1.1), Duration::MAX, "finished → gate closes again");
    }
}
