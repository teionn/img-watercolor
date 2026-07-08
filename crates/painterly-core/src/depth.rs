//! デプスマップ（擬似的な単眼深度推定）: タッチの細かさを奥行きで制御する。
//!
//! 手前 = 0 / 奥 = 1 の深度を、学習モデルなしの 3 つの手掛かりから合成する:
//!   1. 合焦度       — ピントが合っている（局所勾配が強い）領域は被写体 = 手前
//!   2. 大気遠近     — 明るく彩度の低い（ヘイズがかった）領域は遠景
//!   3. 上下事前分布 — 写真では画面上部ほど遠いことが多い
//!
//! 精度が必要な場合は外部デプス（MiDaS 等の出力 PNG、白 = 手前）を
//! `Params::external_depth` で渡すとそちらが優先される。

use crate::buf::{gaussian_blur, quantile, sobel_x, sobel_y, Gray};

/// ヒューリスティック深度推定。戻り値は 0(手前)..1(奥)
pub fn estimate_depth(img: &image::RgbImage, smooth_sigma: f32) -> Gray {
    let (w, h) = (img.width() as usize, img.height() as usize);
    let mut gray = Gray::new(w, h);
    let mut haze = Gray::new(w, h);
    for (i, p) in img.pixels().enumerate() {
        let r = p.0[0] as f32 / 255.0;
        let g = p.0[1] as f32 / 255.0;
        let b = p.0[2] as f32 / 255.0;
        gray.data[i] = r * 0.299 + g * 0.587 + b * 0.114;
        let mx = r.max(g).max(b);
        let mn = r.min(g).min(b);
        let sat = if mx > 1e-6 { (mx - mn) / mx } else { 0.0 };
        // 明るくて彩度が低い = ヘイズ（大気遠近で霞んだ遠景）
        haze.data[i] = (r + g + b) / 3.0 * (1.0 - sat);
    }

    // 合焦度: 軽くならしたグレースケールの勾配強度を広めにぼかして面にする
    let g1 = gaussian_blur(&gray, 1.0);
    let gx = sobel_x(&g1);
    let gy = sobel_y(&g1);
    let mut sharp = Gray::new(w, h);
    for i in 0..w * h {
        sharp.data[i] = gx.data[i].hypot(gy.data[i]);
    }
    let mut sharp = gaussian_blur(&sharp, smooth_sigma);
    let p98 = quantile(&sharp.data, 0.98) + 1e-8;
    for v in &mut sharp.data {
        *v = (*v / p98).clamp(0.0, 1.0);
    }

    let mut haze = gaussian_blur(&haze, smooth_sigma);
    let p98h = quantile(&haze.data, 0.98) + 1e-8;
    for v in &mut haze.data {
        *v = (*v / p98h).clamp(0.0, 1.0);
    }

    // 手掛かりの合成（重みは経験値）: ピンぼけ + ヘイズ + 画面上部 → 遠い
    let mut far = Gray::new(w, h);
    for y in 0..h {
        let fy = if h > 1 { y as f32 / (h - 1) as f32 } else { 0.5 };
        for x in 0..w {
            let i = y * w + x;
            far.data[i] =
                0.40 * (1.0 - sharp.data[i]) + 0.25 * haze.data[i] + 0.35 * (1.0 - fy);
        }
    }
    let mut far = gaussian_blur(&far, smooth_sigma);

    // 2〜98% 分位で 0..1 に伸長（外れ値でレンジが潰れないように）
    let lo = quantile(&far.data, 0.02);
    let hi = quantile(&far.data, 0.98);
    for v in &mut far.data {
        *v = ((*v - lo) / (hi - lo + 1e-8)).clamp(0.0, 1.0);
    }
    far
}

/// 外部デプス PNG（白 = 手前）を 0(手前)..1(奥) の Gray に変換する
pub fn from_external(dm: &image::GrayImage, w: usize, h: usize) -> Gray {
    let resized = image::imageops::resize(
        dm,
        w as u32,
        h as u32,
        image::imageops::FilterType::Triangle,
    );
    let mut out = Gray::new(w, h);
    for (i, p) in resized.pixels().enumerate() {
        out.data[i] = 1.0 - p.0[0] as f32 / 255.0;
    }
    out
}

/// 可視化: 白 = 手前（細かいタッチ）、黒 = 奥（粗いタッチ）
pub fn depth_vis(depth: &Gray) -> image::RgbImage {
    let mut out = image::RgbImage::new(depth.w as u32, depth.h as u32);
    for (i, p) in out.pixels_mut().enumerate() {
        let v = ((1.0 - depth.data[i]).clamp(0.0, 1.0) * 255.0) as u8;
        p.0 = [v, v, v];
    }
    out
}
