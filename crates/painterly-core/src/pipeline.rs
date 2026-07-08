//! パイプライン統括: 画像 1 枚 → 全中間画像 + 完成画。
//!
//! 解像度モデル（3 つの長辺 px 値）:
//!   - pixels     : 採色・形状単純化の解像度。意図的に小さく取る（画家が目を細めるのと同じ）
//!   - resolution : 描画キャンバスの解像度。低解像度で描いてから out_long へ滑らかに拡大する。
//!                  ストロークの縁がすべてアンチエイリアスの階調になる——完成画が
//!                  「ブロックでなくブラシに見える」最大の要因
//!   - out_long   : 最終出力の長辺（キュービック補間で拡大）
//! 方向場 θ は π 周期。リサンプリングは (cos2θ, sin2θ) の 2ch を経由して atan2 で戻すこと。
//! Python 版 painterly/pipeline.py の移植。

use std::path::{Path, PathBuf};

use image::RgbImage;

use crate::brushes::Brushes;
use crate::buf::{
    gaussian_blur, quantile, resize_gray_bilinear, resize_rgb_bilinear, resize_rgb_cubic,
    resize_to_long_side, Gray, Rgb32,
};
use crate::color;
use crate::density;
use crate::depth;
use crate::flowmap;
use crate::palette;
use crate::rng::Rng64;
use crate::strokes::{self, PaintEngine, Tag};

#[derive(Clone, Debug)]
pub struct Params {
    /// 採色・単純化の解像度（長辺 px）。小さいほど大づかみに
    pub pixels: u32,
    /// 描画キャンバスの長辺 px。小さいほど抽象的に
    pub resolution: u32,
    /// 減色数
    pub palette: usize,
    /// 量子化の色空間 "rgb" / "lab"
    pub color_space: String,
    /// 減色前のガウシアン σ（px、pixels 解像度基準）
    pub posterize_blur: f32,
    /// 勾配計算前のガウシアン σ（px、resolution 解像度基準）
    pub normal_blur: f32,
    /// 基準ブラシ半径（キャンバス px）
    pub brush_size: f32,
    /// 輪郭・高密度領域用ブラシ
    pub hard_brush: String,
    /// 中密度領域用ブラシ
    pub standard_brush: String,
    /// 広い面・低コントラスト領域用ブラシ
    pub soft_brush: String,
    /// ストローク密度の全体倍率
    pub strokes_scale: f32,
    /// 密度上位 15% → ハードブラシ（分位数で適応。毛の多い画像でも全部ハードにならない）
    pub hard_quantile: f32,
    /// 密度 55%〜85% → スタンダード、それ以外 → ソフト
    pub standard_quantile: f32,
    pub side_sample_prob: f32,
    /// ウェットブレンディング: 筆を置く前にキャンバス既存色と混ぜる比率
    pub wet: f32,
    pub saturation: f32,
    /// 最終出力の長辺
    pub out_long: u32,
    pub seed: u64,
    pub process_gif: bool,
    /// デプスによるタッチ粗密の強さ 0..1（0 = 無効）。
    /// 手前ほど細かいタッチ、奥ほど大きく粗いタッチになる
    pub depth_detail: f32,
    /// 深度の手前/奥を反転する（推定が逆転する画像への補正用）
    pub depth_invert: bool,
    /// 外部デプスマップ（白 = 手前）。None なら組み込みのヒューリスティック推定
    pub external_depth: Option<image::GrayImage>,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            pixels: 96,
            resolution: 360,
            palette: 36,
            color_space: "rgb".into(),
            posterize_blur: 2.0,
            normal_blur: 8.0,
            brush_size: 15.0,
            hard_brush: "triangle".into(),
            standard_brush: "flat".into(),
            soft_brush: "soft".into(),
            strokes_scale: 1.0,
            hard_quantile: 0.85,
            standard_quantile: 0.55,
            side_sample_prob: 0.3,
            wet: 0.18,
            saturation: 1.15,
            out_long: 1080,
            seed: 42,
            process_gif: false,
            depth_detail: 0.5,
            depth_invert: false,
            external_depth: None,
        }
    }
}

pub struct PipelineResult {
    pub strokes: usize,
    pub out_dir: Option<PathBuf>,
    pub final_image: RgbImage,
    /// collect_frames=true のときの一筆ごとのスナップショット（リプレイ用）
    pub frames: Option<Vec<RgbImage>>,
}

/// 中間画像 / 進捗のコールバック。
/// on_stage(name, rgb)          —— 中間画像が出来た時点で呼ばれる（GUI の逐次表示用）
/// on_paint_progress(frac, img) —— 描画の進捗、frac は 0..1
#[derive(Default)]
pub struct Callbacks<'a> {
    pub on_stage: Option<&'a mut dyn FnMut(&str, &RgbImage)>,
    pub on_paint_progress: Option<&'a mut dyn FnMut(f32, &RgbImage)>,
}

fn blur_rgb_u8(img: &RgbImage, sigma: f32) -> RgbImage {
    let (w, h) = (img.width() as usize, img.height() as usize);
    let mut chans = [Gray::new(w, h), Gray::new(w, h), Gray::new(w, h)];
    for (i, p) in img.pixels().enumerate() {
        for c in 0..3 {
            chans[c].data[i] = p.0[c] as f32;
        }
    }
    let blurred: Vec<Gray> = chans.iter().map(|g| gaussian_blur(g, sigma)).collect();
    let mut out = RgbImage::new(w as u32, h as u32);
    for (i, p) in out.pixels_mut().enumerate() {
        p.0 = [
            blurred[0].data[i].round().clamp(0.0, 255.0) as u8,
            blurred[1].data[i].round().clamp(0.0, 255.0) as u8,
            blurred[2].data[i].round().clamp(0.0, 255.0) as u8,
        ];
    }
    out
}

pub fn run_pipeline(
    img_rgb: &RgbImage,
    out_dir: Option<&Path>,
    params: &Params,
    brushes: &mut Brushes,
    mut callbacks: Callbacks,
    collect_frames: bool,
) -> Result<PipelineResult, String> {
    let p = params;
    if let Some(dir) = out_dir {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let mut rng = Rng64::seed_from(p.seed);
    let mut stages: Vec<(String, RgbImage)> = Vec::new();

    // 中間画像を保存しつつコールバックへ流す
    macro_rules! stage {
        ($name:expr, $img:expr) => {{
            let img: RgbImage = $img;
            if let Some(dir) = out_dir {
                img.save(dir.join(format!("{}.png", $name)))
                    .map_err(|e| format!("save {}: {e}", $name))?;
            }
            if let Some(cb) = callbacks.on_stage.as_deref_mut() {
                cb($name, &img);
            }
            stages.push(($name.to_string(), img));
        }};
    }

    // --- 単純化 & 減色 ---
    let mut small = resize_to_long_side(img_rgb, p.pixels);
    if p.posterize_blur > 0.0 {
        small = blur_rgb_u8(&small, p.posterize_blur);
    }
    let q = palette::quantize(&small, p.palette, &p.color_space, &mut rng);
    let poster = palette::posterize_edges(&q.labels, &q.quantized);
    let pedge = palette::edge_mask(&q.labels, small.width() as usize, small.height() as usize);

    // --- 方向場・密度（キャンバス解像度） ---
    let ana = resize_to_long_side(img_rgb, p.resolution);
    let (cw, ch) = (ana.width() as usize, ana.height() as usize);
    let mut gray = Gray::new(cw, ch);
    for (i, px) in ana.pixels().enumerate() {
        gray.data[i] = (px.0[0] as f32 * 0.299 + px.0[1] as f32 * 0.587 + px.0[2] as f32 * 0.114)
            / 255.0;
    }
    let gray = gaussian_blur(&gray, p.normal_blur.max(0.5));
    let gx = crate::buf::sobel_x(&gray);
    let gy = crate::buf::sobel_y(&gray);
    let nmap = flowmap::normal_map(&gx, &gy, 4.0);
    let (theta, coh) = flowmap::flow_field(&gx, &gy, (p.resolution as f32 * 0.02).max(2.0));
    let fmap = flowmap::flow_map_vis(&theta, &coh);

    let pe_ana = resize_gray_bilinear(&pedge, cw, ch);
    let mut dens =
        density::density_map(&gx, &gy, &pe_ana, (p.resolution as f32 * 0.03).max(2.0), 1.0);

    // --- デプス（タッチの粗密制御） ---
    // 手前ほど密度を保つ = 小さいハードブラシと細部レイヤーが残り、
    // 奥ほど密度を落とす = 大きいソフトブラシに寄る
    let mut depth_g = match &p.external_depth {
        Some(dm) => depth::from_external(dm, cw, ch),
        None => depth::estimate_depth(&ana, (p.resolution as f32 * 0.04).max(3.0)),
    };
    if p.depth_invert {
        for v in &mut depth_g.data {
            *v = 1.0 - *v;
        }
    }
    if p.depth_detail > 0.0 {
        for i in 0..cw * ch {
            dens.data[i] *= 1.0 - p.depth_detail * depth_g.data[i];
        }
    }

    stage!("1_original", img_rgb.clone());
    stage!("2_quantized", q.quantized.clone());
    stage!("3_posterize_edges", poster);
    stage!("4_gray_blur", {
        let mut g = RgbImage::new(cw as u32, ch as u32);
        for (i, px) in g.pixels_mut().enumerate() {
            let v = (gray.data[i].clamp(0.0, 1.0) * 255.0) as u8;
            px.0 = [v, v, v];
        }
        g
    });
    stage!("5_normal_map", nmap);
    stage!("6_flow_map", fmap);
    stage!("depth_map", depth::depth_vis(&depth_g));
    let hard_t = quantile(&dens.data, p.hard_quantile);
    let std_t = quantile(&dens.data, p.standard_quantile);
    stage!("density", density::density_vis(&dens, hard_t));
    stage!("palette_swatch", palette::palette_swatch(&q.palette));
    stage!("color_wheel", palette::color_wheel(&q.palette, &q.counts));

    // --- 描画ターゲット（減色画像を滑らかに拡大 → パレットに再吸着 → 彩度調整） ---
    let target_u8 = resize_rgb_bilinear(&q.quantized, cw as u32, ch as u32);
    let mut target_u8 = palette::apply_palette(&target_u8, &q.palette);
    if (p.saturation - 1.0).abs() > f32::EPSILON {
        for px in target_u8.pixels_mut() {
            let mut hsv = color::rgb_to_hsv(px.0);
            hsv[1] = ((hsv[1] as f32 * p.saturation).clamp(0.0, 255.0)) as u8;
            px.0 = color::hsv_to_rgb(hsv);
        }
    }
    let mut target = Rgb32::new(cw, ch);
    for (i, px) in target_u8.pixels().enumerate() {
        target.data[i] = [
            px.0[0] as f32 / 255.0,
            px.0[1] as f32 / 255.0,
            px.0[2] as f32 / 255.0,
        ];
    }

    let bs = p.brush_size;
    let engine = PaintEngine::new(&target, &theta, &coh, &dens);

    let tag_of = |d: f32| {
        if d > hard_t {
            Tag::Hard
        } else if d > std_t {
            Tag::Standard
        } else {
            Tag::Soft
        }
    };
    let style_of = |d: f32| {
        if d > hard_t {
            p.hard_brush.clone()
        } else if d > std_t {
            p.standard_brush.clone()
        } else {
            p.soft_brush.clone()
        }
    };

    // 下塗り: キャンバス = 画面の平均色、大きなソフトブラシで敷く
    let mean = {
        let mut m = [0.0f64; 3];
        for c in &target.data {
            m[0] += c[0] as f64;
            m[1] += c[1] as f64;
            m[2] += c[2] as f64;
        }
        let n = target.data.len() as f64;
        [(m[0] / n) as f32, (m[1] / n) as f32, (m[2] / n) as f32]
    };
    let mut canvas = Rgb32 { w: cw, h: ch, data: vec![mean; cw * ch] };

    let under = engine.make_strokes(
        &mut rng,
        bs * 2.0 / p.strokes_scale,
        &mut |_d, r| bs * r.uniform(1.3, 1.7),
        &|_d| p.soft_brush.clone(),
        &|_d| Tag::Soft,
        None,
        0.15,
    );

    // 主層: 密度 → サイズ / ハード・スタンダード・ソフトの 3 段階
    let main = engine.make_strokes(
        &mut rng,
        bs * 0.6 / p.strokes_scale,
        &mut |d, _r| density::brush_size_at(d, bs * 0.35, bs * 0.9),
        &style_of,
        &tag_of,
        None,
        p.side_sample_prob,
    );

    // ディテール層: 高密度領域にだけ小さいハードブラシを足す
    let q75 = quantile(&dens.data, 0.75);
    let detail_mask: Vec<bool> = dens.data.iter().map(|&d| d > q75).collect();
    let detail = engine.make_strokes(
        &mut rng,
        (bs * 0.5).max(2.0) / p.strokes_scale,
        &mut |_d, r| (bs * r.uniform(0.25, 0.4)).max(2.5),
        &|_d| p.hard_brush.clone(),
        &|_d| Tag::Hard,
        Some(&detail_mask),
        0.15,
    );

    let total = under.len() + main.len() + detail.len();
    {
        let mut dbg: Vec<strokes::Stroke> = Vec::with_capacity(main.len() + detail.len());
        dbg.extend(main.iter().cloned());
        dbg.extend(detail.iter().cloned());
        stage!("7_strokes_debug", strokes::render_debug((ch, cw), &dbg));
    }

    // --- 描画（暗 → 明、レイヤー順に） ---
    let mut frames: Vec<RgbImage> = Vec::new();
    let want_frames = collect_frames || p.process_gif;
    let frame_every = (total / 120).max(1);
    let mut done_n = 0usize;

    {
        let want_progress = callbacks.on_paint_progress.is_some();
        let mut cb = |_i: usize, cv: &Rgb32| {
            done_n += 1;
            if done_n % frame_every != 0 && done_n != total {
                return;
            }
            let frame = cv.to_u8_01();
            if want_frames {
                frames.push(frame.clone());
            }
            if let Some(pcb) = callbacks.on_paint_progress.as_deref_mut() {
                pcb(done_n as f32 / total as f32, &frame);
            }
        };
        let use_cb = want_frames || want_progress;
        // ディテール層はウェットブレンディングしない——輪郭は下の色に
        // 引きずられず「噛んで」いてほしい
        for (layer, w_) in [(&under, p.wet), (&main, p.wet), (&detail, 0.0f32)] {
            strokes::render(
                &mut canvas,
                layer,
                w_,
                brushes,
                if use_cb { Some(&mut cb) } else { None },
            );
        }
    }

    let low = canvas.to_u8_01();
    let final_img = if p.out_long as usize > cw.max(ch) {
        let s = p.out_long as f32 / cw.max(ch) as f32;
        resize_rgb_cubic(
            &low,
            (cw as f32 * s).round() as u32,
            (ch as f32 * s).round() as u32,
        )
    } else {
        low.clone()
    };
    stage!("8_painting", final_img.clone());

    if p.process_gif && !frames.is_empty() {
        if let Some(dir) = out_dir {
            let mut gif_frames = frames.clone();
            gif_frames.push(low.clone());
            write_process_gif(&dir.join("process.gif"), &gif_frames, cw as u32, ch as u32)?;
        }
    }

    if let Some(dir) = out_dir {
        let sheet = overview(&stages);
        sheet
            .save(dir.join("overview.png"))
            .map_err(|e| format!("save overview: {e}"))?;
    }

    Ok(PipelineResult {
        strokes: total,
        out_dir: out_dir.map(|d| d.to_path_buf()),
        final_image: final_img,
        frames: if collect_frames { Some(frames) } else { None },
    })
}

fn write_process_gif(path: &Path, frames: &[RgbImage], cw: u32, ch: u32) -> Result<(), String> {
    use image::codecs::gif::{GifEncoder, Repeat};
    use image::{Delay, Frame};
    let (gw, gh) = (cw * 2, ch * 2);
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut enc = GifEncoder::new(file);
    enc.set_repeat(Repeat::Infinite).map_err(|e| e.to_string())?;
    for f in frames {
        let up = resize_rgb_cubic(f, gw, gh);
        let rgba = image::DynamicImage::ImageRgb8(up).to_rgba8();
        let frame = Frame::from_parts(rgba, 0, 0, Delay::from_numer_denom_ms(50, 1));
        enc.encode_frame(frame).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 全ステージを 1 枚のグリッドにまとめた一覧画像（濃いグレー地）
fn overview(stages: &[(String, RgbImage)]) -> RgbImage {
    let names = [
        "1_original",
        "2_quantized",
        "3_posterize_edges",
        "4_gray_blur",
        "5_normal_map",
        "6_flow_map",
        "7_strokes_debug",
        "8_painting",
    ];
    let th = 260u32;
    let tiles: Vec<RgbImage> = names
        .iter()
        .filter_map(|n| stages.iter().find(|(name, _)| name == n))
        .map(|(_, img)| {
            let s = th as f32 / img.height() as f32;
            resize_rgb_bilinear(img, (img.width() as f32 * s) as u32, th)
        })
        .collect();
    let tw = tiles.iter().map(|t| t.width()).max().unwrap_or(1);
    let pad = 12u32;
    let cols = 4u32;
    let rows = (tiles.len() as u32).div_ceil(cols);
    let mut sheet = RgbImage::from_pixel(
        cols * (tw + pad) + pad,
        rows * (th + pad) + pad,
        image::Rgb([34, 34, 34]),
    );
    for (i, t) in tiles.iter().enumerate() {
        let r = i as u32 / cols;
        let c = i as u32 % cols;
        let y = pad + r * (th + pad);
        let x = pad + c * (tw + pad) + (tw - t.width()) / 2;
        image::imageops::overlay(&mut sheet, t, x as i64, y as i64);
    }
    sheet
}
