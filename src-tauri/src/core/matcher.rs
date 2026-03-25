use std::cmp::Ordering;
use std::collections::HashSet;
use std::sync::OnceLock;

use regex::Regex;

use super::types::{Candidate, MatchSummary};
use super::vlm::normalize_match_ids;

pub const GRID_CANDIDATE_SIZE: usize = 9;
pub const MAX_VERIFY_GROUPS: usize = 3;
pub const MAX_VERIFY_CANDIDATES: usize = GRID_CANDIDATE_SIZE * MAX_VERIFY_GROUPS;
pub const FINAL_REVIEW_CANDIDATE_LIMIT: usize = 16;
const SCREENING_RELEVANCE_LIMIT: usize = 18;
const ADAPTIVE_SCREENING_LIMIT: usize = 18;
const MIN_FRONTIER_RELEVANCE_SCORE: f32 = 0.25;
const ADAPTIVE_EDGE_RELEVANCE_SCORE: f32 = 0.55;
const ADAPTIVE_TOP_RELEVANCE_AVG: f32 = 0.82;
const ADAPTIVE_TAIL_RELEVANCE_CEILING: f32 = 0.2;

#[derive(Debug, Clone)]
struct ScoredCandidate {
    index: usize,
    candidate: Candidate,
    relevance_score: f32,
    sales_value: Option<f64>,
}

pub fn parse_price_value(price: &str) -> Option<f64> {
    static CURRENCY_RE: OnceLock<Regex> = OnceLock::new();
    static YUAN_RE: OnceLock<Regex> = OnceLock::new();
    static PURE_RE: OnceLock<Regex> = OnceLock::new();

    fn parse_min(captures: &regex::Captures, first: usize, second: usize) -> Option<f64> {
        let first_value = captures
            .get(first)
            .and_then(|m| m.as_str().parse::<f64>().ok())
            .filter(|v| v.is_finite())?;
        let second_value = captures
            .get(second)
            .and_then(|m| m.as_str().parse::<f64>().ok())
            .filter(|v| v.is_finite());
        Some(second_value.map_or(first_value, |v| first_value.min(v)))
    }

    let normalized = price.replace([',', '，'], "");
    let currency_re = CURRENCY_RE.get_or_init(|| {
        Regex::new(r"[¥￥]\s*([0-9]+(?:\.[0-9]+)?)\s*(?:[-~至]\s*([0-9]+(?:\.[0-9]+)?))?")
            .expect("invalid currency regex")
    });
    if let Some(cap) = currency_re.captures(&normalized) {
        return parse_min(&cap, 1, 2);
    }

    let yuan_re = YUAN_RE.get_or_init(|| {
        Regex::new(r"([0-9]+(?:\.[0-9]+)?)\s*(?:[-~至]\s*([0-9]+(?:\.[0-9]+)?))?\s*元")
            .expect("invalid yuan regex")
    });
    if let Some(cap) = yuan_re.captures(&normalized) {
        return parse_min(&cap, 1, 2);
    }

    let pure_re = PURE_RE.get_or_init(|| {
        Regex::new(r"^\s*([0-9]+(?:\.[0-9]+)?)\s*(?:[-~至]\s*([0-9]+(?:\.[0-9]+)?))?\s*$")
            .expect("invalid pure regex")
    });
    pure_re
        .captures(&normalized)
        .and_then(|cap| parse_min(&cap, 1, 2))
}

pub fn parse_positive_price_value(price: &str) -> Option<f64> {
    parse_price_value(price).filter(|value| *value > 0.0)
}

pub fn parse_sales_value(sales: &str) -> Option<f64> {
    static SALES_RE: OnceLock<Regex> = OnceLock::new();

    let normalized = sales
        .replace([',', '，', ' '], "")
        .replace("＋", "+")
        .trim()
        .to_string();
    if normalized.is_empty() {
        return None;
    }

    let sales_re = SALES_RE.get_or_init(|| {
        Regex::new(r"([0-9]+(?:\.[0-9]+)?)(万|千)?\+?")
            .expect("invalid sales regex")
    });
    let captures = sales_re.captures(&normalized)?;
    let base_value = captures
        .get(1)
        .and_then(|value| value.as_str().parse::<f64>().ok())
        .filter(|value| value.is_finite())?;
    let multiplier = match captures.get(2).map(|value| value.as_str()) {
        Some("万") => 10_000.0,
        Some("千") => 1_000.0,
        _ => 1.0,
    };

    Some(base_value * multiplier)
}

fn price_sort_key(price: &str) -> f64 {
    parse_positive_price_value(price).unwrap_or(f64::MAX)
}

pub fn sort_candidates_by_price(candidates: &mut [Candidate]) {
    candidates.sort_by(|a, b| {
        price_sort_key(&a.price)
            .partial_cmp(&price_sort_key(&b.price))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
}

pub fn select_screening_candidates(
    candidates: Vec<Candidate>,
    ozon_name_opt: Option<&str>,
    limit: usize,
) -> Vec<Candidate> {
    if limit == 0 {
        return Vec::new();
    }

    let deduped = dedupe_candidates_by_url(candidates);
    if deduped.len() <= limit {
        return deduped;
    }

    let scored = deduped
        .iter()
        .cloned()
        .enumerate()
        .map(|(index, candidate)| ScoredCandidate {
            relevance_score: score_candidate_relevance(
                &candidate,
                index,
                deduped.len(),
                ozon_name_opt,
            ),
            sales_value: parse_sales_value(&candidate.sales),
            index,
            candidate,
        })
        .collect::<Vec<_>>();

    let mut top_relevance = scored.clone();
    top_relevance.sort_by(|a, b| {
        b.relevance_score
            .partial_cmp(&a.relevance_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.index.cmp(&b.index))
    });
    let adaptive_limit = resolve_screening_limit(&top_relevance, limit);
    let relevance_limit = SCREENING_RELEVANCE_LIMIT.min(adaptive_limit);
    top_relevance.truncate(relevance_limit);

    let frontier_limit = adaptive_limit.saturating_sub(top_relevance.len());
    let mut selected_urls = top_relevance
        .iter()
        .map(|entry| entry.candidate.item_url.clone())
        .collect::<HashSet<_>>();

    for entry in sales_relevance_frontier(&scored)
        .into_iter()
        .take(frontier_limit)
    {
        selected_urls.insert(entry.candidate.item_url.clone());
    }

    let mut selected = deduped
        .iter()
        .filter(|candidate| selected_urls.contains(&candidate.item_url))
        .cloned()
        .collect::<Vec<_>>();

    if selected.len() < adaptive_limit {
        for candidate in &deduped {
            if selected_urls.insert(candidate.item_url.clone()) {
                selected.push(candidate.clone());
            }
            if selected.len() == adaptive_limit {
                break;
            }
        }
    }

    selected.truncate(adaptive_limit);
    selected
}

pub fn build_verification_chunks(
    candidates: Vec<Candidate>,
    ozon_name_opt: Option<&str>,
) -> Vec<Vec<Candidate>> {
    if candidates.is_empty() {
        return Vec::new();
    }

    select_screening_candidates(candidates, ozon_name_opt, MAX_VERIFY_CANDIDATES)
        .into_iter()
        .collect::<Vec<_>>()
        .chunks(GRID_CANDIDATE_SIZE)
        .map(|chunk| chunk.to_vec())
        .collect()
}

pub fn find_cheapest(candidates: Vec<Candidate>) -> Option<Candidate> {
    select_best_match(candidates)
}

pub fn summarize_matches(candidates: Vec<Candidate>) -> MatchSummary {
    if candidates.is_empty() {
        return MatchSummary::NoMatch;
    }

    MatchSummary::Cheapest(select_best_match(candidates).expect("non-empty candidates"))
}

pub fn collect_matched_candidates(chunk: &[Candidate], match_ids: &[usize]) -> Vec<Candidate> {
    normalize_match_ids(match_ids, chunk.len())
        .into_iter()
        .map(|id| chunk[id - 1].clone())
        .collect()
}

pub fn prepare_final_review_candidates(candidates: Vec<Candidate>, limit: usize) -> Vec<Candidate> {
    if limit == 0 {
        return Vec::new();
    }

    let deduped = dedupe_candidates_by_url(candidates);
    if deduped.len() <= limit {
        return deduped;
    }

    let late_sales_slots = 2.min(limit.saturating_sub(1));
    let rank_window = limit.saturating_sub(late_sales_slots).max(1).min(deduped.len());
    let mut selected_urls = deduped
        .iter()
        .take(rank_window)
        .map(|candidate| candidate.item_url.clone())
        .collect::<HashSet<_>>();

    let mut highest_sales_late_candidates = deduped
        .iter()
        .skip(rank_window)
        .filter_map(|candidate| parse_sales_value(&candidate.sales).map(|sales_value| (sales_value, candidate)))
        .collect::<Vec<_>>();
    highest_sales_late_candidates.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                b.1.cos_score_permille
                    .cmp(&a.1.cos_score_permille)
            })
            .then_with(|| a.1.item_url.cmp(&b.1.item_url))
    });

    for (_, candidate) in highest_sales_late_candidates {
        selected_urls.insert(candidate.item_url.clone());
        if selected_urls.len() == limit {
            break;
        }
    }

    let mut selected = deduped
        .into_iter()
        .filter(|candidate| selected_urls.contains(&candidate.item_url))
        .collect::<Vec<_>>();
    if selected.len() > limit {
        selected.truncate(limit);
    }
    selected
}

fn dedupe_candidates_by_url(candidates: Vec<Candidate>) -> Vec<Candidate> {
    let mut seen_urls = HashSet::new();
    let mut deduped = Vec::new();
    for candidate in candidates {
        if seen_urls.insert(candidate.item_url.clone()) {
            deduped.push(candidate);
        }
    }
    deduped
}

fn score_candidate_relevance(
    candidate: &Candidate,
    index: usize,
    total: usize,
    ozon_name_opt: Option<&str>,
) -> f32 {
    let rank_score = if total <= 1 {
        1.0
    } else {
        1.0 - (index as f32 / (total - 1) as f32)
    };
    let cos_component = if candidate.cos_score_permille > 0 {
        candidate.cos_score()
    } else {
        rank_score * 0.85
    };
    let title_component = title_overlap_score(&candidate.title, ozon_name_opt);

    (cos_component * 0.5) + (title_component * 0.3) + (rank_score * 0.2)
}

fn title_overlap_score(title: &str, ozon_name_opt: Option<&str>) -> f32 {
    let Some(ozon_name) = ozon_name_opt else {
        return 0.0;
    };

    let tokens = tokenize_search_text(ozon_name);
    if tokens.is_empty() {
        return 0.0;
    }

    let normalized_title = title.to_lowercase();
    let matched = tokens
        .iter()
        .filter(|token| normalized_title.contains(token.as_str()))
        .count();

    matched as f32 / tokens.len() as f32
}

fn tokenize_search_text(value: &str) -> Vec<String> {
    static TOKEN_RE: OnceLock<Regex> = OnceLock::new();
    let token_re = TOKEN_RE
        .get_or_init(|| Regex::new(r"[\p{Han}]+|[A-Za-z0-9]+").expect("invalid token regex"));

    let mut seen = HashSet::new();
    let mut tokens = Vec::new();
    for matched in token_re.find_iter(value) {
        let token = matched.as_str().trim().to_lowercase();
        if token.chars().count() < 2 {
            continue;
        }
        if seen.insert(token.clone()) {
            tokens.push(token);
        }
    }
    tokens
}

fn resolve_screening_limit(
    scored_by_relevance: &[ScoredCandidate],
    requested_limit: usize,
) -> usize {
    let bounded_limit = requested_limit.min(scored_by_relevance.len());
    if bounded_limit <= SCREENING_RELEVANCE_LIMIT
        || scored_by_relevance.len() <= SCREENING_RELEVANCE_LIMIT
    {
        return bounded_limit;
    }

    let top_window = scored_by_relevance
        .iter()
        .take(6)
        .map(|candidate| candidate.relevance_score)
        .collect::<Vec<_>>();
    if top_window.len() < 6 {
        return bounded_limit;
    }

    let top_average = top_window.iter().sum::<f32>() / top_window.len() as f32;
    let edge_score = scored_by_relevance
        .get(ADAPTIVE_SCREENING_LIMIT - 1)
        .map(|candidate| candidate.relevance_score)
        .unwrap_or_default();
    let weak_tail_count = scored_by_relevance
        .iter()
        .skip(ADAPTIVE_SCREENING_LIMIT)
        .take(GRID_CANDIDATE_SIZE)
        .filter(|candidate| candidate.relevance_score <= ADAPTIVE_TAIL_RELEVANCE_CEILING)
        .count();

    if top_average >= ADAPTIVE_TOP_RELEVANCE_AVG
        && edge_score >= ADAPTIVE_EDGE_RELEVANCE_SCORE
        && weak_tail_count >= GRID_CANDIDATE_SIZE.saturating_sub(1)
    {
        return ADAPTIVE_SCREENING_LIMIT.min(bounded_limit);
    }

    bounded_limit
}

fn select_best_match(candidates: Vec<Candidate>) -> Option<Candidate> {
    let mut best_candidate: Option<(usize, Candidate)> = None;

    for (index, candidate) in candidates.into_iter().enumerate() {
        let should_replace = match &best_candidate {
            None => true,
            Some((best_index, best)) => is_better_match_candidate(
                &candidate,
                index,
                best,
                *best_index,
            ),
        };

        if should_replace {
            best_candidate = Some((index, candidate));
        }
    }

    best_candidate.map(|(_, candidate)| candidate)
}

fn compare_optional_f64_desc(left: Option<f64>, right: Option<f64>) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => left.partial_cmp(&right).unwrap_or(Ordering::Equal),
        (Some(_), None) => Ordering::Greater,
        (None, Some(_)) => Ordering::Less,
        (None, None) => Ordering::Equal,
    }
}

fn is_better_match_candidate(
    candidate: &Candidate,
    index: usize,
    current_best: &Candidate,
    current_best_index: usize,
) -> bool {
    candidate
        .cos_score_permille
        .cmp(&current_best.cos_score_permille)
        .then_with(|| {
            compare_optional_f64_desc(
                parse_sales_value(&candidate.sales),
                parse_sales_value(&current_best.sales),
            )
        })
        .then_with(|| current_best_index.cmp(&index))
        .then_with(|| current_best.item_url.cmp(&candidate.item_url))
        .is_gt()
}

fn sales_relevance_frontier(scored: &[ScoredCandidate]) -> Vec<ScoredCandidate> {
    let mut frontier = scored
        .iter()
        .filter(|candidate| {
            candidate.sales_value.is_some()
                && candidate.relevance_score >= MIN_FRONTIER_RELEVANCE_SCORE
        })
        .filter(|candidate| !is_sales_relevance_dominated(candidate, scored))
        .cloned()
        .collect::<Vec<_>>();

    frontier.sort_by(|a, b| {
        b.sales_value
            .partial_cmp(&a.sales_value)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| {
                b.relevance_score
                    .partial_cmp(&a.relevance_score)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| a.index.cmp(&b.index))
    });
    frontier
}

fn is_sales_relevance_dominated(candidate: &ScoredCandidate, pool: &[ScoredCandidate]) -> bool {
    let Some(candidate_sales) = candidate.sales_value else {
        return true;
    };

    pool.iter().any(|other| {
        if other.index == candidate.index {
            return false;
        }
        let Some(other_sales) = other.sales_value else {
            return false;
        };

        let relevance_not_worse = other.relevance_score + f32::EPSILON >= candidate.relevance_score;
        let sales_not_lower = other_sales + f64::EPSILON >= candidate_sales;
        let strictly_better = other.relevance_score > candidate.relevance_score + f32::EPSILON
            || other_sales > candidate_sales + f64::EPSILON;

        relevance_not_worse && sales_not_lower && strictly_better
    })
}
