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
  "paper_texture", "paper_border", "pigment", "edge_darken",
  "focus_depth", "hard_quantile", "standard_quantile", "side_sample_prob",
];
const SELECTS = ["hard_brush", "standard_brush", "soft_brush", "color_space"];

// フォーカス位置（プレビュー上の正規化座標）。クリックで設定、ダブルクリックで解除
let focusPoint = null;
// ニューラル深度モデルのロード状態と外部デプス PNG のパス
let depthModelLoaded = false;
let externalDepthPath = null;

// 各パラメータの説明（ラベルのツールチップ）
const PARAM_HELP = {
  pixels: "形をどれだけ大づかみに捉えるか。小さいほど単純化される",
  resolution: "描画キャンバスの解像度。小さいほど抽象的・タッチが大きい",
  palette: "減色後の色数",
  posterize_blur: "減色前のぼかし。大きいと色面が滑らかに繋がる",
  normal_blur: "方向場の平滑さ。大きいとストロークが大きくうねる",
  brush_size: "基準ブラシ半径（キャンバス px）",
  strokes_scale: "ストローク本数の倍率",
  wet: "筆を置くとき下の色と混ざる比率（ウェットブレンディング）",
  saturation: "彩度の倍率",
  out_long: "保存画像の長辺ピクセル数",
  depth_detail: "奥行きによるタッチ粗密の強さ（0 で無効）",
  focus_range: "焦点から細かさが保たれる深度範囲。小さいほど被写界深度が浅い",
  detail_min: "フォーカス範囲外（ボケ側）の粗さ下限",
  detail_max: "焦点付近の細かさ上限（1 超でさらに細密）",
  line_strength: "鉛筆下書き風の輪郭線の濃さ（0 で無効）",
  line_width: "輪郭線の太さ。1 未満で細線化",
  focus_depth: "プレビューでクリック指定していないときの焦点深度（0=最前面、1=最奥）",
  hard_quantile: "密度がこの分位数を超える領域にハードブラシを使う",
  standard_quantile: "密度がこの分位数を超える領域にスタンダードブラシを使う",
  side_sample_prob: "隣の色面から色を借りて混ぜるストロークの割合",
  pigment: "透明水彩のグレーズ度（0 = 油彩、1 = 紙の白が透ける水彩）",
  edge_darken: "塗りの縁に顔料が溜まる水彩特有の縁取り",
  paper_texture: "紙目の強さ（水彩時は粒状化も兼ねる）",
  paper_border: "画像外周に残す紙の白フチ（短辺比）",
};

// 既定値（HTML の初期値 = コアの Params::default() と一致させてある）
const DEFAULTS = {};

for (const name of SLIDERS) {
  const input = $(`p-${name}`);
  const span = $(`v-${name}`);
  const label = input.parentElement;
  DEFAULTS[name] = parseFloat(input.value);
  if (PARAM_HELP[name]) label.title = PARAM_HELP[name];

  const decimals = Number.isInteger(parseFloat(input.step)) ? 0
    : (String(input.step).split(".")[1] || "").length;

  // ラベル行を「名前 | 数値入力 | 単位 | 走査」の 1 行 flex に組み直す
  const head = document.createElement("div");
  head.className = "param-head";
  const pname = document.createElement("span");
  pname.className = "pname";
  let nameText = "";
  for (let node = label.firstChild; node && node !== span; ) {
    const next = node.nextSibling;
    if (node.nodeType === Node.TEXT_NODE) nameText += node.textContent;
    label.removeChild(node);
    node = next;
  }
  pname.textContent = nameText.trim();
  let unitText = "";
  for (let node = span.nextSibling; node && node !== input; ) {
    const next = node.nextSibling;
    if (node.nodeType === Node.TEXT_NODE) unitText += node.textContent;
    label.removeChild(node);
    node = next;
  }
  label.insertBefore(head, span);
  head.appendChild(pname);
  head.appendChild(span);

  // 値表示を直接入力できる数値ボックスに置き換える
  const num = document.createElement("input");
  num.type = "number";
  num.className = "val-num";
  num.min = input.min;
  num.max = input.max;
  num.step = input.step;
  span.replaceWith(num);
  if (unitText.trim()) {
    const u = document.createElement("span");
    u.className = "unit";
    u.textContent = unitText.trim();
    head.appendChild(u);
  }

  const sync = () => {
    if (document.activeElement !== num) {
      num.value = parseFloat(input.value).toFixed(decimals);
    }
    // 既定値から変更されている項目をハイライト
    label.classList.toggle("changed", Math.abs(parseFloat(input.value) - DEFAULTS[name]) > 1e-9);
  };
  input.addEventListener("input", () => {
    sync();
    scheduleAutoPreview();
  });
  sync();

  num.addEventListener("change", () => {
    let v = parseFloat(num.value);
    if (Number.isNaN(v)) v = DEFAULTS[name];
    v = Math.min(parseFloat(input.max), Math.max(parseFloat(input.min), v));
    input.value = v;
    input.dispatchEvent(new Event("input"));
    num.blur();
  });

  // ホイールで微調整（Shift で 10 ステップ）
  input.addEventListener("wheel", (e) => {
    e.preventDefault();
    const step = (parseFloat(input.step) || 1) * (e.shiftKey ? 10 : 1);
    const v = parseFloat(input.value) - Math.sign(e.deltaY) * step;
    input.value = Math.min(parseFloat(input.max), Math.max(parseFloat(input.min), v));
    input.dispatchEvent(new Event("input"));
  }, { passive: false });

  // ダブルクリックで既定値に戻す
  input.addEventListener("dblclick", () => {
    input.value = DEFAULTS[name];
    input.dispatchEvent(new Event("input"));
  });

  // 走査ボタン: このパラメータだけを段階的に変えた比較レンダリング
  const btn = document.createElement("button");
  btn.className = "sweep-btn";
  btn.textContent = "走査";
  btn.title = "このパラメータを 6 段階に変えて比較";
  btn.addEventListener("click", (e) => {
    e.preventDefault();
    startSweep(name);
  });
  head.appendChild(btn);
}

// セレクト・チェックボックス・シードの変更も自動プレビュー対象
for (const id of ["p-hard_brush", "p-standard_brush", "p-soft_brush", "p-color_space", "p-depth_invert", "p-seed", "p-process_gif"]) {
  $(id).addEventListener("change", () => scheduleAutoPreview());
}

// カスタムブラシ:「PNG を選択…」を選ぶとファイルダイアログを開き、
// パスを value に持つ option を追加して選択状態にする
for (const id of ["p-hard_brush", "p-standard_brush", "p-soft_brush"]) {
  const sel = $(id);
  let prev = sel.value;
  sel.addEventListener("change", async () => {
    if (sel.value !== "__custom__") {
      prev = sel.value;
      return;
    }
    const path = await dialog.open({
      multiple: false,
      filters: [{ name: "ブラシ先端 (グレースケール PNG)", extensions: ["png"] }],
    });
    if (typeof path !== "string") {
      sel.value = prev; // キャンセル時は元に戻す
      return;
    }
    const opt = document.createElement("option");
    opt.value = path;
    opt.textContent = `📄 ${path.split(/[\\/]/).pop()}`;
    sel.insertBefore(opt, sel.querySelector('option[value="__custom__"]'));
    sel.value = path;
    prev = path;
    scheduleAutoPreview();
  });
}

// --- ニューラル深度モデル（Depth Anything V2 / ONNX） ---
$("p-use_depth").addEventListener("change", async (e) => {
  const chk = e.target;
  if (!chk.checked) {
    scheduleAutoPreview();
    return;
  }
  if (depthModelLoaded) {
    scheduleAutoPreview();
    return;
  }
  const stateEl = $("depth-model-state");
  stateEl.textContent = "モデルを読み込み中…";
  try {
    // まず models/ の既定パスを自動検出、無ければファイル選択
    const path = await invoke("load_depth_model", { path: null }).catch(async () => {
      const picked = await dialog.open({
        multiple: false,
        filters: [{ name: "ONNX モデル", extensions: ["onnx"] }],
      });
      if (typeof picked !== "string") throw new Error("キャンセルされました");
      return await invoke("load_depth_model", { path: picked });
    });
    depthModelLoaded = true;
    stateEl.textContent = `深度モデル: ${String(path).split(/[\\/]/).pop()}`;
    scheduleAutoPreview();
  } catch (err) {
    chk.checked = false;
    stateEl.textContent = `読み込めません: ${err}`;
  }
});

// --- 外部デプス PNG（白 = 手前。指定時はモデルより優先） ---
$("btn-depth-file").addEventListener("click", async () => {
  const path = await dialog.open({
    multiple: false,
    filters: [{ name: "デプスマップ", extensions: ["png", "jpg", "jpeg"] }],
  });
  if (typeof path !== "string") return;
  externalDepthPath = path;
  $("depth-file-name").textContent = path.split(/[\\/]/).pop();
  $("btn-depth-clear").hidden = false;
  scheduleAutoPreview();
});

$("btn-depth-clear").addEventListener("click", () => {
  externalDepthPath = null;
  $("depth-file-name").textContent = "";
  $("btn-depth-clear").hidden = true;
  scheduleAutoPreview();
});

// --- 過程 GIF の保存 ---
$("btn-save-gif").addEventListener("click", async () => {
  const dest = await dialog.save({
    defaultPath: "process.gif",
    filters: [{ name: "GIF", extensions: ["gif"] }],
  });
  if (!dest) return;
  try {
    await invoke("save_process_gif", { dest });
    setStatus(`GIF を保存しました: ${dest}`);
  } catch (e) {
    setStatus(`GIF 保存失敗: ${e}`);
  }
});

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
    focus_depth: num("focus_depth"),
    hard_quantile: num("hard_quantile"),
    standard_quantile: num("standard_quantile"),
    side_sample_prob: num("side_sample_prob"),
    process_gif: $("p-process_gif").checked,
    use_depth_model: depthModelLoaded && $("p-use_depth").checked,
    external_depth_path: externalDepthPath,
    focus_range: num("focus_range"),
    detail_min: num("detail_min"),
    detail_max: num("detail_max"),
    line_strength: num("line_strength"),
    line_width: num("line_width"),
    paper_texture: num("paper_texture"),
    paper_border: num("paper_border"),
    pigment: num("pigment"),
    edge_darken: num("edge_darken"),
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

async function doRender() {
  if (!imagePath || rendering || sweeping) return;
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
}

$("btn-render").addEventListener("click", doRender);

// --- 自動プレビュー: パラメータ変更から少し待って自動レンダリング ---
let autoTimer = null;
let autoDirty = false;

function scheduleAutoPreview() {
  if (!$("auto-preview").checked || !imagePath) return;
  autoDirty = true;
  clearTimeout(autoTimer);
  autoTimer = setTimeout(() => {
    if (rendering || sweeping) return; // 完了時に autoDirty を見て再実行される
    autoDirty = false;
    doRender();
  }, 700);
}

$("auto-preview").addEventListener("change", () => {
  if ($("auto-preview").checked) scheduleAutoPreview();
});

// --- 全パラメータを既定値へ ---
$("btn-reset").addEventListener("click", () => {
  const base = presets.find((p) => p.name === "基本")?.params;
  if (base) {
    applyParams(base);
    if (presetSelect.options.length) presetSelect.value = "基本";
  } else {
    for (const name of SLIDERS) {
      const input = $(`p-${name}`);
      input.value = DEFAULTS[name];
      input.dispatchEvent(new Event("input"));
    }
  }
  setStatus("既定値に戻しました");
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
  $("btn-save-gif").disabled = !payload.gif;
  setStatus(`完成: ${payload.strokes} ストローク, ${(payload.millis / 1000).toFixed(1)} 秒`);
  // レンダリング中にパラメータが変わっていたら自動プレビューを続ける
  if (autoDirty) scheduleAutoPreview();
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
  scheduleAutoPreview();
});

preview.addEventListener("dblclick", clearFocus);

// --- パラメータ走査（スイープ） ---
const sweepPanel = $("sweep-panel");
const sweepGrid = $("sweep-grid");
const sweepTitle = $("sweep-title");
let sweeping = false;
let sweepParamName = null;

// スライダー名 → サイドバーの表示名（ボタンの親ラベルから取得）
function sliderLabelText(name) {
  const label = $(`p-${name}`).parentElement;
  return label.childNodes[0].textContent.trim();
}

async function startSweep(name) {
  if (!imagePath) {
    setStatus("先に画像を開いてください");
    return;
  }
  if (rendering || sweeping) return;
  const input = $(`p-${name}`);
  const min = parseFloat(input.min);
  const max = parseFloat(input.max);
  const step = parseFloat(input.step) || 1;
  const n = 6;
  const values = [];
  for (let i = 0; i < n; i++) {
    let v = min + ((max - min) * i) / (n - 1);
    v = Math.round(v / step) * step;
    v = parseFloat(v.toFixed(4));
    if (!values.includes(v)) values.push(v);
  }

  sweeping = true;
  sweepParamName = name;
  sweepTitle.textContent = `走査: ${sliderLabelText(name)}（クリックで採用）`;
  sweepGrid.innerHTML = "";
  for (const v of values) {
    const cell = document.createElement("div");
    cell.className = "sweep-cell pending";
    cell.textContent = `${v} …`;
    sweepGrid.appendChild(cell);
  }
  sweepPanel.hidden = false;
  $("btn-render").disabled = true;
  setStatus(`走査中: ${sliderLabelText(name)}`);
  try {
    await invoke("start_sweep", {
      path: imagePath,
      params: collectParams(),
      sweepParam: name,
      values,
    });
  } catch (e) {
    sweeping = false;
    sweepPanel.hidden = true;
    $("btn-render").disabled = false;
    setStatus(`走査を開始できません: ${e}`);
  }
}

listen("sweep_result", ({ payload }) => {
  const cell = sweepGrid.children[payload.index];
  if (!cell) return;
  cell.className = "sweep-cell";
  cell.innerHTML = "";
  const img = document.createElement("img");
  img.src = payload.data_url;
  const cap = document.createElement("div");
  cap.textContent = String(payload.value);
  cell.append(img, cap);
  cell.addEventListener("click", () => {
    const input = $(`p-${sweepParamName}`);
    input.value = payload.value;
    input.dispatchEvent(new Event("input"));
    sweepPanel.hidden = true;
    setStatus(`${sliderLabelText(sweepParamName)} = ${payload.value} を採用（レンダリングで確認）`);
  });
});

listen("sweep_done", () => {
  sweeping = false;
  $("btn-render").disabled = !imagePath;
  if (!sweepPanel.hidden) setStatus("走査完了。サムネイルをクリックで値を採用");
});

$("sweep-close").addEventListener("click", () => {
  sweepPanel.hidden = true;
});

// --- プリセット ---
const presetSelect = $("preset-select");
let presets = [];

async function loadPresets() {
  try {
    presets = await invoke("get_presets");
  } catch {
    return; // 旧バックエンドでは黙って無効化
  }
  for (const p of presets) {
    const opt = document.createElement("option");
    opt.value = p.name;
    opt.textContent = p.name;
    opt.title = p.description;
    presetSelect.appendChild(opt);
  }
  presetSelect.addEventListener("change", () => {
    const p = presets.find((x) => x.name === presetSelect.value);
    if (p) applyParams(p.params);
    presetSelect.title = p ? p.description : "";
  });
}

// プリセットの値を各コントロールへ反映（フォーカス位置は維持）
function applyParams(params) {
  for (const name of SLIDERS) {
    if (params[name] === undefined) continue;
    const input = $(`p-${name}`);
    input.value = params[name];
    input.dispatchEvent(new Event("input")); // ラベル更新
  }
  for (const name of SELECTS) {
    if (params[name] !== undefined) $(`p-${name}`).value = params[name];
  }
  $("p-depth_invert").checked = !!params.depth_invert;
  $("p-seed").value = params.seed ?? 42;
}

loadPresets();
