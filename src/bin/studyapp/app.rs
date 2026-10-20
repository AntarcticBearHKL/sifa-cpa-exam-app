use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui;
use egui::RichText;
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};

pub(crate) use crate::{anim, diag, glass, logic, theme, win};

use sfzklearning::{
    domain_root, load_subject, load_user_store, load_wrong_ids, now_iso, now_unix, parse_cli_args,
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
    /// 主观题 it reveals the 参考答案 / 解析, and there is nothing else to do.
    shown: bool,
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
    /// 收藏 condition: 不限 / 已收藏 / 未收藏.
    set_fav: logic::Fav,
    /// 正确率 condition: 不限 / 未作答 / 低于 60% / 低于 100% / 全对.
    set_accuracy: logic::Accuracy,
    set_draw: logic::Draw,
    /// Numeric value used when `set_draw` is `Draw::Custom`; always kept in
    /// the valid range by the input and by `Draw::limit`.
    set_draw_custom: usize,
    set_order: logic::Order,
    set_chapter_filter: String,
    set_query: String,
    session: Option<Session>,

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
    /// The window's **content area** in physical screen pixels as last seen
    /// (Win32 `GetClientRect` + `ClientToScreen`), so a move / resize repaints
    /// the backdrop with its new UV rect. The rect is in the same physical
    /// pixel space as the capture, so no `pixels_per_point` conversion is
    /// involved — that is what lets the crop stay correct across monitors with
    /// different DPI scales.
    glass_content_px: Option<egui::Rect>,
    /// The whole-virtual-screen capture the texture was built from. Kept so a
    /// 模糊度 change re-blurs these pixels instead of photographing the screen
    /// again. It is a *whole-screen* shot and the UV rect is derived from the
    /// window's physical screen rect, so it stays registered with the desktop
    /// while the window moves — which is why a move / resize never needs a
    /// capture.
    glass_shot: Option<glass::blur::Shot>,
    /// Which mechanism kept the window out of the last capture; reported by
    /// `--probe` and written to `log.txt`.
    glass_hide: Option<glass::blur::Hide>,
    /// Top-left of the capture, in physical screen pixels: the virtual
    /// screen's origin, negative when a monitor sits left of / above the
    /// primary.
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
    /// When the next timer capture is due (`InputState::time` seconds); the
    /// desktop is re-photographed every [`GLASS_INTERVAL`] while idle. See
    /// [`glass_work`].
    glass_next_capture: f64,
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

/// How often the frosted backdrop re-photographs the desktop while the app is
/// running: **every 0.5 s**, so a change on the desktop (another window moved,
/// a video, a slideshow) shows up without the user pressing 重新采样背景. The
/// window no longer has to hide itself for the capture — see
/// `glass::blur::Hide` — so re-capturing cannot flash, which is what made this
/// timer impossible before.
const GLASS_INTERVAL: f64 = 0.5;

/// Capture rule for the frosted backdrop, in one pure place so the tests can
/// pin it down: capture **once at startup**, **on the 0.5 s timer**
/// ([`GLASS_INTERVAL`]) and when **重新采样背景** asks for it — but **never**
/// because the window moved or resized. The stored shot is a whole-monitor
/// image whose UV rect follows the window, so it stays registered to the
/// desktop while the window moves and only goes stale when *other windows*
/// change. A timer tick that lands while a move / resize drag is in progress
/// is skipped (`dragging`), so dragging stays cheap and the "a move / resize
/// does not capture" rule is not fought. A 模糊度 change re-blurs the stored
/// pixels (no screen access); 透明度 never comes near the capture.
fn glass_work(
    capture_attempted: bool,
    recapture: bool,
    timer_due: bool,
    dragging: bool,
    has_shot: bool,
    blur_changed: bool,
) -> GlassWork {
    if recapture || !capture_attempted {
        GlassWork::Capture
    } else if has_shot && blur_changed {
        GlassWork::Reblur
    } else if timer_due && !dragging {
        GlassWork::Capture
    } else {
        GlassWork::None
    }
}

/// True while a window move / resize drag is in progress: egui keeps the
/// primary button down for the whole native drag (no release event arrives
/// until it ends), so a timer capture that lands mid-drag can be skipped.
fn dragging(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.pointer.primary_down())
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
            set_fav: logic::Fav::default(),
            set_accuracy: logic::Accuracy::default(),
            set_draw: logic::Draw::default(),
            set_draw_custom: logic::DEFAULT_CUSTOM_DRAW,
            set_order: logic::Order::default(),
            set_chapter_filter: String::new(),
            set_query: String::new(),
            session: None,
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
            glass_content_px: None,
            glass_capture_attempted: false,
            glass_recapture: false,
            glass_next_capture: 0.0,
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

        // Test hook: `CMF_START_KIND` preselects the 题型 (`choice` /
        // `subjective`; anything else keeps 全部) before a session is started,
        // so the display-only 主观题 view can be reached directly. Used
        // together with `CMF_START_SESSION` because synthesised clicks are
        // unreliable on this machine.
        if let Ok(kind) = std::env::var("CMF_START_KIND") {
            app.set_kind = match kind.trim().to_ascii_lowercase().as_str() {
                "choice" => logic::Kind::Choice,
                "subjective" => logic::Kind::Subjective,
                _ => logic::Kind::All,
            };
        }

        // Test hook: `CMF_START_SESSION=1` opens a practice session for the
        // starting subject right away, so the session view can be reached
        // (and screenshotted) without synthesised clicks. Any value but an
        // empty one or `0` enables it.
        let start_session = std::env::var("CMF_START_SESSION")
            .map(|v| {
                let v = v.trim();
                !v.is_empty() && v != "0"
            })
            .unwrap_or(false);
        if start_session {
            app.start_session(false);
        }
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
        self.set_fav = logic::Fav::default();
        self.set_accuracy = logic::Accuracy::default();
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

                // `wrong-*.json` is a derived projection of the per-question
                // stats now: prune ids that do not satisfy `logic::is_wrong`
                // (an older build could insert one without recording an
                // attempt — a question that was only *viewed* ended up as
                // 错题) and write the file back, so those stale entries
                // disappear. The stats stay the only truth; the file is never
                // consulted for filtering.
                prune_wrong_file(&self.state_dir, self.domain, &self.subject_id, &self.user);
                self.subject = Some(s);
            }
            Err(e) => {
                self.subject_error = Some(e);
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
            fav: self.set_fav,
            accuracy: self.set_accuracy,
            query: self.set_query.clone(),
        }
    }

    fn matching_questions(&self) -> Vec<usize> {
        let Some(s) = &self.subject else {
            return Vec::new();
        };
        logic::select(&s.questions, &self.filter(), &self.user)
    }

    fn start_session(&mut self, wrong_random: bool) {
        let Some(s) = self.subject.clone() else {
            return;
        };
        let (order, label) = if wrong_random {
            let idx = logic::wrong_sample(&s.questions, &self.user, 10, rand_seed());
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
            answered: 0,
            correct: 0,
            label,
        });
        self.dirty = true;
        // The first question of the run is shown right away, so it counts as
        // seen here — the same place every later move is counted.
        self.record_current_seen();
    }

    fn session_question(&self) -> Option<Question> {
        let s = self.subject.clone()?;
        let sess = self.session.as_ref()?;
        let qi = *sess.order.get(sess.pos)?;
        s.questions.get(qi).cloned()
    }

    /// Counts the current question as **seen**. Called exactly where the
    /// session moves to a question — entering a session, 下一题, 上一题 — never
    /// per frame, so a re-render does not count and re-showing the same
    /// question counts again. Writes `user.json` immediately, the same way
    /// `record_answer` does; the answer path is never touched.
    fn record_current_seen(&mut self) {
        let Some(q) = self.session_question() else {
            return;
        };
        self.user.record_seen(&q.id, now_unix());
        self.persist_user();
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
        if !ok {
            self.anim.trigger_shake(question_key(&q), now);
        }
        self.reveal_epoch = self.reveal_epoch.wrapping_add(1);
        self.user.record_answer(&qid, ok, now_unix());
        // The wrong file is a projection of the stats, which were just
        // updated: it is rewritten from them, never edited by hand.
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

    /// Is the current question judged, so 下一题 / `→` may move on? See
    /// [`logic::can_advance`]. A choice question needs 提交答案 or 显示答案; a
    /// 主观题 has nothing to judge and is never gated. `←` 上一题 and `Esc`
    /// stay available, so the gate is never a trap.
    fn can_advance_now(&self) -> bool {
        let (Some(q), Some(sess)) = (self.session_question(), self.session.as_ref()) else {
            return false;
        };
        logic::can_advance(&q, sess.submitted, sess.shown)
    }

    fn next_question(&mut self) {
        if let Some(sess) = self.session.as_mut() {
            if !sess.order.is_empty() {
                sess.pos = (sess.pos + 1) % sess.order.len();
            }
            sess.clear_answer();
        }
        self.dirty = true;
        self.record_current_seen();
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
        self.record_current_seen();
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

    /// The 错题 set of the current subject, **derived from the per-question
    /// stats** — the single source of truth (see [`logic::is_wrong`]).
    /// `wrong-<domain>-<subject>.json` is only a projection of this for
    /// tooling that reads it; the app never filters by the file.
    fn derived_wrong_ids(&self) -> Vec<String> {
        let Some(s) = &self.subject else {
            return Vec::new();
        };
        s.questions
            .iter()
            .filter(|q| logic::is_wrong(self.user.stat(&q.id)))
            .map(|q| q.id.clone())
            .collect()
    }

    /// How many questions of the current subject are 错题, derived the same
    /// way — the number behind the practice panel's 错题随机 button.
    fn wrong_count(&self) -> usize {
        self.subject
            .as_ref()
            .map(|s| {
                s.questions
                    .iter()
                    .filter(|q| logic::is_wrong(self.user.stat(&q.id)))
                    .count()
            })
            .unwrap_or(0)
    }

    fn persist_wrong(&self) {
        let ids = self.derived_wrong_ids();
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
    /// desktop because the window moved: the shot is a whole-virtual-screen
    /// image and its UV rect follows the window, so a move / resize only
    /// changes which part of it is drawn (see [`App::glass_uv`]). What is left
    /// is the first capture, the **0.5 s timer** ([`GLASS_INTERVAL`]), a 模糊度
    /// change (re-blur the stored pixels) and a deliberate 重新采样背景. A
    /// capture that fails keeps the previous texture; with no texture at all,
    /// `glass::backdrop` paints the gradient fallback, so the window is never
    /// unreadable.
    fn refresh_glass(&mut self, ctx: &egui::Context) {
        // Geometry bookkeeping, still needed — but only to repaint with the new
        // UV rect. The rect comes straight from Win32 in physical pixels (the
        // content area, where the texture is drawn): a move / resize re-samples
        // the stored shot (see [`App::glass_uv`]); it never captures. A failed
        // read keeps the last known rect, like a failed capture keeps the last
        // shot.
        if let Some(rect) = self.hwnd.and_then(win::content_rect) {
            if Some(rect) != self.glass_content_px {
                self.glass_content_px = Some(rect);
                ctx.request_repaint();
            }
        }

        let now = ctx.input(|i| i.time);
        let blur_changed = self.glass_blur_at_render != Some(self.user.glass_blur);
        let timer_due = now >= self.glass_next_capture;
        let work = glass_work(
            self.glass_capture_attempted,
            self.glass_recapture,
            timer_due,
            dragging(ctx),
            self.glass_shot.is_some(),
            blur_changed,
        );
        match work {
            GlassWork::Capture => {
                self.glass_recapture = false;
                self.glass_capture_attempted = true;
                self.glass_next_capture = now + GLASS_INTERVAL;
                self.capture_glass(ctx);
            }
            GlassWork::Reblur => self.render_glass(ctx),
            GlassWork::None => {}
        }
        // Wake again when the next timer capture is due, so the backdrop keeps
        // re-sampling even while the app is otherwise idle. A drag defers the
        // capture (see `glass_work`) but does not stop the clock.
        if self.hwnd.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(GLASS_INTERVAL));
        }
    }

    /// One capture → store the shot → downscale → Gaussian → texture. The shot
    /// stays in the state so later re-blurs never touch the screen again. The
    /// attempt is recorded even on failure, so a failing capture is not retried
    /// every frame — the next attempt comes from the 0.5 s timer, 重新采样背景
    /// or a re-blur of an existing shot.
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
                        "glass: capture #{n} ok (hide={} affinity_ret={} origin={},{} size={}x{})",
                        hide.id(),
                        hide.affinity_ret(),
                        shot.origin.0,
                        shot.origin.1,
                        shot.width,
                        shot.height
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
    /// The content rect is named in **physical screen pixels by Win32**
    /// ([`win::content_rect`]), not by scaling egui points with
    /// `pixels_per_point`: once the virtual screen spans monitors with
    /// different DPI scales, one scale factor cannot name a position on all of
    /// them. The capture is the whole virtual screen in the same physical
    /// pixel space, so `(content_px - origin) / capture_size` places the crop
    /// with no DPI conversion at all. That is exactly why a move / resize
    /// needs no new capture: it only changes this rect.
    fn glass_uv(&self) -> egui::Rect {
        uv_in_capture(
            self.glass_content_px.unwrap_or(egui::Rect::ZERO),
            egui::vec2(self.glass_origin.0 as f32, self.glass_origin.1 as f32),
            egui::vec2(self.glass_size.0 as f32, self.glass_size.1 as f32),
        )
    }
}

/// See [`App::glass_uv`]. Pure: `content_px` is the window's content area in
/// physical screen pixels (straight from Win32), and `origin` / `size`
/// describe the capture in the same physical pixel space.
fn uv_in_capture(
    content_px: egui::Rect,
    origin: egui::Vec2,
    size: egui::Vec2,
) -> egui::Rect {
    let size = egui::vec2(size.x.max(1.0), size.y.max(1.0));
    let min = (content_px.min.to_vec2() - origin) / size;
    let extent = content_px.size() / size;
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
            uv: self.glass_uv(),
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

/// Prunes `wrong-<domain>-<subject>.json` to the ids that actually satisfy
/// [`logic::is_wrong`] per the per-question stats, and writes it back when
/// anything was dropped. Returns the pruned ids.
///
/// The file exists only because tooling reads it — the stats in `user.json`
/// are the single source of truth — so this is where a stale file written by
/// an older build (which could insert an id without recording an attempt, so
/// a question that was only *viewed* showed up as 错题) gets corrected.
fn prune_wrong_file(
    state_dir: &Path,
    domain: Domain,
    subject_id: &str,
    user: &UserStore,
) -> Vec<String> {
    let stored = load_wrong_ids(state_dir, domain, subject_id);
    let kept: Vec<String> = stored
        .iter()
        .filter(|id| logic::is_wrong(user.stat(id)))
        .cloned()
        .collect();
    if kept != stored {
        if let Err(e) = save_wrong_ids(state_dir, domain, subject_id, &kept) {
            diag::log(state_dir, &format!("修剪错题文件失败: {e}"));
        }
    }
    kept
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
    use super::{glass_work, logic, prune_wrong_file, uv_in_capture, GlassWork, Session};
    use sfzklearning::{
        load_wrong_ids, save_wrong_ids, Domain, OptionItem, Question, UserStore,
    };

    /// A one-question session with a picked answer, ready to be revealed.
    fn choice_session() -> Session {
        Session {
            order: vec![0],
            pos: 0,
            single: Some("A".to_string()),
            multi: Vec::new(),
            submitted: false,
            shown: false,
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
            !logic::can_advance(&q, s.submitted, s.shown),
            "nothing is judged yet: 下一题 must refuse"
        );

        s.reveal();
        assert!(s.shown, "显示答案 reveals the answer");
        assert!(!s.submitted, "and does not grade it");
        assert_eq!(s.answered, 0, "revealing must not count as an answer");
        assert_eq!(s.correct, 0);
        assert!(
            logic::can_advance(&q, s.submitted, s.shown),
            "a revealed question may move on"
        );

        s.clear_answer();
        assert!(!s.shown, "changing the question clears the reveal");
    }

    /// The capture policy. A move / resize must never capture — the stored
    /// whole-monitor shot is re-registered to the window's new position by its
    /// UV rect — and a 模糊度 change re-blurs the stored pixels instead of
    /// photographing the screen. Startup, a deliberate 重新采样背景 and the
    /// 0.5 s timer are the only things that touch the screen.
    #[test]
    fn glass_captures_once_and_reblurs_instead_of_recapturing() {
        // Startup: nothing captured yet.
        assert_eq!(
            glass_work(false, false, false, false, false, false),
            GlassWork::Capture
        );

        // Settled, timer not due, nothing changed.
        assert_eq!(
            glass_work(true, false, false, false, true, false),
            GlassWork::None
        );
        // The window moved or resized: the stored shot stays valid, so the
        // capture must NOT run again (this is the flash).
        assert_eq!(
            glass_work(true, false, false, false, true, false),
            GlassWork::None
        );
        // 模糊度 moved: re-blur the stored pixels, never a new capture.
        assert_eq!(
            glass_work(true, false, false, false, true, true),
            GlassWork::Reblur
        );
        // 重新采样背景: the one deliberate capture.
        assert_eq!(
            glass_work(true, true, false, false, true, false),
            GlassWork::Capture
        );
        assert_eq!(
            glass_work(true, true, false, false, true, true),
            GlassWork::Capture
        );
        // A failed capture is not retried every frame...
        assert_eq!(
            glass_work(true, false, false, false, false, true),
            GlassWork::None
        );
        // ...but a recapture without a stored shot captures again.
        assert_eq!(
            glass_work(true, true, false, false, false, false),
            GlassWork::Capture
        );
    }

    /// The 0.5 s timer: a due tick captures so the backdrop follows other
    /// windows without a manual action, but a tick that lands **mid-drag** is
    /// skipped so a move / resize stays cheap and the "a move / resize does
    /// not capture" rule is not fought. A 模糊度 change still wins — it is a
    /// re-blur, not a screen access.
    #[test]
    fn the_timer_recaptures_but_not_mid_drag() {
        // Timer due, idle: capture.
        assert_eq!(
            glass_work(true, false, true, false, true, false),
            GlassWork::Capture
        );
        // Timer due, mid-drag: skip.
        assert_eq!(
            glass_work(true, false, true, true, true, false),
            GlassWork::None
        );
        // Timer not due, mid-drag: nothing.
        assert_eq!(
            glass_work(true, false, false, true, true, false),
            GlassWork::None
        );
        // 模糊度 change is a re-blur and may run even mid-drag.
        assert_eq!(
            glass_work(true, false, true, true, true, true),
            GlassWork::Reblur
        );
        // 重新采样背景 is an explicit user action: it wins over a drag.
        assert_eq!(
            glass_work(true, true, true, true, true, false),
            GlassWork::Capture
        );
    }

    /// Moving the window re-registers the stored capture instead of replacing
    /// it: the same texture is sampled from a different offset, so the blurred
    /// desktop stays glued to the desktop and the crop does not slide along
    /// with the window. The content rect is physical pixels from Win32, so no
    /// scale factor takes part in the mapping.
    #[test]
    fn the_stored_capture_follows_the_window_without_a_new_capture() {
        let capture = egui::vec2(1920.0, 1080.0);
        let content = egui::vec2(1320.0, 880.0);
        let at = |x: f32| {
            uv_in_capture(
                egui::Rect::from_min_size(egui::pos2(x, 60.0), content),
                egui::Vec2::ZERO,
                capture,
            )
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

        // Registration round trip: the crop maps back to the window's own
        // screen pixels — the invariant the runtime measurement checks.
        assert!((a.min.x * capture.x - 300.0).abs() < 1e-3);

        // A degenerate capture size must not produce NaN.
        let degenerate = uv_in_capture(
            egui::Rect::from_min_size(egui::Pos2::ZERO, content),
            egui::Vec2::ZERO,
            egui::Vec2::ZERO,
        );
        assert!(degenerate.min.x.is_finite() && degenerate.width().is_finite());
    }

    /// One synthetic monitor layout: the virtual screen (`origin` + `size`, in
    /// physical pixels) and a window on one of its monitors.
    struct Layout {
        name: &'static str,
        origin: egui::Vec2,
        size: egui::Vec2,
        window: egui::Rect,
        /// Expected UV of the window's top-left corner in the capture.
        want: egui::Vec2,
    }

    fn layout(
        name: &'static str,
        origin: (f32, f32),
        size: (f32, f32),
        window_min: (f32, f32),
        window_size: (f32, f32),
        want: (f32, f32),
    ) -> Layout {
        Layout {
            name,
            origin: egui::vec2(origin.0, origin.1),
            size: egui::vec2(size.0, size.1),
            window: egui::Rect::from_min_size(
                egui::pos2(window_min.0, window_min.1),
                egui::vec2(window_size.0, window_size.1),
            ),
            want: egui::vec2(want.0, want.1),
        }
    }

    /// Synthetic multi-monitor layouts — the geometry a single-monitor machine
    /// cannot exercise by hand. The capture covers the whole virtual screen,
    /// so a window on the far side of it still has a place in the shot, and
    /// the virtual origin is negative whenever a monitor sits left of / above
    /// the primary.
    #[test]
    fn a_window_on_any_monitor_maps_into_the_virtual_screen_capture() {
        let layouts = [
            // Secondary monitor to the right: the virtual origin stays (0, 0)
            // and the far monitor's top-left is the capture's centre.
            layout(
                "right",
                (0.0, 0.0),
                (3840.0, 1080.0),
                (1920.0, 0.0),
                (1320.0, 880.0),
                (0.5, 0.0),
            ),
            // Secondary monitor to the left: the virtual origin is negative,
            // so the left monitor starts at (0, 0) and the primary at half
            // the capture's width.
            layout(
                "left-secondary",
                (-1920.0, 0.0),
                (3840.0, 1080.0),
                (-1920.0, 0.0),
                (1320.0, 880.0),
                (0.0, 0.0),
            ),
            layout(
                "left-primary",
                (-1920.0, 0.0),
                (3840.0, 1080.0),
                (0.0, 0.0),
                (1320.0, 880.0),
                (0.5, 0.0),
            ),
            // Secondary monitor above: the vertical origin is negative and
            // the primary monitor starts at half the capture's height.
            layout(
                "above-secondary",
                (0.0, -1080.0),
                (1920.0, 2160.0),
                (0.0, -1080.0),
                (1320.0, 880.0),
                (0.0, 0.0),
            ),
            layout(
                "above-primary",
                (0.0, -1080.0),
                (1920.0, 2160.0),
                (0.0, 0.0),
                (1320.0, 880.0),
                (0.0, 0.5),
            ),
            // Mixed / stacked: one monitor above-left and a taller one to the
            // right, offset downward, so the virtual screen is a bounding box
            // none of the monitors matches.
            layout(
                "mixed-right",
                (-1280.0, -1024.0),
                (5760.0, 2664.0),
                (1920.0, 200.0),
                (2560.0, 1440.0),
                (3200.0 / 5760.0, 1224.0 / 2664.0),
            ),
            layout(
                "mixed-above-left",
                (-1280.0, -1024.0),
                (5760.0, 2664.0),
                (-1280.0, -1024.0),
                (1280.0, 1024.0),
                (0.0, 0.0),
            ),
        ];

        for l in &layouts {
            let uv = uv_in_capture(l.window, l.origin, l.size);
            assert!(
                (uv.min.x - l.want.x).abs() < 1e-5 && (uv.min.y - l.want.y).abs() < 1e-5,
                "{}: window at {:?} must sit at UV {:?} of the capture, got {:?}",
                l.name,
                l.window.min,
                l.want,
                uv.min,
            );
            // The crop is exactly the window's own size, in capture UV.
            let want_extent = l.window.size() / l.size;
            assert!(
                (uv.size() - want_extent).length() < 1e-5,
                "{}: the crop must be the window's size in capture UV",
                l.name,
            );
            // Registration round trip: UV back to physical screen pixels is
            // the content area's own top-left again, to the pixel (0 px error).
            let back = uv.min.to_vec2() * l.size + l.origin;
            assert!(
                (back - l.window.min.to_vec2()).length() < 1e-3,
                "{}: UV must map back to the content area's own screen position, got {back:?} vs {:?}",
                l.name,
                l.window.min,
            );
        }
    }

    /// A window that is not on the captured virtual screen (dragged away, or
    /// a stale rect) still yields a finite crop — the GPU clamps sampling;
    /// nothing may become NaN or panic.
    #[test]
    fn a_window_outside_the_capture_stays_finite() {
        let uv = uv_in_capture(
            egui::Rect::from_min_size(egui::pos2(-5000.0, 4000.0), egui::vec2(1320.0, 880.0)),
            egui::Vec2::ZERO,
            egui::vec2(1920.0, 1080.0),
        );
        assert!(uv.min.x.is_finite() && uv.min.y.is_finite());
        assert!(uv.width().is_finite() && uv.height().is_finite());
        assert!(
            uv.min.x < 0.0 && uv.min.y > 1.0,
            "outside the capture means outside the unit square, got {:?}",
            uv.min,
        );
    }

    /// The wrong file is derived from the stats, so loading prunes ids that
    /// were never answered — the bug: a question that was only *viewed* had
    /// been classified as 错题 by an older build — and writes the pruned file
    /// back, while a genuinely wrong id stays.
    #[test]
    fn loading_prunes_unanswered_ids_from_the_wrong_file() {
        let dir = std::env::temp_dir()
            .join(format!("sfzklearning-wrong-prune-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);

        save_wrong_ids(
            &dir,
            Domain::Law,
            "demo",
            &["law:viewed".to_string(), "law:wrong".to_string()],
        )
        .unwrap();
        let mut user = UserStore::default();
        user.record_seen("law:viewed", 0); // viewed only
        user.record_answer("law:wrong", false, 0);

        let kept = prune_wrong_file(&dir, Domain::Law, "demo", &user);
        assert_eq!(
            kept,
            vec!["law:wrong".to_string()],
            "the never-answered id must be pruned, the genuinely wrong one kept"
        );

        let on_disk = load_wrong_ids(&dir, Domain::Law, "demo");
        assert_eq!(
            on_disk,
            vec!["law:wrong".to_string()],
            "the pruned file was written back"
        );
        assert!(
            user.stat("law:viewed")
                .is_some_and(|s| s.attempts == 0 && s.seen == 1),
            "pruning must not touch the stats themselves"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
