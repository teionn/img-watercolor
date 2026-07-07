"""アプリアイコンの生成（依存ライブラリなし、標準ライブラリのみ）。

濃紺の角丸地に、フローに沿った 3 本のブラシストローク風の帯を描く。
    python3 gen_icons.py
で 32x32.png / 128x128.png / icon.png (512) / icon.ico (256 PNG 埋め込み) を出力する。
"""
import math
import struct
import zlib
from pathlib import Path

HERE = Path(__file__).parent


def write_png(path, size, pixel_fn):
    rows = []
    for y in range(size):
        row = bytearray()
        for x in range(size):
            row += bytes(pixel_fn(x / size, y / size))
        rows.append(row)

    def chunk(tag, data):
        c = struct.pack(">I", len(data)) + tag + data
        return c + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF)

    raw = b"".join(b"\x00" + bytes(r) for r in rows)
    png = (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )
    path.write_bytes(png)
    return png


def pixel(fx, fy):
    # 角丸マスク
    r = 0.18
    cx = min(max(fx, r), 1 - r)
    cy = min(max(fy, r), 1 - r)
    if math.hypot(fx - cx, fy - cy) > r:
        return (0, 0, 0, 0)
    # 地: 上から下へ濃紺のグラデーション
    col = [30 + 18 * fy, 34 + 20 * fy, 48 + 26 * fy]
    # フローに沿う 3 本のストローク（サインカーブの帯）
    strokes = [
        (0.34, 0.10, (222, 168, 92)),   # オーカー
        (0.52, 0.11, (96, 148, 202)),   # ブルー
        (0.70, 0.10, (110, 160, 96)),   # オリーブ
    ]
    for base, width, c in strokes:
        yy = base + 0.07 * math.sin((fx - 0.1) * 5.2) + 0.03 * math.sin(fx * 11)
        d = abs(fy - yy)
        if d < width:
            t = 1 - (d / width) ** 2  # ソフトエッジ
            fade = min(1.0, (fx - 0.10) * 8) * min(1.0, (0.90 - fx) * 8)
            fade = max(0.0, fade)
            a = t * fade
            for i in range(3):
                col[i] = col[i] * (1 - a) + c[i] * a
    return (int(col[0]), int(col[1]), int(col[2]), 255)


def main():
    for size, name in [(32, "32x32.png"), (128, "128x128.png"), (512, "icon.png")]:
        write_png(HERE / name, size, pixel)
        print("wrote", name)
    # ICO: 256px PNG を埋め込む（Windows は 256 の PNG 圧縮 ICO に対応）
    png256 = write_png(HERE / "_256.tmp.png", 256, pixel)
    (HERE / "_256.tmp.png").unlink()
    header = struct.pack("<HHH", 0, 1, 1)
    entry = struct.pack("<BBBBHHII", 0, 0, 0, 0, 1, 32, len(png256), 22)
    (HERE / "icon.ico").write_bytes(header + entry + png256)
    print("wrote icon.ico")


if __name__ == "__main__":
    main()
