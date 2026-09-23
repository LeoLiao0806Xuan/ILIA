PRAGMA foreign_keys = ON;

CREATE TABLE schema_metadata (
    schema_version TEXT PRIMARY KEY,
    applied_at TEXT NOT NULL
);

CREATE TABLE documents (
    id TEXT PRIMARY KEY,
    canonical_title TEXT NOT NULL,
    title_zh TEXT,
    short_title TEXT,
    document_type TEXT NOT NULL CHECK (document_type IN (
        'treaty', 'judgment', 'advisory_opinion', 'order', 'resolution',
        'draft_articles', 'customary_rule', 'commentary', 'declaration', 'statute'
    )),
    issuing_body TEXT NOT NULL,
    adoption_date TEXT,
    entry_into_force_date TEXT,
    legal_status TEXT NOT NULL,
    official_source_url TEXT NOT NULL,
    database_version TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE document_aliases (
    id INTEGER PRIMARY KEY,
    document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    language TEXT NOT NULL,
    alias TEXT NOT NULL,
    alias_kind TEXT NOT NULL DEFAULT 'title',
    UNIQUE (document_id, language, alias)
);

CREATE TABLE source_files (
    id INTEGER PRIMARY KEY,
    document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    source_authority TEXT NOT NULL,
    official_source_url TEXT NOT NULL,
    acquisition_url TEXT,
    source_accessed_at TEXT NOT NULL,
    local_file_path TEXT NOT NULL,
    mime_type TEXT NOT NULL,
    byte_length INTEGER NOT NULL CHECK (byte_length > 0),
    sha256 TEXT NOT NULL CHECK (length(sha256) = 64),
    page_count INTEGER,
    license_review TEXT NOT NULL CHECK (license_review IN ('pending', 'cleared', 'restricted', 'blocked')),
    UNIQUE (document_id, sha256)
);

CREATE TABLE document_versions (
    id INTEGER PRIMARY KEY,
    document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    source_file_id INTEGER NOT NULL REFERENCES source_files(id) ON DELETE RESTRICT,
    language TEXT NOT NULL,
    is_official_text INTEGER NOT NULL CHECK (is_official_text IN (0, 1)),
    translation_type TEXT NOT NULL CHECK (translation_type IN (
        'authentic_text', 'official_translation', 'human_translation', 'machine_translation', 'none'
    )),
    version_label TEXT NOT NULL,
    valid_from TEXT,
    valid_to TEXT,
    UNIQUE (document_id, language, version_label)
);

CREATE TABLE source_pages (
    id INTEGER PRIMARY KEY,
    document_version_id INTEGER NOT NULL REFERENCES document_versions(id) ON DELETE CASCADE,
    pdf_page INTEGER NOT NULL CHECK (pdf_page > 0),
    printed_page TEXT,
    raw_text TEXT NOT NULL,
    normalized_text TEXT NOT NULL,
    extraction_method TEXT NOT NULL,
    review_status TEXT NOT NULL CHECK (review_status IN ('unreviewed', 'auto_pass', 'needs_review', 'needs_second_review', 'approved')),
    UNIQUE (document_version_id, pdf_page)
);

CREATE TABLE provisions (
    id INTEGER PRIMARY KEY,
    document_version_id INTEGER NOT NULL REFERENCES document_versions(id) ON DELETE CASCADE,
    part_number TEXT,
    chapter_number TEXT,
    article_number TEXT NOT NULL,
    paragraph_number TEXT,
    subparagraph TEXT,
    heading TEXT,
    text_original TEXT NOT NULL,
    text_normalized TEXT NOT NULL,
    page_start INTEGER NOT NULL,
    page_end INTEGER NOT NULL,
    citation_label TEXT NOT NULL,
    UNIQUE (document_version_id, article_number, paragraph_number, subparagraph),
    UNIQUE (document_version_id, citation_label)
);

CREATE TABLE cases (
    id TEXT PRIMARY KEY,
    document_id TEXT NOT NULL UNIQUE REFERENCES documents(id) ON DELETE CASCADE,
    case_number TEXT,
    case_name TEXT NOT NULL,
    decision_date TEXT NOT NULL,
    decision_kind TEXT NOT NULL CHECK (decision_kind IN ('judgment', 'advisory_opinion', 'order')),
    applicant TEXT,
    respondent TEXT,
    official_report_citation TEXT
);

CREATE TABLE case_paragraphs (
    id INTEGER PRIMARY KEY,
    case_id TEXT NOT NULL REFERENCES cases(id) ON DELETE CASCADE,
    document_version_id INTEGER NOT NULL REFERENCES document_versions(id) ON DELETE CASCADE,
    document_kind TEXT NOT NULL,
    paragraph_number INTEGER NOT NULL CHECK (paragraph_number > 0),
    page_start INTEGER NOT NULL,
    page_end INTEGER NOT NULL,
    language TEXT NOT NULL,
    text_original TEXT NOT NULL,
    text_normalized TEXT NOT NULL,
    citation_label TEXT NOT NULL,
    source_url TEXT NOT NULL,
    UNIQUE (case_id, document_version_id, paragraph_number),
    UNIQUE (document_version_id, citation_label)
);

CREATE TABLE case_passages (
    id INTEGER PRIMARY KEY,
    case_id TEXT NOT NULL REFERENCES cases(id) ON DELETE CASCADE,
    document_version_id INTEGER NOT NULL REFERENCES document_versions(id) ON DELETE CASCADE,
    passage_number INTEGER NOT NULL CHECK (passage_number > 0),
    page_start INTEGER NOT NULL,
    page_end INTEGER NOT NULL,
    language TEXT NOT NULL,
    text_original TEXT NOT NULL,
    text_normalized TEXT NOT NULL,
    citation_label TEXT NOT NULL,
    source_url TEXT NOT NULL,
    UNIQUE (case_id, document_version_id, passage_number),
    UNIQUE (document_version_id, citation_label)
);

CREATE TABLE chunks (
    id TEXT PRIMARY KEY,
    document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    document_version_id INTEGER NOT NULL REFERENCES document_versions(id) ON DELETE CASCADE,
    locator_type TEXT NOT NULL CHECK (locator_type IN ('article', 'case_paragraph', 'case_passage')),
    locator_id INTEGER NOT NULL,
    citation_label TEXT NOT NULL,
    page_start INTEGER NOT NULL,
    page_end INTEGER NOT NULL,
    language TEXT NOT NULL,
    text_original TEXT NOT NULL,
    text_normalized TEXT NOT NULL,
    token_count_estimate INTEGER NOT NULL,
    UNIQUE (document_version_id, citation_label)
);

CREATE VIRTUAL TABLE chunks_fts USING fts5(
    chunk_id UNINDEXED,
    document_id UNINDEXED,
    canonical_title,
    citation_label,
    text,
    tokenize = 'unicode61 remove_diacritics 2'
);

CREATE TABLE embedding_models (
    id TEXT PRIMARY KEY,
    dimension INTEGER NOT NULL CHECK (dimension > 0),
    vector_format TEXT NOT NULL CHECK (vector_format = 'f32le'),
    normalized INTEGER NOT NULL CHECK (normalized IN (0, 1)),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE chunk_embeddings (
    chunk_id TEXT NOT NULL REFERENCES chunks(id) ON DELETE CASCADE,
    model_id TEXT NOT NULL REFERENCES embedding_models(id) ON DELETE CASCADE,
    vector BLOB NOT NULL,
    vector_norm REAL NOT NULL CHECK (vector_norm > 0),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (chunk_id, model_id)
);

CREATE INDEX idx_chunk_embeddings_model ON chunk_embeddings(model_id, chunk_id);

CREATE TABLE treaty_parties (
    treaty_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    state_code TEXT NOT NULL,
    action_type TEXT NOT NULL,
    signature_date TEXT,
    ratification_date TEXT,
    accession_date TEXT,
    effective_date TEXT,
    withdrawal_date TEXT,
    status_checked_at TEXT NOT NULL,
    source_url TEXT NOT NULL,
    PRIMARY KEY (treaty_id, state_code, action_type)
);

CREATE TABLE treaty_statements (
    id INTEGER PRIMARY KEY,
    treaty_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    state_code TEXT NOT NULL,
    statement_type TEXT NOT NULL CHECK (statement_type IN ('reservation', 'declaration', 'objection')),
    made_at TEXT,
    withdrawn_at TEXT,
    text_original TEXT NOT NULL,
    language TEXT NOT NULL,
    status_checked_at TEXT NOT NULL,
    source_url TEXT NOT NULL
);

CREATE TABLE protocol_relations (
    parent_document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    protocol_document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    relation_type TEXT NOT NULL,
    PRIMARY KEY (parent_document_id, protocol_document_id, relation_type)
);

CREATE TABLE import_runs (
    id INTEGER PRIMARY KEY,
    database_version TEXT NOT NULL,
    started_at TEXT NOT NULL,
    completed_at TEXT,
    status TEXT NOT NULL CHECK (status IN ('running', 'passed', 'failed')),
    manifest_sha256 TEXT NOT NULL,
    importer_version TEXT NOT NULL,
    notes TEXT
);

CREATE TABLE qa_findings (
    id INTEGER PRIMARY KEY,
    import_run_id INTEGER NOT NULL REFERENCES import_runs(id) ON DELETE CASCADE,
    document_id TEXT REFERENCES documents(id) ON DELETE CASCADE,
    severity TEXT NOT NULL CHECK (severity IN ('info', 'warning', 'error')),
    check_code TEXT NOT NULL,
    message TEXT NOT NULL,
    resolved INTEGER NOT NULL DEFAULT 0 CHECK (resolved IN (0, 1))
);

CREATE INDEX idx_source_pages_version_page ON source_pages(document_version_id, pdf_page);
CREATE INDEX idx_provisions_article ON provisions(document_version_id, article_number);
CREATE INDEX idx_case_paragraphs_number ON case_paragraphs(case_id, paragraph_number);
CREATE INDEX idx_case_passages_number ON case_passages(case_id, passage_number);
CREATE INDEX idx_chunks_document ON chunks(document_id, language);
CREATE INDEX idx_treaty_parties_state ON treaty_parties(state_code, treaty_id);
