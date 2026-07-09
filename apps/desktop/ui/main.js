// フロントエンドロジック（withGlobalTauri 前提、バンドラ不使用）。
// バックエンドの stage / paint_progress / done / render_error イベントを購読して
// プレビューとサムネイルを更新する。

const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;
const dialog = window.__TAURI__.dialog;

const $ = (id) => document.getElementById(id);
const preview = $("preview");
const statusEl = $("status");
const progressEl = $("progress");
const stagesEl = $("stages");

// ステージ名 → 表示ラベル（表示順もこの順）
const STAGE_LABELS = {
  "1_original": "元画像",
  "2_quantized": "減色",
  "3_posterize_edges": "境界線",
  "4_gray_blur": "グレー",
  "5_normal_map": "法線",
  "6_flow_map": "フロー",
  depth_map: "深度",
  density: "密度",
  line_art: "輪郭線",
  palette_swatch: "パレット",
  color_wheel: "色環",
  "7_strokes_debug": "ストローク",
  "8_painting": "完成",
};

let imagePath = null;
let rendering = false;
let processFrames = []; // 描画過程のフレーム（リプレイ用）
let replayTimer = null;
let finalDataUrl = null;

const SLIDERS = [
  "pixels", "resolution", "palette", "posterize_blur", "normal_blur",
  "brush_size", "strokes_scale", "wet", "saturation", "depth_detail", "out_long",
  "focus_range", "detail_min", "detail_max", "line_strength", "line_width",
];

// フォーカス位置（プレビュー上の正規化座標）。クリックで設定、ダブルクリックで解除
let focusPoint = null;

for (const name of SLIDERS) {
  const input = $(`p-${name}`);
  const label = $(`v-${name}`);
  const fmt = () => {
    const v = parseFloat(input.value);
    label.textContent = Number.isInteger(parseFloat(input.step)) ? v : v.toFixed(2).replace(/0+$/, "").replace(/\.$/, ".0");
  };
  input.addEventListener("input", fmt);
  fmt();
}

function collectParams() {
  const num = (id) => parseFloat($(`p-${id}`).value);
  return {
    pixels: num("pixels") | 0,
    resolution: num("resolution") | 0,
    palette: num("palette") | 0,
    color_space: $("p-color_space").value,
    posterize_blur: num("posterize_blur"),
    normal_blur: num("normal_blur"),
    brush_size: num("brush_size"),
    hard_brush: $("p-hard_brush").value,
    standard_brush: $("p-standard_brush").value,
    soft_brush: $("p-soft_brush").value,
    strokes_scale: num("strokes_scale"),
    wet: num("wet"),
    saturation: num("saturation"),
    out_long: num("out_long") | 0,
    seed: parseInt($("p-seed").value, 10) || 0,
    depth_detail: num("depth_detail"),
    depth_invert: $("p-depth_invert").checked,
    focus_x: focusPoint ? focusPoint.x : null,
    focus_y: focusPoint ? focusPoint.y : null,
    focus_range: num("focus_range"),
    detail_min: num("detail_min"),
    detail_max: num("detail_max"),
    line_strength: num("line_strength"),
    line_width: num("line_width"),
  };
}

function setStatus(text) {
  statusEl.textContent = text;
}

function stopReplay() {
  if (replayTimer) {
    clearInterval(replayTimer);
    replayTimer = null;
  }
}

function showPreview(dataUrl) {
  preview.src = dataUrl;
  document.body.classList.add("has-image");
}

function setStageThumb(name, dataUrl) {
  let el = document.querySelector(`.stage[data-name="${name}"]`);
  if (!el) {
    el = document.createElement("div");
    el.className = "stage";
    el.dataset.name = name;
    el.innerHTML = `<img alt="${name}" /><div>${STAGE_LABELS[name] ?? name}</div>`;
    el.addEventListener("click", () => {
      stopReplay();
      document.querySelectorAll(".stage.active").forEach((s) => s.classList.remove("active"));
      el.classList.add("active");
      showPreview(el.querySelector("img").src);
    });
    // STAGE_LABELS の定義順に並べる
    const order = Object.keys(STAGE_LABELS);
    const idx = order.indexOf(name);
    const next = [...stagesEl.children].find((c) => order.indexOf(c.dataset.name) > idx);
    stagesEl.insertBefore(el, next ?? null);
  }
  el.querySelector("img").src = dataUrl;
}

async function openImage(path) {
  try {
    const info = await invoke("load_image", { path });
    imagePath = path;
    stopReplay();
    clearFocus();
    stagesEl.innerHTML = "";
    processFrames = [];
    finalDataUrl = null;
    $("btn-process").disabled = true;
    $("btn-save").disabled = true;
    showPreview(info.data_url);
    $("btn-render").disabled = false;
    setStatus(`${path.split(/[\\/]/).pop()} (${info.width}×${info.height})`);
  } catch (e) {
    setStatus(`読み込み失敗: ${e}`);
  }
}

$("btn-open").addEventListener("click", async () => {
  const path = await dialog.open({
    multiple: false,
    filters: [{ name: "画像", extensions: ["png", "jpg", "jpeg", "webp", "bmp"] }],
  });
  if (typeof path === "string") await openImage(path);
});

$("btn-render").addEventListener("click", async () => {
  if (!imagePath || rendering) return;
  rendering = true;
  stopReplay();
  processFrames = [];
  progressEl.value = 0;
  $("btn-render").disabled = true;
  $("btn-process").disabled = true;
  setStatus("レンダリング中…");
  try {
    await invoke("start_render", { path: imagePath, params: collectParams() });
  } catch (e) {
    rendering = false;
    $("btn-render").disabled = false;
    setStatus(`開始できません: ${e}`);
  }
});

$("btn-process").addEventListener("click", () => {
  if (!processFrames.length) return;
  stopReplay();
  let i = 0;
  replayTimer = setInterval(() => {
    showPreview(processFrames[i]);
    i += 1;
    if (i >= processFrames.length) {
      stopReplay();
      if (finalDataUrl) showPreview(finalDataUrl);
    }
  }, 40);
});

$("btn-save").addEventListener("click", async () => {
  const dest = await dialog.save({
    defaultPath: "painting.png",
    filters: [{ name: "PNG", extensions: ["png"] }],
  });
  if (!dest) return;
  try {
    await invoke("save_image", { dest });
    setStatus(`保存しました: ${dest}`);
  } catch (e) {
    setStatus(`保存失敗: ${e}`);
  }
});

listen("stage", ({ payload }) => {
  setStageThumb(payload.name, payload.data_url);
});

listen("paint_progress", ({ payload }) => {
  progressEl.value = payload.frac;
  processFrames.push(payload.data_url);
  showPreview(payload.data_url); // 描画の進行をリアルタイム表示
});

listen("done", ({ payload }) => {
  rendering = false;
  progressEl.value = 1;
  finalDataUrl = payload.data_url;
  showPreview(payload.data_url);
  $("btn-render").disabled = false;
  $("btn-save").disabled = false;
  $("btn-process").disabled = processFrames.length === 0;
  setStatus(`完成: ${payload.strokes} ストローク, ${(payload.millis / 1000).toFixed(1)} 秒`);
});

listen("render_error", ({ payload }) => {
  rendering = false;
  $("btn-render").disabled = false;
  setStatus(`エラー: ${payload}`);
});

// ドラッグ＆ドロップ（Tauri のネイティブイベント）
listen("tauri://drag-drop", ({ payload }) => {
  const paths = payload?.paths ?? [];
  if (paths.length && !rendering) openImage(paths[0]);
});

// --- フォーカス位置の指定（クリックで設定、ダブルクリックで解除） ---
const focusMarker = $("focus-marker");
const focusState = $("focus-state");
const wrap = $("preview-wrap");

function clearFocus() {
  focusPoint = null;
  focusMarker.style.display = "none";
  focusState.textContent = "（プレビューをクリックで焦点指定）";
}

preview.addEventListener("click", (e) => {
  const r = preview.getBoundingClientRect();
  focusPoint = {
    x: (e.clientX - r.left) / r.width,
    y: (e.clientY - r.top) / r.height,
  };
  const wr = wrap.getBoundingClientRect();
  focusMarker.style.left = `${e.clientX - wr.left}px`;
  focusMarker.style.top = `${e.clientY - wr.top}px`;
  focusMarker.style.display = "block";
  focusState.textContent = `（焦点: ${focusPoint.x.toFixed(2)}, ${focusPoint.y.toFixed(2)} — ダブルクリックで解除）`;
});

preview.addEventListener("dblclick", clearFocus);
