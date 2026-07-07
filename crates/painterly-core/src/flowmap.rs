//! グレースケール勾配 → 法線マップ / フローマップ（ストロークの方向場）。
//!
//! 不変条件: ストローク方向 = エッジの接線方向（等輝度線）で、π 周期の「向き」
//! であってベクトルではない。平滑化・リサンプリングは構造テンソル
//! （= 2θ の複素数域）上で行うこと。角度を直接平均すると 0/π の境目で壊れる。
//! Python 版 painterly/flowmap.py の移植。

use crate::buf::{gaussian_blur, Gray};

/// 高さ場と見なした勾配から接線空間法線マップを生成（全体が青紫になる）
pub fn normal_map(gx: &Gray, gy: &Gray, strength: f32) -> image::RgbImage {
    let (w, h) = (gx.w, gx.h);
    let mut out = image::RgbImage::new(w as u32, h as u32);
    for (i, p) in out.pixels_mut().enumerate() {
        let nx = -gx.data[i] * strength;
        let ny = gy.data[i] * strength;
        let nz = 1.0f32;
        let norm = (nx * nx + ny * ny + nz * nz).sqrt();
        p.0 = [
            ((nx / norm * 0.5 + 0.5) * 255.0) as u8,
            ((ny / norm * 0.5 + 0.5) * 255.0) as u8,
            ((nz / norm * 0.5 + 0.5) * 255.0) as u8,
        ];
    }
    out
}

/// 構造テンソルを平滑化した方向場。
/// 戻り値 (theta, coherence):
///   theta     — 各画素のストローク方向（エッジ接線、ラジアン、π 周期）
///   coherence — 方向の信頼度 0..1（異方性の強さ）。平坦部はほぼ 0
pub fn flow_field(gx: &Gray, gy: &Gray, smooth_sigma: f32) -> (Gray, Gray) {
    let (w, h) = (gx.w, gx.h);
    let mut jxx = Gray::new(w, h);
    let mut jxy = Gray::new(w, h);
    let mut jyy = Gray::new(w, h);
    for i in 0..w * h {
        jxx.data[i] = gx.data[i] * gx.data[i];
        jxy.data[i] = gx.data[i] * gy.data[i];
        jyy.data[i] = gy.data[i] * gy.data[i];
    }
    let jxx = gaussian_blur(&jxx, smooth_sigma);
    let jxy = gaussian_blur(&jxy, smooth_sigma);
    let jyy = gaussian_blur(&jyy, smooth_sigma);

    let mut theta = Gray::new(w, h);
    let mut coherence = Gray::new(w, h);
    for i in 0..w * h {
        // 構造テンソルの主固有ベクトル方向 = 平滑化された勾配の向き。
        // ストロークはその垂直方向に走らせる
        let phi = 0.5 * (2.0 * jxy.data[i]).atan2(jxx.data[i] - jyy.data[i]);
        theta.data[i] = phi + std::f32::consts::FRAC_PI_2;
        let tmp = ((jxx.data[i] - jyy.data[i]).powi(2) + 4.0 * jxy.data[i] * jxy.data[i]).sqrt();
        let lam1 = (jxx.data[i] + jyy.data[i] + tmp) / 2.0;
        let lam2 = (jxx.data[i] + jyy.data[i] - tmp) / 2.0;
        coherence.data[i] = if lam1 + lam2 > 1e-8 {
            (lam1 - lam2) / (lam1 + lam2 + 1e-8)
        } else {
            0.0
        };
    }
    (theta, coherence)
}

/// フローマップの可視化。方向は π 周期なので 2θ を R/G にエンコードし、
/// 振幅を抑えて下駄を履かせるとオリーブ色基調の flow map らしい見た目になる
pub fn flow_map_vis(theta: &Gray, coherence: &Gray) -> image::RgbImage {
    let (w, h) = (theta.w, theta.h);
    let mut out = image::RgbImage::new(w as u32, h as u32);
    for (i, p) in out.pixels_mut().enumerate() {
        let coh = coherence.data[i];
        let r = (2.0 * theta.data[i]).cos() * 0.22 * (0.3 + 0.7 * coh) + 0.52;
        let g = (2.0 * theta.data[i]).sin() * 0.22 * (0.3 + 0.7 * coh) + 0.50;
        let b = coh * 0.18 + 0.06;
        p.0 = [
            (r.clamp(0.0, 1.0) * 255.0) as u8,
            (g.clamp(0.0, 1.0) * 255.0) as u8,
            (b.clamp(0.0, 1.0) * 255.0) as u8,
        ];
    }
    out
}
