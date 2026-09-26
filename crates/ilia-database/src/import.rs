use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
};

use regex::Regex;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;
use zip::ZipArchive;

use crate::{DatabaseError, USER_SCHEMA_VERSION};

const MAX_IMPORT_BYTES: u64 = 64 * 1024 * 1024;
const MAX_EXTRACTED_BYTES: usize = 32 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum ImportError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("database error: {0}")]
    Database(#[from] DatabaseError),
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("unsupported import format")]
    UnsupportedFormat,
    #[error("file content does not match its extension")]
    ContentMismatch,
    #[error("file exceeds the 64 MiB import limit")]
    TooLarge,
    #[error("the PDF has no usable text layer; OCR is not supported in this version")]
    ScannedPdf,
    #[error("document contains no usable text")]
    EmptyText,
    #[error("DOCX archive is invalid or contains active content")]
    UnsafeDocx,
    #[error("document with the same SHA-256 already exists: {0}")]
    Duplicate(String),
    #[error("import preview is missing or expired")]
    MissingPreview,
    #[error("embedding count or dimension does not match chunks")]
    InvalidEmbeddings,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImportPreview {
    pub preview_id: String,
    pub source_filename: String,
    pub source_sha256: String,
    pub byte_length: u64,
    pub inferred_title: String,
    pub inferred_language: String,
    pub inferred_document_type: String,
    pub text_preview: String,
    pub chunk_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImportChunk {
    pub sequence_number: usize,
    pub citation_label: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CommitImport {
    pub preview_id: String,
    pub title: String,
    pub language: String,
    pub document_type: String,
    pub model_id: String,
    pub embeddings: Vec<Vec<f32>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ImportedDocument {
    pub id: String,
    pub title: String,
    pub source_filename: String,
    pub source_sha256: String,
    pub byte_length: u64,
    pub chunk_count: usize,
}

pub struct UserLibrary {
    database_path: PathBuf,
    preview_root: PathBuf,
}

impl UserLibrary {
    pub fn open(
        database_path: impl Into<PathBuf>,
        app_data_dir: impl AsRef<Path>,
    ) -> Result<Self, ImportError> {
        let database_path = database_path.into();
        validate_schema(&database_path)?;
        let preview_root = app_data_dir.as_ref().join("import-previews");
        fs::create_dir_all(&preview_root)?;
        Ok(Self {
            database_path,
            preview_root,
        })
    }

    pub fn prepare_import(&self, source: impl AsRef<Path>) -> Result<ImportPreview, ImportError> {
        let source = source.as_ref();
        let metadata = fs::metadata(source)?;
        if metadata.len() == 0 {
            return Err(ImportError::EmptyText);
        }
        if metadata.len() > MAX_IMPORT_BYTES {
            return Err(ImportError::TooLarge);
        }
        let bytes = fs::read(source)?;
        let sha256 = format!("{:x}", Sha256::digest(&bytes));
        if let Some(title) = Connection::open(&self.database_path)?
            .query_row(
                "SELECT title FROM user_documents WHERE source_sha256=?1",
                [&sha256],
                |row| row.get::<_, String>(0),
            )
            .optional()?
        {
            return Err(ImportError::Duplicate(title));
        }
        let extension = source
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let text = extract(&extension, &bytes)?;
        let text = normalize_text(&text);
        if text.trim().is_empty() {
            return Err(ImportError::EmptyText);
        }
        if text.len() > MAX_EXTRACTED_BYTES {
            return Err(ImportError::TooLarge);
        }
        let preview_id = Uuid::new_v4().to_string();
        fs::write(
            self.preview_root.join(format!("{preview_id}.txt")),
            text.as_bytes(),
        )?;
        fs::write(
            self.preview_root.join(format!("{preview_id}.json")),
            serde_json::to_vec(&PreviewMetadata {
                source_filename: source
                    .file_name()
                    .and_then(|v| v.to_str())
                    .unwrap_or("document")
                    .to_owned(),
                source_sha256: sha256.clone(),
                byte_length: metadata.len(),
            })
            .map_err(DatabaseError::from)?,
        )?;
        let chunks = chunk_text(&text);
        let inferred_title = infer_title(&text, source);
        Ok(ImportPreview {
            preview_id,
            source_filename: source
                .file_name()
                .and_then(|v| v.to_str())
                .unwrap_or("document")
                .to_owned(),
            source_sha256: sha256,
            byte_length: metadata.len(),
            inferred_title,
            inferred_language: infer_language(&text),
            inferred_document_type: "user_document".into(),
            text_preview: text.chars().take(2_000).collect(),
            chunk_count: chunks.len(),
        })
    }

    pub fn preview_chunks(&self, preview_id: &str) -> Result<Vec<ImportChunk>, ImportError> {
        validate_id(preview_id)?;
        let text = fs::read_to_string(self.preview_root.join(format!("{preview_id}.txt")))
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    ImportError::MissingPreview
                } else {
                    error.into()
                }
            })?;
        Ok(chunk_text(&text))
    }

    pub fn commit_import(&self, request: CommitImport) -> Result<ImportedDocument, ImportError> {
        validate_id(&request.preview_id)?;
        let chunks = self.preview_chunks(&request.preview_id)?;
        if chunks.is_empty() || request.embeddings.len() != chunks.len() {
            return Err(ImportError::InvalidEmbeddings);
        }
        let dimension = request.embeddings[0].len();
        if dimension == 0 || request.embeddings.iter().any(|v| v.len() != dimension) {
            return Err(ImportError::InvalidEmbeddings);
        }
        let metadata: PreviewMetadata = serde_json::from_slice(&fs::read(
            self.preview_root
                .join(format!("{}.json", request.preview_id)),
        )?)
        .map_err(DatabaseError::from)?;
        let id = Uuid::new_v4().to_string();
        let mut connection = Connection::open(&self.database_path)?;
        connection.pragma_update(None, "foreign_keys", true)?;
        let transaction = connection.transaction()?;
        transaction.execute("INSERT INTO user_documents(id,title,language,document_type,source_filename,source_sha256,byte_length,import_status) VALUES (?1,?2,?3,?4,?5,?6,?7,'preparing')",params![id,request.title.trim(),request.language,request.document_type,metadata.source_filename,metadata.source_sha256,metadata.byte_length])?;
        transaction.execute(
            "INSERT INTO user_document_aliases(document_id,language,alias) VALUES (?1,?2,?3)",
            params![id, request.language, request.title.trim()],
        )?;
        for (chunk, vector) in chunks.iter().zip(&request.embeddings) {
            let chunk_id = format!("{id}:{}", chunk.sequence_number);
            let norm = vector.iter().map(|v| v * v).sum::<f32>().sqrt();
            if norm <= f32::EPSILON {
                return Err(ImportError::InvalidEmbeddings);
            }
            let bytes = vector
                .iter()
                .flat_map(|v| v.to_le_bytes())
                .collect::<Vec<_>>();
            transaction.execute("INSERT INTO user_chunks(id,document_id,sequence_number,citation_label,language,text_original,text_normalized) VALUES (?1,?2,?3,?4,?5,?6,?7)",params![chunk_id,id,chunk.sequence_number as i64,chunk.citation_label,request.language,chunk.text,normalize_for_search(&chunk.text)])?;
            transaction.execute("INSERT INTO user_chunks_fts(chunk_id,document_id,title,citation_label,text) VALUES (?1,?2,?3,?4,?5)",params![chunk_id,id,request.title,chunk.citation_label,chunk.text])?;
            transaction.execute("INSERT INTO user_embeddings(chunk_id,model_id,dimension,vector,vector_norm) VALUES (?1,?2,?3,?4,?5)",params![chunk_id,request.model_id,dimension as i64,bytes,norm])?;
        }
        transaction.execute(
            "UPDATE user_documents SET import_status='ready' WHERE id=?1",
            [&id],
        )?;
        transaction.commit()?;
        self.cancel_preview(&request.preview_id)?;
        Ok(ImportedDocument {
            id,
            title: request.title,
            source_filename: metadata.source_filename,
            source_sha256: metadata.source_sha256,
            byte_length: metadata.byte_length,
            chunk_count: chunks.len(),
        })
    }

    pub fn cancel_preview(&self, preview_id: &str) -> Result<(), ImportError> {
        validate_id(preview_id)?;
        for ext in ["txt", "json"] {
            let path = self.preview_root.join(format!("{preview_id}.{ext}"));
            if path.exists() {
                fs::remove_file(path)?;
            }
        }
        Ok(())
    }
    pub fn documents(&self) -> Result<Vec<ImportedDocument>, ImportError> {
        let connection = Connection::open(&self.database_path)?;
        let mut s=connection.prepare("SELECT d.id,d.title,d.source_filename,d.source_sha256,d.byte_length,count(c.id) FROM user_documents d LEFT JOIN user_chunks c ON c.document_id=d.id WHERE d.import_status='ready' GROUP BY d.id ORDER BY d.created_at DESC,d.id")?;
        Ok(s.query_map([], |r| {
            Ok(ImportedDocument {
                id: r.get(0)?,
                title: r.get(1)?,
                source_filename: r.get(2)?,
                source_sha256: r.get(3)?,
                byte_length: r.get::<_, i64>(4)? as u64,
                chunk_count: r.get::<_, i64>(5)? as usize,
            })
        })?
        .collect::<Result<_, _>>()?)
    }
    pub fn delete_document(&self, id: &str) -> Result<bool, ImportError> {
        let mut connection = Connection::open(&self.database_path)?;
        connection.pragma_update(None, "foreign_keys", true)?;
        let transaction = connection.transaction()?;
        transaction.execute("DELETE FROM user_chunks_fts WHERE document_id=?1", [id])?;
        let deleted = transaction.execute("DELETE FROM user_documents WHERE id=?1", [id])? == 1;
        transaction.commit()?;
        Ok(deleted)
    }
    pub fn clear(&self) -> Result<usize, ImportError> {
        let mut connection = Connection::open(&self.database_path)?;
        connection.pragma_update(None, "foreign_keys", true)?;
        let transaction = connection.transaction()?;
        transaction.execute("DELETE FROM user_chunks_fts", [])?;
        let deleted = transaction.execute("DELETE FROM user_documents", [])?;
        transaction.commit()?;
        Ok(deleted)
    }
    pub fn rebuild_fts(&self) -> Result<(), ImportError> {
        let mut connection = Connection::open(&self.database_path)?;
        let transaction = connection.transaction()?;
        transaction.execute("DELETE FROM user_chunks_fts", [])?;
        transaction.execute("INSERT INTO user_chunks_fts(chunk_id,document_id,title,citation_label,text) SELECT c.id,c.document_id,d.title,c.citation_label,c.text_original FROM user_chunks c JOIN user_documents d ON d.id=c.document_id WHERE d.import_status='ready'",[])?;
        transaction.commit()?;
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
struct PreviewMetadata {
    source_filename: String,
    source_sha256: String,
    byte_length: u64,
}
fn validate_schema(path: &Path) -> Result<(), ImportError> {
    let c = Connection::open(path)?;
    let version = c
        .query_row("SELECT max(version) FROM schema_migrations", [], |r| {
            r.get::<_, Option<i64>>(0)
        })?
        .unwrap_or(0);
    if version != USER_SCHEMA_VERSION {
        return Err(DatabaseError::UnsupportedWritableSchema {
            database: "user",
            found: version,
            supported: USER_SCHEMA_VERSION,
        }
        .into());
    }
    Ok(())
}
fn validate_id(id: &str) -> Result<(), ImportError> {
    if id.is_empty() || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        return Err(ImportError::MissingPreview);
    }
    Ok(())
}

fn extract(extension: &str, bytes: &[u8]) -> Result<String, ImportError> {
    match extension {
        "txt" | "md" => String::from_utf8(bytes.to_vec()).map_err(|_| ImportError::ContentMismatch),
        "html" | "htm" => extract_html(bytes),
        "docx" => extract_docx(bytes),
        "pdf" => extract_pdf(bytes),
        _ => Err(ImportError::UnsupportedFormat),
    }
}
fn extract_html(bytes: &[u8]) -> Result<String, ImportError> {
    let raw = String::from_utf8(bytes.to_vec()).map_err(|_| ImportError::ContentMismatch)?;
    if !raw.to_ascii_lowercase().contains('<') {
        return Err(ImportError::ContentMismatch);
    }
    let mut active = raw;
    for element in ["script", "style", "noscript", "iframe", "object", "embed"] {
        let pattern = format!(r"(?is)<{element}[^>]*>.*?</{element}\s*>");
        active = Regex::new(&pattern)
            .unwrap()
            .replace_all(&active, " ")
            .into_owned();
    }
    let tags = Regex::new(r"(?s)<[^>]+>")
        .unwrap()
        .replace_all(&active, " ");
    Ok(decode_entities(&tags))
}
fn extract_docx(bytes: &[u8]) -> Result<String, ImportError> {
    if !bytes.starts_with(b"PK") {
        return Err(ImportError::ContentMismatch);
    }
    let mut archive =
        ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|_| ImportError::UnsafeDocx)?;
    for index in 0..archive.len() {
        let name = archive
            .by_index(index)
            .map_err(|_| ImportError::UnsafeDocx)?
            .name()
            .to_ascii_lowercase();
        if name.ends_with("vbaproject.bin") || name.contains("/embeddings/") {
            return Err(ImportError::UnsafeDocx);
        }
    }
    let mut xml = String::new();
    archive
        .by_name("word/document.xml")
        .map_err(|_| ImportError::UnsafeDocx)?
        .read_to_string(&mut xml)
        .map_err(|_| ImportError::UnsafeDocx)?;
    if xml.len() > MAX_EXTRACTED_BYTES {
        return Err(ImportError::TooLarge);
    }
    let breaks = Regex::new(r"(?i)</w:(p|tr)>|<w:(br|tab)[^>]*/>")
        .unwrap()
        .replace_all(&xml, "\n");
    let tags = Regex::new(r"(?s)<[^>]+>").unwrap().replace_all(&breaks, "");
    Ok(decode_entities(&tags))
}
fn extract_pdf(bytes: &[u8]) -> Result<String, ImportError> {
    if !bytes.starts_with(b"%PDF-") {
        return Err(ImportError::ContentMismatch);
    }
    let raw = String::from_utf8_lossy(bytes);
    let re = Regex::new(r"(?s)\(([^()]*(?:\\.[^()]*)*)\)\s*T[jJ]").unwrap();
    let mut out = String::new();
    for capture in re.captures_iter(&raw) {
        out.push_str(
            &capture[1]
                .replace("\\(", "(")
                .replace("\\)", ")")
                .replace("\\n", "\n"),
        );
        out.push('\n')
    }
    if out.trim().is_empty() {
        return Err(ImportError::ScannedPdf);
    }
    Ok(out)
}
fn decode_entities(value: &str) -> String {
    value
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
}
fn normalize_text(value: &str) -> String {
    value
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .lines()
        .map(str::trim)
        .fold(String::new(), |mut out, line| {
            if line.is_empty() {
                if !out.ends_with("\n\n") {
                    out.push_str("\n\n")
                }
            } else {
                out.push_str(line);
                out.push('\n')
            }
            out
        })
        .trim()
        .to_owned()
}
fn normalize_for_search(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}
fn infer_title(text: &str, path: &Path) -> String {
    text.lines()
        .find(|v| !v.trim().is_empty())
        .map(|v| {
            v.trim()
                .trim_start_matches('#')
                .trim()
                .chars()
                .take(120)
                .collect()
        })
        .filter(|v: &String| !v.is_empty())
        .unwrap_or_else(|| {
            path.file_stem()
                .and_then(|v| v.to_str())
                .unwrap_or("Untitled")
                .to_owned()
        })
}
fn infer_language(text: &str) -> String {
    let chinese = text
        .chars()
        .filter(|c| matches!(*c, '\u{4e00}'..='\u{9fff}'))
        .count();
    if chinese > text.chars().count() / 10 {
        "zh".into()
    } else {
        "en".into()
    }
}
fn chunk_text(text: &str) -> Vec<ImportChunk> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    for paragraph in text.split("\n\n").map(str::trim).filter(|v| !v.is_empty()) {
        if !current.is_empty() && current.chars().count() + paragraph.chars().count() > 1200 {
            let sequence = chunks.len() + 1;
            chunks.push(ImportChunk {
                sequence_number: sequence,
                citation_label: format!("§ {sequence}"),
                text: std::mem::take(&mut current),
            });
        }
        if !current.is_empty() {
            current.push_str("\n\n")
        }
        current.push_str(paragraph)
    }
    if !current.trim().is_empty() {
        let sequence = chunks.len() + 1;
        chunks.push(ImportChunk {
            sequence_number: sequence,
            citation_label: format!("§ {sequence}"),
            text: current,
        });
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ApplicationDatabasePaths;
    fn library() -> (PathBuf, UserLibrary) {
        let root = std::env::temp_dir().join(format!("ilia-import-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let core = root.join("core.sqlite");
        let c = Connection::open(&core).unwrap();
        c.execute_batch("CREATE TABLE schema_metadata(schema_version TEXT PRIMARY KEY,applied_at TEXT);INSERT INTO schema_metadata VALUES('001',CURRENT_TIMESTAMP);").unwrap();
        drop(c);
        let p = ApplicationDatabasePaths::initialize(core, root.join("data")).unwrap();
        let l = UserLibrary::open(p.user, root.join("data")).unwrap();
        (root, l)
    }
    #[test]
    fn imports_transactionally_deduplicates_deletes_and_rebuilds() {
        let (root, library) = library();
        let source = root.join("memo.md");
        fs::write(
            &source,
            "# Maritime memo\n\nCustom evidence about navigation.",
        )
        .unwrap();
        let preview = library.prepare_import(&source).unwrap();
        let chunks = library.preview_chunks(&preview.preview_id).unwrap();
        let saved = library
            .commit_import(CommitImport {
                preview_id: preview.preview_id,
                title: preview.inferred_title,
                language: "en".into(),
                document_type: "commentary".into(),
                model_id: "test".into(),
                embeddings: vec![vec![1.0, 0.0]; chunks.len()],
            })
            .unwrap();
        assert!(matches!(
            library.prepare_import(&source),
            Err(ImportError::Duplicate(_))
        ));
        library.rebuild_fts().unwrap();
        assert!(library.delete_document(&saved.id).unwrap());
        let c = Connection::open(&library.database_path).unwrap();
        for table in [
            "user_documents",
            "user_chunks",
            "user_embeddings",
            "user_chunks_fts",
        ] {
            let count: i64 = c
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))
                .unwrap();
            assert_eq!(count, 0, "{table}");
        }
        drop(c);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn sanitizes_html_and_rejects_scanned_pdf() {
        let html = extract_html(
            b"<html><script>alert(1)</script><style>x</style><p>Safe &amp; useful</p></html>",
        )
        .unwrap();
        assert_eq!(normalize_text(&html), "Safe & useful");
        assert!(matches!(
            extract_pdf(b"%PDF-1.7\nimage only"),
            Err(ImportError::ScannedPdf)
        ));
    }
    #[test]
    fn extracts_minimal_text_pdf() {
        let text = extract_pdf(b"%PDF-1.4\nBT (Article 3 navigation) Tj ET").unwrap();
        assert!(text.contains("Article 3"));
    }

    #[test]
    fn extracts_all_five_supported_formats_without_active_docx_content() {
        assert_eq!(extract("txt", b"plain text").unwrap(), "plain text");
        assert_eq!(extract("md", b"# heading").unwrap(), "# heading");
        assert!(extract("html", b"<p>safe</p>").unwrap().contains("safe"));
        let cursor = std::io::Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(cursor);
        let options = zip::write::SimpleFileOptions::default();
        writer.start_file("word/document.xml", options).unwrap();
        use std::io::Write as _;
        writer.write_all(br#"<w:document><w:body><w:p><w:r><w:t>DOCX safe text</w:t></w:r></w:p></w:body></w:document>"#).unwrap();
        let bytes = writer.finish().unwrap().into_inner();
        assert!(extract("docx", &bytes).unwrap().contains("DOCX safe text"));
        let cursor = std::io::Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(cursor);
        writer.start_file("word/document.xml", options).unwrap();
        writer.write_all(b"<w:t>unsafe</w:t>").unwrap();
        writer.start_file("word/vbaProject.bin", options).unwrap();
        writer.write_all(b"macro").unwrap();
        let bytes = writer.finish().unwrap().into_inner();
        assert!(matches!(
            extract("docx", &bytes),
            Err(ImportError::UnsafeDocx)
        ));
    }
}
