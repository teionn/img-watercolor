//! ブラシテクスチャ: グレースケールの alpha「スタンプ」をプロシージャル生成し、
//! 1 ストローク = 軌跡に沿って一定間隔・局所流向に回転させながら繰り返し押す
//! （Photoshop ブラシの先端スタンプと同じ考え方。カスタム先端は load_custom で登録）。
//!
//! テクスチャの約束事: ストローク方向が +x、ブラシ幅が y。
//! Python 版 painterly/brushes.py の移植。

use std::collections::HashMap;

use crate::buf::{gaussian_blur, resize_gray_area, Gray};
use crate::rng::Rng64;

pub const ANGLE_BINS: usize = 24;
pub const BUILTIN_BRUSHES: [&str; 11] = [
    // 油彩・汎用
    "triangle", "flat", "soft", "oil", "pastel", "charcoal",
    // 塗りのスタイル別（プリセットが使い分ける）
    "wash",     // 水彩の平塗り
    "bleed",    // 水彩のにじみ
    "drybrush", // かすれ筆（ドライブラシ）
    "pencil",   // 鉛筆の芯
    "impasto",  // 油彩の厚塗り
];

const TEX_SIZE: usize = 96;

/// ブラシのベーステクスチャとスタンプキャッシュを持つ。
/// Python 版のモジュールグローバルに相当（Rust ではエンジンごとに保持）。
pub struct Brushes {
    base: HashMap<String, Gray>,
    stamps: HashMap<(String, usize, usize), Gray>,
    noise_cache: HashMap<String, Gray>,
}

impl Default for Brushes {
    fn default() -> Self {
        Self::new()
    }
}

impl Brushes {
    pub fn new() -> Self {
        Brushes { base: HashMap::new(), stamps: HashMap::new(), noise_cache: HashMap::new() }
    }

    /// 帯域制限ノイズ（キーごとにシードを固定し、同スタイルのブラシ形状を安定させる）
    fn noise(&mut self, w: usize, h: usize, sigma: f32, key: &str) -> Gray {
        let cache_key = format!("{key}:{w}x{h}:{sigma}");
        if let Some(n) = self.noise_cache.get(&cache_key) {
            return n.clone();
        }
        // キー文字列から決定的にシードを作る（FNV-1a）
        let mut seed = 0xcbf29ce484222325u64;
        for b in key.bytes() {
            seed ^= b as u64;
            seed = seed.wrapping_mul(0x100000001b3);
        }
        let mut rng = Rng64::seed_from(seed);
        let mut n = Gray::new(w, h);
        for v in &mut n.data {
            *v = rng.random();
        }
        let mut n = gaussian_blur(&n, sigma);
        let mn = n.data.iter().cloned().fold(f32::MAX, f32::min);
        let mx = n.data.iter().cloned().fold(f32::MIN, f32::max);
        for v in &mut n.data {
            *v = (*v - mn) / (mx - mn + 1e-8);
        }
        self.noise_cache.insert(cache_key, n.clone());
        n
    }

    /// 基準ブラシ alpha（0..1）を生成。ストローク方向 = +x。
    ///
    /// すべてソフトエッジにする——alpha は滑らかな減衰で、二値マスクは使わない。
    /// キャンバスは低解像度なので、硬いエッジは拡大後に「ブロック」に見えてしまう。
    fn base_texture(&mut self, style: &str) -> Gray {
        let s = TEX_SIZE;
        let c = (s as f32 - 1.0) / 2.0;
        let half = s as f32 / 2.0;
        let mut a = Gray::new(s, s);

        match style {
            "soft" => {
                // ソフト円ブラシ: ストローク方向に少し伸ばしたガウシアン + ごく薄い剛毛
                // テクスチャ。広い面を塗ったとき流向に沿う筋が残る。純ガウスだとエアブラシになる
                let bristle = self.noise(1, s, 1.8, "soft_bristle");
                for y in 0..s {
                    let v = (y as f32 - c) / half;
                    for x in 0..s {
                        let u = (x as f32 - c) / half;
                        let core = 0.9
                            * (-(u * u / (2.0 * 0.42 * 0.42) + v * v / (2.0 * 0.30 * 0.30))).exp();
                        a.set(x, y, core * (0.86 + 0.14 * bristle.at(0, y)));
                    }
                }
            }
            "flat" => {
                // 平筆: 幅方向を狭くフェザリングした矩形の腹 + 剛毛の縞 + ノイズの毛羽、両端は収まる
                let ragged = self.noise(s, s, 3.0, "flat_edge");
                let bristle = self.noise(1, s, 1.4, "flat_bristle");
                for y in 0..s {
                    let v = (y as f32 - c) / half;
                    for x in 0..s {
                        let u = (x as f32 - c) / half;
                        let rag = ragged.at(x, y) * 0.22;
                        let body_v = 1.0 - smoothstep(0.58 - rag, 0.88, v.abs());
                        let body_u = 1.0 - smoothstep(0.42, 0.95, u.abs());
                        a.set(x, y, body_v * body_u * (0.70 + 0.30 * bristle.at(0, y)));
                    }
                }
                a = gaussian_blur(&a, s as f32 * 0.010);
            }
            "triangle" => {
                // 三角ナイフ: ペインティングナイフ風の三角先端。芯は硬く縁だけ僅かに柔らかい
                let mut tri = Gray::new(s, s);
                let p0 = (c - s as f32 * 0.30, c - s as f32 * 0.34);
                let p1 = (c - s as f32 * 0.30, c + s as f32 * 0.34);
                let p2 = (c + s as f32 * 0.40, c);
                fill_triangle(&mut tri, p0, p1, p2);
                let tri = gaussian_blur(&tri, s as f32 * 0.018);
                let grain = self.noise(s, s, 2.0, "tri_grain");
                for i in 0..s * s {
                    a.data[i] = (tri.data[i] * 1.25).clamp(0.0, 1.0)
                        * (0.85 + 0.15 * grain.data[i]);
                }
            }
            "charcoal" => {
                let grain = self.noise(s, s, 0.9, "charcoal_grain");
                for y in 0..s {
                    let v = (y as f32 - c) / half;
                    for x in 0..s {
                        let u = (x as f32 - c) / half;
                        let r = (u * u + v * v).sqrt();
                        let core = (1.2 - r * 1.35).clamp(0.0, 1.0);
                        a.set(x, y, core * (grain.at(x, y) * 1.6 - 0.25).clamp(0.0, 1.0));
                    }
                }
                a = gaussian_blur(&a, s as f32 * 0.012);
            }
            "pastel" => {
                let paper = self.noise(s, s, 1.6, "pastel_paper");
                for y in 0..s {
                    let v = (y as f32 - c) / half;
                    for x in 0..s {
                        let u = (x as f32 - c) / half;
                        let r = (u * u + v * v).sqrt();
                        let core = (1.35 - r * 1.4).clamp(0.0, 1.0).powf(0.7);
                        a.set(x, y, core * (0.35 + 0.65 * smoothstep(0.30, 0.55, paper.at(x, y))));
                    }
                }
            }
            "oil" => {
                // 油彩ブラシ: ソフトエッジの腹 + 剛毛の溝（厚塗り感）
                let bristle = self.noise(1, s, 1.6, "oil_bristle");
                for y in 0..s {
                    let v = (y as f32 - c) / half;
                    for x in 0..s {
                        let u = (x as f32 - c) / half;
                        let body = (1.0 - smoothstep(0.35, 1.0, v.abs()))
                            * (1.0 - smoothstep(0.25, 1.0, u.abs()));
                        a.set(x, y, body * (0.55 + 0.45 * bristle.at(0, y)));
                    }
                }
            }
            "wash" => {
                // 水彩の平塗り: 輪郭が緩く歪んだ水たまり。内部は低周波のむら、
                // 縁はわずかに濃い（edge_darken と相乗して顔料溜まりになる）
                let warp = self.noise(s, s, 6.0, "wash_warp");
                let mottle = self.noise(s, s, 4.0, "wash_mottle");
                for y in 0..s {
                    let v = (y as f32 - c) / half;
                    for x in 0..s {
                        let u = (x as f32 - c) / half;
                        let r = (u * u + v * v).sqrt() + (warp.at(x, y) - 0.5) * 0.30;
                        let body = 1.0 - smoothstep(0.55, 0.95, r);
                        let inner = 0.72 + 0.16 * mottle.at(x, y);
                        let rim = 0.22 * smoothstep(0.30, 0.85, r);
                        a.set(x, y, body * (inner + rim));
                    }
                }
            }
            "bleed" => {
                // 水彩のにじみ: 輪郭が大きく揺らぐ水たまり（wet-in-wet の置き染み）
                let warp = self.noise(s, s, 4.0, "bleed_warp");
                let mottle = self.noise(s, s, 3.0, "bleed_mottle");
                for y in 0..s {
                    let v = (y as f32 - c) / half;
                    for x in 0..s {
                        let u = (x as f32 - c) / half;
                        let r = (u * u + v * v).sqrt() + (warp.at(x, y) - 0.5) * 0.55;
                        let body = 1.0 - smoothstep(0.35, 0.85, r);
                        a.set(x, y, body * (0.60 + 0.40 * mottle.at(x, y)));
                    }
                }
            }
            "drybrush" => {
                // かすれ筆: 穂先の束がところどころ抜け、紙の凸だけに絵具が残る
                let bristle = self.noise(1, s, 0.8, "dry_bristle");
                let grain = self.noise(s, s, 1.2, "dry_grain");
                for y in 0..s {
                    let v = (y as f32 - c) / half;
                    let streak = smoothstep(0.40, 0.62, bristle.at(0, y));
                    for x in 0..s {
                        let u = (x as f32 - c) / half;
                        let body = (1.0 - smoothstep(0.55, 0.95, v.abs()))
                            * (1.0 - smoothstep(0.50, 1.0, u.abs()));
                        a.set(x, y, body * streak * (0.45 + 0.55 * grain.at(x, y)));
                    }
                }
            }
            "pencil" => {
                // 鉛筆の芯: 小さめの芯 + 強い紙目（tooth）で粒状に削れる
                let tooth = self.noise(s, s, 0.7, "pencil_tooth");
                for y in 0..s {
                    let v = (y as f32 - c) / half;
                    for x in 0..s {
                        let u = (x as f32 - c) / half;
                        let r = (u * u + v * v).sqrt();
                        let core = (1.15 - r * 1.5).clamp(0.0, 1.0).powf(0.8);
                        a.set(x, y, core * smoothstep(0.30, 0.72, tooth.at(x, y)));
                    }
                }
            }
            "impasto" => {
                // 厚塗り: 深い剛毛の溝 + 絵具の塊、端は不規則に欠ける
                let bristle = self.noise(1, s, 1.0, "impasto_bristle");
                let clump = self.noise(s, s, 2.2, "impasto_clump");
                let ragged = self.noise(s, s, 3.0, "impasto_edge");
                for y in 0..s {
                    let v = (y as f32 - c) / half;
                    for x in 0..s {
                        let u = (x as f32 - c) / half;
                        let body = (1.0
                            - smoothstep(0.50 - ragged.at(x, y) * 0.25, 0.90, v.abs()))
                            * (1.0 - smoothstep(0.35, 0.95, u.abs()));
                        let paint = (0.35 + 0.65 * bristle.at(0, y))
                            * (0.55 + 0.45 * smoothstep(0.25, 0.60, clump.at(x, y)));
                        a.set(x, y, body * paint);
                    }
                }
            }
            other => panic!("unknown brush style: {other}"),
        }
        for v in &mut a.data {
            *v = v.clamp(0.0, 1.0);
        }
        a
    }

    /// 回転 + 縮尺済みのスタンプを取得（角度は ANGLE_BINS 段階に量子化してキャッシュ）
    pub fn get_stamp(&mut self, style: &str, radius: f32, angle: f32) -> Gray {
        let rad = (radius.round() as isize).max(2) as usize;
        let abin = {
            let b = (angle / std::f32::consts::PI * ANGLE_BINS as f32).round() as isize;
            b.rem_euclid(ANGLE_BINS as isize) as usize
        };
        let key = (style.to_string(), rad, abin);
        if let Some(st) = self.stamps.get(&key) {
            return st.clone();
        }
        if !self.base.contains_key(style) {
            let tex = self.base_texture(style);
            self.base.insert(style.to_string(), tex);
        }
        let base = self.base[style].clone();
        let d = rad * 2 + 1;
        let tex = resize_gray_area(&base, d, d);
        let ang_deg = (abin as f32 * std::f32::consts::PI / ANGLE_BINS as f32).to_degrees();
        let mut stamp = rotate_bilinear(&tex, ang_deg);
        if rad < 5 {
            stamp = gaussian_blur(&stamp, 0.6); // 極小スタンプのジャギー・ブロック感を除去
        }
        self.stamps.insert(key, stamp.clone());
        stamp
    }

    /// Photoshop から書き出したグレースケール PNG（白 = 着色部）をブラシとして登録
    pub fn load_custom(&mut self, path: &str, style_name: &str) -> Result<(), String> {
        let img = image::open(path).map_err(|e| format!("{path}: {e}"))?.to_luma8();
        let (w, h) = (img.width() as usize, img.height() as usize);
        let s = w.max(h);
        let mut pad = Gray::new(s, s);
        for y in 0..h {
            for x in 0..w {
                pad.set(x, y, img.get_pixel(x as u32, y as u32).0[0] as f32 / 255.0);
            }
        }
        self.base.insert(style_name.to_string(), pad);
        Ok(())
    }
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0 + 1e-8)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// 三角形の塗りつぶし（cv2.fillPoly 相当、値 1.0）
fn fill_triangle(img: &mut Gray, p0: (f32, f32), p1: (f32, f32), p2: (f32, f32)) {
    let (w, h) = (img.w as f32, img.h as f32);
    let ymin = p0.1.min(p1.1).min(p2.1).floor().max(0.0) as usize;
    let ymax = p0.1.max(p1.1).max(p2.1).ceil().min(h - 1.0) as usize;
    let edge = |a: (f32, f32), b: (f32, f32), p: (f32, f32)| -> f32 {
        (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0)
    };
    for y in ymin..=ymax {
        for x in 0..w as usize {
            let p = (x as f32 + 0.0, y as f32 + 0.0);
            let d0 = edge(p0, p1, p);
            let d1 = edge(p1, p2, p);
            let d2 = edge(p2, p0, p);
            let neg = d0 < 0.0 || d1 < 0.0 || d2 < 0.0;
            let pos = d0 > 0.0 || d1 > 0.0 || d2 > 0.0;
            if !(neg && pos) {
                img.set(x, y, 1.0);
            }
        }
    }
}

/// 中心回りの回転（cv2.warpAffine + getRotationMatrix2D 相当、双線形、はみ出しは 0）。
/// 正の角度 = 反時計回り（画像座標系）
fn rotate_bilinear(src: &Gray, ang_deg: f32) -> Gray {
    let d = src.w;
    let c = d as f32 / 2.0;
    let a = ang_deg.to_radians();
    let (sin, cos) = a.sin_cos();
    let mut out = Gray::new(d, d);
    for y in 0..d {
        for x in 0..d {
            // 逆写像: dst → src
            let dx = x as f32 - c;
            let dy = y as f32 - c;
            let sx = cos * dx - sin * dy + c;
            let sy = sin * dx + cos * dy + c;
            if sx < 0.0 || sy < 0.0 || sx > d as f32 - 1.0 || sy > d as f32 - 1.0 {
                continue;
            }
            let x0 = sx.floor() as usize;
            let y0 = sy.floor() as usize;
            let x1 = (x0 + 1).min(d - 1);
            let y1 = (y0 + 1).min(d - 1);
            let tx = sx - x0 as f32;
            let ty = sy - y0 as f32;
            let v = src.at(x0, y0) * (1.0 - tx) * (1.0 - ty)
                + src.at(x1, y0) * tx * (1.0 - ty)
                + src.at(x0, y1) * (1.0 - tx) * ty
                + src.at(x1, y1) * tx * ty;
            out.set(x, y, v);
        }
    }
    out
}
