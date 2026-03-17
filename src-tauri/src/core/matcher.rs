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
    price_value: Option<f64>,
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
            price_value: parse_positive_price_value(&candidate.price),
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

    for entry in price_relevance_frontier(&scored)
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
    let mut valid_items = candidates;
    if valid_items.is_empty() {
        return None;
    }
    sort_candidates_by_price(&mut valid_items);
    valid_items
        .into_iter()
        .find(|item| parse_positive_price_value(&item.price).is_some())
}

pub fn summarize_matches(candidates: Vec<Candidate>) -> MatchSummary {
    if candidates.is_empty() {
        return MatchSummary::NoMatch;
    }

    let total_matches = candidates.len();
    match find_cheapest(candidates) {
        Some(cheapest) => MatchSummary::Cheapest(cheapest),
        None => MatchSummary::MatchedButPriceUnavailable { total_matches },
    }
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

    let priced = dedupe_candidates_by_url(candidates)
        .into_iter()
        .filter(|item| parse_positive_price_value(&item.price).is_some())
        .collect::<Vec<_>>();
    if priced.len() <= limit {
        let mut selected = priced;
        sort_candidates_by_price(&mut selected);
        return selected;
    }

    let late_price_slots = 2.min(limit.saturating_sub(1));
    let rank_window = limit.saturating_sub(late_price_slots).max(1).min(priced.len());
    let mut selected_urls = priced
        .iter()
        .take(rank_window)
        .map(|candidate| candidate.item_url.clone())
        .collect::<HashSet<_>>();

    let mut cheapest_late_candidates = priced
        .iter()
        .skip(rank_window)
        .filter_map(|candidate| {
            parse_positive_price_value(&candidate.price).map(|price_value| (price_value, candidate))
        })
        .collect::<Vec<_>>();
    cheapest_late_candidates.sort_by(|a, b| {
        a.0.partial_cmp(&b.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.1.item_url.cmp(&b.1.item_url))
    });

    for (_, candidate) in cheapest_late_candidates {
        selected_urls.insert(candidate.item_url.clone());
        if selected_urls.len() == limit {
            break;
        }
    }

    let mut selected = priced
        .into_iter()
        .filter(|candidate| selected_urls.contains(&candidate.item_url))
        .collect::<Vec<_>>();
    sort_candidates_by_price(&mut selected);
    selected.truncate(limit);
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

fn price_relevance_frontier(scored: &[ScoredCandidate]) -> Vec<ScoredCandidate> {
    let mut frontier = scored
        .iter()
        .filter(|candidate| {
            candidate.price_value.is_some()
                && candidate.relevance_score >= MIN_FRONTIER_RELEVANCE_SCORE
        })
        .filter(|candidate| !is_price_relevance_dominated(candidate, scored))
        .cloned()
        .collect::<Vec<_>>();

    frontier.sort_by(|a, b| {
        a.price_value
            .partial_cmp(&b.price_value)
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

fn is_price_relevance_dominated(candidate: &ScoredCandidate, pool: &[ScoredCandidate]) -> bool {
    let Some(candidate_price) = candidate.price_value else {
        return true;
    };

    pool.iter().any(|other| {
        if other.index == candidate.index {
            return false;
        }
        let Some(other_price) = other.price_value else {
            return false;
        };

        let relevance_not_worse = other.relevance_score + f32::EPSILON >= candidate.relevance_score;
        let price_not_higher = other_price <= candidate_price;
        let strictly_better = other.relevance_score > candidate.relevance_score + f32::EPSILON
            || other_price + f64::EPSILON < candidate_price;

        relevance_not_worse && price_not_higher && strictly_better
    })
}
