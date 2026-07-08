//! コマンドラインエントリポイント（Python 版 run.py 相当）。
//!
//! 使い方:
//!     painterly input/cat.png
//!     painterly input/*.png --resolution 200 --palette 36 --brush-size 15
//!     painterly input/cat.png --process-gif   # 一筆ごとの描画過程アニメーション

use std::path::{Path, PathBuf};
use std::time::Instant;

use clap::Parser;
use painterly_core::{Brushes, Callbacks, Params, BUILTIN_BRUSHES};

#[derive(Parser)]
#[command(about = "フローマップに沿ったストローク描画による絵画調レンダリング")]
struct Args {
    /// 入力画像パス
    #[arg(required = true)]
    images: Vec<PathBuf>,
    /// 採色・単純化の解像度（長辺 px）。小さいほど大づかみに
    #[arg(long, default_value_t = 96)]
    pixels: u32,
    /// 描画キャンバスの長辺 px。小さいほど抽象的に
    #[arg(long, default_value_t = 360)]
    resolution: u32,
    /// 減色数
    #[arg(long, default_value_t = 36)]
    palette: usize,
    /// 量子化の色空間 (rgb / lab)
    #[arg(long = "color-wheel", default_value = "rgb")]
    color_space: String,
    /// 減色前のガウシアン σ
    #[arg(long, default_value_t = 2.0)]
    posterize_blur: f32,
    /// 勾配計算前のガウシアン σ
    #[arg(long, default_value_t = 8.0)]
    normal_blur: f32,
    /// 基準ブラシ半径（キャンバス px）
    #[arg(long, default_value_t = 15.0)]
    brush_size: f32,
    /// ハードブラシ: ビルトイン名または グレースケール PNG のパス
    #[arg(long, default_value = "triangle")]
    hard_brush: String,
    /// スタンダードブラシ
    #[arg(long, default_value = "flat")]
    standard_brush: String,
    /// ソフトブラシ
    #[arg(long, default_value = "soft")]
    soft_brush: String,
    /// ストローク密度の倍率
    #[arg(long, default_value_t = 1.0)]
    strokes: f32,
    /// ウェットブレンディング比率 0..1
    #[arg(long, default_value_t = 0.18)]
    wet: f32,
    /// 最終出力の長辺 px
    #[arg(long, default_value_t = 1080)]
    out_long: u32,
    #[arg(long, default_value_t = 42)]
    seed: u64,
    /// 一筆ごとの描画過程 GIF を出力
    #[arg(long, default_value_t = false)]
    process_gif: bool,
    /// デプスによるタッチ粗密の強さ 0..1（手前=細かく、奥=粗く。0 で無効）
    #[arg(long, default_value_t = 0.5)]
    depth_detail: f32,
    /// 外部デプスマップ PNG（白 = 手前）。省略時は組み込み推定
    #[arg(long)]
    depth: Option<PathBuf>,
    /// 深度の手前/奥を反転
    #[arg(long, default_value_t = false)]
    depth_invert: bool,
    /// 出力先ディレクトリ（既定: output/<画像名>/）
    #[arg(long)]
    out: Option<PathBuf>,
}

fn main() {
    let args = Args::parse();
    let mut brushes = Brushes::new();

    let mut params = Params {
        pixels: args.pixels,
        resolution: args.resolution,
        palette: args.palette,
        color_space: args.color_space.clone(),
        posterize_blur: args.posterize_blur,
        normal_blur: args.normal_blur,
        brush_size: args.brush_size,
        hard_brush: args.hard_brush.clone(),
        standard_brush: args.standard_brush.clone(),
        soft_brush: args.soft_brush.clone(),
        strokes_scale: args.strokes,
        wet: args.wet,
        out_long: args.out_long,
        seed: args.seed,
        process_gif: args.process_gif,
        depth_detail: args.depth_detail,
        depth_invert: args.depth_invert,
        external_depth: match &args.depth {
            Some(path) => match image::open(path) {
                Ok(img) => Some(img.to_luma8()),
                Err(e) => {
                    eprintln!("[error] デプスマップを読み込めません: {}: {e}", path.display());
                    std::process::exit(1);
                }
            },
            None => None,
        },
        ..Params::default()
    };

    // ビルトイン名でなければカスタムブラシ PNG として登録
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
        match painterly_core::run_pipeline(
            &img,
            Some(&out_dir),
            &params,
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
