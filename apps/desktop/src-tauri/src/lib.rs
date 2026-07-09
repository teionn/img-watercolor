//! Tauri バックエンド: painterly-core をバックグラウンドスレッドで走らせ、
//! 中間画像と描画進捗をイベントでフロントエンドへ流す。
//! Python 版 gui.py（PySide6）の役割を置き換える。

use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use base64::Engine as _;
use image::RgbImage;
use painterly_core::{Brushes, Callbacks, Params, BUILTIN_BRUSHES};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

/// レンダリング状態（多重起動の防止と、保存用に最後の完成画を保持）
#[derive(Default)]
struct RenderState {
    busy: AtomicBool,
    final_image: Mutex<Option<RgbImage>>,
    /// ロード済みのニューラル深度モデル（Depth Anything V2 等）
    depth_model: Mutex<Option<painterly_depth::DepthModel>>,
    /// 直近のレンダリングで生成された process.gif のパス
    last_gif: Mutex<Option<std::path::PathBuf>>,
}

/// DTO のブラシ指定がビルトイン名でなければカスタム PNG として登録する
fn resolve_brushes(p: &mut Params, brushes: &mut Brushes) -> Result<(), String> {
    for (alias, field) in [
        ("custom_hard", &mut p.hard_brush),
        ("custom_standard", &mut p.standard_brush),
        ("custom_soft", &mut p.soft_brush),
    ] {
        if !BUILTIN_BRUSHES.contains(&field.as_str()) {
            brushes.load_custom(field, alias)?;
            *field = alias.to_string();
        }
    }
    Ok(())
}

/// 外部デプス PNG またはニューラル深度モデルから external_depth を用意する
fn resolve_depth(
    p: &mut Params,
    img: &RgbImage,
    external_path: &Option<String>,
    use_model: bool,
    state: &RenderState,
) -> Result<(), String> {
    if let Some(path) = external_path {
        let dm = image::open(path).map_err(|e| format!("デプスマップ {path}: {e}"))?;
        p.external_depth = Some(dm.to_luma8());
    } else if use_model {
        if let Some(model) = state.depth_model.lock().unwrap().as_ref() {
            p.external_depth = Some(model.estimate(img)?);
        }
    }
    Ok(())
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
    focus_depth: f32,
    hard_quantile: f32,
    standard_quantile: f32,
    side_sample_prob: f32,
    process_gif: bool,
    /// ニューラル深度モデルを使う（load_depth_model 済みのとき有効）
    use_depth_model: bool,
    /// 外部デプスマップ PNG のパス（白 = 手前）。指定時はモデルより優先
    external_depth_path: Option<String>,
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
            focus_depth: p.focus_depth,
            hard_quantile: p.hard_quantile,
            standard_quantile: p.standard_quantile,
            side_sample_prob: p.side_sample_prob,
            process_gif: false,
            use_depth_model: false,
            external_depth_path: None,
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
            focus_depth: d.focus_depth,
            hard_quantile: d.hard_quantile,
            standard_quantile: d.standard_quantile,
            side_sample_prob: d.side_sample_prob,
            process_gif: d.process_gif,
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
            focus_depth: p.focus_depth,
            hard_quantile: p.hard_quantile,
            standard_quantile: p.standard_quantile,
            side_sample_prob: p.side_sample_prob,
            process_gif: false,
            use_depth_model: false,
            external_depth_path: None,
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
    /// process.gif が生成されたか（GIF 保存ボタンの活性化用）
    gif: bool,
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
            let dto = params;
            let mut p: Params = dto.clone().into();
            let mut brushes = Brushes::new();
            resolve_brushes(&mut p, &mut brushes)?;
            resolve_depth(&mut p, &img, &dto.external_depth_path, dto.use_depth_model, &state)?;
            // 過程 GIF は一時ディレクトリに書き出し、保存時にコピーする
            let out_dir = if p.process_gif {
                let dir = std::env::temp_dir().join("img-watercolor-gui");
                std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
                Some(dir)
            } else {
                None
            };
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
                out_dir.as_deref(),
                &p,
                &mut brushes,
                Callbacks { on_stage: Some(&mut on_stage), on_paint_progress: Some(&mut on_progress) },
                false,
            )?;
            let gif_path = out_dir
                .map(|d| d.join("process.gif"))
                .filter(|g| g.exists());
            let _ = app2.emit(
                "done",
                DoneEvent {
                    strokes: res.strokes,
                    millis: t0.elapsed().as_millis(),
                    data_url: to_data_url(&res.final_image),
                    gif: gif_path.is_some(),
                },
            );
            *state.last_gif.lock().unwrap() = gif_path;
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

/// スイープ対象フィールドへ値を設定する（フロントエンドのスライダー名と対応）
fn apply_sweep_value(p: &mut Params, name: &str, v: f64) -> Result<(), String> {
    match name {
        "pixels" => p.pixels = v as u32,
        "resolution" => p.resolution = v as u32,
        "palette" => p.palette = v as usize,
        "posterize_blur" => p.posterize_blur = v as f32,
        "normal_blur" => p.normal_blur = v as f32,
        "brush_size" => p.brush_size = v as f32,
        "strokes_scale" => p.strokes_scale = v as f32,
        "wet" => p.wet = v as f32,
        "saturation" => p.saturation = v as f32,
        "out_long" => p.out_long = v as u32,
        "depth_detail" => p.depth_detail = v as f32,
        "focus_range" => p.focus_range = v as f32,
        "detail_min" => p.detail_min = v as f32,
        "detail_max" => p.detail_max = v as f32,
        "line_strength" => p.line_strength = v as f32,
        "line_width" => p.line_width = v as f32,
        "paper_texture" => p.paper_texture = v as f32,
        "paper_border" => p.paper_border = v as f32,
        "pigment" => p.pigment = v as f32,
        "edge_darken" => p.edge_darken = v as f32,
        "focus_depth" => p.focus_depth = v as f32,
        "hard_quantile" => p.hard_quantile = v as f32,
        "standard_quantile" => p.standard_quantile = v as f32,
        "side_sample_prob" => p.side_sample_prob = v as f32,
        "seed" => p.seed = v as u64,
        other => return Err(format!("走査できないパラメータ: {other}")),
    }
    Ok(())
}

#[derive(Serialize, Clone)]
struct SweepResult {
    index: usize,
    value: f64,
    data_url: String,
}

/// パラメータスイープ: 1 つのパラメータを values の各値に変えて連続レンダリングし、
/// 1 枚できるごとに sweep_result イベントで返す。比較を高速にするため拡大は行わない
#[tauri::command]
fn start_sweep(
    app: AppHandle,
    state: State<'_, RenderState>,
    path: String,
    params: ParamsDto,
    sweep_param: String,
    values: Vec<f64>,
) -> Result<(), String> {
    if state.busy.swap(true, Ordering::SeqCst) {
        return Err("レンダリング実行中です".into());
    }
    let app2 = app.clone();
    std::thread::spawn(move || {
        let state = app2.state::<RenderState>();
        let result = (|| -> Result<(), String> {
            let img = image::open(&path).map_err(|e| format!("{path}: {e}"))?.to_rgb8();
            let dto = params;
            let mut base: Params = dto.clone().into();
            let mut brushes = Brushes::new();
            resolve_brushes(&mut base, &mut brushes)?;
            // 深度は画像ごとに一定なので走査前に 1 回だけ解決する
            resolve_depth(&mut base, &img, &dto.external_depth_path, dto.use_depth_model, &state)?;
            for (index, &value) in values.iter().enumerate() {
                let mut p = base.clone();
                apply_sweep_value(&mut p, &sweep_param, value)?;
                p.out_long = p.resolution; // 拡大なしで高速化（プレビュー用途）
                let res = painterly_core::run_pipeline(
                    &img,
                    None,
                    &p,
                    &mut brushes,
                    Callbacks::default(),
                    false,
                )?;
                let _ = app2.emit(
                    "sweep_result",
                    SweepResult {
                        index,
                        value,
                        data_url: to_data_url_preview(&res.final_image, 480),
                    },
                );
            }
            Ok(())
        })();
        match result {
            Ok(()) => {
                let _ = app2.emit("sweep_done", ());
            }
            Err(e) => {
                let _ = app2.emit("render_error", e);
                let _ = app2.emit("sweep_done", ());
            }
        }
        state.busy.store(false, Ordering::SeqCst);
    });
    Ok(())
}

/// ニューラル深度モデルを読み込む。path 未指定なら models/ の既定パスを自動検出。
/// 成功時は読み込んだパスを返す
#[tauri::command]
fn load_depth_model(state: State<'_, RenderState>, path: Option<String>) -> Result<String, String> {
    let path = match path {
        Some(p) => std::path::PathBuf::from(p),
        None => {
            let default = std::path::PathBuf::from("models/depth_anything_v2_small.onnx");
            if !default.exists() {
                return Err(
                    "models/depth_anything_v2_small.onnx が見つかりません（scripts/fetch_models.sh で取得するか、.onnx を指定してください）"
                        .into(),
                );
            }
            default
        }
    };
    let model = painterly_depth::DepthModel::load(&path, None)?;
    *state.depth_model.lock().unwrap() = Some(model);
    Ok(path.display().to_string())
}

/// 直近のレンダリングで生成された process.gif を保存する
#[tauri::command]
fn save_process_gif(state: State<'_, RenderState>, dest: String) -> Result<(), String> {
    let guard = state.last_gif.lock().unwrap();
    let src = guard.as_ref().ok_or("保存できる過程 GIF がありません")?;
    std::fs::copy(src, &dest).map_err(|e| format!("{dest}: {e}"))?;
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
            get_presets,
            start_sweep,
            load_depth_model,
            save_process_gif
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
