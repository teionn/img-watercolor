# デスクトップ & iOS 展開ロードマップ

方針（2026-07 決定）: **Rust コア + ネイティブ UI**。
アルゴリズム本体を pure Rust の `painterly-core` に一本化し、
デスクトップ（Windows 優先）は Tauri、iOS は SwiftUI + UniFFI バインディングで包む。

```
painterly-core (pure Rust / 依存は image・rand のみ)
 ├─ crates/painterly-cli      … CLI（検証・バッチ処理）        [済]
 ├─ apps/desktop (Tauri 2)    … Windows / macOS / Linux        [済・スキャフォールド]
 └─ apps/ios (SwiftUI)        … iPhone / iPad                  [未着手]
```

## フェーズ 1: Rust コア移植 — 済

- `painterly/*.py`（約 740 行の numpy/OpenCV）を `crates/painterly-core` に移植。
  OpenCV 依存を排し、ガウシアン・Sobel・バイラテラル・k-means++・リサイズを自前実装。
- Python 版との対応はモジュール単位（palette / flowmap / density / brushes / strokes / pipeline）。
- 乱数は ChaCha8 のシード固定でプラットフォーム間再現性を確保
  （numpy の乱数列とは一致しないため、Python 版と画素単位では一致しない。作風は同一）。
- 検証: `cargo run --release -p painterly-cli -- input/landscape.png`
  → 全 12 中間画像 + 完成画を出力、512px 入力で約 0.3 秒（Python 版の数十倍高速）。

## フェーズ 2: デスクトップ（Tauri）— スキャフォールド済

- `apps/desktop/`: 静的 HTML/JS フロントエンド + Rust バックエンド。Node.js 不要。
- gui.py の機能をすべて移植済み（D&D、スライダー、逐次ステージ表示、
  リアルタイム描画プレビュー、過程リプレイ、保存）。
- 残タスク:
  - [ ] Windows 実機での `cargo tauri build`（NSIS/MSI）と動作確認
  - [ ] ニューラル深度（painterly-depth）のデスクトップ組み込み
        （モデルパス設定 UI + 同梱するかダウンロードさせるかの判断）
  - [ ] カスタムブラシ PNG の読み込み UI
  - [ ] process.gif 書き出しの UI 露出
  - [ ] レンダリングのキャンセル操作（コアにキャンセルフラグを追加）
  - [ ] CI（GitHub Actions で Windows/macOS/Linux のバンドル生成）

## フェーズ 3: iOS（SwiftUI + UniFFI）— スキャフォールド済

- `crates/painterly-ffi`: UniFFI（proc-macro 方式）の Swift 向け API。
  `render(imageBytes, params, observer)` のブロッキング呼び出し +
  `RenderObserver` コールバック（onStage / onProgress）。
  Linux 上でコンパイルとバインディング生成を確認済み。
- `apps/ios/`: SwiftUI アプリ（PhotosPicker、パラメータシート、
  ステージサムネイル、描画過程のライブ表示とリプレイ、写真ライブラリ保存）。
  Xcode プロジェクトは XcodeGen（project.yml）で生成、
  Rust 側は scripts/build-xcframework.sh で XCFramework 化。
- 残タスク:
  - [ ] macOS + Xcode 実機での初回ビルドと動作確認（この環境では Apple SDK が
        使えないため Swift のコンパイルは未検証）
  - [ ] Bundle ID / Team ID の正式決定（現状 com.imgwatercolor.ios）
  - [ ] アプリアイコン・スクリーンショットなどストア資材
  - [ ] レンダリングのキャンセル、iPad レイアウト最適化
  - [ ] パフォーマンス計測: キャンバス 360px なら A 系チップで 1 秒未満の見込み
        （必要なら rayon を feature 追加）

留意点:

- コアの依存は `image` / `rand` / `rand_chacha` のみに保つ（iOS ビルドを壊さない）。
- UI 文字列は日本語をメインに、英語をフォールバックに。
- Python 版 `painterly/` は参照実装として当面残す（アルゴリズム変更は
  Rust → Python の順で両方に反映するか、Python 側を凍結する）。

## パラメータ互換性

CLI / デスクトップ / iOS で `Params`（`pipeline.rs`）を共有する。
既定値は Python 版 `PARAMS_DEFAULT` と同一。
