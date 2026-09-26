use std::path::Path;

use ilia_core::{DocumentSummary, LibraryKind, MatchKind, SearchHit, StableEvidenceKey};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};

use crate::{DatabaseError, DatabaseQueryFilter, ResolvedDocument, fuzzy_alias_distance};

pub struct UserDatabase {
    connection: Connection,
}

impl UserDatabase {
    pub fn open_read_only(path: impl AsRef<Path>) -> Result<Self, DatabaseError> {
        let connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        connection.pragma_update(None, "query_only", true)?;
        let version = connection
            .query_row("SELECT max(version) FROM schema_migrations", [], |row| {
                row.get::<_, Option<i64>>(0)
            })?
            .unwrap_or(0);
        if version != crate::USER_SCHEMA_VERSION {
            return Err(DatabaseError::UnsupportedWritableSchema {
                database: "user",
                found: version,
                supported: crate::USER_SCHEMA_VERSION,
            });
        }
        Ok(Self { connection })
    }

    pub fn documents(&self) -> Result<Vec<DocumentSummary>, DatabaseError> {
        let mut statement = self.connection.prepare(
            "SELECT id,title,document_type FROM user_documents WHERE import_status='ready' ORDER BY created_at DESC,id",
        )?;
        let rows = statement.query_map([], |row| {
            let document_id: String = row.get(0)?;
            Ok(DocumentSummary {
                library_kind: LibraryKind::User,
                stable_document_key: format!("user:{document_id}"),
                document_id,
                canonical_title: row.get(1)?,
                title_zh: None,
                short_title: None,
                document_type: row.get(2)?,
                legal_status: "user_supplied".to_owned(),
                official_source_url: String::new(),
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn document_text(&self, document_id: &str) -> Result<Option<String>, DatabaseError> {
        let exists = self
            .connection
            .query_row(
                "SELECT 1 FROM user_documents WHERE id=?1 AND import_status='ready'",
                [document_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !exists {
            return Ok(None);
        }
        let mut statement = self.connection.prepare(
            "SELECT text_original FROM user_chunks WHERE document_id=?1 ORDER BY sequence_number",
        )?;
        let parts = statement
            .query_map([document_id], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Some(parts.join("\n\n")))
    }

    pub fn resolve_document_filtered(
        &self,
        query: &str,
        filter: &DatabaseQueryFilter,
    ) -> Result<Option<ResolvedDocument>, DatabaseError> {
        if filter.match_none {
            return Ok(None);
        }
        let normalized_query = normalize(query);
        let document_types = filter.document_types_json()?;
        let document_ids = filter.document_ids_json()?;
        let mut statement = self.connection.prepare(
            r#"SELECT candidates.document_id, candidates.alias FROM (
                SELECT document_id, alias FROM user_document_aliases
                UNION ALL SELECT id, title FROM user_documents) candidates
                JOIN user_documents d ON d.id = candidates.document_id
                WHERE d.import_status = 'ready'
                  AND (json_array_length(?1) = 0 OR d.document_type IN (SELECT value FROM json_each(?1)))
                  AND (json_array_length(?2) = 0 OR d.id IN (SELECT value FROM json_each(?2)))
                ORDER BY length(candidates.alias) DESC"#,
        )?;
        let aliases = statement.query_map(params![document_types, document_ids], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut fuzzy_match: Option<(usize, ResolvedDocument)> = None;
        for alias in aliases {
            let (document_id, alias) = alias?;
            let normalized_alias = normalize(&alias);
            if !normalized_alias.is_empty() && normalized_query.contains(&normalized_alias) {
                return Ok(Some(ResolvedDocument {
                    document_id,
                    matched_alias: alias,
                }));
            }
            if let Some(distance) = fuzzy_alias_distance(query, &alias)
                && fuzzy_match
                    .as_ref()
                    .is_none_or(|(best_distance, _)| distance < *best_distance)
            {
                fuzzy_match = Some((
                    distance,
                    ResolvedDocument {
                        document_id,
                        matched_alias: alias,
                    },
                ));
            }
        }
        Ok(fuzzy_match.map(|(_, resolved)| resolved))
    }

    pub fn document_filtered(
        &self,
        document_id: &str,
        filter: &DatabaseQueryFilter,
    ) -> Result<Option<DocumentSummary>, DatabaseError> {
        if filter.match_none {
            return Ok(None);
        }
        let document_types = filter.document_types_json()?;
        let document_ids = filter.document_ids_json()?;
        self.connection
            .query_row(
                r#"SELECT id, title, language, document_type
                   FROM user_documents
                   WHERE id = ?1 AND import_status = 'ready'
                     AND (json_array_length(?2) = 0 OR document_type IN (SELECT value FROM json_each(?2)))
                     AND (json_array_length(?3) = 0 OR id IN (SELECT value FROM json_each(?3)))"#,
                params![document_id, document_types, document_ids],
                |row| {
                    let document_id: String = row.get(0)?;
                    Ok(DocumentSummary {
                        library_kind: LibraryKind::User,
                        stable_document_key: format!("user:{document_id}"),
                        document_id,
                        canonical_title: row.get(1)?,
                        title_zh: None,
                        short_title: None,
                        document_type: row.get(3)?,
                        legal_status: "user_supplied".to_owned(),
                        official_source_url: String::new(),
                    })
                },
            )
            .optional()
            .map_err(DatabaseError::from)
    }

    pub fn full_text_filtered(
        &self,
        fts_query: &str,
        document_id: Option<&str>,
        limit: usize,
        filter: &DatabaseQueryFilter,
    ) -> Result<Vec<SearchHit>, DatabaseError> {
        if fts_query.trim().is_empty() {
            return Err(DatabaseError::InvalidFtsQuery);
        }
        if filter.match_none {
            return Ok(Vec::new());
        }
        let document_types = filter.document_types_json()?;
        let document_ids = filter.document_ids_json()?;
        let mut statement = self.connection.prepare(
            r#"SELECT c.id, c.document_id, d.title, d.document_type,
                      c.citation_label, c.language, c.text_original,
                      bm25(user_chunks_fts) AS rank
               FROM user_chunks_fts
               JOIN user_chunks c ON c.id = user_chunks_fts.chunk_id
               JOIN user_documents d ON d.id = c.document_id
               WHERE user_chunks_fts MATCH ?1 AND d.import_status = 'ready'
                 AND (?2 IS NULL OR c.document_id = ?2)
                 AND (json_array_length(?3) = 0 OR d.document_type IN (SELECT value FROM json_each(?3)))
                 AND (json_array_length(?4) = 0 OR d.id IN (SELECT value FROM json_each(?4)))
               ORDER BY rank, c.sequence_number
               LIMIT ?5"#,
        )?;
        let rows = statement.query_map(
            params![
                fts_query,
                document_id,
                document_types,
                document_ids,
                limit as i64
            ],
            |row| user_hit(row, MatchKind::FullText, -row.get::<_, f64>(7)?),
        )?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn vector_search_filtered(
        &self,
        query_vector: &[f32],
        model_id: &str,
        document_id: Option<&str>,
        limit: usize,
        filter: &DatabaseQueryFilter,
    ) -> Result<Vec<SearchHit>, DatabaseError> {
        if filter.match_none {
            return Ok(Vec::new());
        }
        let document_types = filter.document_types_json()?;
        let document_ids = filter.document_ids_json()?;
        let query_norm = query_vector
            .iter()
            .map(|value| value * value)
            .sum::<f32>()
            .sqrt();
        let mut statement = self.connection.prepare(
            r#"SELECT c.id, c.document_id, d.title, d.document_type,
                      c.citation_label, c.language, c.text_original,
                      e.vector, e.vector_norm, e.dimension
               FROM user_embeddings e
               JOIN user_chunks c ON c.id = e.chunk_id
               JOIN user_documents d ON d.id = c.document_id
               WHERE e.model_id = ?1 AND d.import_status = 'ready'
                 AND (?2 IS NULL OR c.document_id = ?2)
                 AND (json_array_length(?3) = 0 OR d.document_type IN (SELECT value FROM json_each(?3)))
                 AND (json_array_length(?4) = 0 OR d.id IN (SELECT value FROM json_each(?4)))"#,
        )?;
        let rows = statement.query_map(
            params![model_id, document_id, document_types, document_ids],
            |row| {
                let dimension: i64 = row.get(9)?;
                let bytes: Vec<u8> = row.get(7)?;
                let norm: f32 = row.get(8)?;
                let hit = user_hit(row, MatchKind::Vector, 0.0)?;
                Ok((hit, dimension, bytes, norm))
            },
        )?;
        let mut hits = Vec::new();
        for row in rows {
            let (mut hit, dimension, bytes, vector_norm) = row?;
            if dimension as usize != query_vector.len() {
                return Err(DatabaseError::DimensionMismatch {
                    expected: dimension as usize,
                    actual: query_vector.len(),
                });
            }
            if bytes.len() != query_vector.len() * 4 {
                return Err(DatabaseError::InvalidVector(hit.chunk_id));
            }
            let (chunks, remainder) = bytes.as_chunks::<4>();
            debug_assert!(remainder.is_empty());
            let dot = chunks
                .iter()
                .zip(query_vector)
                .map(|(chunk, query)| f32::from_le_bytes(*chunk) * query)
                .sum::<f32>();
            hit.score = (dot / (vector_norm * query_norm).max(f32::EPSILON)) as f64;
            hits.push(hit);
        }
        hits.sort_by(|left, right| right.score.total_cmp(&left.score));
        hits.truncate(limit);
        Ok(hits)
    }
}

fn user_hit(
    row: &rusqlite::Row<'_>,
    match_kind: MatchKind,
    score: f64,
) -> rusqlite::Result<SearchHit> {
    let chunk_id: String = row.get(0)?;
    let document_id: String = row.get(1)?;
    Ok(SearchHit {
        library_kind: LibraryKind::User,
        stable_key: StableEvidenceKey {
            library: LibraryKind::User,
            document_id: document_id.clone(),
            chunk_id: chunk_id.clone(),
        },
        chunk_id,
        document_id,
        canonical_title: row.get(2)?,
        title_zh: None,
        document_type: row.get(3)?,
        legal_status: "user_supplied".to_owned(),
        official_source_url: String::new(),
        citation_label: row.get(4)?,
        page_start: 0,
        page_end: 0,
        language: row.get(5)?,
        text: row.get(6)?,
        match_kind,
        score,
    })
}

fn normalize(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}
