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
                        .map(|x| value_to_string(x))
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| default_label(i));
                    let text = map
                        .get("text")
                        .or_else(|| map.get("content"))
                        .map(|x| value_to_string(x))
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
    /// `true` when this item is a real 选择题 (has options), otherwise 记忆/自测.
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
                return "记忆/自测".to_string();
            }
            return format!("记忆/自测（{}）", raw);
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

pub fn load_subject(root: &Path, subject_id: &str) -> Result<SubjectFile, String> {
    let p = subject_file_path(root, subject_id);
    let text = read_text(&p).map_err(|e| format!("读取 {} 失败: {e}", p.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("解析 {} 失败: {e}", p.display()))
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

/// ≈ today's scrim: `255 * (1 - 0.12) ≈ 224`.
fn default_glass_transparency() -> u8 {
    12
}

fn default_glass_blur() -> u8 {
    45
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

/// How often a question has been answered, and how often correctly.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct QStat {
    #[serde(default)]
    pub attempts: u32,
    #[serde(default)]
    pub correct: u32,
    /// ISO-8601 UTC timestamp of the last attempt.
    #[serde(default)]
    pub last: String,
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

    /// Records one answer, updating the per-question attempts/accuracy record.
    pub fn record_answer(&mut self, question_id: &str, correct: bool, timestamp: &str) {
        let entry = self.stats.entry(question_id.to_string()).or_default();
        entry.attempts = entry.attempts.saturating_add(1);
        if correct {
            entry.correct = entry.correct.saturating_add(1);
        }
        entry.last = timestamp.to_string();
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
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    iso8601_from_unix(secs)
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

        store.record_answer("q1", true, "t1");
        store.record_answer("q1", false, "t2");
        store.record_answer("q1", true, "t3");
        store.record_answer("q2", false, "t4");

        let s = store.stat("q1").expect("q1 recorded");
        assert_eq!(s.attempts, 3);
        assert_eq!(s.correct, 2);
        assert_eq!(s.last, "t3");
        assert!((s.accuracy() - 2.0 / 3.0).abs() < 1e-5);

        assert_eq!(store.attempted_count(), 2);
        assert_eq!(store.total_attempts(), 4);
        assert!((store.overall_accuracy().unwrap() - 0.5).abs() < 1e-5);
    }

    #[test]
    fn stats_survive_a_round_trip_and_are_additive() {
        let dir = user_test_dir("stats");
        let _ = fs::remove_dir_all(&dir);

        let mut store = UserStore::default();
        store.record_answer("law:a", true, "t1");
        store.record_answer("law:a", true, "t2");
        save_user_store(&dir, &store).unwrap();

        let loaded = load_user_store(&dir);
        assert_eq!(loaded.stat("law:a").unwrap().attempts, 2);
        assert!((loaded.accuracy("law:a").unwrap() - 1.0).abs() < 1e-5);

        let _ = fs::remove_dir_all(&dir);
    }
}
