//! ストローク描画コア。手描き油彩の手順に沿って構成する:
//!
//!   1. ストロークはフローに沿って「流れる」——種点から方向場に沿って両方向に折れ線を伸ばす;
//!   2. 全体を明度で 暗 → 明 にソートして描画——明るい色は常に暗い色の上に載る;
//!   3. 色は減色画像から取り、形はポスタライズ境界に暗黙に拘束される
//!      （トレース中に色が急変したら停止）;
//!   4. 密度がブラシの大きさと硬さを決め、一部のストロークは「横」から色を借りて混ぜる。
//!
//! Python 版 painterly/strokes.py の移植。

use crate::brushes::Brushes;
use crate::buf::{luminance, Gray, Rgb32};
use crate::rng::Rng64;

#[derive(Clone)]
pub struct Stroke {
    pub points: Vec<(f32, f32)>,
    pub color: [f32; 3],
    pub radius: f32,
    pub style: String,
    pub opacity: f32,
    /// 種点の流向。1 スタンプだけのストロークは走向が計算できないのでこれで代用
    pub angle0: f32,
    /// hard / standard / soft。デバッグビューの色分け用
    pub tag: Tag,
    pub lum: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tag {
    Hard,
    Standard,
    Soft,
}

pub struct PaintEngine<'a> {
    pub target: &'a Rgb32, // 0..1 の減色ターゲット（描画解像度）
    pub theta: &'a Gray,
    pub coherence: &'a Gray,
    pub density: &'a Gray,
    pub w: usize,
    pub h: usize,
}

impl<'a> PaintEngine<'a> {
    pub fn new(target: &'a Rgb32, theta: &'a Gray, coherence: &'a Gray, density: &'a Gray) -> Self {
        PaintEngine { target, theta, coherence, density, w: target.w, h: target.h }
    }

    #[inline]
    fn dir_at(&self, x: f32, y: f32) -> (f32, f32) {
        let th = self.theta.at(x as usize, y as usize);
        (th.cos(), th.sin())
    }

    /// 種点から方向場に沿って両方向に折れ線を伸ばす。方向場は π 周期なので、
    /// 毎ステップ前回と同じ側に向きを揃えないとストロークが折り返してしまう。
    /// ターゲット色の急変（≈ ポスタライズ境界越え）か画面外で停止。
    pub fn trace(&self, x0: f32, y0: f32, radius: f32, color: [f32; 3], max_len_factor: f32) -> Vec<(f32, f32)> {
        const COLOR_TOL: f32 = 0.10;
        let step = (radius * 0.4).max(1.5);
        let max_steps = ((radius * max_len_factor / step) as usize).max(2);
        let mut pts = vec![(x0, y0)];
        for direction in [1.0f32, -1.0] {
            let (mut cx, mut cy) = (x0, y0);
            let (mut dx, mut dy) = self.dir_at(x0, y0);
            dx *= direction;
            dy *= direction;
            for _ in 0..max_steps / 2 {
                let (mut ux, mut uy) = self.dir_at(cx, cy);
                if ux * dx + uy * dy < 0.0 {
                    ux = -ux;
                    uy = -uy;
                }
                cx += ux * step;
                cy += uy * step;
                if cx < 0.0 || cx >= self.w as f32 || cy < 0.0 || cy >= self.h as f32 {
                    break;
                }
                let t = self.target.at(cx as usize, cy as usize);
                let dc = ((t[0] - color[0]).powi(2)
                    + (t[1] - color[1]).powi(2)
                    + (t[2] - color[2]).powi(2))
                .sqrt();
                if dc > COLOR_TOL {
                    break;
                }
                if direction > 0.0 {
                    pts.push((cx, cy));
                } else {
                    pts.insert(0, (cx, cy));
                }
                dx = ux;
                dy = uy;
            }
        }
        pts
    }

    /// ジッター付きグリッドに種を撒いてストローク列を生成する（この時点では未描画）。
    /// size_of(density) -> 半径、style_of(density) -> ブラシ名、mask は種領域の制限、
    /// tag_of(density) -> hard/standard/soft。
    #[allow(clippy::too_many_arguments)]
    pub fn make_strokes(
        &self,
        rng: &mut Rng64,
        spacing: f32,
        size_of: &mut dyn FnMut(f32, &mut Rng64) -> f32,
        style_of: &dyn Fn(f32) -> String,
        tag_of: &dyn Fn(f32) -> Tag,
        mask: Option<&[bool]>,
        side_sample_prob: f32,
    ) -> Vec<Stroke> {
        const JITTER: f32 = 0.5;
        const MAX_LEN_FACTOR: f32 = 3.0;
        let mut strokes = Vec::new();
        let mut gy = spacing / 2.0;
        while gy < self.h as f32 {
            let mut gx = spacing / 2.0;
            while gx < self.w as f32 {
                let x = gx + rng.uniform(-1.0, 1.0) * spacing * JITTER;
                let y = gy + rng.uniform(-1.0, 1.0) * spacing * JITTER;
                let xi = (x.clamp(0.0, self.w as f32 - 1.0)) as usize;
                let yi = (y.clamp(0.0, self.h as f32 - 1.0)) as usize;
                let d = self.density.at(xi, yi);
                if let Some(m) = mask {
                    if !m[yi * self.w + xi] {
                        gx += spacing;
                        continue;
                    }
                }
                let radius = size_of(d, rng);
                let base_color = self.target.at(xi, yi);
                let mut color = base_color;

                // 画家が隣の色面から「色を借りる」癖の模倣: 一部のストロークは
                // 流向と垂直方向にオフセットした場所から採色して混ぜる
                if rng.random() < side_sample_prob {
                    let (ux, uy) = self.dir_at(xi as f32, yi as f32);
                    let off = radius * 1.2 * if rng.random() < 0.5 { 1.0 } else { -1.0 };
                    let sx = (xi as f32 - uy * off).clamp(0.0, self.w as f32 - 1.0) as usize;
                    let sy = (yi as f32 + ux * off).clamp(0.0, self.h as f32 - 1.0) as usize;
                    let s = self.target.at(sx, sy);
                    color = [
                        color[0] * 0.6 + s[0] * 0.4,
                        color[1] * 0.6 + s[1] * 0.4,
                        color[2] * 0.6 + s[2] * 0.4,
                    ];
                }

                let pts = self.trace(xi as f32, yi as f32, radius, base_color, MAX_LEN_FACTOR);
                let tag = tag_of(d);
                // ハードブラシは輪郭を「噛ませる」ため不透明度を高く、
                // ソフトブラシは重ね塗りしやすいよう低めに
                let (lo, hi) = match tag {
                    Tag::Hard => (0.90, 1.0),
                    Tag::Standard => (0.82, 0.96),
                    Tag::Soft => (0.75, 0.92),
                };
                let lum = luminance(color);
                strokes.push(Stroke {
                    points: pts,
                    color,
                    radius,
                    style: style_of(d),
                    opacity: rng.uniform(lo, hi),
                    angle0: self.theta.at(xi, yi),
                    tag,
                    lum,
                });
                gx += spacing;
            }
            gy += spacing;
        }
        strokes
    }
}

/// 明度で暗 → 明 にソートして 1 本ずつスタンプする（油彩の順序: 暗で下塗り、明を後に載せる）。
/// wet: ウェットブレンディング——筆を置く前にキャンバスの既存色と混ぜる比率。
/// これでストローク同士が互いに色を「引きずる」。canvas は 0..1 の f32 RGB、in-place 更新。
pub fn render(
    canvas: &mut Rgb32,
    strokes: &[Stroke],
    wet: f32,
    brushes: &mut Brushes,
    mut on_stroke: Option<&mut dyn FnMut(usize, &Rgb32)>,
) {
    let (w, h) = (canvas.w, canvas.h);
    let mut order: Vec<usize> = (0..strokes.len()).collect();
    order.sort_by(|&a, &b| strokes[a].lum.partial_cmp(&strokes[b].lum).unwrap());
    for (i, &si) in order.iter().enumerate() {
        let st = &strokes[si];
        let mut color = st.color;
        if wet > 0.0 {
            let (mx, my) = st.points[st.points.len() / 2];
            let under = canvas.at(
                mx.clamp(0.0, w as f32 - 1.0) as usize,
                my.clamp(0.0, h as f32 - 1.0) as usize,
            );
            for c in 0..3 {
                color[c] = color[c] * (1.0 - wet) + under[c] * wet;
            }
        }
        draw_stroke(canvas, st, color, brushes);
        if let Some(cb) = on_stroke.as_deref_mut() {
            cb(i, canvas);
        }
    }
}

fn draw_stroke(canvas: &mut Rgb32, st: &Stroke, color: [f32; 3], brushes: &mut Brushes) {
    let (w, h) = (canvas.w as isize, canvas.h as isize);
    let spacing = (st.radius * 0.25).max(1.0);

    // 折れ線に沿って等間隔にスタンプ位置をリサンプリング
    let mut stamp_pts: Vec<(f32, f32)> = vec![st.points[0]];
    let mut acc = 0.0f32;
    for k in 0..st.points.len().saturating_sub(1) {
        let (mut x0, mut y0) = st.points[k];
        let (x1, y1) = st.points[k + 1];
        let mut seg = (x1 - x0).hypot(y1 - y0);
        while acc + seg >= spacing {
            let t = (spacing - acc) / seg;
            x0 += (x1 - x0) * t;
            y0 += (y1 - y0) * t;
            seg = (x1 - x0).hypot(y1 - y0);
            stamp_pts.push((x0, y0));
            acc = 0.0;
        }
        acc += seg;
    }

    let n = stamp_pts.len();
    for (j, &(x, y)) in stamp_pts.iter().enumerate() {
        // スタンプの角度は局所的な走向から
        let ang = if n > 1 {
            let k = j.min(n - 2);
            (stamp_pts[k + 1].1 - stamp_pts[k].1).atan2(stamp_pts[k + 1].0 - stamp_pts[k].0)
        } else {
            st.angle0
        };
        // 入り抜き: ストローク両端の不透明度を落とし、中程を最も濃くする。ハードブラシは弱め
        let t = if n > 1 { j as f32 / (n - 1) as f32 } else { 0.5 };
        let floor = match st.tag {
            Tag::Hard => 0.85,
            Tag::Standard => 0.65,
            Tag::Soft => 0.50,
        };
        // f32 では sin(π) が僅かに負になり powf が NaN を返すため 0 でクランプする
        let envelope = floor + (1.0 - floor) * (std::f32::consts::PI * t).sin().max(0.0).powf(0.7);
        let stamp = brushes.get_stamp(&st.style, st.radius, -ang);
        let gain = st.opacity * envelope;
        let d = stamp.w as isize;
        let x0 = (x.round() as isize) - d / 2;
        let y0 = (y.round() as isize) - d / 2;
        for sy in 0..d {
            let cy = y0 + sy;
            if cy < 0 || cy >= h {
                continue;
            }
            for sx in 0..d {
                let cx = x0 + sx;
                if cx < 0 || cx >= w {
                    continue;
                }
                let a = stamp.at(sx as usize, sy as usize) * gain;
                if a <= 0.0 {
                    continue;
                }
                let idx = cy as usize * canvas.w + cx as usize;
                let px = &mut canvas.data[idx];
                for c in 0..3 {
                    px[c] = px[c] * (1.0 - a) + color[c] * a;
                }
            }
        }
    }
}

/// ストロークのデバッグビュー: 各ストロークを折れ線として描き、
/// 色 = ブラシ段階（赤 = ハード / 輪郭、緑 = スタンダード、青 = ソフト / 大きな面）
pub fn render_debug(shape_hw: (usize, usize), strokes: &[Stroke]) -> image::RgbImage {
    const SCALE: usize = 2;
    let (h, w) = shape_hw;
    let mut img = image::RgbImage::from_pixel((w * SCALE) as u32, (h * SCALE) as u32, image::Rgb([30, 30, 30]));
    let mut order: Vec<usize> = (0..strokes.len()).collect();
    order.sort_by(|&a, &b| strokes[a].lum.partial_cmp(&strokes[b].lum).unwrap());
    for &si in &order {
        let st = &strokes[si];
        let col = match st.tag {
            Tag::Hard => [235u8, 60, 50],
            Tag::Standard => [70, 215, 90],
            Tag::Soft => [65, 90, 240],
        };
        let thick = ((st.radius * 0.5 * SCALE as f32) as i32).max(1);
        for k in 0..st.points.len().saturating_sub(1) {
            let (x0, y0) = st.points[k];
            let (x1, y1) = st.points[k + 1];
            draw_thick_line(
                &mut img,
                x0 * SCALE as f32,
                y0 * SCALE as f32,
                x1 * SCALE as f32,
                y1 * SCALE as f32,
                thick as f32 / 2.0,
                col,
            );
        }
        if st.points.len() == 1 {
            let (x, y) = st.points[0];
            draw_thick_line(
                &mut img,
                x * SCALE as f32,
                y * SCALE as f32,
                x * SCALE as f32,
                y * SCALE as f32,
                thick as f32 / 2.0,
                col,
            );
        }
    }
    img
}

/// 単純な太線描画（線分に沿って円を敷き詰める）
fn draw_thick_line(img: &mut image::RgbImage, x0: f32, y0: f32, x1: f32, y1: f32, r: f32, col: [u8; 3]) {
    let (w, h) = (img.width() as i32, img.height() as i32);
    let len = (x1 - x0).hypot(y1 - y0);
    let steps = (len.ceil() as usize).max(1);
    let r = r.max(0.5);
    for s in 0..=steps {
        let t = s as f32 / steps as f32;
        let cx = x0 + (x1 - x0) * t;
        let cy = y0 + (y1 - y0) * t;
        let ix0 = ((cx - r).floor() as i32).max(0);
        let ix1 = ((cx + r).ceil() as i32).min(w - 1);
        let iy0 = ((cy - r).floor() as i32).max(0);
        let iy1 = ((cy + r).ceil() as i32).min(h - 1);
        for y in iy0..=iy1 {
            for x in ix0..=ix1 {
                let dx = x as f32 - cx;
                let dy = y as f32 - cy;
                if dx * dx + dy * dy <= r * r {
                    img.put_pixel(x as u32, y as u32, image::Rgb(col));
                }
            }
        }
    }
}
