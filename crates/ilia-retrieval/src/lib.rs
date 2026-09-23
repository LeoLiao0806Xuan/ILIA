use std::collections::HashMap;

use ilia_core::{EvidenceItem, MatchKind, SearchHit, SearchOptions, SearchResponse};
use ilia_database::{Database, DatabaseError};
use regex::Regex;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RetrievalError {
    #[error(transparent)]
    Database(#[from] DatabaseError),
    #[error("query must not be empty")]
    EmptyQuery,
}

pub struct RetrievalService {
    database: Database,
    article_en: Regex,
    article_zh: Regex,
    paragraph_en: Regex,
    paragraph_zh: Regex,
    terms: Regex,
}

impl RetrievalService {
    pub fn new(database: Database) -> Self {
        Self {
            database,
            article_en: Regex::new(r"(?i)\b(?:article|art\.?)\s*(\d{1,3})\b").unwrap(),
            article_zh: Regex::new(r"第\s*(\d{1,3})\s*条").unwrap(),
            paragraph_en: Regex::new(r"(?i)\b(?:paragraph|para\.?)\s*(\d{1,3})\b").unwrap(),
            paragraph_zh: Regex::new(r"第\s*(\d{1,3})\s*段").unwrap(),
            terms: Regex::new(r"[\p{L}\p{N}]{2,}").unwrap(),
        }
    }

    pub fn search(
        &self,
        query: &str,
        options: SearchOptions,
    ) -> Result<SearchResponse, RetrievalError> {
        let query = query.trim();
        if query.is_empty() {
            return Err(RetrievalError::EmptyQuery);
        }
        let resolved = self.database.resolve_document(query)?;
        let document_id = resolved.as_ref().map(|value| value.document_id.as_str());

        if let Some(number) = capture_number(&self.article_zh, query)
            .or_else(|| capture_number(&self.article_en, query))
        {
            let hits = self.database.article(document_id, number, options.limit)?;
            return Ok(SearchResponse {
                query: query.to_owned(),
                normalized_query: normalize(query),
                detected_document_id: document_id.map(str::to_owned),
                detected_locator: Some(format!("article:{number}")),
                documents: Vec::new(),
                evidence: select_evidence(&hits, options),
                hits,
            });
        }

        if let Some(number) = capture_number(&self.paragraph_zh, query)
            .or_else(|| capture_number(&self.paragraph_en, query))
        {
            let hits = self
                .database
                .case_paragraph(document_id, number, options.limit)?;
            return Ok(SearchResponse {
                query: query.to_owned(),
                normalized_query: normalize(query),
                detected_document_id: document_id.map(str::to_owned),
                detected_locator: Some(format!("paragraph:{number}")),
                documents: Vec::new(),
                evidence: select_evidence(&hits, options),
                hits,
            });
        }

        let searchable = if let Some(resolved) = &resolved {
            query.replace(&resolved.matched_alias, " ")
        } else {
            query.to_owned()
        };
        let fts_query = self
            .terms
            .find_iter(&searchable)
            .map(|term| format!("\"{}\"", term.as_str().replace('"', "\"\"")))
            .collect::<Vec<_>>()
            .join(" AND ");
        let documents = if fts_query.is_empty() {
            match document_id {
                Some(id) => self.database.document(id)?.into_iter().collect(),
                None => Vec::new(),
            }
        } else {
            Vec::new()
        };
        let hits = if fts_query.is_empty() {
            Vec::new()
        } else {
            self.database
                .full_text(&fts_query, document_id, options.limit)?
        };
        Ok(SearchResponse {
            query: query.to_owned(),
            normalized_query: normalize(query),
            detected_document_id: document_id.map(str::to_owned),
            detected_locator: None,
            documents,
            evidence: select_evidence(&hits, options),
            hits,
        })
    }

    pub fn search_hybrid(
        &self,
        query: &str,
        query_vector: &[f32],
        model_id: &str,
        options: SearchOptions,
    ) -> Result<SearchResponse, RetrievalError> {
        let base = self.search(query, options)?;
        if base.detected_locator.is_some() || !base.documents.is_empty() {
            return Ok(base);
        }
        let document_id = base.detected_document_id.as_deref();
        let resolved = self.database.resolve_document(query)?;
        let searchable = if let Some(resolved) = &resolved {
            query.replace(&resolved.matched_alias, " ")
        } else {
            query.to_owned()
        };
        let terms = self
            .terms
            .find_iter(&searchable)
            .map(|term| format!("\"{}\"", term.as_str().replace('"', "\"\"")))
            .collect::<Vec<_>>();
        let strict_fts_query = terms.join(" AND ");
        let mut lexical = if strict_fts_query.is_empty() {
            Vec::new()
        } else {
            self.database
                .full_text(&strict_fts_query, document_id, options.candidate_limit)?
        };
        if lexical.is_empty() && terms.len() > 1 {
            lexical = self.database.full_text(
                &terms.join(" OR "),
                document_id,
                options.candidate_limit,
            )?;
        }
        let vector = self.database.vector_search(
            query_vector,
            model_id,
            document_id,
            options.candidate_limit,
        )?;
        let hits = reciprocal_rank_fusion(lexical, vector, options.limit);
        Ok(SearchResponse {
            query: base.query,
            normalized_query: base.normalized_query,
            detected_document_id: base.detected_document_id,
            detected_locator: None,
            documents: Vec::new(),
            evidence: select_evidence(&hits, options),
            hits,
        })
    }
}

fn reciprocal_rank_fusion(
    lexical: Vec<SearchHit>,
    vector: Vec<SearchHit>,
    limit: usize,
) -> Vec<SearchHit> {
    let mut fused: HashMap<String, (SearchHit, f64)> = HashMap::new();
    for (rank, hit) in lexical.into_iter().enumerate() {
        let score = 1.0 / (60.0 + (rank + 1) as f64);
        let entry = fused.entry(hit.chunk_id.clone()).or_insert((hit, 0.0));
        entry.1 += score;
    }
    for (rank, hit) in vector.into_iter().enumerate() {
        let score = 1.0 / (60.0 + (rank + 1) as f64);
        let entry = fused.entry(hit.chunk_id.clone()).or_insert((hit, 0.0));
        entry.1 += score;
    }
    let mut hits = fused
        .into_values()
        .map(|(mut hit, score)| {
            hit.match_kind = MatchKind::Hybrid;
            hit.score = score;
            hit
        })
        .collect::<Vec<_>>();
    hits.sort_by(|left, right| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left.chunk_id.cmp(&right.chunk_id))
    });
    hits.truncate(limit);
    hits
}

fn select_evidence(hits: &[SearchHit], options: SearchOptions) -> Vec<EvidenceItem> {
    let mut used_chars = 0usize;
    hits.iter()
        .take(options.evidence_limit)
        .filter_map(|hit| {
            let remaining = options.evidence_char_budget.saturating_sub(used_chars);
            if remaining == 0 {
                return None;
            }
            let text = hit.text.chars().take(remaining).collect::<String>();
            used_chars += text.chars().count();
            Some(EvidenceItem {
                rank: used_chars,
                chunk_id: hit.chunk_id.clone(),
                citation_label: hit.citation_label.clone(),
                selection_reason: match hit.match_kind {
                    MatchKind::ExactArticle => "exact article locator".to_owned(),
                    MatchKind::ExactCaseParagraph => "exact case paragraph locator".to_owned(),
                    MatchKind::FullText => "FTS5 lexical match".to_owned(),
                    MatchKind::Vector => "BGE-M3 semantic match".to_owned(),
                    MatchKind::Hybrid => "RRF fusion of FTS5 and BGE-M3".to_owned(),
                },
                text,
            })
        })
        .enumerate()
        .map(|(index, mut item)| {
            item.rank = index + 1;
            item
        })
        .collect()
}

fn capture_number(regex: &Regex, query: &str) -> Option<i64> {
    regex
        .captures(query)
        .and_then(|captures| captures.get(1))
        .and_then(|value| value.as_str().parse().ok())
}

fn normalize(value: &str) -> String {
    value
        .chars()
        .filter(|character| !character.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locator_patterns_cover_english_and_chinese() {
        let article_en = Regex::new(r"(?i)\b(?:article|art\.?)\s*(\d{1,3})\b").unwrap();
        let article_zh = Regex::new(r"第\s*(\d{1,3})\s*条").unwrap();
        let paragraph_en = Regex::new(r"(?i)\b(?:paragraph|para\.?)\s*(\d{1,3})\b").unwrap();
        let paragraph_zh = Regex::new(r"第\s*(\d{1,3})\s*段").unwrap();
        assert_eq!(capture_number(&article_en, "VCLT Article 27"), Some(27));
        assert_eq!(
            capture_number(&article_zh, "维也纳条约法公约第31条"),
            Some(31)
        );
        assert_eq!(
            capture_number(&paragraph_en, "Nicaragua para. 191"),
            Some(191)
        );
        assert_eq!(
            capture_number(&paragraph_zh, "尼加拉瓜案第191段"),
            Some(191)
        );
    }
}
