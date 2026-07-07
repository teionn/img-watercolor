//! 単純な f32 画像バッファと画像処理プリミティブ。
//! OpenCV 相当の演算（ガウシアン、Sobel、リサイズ、分位数）を pure Rust で提供する。
//! 境界は OpenCV 既定の BORDER_REFLECT_101（縁を折り返し、縁自身は繰り返さない）。

/// 1 チャンネル f32 画像
#[derive(Clone)]
pub struct Gray {
    pub w: usize,
    pub h: usize,
    pub data: Vec<f32>,
}

impl Gray {
    pub fn new(w: usize, h: usize) -> Self {
        Gray { w, h, data: vec![0.0; w * h] }
    }
    pub fn filled(w: usize, h: usize, v: f32) -> Self {
        Gray { w, h, data: vec![v; w * h] }
    }
    #[inline]
    pub fn at(&self, x: usize, y: usize) -> f32 {
        self.data[y * self.w + x]
    }
    #[inline]
    pub fn set(&mut self, x: usize, y: usize, v: f32) {
        self.data[y * self.w + x] = v;
    }
    pub fn map(&self, f: impl Fn(f32) -> f32) -> Gray {
        Gray { w: self.w, h: self.h, data: self.data.iter().map(|&v| f(v)).collect() }
    }
}

/// 3 チャンネル f32 画像（RGB、値域は用途による: 0..1 または 0..255）
#[derive(Clone)]
pub struct Rgb32 {
    pub w: usize,
    pub h: usize,
    pub data: Vec<[f32; 3]>,
}

impl Rgb32 {
    pub fn new(w: usize, h: usize) -> Self {
        Rgb32 { w, h, data: vec![[0.0; 3]; w * h] }
    }
    #[inline]
    pub fn at(&self, x: usize, y: usize) -> [f32; 3] {
        self.data[y * self.w + x]
    }
    #[inline]
    pub fn set(&mut self, x: usize, y: usize, v: [f32; 3]) {
        self.data[y * self.w + x] = v;
    }
    pub fn from_u8(img: &image::RgbImage) -> Self {
        let (w, h) = (img.width() as usize, img.height() as usize);
        let data = img
            .pixels()
            .map(|p| [p.0[0] as f32, p.0[1] as f32, p.0[2] as f32])
            .collect();
        Rgb32 { w, h, data }
    }
    /// 値域 0..1 の f32 → u8 画像（clamp あり）
    pub fn to_u8_01(&self) -> image::RgbImage {
        let mut out = image::RgbImage::new(self.w as u32, self.h as u32);
        for (i, p) in out.pixels_mut().enumerate() {
            let c = self.data[i];
            p.0 = [
                (c[0].clamp(0.0, 1.0) * 255.0).round() as u8,
                (c[1].clamp(0.0, 1.0) * 255.0).round() as u8,
                (c[2].clamp(0.0, 1.0) * 255.0).round() as u8,
            ];
        }
        out
    }
}

/// BORDER_REFLECT_101 のインデックス折り返し
#[inline]
pub fn reflect101(i: isize, n: isize) -> usize {
    if n == 1 {
        return 0;
    }
    let mut i = i;
    // 大きくはみ出すことはない（カーネル半径 << 画像サイズ）が念のためループ
    while i < 0 || i >= n {
        if i < 0 {
            i = -i;
        }
        if i >= n {
            i = 2 * n - 2 - i;
        }
    }
    i as usize
}

/// ガウシアンカーネル（OpenCV: CV_32F ではカーネル半径 ≈ 4σ）
fn gaussian_kernel(sigma: f32) -> Vec<f32> {
    let radius = (sigma * 4.0).ceil().max(1.0) as usize;
    let mut k: Vec<f32> = (0..=2 * radius)
        .map(|i| {
            let x = i as f32 - radius as f32;
            (-x * x / (2.0 * sigma * sigma)).exp()
        })
        .collect();
    let s: f32 = k.iter().sum();
    for v in &mut k {
        *v /= s;
    }
    k
}

/// 分離型ガウシアンぼかし（reflect101 境界）
pub fn gaussian_blur(src: &Gray, sigma: f32) -> Gray {
    if sigma <= 0.0 {
        return src.clone();
    }
    let k = gaussian_kernel(sigma);
    let r = (k.len() / 2) as isize;
    let (w, h) = (src.w, src.h);
    let mut tmp = Gray::new(w, h);
    for y in 0..h {
        let row = &src.data[y * w..(y + 1) * w];
        for x in 0..w {
            let mut acc = 0.0;
            for (j, &kv) in k.iter().enumerate() {
                let xi = reflect101(x as isize + j as isize - r, w as isize);
                acc += row[xi] * kv;
            }
            tmp.data[y * w + x] = acc;
        }
    }
    let mut out = Gray::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let mut acc = 0.0;
            for (j, &kv) in k.iter().enumerate() {
                let yi = reflect101(y as isize + j as isize - r, h as isize);
                acc += tmp.data[yi * w + x] * kv;
            }
            out.data[y * w + x] = acc;
        }
    }
    out
}

/// Sobel 3x3（dx=1, dy=0）: [-1 0 1] ⊗ [1 2 1]
pub fn sobel_x(src: &Gray) -> Gray {
    convolve3(src, &[[-1.0, 0.0, 1.0], [-2.0, 0.0, 2.0], [-1.0, 0.0, 1.0]])
}

/// Sobel 3x3（dx=0, dy=1）
pub fn sobel_y(src: &Gray) -> Gray {
    convolve3(src, &[[-1.0, -2.0, -1.0], [0.0, 0.0, 0.0], [1.0, 2.0, 1.0]])
}

fn convolve3(src: &Gray, k: &[[f32; 3]; 3]) -> Gray {
    let (w, h) = (src.w, src.h);
    let mut out = Gray::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let mut acc = 0.0;
            for ky in 0..3 {
                let yi = reflect101(y as isize + ky as isize - 1, h as isize);
                for kx in 0..3 {
                    let xi = reflect101(x as isize + kx as isize - 1, w as isize);
                    acc += src.data[yi * w + xi] * k[ky][kx];
                }
            }
            out.data[y * w + x] = acc;
        }
    }
    out
}

/// f32 グレースケールの双線形リサイズ
pub fn resize_gray_bilinear(src: &Gray, nw: usize, nh: usize) -> Gray {
    let mut out = Gray::new(nw, nh);
    let sx = src.w as f32 / nw as f32;
    let sy = src.h as f32 / nh as f32;
    for y in 0..nh {
        let fy = ((y as f32 + 0.5) * sy - 0.5).clamp(0.0, src.h as f32 - 1.0);
        let y0 = fy.floor() as usize;
        let y1 = (y0 + 1).min(src.h - 1);
        let ty = fy - y0 as f32;
        for x in 0..nw {
            let fx = ((x as f32 + 0.5) * sx - 0.5).clamp(0.0, src.w as f32 - 1.0);
            let x0 = fx.floor() as usize;
            let x1 = (x0 + 1).min(src.w - 1);
            let tx = fx - x0 as f32;
            let a = src.at(x0, y0) * (1.0 - tx) + src.at(x1, y0) * tx;
            let b = src.at(x0, y1) * (1.0 - tx) + src.at(x1, y1) * tx;
            out.set(x, y, a * (1.0 - ty) + b * ty);
        }
    }
    out
}

/// f32 グレースケールの面積平均リサイズ（縮小用、OpenCV INTER_AREA 相当）
pub fn resize_gray_area(src: &Gray, nw: usize, nh: usize) -> Gray {
    if nw >= src.w || nh >= src.h {
        return resize_gray_bilinear(src, nw, nh);
    }
    let mut out = Gray::new(nw, nh);
    let sx = src.w as f32 / nw as f32;
    let sy = src.h as f32 / nh as f32;
    for y in 0..nh {
        let y0 = y as f32 * sy;
        let y1 = (y as f32 + 1.0) * sy;
        for x in 0..nw {
            let x0 = x as f32 * sx;
            let x1 = (x as f32 + 1.0) * sx;
            let mut acc = 0.0;
            let mut area = 0.0;
            let iy0 = y0.floor() as usize;
            let iy1 = (y1.ceil() as usize).min(src.h);
            let ix0 = x0.floor() as usize;
            let ix1 = (x1.ceil() as usize).min(src.w);
            for iy in iy0..iy1 {
                let wy = (y1.min(iy as f32 + 1.0) - y0.max(iy as f32)).max(0.0);
                for ix in ix0..ix1 {
                    let wx = (x1.min(ix as f32 + 1.0) - x0.max(ix as f32)).max(0.0);
                    acc += src.at(ix, iy) * wx * wy;
                    area += wx * wy;
                }
            }
            out.set(x, y, acc / area.max(1e-8));
        }
    }
    out
}

/// numpy.percentile / quantile（線形補間）と同じ定義
pub fn quantile(values: &[f32], q: f32) -> f32 {
    assert!(!values.is_empty());
    let mut v: Vec<f32> = values.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let pos = q.clamp(0.0, 1.0) * (v.len() - 1) as f32;
    let lo = pos.floor() as usize;
    let hi = pos.ceil() as usize;
    let t = pos - lo as f32;
    v[lo] * (1.0 - t) + v[hi] * t
}

/// 明度（「暗→明」ソート用）。RGB は 0..1 の f32
#[inline]
pub fn luminance(rgb: [f32; 3]) -> f32 {
    rgb[0] * 0.299 + rgb[1] * 0.587 + rgb[2] * 0.114
}

/// 長辺を指定 px に縮小（拡大はしない）。u8 RGB 画像用
pub fn resize_to_long_side(img: &image::RgbImage, long_side: u32) -> image::RgbImage {
    let (w, h) = (img.width(), img.height());
    let scale = long_side as f32 / w.max(h) as f32;
    if scale >= 1.0 {
        return img.clone();
    }
    let nw = (w as f32 * scale).round().max(1.0) as u32;
    let nh = (h as f32 * scale).round().max(1.0) as u32;
    image::imageops::resize(img, nw, nh, image::imageops::FilterType::Triangle)
}

/// u8 RGB の双線形リサイズ
pub fn resize_rgb_bilinear(img: &image::RgbImage, nw: u32, nh: u32) -> image::RgbImage {
    image::imageops::resize(img, nw, nh, image::imageops::FilterType::Triangle)
}

/// u8 RGB のキュービック（Catmull-Rom）リサイズ — 最終出力の拡大用
pub fn resize_rgb_cubic(img: &image::RgbImage, nw: u32, nh: u32) -> image::RgbImage {
    image::imageops::resize(img, nw, nh, image::imageops::FilterType::CatmullRom)
}
