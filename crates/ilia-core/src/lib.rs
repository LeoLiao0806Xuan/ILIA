use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DocumentSummary {
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
    pub chunk_id: String,
    pub citation_label: String,
    pub selection_reason: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AnswerCitation {
    pub evidence_number: usize,
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
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            limit: 10,
            candidate_limit: 30,
            evidence_limit: 5,
            evidence_char_budget: 12_000,
        }
    }
}
