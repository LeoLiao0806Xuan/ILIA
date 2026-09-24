use std::path::Path;

use ilia_core::{DocumentSummary, MatchKind, SearchHit};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DatabaseError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("unsupported or missing ILIA schema; expected schema version 001")]
    InvalidSchema,
    #[error("invalid FTS query")]
    InvalidFtsQuery,
    #[error("invalid vector blob for chunk {0}")]
    InvalidVector(String),
    #[error("embedding dimension mismatch: expected {expected}, got {actual}")]
    DimensionMismatch { expected: usize, actual: usize },
}

#[derive(Debug, Clone)]
pub struct ResolvedDocument {
    pub document_id: String,
    pub matched_alias: String,
}

#[derive(Debug, Clone)]
pub struct ChunkForEmbedding {
    pub chunk_id: String,
    pub text: String,
}

pub struct Database {
    connection: Connection,
}

impl Database {
    pub fn open_read_only(path: impl AsRef<Path>) -> Result<Self, DatabaseError> {
        let connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )?;
        connection.pragma_update(None, "query_only", true)?;
        let version: Option<String> = connection
            .query_row(
                "SELECT schema_version FROM schema_metadata ORDER BY applied_at DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()?;
        if version.as_deref() != Some("001") {
            return Err(DatabaseError::InvalidSchema);
        }
        Ok(Self { connection })
    }

    pub fn open_for_indexing(path: impl AsRef<Path>) -> Result<Self, DatabaseError> {
        let connection = Connection::open(path)?;
        connection.pragma_update(None, "foreign_keys", true)?;
        Ok(Self { connection })
    }

    pub fn chunks_for_embedding(&self) -> Result<Vec<ChunkForEmbedding>, DatabaseError> {
        let mut statement = self.connection.prepare(
            r#"SELECT c.id, d.canonical_title || '\n' || c.citation_label || '\n' || c.text_original
               FROM chunks c JOIN documents d ON d.id = c.document_id ORDER BY c.id"#,
        )?;
        let rows = statement.query_map([], |row| {
            Ok(ChunkForEmbedding {
                chunk_id: row.get(0)?,
                text: row.get(1)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn replace_embeddings(
        &mut self,
        model_id: &str,
        dimension: usize,
        embeddings: &[(String, Vec<f32>)],
    ) -> Result<(), DatabaseError> {
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "INSERT INTO embedding_models(id, dimension, vector_format, normalized) VALUES (?1, ?2, 'f32le', 1) ON CONFLICT(id) DO UPDATE SET dimension=excluded.dimension",
            params![model_id, dimension as i64],
        )?;
        transaction.execute(
            "DELETE FROM chunk_embeddings WHERE model_id = ?1",
            [model_id],
        )?;
        for (chunk_id, vector) in embeddings {
            if vector.len() != dimension {
                return Err(DatabaseError::DimensionMismatch {
                    expected: dimension,
                    actual: vector.len(),
                });
            }
            let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
            let bytes = vector
                .iter()
                .flat_map(|value| value.to_le_bytes())
                .collect::<Vec<_>>();
            transaction.execute(
                "INSERT INTO chunk_embeddings(chunk_id, model_id, vector, vector_norm) VALUES (?1, ?2, ?3, ?4)",
                params![chunk_id, model_id, bytes, norm],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn embedding_count(&self, model_id: &str) -> Result<usize, DatabaseError> {
        Ok(self.connection.query_row(
            "SELECT count(*) FROM chunk_embeddings WHERE model_id = ?1",
            [model_id],
            |row| row.get::<_, i64>(0),
        )? as usize)
    }

    pub fn resolve_document(&self, query: &str) -> Result<Option<ResolvedDocument>, DatabaseError> {
        let normalized_query = normalize(query);
        let mut statement = self.connection.prepare(
            r#"SELECT document_id, alias FROM (
            SELECT document_id, alias FROM document_aliases
            UNION ALL SELECT id AS document_id, id AS alias FROM documents)
            ORDER BY length(alias) DESC"#,
        )?;
        let aliases = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for alias in aliases {
            let (document_id, alias) = alias?;
            let normalized_alias = normalize(&alias);
            if !normalized_alias.is_empty() && normalized_query.contains(&normalized_alias) {
                return Ok(Some(ResolvedDocument {
                    document_id,
                    matched_alias: alias,
                }));
            }
        }
        Ok(None)
    }

    pub fn document(&self, document_id: &str) -> Result<Option<DocumentSummary>, DatabaseError> {
        self.connection
            .query_row(
                r#"SELECT id, canonical_title, title_zh, short_title, document_type,
                    legal_status, official_source_url
                    FROM documents WHERE id = ?1"#,
                [document_id],
                |row| {
                    Ok(DocumentSummary {
                        document_id: row.get(0)?,
                        canonical_title: row.get(1)?,
                        title_zh: row.get(2)?,
                        short_title: row.get(3)?,
                        document_type: row.get(4)?,
                        legal_status: row.get(5)?,
                        official_source_url: row.get(6)?,
                    })
                },
            )
            .optional()
            .map_err(DatabaseError::from)
    }

    pub fn documents(&self) -> Result<Vec<DocumentSummary>, DatabaseError> {
        let mut statement = self.connection.prepare(
            r#"SELECT id, canonical_title, title_zh, short_title, document_type,
                legal_status, official_source_url
                FROM documents
                ORDER BY COALESCE(title_zh, canonical_title), canonical_title"#,
        )?;
        let rows = statement.query_map([], |row| {
            Ok(DocumentSummary {
                document_id: row.get(0)?,
                canonical_title: row.get(1)?,
                title_zh: row.get(2)?,
                short_title: row.get(3)?,
                document_type: row.get(4)?,
                legal_status: row.get(5)?,
                official_source_url: row.get(6)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn normalized_document_text(
        &self,
        document_id: &str,
    ) -> Result<Option<String>, DatabaseError> {
        self.connection
            .query_row(
                "SELECT text FROM normalized_document_texts WHERE document_id = ?1",
                [document_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(DatabaseError::from)
    }

    pub fn article(
        &self,
        document_id: Option<&str>,
        article_number: i64,
        limit: usize,
    ) -> Result<Vec<SearchHit>, DatabaseError> {
        let sql = r#"SELECT c.id, c.document_id, d.canonical_title, d.title_zh,
            d.document_type, d.legal_status, d.official_source_url, c.citation_label, c.page_start, c.page_end,
            c.language, c.text_original
            FROM provisions p
            JOIN document_versions v ON v.id = p.document_version_id
            JOIN chunks c ON c.locator_type = 'article' AND c.locator_id = p.id
            JOIN documents d ON d.id = c.document_id
            WHERE CAST(p.article_number AS INTEGER) = ?1
              AND (?2 IS NULL OR d.id = ?2)
            ORDER BY CASE WHEN d.id = ?2 THEN 0 ELSE 1 END, d.canonical_title
            LIMIT ?3"#;
        self.query_hits(
            sql,
            params![article_number, document_id, limit as i64],
            MatchKind::ExactArticle,
        )
    }

    pub fn case_paragraph(
        &self,
        document_id: Option<&str>,
        paragraph_number: i64,
        limit: usize,
    ) -> Result<Vec<SearchHit>, DatabaseError> {
        let sql = r#"SELECT c.id, c.document_id, d.canonical_title, d.title_zh,
            d.document_type, d.legal_status, d.official_source_url, c.citation_label, c.page_start, c.page_end,
            c.language, c.text_original
            FROM case_paragraphs p
            JOIN chunks c ON c.locator_type = 'case_paragraph' AND c.locator_id = p.id
            JOIN documents d ON d.id = c.document_id
            WHERE p.paragraph_number = ?1
              AND (?2 IS NULL OR d.id = ?2)
            ORDER BY CASE WHEN d.id = ?2 THEN 0 ELSE 1 END, d.canonical_title
            LIMIT ?3"#;
        self.query_hits(
            sql,
            params![paragraph_number, document_id, limit as i64],
            MatchKind::ExactCaseParagraph,
        )
    }

    pub fn full_text(
        &self,
        fts_query: &str,
        document_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SearchHit>, DatabaseError> {
        if fts_query.trim().is_empty() {
            return Err(DatabaseError::InvalidFtsQuery);
        }
        let mut statement = self.connection.prepare(
            r#"SELECT c.id, c.document_id, d.canonical_title, d.title_zh,
            d.document_type, d.legal_status, d.official_source_url, c.citation_label, c.page_start, c.page_end,
            c.language, c.text_original, bm25(chunks_fts) AS rank
            FROM chunks_fts
            JOIN chunks c ON c.id = chunks_fts.chunk_id
            JOIN documents d ON d.id = c.document_id
            WHERE chunks_fts MATCH ?1
              AND (?2 IS NULL OR c.document_id = ?2)
            ORDER BY rank, c.citation_label
            LIMIT ?3"#,
        )?;
        let rows = statement.query_map(params![fts_query, document_id, limit as i64], |row| {
            Ok(SearchHit {
                chunk_id: row.get(0)?,
                document_id: row.get(1)?,
                canonical_title: row.get(2)?,
                title_zh: row.get(3)?,
                document_type: row.get(4)?,
                legal_status: row.get(5)?,
                official_source_url: row.get(6)?,
                citation_label: row.get(7)?,
                page_start: row.get(8)?,
                page_end: row.get(9)?,
                language: row.get(10)?,
                text: row.get(11)?,
                match_kind: MatchKind::FullText,
                score: -row.get::<_, f64>(12)?,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn vector_search(
        &self,
        query_vector: &[f32],
        model_id: &str,
        document_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<SearchHit>, DatabaseError> {
        let dimension: Option<i64> = self
            .connection
            .query_row(
                "SELECT dimension FROM embedding_models WHERE id = ?1",
                [model_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(dimension) = dimension else {
            return Ok(Vec::new());
        };
        if query_vector.len() != dimension as usize {
            return Err(DatabaseError::DimensionMismatch {
                expected: dimension as usize,
                actual: query_vector.len(),
            });
        }
        let query_norm = query_vector
            .iter()
            .map(|value| value * value)
            .sum::<f32>()
            .sqrt();
        let mut statement = self.connection.prepare(
            r#"SELECT c.id, c.document_id, d.canonical_title, d.title_zh,
            d.document_type, d.legal_status, d.official_source_url, c.citation_label, c.page_start, c.page_end,
            c.language, c.text_original, e.vector, e.vector_norm
            FROM chunk_embeddings e
            JOIN chunks c ON c.id = e.chunk_id
            JOIN documents d ON d.id = c.document_id
            WHERE e.model_id = ?1 AND (?2 IS NULL OR c.document_id = ?2)"#,
        )?;
        let rows = statement.query_map(params![model_id, document_id], |row| {
            let bytes: Vec<u8> = row.get(12)?;
            let vector_norm: f32 = row.get(13)?;
            Ok((
                SearchHit {
                    chunk_id: row.get(0)?,
                    document_id: row.get(1)?,
                    canonical_title: row.get(2)?,
                    title_zh: row.get(3)?,
                    document_type: row.get(4)?,
                    legal_status: row.get(5)?,
                    official_source_url: row.get(6)?,
                    citation_label: row.get(7)?,
                    page_start: row.get(8)?,
                    page_end: row.get(9)?,
                    language: row.get(10)?,
                    text: row.get(11)?,
                    match_kind: MatchKind::Vector,
                    score: 0.0,
                },
                bytes,
                vector_norm,
            ))
        })?;
        let mut hits = Vec::new();
        for row in rows {
            let (mut hit, bytes, vector_norm) = row?;
            if bytes.len() != query_vector.len() * 4 {
                return Err(DatabaseError::InvalidVector(hit.chunk_id));
            }
            let (vector_chunks, remainder) = bytes.as_chunks::<4>();
            debug_assert!(remainder.is_empty());
            let dot = vector_chunks
                .iter()
                .zip(query_vector)
                .map(|(chunk, query)| {
                    f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]) * query
                })
                .sum::<f32>();
            hit.score = (dot / (vector_norm * query_norm).max(f32::EPSILON)) as f64;
            hits.push(hit);
        }
        hits.sort_by(|left, right| {
            right
                .score
                .total_cmp(&left.score)
                .then_with(|| left.chunk_id.cmp(&right.chunk_id))
        });
        hits.truncate(limit);
        Ok(hits)
    }

    fn query_hits<P: rusqlite::Params>(
        &self,
        sql: &str,
        params: P,
        match_kind: MatchKind,
    ) -> Result<Vec<SearchHit>, DatabaseError> {
        let mut statement = self.connection.prepare(sql)?;
        let rows = statement.query_map(params, |row| {
            Ok(SearchHit {
                chunk_id: row.get(0)?,
                document_id: row.get(1)?,
                canonical_title: row.get(2)?,
                title_zh: row.get(3)?,
                document_type: row.get(4)?,
                legal_status: row.get(5)?,
                official_source_url: row.get(6)?,
                citation_label: row.get(7)?,
                page_start: row.get(8)?,
                page_end: row.get(9)?,
                language: row.get(10)?,
                text: row.get(11)?,
                match_kind,
                score: 1.0,
            })
        })?;
        Ok(rows.collect::<Result<Vec<_>, _>>()?)
    }
}

fn normalize(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}
