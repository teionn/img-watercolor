"""笔刷贴图：程序化生成灰度 alpha "印章"，一笔 = 沿笔迹以固定间距、
按局部流向旋转后反复盖章（和用 Photoshop 笔刷笔尖盖章同一思路，
自定义笔尖走 load_custom）。

贴图约定：笔迹方向为 +x，笔刷宽度沿 y。
"""
import numpy as np
import cv2

_RNG = np.random.default_rng(7)
_NOISE_CACHE = {}


def _noise(shape, sigma, key):
    """带限噪声（缓存种子保证同风格笔刷形态稳定但不同笔略有差异由调用端加扰）。"""
    if (key, shape, sigma) not in _NOISE_CACHE:
        n = _RNG.random(shape).astype(np.float32)
        n = cv2.GaussianBlur(n, (0, 0), sigma)
        n = (n - n.min()) / (n.max() - n.min() + 1e-8)
        _NOISE_CACHE[(key, shape, sigma)] = n
    return _NOISE_CACHE[(key, shape, sigma)]


def _smoothstep(e0, e1, x):
    t = np.clip((x - e0) / (e1 - e0 + 1e-8), 0, 1)
    return t * t * (3 - 2 * t)


def _base_texture(style: str, size: int = 96) -> np.ndarray:
    """生成基准笔刷 alpha（float 0..1），笔迹方向 = +x。

    一律软边——alpha 用平滑衰减而不是二值掩码：画布是低分辨率的，
    放大后硬边会读成"方块"而不是笔触。"""
    s = size
    yy, xx = np.mgrid[0:s, 0:s].astype(np.float32)
    cx = cy = (s - 1) / 2
    u = (xx - cx) / (s / 2)   # 沿笔迹
    v = (yy - cy) / (s / 2)   # 垂直笔迹（笔刷宽度方向）
    r = np.sqrt(u * u + v * v)

    if style == "soft":
        # 软圆刷：沿笔迹略拉长的高斯 + 极淡的鬃毛纹理——大面积平涂时
        # 才会留下顺流向的丝状痕迹，纯高斯会像喷枪
        core = 0.9 * np.exp(-(u ** 2 / (2 * 0.42 ** 2) + v ** 2 / (2 * 0.30 ** 2)))
        bristle = _noise((s, 1), 1.8, "soft_bristle")
        a = core * (0.86 + 0.14 * bristle)

    elif style == "flat":
        # 平头刷：窄羽化的矩形笔肚 + 鬃毛条纹 + 噪声毛边，两端收笔
        ragged = _noise((s, s), 3.0, "flat_edge") * 0.22
        body_v = 1 - _smoothstep(0.58 - ragged, 0.88, np.abs(v))
        body_u = 1 - _smoothstep(0.42, 0.95, np.abs(u))
        bristle = _noise((s, 1), 1.4, "flat_bristle")
        a = body_v * body_u * (0.70 + 0.30 * bristle)
        a = cv2.GaussianBlur(a, (0, 0), s * 0.010)

    elif style == "triangle":
        # 三角刀：类似油画刀的三角笔尖，芯实边略软
        tri = np.zeros((s, s), np.float32)
        pts = np.array([[cx - s * 0.30, cy - s * 0.34],
                        [cx - s * 0.30, cy + s * 0.34],
                        [cx + s * 0.40, cy]], np.int32)
        cv2.fillPoly(tri, [pts], 1.0)
        grain = 0.85 + 0.15 * _noise((s, s), 2.0, "tri_grain")
        a = np.clip(cv2.GaussianBlur(tri, (0, 0), s * 0.018) * 1.25, 0, 1) * grain

    elif style == "charcoal":
        core = np.clip(1.2 - r * 1.35, 0, 1)
        grain = _noise((s, s), 0.9, "charcoal_grain")
        a = core * np.clip(grain * 1.6 - 0.25, 0, 1)
        a = cv2.GaussianBlur(a, (0, 0), s * 0.012)

    elif style == "pastel":
        core = np.clip(1.35 - r * 1.4, 0, 1) ** 0.7
        paper = _noise((s, s), 1.6, "pastel_paper")
        a = core * (0.35 + 0.65 * _smoothstep(0.30, 0.55, paper))

    elif style == "oil":
        # 油画刷：软边笔肚 + 鬃毛沟壑（厚涂感）
        body = (1 - _smoothstep(0.35, 1.0, np.abs(v))) * (1 - _smoothstep(0.25, 1.0, np.abs(u)))
        bristle = _noise((s, 1), 1.6, "oil_bristle")
        a = body * (0.55 + 0.45 * bristle)

    else:
        raise ValueError(f"unknown brush style: {style}")
    return np.clip(a, 0, 1).astype(np.float32)


_BASE = {}
_STAMP_CACHE = {}
ANGLE_BINS = 24


def get_stamp(style: str, radius: float, angle: float) -> np.ndarray:
    """取一枚旋转+缩放后的笔刷印章（角度量化到 ANGLE_BINS 档并缓存）。"""
    rad = max(2, int(round(radius)))
    abin = int(round(angle / np.pi * ANGLE_BINS)) % ANGLE_BINS
    key = (style, rad, abin)
    if key not in _STAMP_CACHE:
        if style not in _BASE:
            _BASE[style] = _base_texture(style)
        base = _BASE[style]
        d = rad * 2 + 1
        tex = cv2.resize(base, (d, d), interpolation=cv2.INTER_AREA)
        ang_deg = np.degrees(abin * np.pi / ANGLE_BINS)
        m = cv2.getRotationMatrix2D((d / 2, d / 2), ang_deg, 1.0)
        stamp = cv2.warpAffine(tex, m, (d, d), flags=cv2.INTER_LINEAR)
        if rad < 5:
            stamp = cv2.GaussianBlur(stamp, (0, 0), 0.6)  # 超小印章去锯齿/方块感
        _STAMP_CACHE[key] = stamp
    return _STAMP_CACHE[key]


def load_custom(path: str, style_name: str):
    """把 Photoshop 笔尖导出的灰度 PNG（白 = 着色区）注册为一种笔刷风格。"""
    tex = cv2.imread(str(path), cv2.IMREAD_GRAYSCALE)
    if tex is None:
        raise FileNotFoundError(path)
    s = max(tex.shape)
    pad = np.zeros((s, s), np.float32)
    pad[:tex.shape[0], :tex.shape[1]] = tex.astype(np.float32) / 255.0
    _BASE[style_name] = pad
    return style_name
