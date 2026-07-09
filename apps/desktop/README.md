# img-watercolor デスクトップアプリ（Tauri 2）

`painterly-core`（pure Rust）をバックエンドに、静的な HTML/JS フロントエンドを
Tauri 2 で包んだデスクトップアプリ。Node.js もバンドラも不要。

## 開発ビルド

```bash
# Tauri CLI（初回のみ）
cargo install tauri-cli --version "^2"

cd apps/desktop/src-tauri
cargo tauri dev        # 開発実行（ホットリロードなし・静的 UI）
cargo tauri build      # 配布用バンドル（.msi / .exe / .dmg / .AppImage）
```

## プラットフォーム別の準備

- **Windows**: [Rust](https://rustup.rs) と WebView2（Windows 10/11 は標準搭載）のみ。
  `cargo tauri build` で NSIS `.exe` / MSI インストーラが生成される。
- **macOS**: Xcode Command Line Tools。`.app` / `.dmg` が生成される。
- **Linux**: `libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev`

## 構成

```
apps/desktop/
  ui/                  # フロントエンド（素の HTML/CSS/JS、withGlobalTauri）
    index.html         # レイアウト: 左パラメータパネル + 右プレビュー/ステージ一覧
    main.js            # invoke / イベント購読 / 過程リプレイ
    styles.css         # ダークテーマ
  src-tauri/
    src/lib.rs         # コマンド: load_image / start_render / save_image
                       # イベント: stage / paint_progress / done / render_error
    tauri.conf.json
    capabilities/      # 権限（dialog のみ追加）
    icons/             # gen_icons.py で生成
```

レンダリングはバックグラウンドスレッドで実行され、中間画像（`stage`）と
描画進捗（`paint_progress`）がイベントとしてフロントエンドへ流れる。
Python 版 `gui.py`（PySide6）と同じ機能構成:
ドラッグ＆ドロップ、全パラメータのスライダー、逐次プレビュー、
過程リプレイ（過程を再生）、完成画の保存。

追加機能:

- **プリセット**: セレクトボックスで名前付きスタイル（基本 / 水彩 / 鮮やか /
  淡彩 / 細密 / ラフ / 鉛筆画）を全スライダーへ一括適用
- **フォーカス指定**: プレビューをクリックで焦点（深度）を指定、
  ダブルクリックで解除。クリック未指定時は「フォーカス深度」スライダー
- **ニューラル深度**: チェックで Depth Anything V2（ONNX）を使用。
  models/ を自動検出、無ければファイル選択（load_depth_model コマンド）
- **外部デプス PNG**: 白 = 手前のデプスマップを直接指定（モデルより優先）
- **カスタムブラシ**: ブラシ選択の「PNG を選択…」で Photoshop 書き出しの
  グレースケール先端画像を使用
- **過程 GIF**: 「過程 GIF を生成」チェック後にレンダリングすると
  GIF ボタンから保存できる（一時ディレクトリ経由、save_process_gif）
- **上級パラメータ**: ハード/スタンダードの刷り分け分位数、横取り採色率
- **操作性**: 数値直接入力 / ホイール微調整（Shift で ×10）/
  ダブルクリックで個別リセット / 変更項目のハイライト / 自動プレビュー

## 注意

- このディレクトリはリポジトリルートの Cargo ワークスペースから独立している
  （webkit2gtk 等のシステム依存をコア/CLI のビルドに持ち込まないため）。
- アイコンの再生成: `python3 src-tauri/icons/gen_icons.py`
