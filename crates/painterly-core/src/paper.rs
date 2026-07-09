//! 紙の表現（Waterlogue 参考）: 紙のテクスチャと、描画領域の外に残す
//! 白い紙の余白。どちらも最終出力解像度で適用する（拡大でボケさせない）。

use image::RgbImage;

use crate::buf::{gaussian_blur, Gray};
use crate::rng::Rng64;

/// 紙の地色（わずかに温かみのあるオフホワイト）
const PAPER: [f32; 3] = [247.0, 245.0, 239.0];

fn normalized_noise(w: usize, h: usize, sigma: f32, rng: &mut Rng64) -> Gray {
    let mut n = Gray::new(w, h);
    for v in &mut n.data {
        *v = rng.random();
    }
    let mut n = gaussian_blur(&n, sigma);
    let mn = n.data.iter().cloned().fold(f32::MAX, f32::min);
    let mx = n.data.iter().cloned().fold(f32::MIN, f32::max);
    let range = (mx - mn).max(1e-8);
    for v in &mut n.data {
        *v = (*v - mn) / range;
    }
    n
}

#[inline]
fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// 紙のテクスチャ（texture 0..1）と紙の余白（border: 短辺に対する比率、0 = なし）を適用。
/// シード固定で再現可能
pub fn apply_paper(img: &mut RgbImage, texture: f32, border: f32, seed: u64) {
    let (w, h) = (img.width() as usize, img.height() as usize);
    if w == 0 || h == 0 || (texture <= 0.0 && border <= 0.0) {
        return;
    }
    let mut rng = Rng64::seed_from(seed ^ 0x70617065); // "pape"

    // 細かい紙目 + 粗いむら（コールドプレス紙の印象）
    let fine = normalized_noise(w, h, 1.1, &mut rng);
    let coarse = normalized_noise(w, h, (w.min(h) as f32 * 0.01).max(4.0), &mut rng);

    // 余白の荒れたエッジ用の低周波ノイズ
    let edge_noise = if border > 0.0 {
        Some(normalized_noise(w, h, (w.min(h) as f32 * 0.02).max(6.0), &mut rng))
    } else {
        None
    };
    let bw = border * w.min(h) as f32;

    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let px = img.get_pixel_mut(x as u32, y as u32);
            let mut c = [px.0[0] as f32, px.0[1] as f32, px.0[2] as f32];

            if texture > 0.0 {
                // 紙目は乗算的に（明部でわずかに目立ち、暗部で沈む）
                let g = 1.0
                    - texture * (0.10 * (fine.data[i] - 0.5) + 0.08 * (coarse.data[i] - 0.5));
                for v in &mut c {
                    *v = (*v * g).clamp(0.0, 255.0);
                }
            }

            if let Some(en) = &edge_noise {
                // 画像端からの距離を余白幅で正規化し、ノイズで縁を荒らす
                let d = (x.min(w - 1 - x)).min(y.min(h - 1 - y)) as f32;
                let e = d / bw.max(1.0) + (en.data[i] - 0.5) * 0.7;
                // e < 1 が余白側。荒れた遷移帯を持たせる
                let a = smoothstep(0.85, 1.15, e);
                if a < 1.0 {
                    // 余白にもうっすら紙目を乗せる
                    let g = 1.0 - 0.06 * (fine.data[i] - 0.5);
                    for ch in 0..3 {
                        let paper = (PAPER[ch] * g).clamp(0.0, 255.0);
                        c[ch] = paper * (1.0 - a) + c[ch] * a;
                    }
                }
            }

            px.0 = [c[0] as u8, c[1] as u8, c[2] as u8];
        }
    }
}
