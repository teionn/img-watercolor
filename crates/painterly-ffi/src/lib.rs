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
    /// ストローク長の上限（半径の倍数）。低いと点描風、高いと流れる長いタッチ
    #[uniffi(default = 3.0)]
    pub brush_length: f32,
    /// 色の境を元絵に忠実にする度合い 0..1（高いほど元画像の色境界で止まり色も元絵寄り）
    #[uniffi(default = 0.5)]
    pub color_fidelity: f32,
    /// 細部保持 0..1（平坦部でストロークを短くし細部の塗り潰しを防ぐ）
    #[uniffi(default = 0.4)]
    pub detail_retention: f32,
    /// フォーカス点まわりのディテール強化 0..1（focus_point 指定時に有効）
    #[uniffi(default = 0.0)]
    pub focus_detail: f32,
    /// 細部の輝度を最終出力に戻す強さ 0..1
    #[uniffi(default = 0.0)]
    pub detail_overlay: f32,
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
    /// デプスによるタッチ粗密の強さ 0..1（手前=細かく、奥=粗く。0 で無効）
    #[uniffi(default = 0.5)]
    pub depth_detail: f32,
    /// 深度の手前/奥を反転
    #[uniffi(default = false)]
    pub depth_invert: bool,
    /// 外部デプスマップ（PNG/JPEG バイト列、白=手前）。None なら組み込み推定
    #[uniffi(default = None)]
    pub depth_image: Option<Vec<u8>>,
    /// フォーカス位置 X（画像上の正規化座標 0..1）。Y とセットで指定
    #[uniffi(default = None)]
    pub focus_x: Option<f32>,
    /// フォーカス位置 Y（画像上の正規化座標 0..1）
    #[uniffi(default = None)]
    pub focus_y: Option<f32>,
    /// 焦点から細かさが保たれる深度範囲（小さいほど被写界深度が浅い）
    #[uniffi(default = 0.6)]
    pub focus_range: f32,
    /// ボケ領域の粗さ下限（密度倍率、小さいほど粗い）
    #[uniffi(default = 0.35)]
    pub detail_min: f32,
    /// 焦点近傍の細かさ上限（1 超で焦点付近をさらに細かく）
    #[uniffi(default = 1.15)]
    pub detail_max: f32,
    /// 輪郭線の強さ 0..1（0 で無効）
    #[uniffi(default = 0.4)]
    pub line_strength: f32,
    /// 輪郭線の太さ（キャンバス px）
    #[uniffi(default = 1.0)]
    pub line_width: f32,
    /// 紙のテクスチャの強さ 0..1
    #[uniffi(default = 0.35)]
    pub paper_texture: f32,
    /// 紙の余白（短辺に対する比率、0 = なし）
    #[uniffi(default = 0.0)]
    pub paper_border: f32,
    /// 透明水彩度 0..1（0 = 油彩、1 = 透明顔料のグレーズ）
    #[uniffi(default = 0.0)]
    pub pigment: f32,
    /// エッジ暗色化 0..1（塗りの縁に顔料が溜まる）
    #[uniffi(default = 0.0)]
    pub edge_darken: f32,
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
            brush_length: p.brush_length,
            color_fidelity: p.color_fidelity,
            detail_retention: p.detail_retention,
            focus_detail: p.focus_detail,
            detail_overlay: p.detail_overlay,
            hard_brush: p.hard_brush,
            standard_brush: p.standard_brush,
            soft_brush: p.soft_brush,
            strokes_scale: p.strokes_scale,
            wet: p.wet,
            saturation: p.saturation,
            out_long: p.out_long,
            seed: p.seed,
            depth_detail: p.depth_detail,
            depth_invert: p.depth_invert,
            depth_image: None,
            focus_x: None,
            focus_y: None,
            focus_range: p.focus_range,
            detail_min: p.detail_min,
            detail_max: p.detail_max,
            line_strength: p.line_strength,
            line_width: p.line_width,
            paper_texture: p.paper_texture,
            paper_border: p.paper_border,
            pigment: p.pigment,
            edge_darken: p.edge_darken,
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
            brush_length: d.brush_length,
            color_fidelity: d.color_fidelity,
            detail_retention: d.detail_retention,
            focus_detail: d.focus_detail,
            detail_overlay: d.detail_overlay,
            hard_brush: d.hard_brush,
            standard_brush: d.standard_brush,
            soft_brush: d.soft_brush,
            strokes_scale: d.strokes_scale,
            wet: d.wet,
            saturation: d.saturation,
            out_long: d.out_long,
            seed: d.seed,
            depth_detail: d.depth_detail,
            depth_invert: d.depth_invert,
            focus_point: d.focus_x.zip(d.focus_y),
            focus_range: d.focus_range,
            detail_min: d.detail_min,
            detail_max: d.detail_max,
            line_strength: d.line_strength,
            line_width: d.line_width,
            paper_texture: d.paper_texture,
            paper_border: d.paper_border,
            pigment: d.pigment,
            edge_darken: d.edge_darken,
            external_depth: d
                .depth_image
                .and_then(|bytes| image::load_from_memory(&bytes).ok())
                .map(|img| img.to_luma8()),
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

/// 名前付きプリセット（UI で 1 タップ適用する用）
#[derive(uniffi::Record)]
pub struct PresetInfo {
    pub name: String,
    pub description: String,
    pub params: RenderParams,
}

#[uniffi::export]
pub fn presets() -> Vec<PresetInfo> {
    painterly_core::presets()
        .iter()
        .map(|p| {
            // Params → RenderParams（フォーカス位置と外部デプスはプリセット対象外）
            let d = &p.params;
            PresetInfo {
                name: p.name.to_string(),
                description: p.description.to_string(),
                params: RenderParams {
                    pixels: d.pixels,
                    resolution: d.resolution,
                    palette: d.palette as u32,
                    color_space: d.color_space.clone(),
                    posterize_blur: d.posterize_blur,
                    normal_blur: d.normal_blur,
                    brush_size: d.brush_size,
                    brush_length: d.brush_length,
                    color_fidelity: d.color_fidelity,
                    detail_retention: d.detail_retention,
                    focus_detail: d.focus_detail,
                    detail_overlay: d.detail_overlay,
                    hard_brush: d.hard_brush.clone(),
                    standard_brush: d.standard_brush.clone(),
                    soft_brush: d.soft_brush.clone(),
                    strokes_scale: d.strokes_scale,
                    wet: d.wet,
                    saturation: d.saturation,
                    out_long: d.out_long,
                    seed: d.seed,
                    depth_detail: d.depth_detail,
                    depth_invert: d.depth_invert,
                    depth_image: None,
                    focus_x: None,
                    focus_y: None,
                    focus_range: d.focus_range,
                    detail_min: d.detail_min,
                    detail_max: d.detail_max,
                    line_strength: d.line_strength,
                    line_width: d.line_width,
                    paper_texture: d.paper_texture,
                    paper_border: d.paper_border,
                    pigment: d.pigment,
                    edge_darken: d.edge_darken,
                },
            }
        })
        .collect()
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
