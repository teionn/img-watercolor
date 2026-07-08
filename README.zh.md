# img-watercolor — 流场引导笔触绘画渲染

> 日本語: [README.md](README.md)

从照片生成**流场图**，让笔刷沿流向"流动"，按**由暗到亮**的油画顺序，
用**密度自适应**的三档软硬笔刷，在**低分辨率画布**上作画再平滑放大，
得到油画速涂风格的图。

算法原理详见 **[doc/algorithm.zh.md](doc/algorithm.zh.md)**
（日本語版: [doc/algorithm.md](doc/algorithm.md)）。

## 快速开始

```bash
pip install -r requirements.txt
python scripts/fetch_samples.py        # 生成 input/ 测试图（可换成自己的照片）

python gui.py                          # GUI（深色界面，英文）
python gui.py input/cat.png            # 启动时直接加载图片

python run.py input/cat.png            # 命令行
python run.py input/*.png --resolution 200 --pixels 80   # 更强的速涂/抽象感
python run.py input/cat.png --process-gif                # 逐笔绘制过程动画
```

GUI 支持拖放图片、全部滑杆参数、后台线程渲染（各中间图算完即刷新、
绘制过程实时预览）、**Show Process** 逐笔回放、**Save Image** 另存成品。

结果输出到 `output/<图名>/`：

| 文件 | 内容 |
|---|---|
| `1_original.png` | 原图 |
| `2_quantized.png` | 调色板量化图（取色来源） |
| `3_posterize_edges.png` | Posterize 色块边界 |
| `4_gray_blur.png` | 灰度 + 高斯模糊 |
| `5_normal_map.png` | 法线图（蓝紫） |
| `6_flow_map.png` | 流场图（橄榄绿，笔触方向场） |
| `7_strokes_debug.png` | 笔触调试视图（红=硬刷/绿=标准刷/蓝=软刷） |
| `8_painting.png` | 最终绘画 |
| `density.png` | 密度图（笔刷调度依据） |
| `palette_swatch.png` / `color_wheel.png` | 色板格子 / 色环散点 |
| `overview.png` | 全阶段拼图 |
| `process.gif` | 逐笔绘制回放（`--process-gif`） |

## 命令行参数

| 参数 | 默认 | 说明 |
|---|---|---|
| `--pixels` | 96 | 取色/概括分辨率，长边 px，越小越概括 |
| `--resolution` | 360 | **绘画画布**长边 px，越小越抽象 |
| `--palette` | 36 | 量化颜色数 |
| `--color-wheel` | rgb | 量化色彩空间（rgb / lab） |
| `--posterize-blur` | 2 | 量化前高斯 σ |
| `--normal-blur` | 8 | 求梯度前高斯 σ |
| `--brush-size` | 15 | 基准笔刷半径（画布 px） |
| `--hard-brush` | triangle | 轮廓/高密度处（triangle/charcoal/…） |
| `--standard-brush` | flat | 中等密度 |
| `--soft-brush` | soft | 大面/低反差处（soft/oil/pastel） |
| `--strokes` | 1.0 | 笔触密度倍率 |
| `--wet` | 0.18 | 湿画法：落笔前与画布颜色的混合比例 |
| `--out-long` | 1080 | 最终输出长边（从画布三次插值放大） |

**质感的三个关键**（详见 [doc/algorithm.zh.md](doc/algorithm.zh.md) 第三节）：
低分辨率画布放大（不是在大图上画）、软边笔刷贴图（没有二值掩码）、
湿混色 + 收笔渐隐。想要更强的抽象感就把 `--resolution` 降到 200。

**用自己的 Photoshop 笔刷**：把笔尖导出为灰度 PNG（白=着色区），
传路径即可：`python run.py input/cat.png --hard-brush my_brush.png`。

## Rust 版（桌面 & iOS 开发主线）

算法本体已移植到 pure Rust 的 **`crates/painterly-core`**
（不依赖 OpenCV / numpy，可直接嵌入桌面与 iOS）。
路线图见 [doc/roadmap.md](doc/roadmap.md)（日本語）。

```bash
cargo run --release -p painterly-cli -- input/cat.png   # CLI（与 run.py 同一套参数）

cd apps/desktop/src-tauri && cargo tauri dev            # 桌面应用（Tauri 2）
```

Rust 版新增 — **深度图控制笔触粗细**：近处笔触细、远处笔触大而松
（详见 [doc/depth.md](doc/depth.md)，日本語）。
`scripts/fetch_models.sh` 下载 Depth Anything V2 small（NeurIPS 2024）后自动
用神经网络估计深度，否则回退到内置启发式（对焦度 + 大气透视 + 上下先验）。
`--depth-detail 0..1` 调节强度（0 关闭）、`--depth my_depth.png` 指定外部深度图
（白 = 近）、`--depth-invert` 反转远近。中间图 `depth_map.png` 可查看估计结果。

## 代码结构

```
crates/
  painterly-core/    # 核心算法（pure Rust，下方 painterly/ 的移植）
  painterly-cli/     # 命令行（run.py 对应）
  painterly-ffi/     # UniFFI 绑定（iOS / Swift）
apps/
  desktop/           # 桌面应用（Tauri 2，Windows/macOS/Linux）
  ios/               # iOS 应用（SwiftUI，构建步骤见 apps/ios/README.md）
painterly/           # Python 参考实现
  palette.py   # K-Means 调色板量化、Posterize 边界、色板/色环可视化
  flowmap.py   # 灰度模糊 → Sobel → 法线图 / 结构张量流场
  density.py   # 密度图（梯度 + 色块边界）→ 笔刷大小/三档软硬调度
  brushes.py   # 程序化笔刷贴图（triangle/flat/soft/oil/pastel/charcoal）+ 自定义 PNG
  strokes.py   # 沿流场走线、由暗到亮排序、湿混色、收笔渐隐、调试视图
  pipeline.py  # 编排 + 全部中间图落盘 + GUI 回调钩子
run.py         # 命令行入口（Python）
gui.py         # PySide6 图形界面（Python，将由 Tauri 版替代）
```

## 调参建议

- 强抽象/速涂感：`--resolution 200 --pixels 80 --palette 36 --brush-size 15`
- 保留更多细节：`--resolution 480 --pixels 160 --palette 64 --strokes 1.3`
- 更"干"的笔触（少混色）：`--wet 0.05`
- 炭笔/色粉风：`--hard-brush charcoal --soft-brush pastel`
