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
    /// デプスによるタッチ粗密の強さ 0..1（0 = 無効）
    pub depth_detail: f32,
    /// 深度の手前/奥を反転する（推定が逆転する画像への補正用）
    pub depth_invert: bool,
    /// 外部デプスマップ（白 = 手前）。None なら組み込みのヒューリスティック推定
    pub external_depth: Option<image::GrayImage>,
    /// フォーカス位置（画像上の正規化座標 0..1）。指定するとその点の深度が焦点になる
    pub focus_point: Option<(f32, f32)>,
    /// focus_point 未指定時の焦点深度 0(手前)..1(奥)
    pub focus_depth: f32,
    /// 焦点から細かさが保たれる深度範囲。小さいほど「被写界深度が浅い」
    pub focus_range: f32,
    /// 範囲外（ボケ領域）の粗さ下限。密度に掛かる倍率（小さいほど粗い）
    pub detail_min: f32,
    /// 焦点近傍の細かさ上限。1 より大きいと焦点付近をさらに細かく
    pub detail_max: f32,
    /// 輪郭線の強さ 0..1（0 = 無効）。ボケ領域では自動的に薄くなる
    pub line_strength: f32,
    /// 輪郭線の太さ（キャンバス px、半径）
    pub line_width: f32,
    /// 紙のテクスチャの強さ 0..1（0 = なし）。最終出力に紙目を乗せる。
    /// pigment > 0 のときは粒状化（顔料が紙の目に沈む）の強さも兼ねる
    pub paper_texture: f32,
    /// 紙の余白（短辺に対する比率、0 = なし）。荒れたエッジの白フチを残す
    pub paper_border: f32,
    /// 透明水彩度 0..1（Curtis 1997 のグレーズ近似）。
    /// 0 = 不透明な油彩、1 = 完全な透明顔料（紙の白が透ける減法混色）。
    /// 下地も mean 色 → 紙白 に連動する
    pub pigment: f32,
    /// エッジ暗色化 0..1。塗りの縁に顔料が溜まる水彩特有の縁取り
    pub edge_darken: f32,
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
            focus_point: None,
            focus_depth: 0.0,
            focus_range: 0.6,
            detail_min: 0.35,
            detail_max: 1.15,
            line_strength: 0.4,
            line_width: 1.0,
            paper_texture: 0.35,
            paper_border: 0.0,
            pigment: 0.0,
            edge_darken: 0.0,
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
    // ぼかす前のグレースケールは輪郭線（鉛筆下書き）の抽出に使う
    let gray_sharp = gray.clone();
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

    // フォーカス重み: 1 = 焦点近傍（細かい）、0 = 深度範囲外（粗い）。
    // 焦点はクリック位置の深度（focus_point）か focus_depth の値
    let focus_depth = match p.focus_point {
        Some((fx, fy)) => {
            let xi = (fx.clamp(0.0, 1.0) * (cw - 1) as f32) as usize;
            let yi = (fy.clamp(0.0, 1.0) * (ch - 1) as f32) as usize;
            depth_g.at(xi, yi)
        }
        None => p.focus_depth.clamp(0.0, 1.0),
    };
    let range = p.focus_range.max(0.05);
    let mut focus_w = Gray::new(cw, ch);
    for i in 0..cw * ch {
        let t = ((depth_g.data[i] - focus_depth).abs() / range).clamp(0.0, 1.0);
        focus_w.data[i] = 1.0 - t * t * (3.0 - 2.0 * t); // smoothstep 減衰
    }
    if p.depth_detail > 0.0 {
        // 密度をフォーカス重みで変調: detail_min（ボケ側の粗さ下限）〜
        // detail_max（焦点側の細かさ上限）を depth_detail の強さでブレンド
        for i in 0..cw * ch {
            let scale = p.detail_min + (p.detail_max - p.detail_min) * focus_w.data[i];
            let eff = 1.0 + (scale - 1.0) * p.depth_detail;
            dens.data[i] = (dens.data[i] * eff).clamp(0.0, 1.2);
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

    // 輪郭線マスク: 「鉛筆で描いた下書き」を目指す。
    // 減色後ではなく **元画像**（ぼかす前のグレースケール）からエッジを取るので、
    // 減色で潰れた顔のパーツ・髪・布のディテール線も拾える。
    // 絵のタッチとは独立したレイヤーとして描画後に重ねる
    let line_mask = if p.line_strength > 0.0 {
        let smoothstep = |e0: f32, e1: f32, x: f32| -> f32 {
            let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
            t * t * (3.0 - 2.0 * t)
        };
        // 軽い平滑化 → 勾配強度 → 分位数正規化 → ソフト閾値
        let g1 = gaussian_blur(&gray_sharp, 1.2);
        let egx = crate::buf::sobel_x(&g1);
        let egy = crate::buf::sobel_y(&g1);
        let mut m = Gray::new(cw, ch);
        for i in 0..cw * ch {
            m.data[i] = egx.data[i].hypot(egy.data[i]);
        }
        let p97 = quantile(&m.data, 0.97) + 1e-8;
        for v in &mut m.data {
            *v = smoothstep(0.25, 0.70, *v / p97);
        }
        if p.line_width > 1.0 {
            m = dilate(&m, p.line_width - 1.0);
        }
        let mut m = gaussian_blur(&m, 0.5);
        // 1.0 未満は細線化: ソフトエッジをべき乗で締めて実効幅を落とす
        if p.line_width < 1.0 {
            let e = (1.0 / p.line_width.max(0.3)).min(3.5);
            for v in &mut m.data {
                *v = v.powf(e);
            }
        }

        // 鉛筆の質感: 細かい紙目 + 粗いノイズで線を途切れさせる（シード固定で再現可能）
        let mut nrng = Rng64::seed_from(p.seed ^ 0x70656e63); // "penc"
        let mut noise = Gray::new(cw, ch);
        for v in &mut noise.data {
            *v = nrng.random();
        }
        let fine = gaussian_blur(&noise, 0.9);
        let coarse = gaussian_blur(&noise, 3.5);
        let norm = |g: &Gray| -> (f32, f32) {
            let mn = g.data.iter().cloned().fold(f32::MAX, f32::min);
            let mx = g.data.iter().cloned().fold(f32::MIN, f32::max);
            (mn, (mx - mn).max(1e-8))
        };
        let (fmn, frange) = norm(&fine);
        let (cmn, crange) = norm(&coarse);
        for i in 0..cw * ch {
            let nf = (fine.data[i] - fmn) / frange;
            let nc = (coarse.data[i] - cmn) / crange;
            let grain = (0.60 + 0.40 * nf) * (0.55 + 0.45 * smoothstep(0.25, 0.75, nc));
            m.data[i] = (m.data[i] * grain).clamp(0.0, 1.0);
        }

        let mut vis = RgbImage::new(cw as u32, ch as u32);
        for (i, px) in vis.pixels_mut().enumerate() {
            let v = ((1.0 - m.data[i]) * 255.0) as u8;
            px.0 = [v, v, v];
        }
        stage!("line_art", vis);
        Some(m)
    } else {
        None
    };

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

    // 下塗り: キャンバス = 画面の平均色、大きなソフトブラシで敷く。
    // 透明水彩（pigment > 0）では紙の白へ寄せる——グレーズは暗くする方向にしか
    // 働かないので、下地が明るくないと発色しない
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
    const PAPER_WHITE: [f32; 3] = [0.97, 0.965, 0.945];
    let base = [
        mean[0] * (1.0 - p.pigment) + PAPER_WHITE[0] * p.pigment,
        mean[1] * (1.0 - p.pigment) + PAPER_WHITE[1] * p.pigment,
        mean[2] * (1.0 - p.pigment) + PAPER_WHITE[2] * p.pigment,
    ];
    let mut canvas = Rgb32 { w: cw, h: ch, data: vec![base; cw * ch] };

    // 水彩設定と粒状化用の紙目（キャンバス解像度、シード固定）
    let wc = strokes::WatercolorCfg {
        pigment: p.pigment,
        edge_darken: p.edge_darken,
        granulation: if p.pigment > 0.0 { p.paper_texture } else { 0.0 },
    };
    let grain = if wc.granulation > 0.0 {
        let mut grng = Rng64::seed_from(p.seed ^ 0x6772616e); // "gran"
        let mut n = Gray::new(cw, ch);
        for v in &mut n.data {
            *v = grng.random();
        }
        let mut n = gaussian_blur(&n, 1.0);
        let mn = n.data.iter().cloned().fold(f32::MAX, f32::min);
        let mx = n.data.iter().cloned().fold(f32::MIN, f32::max);
        let range = (mx - mn).max(1e-8);
        for v in &mut n.data {
            *v = (*v - mn) / range;
        }
        Some(n)
    } else {
        None
    };

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
                &wc,
                grain.as_ref(),
                if use_cb { Some(&mut cb) } else { None },
            );
        }
    }

    // 輪郭線（鉛筆下書き）を重ねる。インクはグラファイトの青灰色。
    // ボケ領域（フォーカス重みが低い）では線も薄くして被写界深度と整合させる
    if let Some(mask) = &line_mask {
        const GRAPHITE: [f32; 3] = [0.16, 0.16, 0.20];
        for i in 0..cw * ch {
            let mut a = mask.data[i].clamp(0.0, 1.0) * p.line_strength;
            if p.depth_detail > 0.0 {
                a *= 1.0 - (1.0 - focus_w.data[i]) * p.depth_detail;
            }
            if a <= 0.0 {
                continue;
            }
            let px = &mut canvas.data[i];
            for c in 0..3 {
                px[c] = px[c] * (1.0 - a) + GRAPHITE[c] * a;
            }
        }
    }

    let low = canvas.to_u8_01();
    let mut final_img = if p.out_long as usize > cw.max(ch) {
        let s = p.out_long as f32 / cw.max(ch) as f32;
        resize_rgb_cubic(
            &low,
            (cw as f32 * s).round() as u32,
            (ch as f32 * s).round() as u32,
        )
    } else {
        low.clone()
    };
    // 紙のテクスチャと余白は最終解像度で適用（拡大でボケさせない）
    crate::paper::apply_paper(&mut final_img, p.paper_texture, p.paper_border, p.seed);
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

/// 円形カーネルでの膨張（輪郭線の太らせ用）
fn dilate(src: &Gray, radius: f32) -> Gray {
    let r = radius.ceil().max(0.0) as isize;
    if r == 0 {
        return src.clone();
    }
    let (w, h) = (src.w as isize, src.h as isize);
    let r2 = radius * radius;
    let mut out = Gray::new(src.w, src.h);
    for y in 0..h {
        for x in 0..w {
            let mut mx = 0.0f32;
            for dy in -r..=r {
                let yy = y + dy;
                if yy < 0 || yy >= h {
                    continue;
                }
                for dx in -r..=r {
                    let xx = x + dx;
                    if xx < 0 || xx >= w {
                        continue;
                    }
                    if (dx * dx + dy * dy) as f32 <= r2 {
                        mx = mx.max(src.data[(yy * w + xx) as usize]);
                    }
                }
            }
            out.data[(y * w + x) as usize] = mx;
        }
    }
    out
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
