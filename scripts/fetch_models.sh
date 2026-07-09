#!/usr/bin/env bash
# ONNX モデルのダウンロード。
#   - 深度推定: Depth Anything V2 small（Apache-2.0）
#   - 顔検出:   UltraFace RFB-320（MIT。Ultra-Light-Fast-Generic-Face-Detector-1MB）
# 取得後、CLI / デスクトップは models/ 配下の既定ファイル名を自動検出して使う。
set -euo pipefail
cd "$(dirname "$0")/.."

mkdir -p models

# onnx を tract で実行できるようにパッチ（cubic Resize / シンボリック次元の除去）
patch_onnx() {
  python3 -c "import onnx" 2>/dev/null || python3 -m pip install --quiet onnx
  python3 "$(dirname "$0")/patch_onnx_for_tract.py" "$1"
}

fetch() {
  local url="$1" out="$2"
  if [ -f "$out" ]; then
    echo "既に存在します: $out"
    return 0
  fi
  echo "ダウンロード中: $url"
  curl -L --fail -o "$out" "$url"
  patch_onnx "$out"
  echo "保存しました: $out ($(du -h "$out" | cut -f1))"
}

# 深度推定モデル
fetch \
  "https://huggingface.co/onnx-community/depth-anything-v2-small/resolve/main/onnx/model.onnx" \
  "models/depth_anything_v2_small.onnx"

# 顔検出モデル（UltraFace RFB-320）。URL が変わった場合は onnx/models の
# validated/vision/body_analysis/ultraface を参照して差し替えてください
fetch \
  "https://github.com/onnx/models/raw/main/validated/vision/body_analysis/ultraface/models/version-RFB-320.onnx" \
  "models/ultraface_rfb_320.onnx"
