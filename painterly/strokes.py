"""笔触绘制核心，按手绘油画的习惯组织：

  1. 笔触沿流场"流动"——从种子点沿方向场双向走折线；
  2. 全局按明度 由暗到亮 排序绘制——亮色永远叠在暗色上（先铺暗部再提亮）；
  3. 颜色取自量化图，形状被 Posterize 边界隐式约束（走线遇到颜色突变即停）；
  4. 密度决定笔刷大小与软硬，部分笔触从"侧面"采色混合。
"""
from dataclasses import dataclass, field
import numpy as np
import cv2

from .brushes import get_stamp
from .utils import luminance


@dataclass
class Stroke:
    points: list          # [(x, y), ...] 折线
    color: np.ndarray     # float RGB 0..1
    radius: float
    style: str
    opacity: float
    angle0: float = 0.0   # 种子点的流场角度（单章笔触没有走向可算，用它兜底）
    tag: str = "soft"     # hard / standard / soft，Debug 视图按它着色
    lum: float = field(init=False)

    def __post_init__(self):
        self.lum = float(luminance(self.color))


class PaintEngine:
    def __init__(self, target_rgb, theta, coherence, density, rng=None):
        """target_rgb: float 0..1 的量化目标图（绘制分辨率）；
        theta/coherence/density: 同分辨率的方向场、相干度、密度图。"""
        self.target = target_rgb
        self.theta = theta
        self.coherence = coherence
        self.density = density
        self.h, self.w = target_rgb.shape[:2]
        self.rng = rng or np.random.default_rng(42)

    def _dir_at(self, x, y):
        th = self.theta[int(y), int(x)]
        return np.cos(th), np.sin(th)

    def trace(self, x0, y0, radius, color, max_len_factor=3.0, color_tol=0.10):
        """从种子点沿流场双向走折线。方向场是 π 周期的，
        每步都把方向翻转到与上一步同侧，笔迹才不会打折返回。
        遇到目标颜色突变（≈越过 Posterize 边界）或出界就停。"""
        step = max(1.5, radius * 0.4)
        max_steps = max(2, int(radius * max_len_factor / step))
        pts = [(x0, y0)]
        for direction in (1.0, -1.0):
            cx, cy = x0, y0
            dx, dy = self._dir_at(x0, y0)
            dx, dy = dx * direction, dy * direction
            for _ in range(max_steps // 2):
                ux, uy = self._dir_at(cx, cy)
                if ux * dx + uy * dy < 0:
                    ux, uy = -ux, -uy
                cx, cy = cx + ux * step, cy + uy * step
                if not (0 <= cx < self.w and 0 <= cy < self.h):
                    break
                if np.linalg.norm(self.target[int(cy), int(cx)] - color) > color_tol:
                    break
                if direction > 0:
                    pts.append((cx, cy))
                else:
                    pts.insert(0, (cx, cy))
                dx, dy = ux, uy
        return pts

    def make_strokes(self, spacing, size_of, style_of, mask=None,
                     side_sample_prob=0.3, jitter=0.5, tag_of=None,
                     max_len_factor=3.0):
        """抖动网格撒种子 → 生成笔触列表（未绘制）。
        size_of(density)->半径, style_of(density)->笔刷名, mask 限制种子区域，
        tag_of(density)->hard/standard/soft（Debug 视图用）。"""
        strokes = []
        for gy in np.arange(spacing / 2, self.h, spacing):
            for gx in np.arange(spacing / 2, self.w, spacing):
                x = gx + self.rng.uniform(-1, 1) * spacing * jitter
                y = gy + self.rng.uniform(-1, 1) * spacing * jitter
                xi, yi = int(np.clip(x, 0, self.w - 1)), int(np.clip(y, 0, self.h - 1))
                d = float(self.density[yi, xi])
                if mask is not None and not mask[yi, xi]:
                    continue
                radius = float(size_of(d))
                color = self.target[yi, xi].copy()

                # 模仿画家从相邻色面"借色"的习惯：部分笔触的颜色
                # 从垂直流向的偏移处采样后混合
                if self.rng.random() < side_sample_prob:
                    ux, uy = self._dir_at(xi, yi)
                    off = radius * 1.2 * (1 if self.rng.random() < 0.5 else -1)
                    sx = int(np.clip(xi - uy * off, 0, self.w - 1))
                    sy = int(np.clip(yi + ux * off, 0, self.h - 1))
                    color = color * 0.6 + self.target[sy, sx] * 0.4

                pts = self.trace(xi, yi, radius, self.target[yi, xi],
                                 max_len_factor=max_len_factor)
                tag = tag_of(d) if tag_of else "soft"
                # 硬刷要"咬"住轮廓，不透明度给高；软刷低一些方便叠色
                lo, hi = {"hard": (0.90, 1.0), "standard": (0.82, 0.96),
                          "soft": (0.75, 0.92)}[tag]
                strokes.append(Stroke(pts, color, radius, style_of(d),
                                      opacity=self.rng.uniform(lo, hi),
                                      angle0=float(self.theta[yi, xi]),
                                      tag=tag))
        return strokes

    @staticmethod
    def render(canvas, strokes, wet=0.0, on_stroke=None):
        """按明度由暗到亮排序后逐笔盖章（油画顺序：暗色打底，亮色后上）。
        wet: 湿画法——落笔前和画布已有颜色掺和的比例，笔触之间才会互相"带色"。
        canvas: float RGB 0..1，就地修改。"""
        h, w = canvas.shape[:2]
        for i, st in enumerate(sorted(strokes, key=lambda s: s.lum)):
            if wet > 0:
                mx, my = st.points[len(st.points) // 2]
                under = canvas[int(np.clip(my, 0, h - 1)), int(np.clip(mx, 0, w - 1))]
                st.color = st.color * (1 - wet) + under * wet
            _draw_stroke(canvas, st)
            if on_stroke is not None:
                on_stroke(i, canvas)
        return canvas


DEBUG_COLORS = {"hard": (235, 60, 50), "standard": (70, 215, 90), "soft": (65, 90, 240)}


def render_debug(shape_hw, strokes, scale=2):
    """笔触调试视图：每根笔触画成折线，颜色 = 笔刷档位
    （红=硬刷/轮廓，绿=标准刷，蓝=软刷/大面）。"""
    h, w = shape_hw
    img = np.full((h * scale, w * scale, 3), 30, np.uint8)
    for st in sorted(strokes, key=lambda s: s.lum):
        pts = (np.array(st.points, np.float32) * scale).astype(np.int32)
        thick = max(1, int(st.radius * 0.5 * scale))
        cv2.polylines(img, [pts], False, DEBUG_COLORS.get(st.tag, (200, 200, 200)),
                      thick, cv2.LINE_AA)
    return img


def _draw_stroke(canvas, st: Stroke):
    h, w = canvas.shape[:2]
    pts = st.points
    spacing = max(1.0, st.radius * 0.25)
    # 沿折线等距重采样出盖章位置
    stamp_pts = [pts[0]]
    acc = 0.0
    for (x0, y0), (x1, y1) in zip(pts, pts[1:]):
        seg = float(np.hypot(x1 - x0, y1 - y0))
        while acc + seg >= spacing:
            t = (spacing - acc) / seg
            x0, y0 = x0 + (x1 - x0) * t, y0 + (y1 - y0) * t
            seg = float(np.hypot(x1 - x0, y1 - y0))
            stamp_pts.append((x0, y0))
            acc = 0.0
        acc += seg

    n = len(stamp_pts)
    for j, (x, y) in enumerate(stamp_pts):
        # 印章角度取局部走向
        if n > 1:
            k = min(j, n - 2)
            ang = np.arctan2(stamp_pts[k + 1][1] - stamp_pts[k][1],
                             stamp_pts[k + 1][0] - stamp_pts[k][0])
        else:
            ang = st.angle0
        # 收笔渐隐：笔触两端压低不透明度（起笔/提笔），中段最实；硬刷渐隐弱
        t = j / (n - 1) if n > 1 else 0.5
        floor = {"hard": 0.85, "standard": 0.65, "soft": 0.50}.get(st.tag, 0.55)
        envelope = floor + (1 - floor) * np.sin(np.pi * t) ** 0.7
        alpha = get_stamp(st.style, st.radius, -ang) * (st.opacity * envelope)
        d = alpha.shape[0]
        x0, y0 = int(round(x)) - d // 2, int(round(y)) - d // 2
        x1, y1 = x0 + d, y0 + d
        sx0, sy0 = max(0, -x0), max(0, -y0)
        sx1, sy1 = d - max(0, x1 - w), d - max(0, y1 - h)
        if sx1 <= sx0 or sy1 <= sy0:
            continue
        a = alpha[sy0:sy1, sx0:sx1, None]
        region = canvas[max(0, y0):min(h, y1), max(0, x0):min(w, x1)]
        region *= (1 - a)
        region += st.color * a
