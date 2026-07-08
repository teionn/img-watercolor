# デプスマップ生成の仕組み

タッチの粗密制御（`--depth-detail`）に使う深度マップは、精度の異なる
3 つのソースから取得できる。優先順位の高い順に:

| ソース | 精度 | 速度 | 依存 |
|---|---|---|---|
| 1. 外部デプス `--depth <png>` | 任意（渡すものに依存） | — | なし |
| 2. ニューラル推定 `--depth-model` / `models/` 自動検出 | 高 | 数秒（CPU） | ONNX モデル（~99MB） |
| 3. 組み込みヒューリスティック（フォールバック） | 低〜中 | 数十 ms | なし |

いずれも「白 = 手前」の規約で `painterly-core` に渡され、
密度マップを `密度 × (1 − depth_detail × 深度)` で変調する。
密度はブラシサイズ・ハード/ソフト割り当て・ディテール層の対象領域を
すべて駆動しているので、これ一点で「手前 = 細かいタッチ、奥 = 粗いタッチ」になる。

## 2. ニューラル推定（crates/painterly-depth）

- **モデル**: [Depth Anything V2](https://depth-anything-v2.github.io/) small
  （Yang et al., *Depth Anything V2*, NeurIPS 2024）。
  ONNX 版（[onnx-community/depth-anything-v2-small](https://huggingface.co/onnx-community/depth-anything-v2-small)、
  Apache-2.0）を `scripts/fetch_models.sh` で取得する。
- **ランタイム**: [tract](https://github.com/sonos/tract)（Sonos 製、pure Rust の
  ONNX 推論エンジン）。ネイティブ依存がないため、デスクトップにも
  iOS の静的ライブラリにもそのまま組み込める。
- **パッチ**: 配布されている ONNX には tract が扱えない要素（位置埋め込み補間の
  cubic Resize、`floor(height/14)` などのシンボリック次元）があるため、
  `scripts/patch_onnx_for_tract.py` で linear 化 + 入力 518 固定に変換して使う
  （fetch_models.sh が自動で実行する）。518 固定なら cubic → linear は等倍補間で無損失。
- **前処理**: 518×518 に引き伸ばし → ImageNet 統計（mean/std）で正規化。
- **後処理**: 出力は逆深度（大きい = 手前）。min-max 正規化して
  白 = 手前のグレースケールにする。相対深度なので絶対距離は持たないが、
  タッチ粗密の制御には相対値で十分。
- **互換**: ImageNet 正規化 + 逆深度出力の ONNX モデルならそのまま差し替え可
  （MiDaS v2.1 [Ranftl et al., TPAMI 2022] など。入力解像度はモデル宣言から自動取得）。

## 3. 組み込みヒューリスティック（crates/painterly-core/src/depth.rs）

学習モデルなし・依存なしのフォールバック。3 つの絵画的手掛かりを加重合成する:

1. **合焦度**（重み 0.40）— ピントが合っている（局所勾配が強い）領域は
   被写体 = 手前。単一画像の Depth from Defocus（Zhuo & Sim,
   *Defocus map estimation from a single image*, Pattern Recognition 2011 など）
   の考え方の簡易版。
2. **大気遠近**（重み 0.25）— 明るく彩度の低い（霞んだ）領域は遠景。
   絵画の空気遠近法と同じ手掛かり。
3. **上下の事前分布**（重み 0.35）— 写真では画面上部ほど遠いことが多い。

風景写真では概ね機能するが、室内・接写・空が写らない構図では
破綻しやすい。その場合は `--depth-invert` で反転するか、
ニューラル推定・外部デプスを使う。

## iOS での扱い

- tract は pure Rust なので `painterly-depth` ごと iOS ビルドに含められる。
  ただしモデル同梱で +99MB、推論は A 系チップの CPU で数秒。
- 代替案（roadmap）: Apple が配布する Depth Anything V2 の Core ML 版を
  Swift 側で実行して `depth_image`（FFI の外部デプス入力）に渡す。
  Neural Engine で高速化でき、Rust 側の変更は不要。
  LiDAR 搭載機なら ARKit の実測深度も同じ経路で使える。
