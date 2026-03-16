use std::path::Path;
use std::time::Instant;

use super::matcher::{
    collect_matched_candidates, prepare_final_review_candidates, select_screening_candidates,
    summarize_matches, FINAL_REVIEW_CANDIDATE_LIMIT, MAX_VERIFY_CANDIDATES,
};
use super::types::{Candidate, MatchSummary};
use super::vlm::{normalize_match_ids, ReferenceImages, VlmBatchRequest, VlmCallTrace, VlmClient};

const FINAL_REVIEW_BATCH_SIZE: usize = 4;

pub trait CandidateFetcher {
    fn fetch_candidates(&self, image_path: &Path) -> Result<Vec<Candidate>, String>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoMatchReason {
    NoCandidates,
    InitialScreenNoMatch,
    FinalReviewRejected,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct CandidateProcessDiagnostics {
    had_candidates: bool,
    had_initial_matches: bool,
    final_review_rejected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CandidateProcessResult {
    summary: MatchSummary,
    diagnostics: CandidateProcessDiagnostics,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrchestrationResult {
    pub summary: MatchSummary,
    pub used_fallback_image: bool,
    pub no_match_reason: Option<NoMatchReason>,
    pub diagnostics: OrchestrationDiagnostics,
}

#[derive(Debug, Clone, Copy)]
pub struct SearchPass<'a> {
    pub image_path: &'a Path,
    pub reference_image_base64: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VlmCallStage {
    Screening,
    FinalReview,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedVlmCall {
    pub pass_label: String,
    pub stage: VlmCallStage,
    pub chunk_index: usize,
    pub match_ids: Vec<usize>,
    pub trace: VlmCallTrace,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OrchestrationDiagnostics {
    pub primary_candidates: Vec<Candidate>,
    pub fallback_candidates: Vec<Candidate>,
    pub vlm_calls: Vec<RecordedVlmCall>,
    pub screening_candidate_count: usize,
    pub screening_chunk_count: usize,
    pub screening_elapsed_ms: u64,
    pub final_review_candidate_count: usize,
    pub final_review_batch_count: usize,
    pub final_review_elapsed_ms: u64,
}

pub fn orchestrate_match<F, V>(
    fetcher: &F,
    vlm: &V,
    primary_pass: SearchPass<'_>,
    fallback_pass: SearchPass<'_>,
    original_ozon_image_base64: &str,
    ozon_name_opt: Option<&str>,
) -> Result<OrchestrationResult, String>
where
    F: CandidateFetcher,
    V: VlmClient,
{
    let mut diagnostics = OrchestrationDiagnostics::default();
    let first_pass = fetcher.fetch_candidates(primary_pass.image_path)?;
    diagnostics.primary_candidates = first_pass.clone();
    let first_report = process_candidates_detailed(
        vlm,
        primary_pass.reference_image_base64,
        Some(original_ozon_image_base64),
        first_pass,
        ozon_name_opt,
        &mut diagnostics,
        "primary",
    )?;

    match &first_report.summary {
        MatchSummary::Cheapest(candidate) => Ok(OrchestrationResult {
            summary: MatchSummary::Cheapest(candidate.clone()),
            used_fallback_image: false,
            no_match_reason: None,
            diagnostics,
        }),
        MatchSummary::NoMatch | MatchSummary::MatchedButPriceUnavailable { .. } => {
            let second_pass = fetcher.fetch_candidates(fallback_pass.image_path)?;
            diagnostics.fallback_candidates = second_pass.clone();
            let second_report = process_candidates_detailed(
                vlm,
                fallback_pass.reference_image_base64,
                Some(original_ozon_image_base64),
                second_pass,
                ozon_name_opt,
                &mut diagnostics,
                "fallback",
            )?;

            let summary = match (&first_report.summary, &second_report.summary) {
                (MatchSummary::MatchedButPriceUnavailable { .. }, MatchSummary::NoMatch) => {
                    first_report.summary.clone()
                }
                _ => second_report.summary.clone(),
            };

            let no_match_reason = matches!(summary, MatchSummary::NoMatch).then(|| {
                resolve_no_match_reason([&first_report.diagnostics, &second_report.diagnostics])
            });

            Ok(OrchestrationResult {
                summary,
                used_fallback_image: true,
                no_match_reason,
                diagnostics,
            })
        }
    }
}

pub fn verify_single_candidate<V>(
    vlm: &V,
    search_reference_image_base64: &str,
    auxiliary_reference_image_base64: Option<&str>,
    candidate: &Candidate,
    ozon_name_opt: Option<&str>,
) -> Result<bool, String>
where
    V: VlmClient,
{
    let result = vlm.match_candidate_grid(
        auxiliary_reference_image_base64.map_or_else(
            || ReferenceImages::screening(search_reference_image_base64),
            |original_image_base64| {
                ReferenceImages::final_review(search_reference_image_base64, original_image_base64)
            },
        ),
        std::slice::from_ref(candidate),
        ozon_name_opt,
    )?;
    Ok(normalize_match_ids(&result.match_ids, 1).contains(&1))
}

pub fn pick_cheapest_after_final_review<V>(
    vlm: &V,
    search_reference_image_base64: &str,
    auxiliary_reference_image_base64: Option<&str>,
    candidates: Vec<Candidate>,
    ozon_name_opt: Option<&str>,
) -> MatchSummary
where
    V: VlmClient,
{
    let mut diagnostics = OrchestrationDiagnostics::default();
    pick_cheapest_after_final_review_detailed(
        vlm,
        search_reference_image_base64,
        auxiliary_reference_image_base64,
        candidates,
        ozon_name_opt,
        &mut diagnostics,
        "primary",
    )
    .summary
}

fn pick_cheapest_after_final_review_detailed<V>(
    vlm: &V,
    search_reference_image_base64: &str,
    auxiliary_reference_image_base64: Option<&str>,
    candidates: Vec<Candidate>,
    ozon_name_opt: Option<&str>,
    diagnostics: &mut OrchestrationDiagnostics,
    pass_label: &str,
) -> CandidateProcessResult
where
    V: VlmClient,
{
    let prepared =
        prepare_final_review_candidates(candidates.clone(), FINAL_REVIEW_CANDIDATE_LIMIT);
    if prepared.is_empty() {
        return CandidateProcessResult {
            summary: summarize_matches(candidates),
            diagnostics: CandidateProcessDiagnostics {
                had_candidates: true,
                had_initial_matches: true,
                final_review_rejected: false,
            },
        };
    }

    diagnostics.final_review_candidate_count += prepared.len();
    let review_batches = prepared
        .chunks(FINAL_REVIEW_BATCH_SIZE)
        .map(|chunk| chunk.to_vec())
        .collect::<Vec<_>>();
    diagnostics.final_review_batch_count += review_batches.len();
    let final_review_started_at = Instant::now();
    let mut has_successful_review = false;
    let mut confirmed_matches = Vec::new();
    let requests = review_batches
        .iter()
        .map(|candidates| VlmBatchRequest {
            references: auxiliary_reference_image_base64.map_or_else(
                || ReferenceImages::screening(search_reference_image_base64),
                |original_image_base64| {
                    ReferenceImages::final_review(
                        search_reference_image_base64,
                        original_image_base64,
                    )
                },
            ),
            candidates,
        })
        .collect::<Vec<_>>();

    for (review_index, (candidates, result)) in review_batches
        .iter()
        .zip(vlm.match_candidate_grids(&requests, ozon_name_opt))
        .enumerate()
    {
        match result {
            Ok(result) => {
                let normalized = normalize_match_ids(&result.match_ids, candidates.len());
                diagnostics.vlm_calls.push(RecordedVlmCall {
                    pass_label: pass_label.to_string(),
                    stage: VlmCallStage::FinalReview,
                    chunk_index: review_index + 1,
                    match_ids: normalized.clone(),
                    trace: result.trace,
                });
                has_successful_review = true;
                confirmed_matches.extend(collect_matched_candidates(candidates, &normalized));
            }
            Err(_) => {}
        }
    }
    diagnostics.final_review_elapsed_ms += elapsed_millis(final_review_started_at.elapsed());

    if !confirmed_matches.is_empty() {
        return CandidateProcessResult {
            summary: summarize_matches(confirmed_matches),
            diagnostics: CandidateProcessDiagnostics {
                had_candidates: true,
                had_initial_matches: true,
                final_review_rejected: false,
            },
        };
    }

    if has_successful_review {
        return CandidateProcessResult {
            summary: MatchSummary::NoMatch,
            diagnostics: CandidateProcessDiagnostics {
                had_candidates: true,
                had_initial_matches: true,
                final_review_rejected: true,
            },
        };
    }

    CandidateProcessResult {
        summary: summarize_matches(candidates),
        diagnostics: CandidateProcessDiagnostics {
            had_candidates: true,
            had_initial_matches: true,
            final_review_rejected: false,
        },
    }
}

pub fn process_candidates<V>(
    vlm: &V,
    search_reference_image_base64: &str,
    auxiliary_reference_image_base64: Option<&str>,
    candidates: Vec<Candidate>,
    ozon_name_opt: Option<&str>,
) -> Result<MatchSummary, String>
where
    V: VlmClient,
{
    let mut diagnostics = OrchestrationDiagnostics::default();
    process_candidates_detailed(
        vlm,
        search_reference_image_base64,
        auxiliary_reference_image_base64,
        candidates,
        ozon_name_opt,
        &mut diagnostics,
        "primary",
    )
    .map(|result| result.summary)
}

fn process_candidates_detailed<V>(
    vlm: &V,
    search_reference_image_base64: &str,
    auxiliary_reference_image_base64: Option<&str>,
    candidates: Vec<Candidate>,
    ozon_name_opt: Option<&str>,
    diagnostics: &mut OrchestrationDiagnostics,
    pass_label: &str,
) -> Result<CandidateProcessResult, String>
where
    V: VlmClient,
{
    let selected_candidates =
        select_screening_candidates(candidates, ozon_name_opt, MAX_VERIFY_CANDIDATES);
    if selected_candidates.is_empty() {
        return Ok(CandidateProcessResult {
            summary: MatchSummary::NoMatch,
            diagnostics: CandidateProcessDiagnostics::default(),
        });
    }
    diagnostics.screening_candidate_count += selected_candidates.len();
    let chunks = selected_candidates
        .chunks(super::matcher::GRID_CANDIDATE_SIZE)
        .map(|chunk| chunk.to_vec())
        .collect::<Vec<_>>();
    diagnostics.screening_chunk_count += chunks.len();

    let mut merged_matches = Vec::new();
    let mut has_success_group = false;
    let screening_started_at = Instant::now();

    let requests = chunks
        .iter()
        .map(|chunk| VlmBatchRequest {
            references: ReferenceImages::screening(search_reference_image_base64),
            candidates: chunk,
        })
        .collect::<Vec<_>>();

    for (chunk_index, (chunk, result)) in chunks
        .iter()
        .zip(vlm.match_candidate_grids(&requests, ozon_name_opt))
        .enumerate()
    {
        match result {
            Ok(result) => {
                let normalized = normalize_match_ids(&result.match_ids, chunk.len());
                diagnostics.vlm_calls.push(RecordedVlmCall {
                    pass_label: pass_label.to_string(),
                    stage: VlmCallStage::Screening,
                    chunk_index: chunk_index + 1,
                    match_ids: normalized.clone(),
                    trace: result.trace,
                });
                has_success_group = true;
                merged_matches.extend(collect_matched_candidates(&chunk, &normalized));
            }
            Err(_) => {}
        }
    }
    diagnostics.screening_elapsed_ms += elapsed_millis(screening_started_at.elapsed());

    if !has_success_group {
        return Err("大模型API调用异常/超时".to_string());
    }

    if merged_matches.is_empty() {
        return Ok(CandidateProcessResult {
            summary: MatchSummary::NoMatch,
            diagnostics: CandidateProcessDiagnostics {
                had_candidates: true,
                had_initial_matches: false,
                final_review_rejected: false,
            },
        });
    }

    Ok(pick_cheapest_after_final_review_detailed(
        vlm,
        search_reference_image_base64,
        auxiliary_reference_image_base64,
        merged_matches,
        ozon_name_opt,
        diagnostics,
        pass_label,
    ))
}

fn resolve_no_match_reason(diagnostics: [&CandidateProcessDiagnostics; 2]) -> NoMatchReason {
    if diagnostics.iter().any(|item| item.final_review_rejected) {
        return NoMatchReason::FinalReviewRejected;
    }

    if diagnostics.iter().any(|item| item.had_candidates) {
        return NoMatchReason::InitialScreenNoMatch;
    }

    NoMatchReason::NoCandidates
}

fn elapsed_millis(duration: std::time::Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}
