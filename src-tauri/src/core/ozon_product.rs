use regex::Regex;
use reqwest::blocking::Client;
use reqwest::Url;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OzonResolutionFailure {
    InvalidUrl,
    Unavailable,
    MissingTitle,
    MissingImage,
    FetchFailed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OzonProductResolution {
    pub title: String,
    pub image_url: String,
    pub image_bytes: Vec<u8>,
}

pub fn classify_ozon_url_mode(value: &str) -> bool {
    parse_product_url(value).is_some()
}

pub fn resolve_ozon_product(
    client: &Client,
    product_url: &str,
) -> Result<OzonProductResolution, OzonResolutionFailure> {
    let parsed_url = parse_product_url(product_url).ok_or(OzonResolutionFailure::InvalidUrl)?;
    let response = client
        .get(parsed_url.clone())
        .send()
        .map_err(|e| OzonResolutionFailure::FetchFailed(format!("fetch product page failed: {e}")))?;

    let status = response.status();
    if matches!(
        status.as_u16(),
        403 | 404 | 410 | 451 | 500 | 502 | 503 | 504
    ) {
        return Err(OzonResolutionFailure::Unavailable);
    }
    if !status.is_success() {
        return Err(OzonResolutionFailure::FetchFailed(format!(
            "unexpected product page status: {status}"
        )));
    }

    let html = response
        .text()
        .map_err(|e| OzonResolutionFailure::FetchFailed(format!("read product page failed: {e}")))?;

    if is_unavailable_html(&html) {
        return Err(OzonResolutionFailure::Unavailable);
    }

    let title = extract_title(&html).ok_or(OzonResolutionFailure::MissingTitle)?;
    let image_url = extract_image_url(&parsed_url, &html).ok_or(OzonResolutionFailure::MissingImage)?;
    let image_bytes = download_image_bytes(client, &image_url)?;

    Ok(OzonProductResolution {
        title,
        image_url,
        image_bytes,
    })
}

fn parse_product_url(value: &str) -> Option<Url> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }

    let url = Url::parse(trimmed).ok()?;
    let host = url.host_str()?;
    if !is_allowed_ozon_host(host) {
        return None;
    }

    let mut segments = url.path_segments()?;
    let first = segments.next()?;
    let second = segments.next()?;
    if first != "product" || second.trim().is_empty() {
        return None;
    }

    Some(url)
}

fn is_allowed_ozon_host(host: &str) -> bool {
    let normalized = host.trim().to_ascii_lowercase();
    normalized == "ozon.ru"
        || normalized.ends_with(".ozon.ru")
        || matches!(normalized.as_str(), "localhost" | "127.0.0.1" | "::1")
}

fn is_unavailable_html(html: &str) -> bool {
    let normalized = html.to_lowercase();
    [
        "такого товара нет",
        "страница не найдена",
        "извините, такой страницы нет",
        "товар закончился",
        "нет в наличии",
        "не удалось найти товар",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
}

fn extract_title(html: &str) -> Option<String> {
    extract_product_fields_from_json_ld(html)
        .0
        .or_else(|| extract_open_graph_meta(html, "og:title"))
        .or_else(|| extract_title_tag(html))
        .map(|value| normalize_text(&value))
        .filter(|value| !value.is_empty())
}

fn extract_image_url(base_url: &Url, html: &str) -> Option<String> {
    let raw = extract_product_fields_from_json_ld(html)
        .1
        .or_else(|| extract_open_graph_meta(html, "og:image"))?;
    base_url.join(raw.trim()).ok().map(|url| url.to_string())
}

fn extract_product_fields_from_json_ld(html: &str) -> (Option<String>, Option<String>) {
    let script_re = Regex::new(
        r#"(?is)<script[^>]*type=["']application/ld\+json["'][^>]*>(.*?)</script>"#,
    )
    .expect("json-ld regex should compile");

    for captures in script_re.captures_iter(html) {
        let Some(content) = captures.get(1).map(|value| value.as_str().trim()) else {
            continue;
        };
        let Ok(parsed) = serde_json::from_str::<Value>(content) else {
            continue;
        };
        if let Some((title, image)) = find_product_fields_in_json_value(&parsed) {
            return (title, image);
        }
    }

    (None, None)
}

fn find_product_fields_in_json_value(value: &Value) -> Option<(Option<String>, Option<String>)> {
    match value {
        Value::Array(items) => {
            for item in items {
                if let Some(found) = find_product_fields_in_json_value(item) {
                    return Some(found);
                }
            }
            None
        }
        Value::Object(map) => {
            if looks_like_product(map.get("@type")) {
                return Some((
                    map.get("name")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                    extract_first_image_value(map.get("image")),
                ));
            }

            if let Some(graph) = map.get("@graph") {
                return find_product_fields_in_json_value(graph);
            }

            None
        }
        _ => None,
    }
}

fn looks_like_product(value: Option<&Value>) -> bool {
    match value {
        Some(Value::String(kind)) => kind.eq_ignore_ascii_case("product"),
        Some(Value::Array(items)) => items.iter().any(|item| {
            item.as_str()
                .map(|value| value.eq_ignore_ascii_case("product"))
                .unwrap_or(false)
        }),
        _ => false,
    }
}

fn extract_first_image_value(value: Option<&Value>) -> Option<String> {
    match value {
        Some(Value::String(url)) => Some(url.to_string()),
        Some(Value::Array(items)) => items.iter().find_map(|item| match item {
            Value::String(url) => Some(url.to_string()),
            Value::Object(map) => map
                .get("url")
                .and_then(Value::as_str)
                .map(str::to_string),
            _ => None,
        }),
        Some(Value::Object(map)) => map
            .get("url")
            .and_then(Value::as_str)
            .map(str::to_string),
        _ => None,
    }
}

fn extract_open_graph_meta(html: &str, property: &str) -> Option<String> {
    let pattern = format!(
        r#"(?is)<meta[^>]+property=["']{}["'][^>]+content=["']([^"']+)["'][^>]*>"#,
        regex::escape(property)
    );
    let meta_re = Regex::new(&pattern).expect("open graph regex should compile");
    meta_re
        .captures(html)
        .and_then(|captures| captures.get(1).map(|value| value.as_str().to_string()))
}

fn extract_title_tag(html: &str) -> Option<String> {
    let title_re = Regex::new(r#"(?is)<title[^>]*>(.*?)</title>"#)
        .expect("title regex should compile");
    title_re
        .captures(html)
        .and_then(|captures| captures.get(1).map(|value| value.as_str().to_string()))
}

fn normalize_text(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn download_image_bytes(client: &Client, image_url: &str) -> Result<Vec<u8>, OzonResolutionFailure> {
    let response = client
        .get(image_url)
        .send()
        .map_err(|e| OzonResolutionFailure::FetchFailed(format!("fetch image failed: {e}")))?;
    let status = response.status();
    if !status.is_success() {
        return Err(OzonResolutionFailure::FetchFailed(format!(
            "unexpected image status: {status}"
        )));
    }
    response
        .bytes()
        .map(|bytes| bytes.to_vec())
        .map_err(|e| OzonResolutionFailure::FetchFailed(format!("read image bytes failed: {e}")))
}
