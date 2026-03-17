use super::matcher::{
    build_verification_chunks, collect_matched_candidates, parse_positive_price_value,
    parse_price_value, prepare_final_review_candidates, select_screening_candidates,
    sort_candidates_by_price,
};
use super::types::Candidate;

fn candidate_with_price(price: &str) -> Candidate {
    Candidate {
        title: "t".to_string(),
        price: price.to_string(),
        item_url: format!("u-{price}"),
        image_url: "i".to_string(),
        cos_score_permille: 0,
    }
}

fn candidate_with_price_and_url(price: &str, item_url: &str) -> Candidate {
    Candidate {
        title: "t".to_string(),
        price: price.to_string(),
        item_url: item_url.to_string(),
        image_url: "i".to_string(),
        cos_score_permille: 0,
    }
}

fn candidate_with_meta(
    title: &str,
    price: &str,
    item_url: &str,
    cos_score_permille: u16,
) -> Candidate {
    Candidate {
        title: title.to_string(),
        price: price.to_string(),
        item_url: item_url.to_string(),
        image_url: format!("https://img.example/{item_url}.jpg"),
        cos_score_permille,
    }
}

#[test]
fn parse_price_value_extracts_min_from_range() {
    let price = parse_price_value("¥12.5-18.0");
    assert_eq!(price, Some(12.5));
}

#[test]
fn parse_price_value_ignores_moq_number_and_uses_currency_price() {
    let price = parse_price_value("2件起批 ¥19.80");
    assert_eq!(price, Some(19.8));
}

#[test]
fn parse_positive_price_value_rejects_zero() {
    assert_eq!(parse_positive_price_value("¥0"), None);
    assert_eq!(parse_positive_price_value("¥0.01"), Some(0.01));
}

#[test]
fn sort_candidates_by_price_keeps_lowest_first_and_non_numeric_last() {
    let mut candidates = vec![
        candidate_with_price("面议"),
        candidate_with_price("¥19.8"),
        candidate_with_price("¥12.0-13.0"),
        candidate_with_price("¥15"),
    ];

    sort_candidates_by_price(&mut candidates);

    assert_eq!(candidates[0].price, "¥12.0-13.0");
    assert_eq!(candidates[3].price, "面议");
}

#[test]
fn build_verification_chunks_caps_selected_pool_to_27() {
    let mut candidates = Vec::new();
    for i in (1..=50).rev() {
        candidates.push(candidate_with_price(&format!("¥{}", i)));
    }

    let chunks = build_verification_chunks(candidates, Some("sample bag"));
    assert_eq!(chunks.len(), 3);
    let flattened = chunks.into_iter().flatten().collect::<Vec<_>>();
    assert_eq!(flattened.len(), 27);
    assert_eq!(flattened[0].price, "¥50");
}

#[test]
fn build_verification_chunks_shrinks_to_two_groups_when_frontier_is_strong() {
    let mut candidates = Vec::new();
    for index in 0..18 {
        candidates.push(candidate_with_meta(
            &format!("travel bag premium {index}"),
            &format!("¥{}", 30 + index),
            &format!("strong-{index}"),
            940,
        ));
    }
    for index in 0..12 {
        candidates.push(candidate_with_meta(
            &format!("office chair {index}"),
            &format!("¥{}", 2 + index),
            &format!("weak-{index}"),
            80,
        ));
    }

    let chunks = build_verification_chunks(candidates, Some("travel bag"));

    assert_eq!(
        chunks.len(),
        2,
        "strong relevance concentration should avoid a third screening batch"
    );
    let flattened = chunks.into_iter().flatten().collect::<Vec<_>>();
    assert_eq!(flattened.len(), 18);
    assert!(flattened
        .iter()
        .all(|candidate| candidate.item_url.starts_with("strong-")));
}

#[test]
fn build_verification_chunks_keeps_three_groups_when_tail_is_still_competitive() {
    let mut candidates = Vec::new();
    for index in 0..30 {
        candidates.push(candidate_with_meta(
            &format!("travel bag competitive {index}"),
            &format!("¥{}", 10 + index),
            &format!("competitive-{index}"),
            780,
        ));
    }

    let chunks = build_verification_chunks(candidates, Some("travel bag"));

    assert_eq!(chunks.len(), 3);
    let flattened = chunks.into_iter().flatten().collect::<Vec<_>>();
    assert_eq!(flattened.len(), 27);
}

#[test]
fn prepare_final_review_candidates_deduplicates_and_sorts_by_price() {
    let candidates = vec![
        candidate_with_price_and_url("¥3.5", "u1"),
        candidate_with_price_and_url("¥2.0", "u2"),
        candidate_with_price_and_url("¥1.5", "u2"),
        candidate_with_price_and_url("面议", "u3"),
        candidate_with_price_and_url("¥1.8", "u4"),
    ];

    let prepared = prepare_final_review_candidates(candidates, 10);
    assert_eq!(prepared.len(), 3);
    assert_eq!(prepared[0].item_url, "u4");
    assert_eq!(prepared[1].item_url, "u2");
    assert_eq!(prepared[2].item_url, "u1");
}

#[test]
fn select_screening_candidates_keeps_relevant_low_price_frontier_items() {
    let selected = select_screening_candidates(
        vec![
            candidate_with_meta("travel bag waterproof", "¥25.0", "u1", 960),
            candidate_with_meta("travel bag duffel", "¥28.0", "u2", 930),
            candidate_with_meta("office chair", "¥3.0", "u3", 120),
            candidate_with_meta("travel bag lightweight", "¥8.5", "u4", 710),
            candidate_with_meta("desk lamp", "¥2.0", "u5", 80),
        ],
        Some("travel bag"),
        3,
    );

    let urls = selected
        .iter()
        .map(|candidate| candidate.item_url.as_str())
        .collect::<Vec<_>>();
    assert_eq!(urls, vec!["u1", "u2", "u4"]);
}

#[test]
fn prepare_final_review_candidates_prioritizes_late_cheaper_matches() {
    let prepared = prepare_final_review_candidates(
        vec![
            candidate_with_price_and_url("¥28.0", "u1"),
            candidate_with_price_and_url("¥29.0", "u2"),
            candidate_with_price_and_url("¥30.0", "u3"),
            candidate_with_price_and_url("¥31.0", "u4"),
            candidate_with_price_and_url("¥4.8", "u5"),
            candidate_with_price_and_url("¥5.2", "u6"),
        ],
        4,
    );

    let urls = prepared
        .iter()
        .map(|candidate| candidate.item_url.as_str())
        .collect::<Vec<_>>();
    assert_eq!(urls, vec!["u5", "u6", "u1", "u2"]);
}

#[test]
fn prepare_final_review_candidates_preserves_top_rank_window_before_late_price_fill() {
    let prepared = prepare_final_review_candidates(
        vec![
            candidate_with_price_and_url("¥28.0", "u1"),
            candidate_with_price_and_url("¥29.0", "u2"),
            candidate_with_price_and_url("¥30.0", "u3"),
            candidate_with_price_and_url("¥31.0", "u4"),
            candidate_with_price_and_url("¥32.0", "u5"),
            candidate_with_price_and_url("¥33.0", "u6"),
            candidate_with_price_and_url("¥34.0", "u7"),
            candidate_with_price_and_url("¥1.2", "u8"),
            candidate_with_price_and_url("¥1.5", "u9"),
            candidate_with_price_and_url("¥1.8", "u10"),
        ],
        8,
    );

    let urls = prepared
        .iter()
        .map(|candidate| candidate.item_url.as_str())
        .collect::<Vec<_>>();
    assert_eq!(urls, vec!["u8", "u9", "u1", "u2", "u3", "u4", "u5", "u6"]);
}

#[test]
fn collect_matched_candidates_uses_normalized_one_based_ids() {
    let chunk = vec![
        candidate_with_price_and_url("¥3.0", "u1"),
        candidate_with_price_and_url("¥2.0", "u2"),
        candidate_with_price_and_url("¥1.0", "u3"),
    ];

    let matched = collect_matched_candidates(&chunk, &[3, 0, 1, 3, 9]);

    assert_eq!(
        matched,
        vec![
            candidate_with_price_and_url("¥3.0", "u1"),
            candidate_with_price_and_url("¥1.0", "u3")
        ]
    );
}
