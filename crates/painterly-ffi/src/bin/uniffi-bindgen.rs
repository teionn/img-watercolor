//! Swift バインディング生成用 CLI。
//! 使い方: cargo run -p painterly-ffi --features cli --bin uniffi-bindgen -- \
//!             generate --library <libpainterly_ffi.dylib> --language swift --out-dir <dir>
fn main() {
    uniffi::uniffi_bindgen_main()
}
