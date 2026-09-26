use std::{collections::BTreeMap, env, fs, io::BufRead, path::PathBuf, process::ExitCode};

use ilia_core::{DocumentType, LibraryScope, SearchFilters, SearchOptions, SearchRequest};
use ilia_database::Database;
use ilia_embedding::{BGE_M3_MODEL_ID, BgeM3Embedder};
use ilia_retrieval::{RetrievalService, TopicRegistry};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
struct EvalCase {
    id: String,
    query: String,
    #[serde(default)]
    expected_document_id: Option<String>,
    #[serde(default)]
    expected_citation_label: Option<String>,
    #[serde(default)]
    expected_document_type: Option<String>,
    #[serde(default)]
    expect_no_results: bool,
    #[serde(default)]
    filters: SearchFilters,
    tags: Vec<String>,
}

#[derive(Debug, Serialize, Default)]
struct TagMetrics {
    total: usize,
    passed: usize,
}

#[derive(Debug, Serialize)]
struct CaseResult {
    id: String,
    query: String,
    passed: bool,
    reasons: Vec<String>,
    returned_citations: Vec<String>,
    tags: Vec<String>,
    relevant_rank: Option<usize>,
    returned_document_ids: Vec<String>,
    filter_violation: bool,
    #[serde(skip_serializing)]
    has_expected_citation: bool,
}

#[derive(Debug, Serialize)]
struct EvalReport {
    total: usize,
    passed: usize,
    failed: usize,
    pass_rate: f64,
    recall_at_1: f64,
    recall_at_5: f64,
    recall_at_10: f64,
    mean_reciprocal_rank: f64,
    exact_locator_accuracy: f64,
    bilingual_document_name_accuracy: f64,
    filter_case_pass_rate: f64,
    filter_escape_count: usize,
    by_tag: BTreeMap<String, TagMetrics>,
    cases: Vec<CaseResult>,
}

fn main() -> ExitCode {
    match run() {
        Ok(0) => ExitCode::SUCCESS,
        Ok(_) => ExitCode::from(1),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<usize, Box<dyn std::error::Error>> {
    let mut args = env::args().skip(1);
    let mut database_path: Option<PathBuf> = None;
    let mut cases_path: Option<PathBuf> = None;
    let mut output_path: Option<PathBuf> = None;
    let mut model_cache: Option<PathBuf> = None;
    let mut topics_path: Option<PathBuf> = None;
    let mut minimum_cases = 0usize;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--db" => database_path = args.next().map(PathBuf::from),
            "--cases" => cases_path = args.next().map(PathBuf::from),
            "--output" => output_path = args.next().map(PathBuf::from),
            "--model-cache" => model_cache = args.next().map(PathBuf::from),
            "--topics" => topics_path = args.next().map(PathBuf::from),
            "--minimum-cases" => {
                minimum_cases = args
                    .next()
                    .ok_or("missing --minimum-cases value")?
                    .parse()?
            }
            _ => return Err(format!("unknown argument: {argument}").into()),
        }
    }
    let database_path = database_path.ok_or("missing --db")?;
    let cases_path = cases_path.ok_or("missing --cases")?;
    let file = fs::File::open(cases_path)?;
    let cases = std::io::BufReader::new(file)
        .lines()
        .filter(|line| {
            line.as_ref()
                .map(|value| !value.trim().is_empty())
                .unwrap_or(true)
        })
        .map(|line| Ok(serde_json::from_str::<EvalCase>(&line?)?))
        .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;
    if cases.len() < minimum_cases {
        return Err(format!(
            "evaluation suite contains {} cases; minimum is {minimum_cases}",
            cases.len()
        )
        .into());
    }
    let mut service = RetrievalService::new(Database::open_read_only(database_path)?);
    if let Some(path) = topics_path {
        service = service.with_topic_registry(TopicRegistry::from_path(path)?);
    }
    let mut embedder = model_cache
        .map(|path| BgeM3Embedder::new(path, false))
        .transpose()?;
    let mut results = Vec::with_capacity(cases.len());

    for case in cases {
        let options = SearchOptions {
            limit: 10,
            ..SearchOptions::default()
        };
        let use_hybrid = case
            .tags
            .iter()
            .any(|tag| tag == "semantic" || tag == "hybrid");
        let request = SearchRequest {
            query: case.query.clone(),
            filters: case.filters.clone(),
            limit: options.limit,
            evidence_limit: options.evidence_limit,
        };
        let response = if use_hybrid {
            let embedder = embedder
                .as_mut()
                .ok_or("semantic case requires --model-cache")?;
            let vector = embedder.embed_query(&case.query)?;
            service.search_hybrid_request(&request, &vector, BGE_M3_MODEL_ID, options)?
        } else {
            service.search_request(&request, options)?
        };
        let mut reasons = Vec::new();
        if case.expect_no_results {
            if !response.hits.is_empty() {
                reasons.push(format!(
                    "expected no results, received {}",
                    response.hits.len()
                ));
            }
        } else {
            if let Some(expected) = &case.expected_document_id
                && !response.hits.iter().any(|hit| &hit.document_id == expected)
                && !response
                    .documents
                    .iter()
                    .any(|document| &document.document_id == expected)
            {
                reasons.push(format!("missing document {expected}"));
            }
            if let Some(expected) = &case.expected_citation_label
                && !response
                    .hits
                    .iter()
                    .any(|hit| &hit.citation_label == expected)
            {
                reasons.push(format!("missing citation {expected}"));
            }
            if let Some(expected) = &case.expected_document_type
                && !response
                    .hits
                    .iter()
                    .any(|hit| &hit.document_type == expected)
                && !response
                    .documents
                    .iter()
                    .any(|document| &document.document_type == expected)
            {
                reasons.push(format!("missing document type {expected}"));
            }
        }
        let relevant_rank = case
            .expected_citation_label
            .as_ref()
            .and_then(|expected| {
                response
                    .hits
                    .iter()
                    .position(|hit| &hit.citation_label == expected)
                    .map(|index| index + 1)
            })
            .or_else(|| {
                case.expected_document_id.as_ref().and_then(|expected| {
                    response
                        .hits
                        .iter()
                        .position(|hit| &hit.document_id == expected)
                        .map(|index| index + 1)
                        .or_else(|| {
                            response
                                .documents
                                .iter()
                                .any(|item| &item.document_id == expected)
                                .then_some(1)
                        })
                })
            });
        let filter_violation = response
            .hits
            .iter()
            .any(|hit| violates_filter(hit, &case.filters));
        if filter_violation {
            reasons.push("result escaped an explicit library, type, or document filter".to_owned());
        }
        results.push(CaseResult {
            id: case.id,
            query: case.query,
            passed: reasons.is_empty(),
            reasons,
            returned_citations: response
                .hits
                .iter()
                .map(|hit| hit.citation_label.clone())
                .collect(),
            tags: case.tags,
            relevant_rank,
            returned_document_ids: response
                .hits
                .iter()
                .map(|hit| hit.document_id.clone())
                .collect(),
            filter_violation,
            has_expected_citation: case.expected_citation_label.is_some()
                || case.expected_document_id.is_some(),
        });
    }

    let passed = results.iter().filter(|result| result.passed).count();
    let total = results.len();
    let ranked = results
        .iter()
        .filter(|result| result.has_expected_citation)
        .collect::<Vec<_>>();
    let ranked_total = ranked.len().max(1) as f64;
    let recall = |cutoff: usize| {
        ranked
            .iter()
            .filter(|result| result.relevant_rank.is_some_and(|rank| rank <= cutoff))
            .count() as f64
            / ranked_total
    };
    let mean_reciprocal_rank = ranked
        .iter()
        .filter_map(|result| result.relevant_rank)
        .map(|rank| 1.0 / rank as f64)
        .sum::<f64>()
        / ranked_total;
    let tagged_accuracy = |tag: &str| {
        let selected = results
            .iter()
            .filter(|result| result.tags.iter().any(|item| item == tag))
            .collect::<Vec<_>>();
        if selected.is_empty() {
            0.0
        } else {
            selected.iter().filter(|result| result.passed).count() as f64 / selected.len() as f64
        }
    };
    let filter_escape_count = results
        .iter()
        .filter(|result| result.filter_violation)
        .count();
    let mut by_tag = BTreeMap::<String, TagMetrics>::new();
    for result in &results {
        for tag in &result.tags {
            let metrics = by_tag.entry(tag.clone()).or_default();
            metrics.total += 1;
            metrics.passed += usize::from(result.passed);
        }
    }
    let report = EvalReport {
        total,
        passed,
        failed: total - passed,
        pass_rate: if total == 0 {
            0.0
        } else {
            passed as f64 / total as f64
        },
        recall_at_1: recall(1),
        recall_at_5: recall(5),
        recall_at_10: recall(10),
        mean_reciprocal_rank,
        exact_locator_accuracy: tagged_accuracy("exact_locator"),
        bilingual_document_name_accuracy: tagged_accuracy("document_name"),
        filter_case_pass_rate: tagged_accuracy("filter"),
        filter_escape_count,
        by_tag,
        cases: results,
    };
    let json = serde_json::to_string_pretty(&report)? + "\n";
    if let Some(output_path) = output_path {
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(output_path, &json)?;
    }
    println!("{json}");
    Ok(report.failed)
}

fn violates_filter(hit: &ilia_core::SearchHit, filters: &SearchFilters) -> bool {
    if filters.library == LibraryScope::CoreOnly && hit.library_kind != ilia_core::LibraryKind::Core
        || filters.library == LibraryScope::UserOnly
            && hit.library_kind != ilia_core::LibraryKind::User
    {
        return true;
    }
    if !filters.document_types.is_empty()
        && !filters
            .document_types
            .iter()
            .any(|kind| document_type_name(kind) == hit.document_type)
    {
        return true;
    }
    if !filters.document_keys.is_empty()
        && !filters.document_keys.iter().any(|key| {
            key == &hit.document_id
                || key
                    == &format!(
                        "{}:{}",
                        match hit.library_kind {
                            ilia_core::LibraryKind::Core => "core",
                            ilia_core::LibraryKind::User => "user",
                        },
                        hit.document_id
                    )
        })
    {
        return true;
    }
    false
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
