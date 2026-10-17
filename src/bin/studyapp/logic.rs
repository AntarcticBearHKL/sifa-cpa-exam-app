//! Pure selection / practice logic, kept out of the UI so it can be tested
//! without opening a window. Everything here takes plain data and returns
//! plain data; `main.rs` only renders the results.

use std::collections::HashSet;

use sfzklearning::{Question, UserStore};

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

/// Where a question may come from: 仅错题 / 仅收藏 / 未做过 / 做错过.
/// Every enabled flag must pass (AND).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Sources {
    pub wrong: bool,
    pub fav: bool,
    pub unseen: bool,
    pub missed: bool,
}

/// How many questions a session may take.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Draw {
    #[default]
    All,
    N10,
    N20,
    N50,
}

impl Draw {
    pub fn label(self) -> &'static str {
        match self {
            Draw::All => "全部",
            Draw::N10 => "10",
            Draw::N20 => "20",
            Draw::N50 => "50",
        }
    }

    pub fn limit(self) -> Option<usize> {
        match self {
            Draw::All => None,
            Draw::N10 => Some(10),
            Draw::N20 => Some(20),
            Draw::N50 => Some(50),
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
    pub query: String,
}

/// Does one question pass the current filter?
pub fn matches(
    q: &Question,
    f: &Filter,
    wrong: &HashSet<String>,
    user: &UserStore,
) -> bool {
    if !f.kind.matches(q) {
        return false;
    }
    if !f.chapters.is_empty() && !f.chapters.contains(&q.chapter_label()) {
        return false;
    }
    let sources = f.sources;
    let attempts = user.stat(&q.id).map_or(0, |st| st.attempts);
    let correct = user.stat(&q.id).map_or(0, |st| st.correct);
    if sources.wrong && !wrong.contains(&q.id) {
        return false;
    }
    if sources.fav && !user.is_favorite(&q.id) {
        return false;
    }
    if sources.unseen && attempts != 0 {
        return false;
    }
    if sources.missed && !(attempts > 0 && correct < attempts) {
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
pub fn select(
    questions: &[Question],
    f: &Filter,
    wrong: &HashSet<String>,
    user: &UserStore,
) -> Vec<usize> {
    (0..questions.len())
        .filter(|&i| matches(&questions[i], f, wrong, user))
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

/// A random draw from the wrong-answer pool, capped at `take`.
pub fn wrong_sample(
    questions: &[Question],
    wrong: &HashSet<String>,
    take: usize,
    seed: u64,
) -> Vec<usize> {
    let pool: Vec<usize> = (0..questions.len())
        .filter(|&i| wrong.contains(&questions[i].id))
        .collect();
    let mut out = shuffled(pool, seed);
    out.truncate(take);
    out
}

/// Shuffle first when asked, then cap to the draw size.
pub fn draw_plan(idx: Vec<usize>, draw: Draw, order: Order, seed: u64) -> Vec<usize> {
    let mut out = match order {
        Order::Sequential => idx,
        Order::Shuffled => shuffled(idx, seed),
    };
    if let Some(n) = draw.limit() {
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

    fn empty() -> (HashSet<String>, UserStore) {
        (HashSet::new(), UserStore::default())
    }

    #[test]
    fn filter_by_kind() {
        let qs = questions();
        let (w, u) = empty();

        let all = select(&qs, &Filter::default(), &w, &u);
        assert_eq!(all, vec![0, 1, 2, 3]);

        let choice = select(
            &qs,
            &Filter {
                kind: Kind::Choice,
                ..Default::default()
            },
            &w,
            &u,
        );
        assert_eq!(choice, vec![0, 1, 2], "记忆自测 must be excluded");

        let memory = select(
            &qs,
            &Filter {
                kind: Kind::Memory,
                ..Default::default()
            },
            &w,
            &u,
        );
        assert_eq!(memory, vec![3]);
    }

    #[test]
    fn filter_by_chapter_wrong_and_favorite() {
        let qs = questions();
        let mut wrong = HashSet::new();
        wrong.insert("q2".to_string());
        wrong.insert("q4".to_string());
        let mut user = UserStore::default();
        user.toggle_favorite("q1");

        let ch2 = select(
            &qs,
            &Filter {
                chapters: ["第二章".to_string()].into(),
                ..Default::default()
            },
            &wrong,
            &user,
        );
        assert_eq!(ch2, vec![2, 3]);

        let both = select(
            &qs,
            &Filter {
                chapters: ["第一章".to_string(), "第二章".to_string()].into(),
                ..Default::default()
            },
            &wrong,
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
            &wrong,
            &user,
        );
        assert_eq!(only_wrong, vec![1, 3]);

        let only_fav = select(
            &qs,
            &Filter {
                sources: Sources {
                    fav: true,
                    ..Default::default()
                },
                ..Default::default()
            },
            &wrong,
            &user,
        );
        assert_eq!(only_fav, vec![0]);
    }

    #[test]
    fn filter_by_query_searches_stem_and_explanation() {
        let qs = questions();
        let (w, u) = empty();
        let hit = select(
            &qs,
            &Filter {
                query: "q3".to_string(),
                ..Default::default()
            },
            &w,
            &u,
        );
        assert_eq!(hit, vec![2], "matches the explanation of q3 only");

        let none = select(
            &qs,
            &Filter {
                query: "不存在的关键词".to_string(),
                ..Default::default()
            },
            &w,
            &u,
        );
        assert!(none.is_empty());
    }

    #[test]
    fn sources_unseen_missed_wrong_and_fav() {
        let qs = questions();
        let mut wrong = HashSet::new();
        wrong.insert("q3".to_string());
        let mut user = UserStore::default();
        user.toggle_favorite("q1");
        user.record_answer("q1", true, "");
        user.record_answer("q1", true, "");
        user.record_answer("q2", false, "");

        let unseen = select(
            &qs,
            &Filter {
                sources: Sources {
                    unseen: true,
                    ..Default::default()
                },
                ..Default::default()
            },
            &wrong,
            &user,
        );
        assert_eq!(unseen, vec![2, 3], "q1/q2 have attempts");

        let missed = select(
            &qs,
            &Filter {
                sources: Sources {
                    missed: true,
                    ..Default::default()
                },
                ..Default::default()
            },
            &wrong,
            &user,
        );
        assert_eq!(missed, vec![1], "attempted but never fully correct");

        let wrong_only = select(
            &qs,
            &Filter {
                sources: Sources {
                    wrong: true,
                    ..Default::default()
                },
                ..Default::default()
            },
            &wrong,
            &user,
        );
        assert_eq!(wrong_only, vec![2]);

        let fav_only = select(
            &qs,
            &Filter {
                sources: Sources {
                    fav: true,
                    ..Default::default()
                },
                ..Default::default()
            },
            &wrong,
            &user,
        );
        assert_eq!(fav_only, vec![0]);

        let combined = select(
            &qs,
            &Filter {
                sources: Sources {
                    unseen: true,
                    fav: true,
                    ..Default::default()
                },
                ..Default::default()
            },
            &wrong,
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
        let mut wrong = HashSet::new();
        wrong.insert("q1".to_string());
        wrong.insert("q3".to_string());

        let draw = wrong_sample(&qs, &wrong, 10, 7);
        assert_eq!(draw.len(), 2);
        for i in &draw {
            assert!(wrong.contains(&qs[*i].id), "only wrong questions may be drawn");
        }

        let capped = wrong_sample(&qs, &wrong, 1, 7);
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
        let seq = draw_plan((0..4).collect(), Draw::All, Order::Sequential, 9);
        assert_eq!(seq, vec![0, 1, 2, 3], "sequential keeps subject order");

        let capped = draw_plan((0..30).collect(), Draw::N20, Order::Sequential, 9);
        assert_eq!(
            capped,
            (0..20).collect::<Vec<_>>(),
            "sequential + cap is the first N"
        );

        let shuffled_all = draw_plan((0..4).collect(), Draw::All, Order::Shuffled, 9);
        assert_eq!(
            shuffled_all,
            shuffled((0..4).collect(), 9),
            "shuffle happens before the cap"
        );

        let shuffled_capped = draw_plan((0..30).collect(), Draw::N10, Order::Shuffled, 9);
        assert_eq!(shuffled_capped.len(), 10);
        assert_eq!(shuffled_capped, shuffled((0..30).collect(), 9)[..10].to_vec());

        let small_pool = draw_plan((0..3).collect(), Draw::N10, Order::Shuffled, 9);
        assert_eq!(small_pool.len(), 3, "a cap larger than the pool keeps everything");
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
}
