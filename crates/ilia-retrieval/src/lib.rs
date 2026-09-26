use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
};

use ilia_core::{
    DocumentType, EvidenceItem, LibraryKind, LibraryScope, MatchKind, SearchFilters, SearchHit,
    SearchOptions, SearchRequest, SearchResponse,
};
use ilia_database::{Database, DatabaseError, DatabaseQueryFilter, UserDatabase};
use regex::Regex;
use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RetrievalError {
    #[error(transparent)]
    Database(#[from] DatabaseError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("query must not be empty")]
    EmptyQuery,
    #[error("unknown topic filter: {0}")]
    UnknownTopic(String),
    #[error("invalid related-document metadata: {0}")]
    InvalidRelations(String),
}

#[derive(Debug, Clone, Default)]
pub struct RelatedDocumentRegistry {
    related: HashMap<String, Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct LightweightReranker {
    lexical_weight: f64,
    original_weight: f64,
}
#[derive(Deserialize)]
struct RerankerWeights {
    format_version: u32,
    lexical_weight: f64,
    original_weight: f64,
}

impl LightweightReranker {
    pub fn from_optional_path(path: impl AsRef<Path>) -> Result<Option<Self>, RetrievalError> {
        let path = path.as_ref();
        if !path.is_file() {
            return Ok(None);
        }
        let weights: RerankerWeights = serde_json::from_slice(&fs::read(path)?)?;
        if weights.format_version != 1
            || weights.lexical_weight < 0.0
            || weights.original_weight < 0.0
        {
            return Err(RetrievalError::InvalidRelations(
                "invalid reranker weights".into(),
            ));
        }
        Ok(Some(Self {
            lexical_weight: weights.lexical_weight,
            original_weight: weights.original_weight,
        }))
    }
    pub fn rerank(&self, query: &str, hits: &mut [SearchHit]) {
        let terms = query
            .to_lowercase()
            .split(|c: char| !c.is_alphanumeric())
            .filter(|v| v.chars().count() >= 2)
            .map(str::to_owned)
            .collect::<HashSet<_>>();
        for hit in hits.iter_mut() {
            let text = format!(
                "{} {} {}",
                hit.canonical_title, hit.citation_label, hit.text
            )
            .to_lowercase();
            let overlap = terms
                .iter()
                .filter(|term| text.contains(term.as_str()))
                .count() as f64
                / terms.len().max(1) as f64;
            hit.score = hit.score * self.original_weight + overlap * self.lexical_weight;
        }
        hits.sort_by(|a, b| b.score.total_cmp(&a.score));
    }
}

#[derive(Deserialize)]
struct RelatedFile {
    format_version: u32,
    relations: Vec<RelatedPair>,
}
#[derive(Deserialize)]
struct RelatedPair {
    left: String,
    right: String,
}

impl RelatedDocumentRegistry {
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, RetrievalError> {
        let file: RelatedFile = serde_json::from_slice(&fs::read(path)?)?;
        if file.format_version != 1 {
            return Err(RetrievalError::InvalidRelations(
                "unsupported format version".into(),
            ));
        }
        let mut related: HashMap<String, Vec<String>> = HashMap::new();
        let mut pairs = HashSet::new();
        for pair in file.relations {
            if pair.left == pair.right
                || pair.left.trim().is_empty()
                || pair.right.trim().is_empty()
            {
                return Err(RetrievalError::InvalidRelations(
                    "self or empty relation".into(),
                ));
            }
            let key = if pair.left < pair.right {
                format!("{}\u{1f}{}", pair.left, pair.right)
            } else {
                format!("{}\u{1f}{}", pair.right, pair.left)
            };
            if !pairs.insert(key) {
                return Err(RetrievalError::InvalidRelations(
                    "duplicate relation".into(),
                ));
            }
            related
                .entry(pair.left.clone())
                .or_default()
                .push(pair.right.clone());
            related.entry(pair.right).or_default().push(pair.left);
        }
        for values in related.values_mut() {
            values.sort();
        }
        Ok(Self { related })
    }
    pub fn related_to(&self, document_id: &str) -> &[String] {
        self.related
            .get(document_id)
            .map(Vec::as_slice)
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, Default)]
pub struct TopicRegistry {
    topics: HashMap<String, HashSet<String>>,
}

#[derive(Debug, Deserialize)]
struct TopicFile {
    topics: HashMap<String, Vec<String>>,
}

impl TopicRegistry {
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, RetrievalError> {
        let file: TopicFile = serde_json::from_slice(&fs::read(path)?)?;
        Ok(Self {
            topics: file
                .topics
                .into_iter()
                .map(|(topic, documents)| (topic, documents.into_iter().collect()))
                .collect(),
        })
    }

    fn documents_for(&self, topic_ids: &[String]) -> Result<HashSet<String>, RetrievalError> {
        let mut documents = HashSet::new();
        for topic_id in topic_ids {
            let topic = self
                .topics
                .get(topic_id)
                .ok_or_else(|| RetrievalError::UnknownTopic(topic_id.clone()))?;
            documents.extend(topic.iter().cloned());
        }
        Ok(documents)
    }
}

pub struct RetrievalService {
    core: Database,
    user: Option<UserDatabase>,
    topics: TopicRegistry,
    article_en: Regex,
    article_zh: Regex,
    paragraph_en: Regex,
    paragraph_zh: Regex,
    terms: Regex,
}

impl RetrievalService {
    pub fn new(core: Database) -> Self {
        Self {
            core,
            user: None,
            topics: TopicRegistry::default(),
            article_en: Regex::new(r"(?i)\b(?:article|art\.?)\s*([0-9]{1,3}|[ivxlcdm]{1,8})\b")
                .unwrap(),
            article_zh: Regex::new(r"第\s*(\d{1,3})\s*条").unwrap(),
            paragraph_en: Regex::new(r"(?i)\b(?:paragraph|para\.?)\s*(\d{1,4})\b").unwrap(),
            paragraph_zh: Regex::new(r"第\s*(\d{1,4})\s*段").unwrap(),
            terms: Regex::new(r"[\p{L}\p{N}]{2,}").unwrap(),
        }
    }

    pub fn with_user_database(mut self, user: UserDatabase) -> Self {
        self.user = Some(user);
        self
    }

    pub fn with_topic_registry(mut self, topics: TopicRegistry) -> Self {
        self.topics = topics;
        self
    }

    pub fn search(
        &self,
        query: &str,
        options: SearchOptions,
    ) -> Result<SearchResponse, RetrievalError> {
        self.search_request(
            &SearchRequest {
                query: query.to_owned(),
                filters: SearchFilters::default(),
                limit: options.limit,
                evidence_limit: options.evidence_limit,
            },
            options,
        )
    }

    pub fn search_request(
        &self,
        request: &SearchRequest,
        mut options: SearchOptions,
    ) -> Result<SearchResponse, RetrievalError> {
        options.limit = request.limit;
        options.evidence_limit = request.evidence_limit;
        let query = request.query.trim();
        if query.is_empty() {
            return Err(RetrievalError::EmptyQuery);
        }
        let (core_filter, user_filter) = self.database_filters(&request.filters)?;
        let core_resolved = self.core.resolve_document_filtered(query, &core_filter)?;
        let user_resolved = self
            .user
            .as_ref()
            .map(|database| database.resolve_document_filtered(query, &user_filter))
            .transpose()?
            .flatten();
        let core_document_id = core_resolved
            .as_ref()
            .map(|value| value.document_id.as_str());

        if let Some(number) = capture_locator(&self.article_zh, query)
            .or_else(|| capture_locator(&self.article_en, query))
        {
            let hits = self.core.article_filtered(
                core_document_id,
                number,
                options.limit,
                &core_filter,
            )?;
            return Ok(response(
                query,
                core_document_id,
                Some(format!("article:{number}")),
                Vec::new(),
                hits,
                options,
            ));
        }

        if let Some(number) = capture_locator(&self.paragraph_zh, query)
            .or_else(|| capture_locator(&self.paragraph_en, query))
        {
            let hits = self.core.case_paragraph_filtered(
                core_document_id,
                number,
                options.limit,
                &core_filter,
            )?;
            return Ok(response(
                query,
                core_document_id,
                Some(format!("paragraph:{number}")),
                Vec::new(),
                hits,
                options,
            ));
        }

        let searchable = strip_aliases(query, core_resolved.as_ref(), user_resolved.as_ref());
        let fts_query = self.fts_query(&searchable, " AND ");
        if fts_query.is_empty() {
            let mut documents = Vec::new();
            if let Some(id) = core_document_id {
                documents.extend(self.core.document_filtered(id, &core_filter)?);
            }
            if let (Some(database), Some(resolved)) = (&self.user, &user_resolved) {
                documents.extend(database.document_filtered(&resolved.document_id, &user_filter)?);
            }
            return Ok(SearchResponse {
                query: query.to_owned(),
                normalized_query: normalize(query),
                detected_document_id: core_document_id.map(str::to_owned).or_else(|| {
                    user_resolved
                        .as_ref()
                        .map(|value| value.document_id.clone())
                }),
                detected_locator: None,
                documents,
                hits: Vec::new(),
                evidence: Vec::new(),
            });
        }

        let mut hits = self.core.full_text_filtered(
            &fts_query,
            core_document_id,
            options.candidate_limit,
            &core_filter,
        )?;
        if let Some(database) = &self.user {
            hits.extend(
                database.full_text_filtered(
                    &fts_query,
                    user_resolved
                        .as_ref()
                        .map(|value| value.document_id.as_str()),
                    options.candidate_limit,
                    &user_filter,
                )?,
            );
        }
        let resolved_document = core_document_id.or_else(|| {
            user_resolved
                .as_ref()
                .map(|value| value.document_id.as_str())
        });
        let hits = rank_and_limit(hits, query, resolved_document, options);
        Ok(response(
            query,
            core_document_id,
            None,
            Vec::new(),
            hits,
            options,
        ))
    }

    pub fn search_hybrid(
        &self,
        query: &str,
        query_vector: &[f32],
        model_id: &str,
        options: SearchOptions,
    ) -> Result<SearchResponse, RetrievalError> {
        self.search_hybrid_request(
            &SearchRequest {
                query: query.to_owned(),
                filters: SearchFilters::default(),
                limit: options.limit,
                evidence_limit: options.evidence_limit,
            },
            query_vector,
            model_id,
            options,
        )
    }

    pub fn search_hybrid_request(
        &self,
        request: &SearchRequest,
        query_vector: &[f32],
        model_id: &str,
        mut options: SearchOptions,
    ) -> Result<SearchResponse, RetrievalError> {
        options.limit = request.limit;
        options.evidence_limit = request.evidence_limit;
        let query = request.query.trim();
        if query.is_empty() {
            return Err(RetrievalError::EmptyQuery);
        }
        let base = self.search_request(request, options)?;
        if base.detected_locator.is_some() || !base.documents.is_empty() {
            return Ok(base);
        }
        let (core_filter, user_filter) = self.database_filters(&request.filters)?;
        let core_resolved = self.core.resolve_document_filtered(query, &core_filter)?;
        let user_resolved = self
            .user
            .as_ref()
            .map(|database| database.resolve_document_filtered(query, &user_filter))
            .transpose()?
            .flatten();
        let searchable = strip_aliases(query, core_resolved.as_ref(), user_resolved.as_ref());
        let strict = self.fts_query(&searchable, " AND ");
        let relaxed = self.fts_query(&searchable, " OR ");
        let core_document = core_resolved
            .as_ref()
            .map(|value| value.document_id.as_str());
        let user_document = user_resolved
            .as_ref()
            .map(|value| value.document_id.as_str());

        let mut lexical = if strict.is_empty() {
            Vec::new()
        } else {
            self.core.full_text_filtered(
                &strict,
                core_document,
                options.candidate_limit,
                &core_filter,
            )?
        };
        if lexical.is_empty() && strict != relaxed && !relaxed.is_empty() {
            lexical = self.core.full_text_filtered(
                &relaxed,
                core_document,
                options.candidate_limit,
                &core_filter,
            )?;
        }
        let mut vector = self.core.vector_search_filtered(
            query_vector,
            model_id,
            core_document,
            options.candidate_limit,
            &core_filter,
        )?;

        if let Some(database) = &self.user {
            let mut user_lexical = if strict.is_empty() {
                Vec::new()
            } else {
                database.full_text_filtered(
                    &strict,
                    user_document,
                    options.candidate_limit,
                    &user_filter,
                )?
            };
            if user_lexical.is_empty() && strict != relaxed && !relaxed.is_empty() {
                user_lexical = database.full_text_filtered(
                    &relaxed,
                    user_document,
                    options.candidate_limit,
                    &user_filter,
                )?;
            }
            lexical.extend(user_lexical);
            vector.extend(database.vector_search_filtered(
                query_vector,
                model_id,
                user_document,
                options.candidate_limit,
                &user_filter,
            )?);
        }

        let fused = reciprocal_rank_fusion(lexical, vector);
        let hits = rank_and_limit(fused, query, core_document.or(user_document), options);
        Ok(response(
            query,
            core_document,
            None,
            Vec::new(),
            hits,
            options,
        ))
    }

    fn fts_query(&self, value: &str, operator: &str) -> String {
        self.terms
            .find_iter(value)
            .map(|term| format!("\"{}\"", term.as_str().replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(operator)
    }

    fn database_filters(
        &self,
        filters: &SearchFilters,
    ) -> Result<(DatabaseQueryFilter, DatabaseQueryFilter), RetrievalError> {
        let document_types = filters
            .document_types
            .iter()
            .map(document_type_name)
            .map(str::to_owned)
            .collect::<Vec<_>>();
        let mut core_ids = Vec::new();
        let mut user_ids = Vec::new();
        for key in &filters.document_keys {
            if let Some(id) = key.strip_prefix("core:") {
                core_ids.push(id.to_owned());
            } else if let Some(id) = key.strip_prefix("user:") {
                user_ids.push(id.to_owned());
            } else {
                core_ids.push(key.clone());
                user_ids.push(key.clone());
            }
        }

        let mut core_match_none = filters.library == LibraryScope::UserOnly;
        let mut user_match_none = filters.library == LibraryScope::CoreOnly || self.user.is_none();
        if !filters.topic_ids.is_empty() {
            let topic_documents = self.topics.documents_for(&filters.topic_ids)?;
            if core_ids.is_empty() {
                core_ids.extend(topic_documents);
            } else {
                core_ids.retain(|id| topic_documents.contains(id));
            }
            core_match_none |= core_ids.is_empty();
            user_match_none = true;
        }
        if !filters.document_keys.is_empty() {
            core_match_none |= core_ids.is_empty();
            user_match_none |= user_ids.is_empty();
        }
        Ok((
            DatabaseQueryFilter {
                document_types: document_types.clone(),
                document_ids: core_ids,
                match_none: core_match_none,
            },
            DatabaseQueryFilter {
                document_types,
                document_ids: user_ids,
                match_none: user_match_none,
            },
        ))
    }
}

fn response(
    query: &str,
    detected_document_id: Option<&str>,
    detected_locator: Option<String>,
    documents: Vec<ilia_core::DocumentSummary>,
    hits: Vec<SearchHit>,
    options: SearchOptions,
) -> SearchResponse {
    SearchResponse {
        query: query.to_owned(),
        normalized_query: normalize(query),
        detected_document_id: detected_document_id.map(str::to_owned),
        detected_locator,
        documents,
        evidence: select_evidence(&hits, options),
        hits,
    }
}

fn reciprocal_rank_fusion(lexical: Vec<SearchHit>, vector: Vec<SearchHit>) -> Vec<SearchHit> {
    let mut fused: HashMap<String, (SearchHit, f64)> = HashMap::new();
    for (rank, hit) in lexical.into_iter().enumerate() {
        let score = 1.0 / (60.0 + (rank + 1) as f64);
        let entry = fused
            .entry(hit.stable_key.to_string())
            .or_insert((hit, 0.0));
        entry.1 += score;
    }
    for (rank, hit) in vector.into_iter().enumerate() {
        let score = 1.0 / (60.0 + (rank + 1) as f64);
        let entry = fused
            .entry(hit.stable_key.to_string())
            .or_insert((hit, 0.0));
        entry.1 += score;
    }
    fused
        .into_values()
        .map(|(mut hit, score)| {
            hit.match_kind = MatchKind::Hybrid;
            hit.score = score;
            hit
        })
        .collect()
}

fn rank_and_limit(
    mut hits: Vec<SearchHit>,
    query: &str,
    resolved_document: Option<&str>,
    options: SearchOptions,
) -> Vec<SearchHit> {
    let normalized_query = normalize(query);
    for hit in &mut hits {
        let title = normalize(&hit.canonical_title);
        let translated = hit.title_zh.as_deref().map(normalize).unwrap_or_default();
        if (!title.is_empty() && normalized_query.contains(&title))
            || (!translated.is_empty() && normalized_query.contains(&translated))
            || normalized_query.contains(&normalize(&hit.document_id))
        {
            hit.score += 0.05;
        }
        for year in years(query) {
            if hit.canonical_title.contains(year) || hit.citation_label.contains(year) {
                hit.score += 0.01;
            }
        }
    }
    hits.sort_by(|left, right| {
        right.score.total_cmp(&left.score).then_with(|| {
            left.stable_key
                .to_string()
                .cmp(&right.stable_key.to_string())
        })
    });

    let mut seen_text = HashSet::new();
    let mut document_counts = HashMap::<(LibraryKind, String), usize>::new();
    let apply_quota = resolved_document.is_none();
    hits.retain(|hit| {
        if !seen_text.insert(normalize(&hit.text)) {
            return false;
        }
        let count = document_counts
            .entry((hit.library_kind, hit.document_id.clone()))
            .or_default();
        if apply_quota && *count >= options.per_document_limit {
            return false;
        }
        *count += 1;
        true
    });
    hits.truncate(options.limit);
    hits
}

fn select_evidence(hits: &[SearchHit], options: SearchOptions) -> Vec<EvidenceItem> {
    let mut evidence = Vec::<EvidenceItem>::new();
    let mut used_chars = 0usize;
    for hit in hits.iter().take(options.evidence_limit) {
        let remaining = options.evidence_char_budget.saturating_sub(used_chars);
        if remaining == 0 {
            break;
        }
        let text = hit.text.chars().take(remaining).collect::<String>();
        used_chars += text.chars().count();
        let adjacent = options.merge_adjacent
            && evidence.last().is_some_and(|previous| {
                previous.related_chunk_ids.last().is_some_and(|last| {
                    are_adjacent(last, &hit.chunk_id)
                        && previous
                            .related_chunk_ids
                            .first()
                            .is_some_and(|first| same_document_chunk(first, &hit.chunk_id))
                })
            });
        if adjacent {
            let previous = evidence.last_mut().expect("evidence is non-empty");
            previous.related_chunk_ids.push(hit.chunk_id.clone());
            previous.citation_label.push_str("; ");
            previous.citation_label.push_str(&hit.citation_label);
            previous.text.push_str("\n\n");
            previous.text.push_str(&text);
            previous
                .selection_reason
                .push_str("; adjacent content merged");
            continue;
        }
        evidence.push(EvidenceItem {
            rank: evidence.len() + 1,
            stable_key: hit.stable_key.clone(),
            chunk_id: hit.chunk_id.clone(),
            related_chunk_ids: vec![hit.chunk_id.clone()],
            citation_label: hit.citation_label.clone(),
            selection_reason: selection_reason(hit.match_kind).to_owned(),
            text,
        });
    }
    evidence
}

fn selection_reason(kind: MatchKind) -> &'static str {
    match kind {
        MatchKind::ExactArticle => "exact article locator",
        MatchKind::ExactCaseParagraph => "exact case paragraph locator",
        MatchKind::FullText => "FTS5 lexical match",
        MatchKind::Vector => "BGE-M3 semantic match",
        MatchKind::Hybrid => "RRF fusion of FTS5 and BGE-M3",
    }
}

fn strip_aliases(
    query: &str,
    core: Option<&ilia_database::ResolvedDocument>,
    user: Option<&ilia_database::ResolvedDocument>,
) -> String {
    let mut searchable = query.to_owned();
    if let Some(resolved) = core {
        searchable = strip_alias(&searchable, &resolved.matched_alias);
    }
    if let Some(resolved) = user {
        searchable = strip_alias(&searchable, &resolved.matched_alias);
    }
    searchable
}

fn strip_alias(query: &str, alias: &str) -> String {
    if query.contains(alias) {
        query.replace(alias, " ")
    } else if ilia_database::fuzzy_alias_distance(query, alias).is_some() {
        String::new()
    } else {
        query.to_owned()
    }
}

fn capture_locator(regex: &Regex, query: &str) -> Option<i64> {
    regex
        .captures(query)
        .and_then(|captures| captures.get(1))
        .and_then(|value| parse_locator(value.as_str()))
}

fn parse_locator(value: &str) -> Option<i64> {
    value
        .parse()
        .ok()
        .or_else(|| roman_to_integer(&value.to_ascii_uppercase()))
}

fn roman_to_integer(value: &str) -> Option<i64> {
    let mut total = 0i64;
    let mut previous = 0i64;
    for character in value.chars().rev() {
        let current = match character {
            'I' => 1,
            'V' => 5,
            'X' => 10,
            'L' => 50,
            'C' => 100,
            'D' => 500,
            'M' => 1000,
            _ => return None,
        };
        if current < previous {
            total -= current;
        } else {
            total += current;
            previous = current;
        }
    }
    (total > 0).then_some(total)
}

fn document_type_name(value: &DocumentType) -> &'static str {
    match value {
        DocumentType::Treaty => "treaty",
        DocumentType::Judgment => "judgment",
        DocumentType::AdvisoryOpinion => "advisory_opinion",
        DocumentType::Order => "order",
        DocumentType::Resolution => "resolution",
        DocumentType::DraftArticles => "draft_articles",
        DocumentType::CustomaryRule => "customary_rule",
        DocumentType::Commentary => "commentary",
        DocumentType::Declaration => "declaration",
        DocumentType::Statute => "statute",
        DocumentType::UserDocument => "user_document",
    }
}

fn years(value: &str) -> impl Iterator<Item = &str> {
    value
        .split(|character: char| !character.is_ascii_digit())
        .filter(|part| part.len() == 4)
}

fn chunk_locator(chunk_id: &str) -> Option<(&str, i64)> {
    let parts = chunk_id.split('-').collect::<Vec<_>>();
    for (index, part) in parts.iter().enumerate() {
        if matches!(*part, "art" | "para" | "page") {
            return parts
                .get(index + 1)
                .and_then(|number| number.parse().ok())
                .map(|number| (*part, number));
        }
    }
    None
}

fn same_document_chunk(left: &str, right: &str) -> bool {
    let Some((left_kind, _)) = chunk_locator(left) else {
        return false;
    };
    let Some((right_kind, _)) = chunk_locator(right) else {
        return false;
    };
    left_kind == right_kind
        && left.split(&format!("-{left_kind}-")).next()
            == right.split(&format!("-{right_kind}-")).next()
}

fn are_adjacent(left: &str, right: &str) -> bool {
    match (chunk_locator(left), chunk_locator(right)) {
        (Some((left_kind, left_number)), Some((right_kind, right_number))) => {
            left_kind == right_kind && (left_number - right_number).abs() == 1
        }
        _ => false,
    }
}

fn normalize(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ilia_core::StableEvidenceKey;

    #[test]
    fn locator_patterns_cover_english_chinese_and_roman_numerals() {
        let article_en =
            Regex::new(r"(?i)\b(?:article|art\.?)\s*([0-9]{1,3}|[ivxlcdm]{1,8})\b").unwrap();
        let article_zh = Regex::new(r"第\s*(\d{1,3})\s*条").unwrap();
        let paragraph_en = Regex::new(r"(?i)\b(?:paragraph|para\.?)\s*(\d{1,4})\b").unwrap();
        let paragraph_zh = Regex::new(r"第\s*(\d{1,4})\s*段").unwrap();
        assert_eq!(capture_locator(&article_en, "VCLT Article 27"), Some(27));
        assert_eq!(capture_locator(&article_en, "UNCLOS Art. III"), Some(3));
        assert_eq!(
            capture_locator(&article_zh, "维也纳条约法公约第31条"),
            Some(31)
        );
        assert_eq!(
            capture_locator(&paragraph_en, "Nicaragua para. 191"),
            Some(191)
        );
        assert_eq!(
            capture_locator(&paragraph_zh, "尼加拉瓜案第191段"),
            Some(191)
        );
    }

    #[test]
    fn adjacent_chunk_detection_requires_same_document_and_locator_kind() {
        assert!(are_adjacent("unclos-1982-art-2-en", "unclos-1982-art-3-en"));
        assert!(same_document_chunk(
            "unclos-1982-art-2-en",
            "unclos-1982-art-3-en"
        ));
        assert!(!same_document_chunk(
            "vclt-1969-art-2-en",
            "unclos-1982-art-3-en"
        ));
    }

    #[test]
    fn adjacent_hits_merge_into_one_evidence_item() {
        let hit = |number: i64| SearchHit {
            library_kind: LibraryKind::Core,
            stable_key: StableEvidenceKey {
                library: LibraryKind::Core,
                document_id: "unclos-1982".to_owned(),
                chunk_id: format!("unclos-1982-art-{number}-en"),
            },
            chunk_id: format!("unclos-1982-art-{number}-en"),
            document_id: "unclos-1982".to_owned(),
            canonical_title: "UNCLOS".to_owned(),
            title_zh: None,
            document_type: "treaty".to_owned(),
            legal_status: "in_force".to_owned(),
            official_source_url: String::new(),
            citation_label: format!("UNCLOS, Article {number}"),
            page_start: number,
            page_end: number,
            language: "en".to_owned(),
            text: format!("Article {number} text"),
            match_kind: MatchKind::Hybrid,
            score: 1.0,
        };
        let evidence = select_evidence(&[hit(2), hit(3)], SearchOptions::default());
        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0].related_chunk_ids.len(), 2);
        assert!(evidence[0].text.contains("Article 3 text"));
    }

    #[test]
    fn related_document_registry_is_bidirectional_and_rejects_duplicates() {
        let root = std::env::temp_dir().join(format!("ilia-relations-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&root);
        let valid = root.join("valid.json");
        std::fs::write(
            &valid,
            r#"{"format_version":1,"relations":[{"left":"a","right":"b"}]}"#,
        )
        .unwrap();
        let registry = RelatedDocumentRegistry::from_path(&valid).unwrap();
        assert_eq!(registry.related_to("a"), ["b"]);
        assert_eq!(registry.related_to("b"), ["a"]);
        let invalid = root.join("invalid.json");
        std::fs::write(&invalid,r#"{"format_version":1,"relations":[{"left":"a","right":"b"},{"left":"b","right":"a"}]}"#).unwrap();
        assert!(RelatedDocumentRegistry::from_path(&invalid).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
