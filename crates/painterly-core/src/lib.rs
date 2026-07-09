//! painterly-core — フローマップに沿ったストローク描画による絵画調レンダリング。
//!
//! Python 版 `painterly/` パッケージの pure Rust 移植。OpenCV / numpy に依存しないため、
//! デスクトップ（Tauri）・iOS（静的ライブラリ + Swift バインディング）のどちらにも
//! そのまま組み込める。
//!
//! モジュール対応:
//!   palette.rs  ← painterly/palette.py （k-means 減色、ポスタライズ境界、パレット可視化）
//!   flowmap.rs  ← painterly/flowmap.py （Sobel → 法線マップ / 構造テンソル方向場）
//!   density.rs  ← painterly/density.py （密度マップ → ブラシサイズ・3 段階割り当て）
//!   brushes.rs  ← painterly/brushes.py （プロシージャルブラシ + カスタム PNG）
//!   strokes.rs  ← painterly/strokes.py （フロー追跡、暗→明ソート、ウェットブレンディング）
//!   pipeline.rs ← painterly/pipeline.py（統括 + 中間画像 + コールバック）

pub mod brushes;
pub mod buf;
pub mod color;
pub mod density;
pub mod depth;
pub mod flowmap;
pub mod palette;
pub mod paper;
pub mod pipeline;
pub mod presets;
pub mod rng;
pub mod strokes;

pub use brushes::{Brushes, BUILTIN_BRUSHES};
pub use pipeline::{run_pipeline, Callbacks, Params, PipelineResult};
pub use presets::{preset, presets, Preset};
