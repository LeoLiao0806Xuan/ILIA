use serde::{Deserialize, Serialize};
use std::{fmt, str::FromStr};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ResearchMode {
    Quick,
    #[default]
    Standard,
    Deep,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum AnswerLanguage {
    Chinese,
    English,
    #[default]
    FollowQuestion,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum LibraryKind {
    Core,
    User,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LibraryScope {
    #[default]
    All,
    CoreOnly,
    UserOnly,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum DocumentType {
    Treaty,
    Judgment,
    AdvisoryOpinion,
    Order,
    Resolution,
    DraftArticles,
    CustomaryRule,
    Commentary,
    Declaration,
    Statute,
    UserDocument,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct SearchFilters {
    pub library: LibraryScope,
    #[serde(default)]
    pub document_types: Vec<DocumentType>,
    #[serde(default)]
    pub topic_ids: Vec<String>,
    #[serde(default)]
    pub document_keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SearchRequest {
    pub query: String,
    #[serde(default)]
    pub filters: SearchFilters,
    pub limit: usize,
    pub evidence_limit: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(try_from = "String", into = "String")]
pub struct StableEvidenceKey {
    pub library: LibraryKind,
    pub document_id: String,
    pub chunk_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StableEvidenceKeyError;

impl StableEvidenceKey {
    pub fn new(
        library: LibraryKind,
        document_id: impl Into<String>,
        chunk_id: impl Into<String>,
    ) -> Result<Self, StableEvidenceKeyError> {
        let document_id = document_id.into();
        let chunk_id = chunk_id.into();
        if document_id.is_empty()
            || chunk_id.is_empty()
            || document_id.contains(':')
            || chunk_id.contains(':')
        {
            return Err(StableEvidenceKeyError);
        }
        Ok(Self {
            library,
            document_id,
            chunk_id,
        })
    }
}

impl fmt::Display for StableEvidenceKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let library = match self.library {
            LibraryKind::Core => "core",
            LibraryKind::User => "user",
        };
        write!(
            formatter,
            "{library}:{}:{}",
            self.document_id, self.chunk_id
        )
    }
}

impl fmt::Display for StableEvidenceKeyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .write_str("stable evidence key must contain a library and non-empty colon-free IDs")
    }
}

impl std::error::Error for StableEvidenceKeyError {}

impl FromStr for StableEvidenceKey {
    type Err = StableEvidenceKeyError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let mut parts = value.split(':');
        let library = match parts.next() {
            Some("core") => LibraryKind::Core,
            Some("user") => LibraryKind::User,
            _ => return Err(StableEvidenceKeyError),
        };
        let document_id = parts.next().ok_or(StableEvidenceKeyError)?;
        let chunk_id = parts.next().ok_or(StableEvidenceKeyError)?;
        if parts.next().is_some() {
            return Err(StableEvidenceKeyError);
        }
        Self::new(library, document_id, chunk_id)
    }
}

impl From<StableEvidenceKey> for String {
    fn from(value: StableEvidenceKey) -> Self {
        value.to_string()
    }
}

impl TryFrom<String> for StableEvidenceKey {
    type Error = StableEvidenceKeyError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        value.parse()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResearchRequest {
    pub request_id: Uuid,
    pub question: String,
    #[serde(default)]
    pub mode: ResearchMode,
    #[serde(default)]
    pub answer_language: AnswerLanguage,
    #[serde(default)]
    pub filters: SearchFilters,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CitationSupport {
    Direct,
    Summary,
    Unsupported,
    Conflict,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CitationFinding {
    pub statement: String,
    pub evidence_numbers: Vec<usize>,
    pub support: CitationSupport,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResearchErrorCode {
    InvalidRequest,
    RetrievalFailed,
    ModelUnavailable,
    GenerationFailed,
    CitationAuditFailed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum ResearchEvent {
    Started,
    RetrievalCompleted {
        evidence_count: usize,
    },
    PlanReady {
        subquestions: Vec<String>,
    },
    AnswerDelta {
        text: String,
    },
    AnswerReplaced {
        text: String,
    },
    CitationAuditCompleted {
        findings: Vec<CitationFinding>,
    },
    Completed,
    Cancelled,
    Failed {
        code: ResearchErrorCode,
        safe_message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DocumentSummary {
    pub library_kind: LibraryKind,
    pub stable_document_key: String,
    pub document_id: String,
    pub canonical_title: String,
    pub title_zh: Option<String>,
    pub short_title: Option<String>,
    pub document_type: String,
    pub legal_status: String,
    pub official_source_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SearchHit {
    pub library_kind: LibraryKind,
    pub stable_key: StableEvidenceKey,
    pub chunk_id: String,
    pub document_id: String,
    pub canonical_title: String,
    pub title_zh: Option<String>,
    pub document_type: String,
    pub legal_status: String,
    pub official_source_url: String,
    pub citation_label: String,
    pub page_start: i64,
    pub page_end: i64,
    pub language: String,
    pub text: String,
    pub match_kind: MatchKind,
    pub score: f64,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CitationStyle {
    Chinese,
    Oscola,
    Bluebook,
    Icj,
    Markdown,
    PlainText,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CitationMetadata {
    pub title: String,
    pub locator: String,
    pub url: Option<String>,
    pub year: Option<i32>,
    pub court: Option<String>,
    pub report: Option<String>,
}

pub fn format_legal_citation(value: &CitationMetadata, style: CitationStyle) -> String {
    let year = value
        .year
        .map(|year| format!(" ({year})"))
        .unwrap_or_default();
    let court = value
        .court
        .as_ref()
        .map(|court| format!(", {court}"))
        .unwrap_or_default();
    let report = value
        .report
        .as_ref()
        .map(|report| format!(", {report}"))
        .unwrap_or_default();
    let base = match style {
        CitationStyle::Chinese => format!("《{}》{}{}{}", value.title, year, value.locator, court),
        CitationStyle::Oscola => format!("{}{} {},{}", value.title, year, value.locator, report)
            .trim_end_matches(',')
            .to_owned(),
        CitationStyle::Bluebook => format!("{}, {}{}{}", value.title, value.locator, report, year),
        CitationStyle::Icj => format!("{}{}{}，{}", value.title, year, report, value.locator),
        CitationStyle::Markdown => format!("*{}*{}，{}", value.title, year, value.locator),
        CitationStyle::PlainText => format!("{}{} — {}", value.title, year, value.locator),
    };
    match (&value.url, style) {
        (Some(url), CitationStyle::Markdown) => format!("[{base}]({url})"),
        (Some(url), CitationStyle::PlainText | CitationStyle::Chinese) => format!("{base}，{url}"),
        _ => base,
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MatchKind {
    ExactArticle,
    ExactCaseParagraph,
    FullText,
    Vector,
    Hybrid,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvidenceItem {
    pub rank: usize,
    pub stable_key: StableEvidenceKey,
    pub chunk_id: String,
    #[serde(default)]
    pub related_chunk_ids: Vec<String>,
    pub citation_label: String,
    pub selection_reason: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnswerCitation {
    pub evidence_number: usize,
    pub stable_key: StableEvidenceKey,
    pub chunk_id: String,
    pub citation_label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnswerResponse {
    pub question: String,
    pub answer: String,
    pub grounded: bool,
    pub citations: Vec<AnswerCitation>,
    pub evidence: Vec<EvidenceItem>,
    pub model_id: String,
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub generation_ms: u128,
    pub warnings: Vec<String>,
    #[serde(default)]
    pub citation_findings: Vec<CitationFinding>,
    #[serde(default)]
    pub citation_rewritten: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResponse {
    pub query: String,
    pub normalized_query: String,
    pub detected_document_id: Option<String>,
    pub detected_locator: Option<String>,
    pub documents: Vec<DocumentSummary>,
    pub hits: Vec<SearchHit>,
    pub evidence: Vec<EvidenceItem>,
}

#[derive(Debug, Clone, Copy)]
pub struct SearchOptions {
    pub limit: usize,
    pub candidate_limit: usize,
    pub evidence_limit: usize,
    pub evidence_char_budget: usize,
    pub per_document_limit: usize,
    pub merge_adjacent: bool,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            limit: 10,
            candidate_limit: 30,
            evidence_limit: 5,
            evidence_char_budget: 12_000,
            per_document_limit: 3,
            merge_adjacent: true,
        }
    }
}

#[cfg(test)]
mod contract_tests {
    use super::*;

    #[test]
    fn stable_evidence_key_round_trips() {
        let key = StableEvidenceKey::new(
            LibraryKind::Core,
            "icj-nicaragua-1986-merits",
            "icj-nicaragua-1986-merits-para-191-en",
        )
        .unwrap();
        assert_eq!(key.to_string().parse::<StableEvidenceKey>().unwrap(), key);
        assert_eq!(
            serde_json::to_value(&key).unwrap(),
            "core:icj-nicaragua-1986-merits:icj-nicaragua-1986-merits-para-191-en"
        );
        assert!(
            "core:document:chunk:extra"
                .parse::<StableEvidenceKey>()
                .is_err()
        );
    }

    #[test]
    fn research_contract_has_stable_json_names() {
        let request = ResearchRequest {
            request_id: Uuid::nil(),
            question: "Article 3".to_owned(),
            mode: ResearchMode::Quick,
            answer_language: AnswerLanguage::English,
            filters: SearchFilters {
                library: LibraryScope::CoreOnly,
                document_types: vec![DocumentType::Treaty],
                topic_ids: Vec::new(),
                document_keys: vec!["unclos-1982".to_owned()],
            },
        };
        let json = serde_json::to_value(request).unwrap();
        assert_eq!(json["mode"], "quick");
        assert_eq!(json["answer_language"], "english");
        assert_eq!(json["filters"]["library"], "core_only");
        assert_eq!(json["filters"]["document_types"][0], "treaty");

        let event = ResearchEvent::Failed {
            code: ResearchErrorCode::ModelUnavailable,
            safe_message: "local model unavailable".to_owned(),
        };
        let event_json = serde_json::to_value(event).unwrap();
        assert_eq!(event_json["type"], "failed");
        assert_eq!(event_json["data"]["code"], "model_unavailable");

        let replacement = serde_json::to_value(ResearchEvent::AnswerReplaced {
            text: "safe answer".to_owned(),
        })
        .unwrap();
        assert_eq!(replacement["type"], "answer_replaced");
        assert_eq!(replacement["data"]["text"], "safe answer");
    }

    #[test]
    fn citation_styles_never_invent_missing_metadata() {
        let value = CitationMetadata {
            title: "United Nations Convention on the Law of the Sea".into(),
            locator: "art 3".into(),
            url: Some("https://example.invalid/unclos".into()),
            year: None,
            court: None,
            report: None,
        };
        let outputs = [
            CitationStyle::Chinese,
            CitationStyle::Oscola,
            CitationStyle::Bluebook,
            CitationStyle::Icj,
            CitationStyle::Markdown,
            CitationStyle::PlainText,
        ]
        .map(|style| format_legal_citation(&value, style));
        assert!(
            outputs
                .iter()
                .all(|output| output.contains("United Nations Convention"))
        );
        assert!(outputs.iter().all(|output| output.contains("art 3")));
        assert!(outputs.iter().all(|output| !output.contains("1982")));
        assert!(outputs[4].contains("https://example.invalid/unclos"));
    }
}
