use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use image::imageops::{overlay, FilterType};
use image::{DynamicImage, Rgba, RgbaImage};

const OUTPUT_CANVAS_SIZE: u32 = 1024;
const PRIMARY_OCCUPANCY_RATIO: f32 = 0.84;
const FALLBACK_OCCUPANCY_RATIO: f32 = 0.68;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NormalizedBBox {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl NormalizedBBox {
    fn validate(&self) -> Result<(), String> {
        if !(0.0..=1.0).contains(&self.x) || !(0.0..=1.0).contains(&self.y) {
            return Err("bbox origin must be within [0.0, 1.0]".to_string());
        }
        if self.width <= 0.0 || self.height <= 0.0 {
            return Err("bbox size must be positive".to_string());
        }
        if self.x + self.width > 1.0 || self.y + self.height > 1.0 {
            return Err("bbox must fit within normalized image bounds".to_string());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum BackgroundStrategy {
    RemoveAndWhitefill,
    KeepOriginal,
    TightCropOnly,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SearchImagePlan {
    pub target_product: String,
    pub scene_type: String,
    pub primary_bbox: NormalizedBBox,
    pub fallback_bbox: NormalizedBBox,
    pub background_strategy: BackgroundStrategy,
    pub subject_confidence: f32,
    pub needs_fallback_context: bool,
}

#[derive(Debug, Clone)]
pub struct GeneratedSearchImages {
    pub primary_path: PathBuf,
    pub fallback_path: PathBuf,
}

#[derive(Debug, Deserialize)]
struct RawNormalizedBBox {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

#[derive(Debug, Deserialize)]
struct RawSearchImagePlan {
    target_product: String,
    scene_type: String,
    primary_bbox: RawNormalizedBBox,
    fallback_bbox: RawNormalizedBBox,
    background_strategy: String,
    subject_confidence: f32,
    needs_fallback_context: bool,
}

pub fn parse_search_image_plan(content: &str) -> Result<SearchImagePlan, String> {
    let raw = serde_json::from_str::<RawSearchImagePlan>(content)
        .map_err(|e| format!("parse search image plan failed: {e}"))?;

    let primary_bbox = normalize_bbox(raw.primary_bbox)?;
    let fallback_bbox = normalize_bbox(raw.fallback_bbox)?;
    if fallback_bbox.width < primary_bbox.width || fallback_bbox.height < primary_bbox.height {
        return Err("fallback bbox must not be smaller than primary bbox".to_string());
    }

    Ok(SearchImagePlan {
        target_product: raw.target_product,
        scene_type: raw.scene_type,
        primary_bbox,
        fallback_bbox,
        background_strategy: parse_background_strategy(&raw.background_strategy)?,
        subject_confidence: raw.subject_confidence,
        needs_fallback_context: raw.needs_fallback_context,
    })
}

fn normalize_bbox(raw: RawNormalizedBBox) -> Result<NormalizedBBox, String> {
    let bbox = NormalizedBBox {
        x: raw.x,
        y: raw.y,
        width: raw.width,
        height: raw.height,
    };
    bbox.validate()?;
    Ok(bbox)
}

fn parse_background_strategy(value: &str) -> Result<BackgroundStrategy, String> {
    match value {
        "remove_and_whitefill" => Ok(BackgroundStrategy::RemoveAndWhitefill),
        "keep_original" => Ok(BackgroundStrategy::KeepOriginal),
        "tight_crop_only" => Ok(BackgroundStrategy::TightCropOnly),
        other => Err(format!("unsupported background strategy: {other}")),
    }
}

pub fn generate_search_images(
    source_path: &Path,
    plan: &SearchImagePlan,
    temp_dir: &Path,
    row_key: &str,
) -> Result<GeneratedSearchImages, String> {
    std::fs::create_dir_all(temp_dir).map_err(|e| format!("create temp dir failed: {e}"))?;
    let source = image::open(source_path).map_err(|e| format!("open source image failed: {e}"))?;

    let primary_crop = extract_crop_or_full(&source, &plan.primary_bbox);
    let fallback_crop = extract_crop_or_full(&source, &plan.fallback_bbox);

    let primary_image = render_on_canvas(&primary_crop, PRIMARY_OCCUPANCY_RATIO);
    let fallback_image = render_on_canvas(&fallback_crop, FALLBACK_OCCUPANCY_RATIO);

    let safe_row_key = sanitize_row_key(row_key);
    let primary_path = temp_dir.join(format!("{safe_row_key}-search_primary.png"));
    let fallback_path = temp_dir.join(format!("{safe_row_key}-search_fallback.png"));

    primary_image
        .save(&primary_path)
        .map_err(|e| format!("save primary search image failed: {e}"))?;
    fallback_image
        .save(&fallback_path)
        .map_err(|e| format!("save fallback search image failed: {e}"))?;

    Ok(GeneratedSearchImages {
        primary_path,
        fallback_path,
    })
}

fn extract_crop_or_full(source: &DynamicImage, bbox: &NormalizedBBox) -> DynamicImage {
    crop_from_bbox(source, bbox).unwrap_or_else(|| source.clone())
}

fn crop_from_bbox(source: &DynamicImage, bbox: &NormalizedBBox) -> Option<DynamicImage> {
    bbox.validate().ok()?;

    let source_width = source.width() as f32;
    let source_height = source.height() as f32;

    let x = (bbox.x * source_width).floor().max(0.0) as u32;
    let y = (bbox.y * source_height).floor().max(0.0) as u32;
    let width = (bbox.width * source_width).round().max(1.0) as u32;
    let height = (bbox.height * source_height).round().max(1.0) as u32;

    if x >= source.width() || y >= source.height() {
        return None;
    }

    let crop_width = width.min(source.width().saturating_sub(x));
    let crop_height = height.min(source.height().saturating_sub(y));
    if crop_width == 0 || crop_height == 0 {
        return None;
    }

    Some(source.crop_imm(x, y, crop_width, crop_height))
}

fn render_on_canvas(source: &DynamicImage, occupancy_ratio: f32) -> DynamicImage {
    let mut canvas = DynamicImage::ImageRgba8(RgbaImage::from_pixel(
        OUTPUT_CANVAS_SIZE,
        OUTPUT_CANVAS_SIZE,
        Rgba([255, 255, 255, 255]),
    ));
    let max_side = ((OUTPUT_CANVAS_SIZE as f32) * occupancy_ratio).round() as u32;

    let (target_width, target_height) = fit_within(
        source.width(),
        source.height(),
        max_side.max(1),
        max_side.max(1),
    );
    let resized = source.resize_exact(target_width, target_height, FilterType::Lanczos3);

    let x = ((OUTPUT_CANVAS_SIZE - target_width) / 2) as i64;
    let y = ((OUTPUT_CANVAS_SIZE - target_height) / 2) as i64;
    overlay(&mut canvas, &resized, x, y);

    canvas
}

fn fit_within(width: u32, height: u32, max_width: u32, max_height: u32) -> (u32, u32) {
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

fn sanitize_row_key(value: &str) -> String {
    let sanitized = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>();

    if sanitized.is_empty() {
        "row".to_string()
    } else {
        sanitized
    }
}
