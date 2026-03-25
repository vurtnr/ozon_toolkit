use super::matcher::{
    build_verification_chunks, collect_matched_candidates, parse_positive_price_value,
    parse_price_value, parse_sales_value, prepare_final_review_candidates, select_screening_candidates,
    sort_candidates_by_price,
};
use super::types::Candidate;

fn candidate_with_price(price: &str) -> Candidate {
    Candidate {
        title: "t".to_string(),
        price: price.to_string(),
        sales: "".to_string(),
        item_url: format!("u-{price}"),
        image_url: "i".to_string(),
        cos_score_permille: 0,
    }
}

fn candidate_with_price_and_url(price: &str, item_url: &str) -> Candidate {
    Candidate {
        title: "t".to_string(),
        price: price.to_string(),
        sales: "".to_string(),
        item_url: item_url.to_string(),
        image_url: "i".to_string(),
        cos_score_permille: 0,
    }
}

fn candidate_with_meta(
    title: &str,
    price: &str,
    sales: &str,
    item_url: &str,
    cos_score_permille: u16,
) -> Candidate {
    Candidate {
        title: title.to_string(),
        price: price.to_string(),
        sales: sales.to_string(),
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
fn parse_sales_value_extracts_plain_and_wan_units() {
    assert_eq!(parse_sales_value("月销123笔"), Some(123.0));
    assert_eq!(parse_sales_value("2.5万+"), Some(25_000.0));
    assert_eq!(parse_sales_value("成交 980"), Some(980.0));
    assert_eq!(parse_sales_value("暂无销量"), None);
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
            "月销500+",
            &format!("strong-{index}"),
            940,
        ));
    }
    for index in 0..12 {
        candidates.push(candidate_with_meta(
            &format!("office chair {index}"),
            &format!("¥{}", 2 + index),
            "月销1",
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
            &format!("月销{}", 100 - index),
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
fn prepare_final_review_candidates_deduplicates_and_preserves_rank_order() {
    let candidates = vec![
        candidate_with_meta("travel bag", "¥3.5", "月销11", "u1", 950),
        candidate_with_meta("travel bag", "¥2.0", "月销9", "u2", 940),
        candidate_with_meta("travel bag", "¥1.5", "月销99", "u2", 930),
        candidate_with_meta("travel bag", "面议", "月销7", "u3", 920),
        candidate_with_meta("travel bag", "¥1.8", "月销8", "u4", 910),
    ];

    let prepared = prepare_final_review_candidates(candidates, 10);
    let urls = prepared
        .iter()
        .map(|candidate| candidate.item_url.as_str())
        .collect::<Vec<_>>();
    assert_eq!(urls, vec!["u1", "u2", "u3", "u4"]);
}

#[test]
fn select_screening_candidates_keeps_relevant_high_sales_frontier_items() {
    let selected = select_screening_candidates(
        vec![
            candidate_with_meta("travel bag waterproof", "¥25.0", "月销51", "u1", 960),
            candidate_with_meta("travel bag duffel", "¥28.0", "月销48", "u2", 930),
            candidate_with_meta("office chair", "¥3.0", "月销9999", "u3", 120),
            candidate_with_meta("travel bag lightweight", "¥8.5", "月销888", "u4", 710),
            candidate_with_meta("desk lamp", "¥2.0", "月销6666", "u5", 80),
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
fn prepare_final_review_candidates_prioritizes_late_high_sales_matches() {
    let prepared = prepare_final_review_candidates(
        vec![
            candidate_with_meta("travel bag", "¥28.0", "月销10", "u1", 950),
            candidate_with_meta("travel bag", "¥29.0", "月销9", "u2", 940),
            candidate_with_meta("travel bag", "¥30.0", "月销8", "u3", 930),
            candidate_with_meta("travel bag", "¥31.0", "月销7", "u4", 920),
            candidate_with_meta("travel bag", "¥40.8", "月销4800", "u5", 600),
            candidate_with_meta("travel bag", "¥41.2", "月销5200", "u6", 590),
        ],
        4,
    );

    let urls = prepared
        .iter()
        .map(|candidate| candidate.item_url.as_str())
        .collect::<Vec<_>>();
    assert_eq!(urls, vec!["u1", "u2", "u5", "u6"]);
}

#[test]
fn prepare_final_review_candidates_preserves_top_rank_window_before_late_sales_fill() {
    let prepared = prepare_final_review_candidates(
        vec![
            candidate_with_meta("travel bag", "¥28.0", "月销10", "u1", 980),
            candidate_with_meta("travel bag", "¥29.0", "月销9", "u2", 970),
            candidate_with_meta("travel bag", "¥30.0", "月销8", "u3", 960),
            candidate_with_meta("travel bag", "¥31.0", "月销7", "u4", 950),
            candidate_with_meta("travel bag", "¥32.0", "月销6", "u5", 940),
            candidate_with_meta("travel bag", "¥33.0", "月销5", "u6", 930),
            candidate_with_meta("travel bag", "¥34.0", "月销4", "u7", 920),
            candidate_with_meta("travel bag", "¥51.2", "月销12000", "u8", 500),
            candidate_with_meta("travel bag", "¥51.5", "月销11000", "u9", 490),
            candidate_with_meta("travel bag", "¥51.8", "月销10000", "u10", 480),
        ],
        8,
    );

    let urls = prepared
        .iter()
        .map(|candidate| candidate.item_url.as_str())
        .collect::<Vec<_>>();
    assert_eq!(urls, vec!["u1", "u2", "u3", "u4", "u5", "u6", "u8", "u9"]);
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
