//! 減色（k-means++）、ポスタライズ境界線、パレットの可視化（スウォッチ / カラーホイール）。
//! Python 版 painterly/palette.py の移植。

use image::RgbImage;

use crate::buf::{luminance, quantile, reflect101};
use crate::color;
use crate::rng::Rng64;

pub struct QuantizeResult {
    pub quantized: RgbImage,
    pub labels: Vec<u16>, // h*w
    pub palette: Vec<[u8; 3]>,
    pub counts: Vec<u32>,
}

/// バイラテラルフィルタ（OpenCV bilateralFilter(d=9, sigmaColor, sigmaSpace) 相当）。
/// 減色前に毛髪・テクスチャレベルのノイズを潰し、色面の境界を綺麗につなげる。
pub fn bilateral_filter(img: &RgbImage, d: isize, sigma_color: f32, sigma_space: f32) -> RgbImage {
    let (w, h) = (img.width() as isize, img.height() as isize);
    let r = d / 2;
    let mut out = RgbImage::new(img.width(), img.height());
    // 色差重みは 3ch の絶対値和で引く（OpenCV と同じ）
    let color_lut: Vec<f32> = (0..=255 * 3)
        .map(|i| (-((i * i) as f32) / (2.0 * sigma_color * sigma_color)).exp())
        .collect();
    let mut space_w = vec![0.0f32; (d * d) as usize];
    for dy in -r..=r {
        for dx in -r..=r {
            space_w[((dy + r) * d + dx + r) as usize] =
                (-((dx * dx + dy * dy) as f32) / (2.0 * sigma_space * sigma_space)).exp();
        }
    }
    for y in 0..h {
        for x in 0..w {
            let c0 = img.get_pixel(x as u32, y as u32).0;
            let mut acc = [0.0f32; 3];
            let mut wsum = 0.0f32;
            for dy in -r..=r {
                let yy = reflect101(y + dy, h);
                for dx in -r..=r {
                    let xx = reflect101(x + dx, w);
                    let c = img.get_pixel(xx as u32, yy as u32).0;
                    let cd = (c[0] as i32 - c0[0] as i32).unsigned_abs()
                        + (c[1] as i32 - c0[1] as i32).unsigned_abs()
                        + (c[2] as i32 - c0[2] as i32).unsigned_abs();
                    let wgt = space_w[((dy + r) * d + dx + r) as usize] * color_lut[cd as usize];
                    acc[0] += c[0] as f32 * wgt;
                    acc[1] += c[1] as f32 * wgt;
                    acc[2] += c[2] as f32 * wgt;
                    wsum += wgt;
                }
            }
            out.put_pixel(
                x as u32,
                y as u32,
                image::Rgb([
                    (acc[0] / wsum).round() as u8,
                    (acc[1] / wsum).round() as u8,
                    (acc[2] / wsum).round() as u8,
                ]),
            );
        }
    }
    out
}

/// k-means++（attempts 回試行して最良のコンパクト性を採用、OpenCV cv2.kmeans 相当）
fn kmeans(
    feats: &[[f32; 3]],
    k: usize,
    attempts: usize,
    max_iter: usize,
    eps: f32,
    rng: &mut Rng64,
) -> (Vec<u16>, Vec<[f32; 3]>) {
    let k = k.min(feats.len());
    let mut best: Option<(f64, Vec<u16>, Vec<[f32; 3]>)> = None;

    for _ in 0..attempts {
        // k-means++ 初期化
        let mut centers: Vec<[f32; 3]> = Vec::with_capacity(k);
        centers.push(feats[rng.gen_range(0..feats.len())]);
        let mut d2: Vec<f32> = feats.iter().map(|f| dist2(*f, centers[0])).collect();
        while centers.len() < k {
            let total: f64 = d2.iter().map(|&v| v as f64).sum();
            let mut target = rng.gen_range_f64(0.0..1.0) * total;
            let mut idx = 0;
            for (i, &v) in d2.iter().enumerate() {
                target -= v as f64;
                if target <= 0.0 {
                    idx = i;
                    break;
                }
                idx = i;
            }
            let c = feats[idx];
            centers.push(c);
            for (i, f) in feats.iter().enumerate() {
                d2[i] = d2[i].min(dist2(*f, c));
            }
        }

        let mut labels = vec![0u16; feats.len()];
        for _ in 0..max_iter {
            // 割り当て
            for (i, f) in feats.iter().enumerate() {
                let mut bi = 0;
                let mut bd = f32::MAX;
                for (j, c) in centers.iter().enumerate() {
                    let d = dist2(*f, *c);
                    if d < bd {
                        bd = d;
                        bi = j;
                    }
                }
                labels[i] = bi as u16;
            }
            // 更新
            let mut sums = vec![[0.0f64; 3]; k];
            let mut ns = vec![0usize; k];
            for (f, &l) in feats.iter().zip(&labels) {
                let s = &mut sums[l as usize];
                s[0] += f[0] as f64;
                s[1] += f[1] as f64;
                s[2] += f[2] as f64;
                ns[l as usize] += 1;
            }
            let mut max_shift = 0.0f32;
            for j in 0..k {
                if ns[j] == 0 {
                    // 空クラスタ: ランダムな点を再割り当て
                    centers[j] = feats[rng.gen_range(0..feats.len())];
                    max_shift = f32::MAX;
                    continue;
                }
                let nc = [
                    (sums[j][0] / ns[j] as f64) as f32,
                    (sums[j][1] / ns[j] as f64) as f32,
                    (sums[j][2] / ns[j] as f64) as f32,
                ];
                max_shift = max_shift.max(dist2(nc, centers[j]).sqrt());
                centers[j] = nc;
            }
            if max_shift < eps {
                break;
            }
        }
        // 最終割り当てとコンパクト性
        let mut compact = 0.0f64;
        for (i, f) in feats.iter().enumerate() {
            let mut bi = 0;
            let mut bd = f32::MAX;
            for (j, c) in centers.iter().enumerate() {
                let d = dist2(*f, *c);
                if d < bd {
                    bd = d;
                    bi = j;
                }
            }
            labels[i] = bi as u16;
            compact += bd as f64;
        }
        if best.as_ref().map_or(true, |(bc, _, _)| compact < *bc) {
            best = Some((compact, labels, centers));
        }
    }
    let (_, labels, centers) = best.unwrap();
    (labels, centers)
}

#[inline]
fn dist2(a: [f32; 3], b: [f32; 3]) -> f32 {
    let d0 = a[0] - b[0];
    let d1 = a[1] - b[1];
    let d2 = a[2] - b[2];
    d0 * d0 + d1 * d1 + d2 * d2
}

/// 3x3 多数決フィルタでラベルの残留斑点を除去（cv2.medianBlur のラベル版）
fn majority_vote_3x3(labels: &[u16], w: usize, h: usize) -> Vec<u16> {
    let mut out = vec![0u16; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut vals = [0u16; 9];
            let mut n = 0;
            for dy in -1isize..=1 {
                let yy = reflect101(y as isize + dy, h as isize);
                for dx in -1isize..=1 {
                    let xx = reflect101(x as isize + dx, w as isize);
                    vals[n] = labels[yy * w + xx];
                    n += 1;
                }
            }
            vals.sort_unstable();
            out[y * w + x] = vals[4]; // 9 要素の中央値 = 多数決に相当
        }
    }
    out
}

/// k-means で画像を k 色に減色する。
/// space="lab" のときは Lab 空間で距離を測る（知覚的により均等）。
pub fn quantize(img: &RgbImage, k: usize, space: &str, rng: &mut Rng64) -> QuantizeResult {
    let (w, h) = (img.width() as usize, img.height() as usize);
    let smooth = bilateral_filter(img, 9, 40.0, 7.0);
    let feats: Vec<[f32; 3]> = if space == "lab" {
        smooth.pixels().map(|p| color::rgb_to_lab_u8scale(p.0)).collect()
    } else {
        smooth
            .pixels()
            .map(|p| [p.0[0] as f32, p.0[1] as f32, p.0[2] as f32])
            .collect()
    };

    let (labels, centers) = kmeans(&feats, k, 3, 25, 0.5, rng);
    let labels = majority_vote_3x3(&labels, w, h);

    let palette: Vec<[u8; 3]> = if space == "lab" {
        centers.iter().map(|c| color::lab_u8scale_to_rgb(*c)).collect()
    } else {
        centers
            .iter()
            .map(|c| {
                [
                    c[0].round().clamp(0.0, 255.0) as u8,
                    c[1].round().clamp(0.0, 255.0) as u8,
                    c[2].round().clamp(0.0, 255.0) as u8,
                ]
            })
            .collect()
    };

    let mut counts = vec![0u32; palette.len()];
    for &l in &labels {
        counts[l as usize] += 1;
    }
    let mut quantized = RgbImage::new(w as u32, h as u32);
    for (i, p) in quantized.pixels_mut().enumerate() {
        p.0 = palette[labels[i] as usize];
    }
    QuantizeResult { quantized, labels, palette, counts }
}

/// 任意の画像を最近傍のパレット色に吸着させる。
/// 低解像度の減色画像を滑らかに拡大したあと、再びパレットに「留め直す」ために使う。
pub fn apply_palette(img: &RgbImage, palette: &[[u8; 3]]) -> RgbImage {
    let mut out = RgbImage::new(img.width(), img.height());
    for (src, dst) in img.pixels().zip(out.pixels_mut()) {
        let c = src.0;
        let mut bi = 0;
        let mut bd = i32::MAX;
        for (j, p) in palette.iter().enumerate() {
            let d = (c[0] as i32 - p[0] as i32).pow(2)
                + (c[1] as i32 - p[1] as i32).pow(2)
                + (c[2] as i32 - p[2] as i32).pow(2);
            if d < bd {
                bd = d;
                bi = j;
            }
        }
        dst.0 = palette[bi];
    }
    out
}

/// 隣接画素のラベルが異なる場所 = 色面の境界。黒地にその場所の色で描く
pub fn posterize_edges(labels: &[u16], quantized: &RgbImage) -> RgbImage {
    let (w, h) = (quantized.width() as usize, quantized.height() as usize);
    let mut out = RgbImage::new(w as u32, h as u32);
    for y in 0..h {
        for x in 0..w {
            let l = labels[y * w + x];
            let e = (x > 0 && labels[y * w + x - 1] != l) || (y > 0 && labels[(y - 1) * w + x] != l);
            if e {
                out.put_pixel(x as u32, y as u32, *quantized.get_pixel(x as u32, y as u32));
            }
        }
    }
    out
}

/// 境界マスク（f32 0/1）
pub fn edge_mask(labels: &[u16], w: usize, h: usize) -> crate::buf::Gray {
    let mut out = crate::buf::Gray::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let l = labels[y * w + x];
            let e = (x > 0 && labels[y * w + x - 1] != l) || (y > 0 && labels[(y - 1) * w + x] != l);
            if e {
                out.set(x, y, 1.0);
            }
        }
    }
    out
}

/// パレットスウォッチ: 色相セグメント + 明度でソートした色タイル
pub fn palette_swatch(palette: &[[u8; 3]]) -> RgbImage {
    let cell = 22usize;
    let cols = 12usize;
    let mut order: Vec<usize> = (0..palette.len()).collect();
    order.sort_by(|&a, &b| {
        let ha = color::rgb_to_hsv(palette[a])[0] / 30;
        let hb = color::rgb_to_hsv(palette[b])[0] / 30;
        let la = luminance([
            palette[a][0] as f32,
            palette[a][1] as f32,
            palette[a][2] as f32,
        ]);
        let lb = luminance([
            palette[b][0] as f32,
            palette[b][1] as f32,
            palette[b][2] as f32,
        ]);
        ha.cmp(&hb).then(lb.partial_cmp(&la).unwrap())
    });
    let rows = palette.len().div_ceil(cols);
    let mut out = RgbImage::from_pixel(
        (cols * cell) as u32,
        (rows * cell) as u32,
        image::Rgb([45, 45, 45]),
    );
    for (i, &idx) in order.iter().enumerate() {
        let (r, c) = (i / cols, i % cols);
        for y in r * cell + 1..(r + 1) * cell - 1 {
            for x in c * cell + 1..(c + 1) * cell - 1 {
                out.put_pixel(x as u32, y as u32, image::Rgb(palette[idx]));
            }
        }
    }
    out
}

/// カラーホイール散布図: 外周に色相環、パレット色を（色相→角度, 彩度→半径）へ配置。
/// 点の面積は画素占有率に比例
pub fn color_wheel(palette: &[[u8; 3]], counts: &[u32]) -> RgbImage {
    let size = 360usize;
    let mut out = RgbImage::from_pixel(size as u32, size as u32, image::Rgb([45, 45, 45]));
    let c = size as f32 / 2.0;
    let r_outer = size as f32 * 0.46;
    let r_ring = size as f32 * 0.035;

    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - c;
            let dy = y as f32 - c;
            let rr = (dx * dx + dy * dy).sqrt();
            if rr > r_outer - r_ring && rr < r_outer {
                let ang = ((-dy).atan2(dx).to_degrees() + 360.0) % 360.0;
                let rgb = color::hsv_to_rgb([(ang / 2.0) as u8, 255, 255]);
                out.put_pixel(x as u32, y as u32, image::Rgb(rgb));
            }
        }
    }

    let total: f32 = counts.iter().sum::<u32>() as f32;
    let r_inner = r_outer - r_ring * 2.0;
    for (i, &col) in palette.iter().enumerate() {
        let hsv = color::rgb_to_hsv(col);
        let theta = (hsv[0] as f32 * 2.0).to_radians();
        let radius = hsv[1] as f32 / 255.0 * r_inner;
        let px = c + radius * theta.cos();
        let py = c - radius * theta.sin();
        let share = counts[i] as f32 / total;
        let pr = (share.sqrt() * size as f32 * 0.12).max(2.0);
        fill_circle(&mut out, px, py, pr, col);
    }
    out
}

fn fill_circle(img: &mut RgbImage, cx: f32, cy: f32, r: f32, col: [u8; 3]) {
    let (w, h) = (img.width() as i32, img.height() as i32);
    let x0 = ((cx - r).floor() as i32).max(0);
    let x1 = ((cx + r).ceil() as i32).min(w - 1);
    let y0 = ((cy - r).floor() as i32).max(0);
    let y1 = ((cy + r).ceil() as i32).min(h - 1);
    for y in y0..=y1 {
        for x in x0..=x1 {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            if dx * dx + dy * dy <= r * r {
                img.put_pixel(x as u32, y as u32, image::Rgb(col));
            }
        }
    }
}

/// 画像から使用頻度の高い色を近似的に返すユーティリティ（GUI プレビュー用途）
pub fn dominant_luma_quantile(img: &RgbImage, q: f32) -> f32 {
    let lums: Vec<f32> = img
        .pixels()
        .map(|p| luminance([p.0[0] as f32, p.0[1] as f32, p.0[2] as f32]))
        .collect();
    quantile(&lums, q)
}
