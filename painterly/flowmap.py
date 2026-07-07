"""灰度梯度 → 法线图 / 流场（笔触方向场）。

关键不变量：笔触方向 = 边缘切线方向（isophote），是 π 周期的"朝向"而非向量，
平滑和重采样必须在结构张量（等价于 2θ 复数域）上做，直接对角度求平均会在
0/π 交界处出错。
"""
import numpy as np
import cv2


def gradients(gray: np.ndarray):
    gx = cv2.Sobel(gray, cv2.CV_32F, 1, 0, ksize=3)
    gy = cv2.Sobel(gray, cv2.CV_32F, 0, 1, ksize=3)
    return gx, gy


def normal_map(gx: np.ndarray, gy: np.ndarray, strength: float = 4.0) -> np.ndarray:
    """把模糊灰度当高度场生成切线空间法线图（所以整体偏蓝紫）。"""
    nx, ny, nz = -gx * strength, gy * strength, np.ones_like(gx)
    norm = np.sqrt(nx * nx + ny * ny + nz * nz)
    n = np.stack([nx / norm, ny / norm, nz / norm], axis=-1)
    return ((n * 0.5 + 0.5) * 255).astype(np.uint8)


def flow_field(gx: np.ndarray, gy: np.ndarray, smooth_sigma: float = 6.0):
    """结构张量平滑后的方向场。

    返回 (theta, coherence)：
      theta      — 每个像素的笔触方向（边缘切线，弧度，π 周期）
      coherence  — 方向的可信度 0..1（各向异性程度），平坦区接近 0
    """
    jxx = cv2.GaussianBlur(gx * gx, (0, 0), smooth_sigma)
    jxy = cv2.GaussianBlur(gx * gy, (0, 0), smooth_sigma)
    jyy = cv2.GaussianBlur(gy * gy, (0, 0), smooth_sigma)

    # 结构张量主特征向量方向 = 平滑后的梯度朝向；笔触沿其垂直方向
    phi = 0.5 * np.arctan2(2 * jxy, jxx - jyy)
    theta = phi + np.pi / 2

    tmp = np.sqrt((jxx - jyy) ** 2 + 4 * jxy * jxy)
    lam1, lam2 = (jxx + jyy + tmp) / 2, (jxx + jyy - tmp) / 2
    coherence = np.where(lam1 + lam2 > 1e-8, (lam1 - lam2) / (lam1 + lam2 + 1e-8), 0.0)
    return theta.astype(np.float32), coherence.astype(np.float32)


def flow_map_vis(theta: np.ndarray, coherence: np.ndarray) -> np.ndarray:
    """流场可视化。方向是 π 周期的，用 2θ 编码进 R/G；
    压低摆幅、抬高基底，得到橄榄绿基调的 flow map 贴图观感。"""
    r = np.cos(2 * theta) * 0.22 * (0.3 + 0.7 * coherence) + 0.52
    g = np.sin(2 * theta) * 0.22 * (0.3 + 0.7 * coherence) + 0.50
    b = coherence * 0.18 + 0.06
    return (np.clip(np.stack([r, g, b], axis=-1), 0, 1) * 255).astype(np.uint8)
