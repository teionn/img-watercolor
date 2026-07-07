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
  - [ ] カスタムブラシ PNG の読み込み UI
  - [ ] process.gif 書き出しの UI 露出
  - [ ] レンダリングのキャンセル操作（コアにキャンセルフラグを追加）
  - [ ] CI（GitHub Actions で Windows/macOS/Linux のバンドル生成）

## フェーズ 3: iOS（SwiftUI + UniFFI）— 未着手

計画:

1. `crates/painterly-ffi` を追加し [UniFFI](https://mozilla.github.io/uniffi-rs/) で
   Swift バインディングを生成（API は `render(imageData, params) -> stages/progress/result`
   のコールバック形式。desktop の lib.rs と同じ粒度）。
2. `cargo build --target aarch64-apple-ios` で静的ライブラリ化し、
   XCFramework（device + simulator）にまとめる。
3. `apps/ios/` に SwiftUI アプリ:
   - PhotosPicker で写真読み込み → パラメータシート（スライダー）→ レンダリング
   - 中間ステージのサムネイル、描画過程のアニメーション再生
   - 写真ライブラリへの保存・共有シート
4. パフォーマンス: キャンバス解像度 360px なら A 系チップで 1 秒未満の見込み
   （コアはシングルスレッドでも Python 比 ~30 倍。必要なら rayon を feature 追加）。

留意点:

- コアの依存は `image` / `rand` / `rand_chacha` のみに保つ（iOS ビルドを壊さない）。
- UI 文字列は日本語をメインに、英語をフォールバックに。
- Python 版 `painterly/` は参照実装として当面残す（アルゴリズム変更は
  Rust → Python の順で両方に反映するか、Python 側を凍結する）。

## パラメータ互換性

CLI / デスクトップ / iOS で `Params`（`pipeline.rs`）を共有する。
既定値は Python 版 `PARAMS_DEFAULT` と同一。
