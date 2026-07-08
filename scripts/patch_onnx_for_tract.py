"""ONNX モデルを tract（pure Rust ランタイム）で読めるようにパッチする。

Depth Anything V2 の ONNX 出力には tract が扱えない要素が 2 つある:
  1. 位置埋め込み補間の Resize (mode=cubic) — tract は cubic 未対応。
     入力を 518 固定にすればこの補間は等倍なので linear に変えても無損失
  2. シンボリック次元 `floor(height/14)` 等を含む value_info —
     tract の TDim パーサが解釈できない。入力形状を固定して除去する

使い方: python3 scripts/patch_onnx_for_tract.py models/depth_anything_v2_small.onnx [入力サイズ=518]
"""
import sys

import onnx


def main():
    path = sys.argv[1]
    size = int(sys.argv[2]) if len(sys.argv) > 2 else 518
    m = onnx.load(path)

    patched = 0
    for n in m.graph.node:
        if n.op_type == "Resize":
            for a in n.attribute:
                if a.name == "mode" and a.s == b"cubic":
                    a.s = b"linear"
                    patched += 1

    n_vi = len(m.graph.value_info)
    m.graph.ClearField("value_info")

    for inp in m.graph.input:
        dims = inp.type.tensor_type.shape.dim
        for i, v in enumerate([1, 3, size, size]):
            dims[i].ClearField("dim_param")
            dims[i].dim_value = v
    for out in m.graph.output:
        for d in out.type.tensor_type.shape.dim:
            d.ClearField("dim_param")
            d.ClearField("dim_value")

    onnx.save(m, path)
    print(f"patched: cubic Resize x{patched} -> linear, "
          f"value_info x{n_vi} 除去, 入力 1x3x{size}x{size} に固定")


if __name__ == "__main__":
    main()
