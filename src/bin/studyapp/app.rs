use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui;
use egui::RichText;
use egui_commonmark::{CommonMarkCache, CommonMarkViewer};

pub(crate) use crate::{anim, glass, logic, theme, win};

use sfzklearning::{
    domain_root, load_subject, load_user_store, load_wrong_ids, now_iso, parse_cli_args,
    resolve_state_dir, save_user_store, save_wrong_ids, write_current, CurrentContext, Domain,
    KnowledgePoint, Question, StateKp, StateOption, StateQuestion, SubjectFile, SummarySubject,
    UserStore,
};

mod launcher;
mod practice;
mod settings;
mod shell;

const APP_TITLE: &str = "sfzklearning";

#[derive(PartialEq, Eq, Clone, Copy)]
enum Screen {
    Launcher,
    Workspace,
}

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
    submitted: bool,
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

    screen: Screen,
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
    /// True when DWM accepted the acrylic backdrop, i.e. the window really shows
    /// the desktop behind it and must therefore paint only a flat scrim.
    acrylic: bool,
    probe_report: Option<String>,
    md_cache: CommonMarkCache,
    dirty: bool,
}

impl App {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let args = parse_cli_args();
        let law_root = domain_root(Domain::Law, &args);
        let cpa_root = domain_root(Domain::Cpa, &args);
        let state_dir = resolve_state_dir(args.state_dir.as_deref());
        let user = load_user_store(&state_dir);

        if let Some(w) = install_fonts(&cc.egui_ctx) {
            eprintln!("[sfzklearning] {w}");
        }
        let mode = theme::Mode::from_id(&user.theme);
        let tokens = theme::install(&cc.egui_ctx, mode);

        let hwnd = win::window_hwnd(cc);
        let acrylic = match win::apply_acrylic(cc, mode) {
            Ok(()) => true,
            Err(e) => {
                eprintln!("[sfzklearning] 系统毛玻璃不可用，回退为不透明背景: {e}");
                false
            }
        };
        let probe_report = std::env::args()
            .any(|a| a == "--probe")
            .then(|| win::probe(cc));

        let law = load_domain_data(&law_root);
        let cpa = load_domain_data(&cpa_root);
        for (label, data) in [("law", &law), ("cpa", &cpa)] {
            if let Some(err) = &data.error {
                eprintln!("[sfzklearning] {label} 数据加载失败: {err}");
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
            screen: Screen::Workspace,
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
            acrylic,
            probe_report,
            md_cache: CommonMarkCache::default(),
            dirty: true,
        };

        // First run (or a forced mode) shows the launcher; otherwise go straight in.
        if app.user.last_mode.is_none() || !forced.is_empty() {
            app.screen = Screen::Launcher;
        }

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
        self.screen = Screen::Workspace;
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
            );
            let chapters = if self.set_chapters.is_empty() {
                "全部章节".to_string()
            } else {
                format!("{} 章", self.set_chapters.len())
            };
            let tail = match self.set_draw.limit() {
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

    fn self_assess(&mut self, remembered: bool, _now: f64) {
        let Some(q) = self.session_question() else {
            return;
        };
        let qid = q.id.clone();
        if let Some(sess) = self.session.as_mut() {
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
            } else if self.screen == Screen::Workspace {
                self.screen = Screen::Launcher;
            }
            return;
        }

        if self.session.is_some() {
            if left {
                self.prev_question();
            }
            if right {
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
            eprintln!("[sfzklearning] 写入错题文件失败: {e}");
        }
    }

    fn persist_user(&self) {
        if let Err(e) = save_user_store(&self.state_dir, &self.user) {
            eprintln!("[sfzklearning] 写入 user.json 失败: {e}");
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
            eprintln!("[sfzklearning] 写入 current.json 失败: {e}");
        }
    }
}

impl eframe::App for App {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);

        if let Some(report) = self.probe_report.take() {
            eprintln!("[sfzklearning][probe] acrylic={} {report}", self.acrylic);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        if self.anim.is_live(now) {
            self.bg_phase += ctx.input(|i| i.stable_dt) * 0.35;
        }
        glass::backdrop(&ctx, &self.tokens, self.bg_phase, !self.acrylic);
        win::chrome(&ctx);
        self.handle_keys(&ctx, now);

        match self.screen {
            Screen::Launcher => self.launcher(ui),
            Screen::Workspace => {
                self.header(ui);
                if self.session.is_none() {
                    self.rail(ui);
                }
                self.workspace(ui);
            }
        }

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
        ..Default::default()
    };

    eframe::run_native(
        APP_TITLE,
        native_options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}
