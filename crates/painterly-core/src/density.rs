//! 密度マップ: 画面の「描き込み密度」に応じてブラシを持ち替える——
//! 輪郭や狭い場所は小さなハードブラシ、広く低コントラストな場所は大きなソフトブラシ。
//!
//! 密度 = 正規化( ぼかし(勾配強度) + ぼかし(ポスタライズ境界) )
//! Python 版 painterly/density.py の移植。

use crate::buf::{gaussian_blur, quantile, Gray};

pub fn density_map(gx: &Gray, gy: &Gray, poster_edge: &Gray, sigma: f32, edge_weight: f32) -> Gray {
    let (w, h) = (gx.w, gx.h);
    let mut mag = Gray::new(w, h);
    for i in 0..w * h {
        mag.data[i] = gx.data[i].hypot(gy.data[i]);
    }
    // 分位数で正規化して極値に全体が潰されるのを防ぐ
    let p98 = quantile(&mag.data, 0.98) + 1e-8;
    for v in &mut mag.data {
        *v /= p98;
    }
    let b1 = gaussian_blur(&mag, sigma);
    let b2 = gaussian_blur(poster_edge, sigma);
    let mut d = Gray::new(w, h);
    for i in 0..w * h {
        d.data[i] = b1.data[i] + edge_weight * b2.data[i];
    }
    let p95 = quantile(&d.data, 0.95) + 1e-8;
    for v in &mut d.data {
        *v = (*v / p95).clamp(0.0, 1.0);
    }
    d
}

/// 赤 = 高密度（小さいハードブラシ）、青 = 低密度（大きいソフトブラシ）
pub fn density_vis(density: &Gray, hard_threshold: f32) -> image::RgbImage {
    let mut out = image::RgbImage::new(density.w as u32, density.h as u32);
    for (i, p) in out.pixels_mut().enumerate() {
        let d = density.data[i];
        if d >= hard_threshold {
            p.0 = [(d * 255.0) as u8, 0, 0];
        } else {
            p.0 = [0, 0, ((1.0 - d) * 255.0) as u8];
        }
    }
    out
}

/// 密度 → ブラシ半径（高密度ほど小さく）。gamma < 1 で中密度も小さめに寄せ、
/// 輪郭の締まりを保つ
pub fn brush_size_at(density: f32, size_min: f32, size_max: f32) -> f32 {
    const GAMMA: f32 = 0.7;
    size_max + (size_min - size_max) * density.powf(GAMMA)
}
