//! Tauri バックエンド: painterly-core をバックグラウンドスレッドで走らせ、
//! 中間画像と描画進捗をイベントでフロントエンドへ流す。
//! Python 版 gui.py（PySide6）の役割を置き換える。

use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use base64::Engine as _;
use image::RgbImage;
use painterly_core::{Brushes, Callbacks, Params};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

/// レンダリング状態（多重起動の防止と、保存用に最後の完成画を保持）
#[derive(Default)]
struct RenderState {
    busy: AtomicBool,
    final_image: Mutex<Option<RgbImage>>,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(default)]
struct ParamsDto {
    pixels: u32,
    resolution: u32,
    palette: usize,
    color_space: String,
    posterize_blur: f32,
    normal_blur: f32,
    brush_size: f32,
    hard_brush: String,
    standard_brush: String,
    soft_brush: String,
    strokes_scale: f32,
    wet: f32,
    saturation: f32,
    out_long: u32,
    seed: u64,
    depth_detail: f32,
    depth_invert: bool,
    focus_x: Option<f32>,
    focus_y: Option<f32>,
    focus_range: f32,
    detail_min: f32,
    detail_max: f32,
    line_strength: f32,
    line_width: f32,
    paper_texture: f32,
    paper_border: f32,
    pigment: f32,
    edge_darken: f32,
}

impl Default for ParamsDto {
    fn default() -> Self {
        let p = Params::default();
        ParamsDto {
            pixels: p.pixels,
            resolution: p.resolution,
            palette: p.palette,
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
            depth_detail: p.depth_detail,
            depth_invert: p.depth_invert,
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

impl From<ParamsDto> for Params {
    fn from(d: ParamsDto) -> Self {
        Params {
            pixels: d.pixels,
            resolution: d.resolution,
            palette: d.palette,
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
            ..Params::default()
        }
    }
}

/// ParamsDto を Params から作る（プリセット送出用）
impl From<&Params> for ParamsDto {
    fn from(p: &Params) -> Self {
        ParamsDto {
            pixels: p.pixels,
            resolution: p.resolution,
            palette: p.palette,
            color_space: p.color_space.clone(),
            posterize_blur: p.posterize_blur,
            normal_blur: p.normal_blur,
            brush_size: p.brush_size,
            hard_brush: p.hard_brush.clone(),
            standard_brush: p.standard_brush.clone(),
            soft_brush: p.soft_brush.clone(),
            strokes_scale: p.strokes_scale,
            wet: p.wet,
            saturation: p.saturation,
            out_long: p.out_long,
            seed: p.seed,
            depth_detail: p.depth_detail,
            depth_invert: p.depth_invert,
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

#[derive(Serialize)]
struct PresetDto {
    name: String,
    description: String,
    params: ParamsDto,
}

/// プリセット一覧（フロントエンドのセレクトボックス用）
#[tauri::command]
fn get_presets() -> Vec<PresetDto> {
    painterly_core::presets()
        .iter()
        .map(|p| PresetDto {
            name: p.name.to_string(),
            description: p.description.to_string(),
            params: ParamsDto::from(&p.params),
        })
        .collect()
}

#[derive(Serialize, Clone)]
struct StageEvent {
    name: String,
    data_url: String,
}

#[derive(Serialize, Clone)]
struct ProgressEvent {
    frac: f32,
    data_url: String,
}

#[derive(Serialize, Clone)]
struct DoneEvent {
    strokes: usize,
    millis: u128,
    data_url: String,
}

#[derive(Serialize)]
struct ImageInfo {
    width: u32,
    height: u32,
    data_url: String,
}

fn to_data_url(img: &RgbImage) -> String {
    let mut buf = Cursor::new(Vec::new());
    // PNG エンコード失敗はメモリ書き込みでは起きない
    image::DynamicImage::ImageRgb8(img.clone())
        .write_to(&mut buf, image::ImageFormat::Png)
        .expect("png encode");
    format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(buf.into_inner())
    )
}

/// プレビュー転送量を抑えるため長辺 max_side に縮小してから data URL 化
fn to_data_url_preview(img: &RgbImage, max_side: u32) -> String {
    let long = img.width().max(img.height());
    if long <= max_side {
        return to_data_url(img);
    }
    let s = max_side as f32 / long as f32;
    let small = image::imageops::resize(
        img,
        (img.width() as f32 * s) as u32,
        (img.height() as f32 * s) as u32,
        image::imageops::FilterType::Triangle,
    );
    to_data_url(&small)
}

#[tauri::command]
fn load_image(path: String) -> Result<ImageInfo, String> {
    let img = image::open(&path).map_err(|e| format!("{path}: {e}"))?.to_rgb8();
    Ok(ImageInfo {
        width: img.width(),
        height: img.height(),
        data_url: to_data_url_preview(&img, 1400),
    })
}

#[tauri::command]
fn start_render(
    app: AppHandle,
    state: State<'_, RenderState>,
    path: String,
    params: ParamsDto,
) -> Result<(), String> {
    if state.busy.swap(true, Ordering::SeqCst) {
        return Err("レンダリング実行中です".into());
    }
    let app2 = app.clone();
    std::thread::spawn(move || {
        let state = app2.state::<RenderState>();
        let result = (|| -> Result<(), String> {
            let img = image::open(&path).map_err(|e| format!("{path}: {e}"))?.to_rgb8();
            let p: Params = params.into();
            let mut brushes = Brushes::new();
            let t0 = std::time::Instant::now();

            let mut on_stage = |name: &str, img: &RgbImage| {
                let _ = app2.emit(
                    "stage",
                    StageEvent { name: name.to_string(), data_url: to_data_url_preview(img, 720) },
                );
            };
            let mut on_progress = |frac: f32, img: &RgbImage| {
                let _ = app2.emit(
                    "paint_progress",
                    ProgressEvent { frac, data_url: to_data_url(img) },
                );
            };
            let res = painterly_core::run_pipeline(
                &img,
                None,
                &p,
                &mut brushes,
                Callbacks { on_stage: Some(&mut on_stage), on_paint_progress: Some(&mut on_progress) },
                false,
            )?;
            let _ = app2.emit(
                "done",
                DoneEvent {
                    strokes: res.strokes,
                    millis: t0.elapsed().as_millis(),
                    data_url: to_data_url(&res.final_image),
                },
            );
            *state.final_image.lock().unwrap() = Some(res.final_image);
            Ok(())
        })();
        if let Err(e) = result {
            let _ = app2.emit("render_error", e);
        }
        state.busy.store(false, Ordering::SeqCst);
    });
    Ok(())
}

#[tauri::command]
fn save_image(state: State<'_, RenderState>, dest: String) -> Result<(), String> {
    let guard = state.final_image.lock().unwrap();
    let img = guard.as_ref().ok_or("保存できる完成画がまだありません")?;
    img.save(&dest).map_err(|e| format!("{dest}: {e}"))
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(RenderState::default())
        .invoke_handler(tauri::generate_handler![
            load_image,
            start_render,
            save_image,
            get_presets
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
