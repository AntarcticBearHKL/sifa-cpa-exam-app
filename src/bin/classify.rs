//! Re-runnable exam-taxonomy classifier.
//!
//! Reads one CMF corpus (`--data-root`), classifies every question into the
//! official taxonomy (`--taxonomy`), and writes per-subject files plus a
//! recomputed summary and a coverage report under `--out`.
//!
//! Invocation (all four arguments are required):
//!
//! ```text
//! classify --domain law --data-root D:\Dev\StudyApp\data\corpus\law \
//!          --taxonomy D:\Dev\StudyApp\data\law\taxonomy.json \
//!          --out D:\Dev\StudyApp\data\law
//! ```
//!
//! The classification itself is deterministic: it uses only the corpus, the
//! taxonomy, and fixed thresholds. `summary.json` carries a `generatedAt`
//! timestamp; the per-subject files do not.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use serde_json::{json, Map, Value};

use sfzklearning::{load_subject, now_iso, read_text, Domain, Question, SubjectFile};

const SUBJECT_MIN: f64 = 0.05;
// Tier-4 is the only numeric tier, so only these two are corpus-tuned.
// Candidate blended scores are either 0.0 or >= 0.00699 (law) / >= 0.01093
// (cpa); 0.005 sits inside that gap, 0.4 is the ~p40 of assigned Tier-4 scores.
const TIER4_CHAPTER_MIN: f64 = 0.005;
const TIER4_HIGH_MIN: f64 = 0.4;
const AMBIGUITY_MARGIN: f64 = 0.20;
const NGRAM_SIZES: [usize; 3] = [2, 3, 4];

const TIER4_KP_WEIGHT: f64 = 3.0;
const TIER4_CHAPTER_WEIGHT: f64 = 2.5;
const TIER4_TEXT_WEIGHT: f64 = 0.5;

// Stops a one-character chapter field from containment-matching every title.
const MIN_TITLE_CHARS: usize = 2;

const TIER_NONE: u8 = 0;

const UNASSIGNED_CHAPTER: &str = "（未分章）";

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct Args {
    domain: Domain,
    data_root: PathBuf,
    taxonomy: PathBuf,
    out: PathBuf,
}

fn usage() -> &'static str {
    "用法: classify --domain <law|cpa> --data-root <语料根目录> --taxonomy <taxonomy.json> --out <输出目录>"
}

fn parse_args() -> Result<Args, String> {
    let mut domain: Option<String> = None;
    let mut data_root: Option<String> = None;
    let mut taxonomy: Option<String> = None;
    let mut out: Option<String> = None;

    let argv: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < argv.len() {
        let key = argv[i].as_str();
        let value = || {
            argv.get(i + 1)
                .cloned()
                .ok_or_else(|| format!("缺少 {} 的值\n{}", key, usage()))
        };
        match key {
            "--domain" => {
                domain = Some(value()?);
                i += 2;
            }
            "--data-root" => {
                data_root = Some(value()?);
                i += 2;
            }
            "--taxonomy" => {
                taxonomy = Some(value()?);
                i += 2;
            }
            "--out" => {
                out = Some(value()?);
                i += 2;
            }
            "-h" | "--help" => return Err(usage().to_string()),
            other => return Err(format!("未知参数 `{other}`\n{}", usage())),
        }
    }

    let domain = domain.ok_or_else(|| format!("缺少 --domain\n{}", usage()))?;
    let domain = Domain::from_id(&domain)
        .ok_or_else(|| format!("--domain 必须是 law 或 cpa（收到 `{domain}`）\n{}", usage()))?;
    let data_root = data_root.ok_or_else(|| format!("缺少 --data-root\n{}", usage()))?;
    let taxonomy = taxonomy.ok_or_else(|| format!("缺少 --taxonomy\n{}", usage()))?;
    let out = out.ok_or_else(|| format!("缺少 --out\n{}", usage()))?;

    Ok(Args {
        domain,
        data_root: PathBuf::from(data_root),
        taxonomy: PathBuf::from(taxonomy),
        out: PathBuf::from(out),
    })
}

// ---------------------------------------------------------------------------
// Taxonomy
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct Taxonomy {
    #[serde(default)]
    exam: String,
    #[serde(default)]
    edition: String,
    #[serde(default)]
    stages: Vec<Stage>,
}

#[derive(Deserialize)]
struct Stage {
    #[serde(default)]
    name: String,
    #[serde(default)]
    subjects: Vec<TaxSubject>,
}

#[derive(Deserialize)]
struct TaxSubject {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    chapters: Vec<TaxChapter>,
}

#[derive(Deserialize)]
struct TaxChapter {
    #[serde(default)]
    no: i64,
    #[serde(default)]
    title: String,
    #[serde(default)]
    sections: Vec<String>,
}

struct Scorer {
    terms: BTreeMap<String, f64>,
    norm: f64,
}

impl Scorer {
    fn score(&self, text_grams: &HashSet<String>) -> f64 {
        if self.norm == 0.0 {
            return 0.0;
        }
        let sum: f64 = self
            .terms
            .iter()
            .filter(|(gram, _)| text_grams.contains(*gram))
            .map(|(_, weight)| *weight)
            .sum();
        sum / self.norm
    }
}

struct ChapterNode {
    title: String,
    key: String,
    no: i64,
    scorer: Scorer,
}

struct SubjectNode {
    id: String,
    name: String,
    group: String,
    order: i64,
    scorer: Scorer,
    chapters: Vec<ChapterNode>,
}

fn ngram_weight(length: usize) -> f64 {
    match length {
        2 => 1.0,
        3 => 2.5,
        4 => 4.0,
        _ => 1.0,
    }
}

fn gram_weight(gram: &str) -> f64 {
    ngram_weight(gram.chars().count())
}

fn idf(gram: &str, document_frequency: &HashMap<String, usize>, node_count: usize) -> f64 {
    let df = document_frequency.get(gram).copied().unwrap_or(0);
    (node_count as f64 / (1.0 + df as f64)).ln().max(0.0)
}

fn make_scorer(
    vocabulary: &HashSet<String>,
    document_frequency: &HashMap<String, usize>,
    node_count: usize,
) -> Scorer {
    // A sorted map keeps both the accumulation order and `score`'s sum
    // order fixed, so repeated runs are bit-identical.
    let mut terms = BTreeMap::new();
    for gram in vocabulary {
        let weight = gram_weight(gram) * idf(gram, document_frequency, node_count);
        if weight > 0.0 {
            terms.insert(gram.clone(), weight);
        }
    }
    let norm_squared: f64 = terms
        .iter()
        .map(|(gram, weight)| weight * idf(gram, document_frequency, node_count))
        .sum();
    Scorer {
        terms,
        norm: norm_squared.sqrt(),
    }
}

/// Flattens `stages[].subjects` into one ordered list (卷别 / 阶段 only supplies
/// the display group). Chapters keep their taxonomy order.
fn build_nodes(tax: &Taxonomy) -> Vec<SubjectNode> {
    struct RawChapter {
        no: i64,
        title: String,
        grams: HashSet<String>,
    }
    struct RawSubject {
        id: String,
        name: String,
        group: String,
        order: i64,
        grams: HashSet<String>,
        chapters: Vec<RawChapter>,
    }

    let mut raw_subjects = Vec::new();
    let mut order = 0i64;
    for stage in &tax.stages {
        for subject in &stage.subjects {
            order += 1;
            let mut vocabulary = String::new();
            vocabulary.push_str(&subject.name);
            vocabulary.push('\n');

            let mut chapters = Vec::new();
            for chapter in &subject.chapters {
                let mut chapter_vocab = chapter.title.clone();
                for section in &chapter.sections {
                    chapter_vocab.push('\n');
                    chapter_vocab.push_str(section);
                }
                vocabulary.push_str(&chapter_vocab);
                vocabulary.push('\n');
                chapters.push(RawChapter {
                    no: chapter.no,
                    title: chapter.title.clone(),
                    grams: ngram_set(&normalize(&chapter_vocab)),
                });
            }

            raw_subjects.push(RawSubject {
                id: subject.id.clone(),
                name: subject.name.clone(),
                group: stage.name.clone(),
                order,
                grams: ngram_set(&normalize(&vocabulary)),
                chapters,
            });
        }
    }

    let mut document_frequency: HashMap<String, usize> = HashMap::new();
    let mut node_count = 0usize;
    for subject in &raw_subjects {
        node_count += 1;
        for gram in &subject.grams {
            *document_frequency.entry(gram.clone()).or_insert(0) += 1;
        }
        for chapter in &subject.chapters {
            node_count += 1;
            for gram in &chapter.grams {
                *document_frequency.entry(gram.clone()).or_insert(0) += 1;
            }
        }
    }

    raw_subjects
        .into_iter()
        .map(|subject| SubjectNode {
            id: subject.id,
            name: subject.name,
            group: subject.group,
            order: subject.order,
            scorer: make_scorer(&subject.grams, &document_frequency, node_count),
            chapters: subject
                .chapters
                .into_iter()
                .map(|chapter| ChapterNode {
                    key: normalize_key(&chapter.title),
                    scorer: make_scorer(&chapter.grams, &document_frequency, node_count),
                    title: chapter.title,
                    no: chapter.no,
                })
                .collect(),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Character n-gram scoring
// ---------------------------------------------------------------------------

fn is_kept_char(c: char) -> bool {
    let c = c.to_ascii_lowercase();
    c.is_ascii_alphanumeric() || ('\u{4E00}'..='\u{9FFF}').contains(&c)
}

/// Drops whitespace/punctuation; keeps CJK ideographs and ASCII alphanumerics.
fn normalize(text: &str) -> Vec<char> {
    text.chars()
        .filter(|c| is_kept_char(*c))
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

fn ngram_set(chars: &[char]) -> HashSet<String> {
    let mut grams = HashSet::new();
    for n in NGRAM_SIZES {
        if chars.len() < n {
            continue;
        }
        for window in chars.windows(n) {
            grams.insert(window.iter().collect::<String>());
        }
    }
    grams
}

/// `normalize` as a comparable string key.
fn normalize_key(text: &str) -> String {
    normalize(text).into_iter().collect()
}

/// Model-generated chapter labels frequently carry a book chapter number
/// (`第十二章…`, `12.1 …`). Parses that number, if any.
fn extract_chapter_no(chapter: &str) -> Option<i64> {
    let chars: Vec<char> = chapter.chars().collect();
    if let Some(start) = chars.iter().position(|c| *c == '第') {
        if let Some(len) = chars[start + 1..].iter().position(|c| *c == '章') {
            let numeral: String = chars[start + 1..start + 1 + len].iter().collect();
            if let Some(no) = parse_numeral(numeral.trim()) {
                return Some(no);
            }
        }
    }
    let text = chapter.trim_start();
    let mut digits = String::new();
    for c in text.chars() {
        if c.is_ascii_digit() && digits.len() < 2 {
            digits.push(c);
            continue;
        }
        if c == '.' || c == '、' {
            return digits.parse::<i64>().ok().filter(|no| *no >= 1);
        }
        return None;
    }
    None
}

/// ASCII digits, or Chinese numerals up to 99 (`十`, `十一`, `二十`, `二十九`).
fn parse_numeral(text: &str) -> Option<i64> {
    if text.is_empty() {
        return None;
    }
    if text.chars().all(|c| c.is_ascii_digit()) {
        return text.parse::<i64>().ok().filter(|no| *no >= 1);
    }
    let mut total = 0i64;
    let mut section = 0i64;
    for c in text.chars() {
        match c {
            '十' => {
                total += section.max(1) * 10;
                section = 0;
            }
            _ => section = chinese_digit(c)?,
        }
    }
    let no = total + section;
    (no >= 1).then_some(no)
}

fn chinese_digit(c: char) -> Option<i64> {
    match c {
        '一' => Some(1),
        '二' => Some(2),
        '三' => Some(3),
        '四' => Some(4),
        '五' => Some(5),
        '六' => Some(6),
        '七' => Some(7),
        '八' => Some(8),
        '九' => Some(9),
        '零' => Some(0),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// Classification
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct Assignment {
    subject_id: String,
    chapter: Option<String>,
    confidence: &'static str,
    tier: u8,
}

impl Assignment {
    fn unassigned(subject_id: impl Into<String>) -> Self {
        Self {
            subject_id: subject_id.into(),
            chapter: None,
            confidence: "none",
            tier: TIER_NONE,
        }
    }
}

fn best_two<'a, T>(
    scores: impl Iterator<Item = (&'a T, f64)>,
) -> (Option<(&'a T, f64)>, f64) {
    let mut best: Option<(&T, f64)> = None;
    let mut second = f64::NEG_INFINITY;
    for (item, value) in scores {
        if best.map_or(true, |(_, current)| value > current) {
            if let Some((_, current)) = best {
                second = second.max(current);
            }
            best = Some((item, value));
        } else if value > second {
            second = value;
        }
    }
    (best, second)
}

fn is_ambiguous(best: f64, second: f64) -> bool {
    second.is_finite() && best > 0.0 && (best - second) < AMBIGUITY_MARGIN * best
}

/// Cross-exam contamination, CPA only: 初级/中级会计 material is not 注会.
fn is_cross_exam(text: &str) -> bool {
    if text.contains("初级会计") || text.contains("中级会计") {
        return true;
    }
    let junior_or_intermediate = text.contains("初级") || text.contains("中级");
    junior_or_intermediate && text.contains("会计实务")
}

/// `stem + explanation + chapter + knowledgePoints`, as specified.
fn question_text(question: &Question) -> (String, String) {
    let chapter = question.chapter.clone().unwrap_or_default();
    let mut text = String::with_capacity(
        question.stem.len()
            + question.explanation.as_deref().map_or(0, str::len)
            + chapter.len()
            + 4,
    );
    text.push_str(&question.stem);
    text.push('\n');
    text.push_str(question.explanation.as_deref().unwrap_or(""));
    text.push('\n');
    text.push_str(&chapter);
    text.push('\n');
    text.push_str(&question.knowledge_points.join(" "));
    (chapter, text)
}

/// Everything the tiers need, derived once per question.
struct Features {
    chapter: String,
    chapter_key: String,
    chapter_grams: HashSet<String>,
    kp_keys: Vec<String>,
    kp_chapter_keys: Vec<String>,
    kp_grams: Vec<HashSet<String>>,
    text: String,
    text_grams: HashSet<String>,
    stem_grams: HashSet<String>,
}

fn features(question: &Question, kp_chapters: &HashMap<String, String>) -> Features {
    let (chapter, text) = question_text(question);
    let mut kp_keys = Vec::with_capacity(question.knowledge_points.len());
    let mut kp_grams = Vec::with_capacity(question.knowledge_points.len());
    let mut kp_chapter_keys = Vec::new();
    for point in &question.knowledge_points {
        kp_keys.push(normalize_key(point));
        kp_grams.push(ngram_set(&normalize(point)));
        if let Some(kp_chapter) = kp_chapters.get(point) {
            let key = strip_leading_digits(&normalize_key(kp_chapter)).to_string();
            if key.chars().count() >= MIN_TITLE_CHARS {
                kp_chapter_keys.push(key);
            }
        }
    }
    let mut stem = question.stem.clone();
    stem.push('\n');
    stem.push_str(question.explanation.as_deref().unwrap_or(""));
    Features {
        chapter_key: normalize_key(&chapter),
        chapter_grams: ngram_set(&normalize(&chapter)),
        text_grams: ngram_set(&normalize(&text)),
        stem_grams: ngram_set(&normalize(&stem)),
        chapter,
        kp_keys,
        kp_chapter_keys,
        kp_grams,
        text,
    }
}

/// Corpus labels often carry the textbook's module number (`01-总论`, `12.2`).
/// `normalize` already drops the separator, so only the leading digits remain.
fn strip_leading_digits(key: &str) -> &str {
    key.trim_start_matches(|c: char| c.is_ascii_digit())
}

fn contains_either_way(query: &str, title: &str) -> bool {
    query.chars().count() >= MIN_TITLE_CHARS
        && title.chars().count() >= MIN_TITLE_CHARS
        && (query == title || query.contains(title) || title.contains(query))
}

fn longest_match<'a>(
    chapters: &'a [ChapterNode],
    matches: impl Fn(&ChapterNode) -> bool,
) -> Option<&'a ChapterNode> {
    let mut best: Option<&ChapterNode> = None;
    for chapter in chapters {
        if !matches(chapter) {
            continue;
        }
        if best.map_or(true, |current| chapter.key.chars().count() > current.key.chars().count()) {
            best = Some(chapter);
        }
    }
    best
}

fn tier1<'a>(subject: &'a SubjectNode, features: &Features) -> Option<&'a ChapterNode> {
    let key = strip_leading_digits(&features.chapter_key);
    if key.chars().count() < MIN_TITLE_CHARS {
        return None;
    }
    longest_match(&subject.chapters, |chapter| {
        contains_either_way(key, &chapter.key)
    })
}

fn tier2<'a>(subject: &'a SubjectNode, features: &Features) -> Option<&'a ChapterNode> {
    let no = extract_chapter_no(&features.chapter)?;
    let mut found: Option<&ChapterNode> = None;
    for chapter in &subject.chapters {
        if chapter.no != no {
            continue;
        }
        if found.is_some() {
            return None;
        }
        found = Some(chapter);
    }
    found
}

fn tier3<'a>(subject: &'a SubjectNode, features: &Features) -> Option<&'a ChapterNode> {
    let mut best: Option<&ChapterNode> = None;
    for key in features
        .kp_keys
        .iter()
        .chain(features.kp_chapter_keys.iter())
    {
        if key.chars().count() < MIN_TITLE_CHARS {
            continue;
        }
        let candidate = longest_match(&subject.chapters, |chapter| {
            key == &chapter.key || key.contains(&chapter.key)
        });
        if let Some(candidate) = candidate {
            if best.map_or(true, |current| candidate.key.chars().count() > current.key.chars().count())
            {
                best = Some(candidate);
            }
        }
    }
    best
}

fn tier4_score(chapter: &ChapterNode, features: &Features) -> f64 {
    let kp = features
        .kp_grams
        .iter()
        .map(|grams| chapter.scorer.score(grams))
        .fold(0.0f64, f64::max);
    let chapter_string = chapter.scorer.score(&features.chapter_grams);
    let text = chapter.scorer.score(&features.stem_grams);
    TIER4_KP_WEIGHT * kp + TIER4_CHAPTER_WEIGHT * chapter_string + TIER4_TEXT_WEIGHT * text
}

struct Decision<'a> {
    tier: u8,
    chapter: Option<&'a ChapterNode>,
    confidence: &'static str,
}

fn tier4_rank<'a>(
    subject: &'a SubjectNode,
    features: &Features,
) -> (Option<&'a ChapterNode>, f64, f64) {
    let (best, second) = best_two(
        subject
            .chapters
            .iter()
            .map(|chapter| (chapter, tier4_score(chapter, features))),
    );
    match best {
        Some((chapter, value)) => (Some(chapter), value, second),
        None => (None, 0.0, second),
    }
}

fn decide<'a>(
    subject: &'a SubjectNode,
    features: &Features,
    subject_ambiguous: bool,
) -> Decision<'a> {
    for (tier, matched) in [
        (1u8, tier1(subject, features)),
        (2u8, tier2(subject, features)),
        (3u8, tier3(subject, features)),
    ] {
        if let Some(chapter) = matched {
            return Decision {
                tier,
                chapter: Some(chapter),
                confidence: "high",
            };
        }
    }

    let (best, value, second) = tier4_rank(subject, features);
    match best {
        Some(chapter) if value >= TIER4_CHAPTER_MIN => {
            let ambiguous = subject_ambiguous || is_ambiguous(value, second);
            let confidence = if value >= TIER4_HIGH_MIN && !ambiguous {
                "high"
            } else {
                "low"
            };
            Decision {
                tier: 4,
                chapter: Some(chapter),
                confidence,
            }
        }
        _ => Decision {
            tier: TIER_NONE,
            chapter: None,
            confidence: "none",
        },
    }
}

fn classify(
    question: &Question,
    nodes: &[SubjectNode],
    cpa: bool,
    features: &Features,
) -> (Assignment, bool) {
    if cpa && is_cross_exam(&features.text) {
        return (Assignment::unassigned("excluded"), true);
    }

    let existing = question.subject_id.as_deref().unwrap_or("").trim();
    let (subject, subject_ambiguous) = match nodes.iter().find(|node| node.id == existing) {
        Some(node) => (node, false),
        None => {
            let (best, second) = best_two(
                nodes
                    .iter()
                    .map(|node| (node, node.scorer.score(&features.text_grams))),
            );
            match best {
                Some((node, value)) if value >= SUBJECT_MIN => {
                    (node, is_ambiguous(value, second))
                }
                _ => return (Assignment::unassigned("unknown"), false),
            }
        }
    };

    let decision = decide(subject, features, subject_ambiguous);
    (
        Assignment {
            subject_id: subject.id.clone(),
            chapter: decision.chapter.map(|chapter| chapter.title.clone()),
            confidence: decision.confidence,
            tier: decision.tier,
        },
        false,
    )
}

// ---------------------------------------------------------------------------
// Output buckets
// ---------------------------------------------------------------------------

#[derive(Default, Clone, Copy)]
struct Tally {
    high: usize,
    low: usize,
    none: usize,
}

impl Tally {
    fn add(&mut self, confidence: &str) {
        match confidence {
            "high" => self.high += 1,
            "low" => self.low += 1,
            _ => self.none += 1,
        }
    }

    fn total(&self) -> usize {
        self.high + self.low + self.none
    }
}

struct Bucket {
    id: String,
    name: String,
    group: Option<String>,
    order: Option<i64>,
    template: Map<String, Value>,
    knowledge_points: Vec<Value>,
    questions: Vec<Value>,
    chapters: BTreeMap<String, Tally>,
}

impl Bucket {
    fn new(id: &str, base: &Map<String, Value>) -> Self {
        let mut template = base.clone();
        template.remove("questions");
        template.remove("knowledgePoints");
        Self {
            id: id.to_string(),
            name: id.to_string(),
            group: None,
            order: None,
            template,
            knowledge_points: Vec::new(),
            questions: Vec::new(),
            chapters: BTreeMap::new(),
        }
    }

    fn set_meta(&mut self, name: &str, group: Option<&str>, order: Option<i64>) {
        self.name = name.to_string();
        self.group = group.map(str::to_string);
        self.order = order;
    }

    fn push_question(&mut self, value: Value, chapter: Option<&str>, confidence: &str) {
        self.questions.push(value);
        self.chapters
            .entry(
                chapter
                    .map(str::to_string)
                    .unwrap_or_else(|| UNASSIGNED_CHAPTER.to_string()),
            )
            .or_default()
            .add(confidence);
    }

    fn to_value(&self) -> Value {
        let mut object = self.template.clone();
        object.insert("subjectId".into(), Value::String(self.id.clone()));
        object.insert("name".into(), Value::String(self.name.clone()));
        object.insert(
            "group".into(),
            self.group.clone().map(Value::String).unwrap_or(Value::Null),
        );
        object.insert(
            "order".into(),
            self.order.map(|value| json!(value)).unwrap_or(Value::Null),
        );
        object.insert(
            "knowledgePoints".into(),
            Value::Array(self.knowledge_points.clone()),
        );
        object.insert("questions".into(), Value::Array(self.questions.clone()));
        Value::Object(object)
    }
}

fn ensure_bucket<'a>(
    buckets: &'a mut BTreeMap<String, Bucket>,
    base: &Map<String, Value>,
    id: &str,
) -> &'a mut Bucket {
    buckets
        .entry(id.to_string())
        .or_insert_with(|| Bucket::new(id, base))
}

// ---------------------------------------------------------------------------
// Corpus enumeration
// ---------------------------------------------------------------------------

fn subject_files(data_root: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let dir = data_root.join("subjects");
    let entries = fs::read_dir(&dir)
        .map_err(|error| format!("读取目录 {} 失败: {error}", dir.display()))?;
    let mut files = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|error| format!("枚举 {} 失败: {error}", dir.display()))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let stem = match path.file_stem().and_then(|stem| stem.to_str()) {
            Some(stem) => stem.to_string(),
            None => continue,
        };
        if stem == "index" {
            continue;
        }
        files.push((stem, path));
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(files)
}

// ---------------------------------------------------------------------------
// Pipeline
// ---------------------------------------------------------------------------

fn run(args: &Args) -> Result<(), String> {
    let taxonomy_text = read_text(&args.taxonomy)
        .map_err(|error| format!("读取 {} 失败: {error}", args.taxonomy.display()))?;
    let taxonomy: Taxonomy = serde_json::from_str(&taxonomy_text)
        .map_err(|error| format!("解析 {} 失败: {error}", args.taxonomy.display()))?;
    if !taxonomy.exam.is_empty() && taxonomy.exam != args.domain.id() {
        return Err(format!(
            "taxonomy 的 exam={} 与 --domain {} 不一致",
            taxonomy.exam,
            args.domain.id()
        ));
    }
    let nodes = build_nodes(&taxonomy);
    if nodes.is_empty() {
        return Err(format!(
            "taxonomy 没有可用的 subjects: {}",
            args.taxonomy.display()
        ));
    }
    let official_ids: HashSet<&str> = nodes.iter().map(|node| node.id.as_str()).collect();

    let files = subject_files(&args.data_root)?;
    if files.is_empty() {
        return Err(format!("{} 下没有 subjects/*.json", args.data_root.display()));
    }

    let subjects_dir = args.out.join("subjects");
    fs::create_dir_all(&subjects_dir)
        .map_err(|error| format!("创建目录 {} 失败: {error}", subjects_dir.display()))?;
    // Re-runnable: clear previously generated subject files.
    for entry in fs::read_dir(&subjects_dir)
        .map_err(|error| format!("读取目录 {} 失败: {error}", subjects_dir.display()))?
    {
        let entry =
            entry.map_err(|error| format!("枚举 {} 失败: {error}", subjects_dir.display()))?;
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|ext| ext.to_str()) == Some("json") {
            fs::remove_file(&path)
                .map_err(|error| format!("删除旧文件 {} 失败: {error}", path.display()))?;
        }
    }

    let cpa = args.domain == Domain::Cpa;
    let mut buckets: BTreeMap<String, Bucket> = BTreeMap::new();
    let mut base_template: Option<Map<String, Value>> = None;
    let mut excluded_count = 0usize;
    let mut total_questions = 0usize;
    let mut total_knowledge_points = 0usize;
    let mut tier_counts = [0usize; 5];
    let mut cross = CrossSubject::default();

    for (stem, path) in &files {
        let raw_text =
            read_text(path).map_err(|error| format!("读取 {} 失败: {error}", path.display()))?;
        let mut raw: Value = serde_json::from_str(&raw_text)
            .map_err(|error| format!("解析 {} 失败: {error}", path.display()))?;
        let parsed: SubjectFile = load_subject(&args.data_root, stem)?;

        let template = raw
            .as_object()
            .cloned()
            .ok_or_else(|| format!("{} 顶层不是 JSON 对象", path.display()))?;
        if base_template.is_none() {
            base_template = Some(template.clone());
        }
        let source_subject_id = parsed
            .subject_id
            .clone()
            .filter(|id| !id.trim().is_empty())
            .unwrap_or_else(|| stem.clone());

        let raw_questions = raw
            .get_mut("questions")
            .and_then(Value::as_array_mut)
            .ok_or_else(|| format!("{} 缺少 questions 数组", path.display()))?;
        if raw_questions.len() != parsed.questions.len() {
            return Err(format!(
                "{}: questions 数量不一致（raw {} != typed {}）",
                path.display(),
                raw_questions.len(),
                parsed.questions.len()
            ));
        }

        let kp_chapters: HashMap<String, String> = parsed
            .knowledge_points
            .iter()
            .filter_map(|point| {
                point
                    .chapter
                    .as_deref()
                    .filter(|chapter| !chapter.trim().is_empty())
                    .map(|chapter| (point.title.clone(), chapter.to_string()))
            })
            .collect();

        for (raw_question, question) in raw_questions.iter_mut().zip(parsed.questions.iter()) {
            let features = features(question, &kp_chapters);
            let (assignment, excluded) = classify(question, &nodes, cpa, &features);
            if excluded {
                excluded_count += 1;
            }
            tier_counts[assignment.tier as usize] += 1;
            cross.observe(&nodes, &features, &assignment);

            let object = raw_question
                .as_object_mut()
                .ok_or_else(|| format!("{} 有非对象题目", path.display()))?;
            object.insert(
                "subjectId".into(),
                Value::String(assignment.subject_id.clone()),
            );
            object.insert(
                "chapter".into(),
                assignment
                    .chapter
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null),
            );
            object.insert(
                "chapterOfficial".into(),
                assignment
                    .chapter
                    .clone()
                    .map(Value::String)
                    .unwrap_or(Value::Null),
            );
            object.insert(
                "chapterConfidence".into(),
                Value::String(assignment.confidence.to_string()),
            );

            let base = base_template
                .as_ref()
                .expect("base template set from the first source file");
            let bucket = ensure_bucket(&mut buckets, base, &assignment.subject_id);
            if let Some(node) = nodes.iter().find(|node| node.id == assignment.subject_id) {
                bucket.set_meta(&node.name, Some(&node.group), Some(node.order));
            } else if assignment.subject_id == "excluded" {
                bucket.set_meta("已排除（跨考试污染）", None, Some(1000));
            } else {
                // `unknown` and corpus-only buckets keep the source metadata, so
                // no taxonomy entry is invented.
                bucket.set_meta(&parsed.name, parsed.group.as_deref(), parsed.order);
            }
            bucket.push_question(
                raw_question.clone(),
                assignment.chapter.as_deref(),
                assignment.confidence,
            );
        }

        // Knowledge points are not classified; they stay with the source
        // subject bucket (official or corpus-only).
        let bucket = ensure_bucket(
            &mut buckets,
            base_template.as_ref().expect("base template"),
            &source_subject_id,
        );
        if official_ids.contains(source_subject_id.as_str()) {
            if let Some(node) = nodes.iter().find(|node| node.id == source_subject_id) {
                bucket.set_meta(&node.name, Some(&node.group), Some(node.order));
            }
        } else if source_subject_id != "excluded" {
            bucket.set_meta(&parsed.name, parsed.group.as_deref(), parsed.order);
        }
        bucket.template = {
            let mut template = template.clone();
            template.remove("questions");
            template.remove("knowledgePoints");
            template
        };
        total_knowledge_points += parsed.knowledge_points.len();
        bucket.knowledge_points.extend(
            raw.get("knowledgePoints")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
        );

        println!(
            "  {}: {} 题 / {} 知识点",
            stem,
            parsed.questions.len(),
            parsed.knowledge_points.len()
        );
        total_questions += parsed.questions.len();
    }

    // Write per-subject files.
    let mut written = 0usize;
    for bucket in buckets.values() {
        let path = subjects_dir.join(format!("{}.json", bucket.id));
        let text = serde_json::to_string_pretty(&bucket.to_value())
            .map_err(|error| format!("序列化 {} 失败: {error}", bucket.id))?;
        fs::write(&path, text.as_bytes())
            .map_err(|error| format!("写入 {} 失败: {error}", path.display()))?;
        written += 1;
    }

    // summary.json — recomputed from the actual buckets.
    let mut ordered: Vec<&Bucket> = buckets.values().collect();
    ordered.sort_by(|a, b| {
        a.order
            .unwrap_or(i64::MAX)
            .cmp(&b.order.unwrap_or(i64::MAX))
            .then_with(|| a.id.cmp(&b.id))
    });

    let total_questions_out: usize = ordered.iter().map(|bucket| bucket.questions.len()).sum();
    let total_kp_out: usize = ordered
        .iter()
        .map(|bucket| bucket.knowledge_points.len())
        .sum();
    let summary_subjects: Vec<Value> = ordered
        .iter()
        .map(|bucket| {
            json!({
                "id": bucket.id,
                "name": bucket.name,
                "group": bucket.group,
                "order": bucket.order,
                "questions": bucket.questions.len(),
                "knowledgePoints": bucket.knowledge_points.len(),
            })
        })
        .collect();
    let summary = json!({
        "schemaVersion": base_template
            .as_ref()
            .and_then(|template| template.get("schemaVersion"))
            .cloned()
            .unwrap_or_else(|| json!("cmf-1.0")),
        "domain": args.domain.id(),
        "generatedAt": now_iso(),
        "counts": {
            "questions": total_questions_out,
            "knowledgePoints": total_kp_out,
        },
        "subjects": summary_subjects,
    });
    let summary_path = args.out.join("summary.json");
    fs::write(
        &summary_path,
        serde_json::to_string_pretty(&summary)
            .map_err(|error| format!("序列化 summary 失败: {error}"))?
            .as_bytes(),
    )
    .map_err(|error| format!("写入 {} 失败: {error}", summary_path.display()))?;

    // classification-report.md
    let report = render_report(
        args.domain,
        &taxonomy.edition,
        &ordered,
        &nodes,
        excluded_count,
        total_questions_out,
        total_kp_out,
        &tier_counts,
        &cross,
    );
    let report_path = args.out.join("classification-report.md");
    fs::write(&report_path, report.as_bytes())
        .map_err(|error| format!("写入 {} 失败: {error}", report_path.display()))?;

    println!(
        "完成: {} 个源文件, 输入 {} 题 / {} 知识点, 输出 {} 题 / {} 知识点, 排除 {} 题, 写出 {} 个科目文件",
        files.len(),
        total_questions,
        total_knowledge_points,
        total_questions_out,
        total_kp_out,
        excluded_count,
        written
    );
    println!(
        "输出: {} , {} , {}",
        subjects_dir.display(),
        summary_path.display(),
        report_path.display()
    );
    Ok(())
}

// ---------------------------------------------------------------------------
// Report
// ---------------------------------------------------------------------------

fn md_escape(text: &str) -> String {
    text.replace('|', "\\|")
}

/// Questions whose assigned subject disagrees with the subject their free text
/// scores highest against. Surfaces source contamination such as 会计 material
/// filed under 财管, which the curated chapter tiers cannot repair.
#[derive(Default)]
struct CrossSubject {
    total: usize,
    pairs: HashMap<(String, String), usize>,
}

impl CrossSubject {
    fn observe(&mut self, nodes: &[SubjectNode], features: &Features, assignment: &Assignment) {
        if assignment.subject_id == "excluded" {
            return;
        }
        let (best, _) = best_two(
            nodes
                .iter()
                .map(|node| (node, node.scorer.score(&features.text_grams))),
        );
        let Some((implied, score)) = best else {
            return;
        };
        if score > 0.0 && implied.id != assignment.subject_id {
            self.total += 1;
            *self
                .pairs
                .entry((assignment.subject_id.clone(), implied.id.clone()))
                .or_insert(0) += 1;
        }
    }

    fn top(&self, limit: usize) -> Vec<(&str, &str, usize)> {
        let mut pairs: Vec<(&str, &str, usize)> = self
            .pairs
            .iter()
            .map(|((from, to), count)| (from.as_str(), to.as_str(), *count))
            .collect();
        pairs.sort_by(|a, b| {
            b.2.cmp(&a.2)
                .then_with(|| a.0.cmp(b.0))
                .then_with(|| a.1.cmp(b.1))
        });
        pairs.truncate(limit);
        pairs
    }
}

#[allow(clippy::too_many_arguments)]
fn render_report(
    domain: Domain,
    edition: &str,
    ordered: &[&Bucket],
    nodes: &[SubjectNode],
    excluded_count: usize,
    total_questions: usize,
    total_knowledge_points: usize,
    tier_counts: &[usize; 5],
    cross: &CrossSubject,
) -> String {
    let mut totals = Tally::default();
    for bucket in ordered {
        for tally in bucket.chapters.values() {
            totals.high += tally.high;
            totals.low += tally.low;
            totals.none += tally.none;
        }
    }

    let mut out = String::new();
    out.push_str(&format!(
        "# 分类覆盖报告 — {}（{}）\n\n",
        domain.label(),
        domain.id()
    ));
    out.push_str(&format!("- 考试: {} · 版本 {}\n", domain.id(), edition));
    out.push_str(&format!("- 总题数: {total_questions}\n"));
    out.push_str(&format!("- 知识点: {total_knowledge_points}\n"));
    out.push_str(&format!(
        "- 置信度: high {} · low {} · none {}\n",
        totals.high, totals.low, totals.none
    ));
    if domain == Domain::Cpa {
        out.push_str(&format!("- 跨考试排除（初级/中级会计）: {excluded_count}\n"));
    }
    out.push_str(&format!(
        "- 决策层级: 精确章名 {} · 章号 {} · 知识点 {} · 加权回退 {} · 未分配 {}\n",
        tier_counts[1], tier_counts[2], tier_counts[3], tier_counts[4], tier_counts[0]
    ));
    out.push('\n');

    for bucket in ordered {
        out.push_str(&format!(
            "## {}（{}） — {} 题 · {} 知识点\n\n",
            bucket.name,
            bucket.id,
            bucket.questions.len(),
            bucket.knowledge_points.len()
        ));
        if bucket.questions.is_empty() {
            out.push_str("> 该科目没有题目。\n\n");
            continue;
        }
        out.push_str("| 章节 | 题数 | high | low | none |\n");
        out.push_str("| --- | ---: | ---: | ---: | ---: |\n");

        let mut seen: HashSet<String> = HashSet::new();
        let node = nodes.iter().find(|node| node.id == bucket.id);
        if let Some(node) = node {
            for chapter in &node.chapters {
                let tally = bucket
                    .chapters
                    .get(&chapter.title)
                    .copied()
                    .unwrap_or_default();
                seen.insert(chapter.title.clone());
                out.push_str(&format!(
                    "| {} | {} | {} | {} | {} |\n",
                    md_escape(&chapter.title),
                    tally.total(),
                    tally.high,
                    tally.low,
                    tally.none
                ));
            }
        }
        for (chapter, tally) in &bucket.chapters {
            if seen.contains(chapter) {
                continue;
            }
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} |\n",
                md_escape(chapter),
                tally.total(),
                tally.high,
                tally.low,
                tally.none
            ));
        }

        let subject_tally = bucket
            .chapters
            .values()
            .fold(Tally::default(), |mut acc, tally| {
                acc.high += tally.high;
                acc.low += tally.low;
                acc.none += tally.none;
                acc
            });
        out.push_str(&format!(
            "| **合计** | **{}** | **{}** | **{}** | **{}** |\n\n",
            subject_tally.total(),
            subject_tally.high,
            subject_tally.low,
            subject_tally.none
        ));
    }

    out.push_str("## 总计\n\n");
    out.push_str("| 科目 | 题数 | high | low | none |\n");
    out.push_str("| --- | ---: | ---: | ---: | ---: |\n");
    for bucket in ordered {
        let tally = bucket
            .chapters
            .values()
            .fold(Tally::default(), |mut acc, tally| {
                acc.high += tally.high;
                acc.low += tally.low;
                acc.none += tally.none;
                acc
            });
        out.push_str(&format!(
            "| {}（{}） | {} | {} | {} | {} |\n",
            md_escape(&bucket.name),
            bucket.id,
            tally.total(),
            tally.high,
            tally.low,
            tally.none
        ));
    }
    out.push_str(&format!(
        "| **总计** | **{}** | **{}** | **{}** | **{}** |\n\n",
        totals.total(),
        totals.high,
        totals.low,
        totals.none
    ));

    let empty: Vec<&Bucket> = ordered
        .iter()
        .copied()
        .filter(|bucket| bucket.questions.is_empty())
        .collect();
    if !empty.is_empty() {
        out.push_str("## 异常提示\n\n");
        for bucket in empty {
            out.push_str(&format!(
                "- {}（{}）有 {} 个知识点但 0 道题。\n",
                bucket.name,
                bucket.id,
                bucket.knowledge_points.len()
            ));
        }
        out.push('\n');
    }

    out.push_str("## 学科错配\n\n");
    out.push_str(&format!(
        "题目文本更贴近另一学科（已分配学科 ≠ 文本最高分学科）的题数: {} / {}\n\n",
        cross.total, total_questions
    ));
    let top = cross.top(10);
    if top.is_empty() {
        out.push_str("> 无。\n\n");
    } else {
        out.push_str("| 已分配学科 | 文本更贴近 | 题数 |\n");
        out.push_str("| --- | --- | ---: |\n");
        for (from, to, count) in top {
            out.push_str(&format!("| {} | {} | {} |\n", md_escape(from), md_escape(to), count));
        }
        out.push('\n');
    }

    out
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn main() {
    let result = parse_args().and_then(|args| run(&args));
    if let Err(error) = result {
        eprintln!("classify: {error}");
        std::process::exit(1);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn chapter(title: &str, sections: &[&str]) -> TaxChapter {
        TaxChapter {
            no: 0,
            title: title.to_string(),
            sections: sections.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn numbered_chapter(no: i64, title: &str) -> TaxChapter {
        TaxChapter {
            no,
            title: title.to_string(),
            sections: Vec::new(),
        }
    }

    fn subject(id: &str, name: &str, chapters: Vec<TaxChapter>) -> TaxSubject {
        TaxSubject {
            id: id.to_string(),
            name: name.to_string(),
            chapters,
        }
    }

    fn taxonomy() -> Taxonomy {
        Taxonomy {
            exam: "law".into(),
            edition: "2026".into(),
            stages: vec![
                Stage {
                    name: "卷一".into(),
                    subjects: vec![subject(
                        "xingfa",
                        "刑法",
                        vec![
                            chapter("刑法概说", &["刑法的概念、性质、任务和机能"]),
                            chapter("犯罪构成", &["犯罪构成要件"]),
                        ],
                    )],
                },
                Stage {
                    name: "卷二".into(),
                    subjects: vec![subject(
                        "minfa",
                        "民法",
                        vec![chapter("民法概述", &["民法的调整对象"])],
                    )],
                },
            ],
        }
    }

    fn question(stem: &str, subject_id: Option<&str>, chapter: Option<&str>) -> Question {
        Question {
            id: "q1".into(),
            subject_id: subject_id.map(str::to_string),
            chapter: chapter.map(str::to_string),
            stem: stem.into(),
            ..Default::default()
        }
    }

    fn classify_with(taxonomy: &Taxonomy, question: &Question, cpa: bool) -> (Assignment, bool) {
        let nodes = build_nodes(taxonomy);
        let features = features(question, &HashMap::new());
        classify(question, &nodes, cpa, &features)
    }

    fn classify_one(question: &Question, cpa: bool) -> (Assignment, bool) {
        classify_with(&taxonomy(), question, cpa)
    }

    #[test]
    fn normalize_keeps_cjk_and_alphanumerics_only() {
        assert_eq!(
            normalize("刑法，第 1 条！Article 2。"),
            vec![
                '刑', '法', '第', '1', '条', 'a', 'r', 't', 'i', 'c', 'l', 'e', '2'
            ]
        );
    }

    #[test]
    fn ngram_score_is_length_normalized() {
        let vocabulary = ngram_set(&normalize("刑法概说"));
        let mut document_frequency = HashMap::new();
        for gram in &vocabulary {
            document_frequency.insert(gram.clone(), 1usize);
        }
        let scorer = make_scorer(&vocabulary, &document_frequency, 10);
        let text = ngram_set(&normalize("刑法的概念与刑法概说"));
        assert!(scorer.score(&text) > 0.0);

        let empty = make_scorer(&HashSet::new(), &HashMap::new(), 10);
        assert_eq!(empty.score(&text), 0.0);
    }

    #[test]
    fn specific_four_gram_beats_generic_two_gram() {
        let specific_vocabulary = ngram_set(&normalize("善意取得"));
        let generic_vocabulary = ngram_set(&normalize("责任"));
        let text = ngram_set(&normalize("本案责任与善意取得"));
        let document_frequency = HashMap::from([("责任".to_string(), 90usize)]);
        let specific = make_scorer(&specific_vocabulary, &document_frequency, 100).score(&text);
        let generic = make_scorer(&generic_vocabulary, &document_frequency, 100).score(&text);
        assert!(
            specific > generic,
            "specific 4-gram ({specific}) must beat generic 2-gram ({generic})"
        );
    }

    #[test]
    fn ubiquitous_generic_title_does_not_beat_specific_short_title() {
        let mut subjects = Vec::new();
        for index in 0..8 {
            subjects.push(subject(
                &format!("generic{index}"),
                "共同",
                vec![chapter("案件程序责任", &[])],
            ));
        }
        subjects.push(subject(
            "target",
            "测试",
            vec![
                chapter("案件程序责任概述", &[]),
                chapter("善意取得", &[]),
            ],
        ));
        let taxonomy = Taxonomy {
            exam: "law".into(),
            edition: "2026".into(),
            stages: vec![Stage {
                name: "卷一".into(),
                subjects,
            }],
        };
        let nodes = build_nodes(&taxonomy);
        let question = question(
            "案件程序责任与善意取得",
            Some("target"),
            None,
        );
        let features = features(&question, &HashMap::new());
        let (assignment, _) = classify(&question, &nodes, false, &features);
        assert_eq!(assignment.chapter.as_deref(), Some("善意取得"));
    }

    #[test]
    fn chapter_numbers_parse_from_curated_labels() {
        assert_eq!(extract_chapter_no("第十二章 涉外经济法律制度"), Some(12));
        assert_eq!(extract_chapter_no("12.1 外商投资准入"), Some(12));
        assert_eq!(extract_chapter_no("第七章资产减值"), Some(7));
        assert_eq!(extract_chapter_no("第二十一章企业合并与合并财务报表"), Some(21));
        assert_eq!(extract_chapter_no("第十章房产税法、契税法和土地增值税法"), Some(10));
        assert_eq!(extract_chapter_no("2023年·实务·真题"), None);
        assert_eq!(extract_chapter_no("cpa_one / accounting"), None);
        assert_eq!(extract_chapter_no("试卷一"), None);
    }

    #[test]
    fn tier1_matches_the_chapter_title_exactly() {
        let (assignment, _) = classify_with(
            &taxonomy(),
            &question("任何题干", Some("xingfa"), Some("犯罪构成")),
            false,
        );
        assert_eq!(assignment.tier, 1);
        assert_eq!(assignment.chapter.as_deref(), Some("犯罪构成"));
        assert_eq!(assignment.confidence, "high");
    }

    #[test]
    fn tier1_prefers_the_longest_matching_title() {
        let taxa = Taxonomy {
            exam: "law".into(),
            edition: "2026".into(),
            stages: vec![Stage {
                name: "卷一".into(),
                subjects: vec![subject(
                    "xingsufa",
                    "刑事诉讼法",
                    vec![
                        chapter("侦查", &[]),
                        chapter("侦查监督", &[]),
                    ],
                )],
            }],
        };
        let (assignment, _) = classify_with(
            &taxa,
            &question("任何题干", Some("xingsufa"), Some("第三章 侦查监督程序")),
            false,
        );
        assert_eq!(assignment.tier, 1);
        assert_eq!(assignment.chapter.as_deref(), Some("侦查监督"));
    }

    #[test]
    fn tier2_routes_a_numbered_chapter() {
        let taxa = Taxonomy {
            exam: "cpa".into(),
            edition: "2026".into(),
            stages: vec![Stage {
                name: "专业阶段".into(),
                subjects: vec![subject(
                    "jingjifa",
                    "经济法",
                    vec![
                        numbered_chapter(11, "反垄断法律制度"),
                        numbered_chapter(12, "涉外经济法律制度"),
                    ],
                )],
            }],
        };
        let (assignment, _) = classify_with(
            &taxa,
            &question(
                "外商投资准入负面清单如何适用",
                Some("jingjifa"),
                Some("12.1 外商投资准入"),
            ),
            false,
        );
        assert_eq!(assignment.tier, 2);
        assert_eq!(assignment.chapter.as_deref(), Some("涉外经济法律制度"));
        assert_eq!(assignment.confidence, "high");
    }

    #[test]
    fn tier2_accepts_a_chinese_chapter_number() {
        let taxa = Taxonomy {
            exam: "cpa".into(),
            edition: "2026".into(),
            stages: vec![Stage {
                name: "专业阶段".into(),
                subjects: vec![subject(
                    "jingjifa",
                    "经济法",
                    vec![
                        numbered_chapter(11, "反垄断法律制度"),
                        numbered_chapter(12, "涉外经济法律制度"),
                    ],
                )],
            }],
        };
        let (assignment, _) = classify_with(
            &taxa,
            &question("任何题干", Some("jingjifa"), Some("第十二章 未知标题")),
            false,
        );
        assert_eq!(assignment.tier, 2);
        assert_eq!(assignment.chapter.as_deref(), Some("涉外经济法律制度"));
    }

    #[test]
    fn tier3_matches_a_knowledge_point_title() {
        let mut question = question("任何题干", Some("xingfa"), Some("第五章 未知章节名"));
        question.knowledge_points = vec!["犯罪构成要件".into()];
        let (assignment, _) = classify_with(&taxonomy(), &question, false);
        assert_eq!(assignment.tier, 3);
        assert_eq!(assignment.chapter.as_deref(), Some("犯罪构成"));
        assert_eq!(assignment.confidence, "high");
    }

    #[test]
    fn tier4_falls_back_to_the_weighted_blend() {
        let mut question = question("犯罪构成要件包括哪些", Some("xingfa"), Some("完全不相关的标签"));
        question.knowledge_points = vec!["另一个标签".into()];
        let (assignment, _) = classify_with(&taxonomy(), &question, false);
        assert_eq!(assignment.tier, 4);
        assert_eq!(assignment.chapter.as_deref(), Some("犯罪构成"));
        assert!(matches!(assignment.confidence, "high" | "low"));
    }

    #[test]
    fn official_subject_is_kept_and_chapter_scored() {
        let (assignment, excluded) =
            classify_one(&question("刑法的概念与性质", Some("xingfa"), None), false);
        assert!(!excluded);
        assert_eq!(assignment.subject_id, "xingfa");
        assert_eq!(assignment.chapter.as_deref(), Some("刑法概说"));
        assert!(matches!(assignment.confidence, "high" | "low"));
    }

    #[test]
    fn unknown_subject_is_resolved_or_left_unknown() {
        let (resolved, _) = classify_one(
            &question("刑法的概念、性质、任务和机能", Some("unknown"), None),
            false,
        );
        assert_eq!(resolved.subject_id, "xingfa");

        let (unknown, _) =
            classify_one(&question("完全无关的一段文字", Some("unknown"), None), false);
        assert_eq!(unknown.subject_id, "unknown");
        assert_eq!(unknown.confidence, "none");
    }

    #[test]
    fn cpa_cross_exam_material_is_excluded() {
        assert!(is_cross_exam("初级会计实务 第三章"));
        assert!(is_cross_exam("中级 会计实务"));
        assert!(!is_cross_exam("注会会计 长期股权投资"));

        let (assignment, excluded) =
            classify_one(&question("初级会计实务 固定资产", Some("kuaiji"), None), true);
        assert!(excluded);
        assert_eq!(assignment.subject_id, "excluded");
        assert!(assignment.chapter.is_none());
        assert_eq!(assignment.confidence, "none");
    }

    #[test]
    fn output_extra_fields_are_ignored_by_the_app_loader() {
        let json = r#"{
            "id": "law:q1",
            "subjectId": "xingfa",
            "chapter": "刑法概说",
            "chapterOfficial": "刑法概说",
            "chapterConfidence": "high",
            "stem": "题干",
            "options": [],
            "answer": []
        }"#;
        let question: Question = serde_json::from_str(json).expect("app loader accepts extras");
        assert_eq!(question.chapter.as_deref(), Some("刑法概说"));
        assert_eq!(question.subject_id.as_deref(), Some("xingfa"));
    }

    #[test]
    fn dump_scores_for_tuning() {
        let Ok(taxonomy_path) = std::env::var("CLASSIFY_TRACE_TAX") else {
            return;
        };
        let data_root = std::env::var("CLASSIFY_TRACE_DATA").expect("CLASSIFY_TRACE_DATA");
        let out_path = std::env::var("CLASSIFY_TRACE_OUT").expect("CLASSIFY_TRACE_OUT");
        let data_root = PathBuf::from(data_root);

        let text = read_text(Path::new(&taxonomy_path)).unwrap();
        let taxonomy: Taxonomy = serde_json::from_str(&text).unwrap();
        let nodes = build_nodes(&taxonomy);
        let files = subject_files(&data_root).unwrap();
        let mut rows = String::new();
        for (stem, _) in &files {
            let parsed = load_subject(&data_root, stem).unwrap();
            for item in &parsed.questions {
                let features = features(item, &HashMap::new());
                let existing = item.subject_id.as_deref().unwrap_or("").trim();
                let official = nodes.iter().find(|node| node.id == existing);
                let (subject, subject_ambiguous) = match official {
                    Some(node) => (node, false),
                    None => {
                        let (best, second) = best_two(
                            nodes
                                .iter()
                                .map(|node| (node, node.scorer.score(&features.text_grams))),
                        );
                        match best {
                            Some((node, value)) if value >= SUBJECT_MIN => {
                                (node, is_ambiguous(value, second))
                            }
                            _ => continue,
                        }
                    }
                };
                if taxonomy.exam == "cpa" && is_cross_exam(&features.text) {
                    continue;
                }
                let decision = decide(subject, &features, subject_ambiguous);
                let (_, best, second) = tier4_rank(subject, &features);
                rows.push_str(&format!(
                    "{}\t{:.6}\t{:.6}\t{}\t{}\t{}\t{}\n",
                    decision.tier,
                    best,
                    second,
                    if official.is_some() { 1 } else { 0 },
                    subject.id,
                    decision
                        .chapter
                        .map(|chapter| chapter.title.as_str())
                        .unwrap_or(""),
                    features.chapter
                ));
            }
        }
        std::fs::write(&out_path, rows).unwrap();
    }
}
