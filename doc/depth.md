# デプスマップ生成の仕組み

タッチの粗密制御（`--depth-detail`）に使う深度マップは、精度の異なる
3 つのソースから取得できる。優先順位の高い順に:

| ソース | 精度 | 速度 | 依存 |
|---|---|---|---|
| 1. 外部デプス `--depth <png>` | 任意（渡すものに依存） | — | なし |
| 2. ニューラル推定 `--depth-model` / `models/` 自動検出 | 高 | 数秒（CPU） | ONNX モデル（~99MB） |
| 3. 組み込みヒューリスティック（フォールバック） | 低〜中 | 数十 ms | なし |

いずれも「白 = 手前」の規約で `painterly-core` に渡される。

## フォーカスモデル（被写界深度）

単純な「手前 = 細かい / 奥 = 粗い」ではなく、カメラのピントと同じモデルで制御する:

- **焦点** — プレビューのクリック / タップ位置の深度（`focus_point`）、
  または深度値の直接指定（`--focus-depth 0..1`、既定 0 = 最前面）
- **フォーカス範囲**（`--focus-range`）— 焦点から細かさが保たれる深度幅。
  小さいほど「被写界深度が浅い」= 焦点面だけが細密になる
- **粗さ・細かさの範囲** — 範囲外（ボケ側）の粗さ下限 `--detail-min`、
  焦点近傍の細かさ上限 `--detail-max`（1 超で通常より細かく）
- **強さ**（`--depth-detail 0..1`）— 効果全体のマスタースライダー（0 = 無効）

実装は焦点重み w（焦点 = 1、範囲外 = 0 の smoothstep 減衰）で密度マップを
`密度 × lerp(detail_min, detail_max, w)` に変調する。密度はブラシサイズ・
ハード/ソフト割り当て・ディテール層の対象領域をすべて駆動しているので、
これ一点で焦点面だけが細密なタッチになる。

## 輪郭線（鉛筆下書き）

`--line-strength 0..1`（既定 0.4、0 で無効）/ `--line-width`（太さ px）。
絵のタッチとは独立した「鉛筆で描いた下書き」レイヤーを重ねる。

- **元画像から抽出する**（減色後ではなく）。ぼかす前のグレースケールに
  軽い平滑化 → Sobel 勾配強度 → 分位数正規化 → ソフト閾値。
  減色で潰れる顔のパーツ・髪・布の皺などのディテール線が残るのが利点
- **鉛筆の質感**: 細かい紙目ノイズと粗いノイズで線に濃淡と途切れを付ける
  （シード固定で再現可能）。インクはグラファイトの青灰色
- ボケ領域では焦点重みに応じて線も薄くなり、被写界深度の表現と整合する。
  中間画像 `line_art.png` で下書きだけを確認できる

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
