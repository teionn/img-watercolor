//! 顔検出。**UltraFace**（Ultra-Light-Fast-Generic-Face-Detector-1MB, RFB-320。
//! MIT ライセンス）の ONNX を tract（pure Rust）で実行する。
//!
//! - 入力: 1x3x240x320（RGB、正規化 (v-127)/128）
//! - 出力: scores [1,N,2]（背景/顔）, boxes [1,N,4]（正規化 [x1,y1,x2,y2]）。
//!   モデル内でアンカー復号済みなので、こちら側でのアンカー生成は不要
//! - 後処理: 信頼度しきい値 + IoU ベースの Non-Maximum Suppression
//!
//! 検出結果（正規化ボックス）は `painterly_core::Params::face_regions` に渡す規約。
//! tract はネイティブ依存ゼロなのでデスクトップにも iOS にも組み込める。

use std::path::Path;

use tract_onnx::prelude::*;

/// UltraFace RFB-320 の入力解像度（幅 320 × 高さ 240）
const INPUT_W: usize = 320;
const INPUT_H: usize = 240;
/// 前処理の正規化定数（UltraFace 仕様）
const MEAN: f32 = 127.0;
const SCALE: f32 = 128.0;

pub struct FaceModel {
    model: TypedRunnableModel<TypedModel>,
}

impl FaceModel {
    /// ONNX モデルを読み込む。入力は 1x3x240x320 に固定する
    pub fn load(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        let model = tract_onnx::onnx()
            .model_for_path(path)
            .map_err(|e| format!("{}: {e}", path.display()))?
            .with_input_fact(0, f32::fact([1, 3, INPUT_H, INPUT_W]).into())
            .map_err(|e| format!("入力形状の設定に失敗: {e}"))?
            .into_optimized()
            .map_err(|e| format!("モデルの最適化に失敗: {e}"))?
            .into_runnable()
            .map_err(|e| format!("runnable 化に失敗: {e}"))?;
        Ok(FaceModel { model })
    }

    /// 顔を検出し、正規化ボックス [x0,y0,x1,y1]（0..1）を信頼度降順で返す。
    /// `conf_threshold` は 0.5〜0.8 が目安（高いほど誤検出が減る）
    pub fn detect(
        &self,
        img: &image::RgbImage,
        conf_threshold: f32,
    ) -> Result<Vec<[f32; 4]>, String> {
        let resized = image::imageops::resize(
            img,
            INPUT_W as u32,
            INPUT_H as u32,
            image::imageops::FilterType::Triangle,
        );
        let input = tract_ndarray::Array4::from_shape_fn((1, 3, INPUT_H, INPUT_W), |(_, c, y, x)| {
            (resized.get_pixel(x as u32, y as u32).0[c] as f32 - MEAN) / SCALE
        });
        let outputs = self
            .model
            .run(tvec!(Tensor::from(input).into()))
            .map_err(|e| format!("推論に失敗: {e}"))?;

        // 出力順はモデル依存なので、最終次元で scores(2) / boxes(4) を判別する
        let mut scores_v: Option<Vec<f32>> = None;
        let mut boxes_v: Option<Vec<f32>> = None;
        for o in outputs.iter() {
            let arr = o
                .to_array_view::<f32>()
                .map_err(|e| format!("出力の読み出しに失敗: {e}"))?;
            let last = *arr.shape().last().unwrap_or(&0);
            let flat: Vec<f32> = arr.iter().cloned().collect();
            match last {
                2 => scores_v = Some(flat),
                4 => boxes_v = Some(flat),
                _ => {}
            }
        }
        let scores = scores_v.ok_or("scores 出力（最終次元 2）が見つかりません")?;
        let boxes = boxes_v.ok_or("boxes 出力（最終次元 4）が見つかりません")?;
        let n = boxes.len() / 4;
        if scores.len() != n * 2 {
            return Err(format!("scores/boxes の数が不一致: {} vs {}", scores.len(), n));
        }

        let mut cand: Vec<(f32, [f32; 4])> = Vec::new();
        for i in 0..n {
            let conf = scores[i * 2 + 1]; // [背景, 顔]
            if conf >= conf_threshold {
                cand.push((
                    conf,
                    [
                        boxes[i * 4].clamp(0.0, 1.0),
                        boxes[i * 4 + 1].clamp(0.0, 1.0),
                        boxes[i * 4 + 2].clamp(0.0, 1.0),
                        boxes[i * 4 + 3].clamp(0.0, 1.0),
                    ],
                ));
            }
        }
        Ok(nms(cand, 0.3))
    }
}

/// 信頼度降順ソート + IoU ベースの Non-Maximum Suppression
fn nms(mut cand: Vec<(f32, [f32; 4])>, iou_thresh: f32) -> Vec<[f32; 4]> {
    cand.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut keep: Vec<[f32; 4]> = Vec::new();
    for (_, b) in cand {
        if keep.iter().all(|k| iou(k, &b) < iou_thresh) {
            keep.push(b);
        }
    }
    keep
}

/// 2 つの正規化ボックス [x0,y0,x1,y1] の Intersection-over-Union
fn iou(a: &[f32; 4], b: &[f32; 4]) -> f32 {
    let x0 = a[0].max(b[0]);
    let y0 = a[1].max(b[1]);
    let x1 = a[2].min(b[2]);
    let y1 = a[3].min(b[3]);
    let inter = (x1 - x0).max(0.0) * (y1 - y0).max(0.0);
    let area_a = (a[2] - a[0]).max(0.0) * (a[3] - a[1]).max(0.0);
    let area_b = (b[2] - b[0]).max(0.0) * (b[3] - b[1]).max(0.0);
    let union = area_a + area_b - inter;
    if union <= 0.0 {
        0.0
    } else {
        inter / union
    }
}

/// 1 回きりの検出用ショートカット
pub fn detect_faces(
    model_path: impl AsRef<Path>,
    img: &image::RgbImage,
    conf: f32,
) -> Result<Vec<[f32; 4]>, String> {
    FaceModel::load(model_path)?.detect(img, conf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iou_basic() {
        let a = [0.0, 0.0, 0.5, 0.5];
        assert!((iou(&a, &a) - 1.0).abs() < 1e-5);
        let c = [0.6, 0.6, 1.0, 1.0]; // 重なりなし
        assert_eq!(iou(&a, &c), 0.0);
        let d = [0.25, 0.25, 0.75, 0.75]; // 部分的に重なる
        assert!(iou(&a, &d) > 0.0 && iou(&a, &d) < 1.0);
    }

    #[test]
    fn nms_suppresses_overlaps() {
        let a = [0.0, 0.0, 0.5, 0.5];
        let b = [0.02, 0.02, 0.52, 0.52]; // a とほぼ重複
        let c = [0.6, 0.6, 1.0, 1.0]; // 別領域
        let kept = nms(vec![(0.9, a), (0.8, b), (0.7, c)], 0.3);
        assert_eq!(kept.len(), 2); // 重複の b が除外され a, c が残る
        assert_eq!(kept[0], a); // 最高信頼が先頭
    }
}
