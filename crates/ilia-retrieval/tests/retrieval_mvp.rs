use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};

use ilia_core::{
    DocumentType, LibraryKind, LibraryScope, MatchKind, SearchFilters, SearchOptions, SearchRequest,
};
use ilia_database::{ApplicationDatabasePaths, Database, UserDatabase};
use ilia_embedding::BGE_M3_MODEL_ID;
use ilia_retrieval::{RetrievalService, TopicRegistry};
use rusqlite::{Connection, params};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(1);

fn core_database_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("data")
        .join("ilia.sqlite3")
}

fn topic_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("corpus")
        .join("manifests")
        .join("document_topics.v1.json")
}

fn service() -> RetrievalService {
    RetrievalService::new(Database::open_read_only(core_database_path()).unwrap())
        .with_topic_registry(TopicRegistry::from_path(topic_path()).unwrap())
}

fn request(query: &str, filters: SearchFilters) -> SearchRequest {
    SearchRequest {
        query: query.to_owned(),
        filters,
        limit: 10,
        evidence_limit: 5,
    }
}

#[test]
fn resolves_english_article_reference() {
    let result = service()
        .search("VCLT Article 27", SearchOptions::default())
        .unwrap();
    assert_eq!(result.detected_document_id.as_deref(), Some("vclt-1969"));
    assert_eq!(result.hits[0].citation_label, "VCLT, Article 27");
    assert_eq!(result.hits[0].match_kind, MatchKind::ExactArticle);
}

#[test]
fn resolves_chinese_article_reference() {
    let result = service()
        .search("联合国宪章第51条", SearchOptions::default())
        .unwrap();
    assert_eq!(result.hits[0].document_id, "un-charter-1945");
    assert_eq!(result.hits[0].citation_label, "UN Charter, Article 51");
}

#[test]
fn resolves_case_paragraph() {
    let result = service()
        .search("Nicaragua paragraph 191", SearchOptions::default())
        .unwrap();
    assert_eq!(result.hits[0].document_id, "icj-nicaragua-1986-merits");
    assert_eq!(
        result.hits[0].citation_label,
        "Nicaragua v. United States (Merits), para. 191"
    );
}

#[test]
fn full_text_search_keeps_legal_type() {
    let result = service()
        .search("ARSIWA directed or controlled", SearchOptions::default())
        .unwrap();
    assert!(
        result
            .hits
            .iter()
            .any(|hit| hit.citation_label == "ARSIWA, Article 8")
    );
    assert!(
        result
            .hits
            .iter()
            .all(|hit| hit.document_type == "draft_articles")
    );
}

#[test]
fn resolves_document_name_without_a_locator() {
    let result = service()
        .search("联合国海洋法公约", SearchOptions::default())
        .unwrap();
    assert!(result.hits.is_empty());
    assert_eq!(result.documents[0].document_id, "unclos-1982");
    assert_eq!(result.documents[0].document_type, "treaty");
}

#[test]
fn resolves_roman_article_reference() {
    let result = service()
        .search("UNCLOS Art. III", SearchOptions::default())
        .unwrap();
    assert_eq!(result.hits[0].document_id, "unclos-1982");
    assert_eq!(result.hits[0].citation_label, "UNCLOS, Article 3");
}

#[test]
fn document_type_filter_applies_to_exact_and_fts_paths() {
    let treaty_only = SearchFilters {
        library: LibraryScope::CoreOnly,
        document_types: vec![DocumentType::Treaty],
        ..SearchFilters::default()
    };
    let exact = service()
        .search_request(
            &request("Nicaragua Article 3", treaty_only.clone()),
            SearchOptions::default(),
        )
        .unwrap();
    assert!(exact.hits.iter().all(|hit| hit.document_type == "treaty"));
    assert!(
        exact
            .hits
            .iter()
            .all(|hit| hit.document_id != "icj-nicaragua-1986-merits")
    );

    let fts = service()
        .search_request(
            &request("directed or controlled", treaty_only),
            SearchOptions::default(),
        )
        .unwrap();
    assert!(fts.hits.iter().all(|hit| hit.document_type == "treaty"));
    assert!(fts.hits.iter().all(|hit| hit.document_id != "arsiwa-2001"));
}

#[test]
fn document_and_topic_filters_apply_inside_vector_path() {
    let connection = Connection::open(core_database_path()).unwrap();
    let bytes: Vec<u8> = connection
        .query_row(
            "SELECT vector FROM chunk_embeddings WHERE model_id = ?1 AND chunk_id = 'arsiwa-2001-art-8-en'",
            [BGE_M3_MODEL_ID],
            |row| row.get(0),
        )
        .unwrap();
    let (chunks, remainder) = bytes.as_chunks::<4>();
    assert!(remainder.is_empty());
    let vector = chunks
        .iter()
        .map(|chunk| f32::from_le_bytes(*chunk))
        .collect::<Vec<_>>();

    let filters = SearchFilters {
        library: LibraryScope::CoreOnly,
        document_types: Vec::new(),
        topic_ids: vec!["law_of_the_sea".to_owned()],
        document_keys: Vec::new(),
    };
    let result = service()
        .search_hybrid_request(
            &request("directed or controlled", filters),
            &vector,
            BGE_M3_MODEL_ID,
            SearchOptions::default(),
        )
        .unwrap();
    let allowed = [
        "unclos-1982",
        "icj-north-sea-1969",
        "icj-continental-shelf-tunisia-libya-1982",
    ];
    assert!(!result.hits.is_empty());
    assert!(
        result
            .hits
            .iter()
            .all(|hit| allowed.contains(&hit.document_id.as_str()))
    );
}

#[test]
fn explicit_document_filter_cannot_leak_other_documents() {
    let filters = SearchFilters {
        library: LibraryScope::CoreOnly,
        document_keys: vec!["core:unclos-1982".to_owned()],
        ..SearchFilters::default()
    };
    let result = service()
        .search_request(&request("Article 3", filters), SearchOptions::default())
        .unwrap();
    assert!(!result.hits.is_empty());
    assert!(
        result
            .hits
            .iter()
            .all(|hit| hit.document_id == "unclos-1982")
    );
}

#[test]
fn broad_search_enforces_per_document_quota() {
    let options = SearchOptions {
        limit: 10,
        per_document_limit: 2,
        ..SearchOptions::default()
    };
    let result = service().search("international law", options).unwrap();
    let mut counts = HashMap::new();
    for hit in result.hits {
        *counts.entry(hit.document_id).or_insert(0usize) += 1;
    }
    assert!(counts.values().all(|count| *count <= 2));
}

fn temporary_directory() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "ilia-retrieval-test-{}-{}",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&path).unwrap();
    path
}

fn seed_user_database(path: &Path) {
    let connection = Connection::open(path).unwrap();
    connection
        .execute(
            "INSERT INTO user_documents(id, title, language, document_type, source_filename, source_sha256, byte_length, import_status) VALUES (?1, ?2, 'en', 'commentary', 'memo.txt', ?3, 100, 'ready')",
            params!["memo-1", "Custom Maritime Memorandum", "a".repeat(64)],
        )
        .unwrap();
    for (id, sequence) in [("memo-chunk-1", 1), ("memo-chunk-2", 2)] {
        connection
            .execute(
                "INSERT INTO user_chunks(id, document_id, sequence_number, citation_label, language, text_original, text_normalized) VALUES (?1, 'memo-1', ?2, ?3, 'en', 'custom maritime memorandum evidence', 'custom maritime memorandum evidence')",
                params![id, sequence, format!("Custom memo, section {sequence}")],
            )
            .unwrap();
        connection
            .execute(
                "INSERT INTO user_chunks_fts(chunk_id, document_id, title, citation_label, text) VALUES (?1, 'memo-1', 'Custom Maritime Memorandum', ?2, 'custom maritime memorandum evidence')",
                params![id, format!("Custom memo, section {sequence}")],
            )
            .unwrap();
    }
}

#[test]
fn federates_user_library_and_compresses_duplicate_text() {
    let temp = temporary_directory();
    let paths = ApplicationDatabasePaths::initialize(core_database_path(), &temp).unwrap();
    seed_user_database(&paths.user);
    {
        let retrieval = RetrievalService::new(Database::open_read_only(&paths.core).unwrap())
            .with_user_database(UserDatabase::open_read_only(&paths.user).unwrap())
            .with_topic_registry(TopicRegistry::from_path(topic_path()).unwrap());
        let filters = SearchFilters {
            library: LibraryScope::UserOnly,
            ..SearchFilters::default()
        };
        let result = retrieval
            .search_request(
                &request("custom maritime memorandum evidence", filters),
                SearchOptions::default(),
            )
            .unwrap();
        assert_eq!(result.hits.len(), 1);
        assert_eq!(result.hits[0].library_kind, LibraryKind::User);
        assert_eq!(
            result.hits[0].stable_key.to_string(),
            "user:memo-1:memo-chunk-1"
        );
    }
    fs::remove_dir_all(temp).unwrap();
}
