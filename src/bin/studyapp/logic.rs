//! Pure selection / practice logic, kept out of the UI so it can be tested
//! without opening a window. Everything here takes plain data and returns
//! plain data; `main.rs` only renders the results.

use std::collections::HashSet;

use sfzklearning::{is_due, mastery, KnowledgePoint, Mastery, Question, QStat, UserStore};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Kind {
    #[default]
    All,
    Choice,
    /// Questions with **no options** (主观题). This is exactly the set 记忆自测
    /// used to select — `Kind::Memory` matched `!q.is_choice()`, i.e. an empty
    /// `options` list — so the two modes were the same set. 主观题 replaces it
    /// and is **display-only**: the session shows the question and one
    /// 显示答案 action, with nothing to answer and nothing to grade.
    Subjective,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::All => "全部",
            Kind::Choice => "选择题",
            Kind::Subjective => "主观题",
        }
    }

    pub fn matches(self, q: &Question) -> bool {
        match self {
            Kind::All => true,
            Kind::Choice => q.is_choice(),
            // `options` empty — identical to `!q.is_choice()`, spelled as the
            // data says it.
            Kind::Subjective => q.options.is_empty(),
        }
    }
}

/// 掌握 filter: 不限 / 未做过 / 薄弱 / 已掌握. Replaces the old 来源 row,
/// whose 仅错题 / 做错过 chips were the same stats predicate; the mastery model
/// already names the two states a learner acts on.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MasteryFilter {
    #[default]
    Any,
    /// Never answered (`attempts == 0`), including questions only viewed.
    Unseen,
    /// Answered at least once, not yet `MASTER_STREAK` correct in a row.
    Weak,
    /// `MASTER_STREAK` correct in a row.
    Mastered,
}

impl MasteryFilter {
    /// Does this condition hold for a question's stats? [`MasteryFilter::Any`]
    /// is a true no-op: it adds no condition at all.
    pub fn matches(self, stat: Option<&QStat>) -> bool {
        match self {
            MasteryFilter::Any => true,
            MasteryFilter::Unseen => stat.map_or(0, |s| s.attempts) == 0,
            MasteryFilter::Weak => mastery(stat) == Mastery::Weak,
            MasteryFilter::Mastered => mastery(stat) == Mastery::Mastered,
        }
    }
}

/// 收藏 filter: 不限 / 已收藏 / 未收藏. Replaces the old 仅收藏 chip, so the
/// favourite condition is expressed in exactly one place.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Fav {
    #[default]
    Any,
    Yes,
    No,
}

/// 正确率 filter: 不限 / 低于 60% / 低于 100% / 全对. 未作答 left this row when
/// 掌握 → 未做过 arrived; two chips for one pool is a trap.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Accuracy {
    #[default]
    Any,
    Below60,
    Below100,
    Perfect,
}

impl Accuracy {
    /// Does this condition hold for a question's stats? [`Accuracy::Any`] is a
    /// true no-op: it adds no condition at all.
    pub fn matches(self, stat: Option<&QStat>) -> bool {
        match self {
            Accuracy::Any => true,
            Accuracy::Below60 => stat.is_some_and(|s| s.attempts > 0 && s.accuracy() < 0.6),
            Accuracy::Below100 => is_wrong(stat),
            Accuracy::Perfect => stat.is_some_and(|s| s.attempts > 0 && s.correct == s.attempts),
        }
    }
}

/// The one 错题 predicate, derived from the per-question stats: a question is
/// 错题 when it has been answered at least once and not every attempt was
/// correct. A question that was only *viewed* (`seen > 0`, `attempts == 0`) is
/// never wrong, and one that was answered correctly every time stops being
/// wrong. The stats in `user.json` are the single source of truth;
/// `wrong-<domain>-<subject>.json` is only a derived projection of this (see
/// `App::persist_wrong`) and is never consulted for filtering.
pub fn is_wrong(stat: Option<&QStat>) -> bool {
    stat.is_some_and(|s| s.attempts > 0 && s.correct < s.attempts)
}

/// Default value shown by the 自定义 number field.
pub const DEFAULT_CUSTOM_DRAW: usize = 20;

/// Upper bound for a custom draw, so a stray input cannot ask for an absurd
/// session size. The available pool is always the real cap anyway.
pub const MAX_CUSTOM_DRAW: usize = 500;

/// How many questions a session may take.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Draw {
    #[default]
    All,
    N10,
    N20,
    N50,
    /// An arbitrary count held separately on `App` (a label cannot carry it).
    Custom,
}

impl Draw {
    pub fn label(self) -> &'static str {
        match self {
            Draw::All => "全部",
            Draw::N10 => "10",
            Draw::N20 => "20",
            Draw::N50 => "50",
            Draw::Custom => "自定义",
        }
    }

    /// The cap `draw_plan` applies. `custom` is only read by `Draw::Custom`
    /// and is clamped so a zero or absurd value cannot slip through.
    pub fn limit(self, custom: usize) -> Option<usize> {
        match self {
            Draw::All => None,
            Draw::N10 => Some(10),
            Draw::N20 => Some(20),
            Draw::N50 => Some(50),
            Draw::Custom => Some(custom.clamp(1, MAX_CUSTOM_DRAW)),
        }
    }
}

/// The session's question order.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Order {
    #[default]
    Sequential,
    Shuffled,
}

impl Order {
    pub fn label(self) -> &'static str {
        match self {
            Order::Sequential => "顺序",
            Order::Shuffled => "随机",
        }
    }
}

/// 练习 / 模考: the segmented control at the top of 怎么练. 模考 replaces the
/// lower setup rows with a fixed paper (年份 / 100 题 / 180 分钟).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum RunMode {
    #[default]
    Practice,
    Exam,
}

impl RunMode {
    pub fn label(self) -> &'static str {
        match self {
            RunMode::Practice => "练习",
            RunMode::Exam => "模考",
        }
    }
}

/// The 练习设置 screen's state.
#[derive(Clone, Debug, Default)]
pub struct Filter {
    pub kind: Kind,
    /// Empty means every chapter.
    pub chapters: HashSet<String>,
    /// 掌握: 不限 / 未做过 / 薄弱 / 已掌握.
    pub mastery: MasteryFilter,
    /// 收藏: 不限 / 已收藏 / 未收藏.
    pub fav: Fav,
    /// 正确率: 不限 / 低于 60% / 低于 100% / 全对.
    pub accuracy: Accuracy,
    /// 年份: `None` = 全部. A question with no year is excluded once a specific
    /// year is chosen — "2002" must not secretly include the undated rest.
    pub year: Option<String>,
    /// 仅练有答案的题: excludes questions whose `answer` list is empty. The
    /// app turns this on by default; the model keeps `false` (= no condition)
    /// as its own default so a bare `Filter` still means "everything".
    pub answered_only: bool,
    pub query: String,
}

impl Filter {
    /// Does one question pass every condition? Split out of [`matches`] so the
    /// answer-only rule (and the rest) can be exercised without a window.
    pub fn admits(&self, q: &Question, user: &UserStore) -> bool {
        if !self.kind.matches(q) {
            return false;
        }
        if !self.chapters.is_empty() && !self.chapters.contains(&q.chapter_label()) {
            return false;
        }
        let stat = user.stat(&q.id);
        if !self.mastery.matches(stat) {
            return false;
        }
        let favorite = user.is_favorite(&q.id);
        if self.fav == Fav::Yes && !favorite {
            return false;
        }
        if self.fav == Fav::No && favorite {
            return false;
        }
        if !self.accuracy.matches(stat) {
            return false;
        }
        if let Some(year) = &self.year {
            if question_year(q).as_deref() != Some(year.as_str()) {
                return false;
            }
        }
        if self.answered_only && q.answer.is_empty() {
            return false;
        }
        let needle = self.query.trim().to_lowercase();
        if !needle.is_empty() {
            let hay = format!(
                "{}\n{}\n{}",
                q.stem,
                q.chapter.clone().unwrap_or_default(),
                q.explanation.clone().unwrap_or_default()
            )
            .to_lowercase();
            if !hay.contains(&needle) {
                return false;
            }
        }
        true
    }
}

/// Does one question pass the current filter?
pub fn matches(q: &Question, f: &Filter, user: &UserStore) -> bool {
    f.admits(q, user)
}

/// Indices of every question passing the filter, in subject order.
pub fn select(questions: &[Question], f: &Filter, user: &UserStore) -> Vec<usize> {
    (0..questions.len())
        .filter(|&i| matches(&questions[i], f, user))
        .collect()
}

/// The year of a question as a filter key. `Question::year` is an untyped JSON
/// value in the corpus (`2002` or `"2002"`), and some questions have none;
/// missing stays `None` so the year filter can exclude it.
pub fn question_year(q: &Question) -> Option<String> {
    q.year_label()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Distinct years among the questions, newest first. Numeric years sort
/// numerically; anything unparsable falls to the end in reverse order, so a
/// mixed corpus still gets a stable descending list.
pub fn years(questions: &[Question]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for q in questions {
        if let Some(y) = question_year(q) {
            if !out.contains(&y) {
                out.push(y);
            }
        }
    }
    out.sort_by(|a, b| match (a.parse::<i64>(), b.parse::<i64>()) {
        (Ok(x), Ok(y)) => y.cmp(&x),
        (Ok(_), Err(_)) => std::cmp::Ordering::Less,
        (Err(_), Ok(_)) => std::cmp::Ordering::Greater,
        (Err(_), Err(_)) => b.cmp(a),
    });
    out
}

/// Attempted share of one chapter, in whole percent. `None` when nothing was
/// attempted — or when the share rounds to 0 % — so a 0 % chapter gets neither
/// a bar nor `0%` text; a 1-of-300 chapter is not worth a full-width mark.
pub fn coverage_percent(attempted: usize, total: usize) -> Option<u32> {
    if total == 0 || attempted == 0 {
        return None;
    }
    let pct = ((attempted as f32 / total as f32) * 100.0).round() as u32;
    (pct > 0).then_some(pct)
}

/// Deterministic Fisher–Yates using xorshift64.
pub fn shuffled(mut idx: Vec<usize>, seed: u64) -> Vec<usize> {
    let mut s = seed | 1;
    for i in (1..idx.len()).rev() {
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        let j = (s % (i as u64 + 1)) as usize;
        idx.swap(i, j);
    }
    idx
}

/// A random draw from the wrong-answer pool (see [`is_wrong`]), capped at
/// `take`.
pub fn wrong_sample(
    questions: &[Question],
    user: &UserStore,
    take: usize,
    seed: u64,
) -> Vec<usize> {
    let pool: Vec<usize> = (0..questions.len())
        .filter(|&i| is_wrong(user.stat(&questions[i].id)))
        .collect();
    let mut out = shuffled(pool, seed);
    out.truncate(take);
    out
}

/// Shuffle first when asked, then cap to the draw size. `custom` supplies the
/// count for [`Draw::Custom`] and is ignored by the fixed presets.
pub fn draw_plan(
    idx: Vec<usize>,
    draw: Draw,
    order: Order,
    seed: u64,
    custom: usize,
) -> Vec<usize> {
    let mut out = match order {
        Order::Sequential => idx,
        Order::Shuffled => shuffled(idx, seed),
    };
    if let Some(n) = draw.limit(custom) {
        out.truncate(n);
    }
    out
}

/// Per chapter: total questions and how many of them have been attempted.
/// Same grouping / order as [`chapter_counts`]; `user` owns the attempted
/// side, so the 覆盖条 is a projection of the stats and nothing else.
pub fn chapter_progress(questions: &[Question], user: &UserStore) -> Vec<(String, usize, usize)> {
    let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut out: Vec<(String, usize, usize)> = Vec::new();
    for q in questions {
        let label = q.chapter_label();
        let i = match index.get(&label) {
            Some(&i) => i,
            None => {
                index.insert(label.clone(), out.len());
                out.push((label, 0, 0));
                out.len() - 1
            }
        };
        out[i].1 += 1;
        if user.stat(&q.id).is_some_and(|s| s.attempts > 0) {
            out[i].2 += 1;
        }
    }
    out
}

/// Distinct chapters in first-seen order, with the number of questions in each.
pub fn chapter_counts(questions: &[Question]) -> Vec<(String, usize)> {
    let mut index: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    let mut out: Vec<(String, usize)> = Vec::new();
    for q in questions {
        let label = q.chapter_label();
        match index.get(&label) {
            Some(&i) => out[i].1 += 1,
            None => {
                index.insert(label.clone(), out.len());
                out.push((label, 1));
            }
        }
    }
    out
}

/// How many questions a 今天的复习 session may hold.
pub const TODAY_CAP: usize = 60;

/// The 今天 queue over the domain's concatenated questions: due Weak first,
/// then due Mastered, then New, capped at `cap`.
///
/// Strict order is the point: a lapse comes back before a mastered question
/// that merely reached its interval, and untouched material waits behind both.
/// `answered_only` drops questions whose `answer` is empty, the same rule the
/// 范围 toggle applies — a cross-subject run must not fill up with 仅供阅读.
pub fn today_queue(
    questions: &[&Question],
    user: &UserStore,
    cap: usize,
    today: i64,
    answered_only: bool,
) -> Vec<usize> {
    let mut weak = Vec::new();
    let mut mastered = Vec::new();
    let mut fresh = Vec::new();
    for (i, q) in questions.iter().enumerate() {
        if answered_only && q.answer.is_empty() {
            continue;
        }
        let stat = user.stat(&q.id);
        match mastery(stat) {
            Mastery::Weak if is_due(stat, today) => weak.push(i),
            Mastery::Mastered if is_due(stat, today) => mastered.push(i),
            Mastery::New => fresh.push(i),
            _ => {}
        }
    }
    let mut out = weak;
    out.extend(mastered);
    out.extend(fresh);
    out.truncate(cap);
    out
}

/// 模考: how many questions one paper holds, and how long it runs.
pub const EXAM_QUESTIONS: usize = 100;
pub const EXAM_MINUTES: u32 = 180;

/// A paper is a random draw of at most [`EXAM_QUESTIONS`] from the current
/// filter — the plan is a shuffle followed by the same cap every other draw
/// uses, so it is deterministic per seed.
pub fn exam_plan(idx: Vec<usize>, seed: u64) -> Vec<usize> {
    let mut out = shuffled(idx, seed);
    out.truncate(EXAM_QUESTIONS);
    out
}

/// A countdown for the exam header: `mm:ss`, plus whether it is inside the
/// last ten minutes (where the colour changes to the accent).
pub fn countdown(remaining: f64) -> (String, bool) {
    let secs = remaining.max(0.0).ceil() as u64;
    (format!("{:02}:{:02}", secs / 60, secs % 60), secs <= 10 * 60)
}

/// One chapter's exam tally.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChapterScore {
    pub chapter: String,
    pub total: usize,
    pub correct: usize,
}

impl ChapterScore {
    /// Whole-percent correct. A chapter with no questions reads 0 %.
    pub fn percent(&self) -> u32 {
        if self.total == 0 {
            0
        } else {
            ((self.correct as f32 / self.total as f32) * 100.0).round() as u32
        }
    }

    /// Below 60 % is the weak set the result view marks.
    pub fn weak(&self) -> bool {
        self.percent() < 60
    }
}

/// Group a paper's outcomes by chapter, in first-seen order. `None` (never
/// answered before 交卷) counts in the chapter total and not in `correct`.
pub fn exam_scores(items: &[(String, Option<bool>)]) -> Vec<ChapterScore> {
    let mut out: Vec<ChapterScore> = Vec::new();
    for (chapter, ok) in items {
        let i = match out.iter().position(|c| &c.chapter == chapter) {
            Some(i) => i,
            None => {
                out.push(ChapterScore {
                    chapter: chapter.clone(),
                    total: 0,
                    correct: 0,
                });
                out.len() - 1
            }
        };
        out[i].total += 1;
        if *ok == Some(true) {
            out[i].correct += 1;
        }
    }
    out
}

/// Is this question tagged with this knowledge point? The corpus tags most
/// questions by **title** and a few by **id**; both are accepted so the
/// overlay's count and its 练这个知识点 session cover the same questions.
pub fn kp_matches(kp: &KnowledgePoint, tag: &str) -> bool {
    tag == kp.id || tag == kp.title
}

/// Indices of the questions tagged with one knowledge point, in subject order.
pub fn kp_question_indices(questions: &[Question], kp: &KnowledgePoint) -> Vec<usize> {
    (0..questions.len())
        .filter(|&i| questions[i].knowledge_points.iter().any(|t| kp_matches(kp, t)))
        .collect()
}

/// `(total, attempted)` per knowledge point, parallel to `kps`: how many
/// questions carry the point, and how many of those have been answered. A
/// question that names the same point twice still counts once.
pub fn kp_question_counts(
    kps: &[KnowledgePoint],
    questions: &[Question],
    user: &UserStore,
) -> Vec<(usize, usize)> {
    let mut index: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for (i, kp) in kps.iter().enumerate() {
        index.entry(kp.id.as_str()).or_insert(i);
        index.entry(kp.title.as_str()).or_insert(i);
    }
    let mut out = vec![(0usize, 0usize); kps.len()];
    for q in questions {
        let mut hit: HashSet<usize> = HashSet::new();
        for tag in &q.knowledge_points {
            if let Some(&i) = index.get(tag.as_str()) {
                hit.insert(i);
            }
        }
        let attempted = user.stat(&q.id).is_some_and(|s| s.attempts > 0);
        for i in hit {
            out[i].0 += 1;
            if attempted {
                out[i].1 += 1;
            }
        }
    }
    out
}

/// `None` when the question cannot be graded yet (no options / empty selection).
pub fn judge(q: &Question, selection: &[String]) -> Option<bool> {
    if q.options.is_empty() || selection.is_empty() {
        return None;
    }
    Some(q.grade(selection))
}

/// May the session move on to the next question? The next question is gated
/// on the current one having been judged: a choice question by submitting it
/// (graded) or by revealing the answer with 显示答案 (`shown`). A **主观题**
/// (no options) has nothing to answer and nothing to grade, so the gate does
/// not apply to it at all — 下一题 and `→` work immediately. `←` 上一题 and
/// `Esc` stay available either way, so the gate is never a trap.
pub fn can_advance(q: &Question, submitted: bool, shown: bool) -> bool {
    if !q.is_choice() {
        return true;
    }
    submitted || shown
}

/// Whether 显示答案 can reveal anything for this question: correct option
/// labels, an 参考答案 text and/or an explanation. Questions with none of the
/// three are common in the data; the button stays visible but disabled for
/// them. A 主观题 very often carries only `answerText`, so it counts as an
/// answer here.
pub fn can_reveal(q: &Question) -> bool {
    !q.answer.is_empty()
        || q.answer_text
            .as_ref()
            .is_some_and(|s| !s.trim().is_empty())
        || q
            .explanation
            .as_ref()
            .is_some_and(|e| !e.trim().is_empty())
}

/// The footer verdict after a wrong choice answer. A question whose `answer`
/// is empty (common in the data) gets a plain 回答错误 plus the reason instead
/// of a dangling "正确答案：" with nothing after it.
pub fn wrong_verdict(q: &Question) -> String {
    if q.answer.is_empty() {
        "回答错误 · 这道题在数据里没有答案".to_string()
    } else {
        format!("回答错误 · 正确答案：{}", q.answer.join("、"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sfzklearning::OptionItem;

    fn q(id: &str, qtype: &str, chapter: &str, opts: usize, answer: &[&str]) -> Question {
        Question {
            id: id.to_string(),
            chapter: Some(chapter.to_string()),
            qtype: Some(qtype.to_string()),
            stem: format!("题干 {id} 关于合同的效力"),
            options: (0..opts)
                .map(|i| OptionItem {
                    label: ((b'A' + i as u8) as char).to_string(),
                    text: format!("选项 {i}"),
                })
                .collect(),
            answer: answer.iter().map(|s| s.to_string()).collect(),
            explanation: Some(format!("解析 {id}")),
            ..Default::default()
        }
    }

    fn questions() -> Vec<Question> {
        vec![
            q("q1", "single_choice", "第一章", 4, &["A"]),
            q("q2", "multiple_choice", "第一章", 4, &["A", "C"]),
            q("q3", "single_choice", "第二章", 4, &["B"]),
            q("q4", "memory", "第二章", 0, &[]),
        ]
    }

    fn empty() -> UserStore {
        UserStore::default()
    }

    #[test]
    fn filter_by_kind() {
        let qs = questions();
        let u = empty();

        let all = select(&qs, &Filter::default(), &u);
        assert_eq!(all, vec![0, 1, 2, 3]);

        let choice = select(
            &qs,
            &Filter {
                kind: Kind::Choice,
                ..Default::default()
            },
            &u,
        );
        assert_eq!(choice, vec![0, 1, 2], "主观题 must be excluded");

        // 主观题 selects the no-options questions — the very set 记忆自测 used
        // to select (`!is_choice()`), so the two were the same mode.
        let subjective = select(
            &qs,
            &Filter {
                kind: Kind::Subjective,
                ..Default::default()
            },
            &u,
        );
        assert_eq!(subjective, vec![3]);
        assert!(
            qs.iter()
                .enumerate()
                .filter(|(_, q)| Kind::Subjective.matches(q))
                .all(|(_, q)| q.options.is_empty()),
            "主观题 must only ever select questions with an empty options list"
        );
    }

    /// The single 错题 predicate: no attempts (never answered, or only viewed)
    /// is not wrong; all attempts correct is not wrong; any wrong attempt is.
    #[test]
    fn is_wrong_needs_an_attempt_and_a_miss() {
        assert!(!is_wrong(None), "no stats entry at all is not wrong");

        let viewed = QStat {
            seen: 3,
            ..Default::default()
        };
        assert!(
            !is_wrong(Some(&viewed)),
            "viewed but never answered (attempts == 0) is not wrong"
        );

        let perfect = QStat {
            attempts: 2,
            correct: 2,
            ..Default::default()
        };
        assert!(!is_wrong(Some(&perfect)), "all attempts correct is not wrong");

        let missed_once = QStat {
            attempts: 3,
            correct: 2,
            ..Default::default()
        };
        assert!(is_wrong(Some(&missed_once)), "a miss among the attempts is wrong");

        let all_wrong = QStat {
            attempts: 1,
            correct: 0,
            ..Default::default()
        };
        assert!(is_wrong(Some(&all_wrong)));
    }

    #[test]
    fn filter_by_chapter_wrong_and_favorite() {
        let qs = questions();
        let mut user = UserStore::default();
        user.toggle_favorite("q1");
        // 错题 comes from the stats now: q2 and q4 were answered wrong.
        user.record_answer("q2", false, 0);
        user.record_answer("q4", false, 0);

        let ch2 = select(
            &qs,
            &Filter {
                chapters: ["第二章".to_string()].into(),
                ..Default::default()
            },
            &user,
        );
        assert_eq!(ch2, vec![2, 3]);

        let both = select(
            &qs,
            &Filter {
                chapters: ["第一章".to_string(), "第二章".to_string()].into(),
                ..Default::default()
            },
            &user,
        );
        assert_eq!(both, vec![0, 1, 2, 3]);

        let only_weak = select(
            &qs,
            &Filter {
                mastery: MasteryFilter::Weak,
                ..Default::default()
            },
            &user,
        );
        assert_eq!(only_weak, vec![1, 3]);

        let only_fav = select(
            &qs,
            &Filter {
                fav: Fav::Yes,
                ..Default::default()
            },
            &user,
        );
        assert_eq!(only_fav, vec![0]);
    }

    /// 收藏 is three-state: 不限 adds nothing, 已收藏 and 未收藏 partition the
    /// pool.
    #[test]
    fn favorite_filter_is_three_state_and_any_is_a_no_op() {
        let qs = questions();
        let mut user = UserStore::default();
        user.toggle_favorite("q2");

        let any = select(&qs, &Filter::default(), &user);
        assert_eq!(any, vec![0, 1, 2, 3], "不限 adds no condition");

        let only = select(
            &qs,
            &Filter {
                fav: Fav::Yes,
                ..Default::default()
            },
            &user,
        );
        assert_eq!(only, vec![1]);

        let none = select(
            &qs,
            &Filter {
                fav: Fav::No,
                ..Default::default()
            },
            &user,
        );
        assert_eq!(none, vec![0, 2, 3]);
    }

    /// The 正确率 conditions, each on its own: 不限 is a no-op, 低于 60% and
    /// 低于 100% are strict upper bounds, and 全对 needs attempts with
    /// `correct == attempts`. 未作答 left this row with 掌握 → 未做过.
    #[test]
    fn accuracy_filter_conditions() {
        let qs = questions();
        let mut user = UserStore::default();
        user.record_answer("q1", true, 0); // 2/2 = 100%
        user.record_answer("q1", true, 0);
        user.record_answer("q2", true, 0); // 2/3 ≈ 67%: below 100, not below 60
        user.record_answer("q2", true, 0);
        user.record_answer("q2", false, 0);
        user.record_answer("q3", false, 0); // 0/1 = 0%: below 60
        // q4 was never answered.

        let pick = |a: Accuracy| {
            select(
                &qs,
                &Filter {
                    accuracy: a,
                    ..Default::default()
                },
                &user,
            )
        };
        assert_eq!(pick(Accuracy::Any), vec![0, 1, 2, 3], "不限 is a no-op");
        assert_eq!(pick(Accuracy::Below60), vec![2]);
        assert_eq!(pick(Accuracy::Below100), vec![1, 2]);
        assert_eq!(pick(Accuracy::Perfect), vec![0]);
    }

    /// The new conditions compose (AND) with the existing filters: 题型,
    /// 章节, the 掌握 chips, 关键词.
    #[test]
    fn new_conditions_compose_with_the_existing_filters() {
        let qs = questions();
        let mut user = UserStore::default();
        user.toggle_favorite("q1");
        user.record_answer("q1", true, 0);
        user.record_answer("q2", false, 0);

        // 已收藏 ∧ 全对 ∧ 第一章 ∧ 选择题 → q1 only.
        let hit = select(
            &qs,
            &Filter {
                kind: Kind::Choice,
                chapters: ["第一章".to_string()].into(),
                fav: Fav::Yes,
                accuracy: Accuracy::Perfect,
                ..Default::default()
            },
            &user,
        );
        assert_eq!(hit, vec![0]);

        // 未收藏 ∧ 低于 100% ∧ 第一章 ∧ 选择题 → q2 only (q1 is favourited).
        let hit = select(
            &qs,
            &Filter {
                kind: Kind::Choice,
                chapters: ["第一章".to_string()].into(),
                fav: Fav::No,
                accuracy: Accuracy::Below100,
                ..Default::default()
            },
            &user,
        );
        assert_eq!(hit, vec![1]);

        // 未做过 ∧ 主观题 → q4.
        let hit = select(
            &qs,
            &Filter {
                kind: Kind::Subjective,
                mastery: MasteryFilter::Unseen,
                ..Default::default()
            },
            &user,
        );
        assert_eq!(hit, vec![3]);

        // 薄弱 (stats-derived) ∧ 未收藏 → q2.
        let hit = select(
            &qs,
            &Filter {
                mastery: MasteryFilter::Weak,
                fav: Fav::No,
                ..Default::default()
            },
            &user,
        );
        assert_eq!(hit, vec![1]);

        // 关键词 ∧ 未收藏 ∧ 未做过 → q4 (matches its own explanation, unfav,
        // never answered).
        let hit = select(
            &qs,
            &Filter {
                query: "q4".to_string(),
                fav: Fav::No,
                mastery: MasteryFilter::Unseen,
                ..Default::default()
            },
            &user,
        );
        assert_eq!(hit, vec![3], "q4 matches its own explanation, is unfav and unanswered");

        let none = select(
            &qs,
            &Filter {
                query: "q2".to_string(),
                fav: Fav::No,
                mastery: MasteryFilter::Unseen,
                ..Default::default()
            },
            &user,
        );
        assert!(none.is_empty(), "q2 was answered: 未做过 excludes it");
    }

    #[test]
    fn filter_by_query_searches_stem_and_explanation() {
        let qs = questions();
        let u = empty();
        let hit = select(
            &qs,
            &Filter {
                query: "q3".to_string(),
                ..Default::default()
            },
            &u,
        );
        assert_eq!(hit, vec![2], "matches the explanation of q3 only");

        let none = select(
            &qs,
            &Filter {
                query: "不存在的关键词".to_string(),
                ..Default::default()
            },
            &u,
        );
        assert!(none.is_empty());
    }

    /// The 掌握 chips read the per-question stats: 未做过 is `attempts == 0`,
    /// 薄弱 is [`Mastery::Weak`] and 已掌握 is [`Mastery::Mastered`]. The old
    /// separate wrong-id set is gone, so a stale entry cannot mark an
    /// unanswered question as 薄弱.
    #[test]
    fn mastery_chips_come_from_the_stats() {
        let qs = questions();
        let mut user = UserStore::default();
        user.toggle_favorite("q1");
        user.record_answer("q1", true, 0);
        user.record_answer("q1", true, 0); // 2 in a row: mastered
        user.record_answer("q2", false, 0); // weak
        user.record_answer("q3", false, 0); // weak

        let pick = |m: MasteryFilter| {
            select(
                &qs,
                &Filter {
                    mastery: m,
                    ..Default::default()
                },
                &user,
            )
        };
        assert_eq!(pick(MasteryFilter::Any), vec![0, 1, 2, 3], "不限 is a no-op");
        assert_eq!(pick(MasteryFilter::Unseen), vec![3], "only q4 has no attempts");
        assert_eq!(pick(MasteryFilter::Weak), vec![1, 2], "attempted, not yet mastered");
        assert_eq!(pick(MasteryFilter::Mastered), vec![0], "two correct in a row");

        // An entry written before the schedule existed is weak and due at once;
        // it must land in 薄弱, not vanish.
        let mut migrated = UserStore::default();
        migrated.stats.insert(
            "q1".to_string(),
            QStat {
                attempts: 1,
                correct: 0,
                ..Default::default()
            },
        );
        let weak = select(
            &qs,
            &Filter {
                mastery: MasteryFilter::Weak,
                ..Default::default()
            },
            &migrated,
        );
        assert_eq!(weak, vec![0]);

        let combined = select(
            &qs,
            &Filter {
                mastery: MasteryFilter::Unseen,
                fav: Fav::Yes,
                ..Default::default()
            },
            &user,
        );
        assert!(
            combined.is_empty(),
            "AND semantics: nothing is both unseen and favourited"
        );
    }

    /// 年份 keeps numeric years even when the corpus stores them as strings,
    /// excludes undated questions once a year is picked, and lists the subject's
    /// years newest first.
    #[test]
    fn year_filter_matches_numbers_and_strings_and_excludes_missing() {
        use serde_json::json;
        let mut qs = questions();
        qs[0].year = json!(2002);
        qs[1].year = json!("2002");
        qs[2].year = json!("2013");
        // q4 has no year.

        assert_eq!(question_year(&qs[0]).as_deref(), Some("2002"));
        assert_eq!(question_year(&qs[1]).as_deref(), Some("2002"));
        assert_eq!(question_year(&qs[3]), None);

        let y2002 = select(
            &qs,
            &Filter {
                year: Some("2002".to_string()),
                ..Default::default()
            },
            &empty(),
        );
        assert_eq!(y2002, vec![0, 1], "a number and a string are the same year");
        assert!(
            !y2002.contains(&3),
            "an undated question must not slip into a chosen year"
        );

        assert_eq!(years(&qs), vec!["2013", "2002"], "newest first, deduplicated");

        let all = select(
            &qs,
            &Filter {
                year: None,
                ..Default::default()
            },
            &empty(),
        );
        assert_eq!(all, vec![0, 1, 2, 3], "全部 adds no condition");
    }

    /// 仅练有答案的题 excludes exactly the questions whose `answer` is empty;
    /// off, it adds no condition at all.
    #[test]
    fn answered_only_excludes_empty_answers() {
        let qs = questions(); // q4 has no answer
        let on = select(
            &qs,
            &Filter {
                answered_only: true,
                ..Default::default()
            },
            &empty(),
        );
        assert_eq!(on, vec![0, 1, 2]);

        let off = select(&qs, &Filter::default(), &empty());
        assert_eq!(off, vec![0, 1, 2, 3]);
    }

    /// 今天的复习: due weak, then due mastered, then new, capped. Not-due
    /// answered questions wait, and an unanswered question is new — never due.
    #[test]
    fn today_queue_orders_due_weak_then_mastered_then_new() {
        let qs = questions();
        let today = 20_000;
        let mut user = UserStore::default();
        // q1: due weak. q2: due mastered. q3: answered, not due yet.
        user.stats.insert(
            "q1".to_string(),
            QStat {
                attempts: 2,
                correct: 1,
                streak: 0,
                due_day: today - 1,
                ..Default::default()
            },
        );
        user.stats.insert(
            "q2".to_string(),
            QStat {
                attempts: 2,
                correct: 2,
                streak: 2,
                due_day: today,
                ..Default::default()
            },
        );
        user.stats.insert(
            "q3".to_string(),
            QStat {
                attempts: 1,
                correct: 1,
                streak: 1,
                due_day: today + 3,
                ..Default::default()
            },
        );
        let refs: Vec<&Question> = qs.iter().collect();

        let queue = today_queue(&refs, &user, 60, today, false);
        assert_eq!(
            queue,
            vec![0, 1, 3],
            "due weak, due mastered, new; q3 is answered and not due"
        );

        let capped = today_queue(&refs, &user, 2, today, false);
        assert_eq!(capped, vec![0, 1], "the cap keeps the front of the order");

        // q4 is new but has no answer; 仅练有答案的题 drops it.
        let answered_only = today_queue(&refs, &user, 60, today, true);
        assert_eq!(answered_only, vec![0, 1], "no answer: not in the pool");
    }

    /// 覆盖条 = attempted / total per chapter, and 0 attempted means no number
    /// and no bar (never `0%` text and never an empty bar).
    #[test]
    fn chapter_coverage_is_attempted_over_total() {
        let qs = questions();
        let mut user = UserStore::default();
        user.record_answer("q1", false, 0);

        let progress = chapter_progress(&qs, &user);
        assert_eq!(
            progress,
            vec![
                ("第一章".to_string(), 2, 1),
                ("第二章".to_string(), 2, 0)
            ]
        );
        assert_eq!(coverage_percent(1, 2), Some(50));
        assert_eq!(coverage_percent(5, 12), Some(42), "rounded to whole percent");
        assert_eq!(coverage_percent(2, 2), Some(100));
        assert_eq!(coverage_percent(0, 2), None, "0 % draws nothing");
        assert_eq!(
            coverage_percent(1, 283),
            None,
            "a share that rounds to 0 % stays invisible too"
        );
        assert_eq!(coverage_percent(1, 100), Some(1));
        assert_eq!(coverage_percent(0, 0), None);
    }

    /// The knowledge overlay counts the questions tagged with a point, by id or
    /// by title (the corpus uses both), and its 练这个知识点 order is the same
    /// set in subject order.
    #[test]
    fn knowledge_points_count_and_select_by_id_or_title() {
        let kp_by_id = KnowledgePoint {
            id: "law:kp:1".to_string(),
            chapter: Some("第一章".to_string()),
            title: "合同的效力".to_string(),
            ..Default::default()
        };
        let kp_by_title = KnowledgePoint {
            id: "law:kp:2".to_string(),
            chapter: Some("第一章".to_string()),
            title: "违约责任".to_string(),
            ..Default::default()
        };
        let kps = vec![kp_by_id.clone(), kp_by_title.clone()];
        let mut qs = questions();
        qs[0].knowledge_points = vec!["law:kp:1".to_string()];
        qs[1].knowledge_points = vec!["合同的效力".to_string()];
        // q3 is tagged twice with the same point: it still counts once.
        qs[2].knowledge_points = vec!["违约责任".to_string(), "违约责任".to_string()];
        qs[3].knowledge_points = vec!["没有这个点".to_string()];

        assert_eq!(kp_question_indices(&qs, &kp_by_id), vec![0, 1]);
        assert_eq!(kp_question_indices(&qs, &kp_by_title), vec![2]);

        let mut user = UserStore::default();
        user.record_answer("q1", false, 0);
        let counts = kp_question_counts(&kps, &qs, &user);
        assert_eq!(counts[0], (2, 1), "id and title both resolve to point 0; q1 attempted");
        assert_eq!(counts[1], (1, 0), "the duplicate tag counts once; q3 not attempted");
    }

    /// A paper is a shuffled draw capped at 100; the countdown formats mm:ss
    /// and turns urgent inside the last ten minutes.
    #[test]
    fn exam_draw_cap_and_countdown() {
        let draw = exam_plan((0..250).collect(), 7);
        assert_eq!(draw.len(), EXAM_QUESTIONS);
        let mut sorted = draw.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), EXAM_QUESTIONS, "a draw never repeats a question");
        assert_eq!(draw, exam_plan((0..250).collect(), 7), "deterministic per seed");

        let small = exam_plan((0..9).collect(), 7);
        assert_eq!(small.len(), 9, "a pool smaller than 100 keeps everything");

        assert_eq!(countdown(180.0 * 60.0), ("180:00".to_string(), false));
        assert_eq!(countdown(600.0), ("10:00".to_string(), true));
        assert_eq!(countdown(599.0), ("09:59".to_string(), true));
        assert_eq!(countdown(-3.0), ("00:00".to_string(), true));
    }

    /// 交卷 groups the paper by chapter; an unanswered question is in the
    /// chapter total but not in `correct`, and below 60 % is the weak mark.
    #[test]
    fn exam_scores_group_by_chapter_and_mark_weak_ones() {
        let items = vec![
            ("第一章".to_string(), Some(true)),
            ("第一章".to_string(), Some(false)),
            ("第二章".to_string(), Some(true)),
            ("第二章".to_string(), Some(true)),
            ("第二章".to_string(), Some(true)),
            ("第三章".to_string(), None),
            ("第三章".to_string(), None),
        ];
        let scores = exam_scores(&items);
        assert_eq!(scores.len(), 3, "first-seen chapter order, no repeats");
        assert_eq!(scores[0].chapter, "第一章");
        assert_eq!((scores[0].correct, scores[0].total), (1, 2));
        assert_eq!(scores[0].percent(), 50);
        assert!(scores[0].weak());
        assert_eq!(scores[1].percent(), 100);
        assert!(!scores[1].weak());
        assert_eq!((scores[2].correct, scores[2].total), (0, 2), "unanswered is not correct");
        assert!(scores[2].weak());
    }

    #[test]
    fn wrong_sample_only_draws_wrong_and_caps() {
        let qs = questions();
        let mut user = UserStore::default();
        user.record_answer("q1", false, 0);
        user.record_answer("q3", false, 0);
        user.record_seen("q4", 0); // viewed but never answered: not 错题

        let draw = wrong_sample(&qs, &user, 10, 7);
        assert_eq!(draw.len(), 2, "the viewed-only question must not be drawn");
        for i in &draw {
            assert!(
                is_wrong(user.stat(&qs[*i].id)),
                "only wrong questions may be drawn"
            );
        }

        let capped = wrong_sample(&qs, &user, 1, 7);
        assert_eq!(capped.len(), 1);
    }

    #[test]
    fn shuffled_is_a_permutation_and_deterministic() {
        let a = shuffled((0..8).collect(), 42);
        let b = shuffled((0..8).collect(), 42);
        assert_eq!(a, b, "same seed must give the same order");
        let mut sorted = a.clone();
        sorted.sort();
        assert_eq!(sorted, (0..8).collect::<Vec<_>>(), "must be a permutation");
    }

    #[test]
    fn draw_plan_orders_then_caps() {
        let seq = draw_plan(
            (0..4).collect(),
            Draw::All,
            Order::Sequential,
            9,
            DEFAULT_CUSTOM_DRAW,
        );
        assert_eq!(seq, vec![0, 1, 2, 3], "sequential keeps subject order");

        let capped = draw_plan(
            (0..30).collect(),
            Draw::N20,
            Order::Sequential,
            9,
            DEFAULT_CUSTOM_DRAW,
        );
        assert_eq!(
            capped,
            (0..20).collect::<Vec<_>>(),
            "sequential + cap is the first N"
        );

        let shuffled_all = draw_plan(
            (0..4).collect(),
            Draw::All,
            Order::Shuffled,
            9,
            DEFAULT_CUSTOM_DRAW,
        );
        assert_eq!(
            shuffled_all,
            shuffled((0..4).collect(), 9),
            "shuffle happens before the cap"
        );

        let shuffled_capped = draw_plan(
            (0..30).collect(),
            Draw::N10,
            Order::Shuffled,
            9,
            DEFAULT_CUSTOM_DRAW,
        );
        assert_eq!(shuffled_capped.len(), 10);
        assert_eq!(shuffled_capped, shuffled((0..30).collect(), 9)[..10].to_vec());

        let small_pool = draw_plan(
            (0..3).collect(),
            Draw::N10,
            Order::Shuffled,
            9,
            DEFAULT_CUSTOM_DRAW,
        );
        assert_eq!(small_pool.len(), 3, "a cap larger than the pool keeps everything");

        let custom = draw_plan(
            (0..30).collect(),
            Draw::Custom,
            Order::Sequential,
            9,
            7,
        );
        assert_eq!(custom, (0..7).collect::<Vec<_>>(), "custom caps at its own count");

        let over_pool = draw_plan(
            (0..3).collect(),
            Draw::Custom,
            Order::Sequential,
            9,
            10,
        );
        assert_eq!(
            over_pool.len(),
            3,
            "a custom count larger than the pool keeps everything, like the presets"
        );

        let clamped_zero = draw_plan(
            (0..3).collect(),
            Draw::Custom,
            Order::Sequential,
            9,
            0,
        );
        assert_eq!(clamped_zero.len(), 1, "a custom count is clamped to at least 1");

        let exact = draw_plan(
            (0..1000).collect(),
            Draw::Custom,
            Order::Sequential,
            9,
            7,
        );
        assert_eq!(exact.len(), 7, "自定义 draws exactly the requested count");

        let clamped_max = draw_plan(
            (0..1000).collect(),
            Draw::Custom,
            Order::Sequential,
            9,
            MAX_CUSTOM_DRAW + 500,
        );
        assert_eq!(
            clamped_max.len(),
            MAX_CUSTOM_DRAW,
            "an absurd custom count is clamped down to the maximum"
        );
    }

    #[test]
    fn chapter_counts_lists_distinct_chapters_in_first_seen_order() {
        let qs = questions();
        assert_eq!(
            chapter_counts(&qs),
            vec![("第一章".to_string(), 2), ("第二章".to_string(), 2)]
        );

        let mut no_chapter = q("q5", "memory", "第五章", 0, &[]);
        no_chapter.chapter = None;
        let mixed = vec![qs[0].clone(), no_chapter];
        assert_eq!(
            chapter_counts(&mixed),
            vec![("第一章".to_string(), 1), ("（未分章）".to_string(), 1)]
        );
    }

    #[test]
    fn judge_requires_options_and_a_selection() {
        let qs = questions();
        let single = &qs[0];
        assert_eq!(judge(single, &[]), None, "nothing selected");
        assert_eq!(judge(single, &["A".into()]), Some(true));
        assert_eq!(judge(single, &["B".into()]), Some(false));

        let multi = &qs[1];
        assert_eq!(judge(multi, &["C".into(), "A".into()]), Some(true), "order-insensitive");
        assert_eq!(judge(multi, &["A".into()]), Some(false));

        let memory = &qs[3];
        assert_eq!(judge(memory, &[]), None, "no options -> not gradable");
    }

    /// 下一题 waits for a judgment on a choice question: a submitted or
    /// revealed one passes, an untouched one does not. A 主观题 (no options)
    /// has nothing to judge, so the gate does not apply to it.
    #[test]
    fn next_is_gated_until_the_question_is_judged() {
        let qs = questions();
        let choice = &qs[0];
        assert!(!can_advance(choice, false, false), "untouched: no next");
        assert!(can_advance(choice, true, false), "submitted: next");
        assert!(can_advance(choice, false, true), "revealed: next");
        assert!(can_advance(choice, true, true));

        // 主观题: nothing to answer, nothing to grade — 下一题 is never gated.
        let subjective = &qs[3];
        assert!(
            can_advance(subjective, false, false),
            "a 主观题 is not gated on any judgment"
        );
        assert!(can_advance(subjective, false, true), "and stays open after 显示答案");
        assert!(can_advance(subjective, true, false), "and for any other state");
    }

    /// 显示答案 is only offered when there is something to show; an empty
    /// `answer` with no explanation must not open an empty reveal. A 主观题's
    /// `answerText` (参考答案) counts as an answer.
    #[test]
    fn can_reveal_needs_an_answer_or_an_explanation() {
        let mut q = q("q1", "single_choice", "第一章", 4, &["A"]);
        assert!(can_reveal(&q), "an answer is enough");

        q.answer.clear();
        assert!(can_reveal(&q), "an explanation alone is still worth revealing");

        q.explanation = None;
        assert!(!can_reveal(&q), "nothing to show");

        q.explanation = Some("   ".to_string());
        assert!(!can_reveal(&q), "a blank explanation counts as none");

        q.answer_text = Some("参考答案正文".to_string());
        assert!(can_reveal(&q), "a 主观题's 参考答案 is an answer");

        q.answer_text = Some("   ".to_string());
        assert!(!can_reveal(&q), "a blank 参考答案 counts as none");
    }

    /// A wrong verdict must name the correct labels, or say why there are
    /// none — never leave a dangling "正确答案：".
    #[test]
    fn wrong_verdict_names_the_answer_or_says_there_is_none() {
        let mut q = q("q1", "single_choice", "第一章", 4, &["A", "C"]);
        assert_eq!(wrong_verdict(&q), "回答错误 · 正确答案：A、C");
        q.answer.clear();
        assert_eq!(wrong_verdict(&q), "回答错误 · 这道题在数据里没有答案");
    }
}
