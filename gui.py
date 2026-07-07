"""PySide6 GUI for the flow-guided painterly renderer.

A grid of intermediate-stage previews on top, parameter sliders below,
Draw / Save Image / Show Process at the bottom right.

    python gui.py [image]
"""
import sys
import time
from pathlib import Path

import cv2
import numpy as np
from PySide6.QtCore import Qt, QThread, QTimer, Signal
from PySide6.QtGui import QImage, QPixmap
from PySide6.QtWidgets import (
    QApplication, QComboBox, QFileDialog, QGridLayout, QHBoxLayout, QLabel,
    QMainWindow, QMessageBox, QPushButton, QScrollArea, QSlider, QVBoxLayout,
    QWidget,
)

from painterly import run_pipeline

STAGES = [
    ("1_original", "Original"),
    ("2_quantized", "Quantized"),
    ("3_posterize_edges", "Posterize Edges"),
    ("4_gray_blur", "Gray Blur"),
    ("5_normal_map", "Normal Map"),
    ("6_flow_map", "Flow Map"),
    ("7_strokes_debug", "Strokes Debug"),
    ("8_painting", "Painting"),
    ("palette_swatch", "Palette"),
    ("color_wheel", "Color Wheel"),
]

BRUSHES = ["Triangle", "Flat", "Soft", "Oil", "Pastel", "Charcoal"]

DARK_QSS = """
QMainWindow, QWidget {
    background: #2b2b2b; color: #d6d6d6; font-size: 12px;
    font-family: "Segoe UI", "Yu Gothic UI", sans-serif;
}
QLabel { color: #d6d6d6; }
QLabel#caption { color: #8a8a8a; font-size: 11px; }
QLabel#tile { background: #1e1e1e; border: 1px solid #3a3a3a; }
QLabel#status { color: #9a9a9a; }
QPushButton {
    background: #3c3c3c; border: 1px solid #555; padding: 5px 14px; border-radius: 2px;
}
QPushButton:hover { background: #4a4a4a; }
QPushButton:disabled { color: #777; border-color: #444; }
QPushButton#primary { background: #444; border: 1px solid #666; font-weight: bold; }
QPushButton#link {
    background: transparent; border: none; color: #4da3ff; text-align: left; padding: 5px 2px;
}
QPushButton#link:disabled { color: #55606e; }
QComboBox {
    background: #3c3c3c; border: 1px solid #555; padding: 3px 8px; border-radius: 2px;
}
QComboBox QAbstractItemView { background: #3c3c3c; selection-background-color: #4da3ff; }
QSlider::groove:horizontal { height: 5px; background: #171717; border-radius: 2px; }
QSlider::sub-page:horizontal { background: #2d8ceb; border-radius: 2px; }
QSlider::handle:horizontal {
    width: 12px; margin: -5px 0; background: #0e0e0e;
    border: 1px solid #666; border-radius: 3px;
}
QScrollArea { border: none; }
"""


def np_to_pixmap(img: np.ndarray) -> QPixmap:
    img = np.ascontiguousarray(img)
    h, w = img.shape[:2]
    qimg = QImage(img.data, w, h, w * 3, QImage.Format_RGB888)
    return QPixmap.fromImage(qimg.copy())


class ParamSlider(QWidget):
    """Label + slider + live value readout. Real value = slider / scale."""

    def __init__(self, name, lo, hi, default, scale=1.0, fmt="{:g}", suffix=""):
        super().__init__()
        self.scale, self.fmt, self.suffix = scale, fmt, suffix
        lay = QHBoxLayout(self)
        lay.setContentsMargins(0, 0, 0, 0)
        lay.addWidget(QLabel(name))
        self.slider = QSlider(Qt.Horizontal)
        self.slider.setRange(int(lo * scale), int(hi * scale))
        self.slider.setValue(int(default * scale))
        self.slider.setFixedWidth(110)
        self.readout = QLabel()
        self.readout.setFixedWidth(48)
        self.slider.valueChanged.connect(self._update_readout)
        self._update_readout()
        lay.addWidget(self.slider)
        lay.addWidget(self.readout)

    def _update_readout(self):
        self.readout.setText(self.fmt.format(self.value()) + self.suffix)

    def value(self):
        v = self.slider.value() / self.scale
        return int(v) if self.scale == 1.0 else v


class Worker(QThread):
    stage = Signal(str, object)
    progress = Signal(float, object)
    done = Signal(dict, float)
    failed = Signal(str)

    def __init__(self, img_rgb, out_dir, params):
        super().__init__()
        self.img, self.out_dir, self.params = img_rgb, out_dir, params

    def run(self):
        try:
            t0 = time.time()
            res = run_pipeline(
                self.img, self.out_dir,
                on_stage=lambda n, i: self.stage.emit(n, i),
                on_paint_progress=lambda f, i: self.progress.emit(f, i),
                collect_frames=True,
                **self.params,
            )
            self.done.emit(res, time.time() - t0)
        except Exception as e:  # noqa: BLE001 - surface anything to the UI
            self.failed.emit(f"{type(e).__name__}: {e}")


class MainWindow(QMainWindow):
    def __init__(self):
        super().__init__()
        self.setWindowTitle("Image Processing")
        self.setAcceptDrops(True)
        self.img_rgb = None
        self.img_path = None
        self.worker = None
        self.frames = []
        self.final = None
        self._replay_timer = QTimer(self)
        self._replay_timer.timeout.connect(self._replay_step)
        self._replay_idx = 0

        central = QWidget()
        self.setCentralWidget(central)
        root = QVBoxLayout(central)

        grid_holder = QWidget()
        grid = QGridLayout(grid_holder)
        grid.setSpacing(10)
        self.tiles = {}
        for i, (key, title) in enumerate(STAGES):
            cell = QVBoxLayout()
            tile = QLabel()
            tile.setObjectName("tile")
            tile.setFixedSize(270, 185)
            tile.setAlignment(Qt.AlignCenter)
            caption = QLabel(title)
            caption.setObjectName("caption")
            caption.setAlignment(Qt.AlignCenter)
            cell.addWidget(tile)
            cell.addWidget(caption)
            w = QWidget()
            w.setLayout(cell)
            grid.addWidget(w, i // 4, i % 4)
            self.tiles[key] = tile
        scroll = QScrollArea()
        scroll.setWidgetResizable(True)
        scroll.setWidget(grid_holder)
        root.addWidget(scroll, stretch=1)

        self.p_pixels = ParamSlider("Pixels", 40, 300, 96, fmt="{:d}", suffix=" px")
        self.p_palette = ParamSlider("Palette", 8, 128, 36, fmt="{:d}")
        self.p_resolution = ParamSlider("Resolution", 120, 720, 360, fmt="{:d}", suffix=" px")
        self.p_poster_blur = ParamSlider("Posterize Blur", 0, 8, 2, scale=10, fmt="{:.1f}")
        self.p_normal_blur = ParamSlider("Normal Blur", 0.5, 20, 8, scale=10, fmt="{:.1f}")
        self.p_brush_size = ParamSlider("Brush Size", 4, 40, 15, fmt="{:d}", suffix=" px")
        self.p_strokes = ParamSlider("Strokes", 0.3, 3.0, 1.0, scale=100, fmt="{:.1f}", suffix="x")
        self.p_wet = ParamSlider("Wet", 0.0, 0.6, 0.18, scale=100, fmt="{:.2f}")

        self.c_wheel = self._combo(["RGB", "Lab"], 0)
        self.c_hard = self._combo(BRUSHES, BRUSHES.index("Triangle"))
        self.c_standard = self._combo(BRUSHES, BRUSHES.index("Flat"))
        self.c_soft = self._combo(BRUSHES, BRUSHES.index("Soft"))

        params = QGridLayout()
        params.setHorizontalSpacing(24)
        params.addWidget(self.p_pixels, 0, 0)
        params.addWidget(self.p_palette, 0, 1)
        params.addLayout(self._labeled("Color Wheel", self.c_wheel), 0, 2)
        params.addWidget(self.p_resolution, 0, 3)
        params.addWidget(self.p_poster_blur, 1, 0)
        params.addWidget(self.p_normal_blur, 1, 1)
        params.addLayout(self._labeled("Hard Brush", self.c_hard), 1, 2)
        params.addLayout(self._labeled("Standard Brush", self.c_standard), 1, 3)
        params.addLayout(self._labeled("Soft Brush", self.c_soft), 2, 0)
        params.addWidget(self.p_brush_size, 2, 1)
        params.addWidget(self.p_strokes, 2, 2)
        params.addWidget(self.p_wet, 2, 3)
        root.addLayout(params)

        actions = QHBoxLayout()
        self.btn_open = QPushButton("Open Image…")
        self.btn_open.clicked.connect(self.open_image)
        self.btn_process = QPushButton("Show Process")
        self.btn_process.setObjectName("link")
        self.btn_process.setEnabled(False)
        self.btn_process.clicked.connect(self.show_process)
        self.btn_save = QPushButton("Save Image")
        self.btn_save.setEnabled(False)
        self.btn_save.clicked.connect(self.save_image)
        self.btn_draw = QPushButton("Draw")
        self.btn_draw.setObjectName("primary")
        self.btn_draw.clicked.connect(self.start_draw)
        self.status = QLabel("Open an image (or drop one here) and press Draw.")
        self.status.setObjectName("status")
        actions.addWidget(self.btn_open)
        actions.addWidget(self.status, stretch=1)
        actions.addWidget(self.btn_process)
        actions.addWidget(self.btn_save)
        actions.addWidget(self.btn_draw)
        root.addLayout(actions)

        self.resize(1220, 820)

    @staticmethod
    def _combo(items, default_idx):
        c = QComboBox()
        c.addItems(items)
        c.setCurrentIndex(default_idx)
        return c

    @staticmethod
    def _labeled(text, widget):
        lay = QHBoxLayout()
        lay.setContentsMargins(0, 0, 0, 0)
        lay.addWidget(QLabel(text))
        lay.addWidget(widget)
        lay.addStretch()
        return lay

    def _set_tile(self, key, img):
        tile = self.tiles[key]
        pm = np_to_pixmap(img).scaled(
            tile.size(), Qt.KeepAspectRatio, Qt.SmoothTransformation)
        tile.setPixmap(pm)

    def open_image(self):
        path, _ = QFileDialog.getOpenFileName(
            self, "Open Image", "input", "Images (*.png *.jpg *.jpeg *.bmp *.webp)")
        if path:
            self.load_image(path)

    def load_image(self, path):
        bgr = cv2.imread(str(path))
        if bgr is None:
            QMessageBox.warning(self, "Image Processing", f"Cannot read image:\n{path}")
            return
        self.img_rgb = cv2.cvtColor(bgr, cv2.COLOR_BGR2RGB)
        self.img_path = Path(path)
        for key, _ in STAGES:
            self.tiles[key].clear()
        self._set_tile("1_original", self.img_rgb)
        self.frames, self.final = [], None
        self.btn_process.setEnabled(False)
        self.btn_save.setEnabled(False)
        self.status.setText(f"Loaded {self.img_path.name} "
                            f"({self.img_rgb.shape[1]}x{self.img_rgb.shape[0]}). Press Draw.")

    def dragEnterEvent(self, e):
        if e.mimeData().hasUrls():
            e.acceptProposedAction()

    def dropEvent(self, e):
        urls = e.mimeData().urls()
        if urls:
            self.load_image(urls[0].toLocalFile())

    def gather_params(self):
        return dict(
            pixels=self.p_pixels.value(),
            resolution=self.p_resolution.value(),
            palette=self.p_palette.value(),
            color_space=self.c_wheel.currentText().lower(),
            posterize_blur=self.p_poster_blur.value(),
            normal_blur=self.p_normal_blur.value(),
            brush_size=self.p_brush_size.value(),
            hard_brush=self.c_hard.currentText().lower(),
            standard_brush=self.c_standard.currentText().lower(),
            soft_brush=self.c_soft.currentText().lower(),
            strokes_scale=self.p_strokes.value(),
            wet=self.p_wet.value(),
        )

    def start_draw(self):
        if self.img_rgb is None:
            self.open_image()
            if self.img_rgb is None:
                return
        self._replay_timer.stop()
        self.btn_draw.setEnabled(False)
        self.btn_process.setEnabled(False)
        self.btn_save.setEnabled(False)
        self.status.setText("Analyzing…")
        out_dir = Path("output") / self.img_path.stem
        self.worker = Worker(self.img_rgb.copy(), out_dir, self.gather_params())
        self.worker.stage.connect(self.on_stage)
        self.worker.progress.connect(self.on_progress)
        self.worker.done.connect(self.on_done)
        self.worker.failed.connect(self.on_failed)
        self.worker.start()

    def on_stage(self, name, img):
        if name in self.tiles:
            self._set_tile(name, img)

    def on_progress(self, frac, canvas):
        self._set_tile("8_painting", canvas)
        self.status.setText(f"Painting… {frac * 100:.0f}%")

    def on_done(self, res, elapsed):
        self.frames = res.get("frames") or []
        self.final = res.get("final")
        self.btn_draw.setEnabled(True)
        self.btn_process.setEnabled(bool(self.frames))
        self.btn_save.setEnabled(self.final is not None)
        self.status.setText(
            f"Done — {res['strokes']} strokes in {elapsed:.1f}s. Saved to {res['out']}\\")

    def on_failed(self, msg):
        self.btn_draw.setEnabled(True)
        self.status.setText("Failed.")
        QMessageBox.critical(self, "Image Processing", msg)

    def show_process(self):
        if not self.frames:
            return
        self._replay_idx = 0
        self.btn_process.setEnabled(False)
        self._replay_timer.start(33)

    def _replay_step(self):
        if self._replay_idx < len(self.frames):
            self._set_tile("8_painting", self.frames[self._replay_idx])
            self._replay_idx += 1
        else:
            self._replay_timer.stop()
            if self.final is not None:
                self._set_tile("8_painting", self.final)
            self.btn_process.setEnabled(True)

    def save_image(self):
        if self.final is None:
            return
        default = str(Path("output") / f"{self.img_path.stem}_painting.png")
        path, _ = QFileDialog.getSaveFileName(
            self, "Save Image", default, "PNG (*.png);;JPEG (*.jpg)")
        if path:
            cv2.imwrite(path, cv2.cvtColor(self.final, cv2.COLOR_RGB2BGR))
            self.status.setText(f"Saved {path}")


def main():
    app = QApplication(sys.argv)
    app.setStyleSheet(DARK_QSS)
    win = MainWindow()
    if len(sys.argv) > 1:
        win.load_image(sys.argv[1])
    win.show()
    sys.exit(app.exec())


if __name__ == "__main__":
    main()
