//! ニューラル単眼深度推定。ONNX モデルを tract（pure Rust）で実行する。
//!
//! - 既定モデル: **Depth Anything V2 small**（Yang et al., "Depth Anything V2",
//!   NeurIPS 2024。Apache-2.0）。`scripts/fetch_models.sh` で取得する。
//!   MiDaS v2.1（Ranftl et al., TPAMI 2022）など、ImageNet 正規化 +
//!   逆深度出力の ONNX モデルならそのまま差し替え可能
//! - 推論: tract はネイティブ依存ゼロの Rust 製 ONNX ランタイムなので、
//!   デスクトップにも iOS 静的ライブラリにもそのまま組み込める
//! - 出力: 逆深度（大きい = 手前）を min-max 正規化した GrayImage（白 = 手前）。
//!   `painterly_core::Params::external_depth` にそのまま渡せる規約

use std::path::Path;

use tract_onnx::prelude::*;
use tract_onnx::tract_hir::infer::Factoid;

/// ImageNet 統計（Depth Anything / MiDaS 共通の前処理仕様）
const MEAN: [f32; 3] = [0.485, 0.456, 0.406];
const STD: [f32; 3] = [0.229, 0.224, 0.225];

/// 入力形状が動的なモデルに使う既定解像度。
/// Depth Anything V2 の学習解像度 518（= 14 の倍数、ViT のパッチサイズ）
const DEFAULT_INPUT: usize = 518;

pub struct DepthModel {
    model: TypedRunnableModel<TypedModel>,
    input_h: usize,
    input_w: usize,
}

impl DepthModel {
    /// ONNX モデルを読み込む。入力サイズはモデル宣言から取り、
    /// 動的な場合は `input_size`（未指定なら 518）で固定する
    pub fn load(path: impl AsRef<Path>, input_size: Option<usize>) -> Result<Self, String> {
        let path = path.as_ref();
        let model = tract_onnx::onnx()
            .model_for_path(path)
            .map_err(|e| format!("{}: {e}", path.display()))?;

        let fact = model
            .input_fact(0)
            .map_err(|e| format!("input fact: {e}"))?
            .clone();
        let dims: Vec<Option<usize>> = fact
            .shape
            .dims()
            .map(|d| match d.concretize() {
                Some(TDim::Val(v)) if v > 0 => Some(v as usize),
                _ => None,
            })
            .collect();
        let fallback = input_size.unwrap_or(DEFAULT_INPUT);
        let (input_h, input_w) = match dims.as_slice() {
            [_, _, Some(h), Some(w)] => (*h, *w),
            _ => (fallback, fallback),
        };

        let model = model
            .with_input_fact(0, f32::fact([1, 3, input_h, input_w]).into())
            .and_then(|m| m.into_optimized())
            .and_then(|m| m.into_runnable())
            .map_err(|e| format!("モデルの最適化に失敗: {e}"))?;
        Ok(DepthModel { model, input_h, input_w })
    }

    /// 深度を推定して GrayImage（白 = 手前）で返す。
    /// 入力はモデル解像度へ引き伸ばして推論する。呼び出し側（painterly-core）が
    /// キャンバス解像度へ再リサイズするため原寸へは戻さない
    pub fn estimate(&self, img: &image::RgbImage) -> Result<image::GrayImage, String> {
        let (h, w) = (self.input_h, self.input_w);
        let resized = image::imageops::resize(
            img,
            w as u32,
            h as u32,
            image::imageops::FilterType::Triangle,
        );

        let input = tract_ndarray::Array4::from_shape_fn((1, 3, h, w), |(_, c, y, x)| {
            let v = resized.get_pixel(x as u32, y as u32).0[c] as f32 / 255.0;
            (v - MEAN[c]) / STD[c]
        });

        let outputs = self
            .model
            .run(tvec!(Tensor::from(input).into()))
            .map_err(|e| format!("推論に失敗: {e}"))?;
        let out = outputs[0]
            .to_array_view::<f32>()
            .map_err(|e| format!("出力の読み出しに失敗: {e}"))?;
        // 出力は [1,H,W] または [1,1,H,W] の逆深度（大きい = 手前）
        let flat: Vec<f32> = out.iter().cloned().collect();
        if flat.len() != h * w {
            return Err(format!("予期しない出力サイズ: {} (期待 {})", flat.len(), h * w));
        }

        let mn = flat.iter().cloned().fold(f32::MAX, f32::min);
        let mx = flat.iter().cloned().fold(f32::MIN, f32::max);
        let range = (mx - mn).max(1e-8);
        let mut depth = image::GrayImage::new(w as u32, h as u32);
        for (i, p) in depth.pixels_mut().enumerate() {
            // 逆深度をそのまま正規化 → 白 = 手前（painterly-core の規約）
            p.0 = [(((flat[i] - mn) / range) * 255.0) as u8];
        }
        Ok(depth)
    }
}

/// 1 回きりの推定用ショートカット
pub fn estimate_depth(
    model_path: impl AsRef<Path>,
    img: &image::RgbImage,
) -> Result<image::GrayImage, String> {
    DepthModel::load(model_path, None)?.estimate(img)
}
