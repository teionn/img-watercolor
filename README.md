# img-watercolor — フローマップに沿ったストローク描画による絵画調レンダリング

> 中文: [README.zh.md](README.zh.md)

写真から**フローマップ**を生成し、その流れに沿ってブラシを走らせる。
**暗い色から明るい色へ**という油彩の塗り重ね順で、**描き込み密度に応じて**
ハード / スタンダード / ソフトの 3 系統のブラシを使い分け、
**低解像度キャンバス**に描いてから滑らかに拡大する——
油彩のラフスケッチ風の絵に仕上げるツール。

アルゴリズムの詳細は **[doc/algorithm.md](doc/algorithm.md)**
（中文版: [doc/algorithm.zh.md](doc/algorithm.zh.md)）を参照。

## クイックスタート

```bash
pip install -r requirements.txt
python scripts/fetch_samples.py        # input/ にサンプル画像を生成（手持ちの写真でも可）

python gui.py                          # GUI（ダークテーマ・英語表記）
python gui.py input/cat.png            # 起動時に画像を読み込む

python run.py input/cat.png            # コマンドライン
python run.py input/*.png --resolution 200 --pixels 80   # よりラフで抽象的な仕上がり
python run.py input/cat.png --process-gif                # 一筆ごとの描画過程アニメーション
```

GUI は画像のドラッグ＆ドロップ、全パラメータのスライダー操作、
バックグラウンドスレッドでのレンダリング（中間画像が出来しだい表示、
描画の進行をリアルタイムプレビュー）、**Show Process** による一筆ごとの
リプレイ、**Save Image** での保存に対応。

結果は `output/<画像名>/` に出力される：

| ファイル | 内容 |
|---|---|
| `1_original.png` | 元画像 |
| `2_quantized.png` | 減色画像（ストロークの色の取得元） |
| `3_posterize_edges.png` | ポスタライズ境界線 |
| `4_gray_blur.png` | グレースケール + ガウスぼかし |
| `5_normal_map.png` | 法線マップ（青紫） |
| `6_flow_map.png` | フローマップ（オリーブ色、ストロークの方向場） |
| `7_strokes_debug.png` | ストロークのデバッグビュー（赤=ハード / 緑=スタンダード / 青=ソフト） |
| `8_painting.png` | 完成画 |
| `density.png` | 密度マップ（ブラシ割り当ての根拠） |
| `palette_swatch.png` / `color_wheel.png` | パレットスウォッチ / カラーホイール散布図 |
| `overview.png` | 全ステージの一覧画像 |
| `process.gif` | 一筆ごとの描画リプレイ（`--process-gif`） |

## コマンドラインオプション

| オプション | 既定値 | 説明 |
|---|---|---|
| `--pixels` | 96 | 色取得・単純化の解像度（長辺 px）。小さいほど大づかみに |
| `--resolution` | 360 | **描画キャンバス**の長辺 px。小さいほど抽象的に |
| `--palette` | 36 | 減色数 |
| `--color-wheel` | rgb | 量子化の色空間（rgb / lab） |
| `--posterize-blur` | 2 | 減色前のガウシアン σ |
| `--normal-blur` | 8 | 勾配計算前のガウシアン σ |
| `--brush-size` | 15 | 基準ブラシ半径（キャンバス px） |
| `--hard-brush` | triangle | 輪郭・高密度領域用（triangle/charcoal/…） |
| `--standard-brush` | flat | 中密度領域用 |
| `--soft-brush` | soft | 広い面・低コントラスト領域用（soft/oil/pastel） |
| `--strokes` | 1.0 | ストローク密度の倍率 |
| `--wet` | 0.18 | ウェットブレンディング：筆を置く前にキャンバスの色と混ぜる比率 |
| `--out-long` | 1080 | 出力画像の長辺（キャンバスからバイキュービック補間で拡大） |

**質感を決める 3 つの要素**（詳細は [doc/algorithm.md](doc/algorithm.md) 第三節）：
低解像度キャンバスからの拡大（大きな画像に直接描かない）、
ソフトエッジのブラシテクスチャ（二値マスクを使わない）、
ウェットブレンディング + 入り抜き。より抽象的にしたければ
`--resolution` を 200 まで下げる。

**手持ちの Photoshop ブラシを使う**：ブラシ先端をグレースケール PNG
（白 = 着色部）として書き出し、パスを渡すだけ：
`python run.py input/cat.png --hard-brush my_brush.png`

## Rust 版（デスクトップ & iOS 展開の本流）

アルゴリズム本体は pure Rust の **`crates/painterly-core`** に移植済み
（OpenCV / numpy 非依存 → デスクトップにも iOS にもそのまま組み込める）。
展開計画の詳細は [doc/roadmap.md](doc/roadmap.md) を参照。

```bash
cargo run --release -p painterly-cli -- input/cat.png   # CLI（run.py と同じオプション体系）

cd apps/desktop/src-tauri && cargo tauri dev            # デスクトップアプリ（Tauri 2）
```

Rust 版の追加機能 — **デプスマップによるタッチの粗密制御**：
奥行きに応じて手前ほど細かいタッチ、奥ほど大きく粗いタッチで描く
（詳細は [doc/depth.md](doc/depth.md)）。

```bash
scripts/fetch_models.sh        # Depth Anything V2 small (ONNX, ~99MB) を取得
cargo run --release -p painterly-cli -- input/cat.png --depth-detail 0.7
```

- `--depth-detail 0..1` — 強さ（0 で無効、既定 0.5）
- モデルがあれば **Depth Anything V2**（NeurIPS 2024）で深度推定、
  無ければ組み込みのヒューリスティック（合焦度 + 大気遠近 + 上下事前分布）
- `--focus-x/--focus-y`（GUI ではクリック / タップ）— フォーカス位置。
  `--focus-range` で被写界深度、`--detail-min/--detail-max` で粗さ・細かさの範囲
- `--line-strength` / `--line-width` — 輪郭線（主線）を重ねる。ボケ領域では自動的に薄くなる
- `--depth my_depth.png` — 外部デプス（白 = 手前）を直接指定
- `--depth-invert` — 手前/奥の反転。中間画像 `depth_map.png` / `line_art.png` で確認できる

## コード構成

```
crates/
  painterly-core/    # コアアルゴリズム（pure Rust、下記 painterly/ の移植）
  painterly-cli/     # コマンドライン（run.py 相当）
  painterly-ffi/     # UniFFI バインディング（iOS / Swift 向け）
apps/
  desktop/           # デスクトップアプリ（Tauri 2、Windows/macOS/Linux）
  ios/               # iOS アプリ（SwiftUI、ビルド手順は apps/ios/README.md）
painterly/           # Python 参照実装
  palette.py   # k-means による減色、ポスタライズ境界線、パレット可視化
  flowmap.py   # ガウスぼかし → Sobel → 法線マップ / 構造テンソルによるフローマップ
  density.py   # 密度マップ（勾配 + 境界線）→ ブラシサイズ・3 系統の割り当て
  brushes.py   # プロシージャルなブラシテクスチャ + カスタム PNG の読み込み
  strokes.py   # フローに沿ったトレース、暗→明ソート、ウェットブレンディング、入り抜き、デバッグビュー
  pipeline.py  # パイプライン統括 + 中間画像の書き出し + GUI 用フック
run.py         # コマンドラインエントリポイント（Python）
gui.py         # PySide6 GUI（Python、Tauri 版に置き換え予定）
```

## パラメータの目安

- ラフで抽象的に：`--resolution 200 --pixels 80 --palette 36 --brush-size 15`
- ディテールを残す：`--resolution 480 --pixels 160 --palette 64 --strokes 1.3`
- 乾いたタッチ（混色を抑える）：`--wet 0.05`
- 木炭・パステル調：`--hard-brush charcoal --soft-brush pastel`
