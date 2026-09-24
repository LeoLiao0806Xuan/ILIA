use std::path::PathBuf;

use ilia_core::{MatchKind, SearchOptions};
use ilia_database::Database;
use ilia_retrieval::RetrievalService;

fn service() -> RetrievalService {
    let database = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("data")
        .join("ilia.sqlite3");
    RetrievalService::new(Database::open_read_only(database).unwrap())
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
