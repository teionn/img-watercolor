"""调色板量化、Posterize 色块边界，以及调色板的两种可视化（色板格子、色环散点）。"""
import numpy as np
import cv2

from .utils import luminance


def quantize(img_rgb: np.ndarray, k: int, space: str = "rgb"):
    """K-Means 把图像聚成 k 种颜色。

    返回 (quantized_rgb uint8, labels HxW int32, palette k x 3 uint8, counts k)。
    space='lab' 时在 Lab 空间聚类（感知上更均匀），色板仍以 RGB 返回。
    """
    h, w = img_rgb.shape[:2]
    # 先双边滤波压掉毛发/纹理级噪声，否则色块边界全是椒盐点，连不成干净的轮廓线
    smooth = cv2.bilateralFilter(img_rgb, 9, 40, 7)
    if space == "lab":
        feat = cv2.cvtColor(smooth, cv2.COLOR_RGB2LAB).reshape(-1, 3).astype(np.float32)
    else:
        feat = smooth.reshape(-1, 3).astype(np.float32)

    criteria = (cv2.TERM_CRITERIA_EPS + cv2.TERM_CRITERIA_MAX_ITER, 25, 0.5)
    _, labels, centers = cv2.kmeans(
        feat, k, None, criteria, attempts=3, flags=cv2.KMEANS_PP_CENTERS
    )
    labels = labels.reshape(h, w)
    if k <= 255:
        # 3x3 多数投票清掉残余斑点（median 对局部二值情形即多数投票）
        labels = cv2.medianBlur(labels.astype(np.uint8), 3).astype(np.int32)

    if space == "lab":
        centers = cv2.cvtColor(
            centers.reshape(1, -1, 3).astype(np.uint8), cv2.COLOR_LAB2RGB
        ).reshape(-1, 3)
    else:
        centers = centers.astype(np.uint8)

    counts = np.bincount(labels.ravel(), minlength=k)
    quantized = centers[labels]
    return quantized, labels, centers, counts


def apply_palette(img_rgb: np.ndarray, palette: np.ndarray, chunk: int = 200_000) -> np.ndarray:
    """把任意图像逐像素吸附到最近的调色板颜色（分块算避免 HxWxK 距离矩阵爆内存）。
    用于把低分辨率量化图平滑放大后重新"钉"回色板，消掉最近邻放大的马赛克边。"""
    flat = img_rgb.reshape(-1, 3).astype(np.int32)
    pal = palette.astype(np.int32)
    out = np.empty(flat.shape[0], np.int32)
    for i in range(0, flat.shape[0], chunk):
        d = ((flat[i:i + chunk, None, :] - pal[None, :, :]) ** 2).sum(-1)
        out[i:i + chunk] = d.argmin(1)
    return palette[out].reshape(img_rgb.shape)


def posterize_edges(labels: np.ndarray, quantized: np.ndarray) -> np.ndarray:
    """相邻像素聚类标签不同处即色块边界，黑底上用该处颜色描出。"""
    edge = np.zeros(labels.shape, bool)
    edge[:, 1:] |= labels[:, 1:] != labels[:, :-1]
    edge[1:, :] |= labels[1:, :] != labels[:-1, :]
    vis = np.zeros_like(quantized)
    vis[edge] = quantized[edge]
    return vis


def edge_mask(labels: np.ndarray) -> np.ndarray:
    edge = np.zeros(labels.shape, np.float32)
    edge[:, 1:] = np.maximum(edge[:, 1:], (labels[:, 1:] != labels[:, :-1]).astype(np.float32))
    edge[1:, :] = np.maximum(edge[1:, :], (labels[1:, :] != labels[:-1, :]).astype(np.float32))
    return edge


def palette_swatch(palette: np.ndarray, cell: int = 22, cols: int = 12) -> np.ndarray:
    """色板格子：按 色相段 + 明度 排序的小方块。"""
    hsv = cv2.cvtColor(palette.reshape(1, -1, 3), cv2.COLOR_RGB2HSV).reshape(-1, 3)
    order = np.lexsort((-luminance(palette), hsv[:, 0] // 30))
    k = len(palette)
    rows = int(np.ceil(k / cols))
    out = np.full((rows * cell, cols * cell, 3), 45, np.uint8)
    for i, idx in enumerate(order):
        r, c = divmod(i, cols)
        out[r * cell + 1:(r + 1) * cell - 1, c * cell + 1:(c + 1) * cell - 1] = palette[idx]
    return out


def color_wheel(palette: np.ndarray, counts: np.ndarray, size: int = 360) -> np.ndarray:
    """色环散点图：外圈是色相环，调色板颜色按 (色相→角度, 饱和度→半径) 摆放，
    点的面积 ∝ 像素占比。"""
    out = np.full((size, size, 3), 45, np.uint8)
    cx = cy = size / 2
    r_outer, r_ring = size * 0.46, size * 0.035

    yy, xx = np.mgrid[0:size, 0:size]
    dx, dy = xx - cx, yy - cy
    rr = np.hypot(dx, dy)
    ang = (np.degrees(np.arctan2(-dy, dx)) + 360) % 360  # 数学角，逆时针
    ring = (rr > r_outer - r_ring) & (rr < r_outer)
    hsv = np.zeros((size, size, 3), np.uint8)
    hsv[..., 0] = (ang / 2).astype(np.uint8)  # OpenCV 的 H 是 0..180
    hsv[..., 1] = 255
    hsv[..., 2] = 255
    ring_rgb = cv2.cvtColor(hsv, cv2.COLOR_HSV2RGB)
    out[ring] = ring_rgb[ring]

    pal_hsv = cv2.cvtColor(palette.reshape(1, -1, 3), cv2.COLOR_RGB2HSV).reshape(-1, 3)
    share = counts / counts.sum()
    r_inner = r_outer - r_ring * 2
    for (h, s, v), col, sh in zip(pal_hsv, palette, share):
        theta = np.radians(h * 2.0)
        radius = (s / 255.0) * r_inner
        px = int(cx + radius * np.cos(theta))
        py = int(cy - radius * np.sin(theta))
        pr = max(2, int(np.sqrt(sh) * size * 0.12))
        cv2.circle(out, (px, py), pr, tuple(int(c) for c in col), -1, cv2.LINE_AA)
    return out
