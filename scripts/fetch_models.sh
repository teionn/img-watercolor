#!/usr/bin/env bash
# 深度推定モデルのダウンロード。
# Depth Anything V2 small（Apache-2.0）の ONNX 版を Hugging Face から取得する。
# 取得後、CLI は models/depth_anything_v2_small.onnx を自動検出して使う。
set -euo pipefail
cd "$(dirname "$0")/.."

mkdir -p models
URL="https://huggingface.co/onnx-community/depth-anything-v2-small/resolve/main/onnx/model.onnx"
OUT="models/depth_anything_v2_small.onnx"

if [ -f "$OUT" ]; then
  echo "既に存在します: $OUT"
  exit 0
fi
echo "ダウンロード中: $URL"
curl -L --fail -o "$OUT" "$URL"

# tract で実行できるようにパッチ（cubic Resize / シンボリック次元の除去）
python3 -c "import onnx" 2>/dev/null || python3 -m pip install --quiet onnx
python3 "$(dirname "$0")/patch_onnx_for_tract.py" "$OUT"

echo "保存しました: $OUT ($(du -h "$OUT" | cut -f1))"
