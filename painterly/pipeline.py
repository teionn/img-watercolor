"""管线编排：输入一张图 → 全部中间图 + 最终画落盘。

分辨率模型（三个长边像素值）：
  - pixels      : 取色/形状概括分辨率。刻意取得很小，等价于画家眯眼观察
  - resolution  : 绘画画布分辨率。在低分辨率画布上作画、最后平滑放大到
                  out_long —— 笔触边缘全是抗锯齿过渡，这是成品
                  "像笔刷不像方块"的最大来源
  - out_long    : 最终输出长边（三次插值放大）
方向场 θ 是 π 周期的，重采样必须走 (cos2θ, sin2θ) 两通道再 atan2 还原。
"""
from pathlib import Path
import numpy as np
import cv2

from . import palette as P, flowmap as F, density as D
from .strokes import PaintEngine, render_debug
from .utils import resize_to_long_side, save_rgb

PARAMS_DEFAULT = dict(
    pixels=96,              # 取色/概括分辨率（长边 px），越小形状越概括
    resolution=360,         # 绘画画布长边 px，越小成品越抽象
    palette=36,             # 量化颜色数
    color_space="rgb",      # 量化色彩空间 rgb / lab
    posterize_blur=2.0,     # 量化前的高斯 σ（px，@pixels 分辨率）
    normal_blur=8.0,        # 求梯度前的高斯 σ（px，@resolution 分辨率）
    brush_size=15,          # 基准笔刷半径（画布坐标系 px）
    hard_brush="triangle",  # 轮廓/高密度处
    standard_brush="flat",  # 中等密度
    soft_brush="soft",      # 大面/低反差处
    strokes_scale=1.0,      # 笔触密度整体倍率
    hard_quantile=0.85,     # 密度前 15% → 硬刷（按分位数自适应，毛发类图不至于全是硬刷）
    standard_quantile=0.55, # 密度 55%~85% → 标准刷，其余 → 软刷
    side_sample_prob=0.3,
    wet=0.18,               # 湿画法：落笔前与画布已有颜色的混合比例
    saturation=1.15,
    out_long=1080,
    seed=42,
    process_gif=False,
)


def _upscale_theta(theta, coherence, size_wh):
    c2 = cv2.resize(np.cos(2 * theta), size_wh, interpolation=cv2.INTER_LINEAR)
    s2 = cv2.resize(np.sin(2 * theta), size_wh, interpolation=cv2.INTER_LINEAR)
    coh = cv2.resize(coherence, size_wh, interpolation=cv2.INTER_LINEAR)
    return 0.5 * np.arctan2(s2, c2).astype(np.float32), coh


def run_pipeline(img_rgb: np.ndarray, out_dir, on_stage=None, on_paint_progress=None,
                 collect_frames=False, **overrides):
    """on_stage(name, rgb_uint8)      —— 每个中间图算完即回调（GUI 实时刷新用）
    on_paint_progress(frac, canvas)   —— 绘制过程回调，frac 0..1
    collect_frames=True               —— 结果 dict 里带上逐笔快照帧（回放用）"""
    p = {**PARAMS_DEFAULT, **overrides}
    out = Path(out_dir)
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(p["seed"])

    def _stage(name, img):
        save_rgb(out / f"{name}.png", img)
        if on_stage:
            on_stage(name, img)

    small = resize_to_long_side(img_rgb, p["pixels"])
    if p["posterize_blur"] > 0:
        small = cv2.GaussianBlur(small, (0, 0), p["posterize_blur"])
    quant, labels, pal, counts = P.quantize(small, p["palette"], p["color_space"])
    poster = P.posterize_edges(labels, quant)
    pedge = P.edge_mask(labels)

    ana = resize_to_long_side(img_rgb, p["resolution"])
    ch, cw = ana.shape[:2]
    gray = cv2.cvtColor(ana, cv2.COLOR_RGB2GRAY).astype(np.float32) / 255.0
    gray = cv2.GaussianBlur(gray, (0, 0), max(0.5, p["normal_blur"]))
    gx, gy = F.gradients(gray)
    nmap = F.normal_map(gx, gy)
    theta, coh = F.flow_field(gx, gy, smooth_sigma=max(2.0, p["resolution"] * 0.02))
    fmap = F.flow_map_vis(theta, coh)

    pe_ana = cv2.resize(pedge, (cw, ch), interpolation=cv2.INTER_LINEAR)
    dens = D.density_map(gx, gy, pe_ana, sigma=max(2.0, p["resolution"] * 0.03))

    _stage("1_original", img_rgb)
    _stage("2_quantized", quant)
    _stage("3_posterize_edges", poster)
    _stage("4_gray_blur", cv2.cvtColor((gray * 255).astype(np.uint8), cv2.COLOR_GRAY2RGB))
    _stage("5_normal_map", nmap)
    _stage("6_flow_map", fmap)
    hard_t = float(np.quantile(dens, p["hard_quantile"]))
    std_t = float(np.quantile(dens, p["standard_quantile"]))
    _stage("density", D.density_vis(dens, hard_t))
    _stage("palette_swatch", P.palette_swatch(pal))
    _stage("color_wheel", P.color_wheel(pal, counts))

    target_u8 = cv2.resize(quant, (cw, ch), interpolation=cv2.INTER_LINEAR)
    target_u8 = P.apply_palette(target_u8, pal)
    if p["saturation"] != 1.0:
        hsv = cv2.cvtColor(target_u8, cv2.COLOR_RGB2HSV).astype(np.float32)
        hsv[..., 1] = np.clip(hsv[..., 1] * p["saturation"], 0, 255)
        target_u8 = cv2.cvtColor(hsv.astype(np.uint8), cv2.COLOR_HSV2RGB)
    target = target_u8.astype(np.float32) / 255

    dens_p = dens
    theta_p, coh_p = theta, coh
    bs = float(p["brush_size"])

    def style_of(d):
        if d > hard_t:
            return p["hard_brush"]
        return p["standard_brush"] if d > std_t else p["soft_brush"]

    def tag_of(d):
        if d > hard_t:
            return "hard"
        return "standard" if d > std_t else "soft"

    engine = PaintEngine(target, theta_p, coh_p, dens_p, rng=rng)

    # 底色：画布 = 画面均色，大号软刷铺底
    canvas = np.ones_like(target) * target.reshape(-1, 3).mean(axis=0)
    under = engine.make_strokes(
        spacing=bs * 2.0 / p["strokes_scale"],
        size_of=lambda d: bs * rng.uniform(1.3, 1.7),
        style_of=lambda d: p["soft_brush"],
        tag_of=lambda d: "soft",
        side_sample_prob=0.15,
    )

    # 主层：密度 → 尺寸 / 硬标软三档
    main = engine.make_strokes(
        spacing=bs * 0.6 / p["strokes_scale"],
        size_of=lambda d: D.brush_size_at(np.float32(d), bs * 0.35, bs * 0.9),
        style_of=style_of,
        tag_of=tag_of,
        side_sample_prob=p["side_sample_prob"],
    )

    # 细节层：只在高密度区补小号硬刷
    detail_mask = dens_p > np.quantile(dens_p, 0.75)
    detail = engine.make_strokes(
        spacing=max(2.0, bs * 0.5) / p["strokes_scale"],
        size_of=lambda d: max(2.5, bs * rng.uniform(0.25, 0.4)),
        style_of=lambda d: p["hard_brush"],
        tag_of=lambda d: "hard",
        mask=detail_mask,
        side_sample_prob=0.15,
    )

    all_strokes = under + main + detail
    _stage("7_strokes_debug", render_debug((ch, cw), main + detail))

    total = len(all_strokes)
    frames = []
    want_frames = collect_frames or p["process_gif"]
    frame_every = max(1, total // 120)
    done_n = [0]

    def cb(_i, cv_):
        done_n[0] += 1
        if done_n[0] % frame_every and done_n[0] != total:
            return
        frame = (np.clip(cv_, 0, 1) * 255).astype(np.uint8).copy()
        if want_frames:
            frames.append(frame)
        if on_paint_progress:
            on_paint_progress(done_n[0] / total, frame)

    use_cb = cb if (want_frames or on_paint_progress) else None
    # 细节层不做湿混色——轮廓要"咬"得住，不能被底色带糊
    for layer, w_ in ((under, p["wet"]), (main, p["wet"]), (detail, 0.0)):
        PaintEngine.render(canvas, layer, wet=w_, on_stroke=use_cb)

    low = (np.clip(canvas, 0, 1) * 255).astype(np.uint8)
    if p["out_long"] > max(ch, cw):
        s = p["out_long"] / max(ch, cw)
        final = cv2.resize(low, (round(cw * s), round(ch * s)), interpolation=cv2.INTER_CUBIC)
    else:
        final = low
    _stage("8_painting", final)

    if p["process_gif"] and frames:
        from PIL import Image
        frames.append(low)
        gw, gh = cw * 2, ch * 2
        imgs = [Image.fromarray(cv2.resize(f, (gw, gh), interpolation=cv2.INTER_CUBIC))
                for f in frames]
        imgs[0].save(out / "process.gif", save_all=True, append_images=imgs[1:],
                     duration=50, loop=0)

    _overview(out)
    return dict(strokes=total, out=str(out),
                frames=frames if collect_frames else None, final=final)


def _overview(out: Path):
    """把全部阶段拼成一张总览图（宫格 + 深灰底）。"""
    names = ["1_original", "2_quantized", "3_posterize_edges", "4_gray_blur",
             "5_normal_map", "6_flow_map", "7_strokes_debug", "8_painting"]
    tiles = []
    th = 260
    for n in names:
        img = cv2.imread(str(out / f"{n}.png"))
        s = th / img.shape[0]
        tiles.append(cv2.resize(img, (int(img.shape[1] * s), th)))
    tw = max(t.shape[1] for t in tiles)
    pad = 12
    cols = 4
    rows = int(np.ceil(len(tiles) / cols))
    sheet = np.full((rows * (th + pad) + pad, cols * (tw + pad) + pad, 3), 34, np.uint8)
    for i, t in enumerate(tiles):
        r, c = divmod(i, cols)
        y, x = pad + r * (th + pad), pad + c * (tw + pad) + (tw - t.shape[1]) // 2
        sheet[y:y + th, x:x + t.shape[1]] = t
    cv2.imwrite(str(out / "overview.png"), sheet)
