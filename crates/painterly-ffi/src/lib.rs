//! painterly-core の UniFFI ラッパー（iOS / Swift 向け API）。
//!
//! 画像は PNG / JPEG のバイト列でやり取りする（Swift 側は
//! `UIImage.pngData()` ↔ `UIImage(data:)` と噛み合う最小の共通表現）。
//! `render` はブロッキングなので、Swift 側はバックグラウンドタスクから呼び、
//! 進捗は `RenderObserver` コールバックで受け取る。

use image::RgbImage;
use painterly_core::{Brushes, Callbacks, Params};

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum PainterlyError {
    #[error("{msg}")]
    Failed { msg: String },
}

impl From<String> for PainterlyError {
    fn from(msg: String) -> Self {
        PainterlyError::Failed { msg }
    }
}

/// レンダリングパラメータ。既定値は Python 版 PARAMS_DEFAULT / CLI と同一
#[derive(uniffi::Record, Clone)]
pub struct RenderParams {
    /// 採色・単純化の解像度（長辺 px）。小さいほど大づかみに
    #[uniffi(default = 96)]
    pub pixels: u32,
    /// 描画キャンバスの長辺 px。小さいほど抽象的に
    #[uniffi(default = 360)]
    pub resolution: u32,
    /// 減色数
    #[uniffi(default = 36)]
    pub palette: u32,
    /// 量子化の色空間 "rgb" / "lab"
    #[uniffi(default = "rgb")]
    pub color_space: String,
    /// 減色前のガウシアン σ
    #[uniffi(default = 2.0)]
    pub posterize_blur: f32,
    /// 勾配計算前のガウシアン σ
    #[uniffi(default = 8.0)]
    pub normal_blur: f32,
    /// 基準ブラシ半径（キャンバス px）
    #[uniffi(default = 15.0)]
    pub brush_size: f32,
    /// 輪郭・高密度領域用ブラシ（triangle/flat/soft/oil/pastel/charcoal）
    #[uniffi(default = "triangle")]
    pub hard_brush: String,
    #[uniffi(default = "flat")]
    pub standard_brush: String,
    #[uniffi(default = "soft")]
    pub soft_brush: String,
    /// ストローク密度の倍率
    #[uniffi(default = 1.0)]
    pub strokes_scale: f32,
    /// ウェットブレンディング比率 0..1
    #[uniffi(default = 0.18)]
    pub wet: f32,
    #[uniffi(default = 1.15)]
    pub saturation: f32,
    /// 最終出力の長辺 px
    #[uniffi(default = 1080)]
    pub out_long: u32,
    #[uniffi(default = 42)]
    pub seed: u64,
}

impl Default for RenderParams {
    fn default() -> Self {
        let p = Params::default();
        RenderParams {
            pixels: p.pixels,
            resolution: p.resolution,
            palette: p.palette as u32,
            color_space: p.color_space,
            posterize_blur: p.posterize_blur,
            normal_blur: p.normal_blur,
            brush_size: p.brush_size,
            hard_brush: p.hard_brush,
            standard_brush: p.standard_brush,
            soft_brush: p.soft_brush,
            strokes_scale: p.strokes_scale,
            wet: p.wet,
            saturation: p.saturation,
            out_long: p.out_long,
            seed: p.seed,
        }
    }
}

impl From<RenderParams> for Params {
    fn from(d: RenderParams) -> Self {
        Params {
            pixels: d.pixels,
            resolution: d.resolution,
            palette: d.palette as usize,
            color_space: d.color_space,
            posterize_blur: d.posterize_blur,
            normal_blur: d.normal_blur,
            brush_size: d.brush_size,
            hard_brush: d.hard_brush,
            standard_brush: d.standard_brush,
            soft_brush: d.soft_brush,
            strokes_scale: d.strokes_scale,
            wet: d.wet,
            saturation: d.saturation,
            out_long: d.out_long,
            seed: d.seed,
            ..Params::default()
        }
    }
}

#[derive(uniffi::Record)]
pub struct RenderResult {
    /// 完成画（PNG バイト列）
    pub png: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub strokes: u32,
    pub millis: u64,
}

/// 進捗コールバック。Rust のレンダリングスレッドから呼ばれるため、
/// Swift 実装側で MainActor へのディスパッチを行うこと
#[uniffi::export(callback_interface)]
pub trait RenderObserver: Send + Sync {
    /// 中間画像が出来た時点で呼ばれる（name は "2_quantized" など）
    fn on_stage(&self, name: String, png: Vec<u8>);
    /// 描画の進捗。frac は 0..1、png はその時点のキャンバス
    fn on_progress(&self, frac: f32, png: Vec<u8>);
}

fn encode_png(img: &RgbImage) -> Result<Vec<u8>, PainterlyError> {
    let mut buf = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(img.clone())
        .write_to(&mut buf, image::ImageFormat::Png)
        .map_err(|e| PainterlyError::Failed { msg: format!("png encode: {e}") })?;
    Ok(buf.into_inner())
}

/// プレビュー転送量を抑えるため長辺 max_side に縮小してから PNG 化
fn encode_png_preview(img: &RgbImage, max_side: u32) -> Result<Vec<u8>, PainterlyError> {
    let long = img.width().max(img.height());
    if long <= max_side {
        return encode_png(img);
    }
    let s = max_side as f32 / long as f32;
    let small = image::imageops::resize(
        img,
        (img.width() as f32 * s) as u32,
        (img.height() as f32 * s) as u32,
        image::imageops::FilterType::Triangle,
    );
    encode_png(&small)
}

/// 既定パラメータを返す（Swift 側の初期 UI 状態用）
#[uniffi::export]
pub fn default_params() -> RenderParams {
    RenderParams::default()
}

/// 組み込みブラシ名の一覧
#[uniffi::export]
pub fn builtin_brushes() -> Vec<String> {
    painterly_core::BUILTIN_BRUSHES.iter().map(|s| s.to_string()).collect()
}

/// 画像（PNG/JPEG バイト列）を絵画調にレンダリングする。ブロッキング呼び出し
#[uniffi::export]
pub fn render(
    image_bytes: Vec<u8>,
    params: RenderParams,
    observer: Option<Box<dyn RenderObserver>>,
) -> Result<RenderResult, PainterlyError> {
    let img = image::load_from_memory(&image_bytes)
        .map_err(|e| PainterlyError::Failed { msg: format!("画像を読み込めません: {e}") })?
        .to_rgb8();
    let p: Params = params.into();
    let mut brushes = Brushes::new();
    let t0 = std::time::Instant::now();

    let mut stage_err: Option<PainterlyError> = None;
    let mut on_stage = |name: &str, img: &RgbImage| {
        if let Some(obs) = observer.as_deref() {
            match encode_png_preview(img, 720) {
                Ok(png) => obs.on_stage(name.to_string(), png),
                Err(e) => stage_err = Some(e),
            }
        }
    };
    let mut on_progress = |frac: f32, img: &RgbImage| {
        if let Some(obs) = observer.as_deref() {
            if let Ok(png) = encode_png(img) {
                obs.on_progress(frac, png);
            }
        }
    };

    let res = painterly_core::run_pipeline(
        &img,
        None,
        &p,
        &mut brushes,
        Callbacks {
            on_stage: if observer.is_some() { Some(&mut on_stage) } else { None },
            on_paint_progress: if observer.is_some() { Some(&mut on_progress) } else { None },
        },
        false,
    )
    .map_err(PainterlyError::from)?;

    Ok(RenderResult {
        width: res.final_image.width(),
        height: res.final_image.height(),
        png: encode_png(&res.final_image)?,
        strokes: res.strokes as u32,
        millis: t0.elapsed().as_millis() as u64,
    })
}
