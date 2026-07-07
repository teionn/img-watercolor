"""命令行入口。参数默认值与详细说明见 painterly.pipeline.PARAMS_DEFAULT 和 README。

用法:
    python run.py input/cat.png
    python run.py input/*.png --resolution 200 --palette 36 --brush-size 15
    python run.py input/cat.png --process-gif   # 逐笔绘制过程动画
"""
import argparse
import sys
import time
from pathlib import Path

import cv2

from painterly import run_pipeline
from painterly.brushes import load_custom

BUILTIN_BRUSHES = ["triangle", "flat", "soft", "oil", "pastel", "charcoal"]


def main():
    ap = argparse.ArgumentParser(description="流场引导的笔触绘画渲染")
    ap.add_argument("images", nargs="+", help="输入图片路径")
    ap.add_argument("--pixels", type=int, default=96, help="取色/概括分辨率，长边 px，越小越概括")
    ap.add_argument("--resolution", type=int, default=360, help="绘画画布长边 px，越小越抽象")
    ap.add_argument("--palette", type=int, default=36, help="量化颜色数")
    ap.add_argument("--color-wheel", choices=["rgb", "lab"], default="rgb", help="量化色彩空间")
    ap.add_argument("--posterize-blur", type=float, default=2.0, help="量化前高斯σ")
    ap.add_argument("--normal-blur", type=float, default=8.0, help="求梯度前高斯σ")
    ap.add_argument("--brush-size", type=float, default=15, help="基准笔刷半径（画布 px）")
    ap.add_argument("--brush-length", type=float, default=3.0,
                    help="笔触长度上限（半径的倍数），实际还受色块边界截断")
    ap.add_argument("--hard-brush", default="triangle", help=f"硬笔刷: {BUILTIN_BRUSHES} 或灰度 PNG 路径")
    ap.add_argument("--standard-brush", default="flat", help=f"标准笔刷: {BUILTIN_BRUSHES} 或灰度 PNG 路径")
    ap.add_argument("--soft-brush", default="soft", help=f"软笔刷: {BUILTIN_BRUSHES} 或灰度 PNG 路径")
    ap.add_argument("--strokes", type=float, default=1.0, help="笔触密度倍率")
    ap.add_argument("--wet", type=float, default=0.18, help="湿画法混色比例 0..1")
    ap.add_argument("--out-long", type=int, default=1080, help="最终输出长边像素")
    ap.add_argument("--seed", type=int, default=42)
    ap.add_argument("--process-gif", action="store_true", help="输出逐笔绘制过程动画")
    args = ap.parse_args()

    for name, attr in [("hard", "hard_brush"), ("standard", "standard_brush"), ("soft", "soft_brush")]:
        val = getattr(args, attr)
        if val not in BUILTIN_BRUSHES:
            setattr(args, attr, load_custom(val, f"custom_{name}"))

    for path in args.images:
        path = Path(path)
        bgr = cv2.imread(str(path))
        if bgr is None:
            print(f"[skip] 读不到图片: {path}", file=sys.stderr)
            continue
        rgb = cv2.cvtColor(bgr, cv2.COLOR_BGR2RGB)
        out_dir = Path("output") / path.stem
        t0 = time.time()
        info = run_pipeline(
            rgb, out_dir,
            pixels=args.pixels, resolution=args.resolution,
            palette=args.palette, color_space=args.color_wheel,
            posterize_blur=args.posterize_blur, normal_blur=args.normal_blur,
            brush_size=args.brush_size, brush_length=args.brush_length,
            hard_brush=args.hard_brush,
            standard_brush=args.standard_brush, soft_brush=args.soft_brush,
            strokes_scale=args.strokes, wet=args.wet,
            out_long=args.out_long, seed=args.seed,
            process_gif=args.process_gif,
        )
        print(f"[done] {path.name}: {info['strokes']} strokes, "
              f"{time.time() - t0:.1f}s -> {info['out']}")


if __name__ == "__main__":
    main()
