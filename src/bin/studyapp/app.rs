use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui;
use egui::RichText;
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};

pub(crate) use crate::{anim, diag, glass, logic, theme, win};

use sfzklearning::{
    domain_root, load_subject, load_user_store, load_wrong_ids, now_iso, parse_cli_args,
    resolve_state_dir, save_user_store, save_wrong_ids, write_current, CurrentContext, Domain,
    KnowledgePoint, Question, StateKp, StateOption, StateQuestion, SubjectFile, SummarySubject,
    UserStore,
};

mod practice;
mod settings;
mod shell;

const APP_TITLE: &str = "sfzklearning";

#[derive(Default)]
struct DomainData {
    subjects: Vec<SummarySubject>,
    error: Option<String>,
}

/// One practice run: a fixed order of questions plus its live state.
struct Session {
    order: Vec<usize>,
    pos: usize,
    single: Option<String>,
    multi: Vec<String>,
    /// 提交答案 was used: the choice question has been graded, so it counts.
    submitted: bool,
    /// 显示答案 was used: the answer is visible without being graded. For a
    /// choice question that means the correct options are highlighted and the
    /// explanation is open, exactly like a submitted one — but nothing was
    /// recorded, so the question still counts as 未做过 and comes back. For a
    /// memory / self-test question it reveals the 参考答案 before the
    /// self-assessment.
    shown: bool,
    /// The current memory / self-test question has been rated 记住了 /
    /// 没记住. Choice questions are judged by `submitted` / `shown` instead.
    self_assessed: bool,
    answered: usize,
    correct: usize,
    label: String,
}

impl Session {
    fn clear_answer(&mut self) {
        self.single = None;
        self.multi.clear();
        self.submitted = false;
        self.shown = false;
        self.self_assessed = false;
    }
    fn selection(&self, is_multi: bool) -> Vec<String> {
        if is_multi {
            self.multi.clone()
        } else {
            self.single.clone().into_iter().collect()
        }
    }
    /// 显示答案: reveal without grading. This deliberately records nothing —
    /// no `answered` / `correct`, no per-question stats, no wrong-question
    /// entry — so a revealed question stays 未做过 and comes up again; that is
    /// the point of studying it.
    fn reveal(&mut self) {
        self.shown = true;
    }
}

struct App {
    law_root: PathBuf,
    cpa_root: PathBuf,
    state_dir: PathBuf,
    tokens: theme::Tokens,
    mode: theme::Mode,
    /// Raw Win32 window handle captured at startup, so the DWM
    /// immersive-dark flag can follow a runtime theme switch.
    hwnd: Option<isize>,

    law: DomainData,
    cpa: DomainData,

    domain: Domain,

    subject_id: String,
    subject_name: String,
    subject: Option<Arc<SubjectFile>>,
    subject_error: Option<String>,

    /// title -> index into `subject.knowledge_points` (first match wins).
    kp_title_map: std::collections::HashMap<String, usize>,

    // 做题 — setup
    set_kind: logic::Kind,
    set_chapters: HashSet<String>,
    set_sources: logic::Sources,
    set_draw: logic::Draw,
    /// Numeric value used when `set_draw` is `Draw::Custom`; always kept in
    /// the valid range by the input and by `Draw::limit`.
    set_draw_custom: usize,
    set_order: logic::Order,
    set_chapter_filter: String,
    set_query: String,
    session: Option<Session>,

    wrong_ids: HashSet<String>,
    user: UserStore,
    note_open: bool,
    note_draft: String,
    note_draft_for: Option<String>,

    settings_open: bool,
    settings_tab: usize,
    /// Bumped whenever the settings modal opens, so its entrance fade gets a
    /// fresh motion clock (0 = never opened).
    settings_epoch: u64,
    /// Bumped on every answer submission, so the reveal fade gets a fresh
    /// motion clock.
    reveal_epoch: u64,

    /// (list key, index) of the last row the pointer hovered, for the rail.
    anim: anim::Animator,
    bg_phase: f32,
    /// The captured, Gaussian-blurred desktop (see `glass::blur`). `None` when
    /// no capture has succeeded, in which case `glass::backdrop` paints the
    /// gradient fallback instead.
    glass_texture: Option<egui::TextureHandle>,
    /// The window's outer rect as last seen, so a move / resize repaints the
    /// backdrop with its new UV rect (the capture itself is never re-run).
    glass_geom: Option<egui::Rect>,
    /// The whole-monitor capture the texture was built from. Kept so a 模糊度
    /// change re-blurs these pixels instead of photographing the screen again.
    /// It is a *whole-monitor* shot and the UV rect is derived from the
    /// window's screen position, so it stays registered with the desktop while
    /// the window moves — which is why a move / resize never needs a capture.
    glass_shot: Option<glass::blur::Shot>,
    /// Which mechanism kept the window out of the last capture; reported by
    /// `--probe` and written to `log.txt`.
    glass_hide: Option<glass::blur::Hide>,
    /// Top-left of the capture, in physical screen pixels.
    glass_origin: (i32, i32),
    /// Size of the capture, in physical pixels.
    glass_size: (usize, usize),
    /// The 模糊度 the current texture was built with, so moving that slider
    /// re-runs downscale + Gaussian + the upload from [`App::glass_shot`].
    glass_blur_at_render: Option<u8>,
    /// A capture has been attempted (and may have failed), so a failing one is
    /// not retried every frame.
    glass_capture_attempted: bool,
    /// A 重新采样背景 click asked for one more capture.
    glass_recapture: bool,
    /// How many times `glass::blur::capture` has run this session. Every
    /// capture is logged, so a regression that starts re-capturing on move /
    /// resize shows up in `log.txt` instead of only as a flicker.
    glass_captures: u32,
    probe_report: Option<String>,
    md_cache: CommonMarkCache,
    dirty: bool,
}

/// What the frosted backdrop has to do this frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum GlassWork {
    /// Nothing: the texture is current.
    None,
    /// Photograph the desktop once more (`SetWindowDisplayAffinity` or the
    /// transparent-for-a-few-ms fallback; see `glass::blur::Hide`).
    Capture,
    /// Re-run downscale + Gaussian + the texture upload from the stored shot.
    /// No screen access at all.
    Reblur,
}

/// Capture rule for the frosted backdrop, in one pure place so the tests can
/// pin it down: capture **once** (or when 重新采样背景 asks for it), then never
/// again for a move / resize — the stored shot is a whole-monitor image whose
/// UV rect follows the window, so it stays registered to the desktop while the
/// window moves and only goes stale when *other windows* change. A 模糊度
/// change re-blurs the stored pixels; 透明度 never comes near the capture.
fn glass_work(
    capture_attempted: bool,
    recapture: bool,
    has_shot: bool,
    blur_changed: bool,
) -> GlassWork {
    if recapture || !capture_attempted {
        GlassWork::Capture
    } else if has_shot && blur_changed {
        GlassWork::Reblur
    } else {
        GlassWork::None
    }
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let args = parse_cli_args();
        let law_root = domain_root(Domain::Law, &args);
        let cpa_root = domain_root(Domain::Cpa, &args);
        let state_dir = resolve_state_dir(args.state_dir.as_deref());
        let user = load_user_store(&state_dir);

        if let Some(w) = install_fonts(&cc.egui_ctx) {
            diag::log(&state_dir, &w);
        }
        let mode = theme::Mode::from_id(&user.theme);
        let tokens = theme::install(&cc.egui_ctx, mode);

        let hwnd = win::window_hwnd(cc);
        win::apply_chrome(cc, mode);
        let probe_report = std::env::args()
            .any(|a| a == "--probe")
            .then(|| win::probe(cc));

        let law = load_domain_data(&law_root);
        let cpa = load_domain_data(&cpa_root);
        for (label, data) in [("law", &law), ("cpa", &cpa)] {
            if let Some(err) = &data.error {
                diag::log(&state_dir, &format!("{label} 数据加载失败: {err}"));
            }
        }

        let forced = std::env::var("CMF_START_DOMAIN")
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        let domain = match forced.as_str() {
            "cpa" if !cpa.subjects.is_empty() => Domain::Cpa,
            "law" if !law.subjects.is_empty() => Domain::Law,
            _ => user
                .last_mode
                .as_deref()
                .and_then(Domain::from_id)
                .unwrap_or({
                    if !law.subjects.is_empty() {
                        Domain::Law
                    } else {
                        Domain::Cpa
                    }
                }),
        };

        let mut app = App {
            law_root,
            cpa_root,
            state_dir,
            tokens,
            mode,
            hwnd,
            law,
            cpa,
            domain,
            subject_id: String::new(),
            subject_name: String::new(),
            subject: None,
            subject_error: None,
            kp_title_map: std::collections::HashMap::new(),
            set_kind: logic::Kind::All,
            set_chapters: HashSet::new(),
            set_sources: logic::Sources::default(),
            set_draw: logic::Draw::default(),
            set_draw_custom: logic::DEFAULT_CUSTOM_DRAW,
            set_order: logic::Order::default(),
            set_chapter_filter: String::new(),
            set_query: String::new(),
            session: None,
            wrong_ids: HashSet::new(),
            user,
            note_open: false,
            note_draft: String::new(),
            note_draft_for: None,
            settings_open: false,
            settings_tab: 0,
            settings_epoch: 0,
            reveal_epoch: 0,
            anim: anim::Animator::new(),
            bg_phase: 0.0,
            glass_texture: None,
            glass_shot: None,
            glass_hide: None,
            glass_origin: (0, 0),
            glass_size: (0, 0),
            glass_blur_at_render: None,
            glass_geom: None,
            glass_capture_attempted: false,
            glass_recapture: false,
            glass_captures: 0,
            probe_report,
            md_cache: CommonMarkCache::default(),
            dirty: true,
        };

        let subs = app.data(app.domain).subjects.clone();
        let wanted = std::env::var("CMF_START_SUBJECT").unwrap_or_default();
        let wanted = wanted.trim();
        let chosen = subs
            .iter()
            .find(|s| !wanted.is_empty() && s.id == wanted)
            .or_else(|| subs.first());
        if let Some(first) = chosen {
            app.subject_id = first.id.clone();
        }
        app.reload_subject();
        app
    }

    fn data(&self, d: Domain) -> &DomainData {
        match d {
            Domain::Law => &self.law,
            Domain::Cpa => &self.cpa,
        }
    }

    fn root(&self, d: Domain) -> &Path {
        match d {
            Domain::Law => &self.law_root,
            Domain::Cpa => &self.cpa_root,
        }
    }

    /// Subjects of the current mode, grouped by their official group, in order.
    /// Groups in [`HIDDEN_GROUPS`] are skipped entirely.
    fn subject_groups(&self) -> Vec<(String, Vec<SummarySubject>)> {
        let mut groups: Vec<(String, Vec<SummarySubject>)> = Vec::new();
        for s in &self.data(self.domain).subjects {
            let Some(key) = group_of(s) else {
                continue;
            };
            match groups.iter_mut().find(|(g, _)| *g == key) {
                Some((_, list)) => list.push(s.clone()),
                None => groups.push((key, vec![s.clone()])),
            }
        }
        groups
    }

    /// Switches the palette everywhere: tokens, egui visuals (both style
    /// slots), the egui theme preference (native window chrome) and the DWM
    /// immersive-dark flag. The choice is persisted immediately.
    fn set_mode(&mut self, ctx: &egui::Context, mode: theme::Mode) {
        if self.mode == mode {
            return;
        }
        self.mode = mode;
        self.tokens = theme::Tokens::new(mode);
        theme::apply(ctx, &self.tokens, mode);
        if let Some(hwnd) = self.hwnd {
            win::set_immersive_dark(hwnd, mode == theme::Mode::Dark);
        }
        self.user.theme = mode.id().to_string();
        self.persist_user();
    }

    fn enter_mode(&mut self, d: Domain) {
        self.domain = d;
        self.user.last_mode = Some(d.id().to_string());
        self.persist_user();
        self.subject_id.clear();
        let subs = self.data(d).subjects.clone();
        if let Some(first) = subs.first() {
            self.subject_id = first.id.clone();
        }
        self.session = None;
        self.reload_subject();
    }

    fn select_subject(&mut self, id: String) {
        if self.subject_id == id {
            return;
        }
        self.subject_id = id;
        self.session = None;
        self.reload_subject();
    }

    fn reload_subject(&mut self) {
        self.subject = None;
        self.subject_error = None;
        self.subject_name.clear();
        self.kp_title_map.clear();
        self.session = None;
        self.set_chapters.clear();
        self.set_sources = logic::Sources::default();
        self.set_draw = logic::Draw::default();
        self.set_draw_custom = logic::DEFAULT_CUSTOM_DRAW;
        self.set_order = logic::Order::default();
        self.set_chapter_filter.clear();
        self.set_query.clear();

        if self.subject_id.is_empty() {
            self.dirty = true;
            return;
        }

        let root = self.root(self.domain).to_path_buf();
        match load_subject(&root, &self.subject_id) {
            Ok(sf) => {
                let s = Arc::new(sf);
                self.subject_name = if s.name.is_empty() {
                    self.data(self.domain)
                        .subjects
                        .iter()
                        .find(|x| x.id == self.subject_id)
                        .map(|x| x.name.clone())
                        .unwrap_or_default()
                } else {
                    s.name.clone()
                };

                for (i, kp) in s.knowledge_points.iter().enumerate() {
                    self.kp_title_map.entry(kp.title.clone()).or_insert(i);
                }

                self.wrong_ids = load_wrong_ids(&self.state_dir, self.domain, &self.subject_id)
                    .into_iter()
                    .collect();
                self.subject = Some(s);
            }
            Err(e) => {
                self.subject_error = Some(e);
                self.wrong_ids.clear();
            }
        }
        self.dirty = true;
    }

    // ------------------------------------------------------------------
    // 做题 — setup + session
    // ------------------------------------------------------------------

    fn filter(&self) -> logic::Filter {
        logic::Filter {
            kind: self.set_kind,
            chapters: self.set_chapters.clone(),
            sources: self.set_sources,
            query: self.set_query.clone(),
        }
    }

    fn matching_questions(&self) -> Vec<usize> {
        let Some(s) = &self.subject else {
            return Vec::new();
        };
        logic::select(&s.questions, &self.filter(), &self.wrong_ids, &self.user)
    }

    fn start_session(&mut self, wrong_random: bool) {
        let Some(s) = self.subject.clone() else {
            return;
        };
        let (order, label) = if wrong_random {
            let idx = logic::wrong_sample(&s.questions, &self.wrong_ids, 10, rand_seed());
            (idx, "错题随机抽查".to_string())
        } else {
            let idx = logic::draw_plan(
                self.matching_questions(),
                self.set_draw,
                self.set_order,
                rand_seed(),
                self.set_draw_custom,
            );
            let chapters = if self.set_chapters.is_empty() {
                "全部章节".to_string()
            } else {
                format!("{} 章", self.set_chapters.len())
            };
            let tail = match self.set_draw.limit(self.set_draw_custom) {
                Some(n) => format!("{} {n} 题", self.set_order.label()),
                None => self.set_order.label().to_string(),
            };
            (
                idx,
                format!("{} · {} · {}", self.set_kind.label(), chapters, tail),
            )
        };

        if order.is_empty() {
            return;
        }
        self.session = Some(Session {
            order,
            pos: 0,
            single: None,
            multi: Vec::new(),
            submitted: false,
            shown: false,
            self_assessed: false,
            answered: 0,
            correct: 0,
            label,
        });
        self.dirty = true;
    }

    fn session_question(&self) -> Option<Question> {
        let s = self.subject.clone()?;
        let sess = self.session.as_ref()?;
        let qi = *sess.order.get(sess.pos)?;
        s.questions.get(qi).cloned()
    }

    fn submit(&mut self, now: f64) {
        let Some(q) = self.session_question() else {
            return;
        };
        let sel = self
            .session
            .as_ref()
            .map(|s| s.selection(q.is_multi()))
            .unwrap_or_default();
        let Some(ok) = logic::judge(&q, &sel) else {
            return;
        };
        let qid = q.id.clone();
        if let Some(sess) = self.session.as_mut() {
            sess.submitted = true;
            sess.answered += 1;
            if ok {
                sess.correct += 1;
            }
        }
        if ok {
            self.wrong_ids.remove(&qid);
        } else {
            self.wrong_ids.insert(qid.clone());
            self.anim.trigger_shake(question_key(&q), now);
        }
        self.reveal_epoch = self.reveal_epoch.wrapping_add(1);
        self.user.record_answer(&qid, ok, &now_iso());
        self.persist_wrong();
        self.persist_user();
        self.dirty = true;
    }

    /// 显示答案 for a choice question: reveal the correct options and the
    /// explanation without grading. Records nothing (see [`Session::reveal`]);
    /// only the reveal animation is re-armed.
    fn reveal_answer(&mut self) {
        let Some(sess) = self.session.as_mut() else {
            return;
        };
        sess.reveal();
        self.reveal_epoch = self.reveal_epoch.wrapping_add(1);
        self.dirty = true;
    }

    fn self_assess(&mut self, remembered: bool, _now: f64) {
        let Some(q) = self.session_question() else {
            return;
        };
        let qid = q.id.clone();
        if let Some(sess) = self.session.as_mut() {
            sess.self_assessed = true;
            sess.answered += 1;
            if remembered {
                sess.correct += 1;
            }
        }
        if remembered {
            self.wrong_ids.remove(&qid);
        } else {
            self.wrong_ids.insert(qid.clone());
        }
        self.user.record_answer(&qid, remembered, &now_iso());
        self.persist_wrong();
        self.persist_user();
        self.next_question();
    }

    /// Is the current question judged, so 下一题 / `→` may move on? See
    /// [`logic::can_advance`]. `←` 上一题 and `Esc` stay available, so the
    /// gate is never a trap. A memory question's self-assessment advances by
    /// itself, so this gate mostly guards against skipping it.
    fn can_advance_now(&self) -> bool {
        let (Some(q), Some(sess)) = (self.session_question(), self.session.as_ref()) else {
            return false;
        };
        logic::can_advance(&q, sess.submitted, sess.shown, sess.self_assessed)
    }

    fn next_question(&mut self) {
        if let Some(sess) = self.session.as_mut() {
            if !sess.order.is_empty() {
                sess.pos = (sess.pos + 1) % sess.order.len();
            }
            sess.clear_answer();
        }
        self.dirty = true;
    }

    fn prev_question(&mut self) {
        if let Some(sess) = self.session.as_mut() {
            if !sess.order.is_empty() {
                sess.pos = if sess.pos == 0 {
                    sess.order.len() - 1
                } else {
                    sess.pos - 1
                };
            }
            sess.clear_answer();
        }
        self.dirty = true;
    }

    // ------------------------------------------------------------------
    // keyboard
    // ------------------------------------------------------------------

    /// App-level shortcuts. A borderless window has no system shortcuts left, so
    /// these replace what the title bar used to give us. Arrow keys are skipped
    /// while a text field has focus (the search / note editors need them).
    fn handle_keys(&mut self, ctx: &egui::Context, now: f64) {
        if ctx.text_edit_focused() {
            return;
        }
        let (left, right, esc, enter) = ctx.input_mut(|i| {
            (
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft),
                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
                i.consume_key(egui::Modifiers::NONE, egui::Key::Enter),
            )
        });

        if esc {
            if self.settings_open {
                self.settings_open = false;
                return;
            }
            if self.session.is_some() {
                self.flush_note();
                self.session = None;
                self.note_draft_for = None;
                self.dirty = true;
            }
            return;
        }

        if self.session.is_some() {
            if left {
                self.prev_question();
            }
            // `→` obeys the same gate as 下一题 ▶: the current question must be
            // judged first. `←` and `Esc` stay available, so this is never a
            // trap.
            if right && self.can_advance_now() {
                self.next_question();
            }
            if enter {
                self.submit(now);
            }
        }
    }

    // ------------------------------------------------------------------
    // persistence
    // ------------------------------------------------------------------

    fn persist_wrong(&self) {
        let ids: Vec<String> = self.wrong_ids.iter().cloned().collect();
        if let Err(e) = save_wrong_ids(&self.state_dir, self.domain, &self.subject_id, &ids) {
            diag::log(&self.state_dir, &format!("写入错题文件失败: {e}"));
        }
    }

    fn persist_user(&self) {
        if let Err(e) = save_user_store(&self.state_dir, &self.user) {
            diag::log(&self.state_dir, &format!("写入 user.json 失败: {e}"));
        }
    }

    /// Commits the note draft for the question it belongs to, if it changed.
    fn flush_note(&mut self) {
        let Some(id) = self.note_draft_for.clone() else {
            return;
        };
        let stored = self.user.note(&id).unwrap_or_default().to_string();
        if self.note_draft.trim() != stored.trim() {
            self.user.set_note(&id, &self.note_draft, &now_iso());
            self.persist_user();
        }
    }

    fn build_context(&self) -> CurrentContext {
        let mut c = CurrentContext {
            ts: now_iso(),
            domain: self.domain.id().to_string(),
            tab: "practice".to_string(),
            subject_id: self.subject_id.clone(),
            subject_name: self.subject_name.clone(),
            question: None,
            knowledge_points: Vec::new(),
        };

        let Some(subject) = &self.subject else {
            return c;
        };

        if let Some(q) = self.session_question() {
            let mut sq = state_from_question(&q);
            sq.note = self.user.note(&q.id).map(str::to_owned);
            sq.favorite = self.user.is_favorite(&q.id);
            c.question = Some(sq);
            for title in &q.knowledge_points {
                if let Some(&i) = self.kp_title_map.get(title) {
                    if let Some(kp) = subject.knowledge_points.get(i) {
                        c.knowledge_points.push(state_from_kp(kp));
                    }
                }
            }
        }
        c
    }

    fn flush_state(&mut self) {
        let ctx = self.build_context();
        if let Err(e) = write_current(&self.state_dir, &ctx) {
            diag::log(&self.state_dir, &format!("写入 current.json 失败: {e}"));
        }
    }

    // ------------------------------------------------------------------
    // frosted glass: capture + Gaussian blur
    // ------------------------------------------------------------------

    /// Keeps the frosted backdrop current without ever re-photographing the
    /// desktop because the window moved: the shot is a whole-monitor image and
    /// its UV rect follows the window, so a move / resize only changes which
    /// part of it is drawn (see [`App::glass_uv`]). What is left is the first
    /// capture, a 模糊度 change (re-blur the stored pixels) and a deliberate
    /// 重新采样背景. A capture that fails keeps the previous texture; with no
    /// texture at all, `glass::backdrop` paints the gradient fallback, so the
    /// window is never unreadable.
    fn refresh_glass(&mut self, ctx: &egui::Context) {
        // Geometry bookkeeping, still needed — but only to repaint with the new
        // UV rect. A move / resize re-samples the stored shot (see
        // [`App::glass_uv`]); it never captures.
        let outer = ctx.input(|i| i.viewport().outer_rect);
        if outer != self.glass_geom {
            self.glass_geom = outer;
            ctx.request_repaint();
        }

        let blur_changed = self.glass_blur_at_render != Some(self.user.glass_blur);
        let work = glass_work(
            self.glass_capture_attempted,
            self.glass_recapture,
            self.glass_shot.is_some(),
            blur_changed,
        );
        match work {
            GlassWork::Capture => {
                self.glass_recapture = false;
                self.glass_capture_attempted = true;
                self.capture_glass(ctx);
            }
            GlassWork::Reblur => self.render_glass(ctx),
            GlassWork::None => {}
        }
    }

    /// One capture → store the shot → downscale → Gaussian → texture. The shot
    /// stays in the state so later re-blurs never touch the screen again. The
    /// attempt is recorded even on failure, so a failing capture is not
    /// retried every frame (only the first frame, 重新采样背景 or a re-blur of
    /// an existing shot come back here).
    fn capture_glass(&mut self, ctx: &egui::Context) {
        self.glass_captures = self.glass_captures.saturating_add(1);
        let n = self.glass_captures;
        let Some(hwnd) = self.hwnd else {
            diag::log(&self.state_dir, &format!("glass: capture #{n} skipped (no Win32 handle)"));
            return;
        };
        match glass::blur::capture(hwnd) {
            Some((shot, hide)) => {
                diag::log(
                    &self.state_dir,
                    &format!(
                        "glass: capture #{n} ok (hide={} affinity_ret={})",
                        hide.id(),
                        hide.affinity_ret()
                    ),
                );
                self.glass_hide = Some(hide);
                self.glass_shot = Some(shot);
                self.render_glass(ctx);
            }
            None => diag::log(
                &self.state_dir,
                &format!("glass: capture #{n} failed, keeping the previous shot"),
            ),
        }
    }

    /// Downscale + Gaussian + texture upload from the stored capture. Runs on
    /// the first capture and whenever 模糊度 changes; no screen access.
    fn render_glass(&mut self, ctx: &egui::Context) {
        let Some(shot) = self.glass_shot.as_ref() else {
            return;
        };
        let (halvings, sigma) = glass::blur::params(self.user.glass_blur);
        let (w, h, rgb) = glass::blur::downscale(shot, halvings);
        let blurred = glass::blur::gaussian(&rgb, w, h, sigma);
        let image = glass::blur::to_color_image(&blurred, w, h);

        self.glass_origin = shot.origin;
        self.glass_size = (shot.width, shot.height);
        self.glass_blur_at_render = Some(self.user.glass_blur);
        match &mut self.glass_texture {
            Some(handle) => handle.set(image, egui::TextureOptions::LINEAR),
            None => {
                self.glass_texture = Some(ctx.load_texture(
                    "glass_desktop",
                    image,
                    egui::TextureOptions::LINEAR,
                ));
            }
        }
    }

    /// UV rect of the window's content area inside the captured texture.
    ///
    /// The content rect (egui points, origin at the window's client area) is
    /// converted to physical screen pixels and offset by the capture origin,
    /// so the blurred desktop stays registered with the real desktop while the
    /// window moves instead of sliding along with it. That is exactly why a
    /// move / resize needs no new capture: it only changes this rect.
    fn glass_uv(&self, ctx: &egui::Context) -> egui::Rect {
        let inner = ctx
            .input(|i| i.viewport().inner_rect)
            .map_or(egui::Pos2::ZERO, |r| r.min);
        uv_in_capture(
            inner.to_vec2(),
            ctx.pixels_per_point(),
            ctx.content_rect().size(),
            egui::vec2(self.glass_origin.0 as f32, self.glass_origin.1 as f32),
            egui::vec2(self.glass_size.0 as f32, self.glass_size.1 as f32),
        )
    }
}

/// See [`App::glass_uv`]. Pure: `inner_min` and `content_size` are in egui
/// points (window-relative for the size, screen-relative for the position),
/// `ppp` converts them to physical pixels, and `origin` / `size` describe the
/// capture in physical pixels.
fn uv_in_capture(
    inner_min: egui::Vec2,
    ppp: f32,
    content_size: egui::Vec2,
    origin: egui::Vec2,
    size: egui::Vec2,
) -> egui::Rect {
    let size = egui::vec2(size.x.max(1.0), size.y.max(1.0));
    let min = (inner_min * ppp - origin) / size;
    let extent = content_size * ppp / size;
    egui::Rect::from_min_size(min.to_pos2(), extent)
}

impl eframe::App for App {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);

        // First, so the probe can report the real state and the first painted
        // frame already shows the captured desktop.
        self.refresh_glass(&ctx);

        if let Some(report) = self.probe_report.take() {
            let glass = match (&self.glass_texture, self.glass_hide) {
                (Some(_), Some(hide)) => format!(
                    "captured ({}, affinity_ret={})",
                    hide.id(),
                    hide.affinity_ret()
                ),
                _ => "fallback".to_string(),
            };
            diag::log(&self.state_dir, &format!("probe {report} glass={glass}"));
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        if self.anim.is_live(now) {
            self.bg_phase += ctx.input(|i| i.stable_dt) * 0.35;
        }
        let desktop = self.glass_texture.as_ref().map(|texture| glass::DesktopGlass {
            texture,
            uv: self.glass_uv(&ctx),
        });
        glass::backdrop(
            &ctx,
            &self.tokens,
            self.bg_phase,
            desktop,
            self.user.glass_transparency,
        );
        win::chrome(&ctx);
        self.handle_keys(&ctx, now);

        self.header(ui);
        if self.session.is_none() {
            self.rail(ui);
        }
        self.workspace(ui);

        if self.settings_open {
            self.settings_modal(&ctx);
        }

        self.anim.prune(now);
        if self.anim.is_live(now) {
            ctx.request_repaint();
        }
        if self.dirty {
            self.flush_state();
            self.dirty = false;
        }
    }
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

/// Subject groups that are not listed anywhere in the UI. 综合阶段 holds a
/// single subject with 0 questions (职业能力综合测试); 其他 (未分类 / 已排除)
/// stays visible.
const HIDDEN_GROUPS: [&str; 1] = ["综合阶段"];

/// The rail group a subject belongs to, or `None` when it is hidden
/// ([`HIDDEN_GROUPS`]); an absent or blank group falls back to 其他.
fn group_of(s: &SummarySubject) -> Option<String> {
    let key = s
        .group
        .as_deref()
        .map(str::trim)
        .filter(|g| !g.is_empty())
        .unwrap_or("其他")
        .to_string();
    (!HIDDEN_GROUPS.contains(&key.as_str())).then_some(key)
}

fn rand_seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(12345)
}

fn question_key(q: &Question) -> u64 {
    question_key_variant(q, 0)
}

/// [`question_key`] mixed with one extra piece of state, so a variant of the
/// same question (e.g. "at session position N") gets its own motion clock.
fn question_key_variant(q: &Question, extra: u64) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    q.id.hash(&mut hasher);
    extra.hash(&mut hasher);
    hasher.finish()
}

fn state_from_question(q: &Question) -> StateQuestion {
    StateQuestion {
        id: q.id.clone(),
        stem: q.stem.clone(),
        type_: q.qtype.clone(),
        type_raw: q.type_raw.clone(),
        chapter: q.chapter.clone(),
        year: if q.year.is_null() {
            None
        } else {
            Some(q.year.clone())
        },
        options: q
            .options
            .iter()
            .map(|o| StateOption {
                label: o.label.clone(),
                text: o.text.clone(),
            })
            .collect(),
        answer: q.answer.clone(),
        answer_text: q.answer_text.clone(),
        explanation: q.explanation.clone(),
        knowledge_points: q.knowledge_points.clone(),
        note: None,
        favorite: false,
        source: q.source.as_ref().map(|s| s.to_state()).unwrap_or_default(),
    }
}

fn state_from_kp(kp: &KnowledgePoint) -> StateKp {
    StateKp {
        id: kp.id.clone(),
        title: kp.title.clone(),
        chapter: kp.chapter.clone(),
        content: kp.content.clone(),
        mnemonic: kp.mnemonic.clone(),
        summary: kp.summary.clone(),
        source: kp.source.as_ref().map(|s| s.to_state()).unwrap_or_default(),
    }
}

fn load_domain_data(root: &Path) -> DomainData {
    match sfzklearning::list_subjects(root) {
        Ok(subjects) => DomainData {
            subjects,
            error: None,
        },
        Err(e) => DomainData {
            subjects: Vec::new(),
            error: Some(e),
        },
    }
}

/// Install a CJK font from `C:\Windows\Fonts` so Chinese renders.
/// Returns a warning when none was found.
fn install_fonts(ctx: &egui::Context) -> Option<String> {
    let candidates: [(&str, u32); 5] = [
        (r"C:\Windows\Fonts\msyh.ttc", 0),
        (r"C:\Windows\Fonts\msyhbd.ttc", 0),
        (r"C:\Windows\Fonts\simhei.ttf", 0),
        (r"C:\Windows\Fonts\Deng.ttf", 0),
        (r"C:\Windows\Fonts\simsun.ttc", 0),
    ];

    for (path, index) in candidates {
        if let Ok(bytes) = std::fs::read(path) {
            let mut fd = egui::FontData::from_owned(bytes);
            fd.index = index;
            let mut fonts = egui::FontDefinitions::default();
            fonts.font_data.insert("cjk".to_owned(), Arc::new(fd));
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "cjk".to_owned());
            fonts
                .families
                .entry(egui::FontFamily::Monospace)
                .or_default()
                .insert(0, "cjk".to_owned());
            ctx.set_fonts(fonts);
            return None;
        }
    }

    Some("警告：未找到 CJK 字体，中文可能无法显示。".to_string())
}

pub fn run() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(APP_TITLE)
            .with_inner_size([1320.0, 880.0])
            .with_min_inner_size([900.0, 600.0])
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(true),
        // `Glow` (OpenGL) rather than the default `Wgpu`: wgpu reports only
        // `CompositeAlphaMode::Opaque` for a plain Win32 surface, so a wgpu
        // window can never be translucent — and transparency is still needed
        // for the rounded corners (the captured desktop itself is painted by
        // the app, see `glass::blur`).
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };

    eframe::run_native(
        APP_TITLE,
        native_options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}

#[cfg(test)]
mod tests {
    use super::{glass_work, logic, uv_in_capture, GlassWork, Session};
    use sfzklearning::{OptionItem, Question};

    /// A one-question session with a picked answer, ready to be revealed.
    fn choice_session() -> Session {
        Session {
            order: vec![0],
            pos: 0,
            single: Some("A".to_string()),
            multi: Vec::new(),
            submitted: false,
            shown: false,
            self_assessed: false,
            answered: 0,
            correct: 0,
            label: "测试".to_string(),
        }
    }

    fn choice_question() -> Question {
        Question {
            id: "q1".to_string(),
            qtype: Some("single_choice".to_string()),
            options: vec![OptionItem {
                label: "A".to_string(),
                text: "选项".to_string(),
            }],
            answer: vec!["A".to_string()],
            ..Default::default()
        }
    }

    /// 显示答案 opens the 下一题 gate without recording anything: no
    /// `answered` / `correct` (and with them, no per-question stats and no
    /// wrong-question entry) — the question stays 未做过 and comes up again.
    #[test]
    fn revealing_the_answer_records_nothing_and_opens_the_next_gate() {
        let q = choice_question();
        let mut s = choice_session();
        assert!(
            !logic::can_advance(&q, s.submitted, s.shown, s.self_assessed),
            "nothing is judged yet: 下一题 must refuse"
        );

        s.reveal();
        assert!(s.shown, "显示答案 reveals the answer");
        assert!(!s.submitted, "and does not grade it");
        assert_eq!(s.answered, 0, "revealing must not count as an answer");
        assert_eq!(s.correct, 0);
        assert!(
            logic::can_advance(&q, s.submitted, s.shown, s.self_assessed),
            "a revealed question may move on"
        );

        s.clear_answer();
        assert!(!s.shown, "changing the question clears the reveal");
        assert!(!s.self_assessed);
    }

    /// The capture policy, which is where the flicker came from: a move /
    /// resize must never capture again — the stored whole-monitor shot is
    /// re-registered to the window's new position by its UV rect — and a 模糊度
    /// change re-blurs the stored pixels instead of photographing the screen.
    /// Only the first capture and a deliberate 重新采样背景 touch the screen.
    #[test]
    fn glass_captures_once_and_reblurs_instead_of_recapturing() {
        // Startup: nothing captured yet.
        assert_eq!(glass_work(false, false, false, false), GlassWork::Capture);

        // Settled and nothing changed.
        assert_eq!(glass_work(true, false, true, false), GlassWork::None);
        // The window moved or resized: the stored shot stays valid, so the
        // capture must NOT run again (this is the flash).
        assert_eq!(glass_work(true, false, true, false), GlassWork::None);
        // 模糊度 moved: re-blur the stored pixels, never a new capture.
        assert_eq!(glass_work(true, false, true, true), GlassWork::Reblur);
        // 重新采样背景: the one deliberate capture.
        assert_eq!(glass_work(true, true, true, false), GlassWork::Capture);
        assert_eq!(glass_work(true, true, true, true), GlassWork::Capture);
        // A failed capture is not retried every frame...
        assert_eq!(glass_work(true, false, false, true), GlassWork::None);
        // ...but a recapture without a stored shot captures again.
        assert_eq!(glass_work(true, true, false, false), GlassWork::Capture);
    }

    /// Moving the window re-registers the stored capture instead of replacing
    /// it: the same texture is sampled from a different offset, so the blurred
    /// desktop stays glued to the desktop and the crop does not slide along
    /// with the window.
    #[test]
    fn the_stored_capture_follows_the_window_without_a_new_capture() {
        let capture = egui::vec2(1920.0, 1080.0);
        let content = egui::vec2(1320.0, 880.0);
        let at = |x: f32| {
            uv_in_capture(egui::vec2(x, 60.0), 1.0, content, egui::Vec2::ZERO, capture)
        };

        let a = at(300.0);
        let b = at(500.0);
        assert!((a.min.x - 300.0 / 1920.0).abs() < 1e-6, "crop at the window's own position");
        assert!(
            (b.min.x - a.min.x - 200.0 / 1920.0).abs() < 1e-6,
            "moving 200px samples 200px further into the desktop"
        );
        assert_eq!(a.size(), b.size(), "a move does not change the crop size");
        assert!(
            (a.width() - content.x / capture.x).abs() < 1e-6,
            "the crop is the content area in capture UV"
        );

        // High-DPI: the window rect is in points, the capture in physical px.
        let hidpi =
            uv_in_capture(egui::vec2(300.0, 60.0), 2.0, content, egui::Vec2::ZERO, capture);
        assert!((hidpi.min.x - 600.0 / 1920.0).abs() < 1e-6);

        // A degenerate capture size must not produce NaN.
        let degenerate =
            uv_in_capture(egui::Vec2::ZERO, 1.0, content, egui::Vec2::ZERO, egui::Vec2::ZERO);
        assert!(degenerate.min.x.is_finite() && degenerate.width().is_finite());
    }
}
