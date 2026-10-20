//! sfzklearning shared library.
//!
//! Responsibilities:
//! * The CMF data model (`summary.json` + `subjects/<id>.json`).
//! * Tolerant loaders for the two on-disk domains (法考 / 注会).
//! * The GUI -> MCP "current context" contract (`state/current.json`).

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------------------
// Domain
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Domain {
    Law,
    Cpa,
}

impl Domain {
    pub const ALL: [Domain; 2] = [Domain::Law, Domain::Cpa];

    pub fn id(self) -> &'static str {
        match self {
            Domain::Law => "law",
            Domain::Cpa => "cpa",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Domain::Law => "法考",
            Domain::Cpa => "注会",
        }
    }

    pub fn from_id(s: &str) -> Option<Domain> {
        match s.trim() {
            "law" => Some(Domain::Law),
            "cpa" => Some(Domain::Cpa),
            _ => None,
        }
    }

    pub fn env_var(self) -> &'static str {
        match self {
            Domain::Law => "CMF_LAW_DATA",
            Domain::Cpa => "CMF_CPA_DATA",
        }
    }

    /// Where a domain's data is looked up when no CLI/env override is set.
    ///
    /// Portable layout first (`<exe>/data/<domain>` — the release zip unpacks
    /// the binary and `data/` side by side), then the Cargo layout
    /// (`target/<profile>/../../data/<domain>` → project root).
    pub fn default_root(self) -> PathBuf {
        let id = self.id();
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                let beside = dir.join("data").join(id);
                if beside.is_dir() {
                    return beside;
                }
                if let Some(root) = dir.parent().and_then(|p| p.parent()) {
                    let in_project = root.join("data").join(id);
                    if in_project.is_dir() || root.join("Cargo.toml").is_file() {
                        return in_project;
                    }
                }
                return beside;
            }
        }
        PathBuf::from("data").join(id)
    }
}

// ---------------------------------------------------------------------------
// JSON helpers (tolerant: the corpus is heterogeneous)
// ---------------------------------------------------------------------------

pub fn value_to_string(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

/// Accepts `null` | `"s"` | `123` | `true` -> `String`.
fn de_string_lenient<'de, D>(d: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let v = Value::deserialize(d)?;
    Ok(value_to_string(&v))
}

/// Accepts `null` -> `None`, anything else -> best-effort string.
fn de_opt_string_lenient<'de, D>(d: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let v = Value::deserialize(d)?;
    Ok(match v {
        Value::Null => None,
        other => Some(value_to_string(&other)),
    })
}

/// Accepts `null` | number | numeric-string -> `Option<i64>`.
fn de_opt_i64<'de, D>(d: D) -> Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    let v = Value::deserialize(d)?;
    Ok(match v {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        Value::String(s) => s.trim().parse::<i64>().ok(),
        _ => None,
    })
}

/// Accepts `null` | `"x"` | `["a","b"]` -> `Vec<String>` (non-strings dropped).
fn de_str_or_vec<'de, D>(d: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let v = Value::deserialize(d)?;
    let mut out = Vec::new();
    match v {
        Value::Null => {}
        Value::String(s) => {
            if !s.is_empty() {
                out.push(s);
            }
        }
        Value::Array(arr) => {
            for item in arr {
                match item {
                    Value::String(s) => out.push(s),
                    Value::Number(n) => out.push(n.to_string()),
                    Value::Bool(b) => out.push(b.to_string()),
                    _ => {}
                }
            }
        }
        other => out.push(value_to_string(&other)),
    }
    Ok(out)
}

fn default_label(i: usize) -> String {
    if i < 26 {
        ((b'A' + i as u8) as char).to_string()
    } else {
        format!("{}", i + 1)
    }
}

/// Accepts an array of `{label,text}` objects, bare strings, or anything.
fn de_options<'de, D>(d: D) -> Result<Vec<OptionItem>, D::Error>
where
    D: Deserializer<'de>,
{
    let v = Value::deserialize(d)?;
    let mut out = Vec::new();
    if let Value::Array(arr) = v {
        for (i, item) in arr.into_iter().enumerate() {
            match item {
                Value::Object(map) => {
                    let label = map
                        .get("label")
                        .map(value_to_string)
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| default_label(i));
                    let text = map
                        .get("text")
                        .or_else(|| map.get("content"))
                        .map(value_to_string)
                        .unwrap_or_default();
                    out.push(OptionItem { label, text });
                }
                Value::String(s) => out.push(OptionItem {
                    label: default_label(i),
                    text: s,
                }),
                other => out.push(OptionItem {
                    label: default_label(i),
                    text: value_to_string(&other),
                }),
            }
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// CMF data model
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Source {
    #[serde(default, deserialize_with = "de_opt_string_lenient")]
    pub repo: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string_lenient")]
    pub path: Option<String>,
    #[serde(default, rename = "nativeId", deserialize_with = "de_opt_string_lenient")]
    pub native_id: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string_lenient")]
    pub url: Option<String>,
}

impl Source {
    /// `repo · path · url` — the provenance line shown in the UI.
    pub fn provenance(&self) -> String {
        let mut parts = Vec::new();
        if let Some(r) = self.repo.as_ref().filter(|s| !s.is_empty()) {
            parts.push(r.clone());
        }
        if let Some(p) = self.path.as_ref().filter(|s| !s.is_empty()) {
            parts.push(p.clone());
        }
        if let Some(u) = self.url.as_ref().filter(|s| !s.is_empty()) {
            parts.push(u.clone());
        }
        if parts.is_empty() {
            "（无出处信息）".to_string()
        } else {
            parts.join(" · ")
        }
    }

    pub fn to_state(&self) -> StateSource {
        StateSource {
            repo: self.repo.clone(),
            path: self.path.clone(),
            url: self.url.clone(),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct KnowledgePoint {
    #[serde(default, deserialize_with = "de_string_lenient")]
    pub id: String,
    #[serde(default, rename = "subjectId", deserialize_with = "de_opt_string_lenient")]
    pub subject_id: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string_lenient")]
    pub chapter: Option<String>,
    #[serde(default, deserialize_with = "de_string_lenient")]
    pub title: String,
    #[serde(default, deserialize_with = "de_opt_string_lenient")]
    pub content: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string_lenient")]
    pub summary: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string_lenient")]
    pub mnemonic: Option<String>,
    #[serde(default, rename = "parentId", deserialize_with = "de_opt_string_lenient")]
    pub parent_id: Option<String>,
    #[serde(default, deserialize_with = "de_opt_i64")]
    pub level: Option<i64>,
    #[serde(default, deserialize_with = "de_opt_string_lenient")]
    pub frequency: Option<String>,
    #[serde(default, deserialize_with = "de_str_or_vec")]
    pub tags: Vec<String>,
    #[serde(default, deserialize_with = "de_opt_i64")]
    pub order: Option<i64>,
    #[serde(default)]
    pub source: Option<Source>,
}

impl KnowledgePoint {
    pub fn chapter_label(&self) -> String {
        self.chapter
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "（未分章）".to_string())
    }

    pub fn frequency_label(&self) -> Option<&'static str> {
        match self.frequency.as_deref().map(str::trim) {
            Some("high") => Some("高频"),
            Some("medium") => Some("中频"),
            Some("low") => Some("低频"),
            _ => None,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct OptionItem {
    #[serde(default, deserialize_with = "de_string_lenient")]
    pub label: String,
    #[serde(default, deserialize_with = "de_string_lenient")]
    pub text: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Question {
    #[serde(default, deserialize_with = "de_string_lenient")]
    pub id: String,
    #[serde(default, rename = "subjectId", deserialize_with = "de_opt_string_lenient")]
    pub subject_id: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string_lenient")]
    pub chapter: Option<String>,
    #[serde(default, rename = "type", deserialize_with = "de_opt_string_lenient")]
    pub qtype: Option<String>,
    #[serde(default, rename = "typeRaw", deserialize_with = "de_opt_string_lenient")]
    pub type_raw: Option<String>,
    #[serde(default, deserialize_with = "de_string_lenient")]
    pub stem: String,
    #[serde(default, deserialize_with = "de_options")]
    pub options: Vec<OptionItem>,
    #[serde(default, deserialize_with = "de_str_or_vec")]
    pub answer: Vec<String>,
    #[serde(default, rename = "answerText", deserialize_with = "de_opt_string_lenient")]
    pub answer_text: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string_lenient")]
    pub explanation: Option<String>,
    #[serde(default)]
    pub year: Value,
    #[serde(default, rename = "knowledgePoints", deserialize_with = "de_str_or_vec")]
    pub knowledge_points: Vec<String>,
    #[serde(default)]
    pub difficulty: Value,
    #[serde(default)]
    pub score: Value,
    #[serde(default)]
    pub subquestions: Vec<Value>,
    #[serde(default, deserialize_with = "de_str_or_vec")]
    pub tags: Vec<String>,
    #[serde(default)]
    pub source: Option<Source>,
}

impl Question {
    /// `true` when this item is a real 选择题 (has options), otherwise 主观题
    /// (no options — shown in the display-only 主观题 mode).
    pub fn is_choice(&self) -> bool {
        !self.options.is_empty()
    }

    /// single vs multiple: explicit type first, then answer length.
    pub fn is_multi(&self) -> bool {
        let t = self
            .qtype
            .as_deref()
            .or(self.type_raw.as_deref())
            .unwrap_or("")
            .to_ascii_lowercase();
        if t.contains("multiple") || t.contains("multi") || t.contains("不定项") || t.contains("多选") {
            return true;
        }
        if t.contains("single") || t.contains("单选") {
            return false;
        }
        self.answer.len() > 1
    }

    /// Human readable 题型 badge.
    pub fn type_label(&self) -> String {
        if !self.is_choice() {
            let raw = self.type_raw.as_deref().unwrap_or("");
            if raw.is_empty() {
                return "主观题".to_string();
            }
            return format!("主观题（{}）", raw);
        }
        match self.qtype.as_deref().unwrap_or("") {
            "single_choice" => "单选",
            "multiple_choice" => "多选",
            "indefinite_choice" => "不定项",
            other if !other.is_empty() => other,
            _ => {
                if self.is_multi() {
                    "多选"
                } else {
                    "单选"
                }
            }
        }
        .to_string()
    }

    pub fn chapter_label(&self) -> String {
        self.chapter
            .clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| "（未分章）".to_string())
    }

    pub fn year_label(&self) -> Option<String> {
        match &self.year {
            Value::Null => None,
            Value::String(s) if s.trim().is_empty() => None,
            other => Some(value_to_string(other)),
        }
    }

    pub fn answer_sorted(&self) -> Vec<String> {
        let mut a = self.answer.clone();
        a.sort();
        a
    }

    /// Correct when the selection set equals the answer set (case/space-insensitive).
    pub fn grade(&self, selected: &[String]) -> bool {
        let norm = |v: &[String]| {
            let mut s: Vec<String> = v
                .iter()
                .map(|x| x.trim().to_ascii_uppercase())
                .filter(|x| !x.is_empty())
                .collect();
            s.sort();
            s.dedup();
            s
        };
        norm(selected) == norm(&self.answer)
    }

    pub fn stem_preview(&self, max: usize) -> String {
        let one_line: String = self
            .stem
            .chars()
            .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
            .collect();
        let t = one_line.trim();
        if t.chars().count() <= max {
            t.to_string()
        } else {
            let s: String = t.chars().take(max).collect();
            format!("{}…", s)
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct SubjectFile {
    #[serde(default, rename = "subjectId", deserialize_with = "de_opt_string_lenient")]
    pub subject_id: Option<String>,
    #[serde(default, deserialize_with = "de_string_lenient")]
    pub name: String,
    #[serde(default, deserialize_with = "de_opt_string_lenient")]
    pub group: Option<String>,
    #[serde(default, deserialize_with = "de_opt_i64")]
    pub order: Option<i64>,
    #[serde(default, rename = "knowledgePoints")]
    pub knowledge_points: Vec<KnowledgePoint>,
    #[serde(default)]
    pub questions: Vec<Question>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct SummarySubject {
    #[serde(default, deserialize_with = "de_string_lenient")]
    pub id: String,
    #[serde(default, deserialize_with = "de_string_lenient")]
    pub name: String,
    #[serde(default, deserialize_with = "de_opt_string_lenient")]
    pub group: Option<String>,
    #[serde(default, deserialize_with = "de_opt_i64")]
    pub order: Option<i64>,
    #[serde(default, deserialize_with = "de_opt_i64")]
    pub questions: Option<i64>,
    #[serde(default, rename = "knowledgePoints", deserialize_with = "de_opt_i64")]
    pub knowledge_points: Option<i64>,
    #[serde(default, deserialize_with = "de_opt_i64")]
    pub resources: Option<i64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Summary {
    #[serde(default, rename = "schemaVersion", deserialize_with = "de_opt_string_lenient")]
    pub schema_version: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string_lenient")]
    pub domain: Option<String>,
    #[serde(default)]
    pub subjects: Vec<SummarySubject>,
}

/// The domain's question total as `summary.json` states it. Subjects with no
/// recorded count contribute nothing, so an absent `questions` field cannot turn
/// the 新题 figure into a wild number.
pub fn subject_question_total(subjects: &[SummarySubject]) -> usize {
    subjects
        .iter()
        .filter_map(|s| s.questions)
        .filter(|n| *n > 0)
        .map(|n| n as usize)
        .sum()
}

// ---------------------------------------------------------------------------
// Loaders
// ---------------------------------------------------------------------------

/// Reads a text file as UTF-8, tolerating/stripping a leading BOM.
pub fn read_text(path: &Path) -> Result<String, String> {
    let bytes = fs::read(path).map_err(|e| e.to_string())?;
    let bytes = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        &bytes[3..]
    } else {
        &bytes[..]
    };
    String::from_utf8(bytes.to_vec()).map_err(|e| e.to_string())
}

pub fn subject_file_path(root: &Path, subject_id: &str) -> PathBuf {
    root.join("subjects").join(format!("{subject_id}.json"))
}

pub fn load_summary(root: &Path) -> Result<Summary, String> {
    let p = root.join("summary.json");
    let text = read_text(&p).map_err(|e| format!("读取 {} 失败: {e}", p.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("解析 {} 失败: {e}", p.display()))
}

/// Join the stray spaces a reflowed source left inside Chinese text.
///
/// The corpus was reflowed with a space at every source line break, which is
/// right for English and wrong for Chinese: `不是公\n民获得` arrived as
/// `不是公 民获得`. In prose a space whose neighbours are **both** non-ASCII is
/// always that artefact, so it is dropped (the whole run, so `站  在`
/// collapses too). A space with an ASCII neighbour is kept: `Word 文档`,
/// `A 项` and `2002 年` are either real or read correctly either way.
///
/// Deliberately **not** applied to `explanation`: it is Markdown, where
/// `### 第一章 法的本体` uses the same CJK-space-CJK shape on purpose.
///
/// Applied on load on top of the same fix in the shipped data, so a
/// regenerated subject file cannot put the artefact back on screen.
pub fn join_cjk_spaces(text: &str) -> String {
    if !text.contains(' ') {
        return text.to_owned();
    }
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != ' ' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let mut j = i;
        while j < chars.len() && chars[j] == ' ' {
            j += 1;
        }
        let inside_cjk =
            i > 0 && !chars[i - 1].is_ascii() && j < chars.len() && !chars[j].is_ascii();
        if !inside_cjk {
            out.extend(&chars[i..j]);
        }
        i = j;
    }
    out
}

/// [`join_cjk_spaces`] over the prose lines of a Markdown block.
///
/// Explanations are Markdown, so a heading or a table row keeps its spacing:
/// `### 第一章 法的本体` and `| 第一章 法的本体 | 甲 |` have exactly the shape
/// of the artefact. Every other line is prose and is joined.
pub fn join_cjk_spaces_in_markdown(text: &str) -> String {
    if !text.contains(' ') {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let head = line.trim_start();
        if head.starts_with('#') || line.contains('|') {
            out.push_str(line);
        } else {
            out.push_str(&join_cjk_spaces(line));
        }
    }
    out
}

pub fn load_subject(root: &Path, subject_id: &str) -> Result<SubjectFile, String> {
    let p = subject_file_path(root, subject_id);
    let text = read_text(&p).map_err(|e| format!("读取 {} 失败: {e}", p.display()))?;
    let mut file: SubjectFile =
        serde_json::from_str(&text).map_err(|e| format!("解析 {} 失败: {e}", p.display()))?;
    for q in &mut file.questions {
        q.stem = join_cjk_spaces(&q.stem);
        for o in &mut q.options {
            o.text = join_cjk_spaces(&o.text);
        }
        if let Some(t) = q.answer_text.as_mut() {
            *t = join_cjk_spaces(t);
        }
        if let Some(e) = q.explanation.as_mut() {
            *e = join_cjk_spaces_in_markdown(e);
        }
    }
    Ok(file)
}

/// Subject list for a domain, preferring `summary.json` but tolerating its absence.
pub fn list_subjects(root: &Path) -> Result<Vec<SummarySubject>, String> {
    match load_summary(root) {
        Ok(s) if !s.subjects.is_empty() => {
            let mut subs = s.subjects;
            subs.sort_by_key(|x| x.order.unwrap_or(i64::MAX));
            Ok(subs)
        }
        _ => {
            // Fallback: subjects/index.json
            let p = root.join("subjects").join("index.json");
            let text = read_text(&p).map_err(|e| format!("读取 {} 失败: {e}", p.display()))?;
            let v: Value =
                serde_json::from_str(&text).map_err(|e| format!("解析 {} 失败: {e}", p.display()))?;
            let mut subs = Vec::new();
            if let Some(arr) = v.get("subjects").and_then(|x| x.as_array()) {
                for it in arr {
                    subs.push(SummarySubject {
                        id: it.get("id").map(value_to_string).unwrap_or_default(),
                        name: it.get("name").map(value_to_string).unwrap_or_default(),
                        group: it.get("group").map(value_to_string),
                        order: it.get("order").and_then(|x| x.as_i64()),
                        questions: it.get("questions").and_then(|x| x.as_i64()),
                        knowledge_points: it.get("knowledgePoints").and_then(|x| x.as_i64()),
                        resources: it.get("resources").and_then(|x| x.as_i64()),
                    });
                }
            }
            Ok(subs)
        }
    }
}

// ---------------------------------------------------------------------------
// state/current.json contract (GUI writes, MCP reads)
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct StateSource {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
}

impl StateSource {
    pub fn provenance(&self) -> String {
        let mut parts = Vec::new();
        if let Some(r) = self.repo.as_ref().filter(|s| !s.is_empty()) {
            parts.push(r.clone());
        }
        if let Some(p) = self.path.as_ref().filter(|s| !s.is_empty()) {
            parts.push(p.clone());
        }
        if let Some(u) = self.url.as_ref().filter(|s| !s.is_empty()) {
            parts.push(u.clone());
        }
        if parts.is_empty() {
            "（无出处信息）".to_string()
        } else {
            parts.join(" · ")
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct StateOption {
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub text: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct StateQuestion {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub stem: String,
    #[serde(default, rename = "type")]
    pub type_: Option<String>,
    #[serde(default, rename = "typeRaw")]
    pub type_raw: Option<String>,
    #[serde(default)]
    pub chapter: Option<String>,
    #[serde(default)]
    pub year: Option<Value>,
    #[serde(default)]
    pub options: Vec<StateOption>,
    #[serde(default)]
    pub answer: Vec<String>,
    #[serde(default, rename = "answerText")]
    pub answer_text: Option<String>,
    #[serde(default)]
    pub explanation: Option<String>,
    #[serde(default, rename = "knowledgePoints")]
    pub knowledge_points: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub favorite: bool,
    #[serde(default)]
    pub source: StateSource,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct StateKp {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub chapter: Option<String>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub mnemonic: Option<String>,
    #[serde(default)]
    pub summary: Option<String>,
    #[serde(default)]
    pub source: StateSource,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct CurrentContext {
    #[serde(default)]
    pub ts: String,
    #[serde(default)]
    pub domain: String,
    #[serde(default)]
    pub tab: String,
    #[serde(default, rename = "subjectId")]
    pub subject_id: String,
    #[serde(default, rename = "subjectName")]
    pub subject_name: String,
    #[serde(default)]
    pub question: Option<StateQuestion>,
    #[serde(default, rename = "knowledgePoints")]
    pub knowledge_points: Vec<StateKp>,
}

impl CurrentContext {
    pub fn domain_label(&self) -> &'static str {
        match self.domain.as_str() {
            "law" => "法考",
            "cpa" => "注会",
            _ => "未知",
        }
    }

    pub fn tab_label(&self) -> &'static str {
        match self.tab.as_str() {
            "learn" => "学习",
            "practice" => "做题",
            _ => "未知",
        }
    }

    /// Markdown summary consumed by the MCP `get_current_study_context` tool.
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        out.push_str("# 当前学习上下文\n\n");
        out.push_str(&format!(
            "- 领域: {}（{}）\n- 科目: {}（{}）\n- 当前标签页: {}\n- 记录时间: {}\n\n",
            self.domain_label(),
            self.domain,
            if self.subject_name.is_empty() {
                "（未选择）"
            } else {
                &self.subject_name
            },
            self.subject_id,
            self.tab_label(),
            self.ts,
        ));

        if let Some(q) = &self.question {
            out.push_str("## 当前题目\n\n");
            out.push_str(&format!("- 题目ID: {}\n", q.id));
            out.push_str(&format!(
                "- 题型: {}\n",
                q.type_.clone().unwrap_or_else(|| "（未知）".to_string())
            ));
            if let Some(raw) = q.type_raw.as_ref().filter(|s| !s.is_empty()) {
                out.push_str(&format!("- 原始题型: {}\n", raw));
            }
            if let Some(c) = q.chapter.as_ref().filter(|s| !s.is_empty()) {
                out.push_str(&format!("- 章节: {}\n", c));
            }
            if let Some(y) = q.year.as_ref().filter(|v| !v.is_null()) {
                out.push_str(&format!("- 年份: {}\n", value_to_string(y)));
            }
            out.push_str("\n### 题干\n\n");
            out.push_str(q.stem.trim());
            out.push_str("\n\n");

            if !q.options.is_empty() {
                out.push_str("### 选项\n\n");
                for o in &q.options {
                    out.push_str(&format!("- **{}**. {}\n", o.label, o.text));
                }
                out.push('\n');
            }

            if !q.answer.is_empty() {
                out.push_str(&format!("### 正确答案\n\n{}\n\n", q.answer.join("、")));
            }
            if let Some(at) = q.answer_text.as_ref().filter(|s| !s.trim().is_empty()) {
                out.push_str(&format!("### 参考答案\n\n{}\n\n", at.trim()));
            }
            if let Some(exp) = q.explanation.as_ref().filter(|s| !s.trim().is_empty()) {
                out.push_str("### 解析\n\n");
                out.push_str(exp.trim());
                out.push_str("\n\n");
            }
            if q.favorite {
                out.push_str("### 用户标记\n\n已收藏\n\n");
            }
            if let Some(note) = q.note.as_ref().filter(|s| !s.trim().is_empty()) {
                out.push_str(&format!("### 用户备注\n\n{}\n\n", note.trim()));
            }
            if !q.knowledge_points.is_empty() {
                out.push_str("### 题目关联知识点（标题）\n\n");
                for t in &q.knowledge_points {
                    out.push_str(&format!("- {}\n", t));
                }
                out.push('\n');
            }
            out.push_str(&format!("- 出处: {}\n\n", q.source.provenance()));
        } else {
            out.push_str("## 当前题目\n\n（学习模式：当前未在做题）\n\n");
        }

        out.push_str("## 关联知识点\n\n");
        if self.knowledge_points.is_empty() {
            out.push_str("（无）\n");
        } else {
            for (i, kp) in self.knowledge_points.iter().enumerate() {
                out.push_str(&format!("### {}. {}\n\n", i + 1, kp.title));
                if let Some(c) = kp.chapter.as_ref().filter(|s| !s.trim().is_empty()) {
                    out.push_str(&format!("- 章节: {}\n", c));
                }
                if let Some(s) = kp.summary.as_ref().filter(|s| !s.trim().is_empty()) {
                    out.push_str(&format!("- 要点小结: {}\n", s.trim()));
                }
                if let Some(m) = kp.mnemonic.as_ref().filter(|s| !s.trim().is_empty()) {
                    out.push_str(&format!("- 记忆口诀: {}\n", m.trim()));
                }
                out.push_str(&format!("- 出处: {}\n", kp.source.provenance()));
                if let Some(content) = kp.content.as_ref().filter(|s| !s.trim().is_empty()) {
                    out.push_str("\n**内容：**\n\n");
                    out.push_str(content.trim());
                    out.push('\n');
                }
                out.push('\n');
            }
        }

        out
    }
}

/// Resolve the state directory.
///
/// Order: explicit CLI `--state-dir` > `CMF_STATE_DIR` env > `<exe>/state`
/// (portable: the binary and `state/` side by side) > `<exe>/../../state`
/// (the Cargo layout: `target/release` → project root).
pub fn resolve_state_dir(cli: Option<&str>) -> PathBuf {
    if let Some(p) = cli.filter(|s| !s.trim().is_empty()) {
        return PathBuf::from(p);
    }
    if let Ok(p) = std::env::var("CMF_STATE_DIR") {
        if !p.trim().is_empty() {
            return PathBuf::from(p);
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let beside = dir.join("state");
            if beside.is_dir() {
                return beside;
            }
            if let Some(root) = dir.parent().and_then(|p| p.parent()) {
                let in_project = root.join("state");
                if in_project.is_dir() || root.join("Cargo.toml").is_file() {
                    return in_project;
                }
            }
            return beside;
        }
    }
    PathBuf::from("state")
}

pub fn current_json_path(state_dir: &Path) -> PathBuf {
    state_dir.join("current.json")
}

/// Best-effort read of `current.json` (`None` when absent or unparsable).
pub fn read_current(state_dir: &Path) -> Option<CurrentContext> {
    let p = current_json_path(state_dir);
    let text = read_text(&p).ok()?;
    serde_json::from_str::<CurrentContext>(&text).ok()
}

/// Atomic write of `current.json` (tmp + rename) so a reader never sees a partial file.
pub fn write_current(state_dir: &Path, ctx: &CurrentContext) -> Result<(), String> {
    fs::create_dir_all(state_dir).map_err(|e| e.to_string())?;
    let final_path = current_json_path(state_dir);
    let tmp_path = state_dir.join("current.json.tmp");
    let text = serde_json::to_string_pretty(ctx).map_err(|e| e.to_string())?;
    fs::write(&tmp_path, text.as_bytes()).map_err(|e| e.to_string())?;
    // rename overwrites on Windows when the destination exists.
    if let Err(e) = fs::rename(&tmp_path, &final_path) {
        // Fall back to a direct write if rename is blocked (e.g. file locked).
        fs::write(&final_path, text.as_bytes()).map_err(|e2| format!("{e}; {e2}"))?;
        let _ = fs::remove_file(&tmp_path);
    }
    Ok(())
}

pub fn wrong_ids_path(state_dir: &Path, domain: Domain, subject_id: &str) -> PathBuf {
    state_dir.join(format!("wrong-{}-{}.json", domain.id(), subject_id))
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct WrongSet {
    #[serde(default)]
    pub ids: Vec<String>,
}

pub fn load_wrong_ids(state_dir: &Path, domain: Domain, subject_id: &str) -> Vec<String> {
    let p = wrong_ids_path(state_dir, domain, subject_id);
    read_text(&p)
        .ok()
        .and_then(|t| serde_json::from_str::<WrongSet>(&t).ok())
        .map(|w| w.ids)
        .unwrap_or_default()
}

pub fn save_wrong_ids(
    state_dir: &Path,
    domain: Domain,
    subject_id: &str,
    ids: &[String],
) -> Result<(), String> {
    fs::create_dir_all(state_dir).map_err(|e| e.to_string())?;
    let p = wrong_ids_path(state_dir, domain, subject_id);
    let mut sorted = ids.to_vec();
    sorted.sort();
    sorted.dedup();
    let text = serde_json::to_string_pretty(&WrongSet { ids: sorted }).map_err(|e| e.to_string())?;
    fs::write(&p, text.as_bytes()).map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// Local user store (favourites + notes)
// ---------------------------------------------------------------------------

/// One saved note for a question.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct NoteEntry {
    #[serde(default)]
    pub text: String,
    /// ISO-8601 UTC timestamp of the last edit.
    #[serde(default)]
    pub updated: String,
}

/// The whole local user store. One global file, because question ids already
/// carry a domain prefix (e.g. `law:...`, `cpa:...`).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct UserStore {
    /// Favourited question ids.
    #[serde(default)]
    pub favorites: Vec<String>,
    #[serde(default)]
    pub notes: std::collections::BTreeMap<String, NoteEntry>,
    /// Per-question practice record, keyed by question id.
    #[serde(default)]
    pub stats: std::collections::BTreeMap<String, QStat>,
    /// Domain the user last studied (`law` / `cpa`) — the startup domain, and what
    /// the header's mode button writes when it toggles.
    #[serde(default)]
    pub last_mode: Option<String>,
    /// UI theme (`dark` / `light`). Absent or unknown values mean `dark`.
    #[serde(default = "default_theme")]
    pub theme: String,
    /// Frosted-glass see-through, `0..=100` percent. Drives the scrim alpha:
    /// `255 * (1 - value/100)` (0 opaque, 100 fully clear). Files written
    /// before this field existed fall back to [`default_glass_transparency`].
    #[serde(default = "default_glass_transparency")]
    pub glass_transparency: u8,
    /// Frosted-glass blur strength, `0..=100`. `0` keeps the captured desktop
    /// sharp; larger values blur it more (see `glass::blur::params`).
    #[serde(default = "default_glass_blur")]
    pub glass_blur: u8,
}

fn default_theme() -> String {
    "dark".to_string()
}

/// Frosted-glass defaults: 20 % see-through (`255 * (1 - 0.20) = 204` scrim
/// alpha) and the maximum blur. Chosen by the user, so both are the values a
/// fresh install — and any `user.json` written before these fields existed —
/// starts from.
fn default_glass_transparency() -> u8 {
    20
}

fn default_glass_blur() -> u8 {
    100
}

impl Default for UserStore {
    fn default() -> Self {
        Self {
            favorites: Vec::new(),
            notes: std::collections::BTreeMap::new(),
            stats: std::collections::BTreeMap::new(),
            last_mode: None,
            theme: default_theme(),
            glass_transparency: default_glass_transparency(),
            glass_blur: default_glass_blur(),
        }
    }
}

/// How often a question has been seen, answered, and answered correctly.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct QStat {
    #[serde(default)]
    pub attempts: u32,
    #[serde(default)]
    pub correct: u32,
    /// How often the question has become the current question of a practice
    /// session (see [`UserStore::record_seen`]). `#[serde(default)]` so a
    /// `user.json` written before this counter existed still loads — its
    /// entries then read `seen == 0`.
    #[serde(default)]
    pub seen: u32,
    /// ISO-8601 UTC timestamp of the last recorded interaction (an answer or a
    /// view; see [`UserStore::record_answer`] / [`UserStore::record_seen`]).
    #[serde(default)]
    pub last: String,
    /// Consecutive correct answers, reset by a miss. [`MASTER_STREAK`] in a row
    /// graduates the question out of the weak set (see [`mastery`]).
    #[serde(default)]
    pub streak: u32,
    /// Unix day (UTC days since 1970-01-01) this question is next due for
    /// review; `0` when it has never been scheduled. Kept as a day number
    /// because a day is all the schedule needs: one comparison against today.
    #[serde(default)]
    pub due_day: i64,
}

/// Consecutive correct answers that graduate a question out of the weak set.
pub const MASTER_STREAK: u32 = 2;

/// How well a question is known, derived from its stats.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mastery {
    /// Never answered (`attempts == 0`), including questions only viewed.
    New,
    /// Answered at least once, and not yet [`MASTER_STREAK`] correct in a row.
    Weak,
    /// [`MASTER_STREAK`] correct in a row. It leaves the weak set and comes
    /// back as due once its interval elapses, so "已掌握" is a state, not a
    /// one-way door.
    Mastered,
}

/// [`Mastery`] of one question from its saved record.
pub fn mastery(stat: Option<&QStat>) -> Mastery {
    match stat {
        Some(s) if s.attempts > 0 && s.streak >= MASTER_STREAK => Mastery::Mastered,
        Some(s) if s.attempts > 0 => Mastery::Weak,
        _ => Mastery::New,
    }
}

/// Days until a question is due again, by consecutive-correct streak: a miss
/// comes back tomorrow, then 3, 7, 14 and 30 days. Deliberately short and
/// explainable rather than an SM-2 score — this is a study app, and the learner
/// should be able to predict when a question returns.
pub fn interval_days(streak: u32) -> i64 {
    match streak {
        0 => 1,
        1 => 3,
        2 => 7,
        3 => 14,
        _ => 30,
    }
}

/// Unix day (UTC) of a timestamp in seconds.
pub fn unix_day(secs: u64) -> i64 {
    (secs / 86_400) as i64
}

/// Today as a unix day (UTC).
pub fn today_unix_day() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| unix_day(d.as_secs()))
        .unwrap_or(0)
}

/// `YYYY-MM-DD` of a unix day. The 今天 block compares a stat's `last`
/// timestamp against this prefix, so "done today" survives a restart without
/// re-parsing every timestamp.
pub fn date_from_unix_day(day: i64) -> String {
    let (y, m, d) = civil_from_days(day);
    format!("{y:04}-{m:02}-{d:02}")
}

/// Chinese weekday of a unix day, Monday first. 1970-01-01 was a Thursday.
pub fn weekday_label(day: i64) -> &'static str {
    const NAMES: [&str; 7] = ["周一", "周二", "周三", "周四", "周五", "周六", "周日"];
    NAMES[(day + 3).rem_euclid(7) as usize]
}

/// Is this question id in `domain`? Ids carry the domain as their prefix
/// (`law:…` / `cpa:…`), which is what lets `user.json` be one file.
pub fn id_in_domain(id: &str, domain: Domain) -> bool {
    id.strip_prefix(domain.id())
        .is_some_and(|rest| rest.starts_with(':'))
}

/// The counts behind the 今天 block. Derived from `user.json` alone — no
/// subject file is opened for it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TodayCounts {
    /// Answered questions of this domain whose review day has arrived.
    pub due: usize,
    /// Domain questions never answered (the summary total minus attempted).
    pub new: usize,
    /// Domain favourites that are not [`Mastery::Mastered`].
    pub favorites: usize,
    /// Domain stats touched today (an answer or a view; see `QStat::last`).
    pub done: usize,
}

/// [`TodayCounts`] for one domain.
///
/// `total_questions` is the domain total from `summary.json`
/// ([`subject_question_total`]); `today_day` / `today_date` are
/// [`today_unix_day`] and [`date_from_unix_day`] of the same moment.
pub fn today_counts(
    stats: &std::collections::BTreeMap<String, QStat>,
    favorites: &[String],
    domain: Domain,
    total_questions: usize,
    today_day: i64,
    today_date: &str,
) -> TodayCounts {
    let mut attempted = 0;
    let mut due = 0;
    let mut done = 0;
    for (id, stat) in stats {
        if !id_in_domain(id, domain) {
            continue;
        }
        if stat.attempts > 0 {
            attempted += 1;
        }
        if is_due(Some(stat), today_day) {
            due += 1;
        }
        if stat.last.starts_with(today_date) {
            done += 1;
        }
    }
    let favorites = favorites
        .iter()
        .filter(|id| id_in_domain(id, domain))
        .filter(|id| mastery(stats.get(id.as_str())) != Mastery::Mastered)
        .count();
    TodayCounts {
        due,
        new: total_questions.saturating_sub(attempted),
        favorites,
        done,
    }
}

/// Is this question due for review today?
///
/// New questions are not "due" — they are new, and the today queue counts them
/// separately. A question that was answered and never scheduled (a `user.json`
/// written before `due_day` existed) is due immediately, which is what makes the
/// migration land the existing 错题 set in the queue instead of hiding it.
pub fn is_due(stat: Option<&QStat>, today: i64) -> bool {
    match stat {
        Some(s) if s.attempts > 0 => s.due_day <= today,
        _ => false,
    }
}

impl QStat {
    /// `0.0..=1.0`; `0.0` when never attempted.
    pub fn accuracy(&self) -> f32 {
        if self.attempts == 0 {
            0.0
        } else {
            self.correct as f32 / self.attempts as f32
        }
    }
}

impl UserStore {
    pub fn is_favorite(&self, question_id: &str) -> bool {
        self.favorites.iter().any(|id| id == question_id)
    }

    /// Adds or removes; returns the new state (`true` == now favourited).
    pub fn toggle_favorite(&mut self, question_id: &str) -> bool {
        if self.favorites.iter().any(|id| id == question_id) {
            self.favorites.retain(|id| id != question_id);
            false
        } else {
            self.favorites.push(question_id.to_string());
            true
        }
    }

    pub fn favorite_count(&self) -> usize {
        self.favorites.len()
    }

    /// Records one answer: bumps attempts/accuracy, the consecutive-correct
    /// streak, and schedules the next review (see [`interval_days`]).
    pub fn record_answer(&mut self, question_id: &str, correct: bool, now: u64) {
        let entry = self.stats.entry(question_id.to_string()).or_default();
        entry.attempts = entry.attempts.saturating_add(1);
        if correct {
            entry.correct = entry.correct.saturating_add(1);
            entry.streak = entry.streak.saturating_add(1);
        } else {
            // A miss resets the streak, so the question returns to the weak
            // set and is scheduled for tomorrow rather than in 7 days.
            entry.streak = 0;
        }
        entry.last = iso8601_from_unix(now);
        entry.due_day = unix_day(now) + interval_days(entry.streak);
    }

    /// Records one view of a question: it became the current question of a
    /// practice session. Bumps `seen` and the interaction timestamp only —
    /// `attempts` / `correct` / the review schedule are answer-only and are
    /// never touched here. Called once per visit (entering a session, 下一题,
    /// 上一题), not per frame, so re-showing the same question counts again.
    pub fn record_seen(&mut self, question_id: &str, now: u64) {
        let entry = self.stats.entry(question_id.to_string()).or_default();
        entry.seen = entry.seen.saturating_add(1);
        entry.last = iso8601_from_unix(now);
    }

    pub fn stat(&self, question_id: &str) -> Option<&QStat> {
        self.stats.get(question_id)
    }

    /// `None` when the question has never been attempted.
    pub fn accuracy(&self, question_id: &str) -> Option<f32> {
        self.stat(question_id).filter(|s| s.attempts > 0).map(QStat::accuracy)
    }

    /// Questions with at least one attempt.
    pub fn attempted_count(&self) -> usize {
        self.stats.values().filter(|s| s.attempts > 0).count()
    }

    pub fn total_attempts(&self) -> u64 {
        self.stats.values().map(|s| s.attempts as u64).sum()
    }

    /// Overall accuracy across every attempted question.
    pub fn overall_accuracy(&self) -> Option<f32> {
        let attempts = self.total_attempts();
        if attempts == 0 {
            return None;
        }
        let correct: u64 = self.stats.values().map(|s| s.correct as u64).sum();
        Some(correct as f32 / attempts as f32)
    }

    /// `None` when there is no note, or the note is blank.
    pub fn note(&self, question_id: &str) -> Option<&str> {
        self.notes
            .get(question_id)
            .map(|n| n.text.as_str())
            .filter(|t| !t.is_empty())
    }

    pub fn note_count(&self) -> usize {
        self.notes.values().filter(|n| !n.text.is_empty()).count()
    }

    /// Insert / update / remove. Blank (`trim().is_empty()`) text REMOVES the
    /// note. Stored text is the trimmed string.
    pub fn set_note(&mut self, question_id: &str, text: &str, timestamp: &str) {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            self.notes.remove(question_id);
        } else {
            self.notes.insert(
                question_id.to_string(),
                NoteEntry {
                    text: trimmed.to_string(),
                    updated: timestamp.to_string(),
                },
            );
        }
    }

    /// Drops every id that is not in `keep` from `favorites` (used for pruning).
    pub fn retain_favorites(&mut self, keep: &std::collections::HashSet<String>) {
        self.favorites.retain(|id| keep.contains(id));
    }
}

pub fn user_store_path(state_dir: &Path) -> PathBuf {
    state_dir.join("user.json")
}

/// Best-effort read: a missing or unparsable file yields `UserStore::default()`.
pub fn load_user_store(state_dir: &Path) -> UserStore {
    let p = user_store_path(state_dir);
    read_text(&p)
        .ok()
        .and_then(|t| serde_json::from_str::<UserStore>(&t).ok())
        .unwrap_or_default()
}

/// Atomic write (tmp + rename), same approach as the existing `write_current`.
pub fn save_user_store(state_dir: &Path, store: &UserStore) -> Result<(), String> {
    fs::create_dir_all(state_dir).map_err(|e| e.to_string())?;
    let final_path = user_store_path(state_dir);
    let tmp_path = state_dir.join("user.json.tmp");
    let mut out = store.clone();
    out.favorites.sort();
    out.favorites.dedup();
    let text = serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?;
    fs::write(&tmp_path, text.as_bytes()).map_err(|e| e.to_string())?;
    // rename overwrites on Windows when the destination exists.
    if let Err(e) = fs::rename(&tmp_path, &final_path) {
        // Fall back to a direct write if rename is blocked (e.g. file locked).
        fs::write(&final_path, text.as_bytes()).map_err(|e2| format!("{e}; {e2}"))?;
        let _ = fs::remove_file(&tmp_path);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// CLI args + time
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default)]
pub struct CliArgs {
    pub law_data: Option<String>,
    pub cpa_data: Option<String>,
    pub state_dir: Option<String>,
}

pub fn parse_cli_args() -> CliArgs {
    let mut a = CliArgs::default();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        let key = args[i].as_str();
        let next = args.get(i + 1).cloned();
        match key {
            "--law-data" | "--law_data" => {
                a.law_data = next.clone();
                i += 2;
            }
            "--cpa-data" | "--cpa_data" => {
                a.cpa_data = next.clone();
                i += 2;
            }
            "--state-dir" | "--state_dir" => {
                a.state_dir = next.clone();
                i += 2;
            }
            "--help" | "-h" => {
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }
    a
}

pub fn domain_root(domain: Domain, args: &CliArgs) -> PathBuf {
    let cli = match domain {
        Domain::Law => args.law_data.clone(),
        Domain::Cpa => args.cpa_data.clone(),
    };
    if let Some(p) = cli.filter(|s| !s.trim().is_empty()) {
        return PathBuf::from(p);
    }
    if let Ok(p) = std::env::var(domain.env_var()) {
        if !p.trim().is_empty() {
            return PathBuf::from(p);
        }
    }
    domain.default_root()
}

/// ISO-8601 UTC timestamp (e.g. `2026-10-15T03:49:09Z`).
pub fn now_iso() -> String {
    iso8601_from_unix(now_unix())
}

/// Seconds since the Unix epoch, for the review schedule.
pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn iso8601_from_unix(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (y, m, d) = civil_from_days(days);
    let hh = rem / 3600;
    let mm = (rem % 3600) / 60;
    let ss = rem % 60;
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The artefact this exists for: a source line break reflowed into a space
    /// inside Chinese prose (`data/law/subjects/xianfa.json` really holds
    /// `不是公 民获得…`). ASCII neighbours must be left alone -- those spaces
    /// are real or read correctly.
    #[test]
    fn join_cjk_spaces_drops_only_the_reflow_artefact() {
        assert_eq!(
            join_cjk_spaces("下列选项中哪一种情况不是公 民获得物质帮助权的条件?"),
            "下列选项中哪一种情况不是公民获得物质帮助权的条件?"
        );
        // A break can land after punctuation, and a run can be longer than one.
        assert_eq!(join_cjk_spaces("交付使用；未 经验收"), "交付使用；未经验收");
        assert_eq!(join_cjk_spaces("行人在人行道上站  立"), "行人在人行道上站立");
        // Kept: real separators around ASCII.
        assert_eq!(join_cjk_spaces("见 Word 文档第 3 条"), "见 Word 文档第 3 条");
        assert_eq!(join_cjk_spaces("2022 年客观题"), "2022 年客观题");
        assert_eq!(join_cjk_spaces("A 项正确"), "A 项正确");
        // An ideographic space (U+3000) is not the artefact.
        assert_eq!(join_cjk_spaces("公\u{3000}民"), "公\u{3000}民");
        // The rule is not Markdown-aware, which is exactly why `explanation` is
        // never run through it: a heading separator has the same shape.
        assert_eq!(join_cjk_spaces("### 第一章 法的本体"), "### 第一章法的本体");
    }

    #[test]
    fn join_cjk_spaces_leaves_markdown_structure_alone() {
        // Prose is joined; a heading and a table row keep their separators.
        assert_eq!(
            join_cjk_spaces_in_markdown(
                "解析】并非所有的法院判决都 能引起变动\n### 第一章 法的本体\n| 第一章 法的本体 | 甲 |"
            ),
            "解析】并非所有的法院判决都能引起变动\n### 第一章 法的本体\n| 第一章 法的本体 | 甲 |"
        );
    }

    #[test]
    fn iso_roundtrip_shape() {
        assert_eq!(iso8601_from_unix(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso8601_from_unix(1_700_000_000), "2023-11-14T22:13:20Z");
    }

    #[test]
    fn grading_is_set_based() {
        let q = Question {
            answer: vec!["B".into(), "A".into()],
            ..Default::default()
        };
        assert!(q.grade(&["a".into(), "B".into()]));
        assert!(!q.grade(&["A".into()]));
    }

    fn user_test_dir(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "sfzklearning-user-test-{}-{}",
            std::process::id(),
            name
        ))
    }

    #[test]
    fn user_store_round_trip() {
        let dir = user_test_dir("round-trip");
        let _ = fs::remove_dir_all(&dir);

        let mut store = UserStore::default();
        assert!(store.toggle_favorite("law:a"));
        assert!(store.toggle_favorite("cpa:b"));
        store.set_note("law:a", "hello", "2026-01-01T00:00:00Z");
        store.set_note("cpa:b", "world", "2026-01-02T00:00:00Z");
        store.glass_transparency = 37;
        store.glass_blur = 80;

        save_user_store(&dir, &store).unwrap();
        let loaded = load_user_store(&dir);

        assert_eq!(loaded.favorite_count(), 2);
        assert!(loaded.is_favorite("law:a"));
        assert!(loaded.is_favorite("cpa:b"));
        assert_eq!(loaded.note("law:a"), Some("hello"));
        assert_eq!(loaded.note("cpa:b"), Some("world"));
        assert_eq!(loaded.note("law:a").map(str::to_string), store.note("law:a").map(str::to_string));
        assert_eq!(loaded.glass_transparency, 37);
        assert_eq!(loaded.glass_blur, 80);

        let _ = fs::remove_dir_all(&dir);
    }

    /// The real compatibility risk: a `user.json` written before the glass
    /// fields existed must still load, with the two settings at their defaults.
    #[test]
    fn user_store_without_glass_fields_deserializes() {
        let dir = user_test_dir("legacy-glass");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            user_store_path(&dir),
            br#"{
                "favorites": ["law:a"],
                "notes": {"law:a": {"text": "hi", "updated": "t"}},
                "stats": {},
                "last_mode": "cpa",
                "theme": "light"
            }"#,
        )
        .unwrap();

        let loaded = load_user_store(&dir);
        assert!(loaded.is_favorite("law:a"), "the old fields still load");
        assert_eq!(loaded.note("law:a"), Some("hi"));
        assert_eq!(loaded.last_mode.as_deref(), Some("cpa"));
        assert_eq!(loaded.theme, "light");
        assert_eq!(
            loaded.glass_transparency,
            default_glass_transparency(),
            "a legacy file keeps the default transparency"
        );
        assert_eq!(loaded.glass_blur, default_glass_blur());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn user_store_toggle_add_then_remove() {
        let mut store = UserStore::default();
        assert!(store.toggle_favorite("law:x"));
        assert!(store.is_favorite("law:x"));
        assert!(!store.toggle_favorite("law:x"));
        assert!(!store.is_favorite("law:x"));
        assert_eq!(store.favorite_count(), 0);
    }

    #[test]
    fn user_store_no_duplicate_favorites() {
        let mut store = UserStore::default();
        assert!(store.toggle_favorite("law:dup"));
        assert!(!store.toggle_favorite("law:dup"));
        assert_eq!(store.favorite_count(), 0);

        let mut store2 = UserStore {
            favorites: vec!["law:dup".into(), "law:dup".into(), "cpa:x".into()],
            ..Default::default()
        };
        assert!(store2.is_favorite("law:dup"));
        assert!(!store2.toggle_favorite("law:dup"));
        assert!(!store2.is_favorite("law:dup"));
        assert_eq!(store2.favorite_count(), 1);
    }

    #[test]
    fn user_store_set_blank_note_removes() {
        let dir = user_test_dir("blank-note");
        let _ = fs::remove_dir_all(&dir);

        let mut store = UserStore::default();
        store.set_note("law:a", "content", "2026-01-01T00:00:00Z");
        assert_eq!(store.note("law:a"), Some("content"));

        store.set_note("law:a", "", "2026-01-01T00:01:00Z");
        assert_eq!(store.note("law:a"), None);
        assert_eq!(store.note_count(), 0);

        store.set_note("law:b", "   \n\t ", "2026-01-01T00:02:00Z");
        assert_eq!(store.note_count(), 0);

        save_user_store(&dir, &store).unwrap();
        let loaded = load_user_store(&dir);
        assert_eq!(loaded.note_count(), 0);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn user_store_note_count_ignores_blank() {
        let mut store = UserStore::default();
        store.notes.insert(
            "law:a".into(),
            NoteEntry {
                text: "kept".into(),
                updated: "t".into(),
            },
        );
        store.notes.insert(
            "law:b".into(),
            NoteEntry {
                text: String::new(),
                updated: "t".into(),
            },
        );
        assert_eq!(store.note_count(), 1);
        assert_eq!(store.note("law:b"), None);
    }

    #[test]
    fn user_store_missing_dir_is_default() {
        let dir = user_test_dir("missing");
        let _ = fs::remove_dir_all(&dir);
        let loaded = load_user_store(&dir);
        assert_eq!(loaded.favorite_count(), 0);
        assert_eq!(loaded.note_count(), 0);
    }

    #[test]
    fn user_store_corrupt_file_is_default() {
        let dir = user_test_dir("corrupt");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(user_store_path(&dir), b"{ not json").unwrap();

        let loaded = load_user_store(&dir);
        assert_eq!(loaded.favorite_count(), 0);
        assert_eq!(loaded.note_count(), 0);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn stats_accumulate_and_report_accuracy() {
        let mut store = UserStore::default();
        assert_eq!(store.attempted_count(), 0);
        assert!(store.accuracy("q1").is_none());
        assert!(store.overall_accuracy().is_none());

        store.record_answer("q1", true, 0);
        store.record_answer("q1", false, 86_400);
        store.record_answer("q1", true, 2 * 86_400);
        store.record_answer("q2", false, 0);

        let s = store.stat("q1").expect("q1 recorded");
        assert_eq!(s.attempts, 3);
        assert_eq!(s.correct, 2);
        assert_eq!(s.last, "1970-01-03T00:00:00Z");
        assert!((s.accuracy() - 2.0 / 3.0).abs() < 1e-5);
        // Right, miss, right: the miss reset the streak, so it is 1 -- not
        // mastered yet -- and the next review is `interval_days(1)` days out.
        assert_eq!(s.streak, 1);
        assert_eq!(s.due_day, 2 + interval_days(1));
        assert_eq!(mastery(Some(s)), Mastery::Weak);

        assert_eq!(store.attempted_count(), 2);
        assert_eq!(store.total_attempts(), 4);
        assert!((store.overall_accuracy().unwrap() - 0.5).abs() < 1e-5);
    }

    #[test]
    fn stats_survive_a_round_trip_and_are_additive() {
        let dir = user_test_dir("stats");
        let _ = fs::remove_dir_all(&dir);

        let mut store = UserStore::default();
        store.record_answer("law:a", true, 0);
        store.record_answer("law:a", true, 0);
        save_user_store(&dir, &store).unwrap();

        let loaded = load_user_store(&dir);
        assert_eq!(loaded.stat("law:a").unwrap().attempts, 2);
        assert!((loaded.accuracy("law:a").unwrap() - 1.0).abs() < 1e-5);

        let _ = fs::remove_dir_all(&dir);
    }

    /// The compatibility risk of the `seen` counter: a `stats` entry written
    /// before it existed has `attempts` / `correct` / `last` only, and must
    /// still load — with `seen == 0`.
    #[test]
    fn user_store_without_seen_field_deserializes() {
        let dir = user_test_dir("legacy-seen");
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            user_store_path(&dir),
            br#"{
                "favorites": [],
                "notes": {},
                "stats": {
                    "law:a": {"attempts": 4, "correct": 3, "last": "2026-01-01T00:00:00Z"}
                }
            }"#,
        )
        .unwrap();

        let loaded = load_user_store(&dir);
        let s = loaded.stat("law:a").expect("the legacy entry still loads");
        assert_eq!(s.attempts, 4);
        assert_eq!(s.correct, 3);
        assert_eq!(s.last, "2026-01-01T00:00:00Z");
        assert_eq!(s.seen, 0, "a file written before the counter has seen == 0");

        let _ = fs::remove_dir_all(&dir);
    }

    /// Viewing counts, answering counts, and the two never mix: two visits
    /// plus one answer leave `seen == 2` / `attempts == 1` / `correct == 1`.
    #[test]
    fn seen_increments_per_view_and_leaves_attempts_alone() {
        let mut store = UserStore::default();
        store.record_seen("q1", 0);
        assert_eq!(store.stat("q1").unwrap().seen, 1);
        assert_eq!(store.stat("q1").unwrap().attempts, 0, "a view is not an answer");
        assert_eq!(store.stat("q1").unwrap().correct, 0);

        store.record_answer("q1", true, 86_400);
        store.record_seen("q1", 3 * 86_400);
        let s = store.stat("q1").unwrap();
        assert_eq!(s.seen, 2, "re-showing the same question counts again");
        assert_eq!(s.attempts, 1);
        assert_eq!(s.correct, 1);
        assert_eq!(s.last, "1970-01-04T00:00:00Z", "the timestamp follows both kinds of record");
        // A view updates `last` and nothing else: the answer's schedule stands.
        assert_eq!(s.streak, 1);
        assert_eq!(s.due_day, 1 + interval_days(1));

        // A view must not invent an attempt either.
        store.record_seen("q2", 0);
        assert_eq!(store.attempted_count(), 1);
        assert_eq!(store.total_attempts(), 1);
    }

    /// The review model: new until answered, weak until two correct in a row,
    /// then mastered -- and a miss drops it back to weak. A `user.json` written
    /// before the schedule existed is weak and due immediately, which is what
    /// lands the existing 错题 set in today's queue.
    #[test]
    fn mastery_and_due_follow_the_streak() {
        let mut store = UserStore::default();
        assert_eq!(mastery(store.stat("q1")), Mastery::New);
        assert!(!is_due(store.stat("q1"), 0), "a question never answered is not due");

        store.record_answer("q1", true, 0);
        assert_eq!(mastery(store.stat("q1")), Mastery::Weak, "one correct is not mastery");
        assert_eq!(store.stat("q1").unwrap().streak, 1);
        assert_eq!(store.stat("q1").unwrap().due_day, interval_days(1));
        assert!(!is_due(store.stat("q1"), 1), "not due before its interval");
        assert!(is_due(store.stat("q1"), interval_days(1)), "due on the day");

        store.record_answer("q1", true, 2 * 86_400);
        assert_eq!(mastery(store.stat("q1")), Mastery::Mastered);
        assert_eq!(store.stat("q1").unwrap().streak, 2);

        store.record_answer("q1", false, 3 * 86_400);
        assert_eq!(mastery(store.stat("q1")), Mastery::Weak, "a miss drops it back");
        assert_eq!(store.stat("q1").unwrap().streak, 0);
        assert_eq!(store.stat("q1").unwrap().due_day, 3 + interval_days(0));

        let migrated = QStat {
            attempts: 1,
            correct: 0,
            ..Default::default()
        };
        assert_eq!(mastery(Some(&migrated)), Mastery::Weak);
        assert!(is_due(Some(&migrated), 20_000), "an unscheduled miss is due at once");
    }

    /// The 今天 block's date line: the day number of an ISO timestamp is the
    /// same day its `last` prefix names, and the weekday follows the calendar
    /// (1970-01-01 was a Thursday).
    #[test]
    fn date_and_weekday_come_from_the_unix_day() {
        assert_eq!(date_from_unix_day(0), "1970-01-01");
        assert_eq!(weekday_label(0), "周四");
        assert_eq!(weekday_label(5), "周二", "1970-01-06 was a Tuesday");

        // 1_700_000_000 s = 2023-11-14T22:13:20Z, a Tuesday.
        assert_eq!(date_from_unix_day(19_675), "2023-11-14");
        assert_eq!(weekday_label(19_675), "周二");
        assert!(iso8601_from_unix(1_700_000_000).starts_with(&date_from_unix_day(19_675)));
    }

    /// The 今天 numbers are domain-scoped and come from `user.json` alone:
    /// 到期 only counts answered questions whose day arrived, 新题 is the
    /// summary total minus the domain's attempted entries, 收藏 skips mastered
    /// ones, and 今日 counts every entry touched today (a view included).
    #[test]
    fn today_counts_are_derived_from_stats_and_favorites_only() {
        let today = 20_000;
        let date = date_from_unix_day(today);
        let mut stats = std::collections::BTreeMap::new();
        stats.insert(
            "law:due-weak".to_string(),
            QStat {
                attempts: 2,
                correct: 1,
                streak: 0,
                due_day: today - 1,
                last: format!("{date}T08:00:00Z"),
                ..Default::default()
            },
        );
        stats.insert(
            "law:due-mastered".to_string(),
            QStat {
                attempts: 2,
                correct: 2,
                streak: 2,
                due_day: today,
                last: "2020-01-01T00:00:00Z".to_string(),
                ..Default::default()
            },
        );
        stats.insert(
            "law:future".to_string(),
            QStat {
                attempts: 1,
                correct: 1,
                streak: 1,
                due_day: today + 3,
                last: String::new(),
                ..Default::default()
            },
        );
        stats.insert(
            "law:viewed".to_string(),
            QStat {
                seen: 1,
                last: format!("{date}T09:00:00Z"),
                ..Default::default()
            },
        );
        stats.insert(
            "cpa:due".to_string(),
            QStat {
                attempts: 1,
                correct: 0,
                due_day: 0,
                last: format!("{date}T10:00:00Z"),
                ..Default::default()
            },
        );
        let favorites = vec![
            "law:due-mastered".to_string(), // mastered: still a favourite, not today's
            "law:future".to_string(),
            "law:new".to_string(),
            "cpa:due".to_string(), // other domain
        ];

        let law = today_counts(&stats, &favorites, Domain::Law, 100, today, &date);
        assert_eq!(law.due, 2, "the two answered law questions whose day arrived");
        assert_eq!(law.new, 97, "100 summary questions minus 3 attempted law entries");
        assert_eq!(law.favorites, 2, "mastered favourites are not 今天's favourites");
        assert_eq!(law.done, 2, "an answer and a view both touch `last` today");

        let cpa = today_counts(&stats, &favorites, Domain::Cpa, 7, today, &date);
        assert_eq!(cpa.due, 1, "only cpa entries count");
        assert_eq!(cpa.new, 6);
        assert_eq!(cpa.favorites, 1);
        assert_eq!(cpa.done, 1);

        // Domain ids are matched on the `id:` prefix, not a bare substring.
        assert!(id_in_domain("law:a", Domain::Law));
        assert!(!id_in_domain("lawful:a", Domain::Law));
        assert!(!id_in_domain("cpa:a", Domain::Law));

        let subs = [
            SummarySubject {
                questions: Some(10),
                ..Default::default()
            },
            SummarySubject {
                questions: Some(32),
                ..Default::default()
            },
            SummarySubject::default(),
        ];
        assert_eq!(subject_question_total(&subs), 42);
    }
}
