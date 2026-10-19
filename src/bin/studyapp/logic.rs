//! Pure selection / practice logic, kept out of the UI so it can be tested
//! without opening a window. Everything here takes plain data and returns
//! plain data; `main.rs` only renders the results.

use std::collections::HashSet;

use sfzklearning::{Question, QStat, UserStore};

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Kind {
    #[default]
    All,
    Choice,
    Memory,
}

impl Kind {
    pub fn label(self) -> &'static str {
        match self {
            Kind::All => "全部",
            Kind::Choice => "选择题",
            Kind::Memory => "记忆自测",
        }
    }

    pub fn matches(self, q: &Question) -> bool {
        match self {
            Kind::All => true,
            Kind::Choice => q.is_choice(),
            Kind::Memory => !q.is_choice(),
        }
    }
}

/// Where a question may come from: 仅错题 / 未做过 / 做错过.
/// Every enabled flag must pass (AND). 仅错题 and 做错过 are the same
/// stats-derived predicate ([`is_wrong`]); both chips stay because they read
/// as different questions to the user.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Sources {
    pub wrong: bool,
    pub unseen: bool,
    pub missed: bool,
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

/// 正确率 filter: 不限 / 未作答 / 低于 60% / 低于 100% / 全对.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Accuracy {
    #[default]
    Any,
    Unanswered,
    Below60,
    Below100,
    Perfect,
}

impl Accuracy {
    /// Does this condition hold for a question's stats? [`Accuracy::Any`] is a
    /// true no-op: it adds no condition at all.
    pub fn matches(self, stat: Option<&QStat>) -> bool {
        let attempts = stat.map_or(0, |s| s.attempts);
        match self {
            Accuracy::Any => true,
            Accuracy::Unanswered => attempts == 0,
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

/// The 练习设置 screen's state.
#[derive(Clone, Debug, Default)]
pub struct Filter {
    pub kind: Kind,
    /// Empty means every chapter.
    pub chapters: HashSet<String>,
    pub sources: Sources,
    /// 收藏: 不限 / 已收藏 / 未收藏.
    pub fav: Fav,
    /// 正确率: 不限 / 未作答 / 低于 60% / 低于 100% / 全对.
    pub accuracy: Accuracy,
    pub query: String,
}

/// Does one question pass the current filter?
pub fn matches(q: &Question, f: &Filter, user: &UserStore) -> bool {
    if !f.kind.matches(q) {
        return false;
    }
    if !f.chapters.is_empty() && !f.chapters.contains(&q.chapter_label()) {
        return false;
    }
    let stat = user.stat(&q.id);
    let sources = f.sources;
    if sources.wrong && !is_wrong(stat) {
        return false;
    }
    if sources.unseen && stat.map_or(0, |st| st.attempts) != 0 {
        return false;
    }
    if sources.missed && !is_wrong(stat) {
        return false;
    }
    let favorite = user.is_favorite(&q.id);
    if f.fav == Fav::Yes && !favorite {
        return false;
    }
    if f.fav == Fav::No && favorite {
        return false;
    }
    if !f.accuracy.matches(stat) {
        return false;
    }
    let needle = f.query.trim().to_lowercase();
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

/// Indices of every question passing the filter, in subject order.
pub fn select(questions: &[Question], f: &Filter, user: &UserStore) -> Vec<usize> {
    (0..questions.len())
        .filter(|&i| matches(&questions[i], f, user))
        .collect()
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

/// `None` when the question cannot be graded yet (no options / empty selection).
pub fn judge(q: &Question, selection: &[String]) -> Option<bool> {
    if q.options.is_empty() || selection.is_empty() {
        return None;
    }
    Some(q.grade(selection))
}

/// May the session move on to the next question? The next question is gated
/// on the current one having been judged: a choice question by submitting it
/// (graded) or by revealing the answer with 显示答案 (`shown`); a memory /
/// self-test question by its own self-assessment (记住了 / 没记住). `←` 上一题
/// and `Esc` stay available, so the gate is never a trap.
pub fn can_advance(q: &Question, submitted: bool, shown: bool, self_assessed: bool) -> bool {
    if q.is_choice() {
        submitted || shown
    } else {
        self_assessed
    }
}

/// Whether 显示答案 can reveal anything for this question: correct option
/// labels and/or an explanation. Questions with neither are common in the
/// data; the button stays visible but disabled for them.
pub fn can_reveal(q: &Question) -> bool {
    !q.answer.is_empty()
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
        assert_eq!(choice, vec![0, 1, 2], "记忆自测 must be excluded");

        let memory = select(
            &qs,
            &Filter {
                kind: Kind::Memory,
                ..Default::default()
            },
            &u,
        );
        assert_eq!(memory, vec![3]);
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
        user.record_answer("q2", false, "");
        user.record_answer("q4", false, "");

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

        let only_wrong = select(
            &qs,
            &Filter {
                sources: Sources {
                    wrong: true,
                    ..Default::default()
                },
                ..Default::default()
            },
            &user,
        );
        assert_eq!(only_wrong, vec![1, 3]);

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

    /// The 正确率 conditions, each on its own: 不限 is a no-op, 未作答 is
    /// `attempts == 0`, 低于 60% and 低于 100% are strict upper bounds, and
    /// 全对 needs attempts with `correct == attempts`.
    #[test]
    fn accuracy_filter_conditions() {
        let qs = questions();
        let mut user = UserStore::default();
        user.record_answer("q1", true, "t"); // 2/2 = 100%
        user.record_answer("q1", true, "t");
        user.record_answer("q2", true, "t"); // 2/3 ≈ 67%: below 100, not below 60
        user.record_answer("q2", true, "t");
        user.record_answer("q2", false, "t");
        user.record_answer("q3", false, "t"); // 0/1 = 0%: below 60
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
        assert_eq!(pick(Accuracy::Unanswered), vec![3]);
        assert_eq!(pick(Accuracy::Below60), vec![2]);
        assert_eq!(pick(Accuracy::Below100), vec![1, 2]);
        assert_eq!(pick(Accuracy::Perfect), vec![0]);
    }

    /// The new conditions compose (AND) with the existing filters: 题型,
    /// 章节, the 来源 chips, 关键词.
    #[test]
    fn new_conditions_compose_with_the_existing_filters() {
        let qs = questions();
        let mut user = UserStore::default();
        user.toggle_favorite("q1");
        user.record_answer("q1", true, "");
        user.record_answer("q2", false, "");

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

        // 未作答 ∧ 记忆自测 → q4.
        let hit = select(
            &qs,
            &Filter {
                kind: Kind::Memory,
                accuracy: Accuracy::Unanswered,
                ..Default::default()
            },
            &user,
        );
        assert_eq!(hit, vec![3]);

        // 仅错题 (stats-derived) ∧ 未收藏 → q2.
        let hit = select(
            &qs,
            &Filter {
                sources: Sources {
                    wrong: true,
                    ..Default::default()
                },
                fav: Fav::No,
                ..Default::default()
            },
            &user,
        );
        assert_eq!(hit, vec![1]);

        // 关键词 ∧ 未收藏 ∧ 未作答 → q4 (matches its own explanation, unfav,
        // never answered).
        let hit = select(
            &qs,
            &Filter {
                query: "q4".to_string(),
                fav: Fav::No,
                accuracy: Accuracy::Unanswered,
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
                accuracy: Accuracy::Unanswered,
                ..Default::default()
            },
            &user,
        );
        assert!(none.is_empty(), "q2 was answered: 未作答 excludes it");
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

    /// The 来源 chips read the per-question stats: 未做过 is `attempts == 0`,
    /// and 仅错题 / 做错过 are the same stats-derived predicate ([`is_wrong`]).
    /// The old separate wrong-id set is gone, so a stale entry cannot mark an
    /// unanswered question as 错题.
    #[test]
    fn sources_unseen_missed_and_wrong_come_from_the_stats() {
        let qs = questions();
        let mut user = UserStore::default();
        user.toggle_favorite("q1");
        user.record_answer("q1", true, "");
        user.record_answer("q1", true, "");
        user.record_answer("q2", false, "");
        user.record_answer("q3", false, "");

        let unseen = select(
            &qs,
            &Filter {
                sources: Sources {
                    unseen: true,
                    ..Default::default()
                },
                ..Default::default()
            },
            &user,
        );
        assert_eq!(unseen, vec![3], "only q4 has no attempts");

        let missed = select(
            &qs,
            &Filter {
                sources: Sources {
                    missed: true,
                    ..Default::default()
                },
                ..Default::default()
            },
            &user,
        );
        assert_eq!(missed, vec![1, 2], "attempted but never fully correct");

        let wrong_only = select(
            &qs,
            &Filter {
                sources: Sources {
                    wrong: true,
                    ..Default::default()
                },
                ..Default::default()
            },
            &user,
        );
        assert_eq!(wrong_only, vec![1, 2], "仅错题 is the same predicate as 做错过");

        let fav_only = select(
            &qs,
            &Filter {
                fav: Fav::Yes,
                ..Default::default()
            },
            &user,
        );
        assert_eq!(fav_only, vec![0]);

        let combined = select(
            &qs,
            &Filter {
                sources: Sources {
                    unseen: true,
                    ..Default::default()
                },
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

    #[test]
    fn wrong_sample_only_draws_wrong_and_caps() {
        let qs = questions();
        let mut user = UserStore::default();
        user.record_answer("q1", false, "");
        user.record_answer("q3", false, "");
        user.record_seen("q4", "t"); // viewed but never answered: not 错题

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

    /// 下一题 waits for a judgment: a submitted or revealed choice question
    /// passes, an untouched one does not; a memory question needs its own
    /// self-assessment (revealing the answer is not an assessment).
    #[test]
    fn next_is_gated_until_the_question_is_judged() {
        let qs = questions();
        let choice = &qs[0];
        assert!(!can_advance(choice, false, false, false), "untouched: no next");
        assert!(
            !can_advance(choice, false, false, true),
            "self-assessment does not judge a choice question"
        );
        assert!(can_advance(choice, true, false, false), "submitted: next");
        assert!(can_advance(choice, false, true, false), "revealed: next");
        assert!(can_advance(choice, true, true, false));

        let memory = &qs[3];
        assert!(!can_advance(memory, false, false, false), "not assessed: no next");
        assert!(
            !can_advance(memory, false, true, false),
            "seeing the answer is not self-assessment"
        );
        assert!(!can_advance(memory, true, false, false), "submitting does not apply");
        assert!(can_advance(memory, false, false, true), "self-assessed: next");
    }

    /// 显示答案 is only offered when there is something to show; an empty
    /// `answer` with no explanation must not open an empty reveal.
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
