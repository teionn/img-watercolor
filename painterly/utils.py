import numpy as np
import cv2


def resize_to_long_side(img: np.ndarray, long_side: int) -> np.ndarray:
    h, w = img.shape[:2]
    scale = long_side / max(h, w)
    if scale >= 1:
        return img
    return cv2.resize(img, (round(w * scale), round(h * scale)), interpolation=cv2.INTER_AREA)


def luminance(rgb: np.ndarray) -> np.ndarray:
    """明度（用于"由暗到亮"的排序）。rgb 为 float [0,1] 或 uint8。"""
    rgb = np.asarray(rgb, dtype=np.float32)
    return rgb[..., 0] * 0.299 + rgb[..., 1] * 0.587 + rgb[..., 2] * 0.114


def save_rgb(path, img):
    """img: uint8 RGB → 写盘（cv2 需要 BGR）。"""
    cv2.imwrite(str(path), cv2.cvtColor(img, cv2.COLOR_RGB2BGR))
