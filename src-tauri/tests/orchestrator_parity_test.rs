use std::collections::VecDeque;
use std::path::Path;
use std::sync::Mutex;

use desktop_app_lib::core::orchestrator::{
    orchestrate_match, pick_cheapest_after_final_review, process_candidates,
    verify_single_candidate, CandidateFetcher, NoMatchReason, SearchPass,
};
use desktop_app_lib::core::types::{Candidate, MatchSummary};
use desktop_app_lib::core::vlm::{
    normalize_match_ids, parse_vlm_response_content, ReferenceImages, VlmCallTrace, VlmClient,
    VlmMatchResult,
};

fn candidate(price: &str, item_url: &str) -> Candidate {
    Candidate {
        title: format!("title-{item_url}"),
        price: price.to_string(),
        sales: "".to_string(),
        item_url: item_url.to_string(),
        image_url: format!("https://img.example/{item_url}.jpg"),
        cos_score_permille: 0,
    }
}

fn candidate_with_rank(
    price: &str,
    sales: &str,
    item_url: &str,
    cos_score_permille: u16,
) -> Candidate {
    Candidate {
        title: format!("title-{item_url}"),
        price: price.to_string(),
        sales: sales.to_string(),
        item_url: item_url.to_string(),
        image_url: format!("https://img.example/{item_url}.jpg"),
        cos_score_permille,
    }
}

fn candidate_series(count: usize) -> Vec<Candidate> {
    (0..count)
        .map(|index| {
            candidate_with_rank(
                &format!("¥{}", 10 + index),
                &format!("月销{}", 200 - index),
                &format!("candidate-{index}"),
                780,
            )
        })
        .collect()
}

#[derive(Default)]
struct RecordingFetcher {
    responses: Mutex<VecDeque<Result<Vec<Candidate>, String>>>,
    calls: Mutex<Vec<String>>,
}

impl RecordingFetcher {
    fn with_responses(responses: Vec<Result<Vec<Candidate>, String>>) -> Self {
        Self {
            responses: Mutex::new(VecDeque::from(responses)),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn recorded_calls(&self) -> Vec<String> {
        self.calls.lock().expect("calls lock").clone()
    }
}

impl CandidateFetcher for RecordingFetcher {
    fn fetch_candidates(&self, image_path: &Path) -> Result<Vec<Candidate>, String> {
        self.calls
            .lock()
            .expect("calls lock")
            .push(image_path.display().to_string());
        self.responses
            .lock()
            .expect("responses lock")
            .pop_front()
            .expect("test fetch response should exist")
    }
}

#[derive(Default)]
struct ScriptedVlm {
    replies: Mutex<VecDeque<Result<Vec<usize>, String>>>,
    calls: Mutex<Vec<RecordedVlmCall>>,
}

impl ScriptedVlm {
    fn with_replies(replies: Vec<Result<Vec<usize>, String>>) -> Self {
        Self {
            replies: Mutex::new(VecDeque::from(replies)),
            calls: Mutex::new(Vec::new()),
        }
    }

    fn recorded_calls(&self) -> Vec<RecordedVlmCall> {
        self.calls.lock().expect("calls lock").clone()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RecordedVlmCall {
    primary_reference_image_base64: String,
    auxiliary_reference_image_base64: Option<String>,
    candidate_count: usize,
}

impl VlmClient for ScriptedVlm {
    fn match_candidate_grid(
        &self,
        references: ReferenceImages<'_>,
        candidates: &[Candidate],
        _ozon_name_opt: Option<&str>,
    ) -> Result<VlmMatchResult, String> {
        self.calls
            .lock()
            .expect("calls lock")
            .push(RecordedVlmCall {
                primary_reference_image_base64: references
                    .primary_reference_image_base64
                    .to_string(),
                auxiliary_reference_image_base64: references
                    .auxiliary_reference_image_base64
                    .map(str::to_string),
                candidate_count: candidates.len(),
            });
        let match_ids = self
            .replies
            .lock()
            .expect("replies lock")
            .pop_front()
            .expect("test VLM reply should exist")?;
        Ok(VlmMatchResult {
            match_ids,
            trace: VlmCallTrace {
                system_prompt: "test-system".to_string(),
                user_prompt: "test-user".to_string(),
                raw_response_text: "{}".to_string(),
                grid_jpeg_bytes: Vec::new(),
                candidates: candidates.to_vec(),
            },
        })
    }
}

#[test]
fn orchestrator_uses_fallback_image_when_primary_returns_no_match() {
    let fetcher = RecordingFetcher::with_responses(vec![
        Ok(vec![candidate("¥19.90", "first-pass")]),
        Ok(vec![candidate("¥8.80", "second-pass")]),
    ]);
    let vlm = ScriptedVlm::with_replies(vec![Ok(vec![]), Ok(vec![1]), Ok(vec![1])]);

    let result = orchestrate_match(
        &fetcher,
        &vlm,
        SearchPass {
            image_path: Path::new("/tmp/search_primary.png"),
            reference_image_base64: "data:image/png;base64,processed-primary",
        },
        SearchPass {
            image_path: Path::new("/tmp/search_fallback.png"),
            reference_image_base64: "data:image/png;base64,processed-fallback",
        },
        "data:image/jpeg;base64,source",
        Some("sample ozon name"),
    )
    .expect("orchestration should succeed");

    assert_eq!(
        fetcher.recorded_calls(),
        vec![
            "/tmp/search_primary.png".to_string(),
            "/tmp/search_fallback.png".to_string(),
        ]
    );
    assert_eq!(
        result.summary,
        MatchSummary::Cheapest(candidate("¥8.80", "second-pass"))
    );
    assert!(result.used_fallback_image);
    assert_eq!(result.no_match_reason, None);
}

#[test]
fn orchestrator_keeps_primary_match_when_primary_matches_without_price() {
    let fetcher = RecordingFetcher::with_responses(vec![
        Ok(vec![candidate("面议", "first-pass-no-price")]),
        Ok(vec![candidate("¥6.60", "second-pass")]),
    ]);
    let vlm = ScriptedVlm::with_replies(vec![Ok(vec![1]), Ok(vec![1]), Ok(vec![1]), Ok(vec![1])]);

    let result = orchestrate_match(
        &fetcher,
        &vlm,
        SearchPass {
            image_path: Path::new("/tmp/search_primary.png"),
            reference_image_base64: "data:image/png;base64,processed-primary",
        },
        SearchPass {
            image_path: Path::new("/tmp/search_fallback.png"),
            reference_image_base64: "data:image/png;base64,processed-fallback",
        },
        "data:image/jpeg;base64,source",
        Some("sample ozon name"),
    )
    .expect("orchestration should succeed");

    assert_eq!(
        fetcher.recorded_calls(),
        vec!["/tmp/search_primary.png".to_string()]
    );
    assert_eq!(
        result.summary,
        MatchSummary::Cheapest(candidate("面议", "first-pass-no-price"))
    );
    assert_eq!(result.no_match_reason, None);
    assert!(!result.used_fallback_image);
}

#[test]
fn orchestrator_returns_no_match_after_two_passes() {
    let fetcher = RecordingFetcher::with_responses(vec![
        Ok(vec![candidate("¥19.90", "first-pass")]),
        Ok(vec![candidate("¥8.80", "second-pass")]),
    ]);
    let vlm = ScriptedVlm::with_replies(vec![Ok(vec![]), Ok(vec![])]);

    let result = orchestrate_match(
        &fetcher,
        &vlm,
        SearchPass {
            image_path: Path::new("/tmp/search_primary.png"),
            reference_image_base64: "data:image/png;base64,processed-primary",
        },
        SearchPass {
            image_path: Path::new("/tmp/search_fallback.png"),
            reference_image_base64: "data:image/png;base64,processed-fallback",
        },
        "data:image/jpeg;base64,source",
        Some("sample ozon name"),
    )
    .expect("orchestration should succeed");

    assert_eq!(
        fetcher.recorded_calls(),
        vec![
            "/tmp/search_primary.png".to_string(),
            "/tmp/search_fallback.png".to_string(),
        ]
    );
    assert_eq!(result.summary, MatchSummary::NoMatch);
    assert!(result.used_fallback_image);
    assert_eq!(
        result.no_match_reason,
        Some(NoMatchReason::InitialScreenNoMatch)
    );
}

#[test]
fn orchestrator_keeps_price_unavailable_when_fallback_finds_no_match() {
    let fetcher = RecordingFetcher::with_responses(vec![
        Ok(vec![candidate("面议", "first-pass-no-price")]),
        Ok(vec![candidate("¥8.80", "second-pass")]),
    ]);
    let vlm = ScriptedVlm::with_replies(vec![Ok(vec![1]), Ok(vec![1]), Ok(vec![])]);

    let result = orchestrate_match(
        &fetcher,
        &vlm,
        SearchPass {
            image_path: Path::new("/tmp/search_primary.png"),
            reference_image_base64: "data:image/png;base64,processed-primary",
        },
        SearchPass {
            image_path: Path::new("/tmp/search_fallback.png"),
            reference_image_base64: "data:image/png;base64,processed-fallback",
        },
        "data:image/jpeg;base64,source",
        Some("sample ozon name"),
    )
    .expect("orchestration should succeed");

    assert_eq!(
        fetcher.recorded_calls(),
        vec!["/tmp/search_primary.png".to_string()]
    );
    assert_eq!(
        result.summary,
        MatchSummary::Cheapest(candidate("面议", "first-pass-no-price"))
    );
    assert!(!result.used_fallback_image);
    assert_eq!(result.no_match_reason, None);
}

#[test]
fn process_candidates_returns_no_match_when_groups_succeed_without_hits() {
    let vlm = ScriptedVlm::with_replies(vec![Ok(vec![])]);
    let candidates = vec![candidate("¥19.90", "candidate-1")];

    let result = process_candidates(
        &vlm,
        "data:image/png;base64,processed-search",
        Some("data:image/jpeg;base64,source"),
        candidates,
        Some("sample ozon name"),
    )
    .expect("candidate processing should succeed");

    assert_eq!(result, MatchSummary::NoMatch);
}

#[test]
fn process_candidates_stops_after_first_screening_wave_when_early_hits_exist() {
    let vlm = ScriptedVlm::with_replies(vec![Ok(vec![1]), Ok(vec![]), Ok(vec![1])]);

    let result = process_candidates(
        &vlm,
        "data:image/png;base64,processed-search",
        Some("data:image/jpeg;base64,source"),
        candidate_series(27),
        Some("candidate"),
    )
    .expect("candidate processing should succeed");

    assert!(matches!(result, MatchSummary::Cheapest(_)));
    assert_eq!(
        vlm.recorded_calls()
            .into_iter()
            .map(|call| call.candidate_count)
            .collect::<Vec<_>>(),
        vec![9, 9, 1],
    );
}

#[test]
fn process_candidates_expands_to_second_screening_wave_after_early_miss() {
    let vlm = ScriptedVlm::with_replies(vec![Ok(vec![]), Ok(vec![]), Ok(vec![1]), Ok(vec![1])]);

    let result = process_candidates(
        &vlm,
        "data:image/png;base64,processed-search",
        Some("data:image/jpeg;base64,source"),
        candidate_series(27),
        Some("candidate"),
    )
    .expect("candidate processing should succeed");

    assert!(matches!(result, MatchSummary::Cheapest(_)));
    assert_eq!(
        vlm.recorded_calls()
            .into_iter()
            .map(|call| call.candidate_count)
            .collect::<Vec<_>>(),
        vec![9, 9, 9, 1],
    );
}

#[test]
fn normalize_match_ids_discards_invalid_ids_and_deduplicates() {
    let normalized = normalize_match_ids(&[3, 0, 1, 1, 9, 2], 3);

    assert_eq!(normalized, vec![1, 2, 3]);
}

#[test]
fn parse_vlm_response_content_returns_empty_array_for_malformed_content() {
    let match_ids = parse_vlm_response_content("not-json", 4);

    assert!(match_ids.is_empty());
}

#[test]
fn parse_vlm_response_content_recovers_match_ids_from_truncated_json() {
    let match_ids = parse_vlm_response_content(
        "{\n  \"reasoning\": \"too long\",\n  \"match_ids\": [1, 2, 4, 7",
        4,
    );

    assert_eq!(match_ids, vec![1, 2, 4]);
}

#[test]
fn verify_single_candidate_returns_true_when_slot_one_matches() {
    let vlm = ScriptedVlm::with_replies(vec![Ok(vec![1])]);
    let matched = verify_single_candidate(
        &vlm,
        "data:image/png;base64,processed-search",
        Some("data:image/jpeg;base64,source"),
        &candidate("¥8.80", "single"),
        Some("sample ozon name"),
    )
    .expect("single verify should succeed");

    assert!(matched);
}

#[test]
fn pick_cheapest_after_final_review_prefers_highest_similarity_then_highest_sales() {
    let vlm = ScriptedVlm::with_replies(vec![Ok(vec![1, 2])]);
    let result = pick_cheapest_after_final_review(
        &vlm,
        "data:image/png;base64,processed-search",
        Some("data:image/jpeg;base64,source"),
        vec![
            candidate_with_rank("¥8.80", "月销80", "candidate-b", 920),
            candidate_with_rank("¥16.60", "月销900", "candidate-a", 980),
        ],
        Some("sample ozon name"),
    );

    assert_eq!(
        result,
        MatchSummary::Cheapest(candidate_with_rank(
            "¥16.60", "月销900", "candidate-a", 980,
        ))
    );
}

#[test]
fn pick_cheapest_after_final_review_batches_strict_reviews() {
    let vlm = ScriptedVlm::with_replies(vec![Ok(vec![]), Ok(vec![2])]);
    let result = pick_cheapest_after_final_review(
        &vlm,
        "data:image/png;base64,processed-search",
        Some("data:image/jpeg;base64,source"),
        vec![
            candidate("¥12.80", "candidate-1"),
            candidate("¥15.90", "candidate-2"),
            candidate("¥18.20", "candidate-3"),
            candidate("¥9.60", "candidate-4"),
            candidate("¥7.80", "candidate-5"),
            candidate("¥6.20", "candidate-6"),
            candidate("¥20.10", "candidate-7"),
            candidate("¥22.40", "candidate-8"),
        ],
        Some("sample ozon name"),
    );

    assert_eq!(
        result,
        MatchSummary::Cheapest(candidate("¥6.20", "candidate-6"))
    );
    assert_eq!(
        vlm.recorded_calls(),
        vec![
            RecordedVlmCall {
                primary_reference_image_base64: "data:image/png;base64,processed-search"
                    .to_string(),
                auxiliary_reference_image_base64: Some("data:image/jpeg;base64,source".to_string()),
                candidate_count: 4,
            },
            RecordedVlmCall {
                primary_reference_image_base64: "data:image/png;base64,processed-search"
                    .to_string(),
                auxiliary_reference_image_base64: Some("data:image/jpeg;base64,source".to_string()),
                candidate_count: 4,
            },
        ]
    );
}

#[test]
fn pick_cheapest_after_final_review_prefers_best_confirmed_match_while_still_finishing_review_wave() {
    let vlm = ScriptedVlm::with_replies(vec![Ok(vec![2]), Ok(vec![1])]);
    let result = pick_cheapest_after_final_review(
        &vlm,
        "data:image/png;base64,processed-search",
        Some("data:image/jpeg;base64,source"),
        vec![
            candidate_with_rank("¥12.80", "月销11", "candidate-1", 910),
            candidate_with_rank("¥15.90", "月销13", "candidate-2", 900),
            candidate_with_rank("¥18.20", "月销17", "candidate-3", 890),
            candidate_with_rank("¥9.60", "月销19", "candidate-4", 880),
            candidate_with_rank("¥7.80", "月销50", "candidate-5", 990),
            candidate_with_rank("¥6.20", "月销9", "candidate-6", 700),
            candidate_with_rank("¥20.10", "月销8", "candidate-7", 680),
            candidate_with_rank("¥22.40", "月销7", "candidate-8", 670),
        ],
        Some("sample ozon name"),
    );

    assert_eq!(
        result,
        MatchSummary::Cheapest(candidate_with_rank(
            "¥7.80",
            "月销50",
            "candidate-5",
            990,
        ))
    );
    assert_eq!(
        vlm.recorded_calls(),
        vec![
            RecordedVlmCall {
                primary_reference_image_base64: "data:image/png;base64,processed-search"
                    .to_string(),
                auxiliary_reference_image_base64: Some(
                    "data:image/jpeg;base64,source".to_string()
                ),
                candidate_count: 4,
            },
            RecordedVlmCall {
                primary_reference_image_base64: "data:image/png;base64,processed-search"
                    .to_string(),
                auxiliary_reference_image_base64: Some(
                    "data:image/jpeg;base64,source".to_string()
                ),
                candidate_count: 4,
            },
        ]
    );
}

#[test]
fn process_candidates_returns_best_match_even_when_only_unpriced_matches_exist() {
    let vlm = ScriptedVlm::with_replies(vec![Ok(vec![1]), Ok(vec![1])]);
    let result = process_candidates(
        &vlm,
        "data:image/png;base64,processed-search",
        Some("data:image/jpeg;base64,source"),
        vec![candidate_with_rank("面议", "月销12", "candidate-a", 860)],
        Some("sample ozon name"),
    )
    .expect("candidate processing should succeed");

    assert_eq!(
        result,
        MatchSummary::Cheapest(candidate_with_rank(
            "面议",
            "月销12",
            "candidate-a",
            860,
        ))
    );
}

#[test]
fn process_candidates_errors_when_all_group_checks_fail() {
    let vlm = ScriptedVlm::with_replies(vec![Err("vlm timeout".to_string())]);
    let result = process_candidates(
        &vlm,
        "data:image/png;base64,processed-search",
        Some("data:image/jpeg;base64,source"),
        vec![candidate("¥8.80", "candidate-a")],
        Some("sample ozon name"),
    );

    assert!(result.is_err());
}

#[test]
fn orchestrator_uses_processed_search_image_for_screening_and_original_only_for_final_review() {
    let fetcher = RecordingFetcher::with_responses(vec![Ok(vec![
        candidate("¥8.80", "candidate-a"),
        candidate("¥9.90", "candidate-b"),
    ])]);
    let vlm = ScriptedVlm::with_replies(vec![Ok(vec![1]), Ok(vec![1])]);

    let result = orchestrate_match(
        &fetcher,
        &vlm,
        SearchPass {
            image_path: Path::new("/tmp/search_primary.png"),
            reference_image_base64: "data:image/png;base64,processed-primary",
        },
        SearchPass {
            image_path: Path::new("/tmp/search_fallback.png"),
            reference_image_base64: "data:image/png;base64,processed-fallback",
        },
        "data:image/jpeg;base64,original-source",
        Some("sample ozon name"),
    )
    .expect("orchestration should succeed");

    assert_eq!(
        result.summary,
        MatchSummary::Cheapest(candidate("¥8.80", "candidate-a"))
    );
    assert_eq!(
        vlm.recorded_calls(),
        vec![
            RecordedVlmCall {
                primary_reference_image_base64: "data:image/png;base64,processed-primary"
                    .to_string(),
                auxiliary_reference_image_base64: None,
                candidate_count: 2,
            },
            RecordedVlmCall {
                primary_reference_image_base64: "data:image/png;base64,processed-primary"
                    .to_string(),
                auxiliary_reference_image_base64: Some(
                    "data:image/jpeg;base64,original-source".to_string()
                ),
                candidate_count: 1,
            },
        ]
    );
}
