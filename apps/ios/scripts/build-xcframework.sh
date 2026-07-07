#!/usr/bin/env bash
# Rust コアを iOS 向けにビルドし、Swift バインディングと XCFramework を生成する。
# macOS + Xcode + rustup が必要。Apple Silicon を想定
# （Intel Mac のシミュレータは x86_64-apple-ios を追加して lipo すること）。
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
IOS_DIR="$ROOT/apps/ios"
cd "$ROOT"

echo "==> iOS ターゲットの追加"
for t in aarch64-apple-ios aarch64-apple-ios-sim; do
  rustup target add "$t" >/dev/null
done

echo "==> Rust 静的ライブラリのビルド（実機 / シミュレータ）"
cargo build -p painterly-ffi --release --target aarch64-apple-ios
cargo build -p painterly-ffi --release --target aarch64-apple-ios-sim

echo "==> Swift バインディングの生成"
cargo build -p painterly-ffi --release   # ホスト用 dylib（メタデータ読み出しに使う）
BINDINGS="$IOS_DIR/Generated"
rm -rf "$BINDINGS" && mkdir -p "$BINDINGS"
cargo run -p painterly-ffi --release --features cli --bin uniffi-bindgen -- \
  generate --library "$ROOT/target/release/libpainterly_ffi.dylib" \
  --language swift --out-dir "$BINDINGS"

# ヘッダと modulemap は XCFramework 側に移す（modulemap は module.modulemap 固定名）
HEADERS="$ROOT/target/ios-headers"
rm -rf "$HEADERS" && mkdir -p "$HEADERS"
mv "$BINDINGS/painterly_ffiFFI.h" "$HEADERS/"
mv "$BINDINGS/painterly_ffiFFI.modulemap" "$HEADERS/module.modulemap"

echo "==> XCFramework の作成"
OUT="$IOS_DIR/Frameworks/PainterlyFFI.xcframework"
rm -rf "$OUT" && mkdir -p "$IOS_DIR/Frameworks"
xcodebuild -create-xcframework \
  -library "$ROOT/target/aarch64-apple-ios/release/libpainterly_ffi.a" -headers "$HEADERS" \
  -library "$ROOT/target/aarch64-apple-ios-sim/release/libpainterly_ffi.a" -headers "$HEADERS" \
  -output "$OUT"

echo "==> 完了: $OUT"
echo "次: cd apps/ios && xcodegen generate && open ImgWatercolor.xcodeproj"
