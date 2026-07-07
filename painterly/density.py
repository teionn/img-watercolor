"""密度图：按画面"密度"调度笔刷——轮廓和狭窄处小硬刷，开阔低反差处大软刷。

密度 = 归一化( 模糊(梯度幅值) + 模糊(Posterize 边界) )，
密度高 → 笔刷小、用硬刷；密度低 → 笔刷大、用软刷。
"""
import numpy as np
import cv2


def density_map(gx: np.ndarray, gy: np.ndarray, poster_edge: np.ndarray,
                sigma: float = 8.0, edge_weight: float = 1.0) -> np.ndarray:
    mag = np.hypot(gx, gy)
    mag = mag / (np.percentile(mag, 98) + 1e-8)  # 按分位数归一，避免个别极值把整体压扁
    d = cv2.GaussianBlur(mag, (0, 0), sigma) + edge_weight * cv2.GaussianBlur(poster_edge, (0, 0), sigma)
    d = d / (np.percentile(d, 95) + 1e-8)
    return np.clip(d, 0, 1).astype(np.float32)


def density_vis(density: np.ndarray, hard_threshold: float) -> np.ndarray:
    """红 = 高密度/小硬刷区，蓝 = 低密度/大软刷区。"""
    h, w = density.shape
    vis = np.zeros((h, w, 3), np.float32)
    hard = density >= hard_threshold
    vis[..., 0] = np.where(hard, density, 0)          # R
    vis[..., 2] = np.where(~hard, 1.0 - density, 0)   # B
    return (vis * 255).astype(np.uint8)


def brush_size_at(density: np.ndarray, size_min: float, size_max: float,
                  gamma: float = 0.7) -> np.ndarray:
    """密度 → 笔刷半径的映射（密度高 → 小）。gamma<1 让中等密度也偏向小刷子，
    轮廓才收得住。"""
    return size_max + (size_min - size_max) * np.power(density, gamma)
