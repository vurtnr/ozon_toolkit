use std::collections::{HashMap, HashSet};
use std::env;
use std::io::Cursor;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use image::imageops::{overlay, FilterType};
use image::{DynamicImage, GenericImage, Rgba, RgbaImage};
use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::json;

use super::matcher::GRID_CANDIDATE_SIZE;
use super::search_image::{parse_search_image_plan, SearchImagePlan};
use super::types::Candidate;

const DASHSCOPE_API_URL: &str =
    "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions";
const DASHSCOPE_MODEL_NAME: &str = "qwen3-vl-plus";
const MAX_PARALLEL_TILE_DOWNLOADS: usize = 4;
const DEFAULT_MAX_PARALLEL_VLM_BATCHES: usize = 3;

#[derive(Debug, Deserialize)]
struct VlmResponse {
    #[serde(default)]
    match_ids: Vec<usize>,
}

pub trait VlmClient {
    fn match_candidate_grid(
        &self,
        references: ReferenceImages<'_>,
        candidates: &[Candidate],
        ozon_name_opt: Option<&str>,
    ) -> Result<VlmMatchResult, String>;

    fn match_candidate_grids<'a>(
        &self,
        requests: &[VlmBatchRequest<'a>],
        ozon_name_opt: Option<&str>,
    ) -> Vec<Result<VlmMatchResult, String>> {
        requests
            .iter()
            .map(|request| {
                self.match_candidate_grid(request.references, request.candidates, ozon_name_opt)
            })
            .collect()
    }
}

pub trait SearchImagePlanner {
    fn plan_search_images(
        &self,
        ozon_image_base64: &str,
        ozon_name: &str,
    ) -> Result<SearchImagePlan, String>;
}

#[derive(Debug, Clone, Copy)]
pub struct ReferenceImages<'a> {
    pub primary_reference_image_base64: &'a str,
    pub auxiliary_reference_image_base64: Option<&'a str>,
}

impl<'a> ReferenceImages<'a> {
    pub fn screening(primary_reference_image_base64: &'a str) -> Self {
        Self {
            primary_reference_image_base64,
            auxiliary_reference_image_base64: None,
        }
    }

    pub fn final_review(
        primary_reference_image_base64: &'a str,
        auxiliary_reference_image_base64: &'a str,
    ) -> Self {
        Self {
            primary_reference_image_base64,
            auxiliary_reference_image_base64: Some(auxiliary_reference_image_base64),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct VlmBatchRequest<'a> {
    pub references: ReferenceImages<'a>,
    pub candidates: &'a [Candidate],
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VlmCallTrace {
    pub system_prompt: String,
    pub user_prompt: String,
    pub raw_response_text: String,
    pub grid_jpeg_bytes: Vec<u8>,
    pub candidates: Vec<Candidate>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VlmMatchResult {
    pub match_ids: Vec<usize>,
    pub trace: VlmCallTrace,
}

#[derive(Clone)]
pub struct DashScopeVlmClient {
    client: Client,
    api_key: String,
    tile_cache: Arc<Mutex<HashMap<String, DynamicImage>>>,
}

impl DashScopeVlmClient {
    pub fn new(api_key: impl Into<String>) -> Result<Self, String> {
        let api_key = api_key.into();
        if api_key.trim().is_empty() {
            return Err("❌ 找不到 DASHSCOPE_API_KEY".to_string());
        }

        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|e| format!("初始化 DashScope HTTP 客户端失败: {e}"))?;

        Ok(Self {
            client,
            api_key,
            tile_cache: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub fn from_env() -> Result<Self, String> {
        let api_key =
            env::var("DASHSCOPE_API_KEY").map_err(|_| "❌ 找不到 DASHSCOPE_API_KEY".to_string())?;
        Self::new(api_key)
    }
}

#[derive(Debug)]
struct OwnedVlmBatchRequest {
    primary_reference_image_base64: String,
    auxiliary_reference_image_base64: Option<String>,
    candidates: Vec<Candidate>,
}

fn grid_artifact_unavailable_trace(candidates: &[Candidate]) -> VlmCallTrace {
    VlmCallTrace {
        system_prompt: "[GRID_ARTIFACT_UNAVAILABLE]".to_string(),
        user_prompt: "[GRID_ARTIFACT_UNAVAILABLE]".to_string(),
        raw_response_text:
            "[GRID_ARTIFACT_UNAVAILABLE] unable to download or decode any candidate image into the comparison grid"
                .to_string(),
        grid_jpeg_bytes: Vec::new(),
        candidates: candidates.to_vec(),
    }
}

impl VlmClient for DashScopeVlmClient {
    fn match_candidate_grid(
        &self,
        references: ReferenceImages<'_>,
        candidates: &[Candidate],
        ozon_name_opt: Option<&str>,
    ) -> Result<VlmMatchResult, String> {
        match_candidate_grid_with_shared_cache(
            &self.client,
            &self.api_key,
            &self.tile_cache,
            references,
            candidates,
            ozon_name_opt,
        )
    }

    fn match_candidate_grids<'a>(
        &self,
        requests: &[VlmBatchRequest<'a>],
        ozon_name_opt: Option<&str>,
    ) -> Vec<Result<VlmMatchResult, String>> {
        if requests.len() <= 1 {
            return requests
                .iter()
                .map(|request| {
                    self.match_candidate_grid(request.references, request.candidates, ozon_name_opt)
                })
                .collect();
        }

        let owned_requests = requests
            .iter()
            .map(|request| OwnedVlmBatchRequest {
                primary_reference_image_base64: request
                    .references
                    .primary_reference_image_base64
                    .to_string(),
                auxiliary_reference_image_base64: request
                    .references
                    .auxiliary_reference_image_base64
                    .map(str::to_string),
                candidates: request.candidates.to_vec(),
            })
            .collect::<Vec<_>>();
        let ozon_name = ozon_name_opt.map(str::to_string);

        parallel_map_limited(
            owned_requests,
            resolve_parallel_vlm_batch_limit(requests.len()),
            |request| {
            match_candidate_grid_with_shared_cache(
                &self.client,
                &self.api_key,
                &self.tile_cache,
                ReferenceImages {
                    primary_reference_image_base64: &request.primary_reference_image_base64,
                    auxiliary_reference_image_base64: request
                        .auxiliary_reference_image_base64
                        .as_deref(),
                },
                &request.candidates,
                ozon_name.as_deref(),
            )
            },
        )
    }
}

fn resolve_parallel_vlm_batch_limit(request_count: usize) -> usize {
    request_count.clamp(1, DEFAULT_MAX_PARALLEL_VLM_BATCHES)
}

impl SearchImagePlanner for DashScopeVlmClient {
    fn plan_search_images(
        &self,
        ozon_image_base64: &str,
        ozon_name: &str,
    ) -> Result<SearchImagePlan, String> {
        verify_search_image_plan(&self.client, &self.api_key, ozon_image_base64, ozon_name)
    }
}

pub fn normalize_match_ids(match_ids: &[usize], valid_count: usize) -> Vec<usize> {
    let mut ids = match_ids
        .iter()
        .copied()
        .filter(|id| *id >= 1 && *id <= valid_count)
        .collect::<Vec<_>>();
    ids.sort_unstable();
    ids.dedup();
    ids
}

pub fn parse_vlm_response_content(content: &str, valid_count: usize) -> Vec<usize> {
    if let Ok(parsed) = serde_json::from_str::<VlmResponse>(content) {
        return normalize_match_ids(&parsed.match_ids, valid_count);
    }

    if let Ok(parsed) = serde_json::from_str::<Vec<usize>>(content) {
        return normalize_match_ids(&parsed, valid_count);
    }

    recover_match_ids_from_partial_content(content, valid_count)
}

fn recover_match_ids_from_partial_content(content: &str, valid_count: usize) -> Vec<usize> {
    extract_match_ids_from_array_fragment(content)
        .map(|match_ids| normalize_match_ids(&match_ids, valid_count))
        .unwrap_or_default()
}

fn extract_match_ids_from_array_fragment(content: &str) -> Option<Vec<usize>> {
    let trimmed = content.trim();
    if trimmed.starts_with('[') {
        return Some(parse_usize_array_fragment(trimmed));
    }

    let match_ids_anchor = trimmed
        .find("\"match_ids\"")
        .or_else(|| trimmed.find("match_ids"))?;
    let array_anchor = trimmed[match_ids_anchor..].find('[')? + match_ids_anchor;
    Some(parse_usize_array_fragment(&trimmed[array_anchor..]))
}

fn parse_usize_array_fragment(content: &str) -> Vec<usize> {
    let Some(array_start) = content.find('[') else {
        return Vec::new();
    };
    let array_body = &content[array_start + 1..];
    let array_end = array_body.find(']').unwrap_or(array_body.len());
    array_body[..array_end]
        .split(|ch: char| !ch.is_ascii_digit())
        .filter(|fragment| !fragment.is_empty())
        .filter_map(|fragment| fragment.parse::<usize>().ok())
        .collect()
}

fn parallel_map_limited<T, U, F>(items: Vec<T>, limit: usize, worker: F) -> Vec<U>
where
    T: Send,
    U: Send,
    F: Fn(T) -> U + Sync,
{
    if items.is_empty() {
        return Vec::new();
    }

    let effective_limit = limit.max(1);
    let worker = &worker;
    let mut indexed_results = Vec::with_capacity(items.len());
    let mut iter = items.into_iter().enumerate();

    while let Some(first) = iter.next() {
        let mut batch = vec![first];
        for _ in 1..effective_limit {
            if let Some(item) = iter.next() {
                batch.push(item);
            } else {
                break;
            }
        }

        let mut batch_results = std::thread::scope(|scope| {
            let mut handles = Vec::with_capacity(batch.len());
            for (index, item) in batch {
                let worker = worker;
                handles.push(scope.spawn(move || (index, worker(item))));
            }

            handles
                .into_iter()
                .map(|handle| handle.join().expect("parallel worker panicked"))
                .collect::<Vec<_>>()
        });

        indexed_results.append(&mut batch_results);
    }

    indexed_results.sort_by_key(|(index, _)| *index);
    indexed_results
        .into_iter()
        .map(|(_, value)| value)
        .collect()
}

fn fetch_and_resize(client: &Client, url: &str, size: u32) -> Option<DynamicImage> {
    let response = client
        .get(url)
        .timeout(Duration::from_secs(10))
        .send()
        .ok()?;
    let bytes = response.bytes().ok()?;
    let image = image::load_from_memory(&bytes).ok()?;
    Some(render_image_on_square_tile(&image, size))
}

fn build_tile_cache_key(url: &str, size: u32) -> String {
    format!("{size}:{url}")
}

fn match_candidate_grid_with_shared_cache(
    client: &Client,
    api_key: &str,
    shared_tile_cache: &Arc<Mutex<HashMap<String, DynamicImage>>>,
    references: ReferenceImages<'_>,
    candidates: &[Candidate],
    ozon_name_opt: Option<&str>,
) -> Result<VlmMatchResult, String> {
    if candidates.is_empty() {
        return Ok(VlmMatchResult {
            match_ids: Vec::new(),
            trace: VlmCallTrace {
                system_prompt: String::new(),
                user_prompt: String::new(),
                raw_response_text: String::new(),
                grid_jpeg_bytes: Vec::new(),
                candidates: Vec::new(),
            },
        });
    }

    let Some((grid_base64, grid_jpeg_bytes)) =
        create_grid_artifact_with_shared_cache(client, shared_tile_cache, candidates)
    else {
        return Ok(VlmMatchResult {
            match_ids: Vec::new(),
            trace: grid_artifact_unavailable_trace(candidates),
        });
    };

    verify_with_qwen_vl(
        client,
        api_key,
        references,
        &grid_base64,
        grid_jpeg_bytes,
        candidates,
        ozon_name_opt,
    )
}

fn draw_filled_rect(
    canvas: &mut DynamicImage,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    color: image::Rgba<u8>,
) {
    let x_end = (x + width).min(canvas.width());
    let y_end = (y + height).min(canvas.height());
    for py in y..y_end {
        for px in x..x_end {
            canvas.put_pixel(px, py, color);
        }
    }
}

fn draw_digit(
    canvas: &mut DynamicImage,
    x: u32,
    y: u32,
    digit: u32,
    scale: u32,
    color: image::Rgba<u8>,
) {
    const DIGIT_FONT_3X5: [[[u8; 3]; 5]; 10] = [
        [[1, 1, 1], [1, 0, 1], [1, 0, 1], [1, 0, 1], [1, 1, 1]],
        [[0, 1, 0], [1, 1, 0], [0, 1, 0], [0, 1, 0], [1, 1, 1]],
        [[1, 1, 1], [0, 0, 1], [1, 1, 1], [1, 0, 0], [1, 1, 1]],
        [[1, 1, 1], [0, 0, 1], [1, 1, 1], [0, 0, 1], [1, 1, 1]],
        [[1, 0, 1], [1, 0, 1], [1, 1, 1], [0, 0, 1], [0, 0, 1]],
        [[1, 1, 1], [1, 0, 0], [1, 1, 1], [0, 0, 1], [1, 1, 1]],
        [[1, 1, 1], [1, 0, 0], [1, 1, 1], [1, 0, 1], [1, 1, 1]],
        [[1, 1, 1], [0, 0, 1], [0, 0, 1], [0, 0, 1], [0, 0, 1]],
        [[1, 1, 1], [1, 0, 1], [1, 1, 1], [1, 0, 1], [1, 1, 1]],
        [[1, 1, 1], [1, 0, 1], [1, 1, 1], [0, 0, 1], [1, 1, 1]],
    ];

    let Some(pattern) = DIGIT_FONT_3X5.get(digit as usize) else {
        return;
    };

    for (row_idx, row) in pattern.iter().enumerate() {
        for (col_idx, bit) in row.iter().enumerate() {
            if *bit == 1 {
                draw_filled_rect(
                    canvas,
                    x + col_idx as u32 * scale,
                    y + row_idx as u32 * scale,
                    scale,
                    scale,
                    color,
                );
            }
        }
    }
}

fn draw_tile_index_label(
    canvas: &mut DynamicImage,
    tile_index: usize,
    tile_size: u32,
    grid_size: u32,
) {
    let x = (tile_index as u32 % grid_size) * tile_size;
    let y = (tile_index as u32 / grid_size) * tile_size;

    draw_filled_rect(
        canvas,
        x + 10,
        y + 10,
        46,
        34,
        image::Rgba([255, 255, 255, 220]),
    );
    draw_digit(
        canvas,
        x + 23,
        y + 16,
        (tile_index + 1) as u32,
        4,
        image::Rgba([0, 0, 0, 255]),
    );
}

fn create_grid_artifact_with_shared_cache(
    client: &Client,
    shared_tile_cache: &Arc<Mutex<HashMap<String, DynamicImage>>>,
    candidates: &[Candidate],
) -> Option<(String, Vec<u8>)> {
    let tile_size = 300;
    let candidate_window = candidates
        .iter()
        .take(GRID_CANDIDATE_SIZE)
        .collect::<Vec<_>>();
    let mut resolved_tiles = vec![None; candidate_window.len()];
    let mut missing_jobs = Vec::new();
    let mut seen_missing = HashSet::new();

    {
        let tile_cache = shared_tile_cache.lock().ok()?;
        for (index, candidate) in candidate_window.iter().enumerate() {
            let cache_key = build_tile_cache_key(&candidate.image_url, tile_size);
            if let Some(cached) = tile_cache.get(&cache_key) {
                resolved_tiles[index] = Some(cached.clone());
            } else if seen_missing.insert(cache_key.clone()) {
                missing_jobs.push((cache_key, candidate.image_url.clone()));
            }
        }
    }

    let loaded_tiles = parallel_map_limited(
        missing_jobs,
        MAX_PARALLEL_TILE_DOWNLOADS,
        |(cache_key, image_url)| (cache_key, fetch_and_resize(client, &image_url, tile_size)),
    );

    if !loaded_tiles.is_empty() {
        let mut tile_cache = shared_tile_cache.lock().ok()?;
        for (cache_key, image_opt) in loaded_tiles {
            if let Some(image) = image_opt {
                tile_cache.entry(cache_key).or_insert(image);
            }
        }

        for (index, candidate) in candidate_window.iter().enumerate() {
            if resolved_tiles[index].is_none() {
                let cache_key = build_tile_cache_key(&candidate.image_url, tile_size);
                resolved_tiles[index] = tile_cache.get(&cache_key).cloned();
            }
        }
    }

    create_grid_artifact_from_tiles(candidates, resolved_tiles, tile_size)
}

#[cfg(test)]
fn create_grid_artifact(client: &Client, candidates: &[Candidate]) -> Option<(String, Vec<u8>)> {
    let mut tile_cache = HashMap::new();
    create_grid_artifact_with_loader(candidates, &mut tile_cache, |url, size| {
        fetch_and_resize(client, url, size)
    })
}

#[cfg(test)]
fn create_grid_artifact_with_loader<F>(
    candidates: &[Candidate],
    tile_cache: &mut HashMap<String, DynamicImage>,
    mut load_tile: F,
) -> Option<(String, Vec<u8>)>
where
    F: FnMut(&str, u32) -> Option<DynamicImage>,
{
    let tile_size = 300;
    let grid_size = (GRID_CANDIDATE_SIZE as f64).sqrt() as u32;
    let canvas_size = tile_size * grid_size;
    let canvas_img =
        image::RgbaImage::from_pixel(canvas_size, canvas_size, image::Rgba([255, 255, 255, 255]));
    let mut canvas = DynamicImage::ImageRgba8(canvas_img);

    let mut has_image = false;
    for (index, candidate) in candidates.iter().take(GRID_CANDIDATE_SIZE).enumerate() {
        let cache_key = build_tile_cache_key(&candidate.image_url, tile_size);
        let tile = if let Some(cached) = tile_cache.get(&cache_key) {
            Some(cached.clone())
        } else {
            match load_tile(&candidate.image_url, tile_size) {
                Some(image) => {
                    tile_cache.insert(cache_key, image.clone());
                    Some(image)
                }
                None => None,
            }
        };

        if let Some(image) = tile {
            has_image = true;
            let x = (index as u32 % grid_size) * tile_size;
            let y = (index as u32 / grid_size) * tile_size;
            let _ = canvas.copy_from(&image, x, y);
        }
        draw_tile_index_label(&mut canvas, index, tile_size, grid_size);
    }

    if !has_image {
        return None;
    }

    let mut buffer = Cursor::new(Vec::new());
    canvas
        .write_to(&mut buffer, image::ImageFormat::Jpeg)
        .ok()?;
    let jpeg_bytes = buffer.into_inner();
    Some((
        format!(
            "data:image/jpeg;base64,{}",
            BASE64_STANDARD.encode(&jpeg_bytes)
        ),
        jpeg_bytes,
    ))
}

fn create_grid_artifact_from_tiles(
    candidates: &[Candidate],
    tiles: Vec<Option<DynamicImage>>,
    tile_size: u32,
) -> Option<(String, Vec<u8>)> {
    let grid_size = (GRID_CANDIDATE_SIZE as f64).sqrt() as u32;
    let canvas_size = tile_size * grid_size;
    let canvas_img =
        image::RgbaImage::from_pixel(canvas_size, canvas_size, image::Rgba([255, 255, 255, 255]));
    let mut canvas = DynamicImage::ImageRgba8(canvas_img);

    let mut has_image = false;
    for (index, _) in candidates.iter().take(GRID_CANDIDATE_SIZE).enumerate() {
        if let Some(image) = tiles.get(index).cloned().flatten() {
            has_image = true;
            let x = (index as u32 % grid_size) * tile_size;
            let y = (index as u32 / grid_size) * tile_size;
            let _ = canvas.copy_from(&image, x, y);
        }
        draw_tile_index_label(&mut canvas, index, tile_size, grid_size);
    }

    if !has_image {
        return None;
    }

    let mut buffer = Cursor::new(Vec::new());
    canvas
        .write_to(&mut buffer, image::ImageFormat::Jpeg)
        .ok()?;
    let jpeg_bytes = buffer.into_inner();
    Some((
        format!(
            "data:image/jpeg;base64,{}",
            BASE64_STANDARD.encode(&jpeg_bytes)
        ),
        jpeg_bytes,
    ))
}

#[cfg(test)]
pub fn create_grid_artifact_with_cache_loader<F>(
    candidates: &[Candidate],
    tile_cache: &mut HashMap<String, DynamicImage>,
    load_tile: F,
) -> Option<(String, Vec<u8>)>
where
    F: FnMut(&str, u32) -> Option<DynamicImage>,
{
    create_grid_artifact_with_loader(candidates, tile_cache, load_tile)
}

fn render_image_on_square_tile(source: &DynamicImage, tile_size: u32) -> DynamicImage {
    let mut canvas = DynamicImage::ImageRgba8(RgbaImage::from_pixel(
        tile_size,
        tile_size,
        Rgba([255, 255, 255, 255]),
    ));
    let (target_width, target_height) =
        fit_within_dimensions(source.width(), source.height(), tile_size, tile_size);
    let resized = source.resize_exact(target_width, target_height, FilterType::Lanczos3);
    let x = ((tile_size - target_width) / 2) as i64;
    let y = ((tile_size - target_height) / 2) as i64;
    overlay(&mut canvas, &resized, x, y);
    canvas
}

fn fit_within_dimensions(width: u32, height: u32, max_width: u32, max_height: u32) -> (u32, u32) {
    if width == 0 || height == 0 {
        return (1, 1);
    }

    let width_ratio = max_width as f32 / width as f32;
    let height_ratio = max_height as f32 / height as f32;
    let scale = width_ratio.min(height_ratio);

    let target_width = ((width as f32) * scale).round().max(1.0) as u32;
    let target_height = ((height as f32) * scale).round().max(1.0) as u32;
    (target_width, target_height)
}

fn sanitize_title(title: &str) -> String {
    title
        .replace(['\n', '\r'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn build_candidate_title_context(candidates: &[Candidate]) -> String {
    candidates
        .iter()
        .take(GRID_CANDIDATE_SIZE)
        .enumerate()
        .map(|(index, candidate)| {
            format!(
                "编号{} 标题参考：【{}】",
                index + 1,
                sanitize_title(&candidate.title)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn build_screening_prompts(
    candidates: &[Candidate],
    ozon_name_opt: Option<&str>,
) -> (String, String) {
    let valid_count = candidates.len();
    let system_prompt =
        "你是SKU候选召回器。目标是从候选图中尽量召回所有可能同款或高度相似、值得进一步复核的候选。宁可少量误召回，也不要漏掉潜在同款。只返回JSON。"
            .to_string();
    let product_name_context = ozon_name_opt
        .map(|name| format!("🚨 商品名称参考：【{}】。\n", name))
        .unwrap_or_default();
    let title_context = build_candidate_title_context(candidates);
    let user_prompt = format!(
        "图 A 是当前用于 1688 以图搜款的搜索参考图。图 B 是候选商品九宫格（编号 1 到 9）。\n\
        🚨 图 B 中只有前 {} 个格子有商品！\n\
        🚨 每个格子左上角有白底黑字编号，必须严格按图中编号返回，禁止用方位词推断编号。\n\
        {}\
        候选标题参考：\n{}\n\
        任务：\n\
        1. 这是初筛召回，不是最终定版。请召回所有可能同款或高度相似、值得进入下一轮严格复核的候选。\n\
        2. 只要商品主体结构、核心部件、连接方式大体一致，或疑似同模具/同系列变体，就应该先召回。\n\
        3. 只有在主体结构明显不同、几乎不可能是同款时才排除。\n\
        4. 若商品名称参考偏泛类目或与候选语言不同，以图片主体结构为准，不要因名称泛化而漏召回。\n\
        5. 宁可多召回少量疑似项，也不要漏掉潜在真实同款。\n\
        6. 只输出 JSON，不要输出 reasoning 字段，不要解释，不要 markdown。\n\
        严格输出 JSON：\n\
        {{\n  \"match_ids\": [1,2]\n}}",
        valid_count, product_name_context, title_context
    );

    (system_prompt, user_prompt)
}

fn build_final_review_prompts(
    candidates: &[Candidate],
    ozon_name_opt: Option<&str>,
    has_auxiliary_reference: bool,
) -> (String, String) {
    let valid_count = candidates.len();
    let system_prompt =
        "你是SKU同款鉴定器。必须严格匹配同一物理模具，宁可漏判也不能误判。只返回JSON。".to_string();
    let candidate_title_context = candidates
        .iter()
        .enumerate()
        .map(|(index, candidate)| {
            format!(
                "编号{} 标题参考：【{}】",
                index + 1,
                sanitize_title(&candidate.title)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let candidate_title_context = if candidate_title_context.is_empty() {
        String::new()
    } else {
        format!("候选标题参考：\n{}\n", candidate_title_context)
    };

    let reference_context = if has_auxiliary_reference {
        "图 A 是 1688 搜图使用的处理后搜索图。图 B 是原始商品图（仅作辅助复核）。图 C 是候选商品九宫格（编号 1 到 9）。"
    } else {
        "图 A 是 1688 搜图使用的处理后搜索图。图 B 是候选商品九宫格（编号 1 到 9）。"
    };

    let user_prompt = if let Some(name) = ozon_name_opt {
        format!(
            "{}\n\
            🚨 候选图中只有前 {} 个格子有商品！\n\
            🚨 每个格子左上角有白底黑字编号，必须严格按图中编号返回，禁止用方位词推断编号。\n\
            🚨 商品名称参考：【{}】。\n\
            {}\
            规则：\n\
            1. 以图 A 的处理后搜索图为主参考，必要时用图 B 的原始商品图辅助确认，忽略背景、文字、水印、角度。\n\
            2. 图 B 中的营销文案、赠品、配件拆解图、背景植物或汽车、悬挂绳、挂钩、插头、包装盒、摆拍道具都不是必须一致的判定条件；除非这些部件明确属于商品主体本体，否则忽略。\n\
            3. 商品名称只作辅助；若名称偏泛类目或与候选语言不同，仍以图片主体结构为准。\n\
            4. 仅当核心结构、部件形态、连接方式都一致才算同款；拿不准必须排除。\n\
            5. 只输出 JSON，不要输出 reasoning 字段，不要解释，不要 markdown。\n\
            6. 若是同款就返回编号；无同款返回空数组。\n\
            严格输出 JSON：\n\
            {{\n  \"match_ids\": [1]\n}}",
            reference_context, valid_count, name, candidate_title_context
        )
    } else {
        format!(
            "{}\n\
            🚨 候选图中只有前 {} 个格子有商品！\n\
            🚨 每个格子左上角有白底黑字编号，必须严格按图中编号返回，禁止用方位词推断编号。\n\
            {}\
            规则：\n\
            1. 以图 A 的处理后搜索图为主参考，必要时用图 B 的原始商品图辅助确认，忽略背景、文字、水印、角度。\n\
            2. 参考图中的营销文案、赠品、配件拆解图、背景、悬挂绳、挂钩、插头、包装盒、摆拍道具都不是必须一致的判定条件；除非这些部件明确属于商品主体本体，否则忽略。\n\
            3. 商品名称只作辅助；若名称偏泛类目或与候选语言不同，仍以图片主体结构为准。\n\
            4. 仅当核心结构、部件形态、连接方式都一致才算同款；拿不准必须排除。\n\
            5. 只输出 JSON，不要输出 reasoning 字段，不要解释，不要 markdown。\n\
            6. 若是同款就返回编号；无同款返回空数组。\n\
            严格输出 JSON：\n\
            {{\n  \"match_ids\": [1]\n}}",
            reference_context, valid_count, candidate_title_context
        )
    };

    (system_prompt, user_prompt)
}

fn build_prompts(
    candidates: &[Candidate],
    ozon_name_opt: Option<&str>,
    has_auxiliary_reference: bool,
) -> (String, String) {
    if has_auxiliary_reference || candidates.len() <= 1 {
        build_final_review_prompts(candidates, ozon_name_opt, has_auxiliary_reference)
    } else {
        build_screening_prompts(candidates, ozon_name_opt)
    }
}

fn build_search_image_planning_prompts(ozon_name: &str) -> (String, String) {
    let system_prompt =
        "你是商品搜索图规划器。你必须识别真正要售卖的商品主体，并只返回严格 JSON。".to_string();
    let user_prompt = format!(
        "输入是一张 ozon 商品图，商品标题是【{}】。\n\
        请识别真正需要去 1688 以图搜款的商品主体，并输出严格 JSON。\n\
        规则：\n\
        1. primary_bbox 用于首搜，要更聚焦商品主体。\n\
        2. fallback_bbox 用于二次搜，要比 primary_bbox 更保守，保留更多结构上下文。\n\
        3. bbox 必须是 0 到 1 的归一化坐标。\n\
        4. 如果不确定，请保守，不要胡乱推断。\n\
        5. 只返回 JSON，不要返回 markdown。\n\
        JSON 结构：\n\
        {{\n\
          \"target_product\": \"...\",\n\
          \"scene_type\": \"single_product\",\n\
          \"primary_bbox\": {{\"x\": 0.1, \"y\": 0.1, \"width\": 0.5, \"height\": 0.5}},\n\
          \"fallback_bbox\": {{\"x\": 0.05, \"y\": 0.05, \"width\": 0.7, \"height\": 0.7}},\n\
          \"background_strategy\": \"remove_and_whitefill\",\n\
          \"subject_confidence\": 0.9,\n\
          \"needs_fallback_context\": true\n\
        }}",
        ozon_name
    );

    (system_prompt, user_prompt)
}

fn verify_with_qwen_vl(
    client: &Client,
    api_key: &str,
    references: ReferenceImages<'_>,
    grid_base64: &str,
    grid_jpeg_bytes: Vec<u8>,
    candidates: &[Candidate],
    ozon_name_opt: Option<&str>,
) -> Result<VlmMatchResult, String> {
    let valid_count = candidates.len();
    let has_auxiliary_reference = references.auxiliary_reference_image_base64.is_some();
    let (system_prompt, user_prompt) =
        build_prompts(candidates, ozon_name_opt, has_auxiliary_reference);
    let mut content = vec![
        json!({ "type": "text", "text": user_prompt }),
        json!({
            "type": "image_url",
            "image_url": { "url": references.primary_reference_image_base64 }
        }),
    ];
    if let Some(auxiliary_reference_image_base64) = references.auxiliary_reference_image_base64 {
        content.push(json!({
            "type": "image_url",
            "image_url": { "url": auxiliary_reference_image_base64 }
        }));
    }
    content.push(json!({
        "type": "image_url",
        "image_url": { "url": grid_base64 }
    }));
    let payload = json!({
        "model": DASHSCOPE_MODEL_NAME,
        "temperature": 0.01,
        "max_tokens": 220,
        "response_format": { "type": "json_object" },
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": content }
        ]
    });

    let response = client
        .post(DASHSCOPE_API_URL)
        .header("Authorization", format!("Bearer {api_key}"))
        .json(&payload)
        .send()
        .map_err(|e| format!("💥 大模型网络请求失败: {e}"))?;

    let status = response.status();
    if !status.is_success() {
        let error_text = response.text().unwrap_or_default();
        return Err(format!(
            "💥 大模型 API 崩溃或被限流！状态码: {} 详情: {}",
            status, error_text
        ));
    }

    let body = response
        .json::<serde_json::Value>()
        .map_err(|e| format!("parse dashscope response failed: {e}"))?;
    let content = extract_message_content_text(&body["choices"][0]["message"]["content"]);

    Ok(VlmMatchResult {
        match_ids: parse_vlm_response_content(&content, valid_count),
        trace: VlmCallTrace {
            system_prompt,
            user_prompt,
            raw_response_text: content,
            grid_jpeg_bytes,
            candidates: candidates.to_vec(),
        },
    })
}

fn extract_message_content_text(content: &serde_json::Value) -> String {
    if let Some(text) = content.as_str() {
        return text.to_string();
    }

    if let Some(items) = content.as_array() {
        let mut parts = Vec::new();
        for item in items {
            if let Some(text) = item.get("text").and_then(|value| value.as_str()) {
                parts.push(text.to_string());
            } else if let Some(text) = item.get("content").and_then(|value| value.as_str()) {
                parts.push(text.to_string());
            }
        }
        if !parts.is_empty() {
            return parts.join("\n");
        }
    }

    content.to_string()
}

fn verify_search_image_plan(
    client: &Client,
    api_key: &str,
    ozon_image_base64: &str,
    ozon_name: &str,
) -> Result<SearchImagePlan, String> {
    let (system_prompt, user_prompt) = build_search_image_planning_prompts(ozon_name);
    let payload = json!({
        "model": DASHSCOPE_MODEL_NAME,
        "temperature": 0.01,
        "max_tokens": 500,
        "response_format": { "type": "json_object" },
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": [
                { "type": "text", "text": user_prompt },
                { "type": "image_url", "image_url": { "url": ozon_image_base64 } }
            ]}
        ]
    });

    let response = client
        .post(DASHSCOPE_API_URL)
        .header("Authorization", format!("Bearer {api_key}"))
        .json(&payload)
        .send()
        .map_err(|e| format!("💥 搜索图规划请求失败: {e}"))?;

    let status = response.status();
    if !status.is_success() {
        let error_text = response.text().unwrap_or_default();
        return Err(format!(
            "💥 搜索图规划 API 失败！状态码: {} 详情: {}",
            status, error_text
        ));
    }

    let body = response
        .json::<serde_json::Value>()
        .map_err(|e| format!("parse dashscope planner response failed: {e}"))?;
    let content = body["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or_default();

    parse_search_image_plan(content)
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use std::thread;
    use std::time::Duration;

    use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
    use reqwest::blocking::Client;

    use super::{
        build_prompts, build_screening_prompts, create_grid_artifact, fit_within_dimensions,
        parallel_map_limited, Candidate, ReferenceImages, VlmBatchRequest, VlmCallTrace,
        VlmClient, VlmMatchResult,
    };

    fn candidate(title: &str) -> Candidate {
        Candidate {
            title: title.to_string(),
            price: "¥1.00".to_string(),
            item_url: "https://detail.1688.com/offer/1.html".to_string(),
            image_url: "https://img.1688.com/1.jpg".to_string(),
            cos_score_permille: 0,
        }
    }

    #[test]
    fn screening_prompt_is_recall_oriented_and_includes_titles() {
        let (_, user_prompt) = build_prompts(
            &[
                candidate("Portable travel bag"),
                candidate("Waterproof duffel bag"),
            ],
            Some("ozon bag"),
            false,
        );

        assert!(user_prompt.contains("召回所有可能同款或高度相似"));
        assert!(user_prompt.contains("搜索参考图"));
        assert!(user_prompt.contains("编号1"));
        assert!(user_prompt.contains("Portable travel bag"));
        assert!(user_prompt.contains("编号2"));
    }

    #[test]
    fn screening_prompt_keeps_image_structure_as_source_of_truth_when_title_is_generic() {
        let (_, user_prompt) = build_screening_prompts(
            &[
                candidate("Marine boarding ladder"),
                candidate("Inflatable boat ladder"),
            ],
            Some("Accessories and components"),
        );

        assert!(user_prompt.contains("名称参考偏泛类目或与候选语言不同"));
    }

    #[test]
    fn screening_prompt_requests_match_ids_only_without_reasoning() {
        let (_, user_prompt) = build_screening_prompts(
            &[candidate("Portable travel bag"), candidate("Waterproof duffel bag")],
            Some("ozon bag"),
        );

        assert!(user_prompt.contains("\"match_ids\""));
        assert!(user_prompt.contains("不要输出 reasoning"));
        assert!(!user_prompt.contains("\"reasoning\""));
    }

    #[test]
    fn single_candidate_prompt_remains_strict_for_final_review() {
        let (_, user_prompt) =
            build_prompts(&[candidate("Portable travel bag")], Some("ozon bag"), false);

        assert!(user_prompt.contains("仅当核心结构、部件形态、连接方式都一致才算同款"));
        assert!(!user_prompt.contains("召回所有可能同款或高度相似"));
    }

    #[test]
    fn final_review_prompt_mentions_original_image_as_auxiliary_reference() {
        let (_, user_prompt) =
            build_prompts(&[candidate("Portable travel bag")], Some("ozon bag"), true);

        assert!(user_prompt.contains("原始商品图（仅作辅助复核）"));
        assert!(user_prompt.contains("以图 A 的处理后搜索图为主参考"));
        assert!(user_prompt.contains("营销文案"));
        assert!(user_prompt.contains("赠品"));
        assert!(user_prompt.contains("配件拆解图"));
        assert!(user_prompt.contains("悬挂绳"));
        assert!(!user_prompt.contains("\"reasoning\""));
    }

    #[test]
    fn multi_candidate_final_review_with_auxiliary_reference_remains_strict() {
        let (_, user_prompt) = build_prompts(
            &[
                candidate("Portable travel bag"),
                candidate("Waterproof duffel bag"),
            ],
            Some("ozon bag"),
            true,
        );

        assert!(user_prompt.contains("仅当核心结构、部件形态、连接方式都一致才算同款"));
        assert!(!user_prompt.contains("召回所有可能同款或高度相似"));
        assert!(user_prompt.contains("Portable travel bag"));
        assert!(user_prompt.contains("Waterproof duffel bag"));
    }

    #[test]
    fn fit_within_dimensions_preserves_aspect_ratio() {
        assert_eq!(fit_within_dimensions(400, 200, 300, 300), (300, 150));
        assert_eq!(fit_within_dimensions(200, 400, 300, 300), (150, 300));
    }

    #[test]
    fn create_grid_artifact_supports_webp_candidates() {
        // Small WebP fixture.
        let webp_bytes = BASE64_STANDARD
            .decode("UklGRsgAAABXRUJQVlA4WAoAAAAQAAAABwAABwAAQUxQSD0AAAABcBPbtlrtLswcMelAB1NFMtBFJOSj0IgIIMhjgOQ/fVOglFfzgXETbuOBx7iFq4O3ucgFkH7nXwIQZx4AAFZQOCBkAAAAMAIAnQEqCAAIAAIANCWwAnS6AHegAwHEzoAA/u5KlE6pisQlFsqEcb2S6FnD+NEJBz3v/i5qrH2wFiVUf+Le+/2OTLNufKu1v3X/q5sY6z+kNnmfIbGZpkP+jz2f/t/+QAAAAA==")
            .expect("valid webp fixture");
        assert!(
            image::load_from_memory(&webp_bytes).is_ok(),
            "fixture must decode as webp"
        );
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test server");
        let address = listener.local_addr().expect("read local addr");
        let response_body = webp_bytes.clone();

        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept request");
            let mut request_buffer = [0_u8; 1024];
            let _ = stream.read(&mut request_buffer);
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: image/webp\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                response_body.len()
            );
            stream.write_all(header.as_bytes()).expect("write header");
            stream.write_all(&response_body).expect("write body");
            stream.flush().expect("flush body");
        });

        let client = Client::builder().build().expect("build client");
        let candidates = vec![Candidate {
            title: "sample webp".to_string(),
            price: "¥1.00".to_string(),
            item_url: "https://detail.1688.com/offer/1.html".to_string(),
            image_url: format!("http://{address}/sample.webp"),
            cos_score_permille: 0,
        }];

        let artifact = create_grid_artifact(&client, &candidates);

        server.join().expect("server should exit cleanly");
        assert!(
            artifact.is_some(),
            "webp candidate should build grid artifact"
        );
    }

    #[test]
    fn parallel_map_limited_preserves_order_and_caps_concurrency() {
        let current = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);

        let results = parallel_map_limited(vec![1, 2, 3, 4], 2, |value| {
            let active = current.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(active, Ordering::SeqCst);
            thread::sleep(Duration::from_millis(30));
            current.fetch_sub(1, Ordering::SeqCst);
            value * 10
        });

        assert_eq!(results, vec![10, 20, 30, 40]);
        assert_eq!(peak.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn resolve_parallel_vlm_batch_limit_uses_three_way_parallelism_by_default() {
        assert_eq!(super::resolve_parallel_vlm_batch_limit(0), 1);
        assert_eq!(super::resolve_parallel_vlm_batch_limit(1), 1);
        assert_eq!(super::resolve_parallel_vlm_batch_limit(2), 2);
        assert_eq!(super::resolve_parallel_vlm_batch_limit(3), 3);
        assert_eq!(super::resolve_parallel_vlm_batch_limit(4), 3);
    }

    #[derive(Default)]
    struct OrderedBatchVlm {
        calls: Mutex<Vec<String>>,
        replies: Mutex<VecDeque<Vec<usize>>>,
    }

    impl OrderedBatchVlm {
        fn with_replies(replies: Vec<Vec<usize>>) -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
                replies: Mutex::new(VecDeque::from(replies)),
            }
        }
    }

    impl VlmClient for OrderedBatchVlm {
        fn match_candidate_grid(
            &self,
            references: ReferenceImages<'_>,
            candidates: &[Candidate],
            _ozon_name_opt: Option<&str>,
        ) -> Result<VlmMatchResult, String> {
            self.calls.lock().expect("calls lock").push(format!(
                "{}:{}",
                references.primary_reference_image_base64,
                candidates.len()
            ));
            let match_ids = self
                .replies
                .lock()
                .expect("replies lock")
                .pop_front()
                .unwrap_or_default();

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
    fn default_batch_matching_keeps_request_order() {
        let vlm = OrderedBatchVlm::with_replies(vec![vec![1], vec![2]]);
        let first_candidates = vec![candidate("Portable travel bag")];
        let second_candidates = vec![
            candidate("Waterproof duffel bag"),
            candidate("Outdoor carry bag"),
        ];
        let requests = vec![
            VlmBatchRequest {
                references: ReferenceImages::screening("ref-a"),
                candidates: &first_candidates,
            },
            VlmBatchRequest {
                references: ReferenceImages::screening("ref-b"),
                candidates: &second_candidates,
            },
        ];

        let results = vlm.match_candidate_grids(&requests, Some("ozon bag"));

        assert_eq!(results.len(), 2);
        assert_eq!(
            results[0].as_ref().expect("first result").match_ids,
            vec![1]
        );
        assert_eq!(
            results[1].as_ref().expect("second result").match_ids,
            vec![2]
        );
        assert_eq!(
            vlm.calls.lock().expect("calls lock").clone(),
            vec!["ref-a:1".to_string(), "ref-b:2".to_string()]
        );
    }
}
