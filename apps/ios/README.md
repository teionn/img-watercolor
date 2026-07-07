# img-watercolor iOS アプリ（SwiftUI + Rust コア）

`painterly-core` を UniFFI 経由で呼び出す SwiftUI アプリ。
写真を選ぶ → パラメータ調整 → レンダリング → 中間ステージ閲覧・過程リプレイ → 写真に保存。

## 必要なもの

- macOS + **Xcode 16 以降**（シミュレータ実行はこれだけで可）
- [rustup](https://rustup.rs)（iOS ターゲットはビルドスクリプトが自動追加）
- [XcodeGen](https://github.com/yonaskolb/XcodeGen): `brew install xcodegen`
- 実機で動かす場合: Apple ID（無料アカウントで 7 日署名）
  / 配布する場合: Apple Developer Program

## ビルド手順

```bash
# 1. Rust 静的ライブラリ + Swift バインディング + XCFramework を生成
./scripts/build-xcframework.sh

# 2. Xcode プロジェクトを生成して開く
xcodegen generate
open ImgWatercolor.xcodeproj
```

Xcode で Signing の Team を設定し、シミュレータまたは実機で Run。
Rust 側（`crates/painterly-core` / `crates/painterly-ffi`）を変更したときは
手順 1 を再実行するだけでよい（プロジェクト再生成は不要）。

## 構成

```
apps/ios/
  project.yml        # XcodeGen 定義（.xcodeproj は生成物なのでコミットしない）
  scripts/
    build-xcframework.sh
  Sources/
    ImgWatercolorApp.swift   # エントリポイント
    ContentView.swift        # プレビュー / ステージ一覧 / 操作バー
    ParamsSheet.swift        # パラメータ編集シート
    RenderViewModel.swift    # レンダリング状態 + Rust コールバックの橋渡し
  Generated/         # UniFFI 生成の Swift バインディング（スクリプトが生成）
  Frameworks/        # PainterlyFFI.xcframework（スクリプトが生成）
```

## 実装メモ

- `render()` はブロッキング呼び出しなので `Task.detached` から呼ぶ。
  進捗（`onStage` / `onProgress`）は Rust のレンダリングスレッドから届くため、
  `ObserverBridge` で MainActor へ橋渡ししている。
- 画像の受け渡しは PNG/JPEG のバイト列（`UIImage.pngData()` ↔ `UIImage(data:)`）。
- 既定パラメータ・ブラシ一覧は Rust 側から取得（`defaultParams()` / `builtinBrushes()`）
  なので、コアの既定値変更が自動で反映される。
- Intel Mac のシミュレータで動かす場合は `x86_64-apple-ios` をビルドして
  `lipo -create` で sim ライブラリを作ること（スクリプトは Apple Silicon 前提）。
