//! コマンドラインエントリポイント（Python 版 run.py 相当）。
//!
//! 使い方:
//!     painterly input/cat.png
//!     painterly input/cat.png --preset 水彩紙
//!     painterly input/*.png --resolution 200 --palette 36 --brush-size 15
//!     painterly input/cat.png --process-gif   # 一筆ごとの描画過程アニメーション
//!
//! 各フラグは「指定したものだけ」プリセット（または既定値）を上書きする。

use std::path::{Path, PathBuf};
use std::time::Instant;

use clap::Parser;
use painterly_core::{Brushes, Callbacks, Params, BUILTIN_BRUSHES};

#[derive(Parser)]
#[command(about = "フローマップに沿ったストローク描画による絵画調レンダリング")]
struct Args {
    /// 入力画像パス
    #[arg(required_unless_present = "list_presets")]
    images: Vec<PathBuf>,
    /// プリセット名（--list-presets で一覧）。個別フラグで上書き可
    #[arg(long)]
    preset: Option<String>,
    /// プリセット一覧を表示して終了
    #[arg(long, default_value_t = false)]
    list_presets: bool,
    /// 全ビルトインブラシのスタンプ一覧画像を書き出して終了
    #[arg(long)]
    brush_catalog: Option<PathBuf>,
    /// 採色・単純化の解像度（長辺 px）。小さいほど大づかみに [既定: 96]
    #[arg(long)]
    pixels: Option<u32>,
    /// 描画キャンバスの長辺 px。小さいほど抽象的に [既定: 360]
    #[arg(long)]
    resolution: Option<u32>,
    /// 減色数 [既定: 36]
    #[arg(long)]
    palette: Option<usize>,
    /// 量子化の色空間 (rgb / lab) [既定: rgb]
    #[arg(long = "color-wheel")]
    color_space: Option<String>,
    /// 減色前のガウシアン σ [既定: 2.0]
    #[arg(long)]
    posterize_blur: Option<f32>,
    /// 勾配計算前のガウシアン σ [既定: 8.0]
    #[arg(long)]
    normal_blur: Option<f32>,
    /// 基準ブラシ半径（キャンバス px） [既定: 15]
    #[arg(long)]
    brush_size: Option<f32>,
    /// ハードブラシ: ビルトイン名または グレースケール PNG のパス [既定: triangle]
    #[arg(long)]
    hard_brush: Option<String>,
    /// スタンダードブラシ [既定: flat]
    #[arg(long)]
    standard_brush: Option<String>,
    /// ソフトブラシ [既定: soft]
    #[arg(long)]
    soft_brush: Option<String>,
    /// ストローク密度の倍率 [既定: 1.0]
    #[arg(long)]
    strokes: Option<f32>,
    /// ウェットブレンディング比率 0..1 [既定: 0.18]
    #[arg(long)]
    wet: Option<f32>,
    /// 彩度 [既定: 1.15]
    #[arg(long)]
    saturation: Option<f32>,
    /// 最終出力の長辺 px [既定: 1080]
    #[arg(long)]
    out_long: Option<u32>,
    #[arg(long)]
    seed: Option<u64>,
    /// 一筆ごとの描画過程 GIF を出力
    #[arg(long, default_value_t = false)]
    process_gif: bool,
    /// デプスによるタッチ粗密の強さ 0..1（0 で無効） [既定: 0.5]
    #[arg(long)]
    depth_detail: Option<f32>,
    /// 外部デプスマップ PNG（白 = 手前）。省略時はモデル推定 or 組み込み推定
    #[arg(long)]
    depth: Option<PathBuf>,
    /// 深度の手前/奥を反転
    #[arg(long, default_value_t = false)]
    depth_invert: bool,
    /// 深度推定の ONNX モデル。省略時は models/depth_anything_v2_small.onnx を
    /// 自動検出、それも無ければ組み込みのヒューリスティック推定
    #[arg(long)]
    depth_model: Option<PathBuf>,
    /// フォーカス位置 X（画像上の正規化座標 0..1）。Y とセットで指定
    #[arg(long)]
    focus_x: Option<f32>,
    /// フォーカス位置 Y（画像上の正規化座標 0..1）
    #[arg(long)]
    focus_y: Option<f32>,
    /// フォーカス位置未指定時の焦点深度 0(手前)..1(奥) [既定: 0]
    #[arg(long)]
    focus_depth: Option<f32>,
    /// 焦点から細かさが保たれる深度範囲（小さいほど被写界深度が浅い） [既定: 0.6]
    #[arg(long)]
    focus_range: Option<f32>,
    /// ボケ領域の粗さ下限（密度倍率、小さいほど粗い） [既定: 0.35]
    #[arg(long)]
    detail_min: Option<f32>,
    /// 焦点近傍の細かさ上限（1 超で焦点付近をさらに細かく） [既定: 1.15]
    #[arg(long)]
    detail_max: Option<f32>,
    /// 輪郭線（鉛筆下書き）の強さ 0..1（0 で無効） [既定: 0.4]
    #[arg(long)]
    line_strength: Option<f32>,
    /// 輪郭線の太さ（キャンバス px） [既定: 1.0]
    #[arg(long)]
    line_width: Option<f32>,
    /// 紙のテクスチャの強さ 0..1 [既定: 0.35]
    #[arg(long)]
    paper_texture: Option<f32>,
    /// 紙の余白（短辺に対する比率、例 0.05） [既定: 0]
    #[arg(long)]
    paper_border: Option<f32>,
    /// 透明水彩度 0..1（0 = 油彩、1 = 透明顔料のグレーズ） [既定: 0]
    #[arg(long)]
    pigment: Option<f32>,
    /// エッジ暗色化 0..1（塗りの縁に顔料が溜まる） [既定: 0]
    #[arg(long)]
    edge_darken: Option<f32>,
    /// 出力先ディレクトリ（既定: output/<画像名>/）
    #[arg(long)]
    out: Option<PathBuf>,
}

/// 全ビルトインブラシを 1 枚のグリッド画像にする（形状確認・ドキュメント用）
fn write_brush_catalog(out: &Path) {
    let mut brushes = Brushes::new();
    let radius = 44.0;
    let d = radius as usize * 2 + 1;
    let pad = 10usize;
    let cols = 4usize;
    let rows = BUILTIN_BRUSHES.len().div_ceil(cols);
    let mut sheet = image::RgbImage::from_pixel(
        (cols * (d + pad) + pad) as u32,
        (rows * (d + pad) + pad) as u32,
        image::Rgb([30, 30, 34]),
    );
    for (i, name) in BUILTIN_BRUSHES.iter().enumerate() {
        let stamp = brushes.get_stamp(name, radius, 0.0);
        let (r, c) = (i / cols, i % cols);
        let (ox, oy) = (pad + c * (d + pad), pad + r * (d + pad));
        for y in 0..d {
            for x in 0..d {
                let v = (stamp.at(x, y).clamp(0.0, 1.0) * 255.0) as u8;
                sheet.put_pixel((ox + x) as u32, (oy + y) as u32, image::Rgb([v, v, v]));
            }
        }
        println!("{:>2}: {}", i + 1, name);
    }
    match sheet.save(out) {
        Ok(()) => println!("-> {}", out.display()),
        Err(e) => eprintln!("[error] {}: {e}", out.display()),
    }
}

fn main() {
    let args = Args::parse();

    if args.list_presets {
        for p in painterly_core::presets() {
            println!("{:　<6} {}", p.name, p.description);
        }
        return;
    }

    if let Some(out) = &args.brush_catalog {
        write_brush_catalog(out);
        return;
    }

    // プリセット（または既定値）をベースに、指定されたフラグだけ上書き
    let mut params = match &args.preset {
        Some(name) => painterly_core::preset(name).unwrap_or_else(|| {
            eprintln!("[error] 不明なプリセット: {name}（--list-presets で一覧）");
            std::process::exit(1);
        }),
        None => Params::default(),
    };
    macro_rules! merge {
        ($($field:ident <- $arg:ident),* $(,)?) => {
            $(if let Some(v) = args.$arg.clone() { params.$field = v; })*
        };
    }
    merge!(
        pixels <- pixels, resolution <- resolution, palette <- palette,
        color_space <- color_space, posterize_blur <- posterize_blur,
        normal_blur <- normal_blur, brush_size <- brush_size,
        hard_brush <- hard_brush, standard_brush <- standard_brush,
        soft_brush <- soft_brush, strokes_scale <- strokes, wet <- wet,
        saturation <- saturation, out_long <- out_long, seed <- seed,
        depth_detail <- depth_detail, focus_depth <- focus_depth,
        focus_range <- focus_range, detail_min <- detail_min,
        detail_max <- detail_max, line_strength <- line_strength,
        line_width <- line_width, paper_texture <- paper_texture,
        paper_border <- paper_border, pigment <- pigment,
        edge_darken <- edge_darken,
    );
    params.process_gif = args.process_gif;
    params.depth_invert = args.depth_invert;
    params.focus_point = args.focus_x.zip(args.focus_y);
    if let Some(path) = &args.depth {
        match image::open(path) {
            Ok(img) => params.external_depth = Some(img.to_luma8()),
            Err(e) => {
                eprintln!("[error] デプスマップを読み込めません: {}: {e}", path.display());
                std::process::exit(1);
            }
        }
    }

    // ビルトイン名でなければカスタムブラシ PNG として登録
    let mut brushes = Brushes::new();
    for (name, field) in [
        ("custom_hard", &mut params.hard_brush),
        ("custom_standard", &mut params.standard_brush),
        ("custom_soft", &mut params.soft_brush),
    ] {
        if !BUILTIN_BRUSHES.contains(&field.as_str()) {
            if let Err(e) = brushes.load_custom(field, name) {
                eprintln!("[error] ブラシを読み込めません: {e}");
                std::process::exit(1);
            }
            *field = name.to_string();
        }
    }

    // 深度推定モデル: --depth 指定時は不要。--depth-model か models/ の既定パスを使う
    let depth_model = if params.external_depth.is_some() {
        None
    } else {
        let model_path = args.depth_model.clone().or_else(|| {
            let default = PathBuf::from("models/depth_anything_v2_small.onnx");
            default.exists().then_some(default)
        });
        match model_path {
            Some(path) => match painterly_depth::DepthModel::load(&path, None) {
                Ok(m) => {
                    eprintln!("[info] 深度モデル: {}", path.display());
                    Some(m)
                }
                Err(e) => {
                    eprintln!("[error] 深度モデルを読み込めません: {e}");
                    std::process::exit(1);
                }
            },
            None => None,
        }
    };

    for path in &args.images {
        let img = match image::open(path) {
            Ok(i) => i.to_rgb8(),
            Err(e) => {
                eprintln!("[skip] 画像を読み込めません: {}: {e}", path.display());
                continue;
            }
        };
        let stem = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
        let out_dir = args
            .out
            .clone()
            .unwrap_or_else(|| Path::new("output").join(&stem));
        let t0 = Instant::now();
        // モデル推定は画像ごとに行う
        let mut params_img = params.clone();
        if let Some(model) = &depth_model {
            match model.estimate(&img) {
                Ok(d) => params_img.external_depth = Some(d),
                Err(e) => {
                    eprintln!("[warn] 深度推定に失敗、組み込み推定を使用: {e}");
                }
            }
        }
        match painterly_core::run_pipeline(
            &img,
            Some(&out_dir),
            &params_img,
            &mut brushes,
            Callbacks::default(),
            false,
        ) {
            Ok(info) => println!(
                "[done] {}: {} strokes, {:.1}s -> {}",
                path.file_name().unwrap_or_default().to_string_lossy(),
                info.strokes,
                t0.elapsed().as_secs_f32(),
                out_dir.display()
            ),
            Err(e) => eprintln!("[error] {}: {e}", path.display()),
        }
    }
}
