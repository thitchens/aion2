const { invoke } = window.__TAURI__.core;

const canvas = document.getElementById("overlay");
const ctx = canvas.getContext("2d");
const dimsEl = document.getElementById("dims");
const barEl = document.getElementById("picker-bar");

const MIN_W = 16;
const MIN_H = 12;
const HANDLE = 8;
const HANDLE_HIT = 12;

/** @typedef {{ x: number, y: number, w: number, h: number }} Rect */
/** @typedef {'nw'|'n'|'ne'|'e'|'se'|'s'|'sw'|'w'|'move'|'draw'|null} Hit */

/**
 * Region in picker logical/CSS client pixels. Rust converts to physical screen
 * coords via the picker window's inner position + scale factor (not browser screenX).
 * @type {Rect | null}
 */
let clientRect = null;

/** @type {{ origin_x: number, origin_y: number, scale: number } | null} */
let coordSpace = null;

/** @type {{ mode: Hit, start: {x:number,y:number}, orig: Rect } | null} */
let drag = null;
let redrawArmed = false;

const HANDLE_CURSORS = {
  nw: "nwse-resize",
  n: "ns-resize",
  ne: "nesw-resize",
  e: "ew-resize",
  se: "nwse-resize",
  s: "ns-resize",
  sw: "nesw-resize",
  w: "ew-resize",
  move: "move",
  draw: "crosshair",
};

function clampClientRect(r) {
  const maxW = canvas.width;
  const maxH = canvas.height;
  let { x, y, w, h } = r;
  w = Math.max(MIN_W, Math.min(w, maxW));
  h = Math.max(MIN_H, Math.min(h, maxH));
  x = Math.max(0, Math.min(x, maxW - w));
  y = Math.max(0, Math.min(y, maxH - h));
  return { x, y, w, h };
}

function toClientPayload(r) {
  return {
    x: Math.round(r.x),
    y: Math.round(r.y),
    width: Math.max(1, Math.round(r.w)),
    height: Math.max(1, Math.round(r.h)),
  };
}

function toPhysicalPreview(r) {
  if (!coordSpace) return toClientPayload(r);
  const s = coordSpace.scale || 1;
  return {
    x: Math.round(coordSpace.origin_x + r.x * s),
    y: Math.round(coordSpace.origin_y + r.y * s),
    width: Math.max(1, Math.round(r.w * s)),
    height: Math.max(1, Math.round(r.h * s)),
  };
}

function updateDims() {
  if (!clientRect) {
    dimsEl.textContent = "No region — drag to draw";
    return;
  }
  const p = toPhysicalPreview(clientRect);
  dimsEl.textContent = `${p.width} × ${p.height}  @  ${p.x}, ${p.y}`;
}

function handlePoints(r) {
  const x0 = r.x;
  const y0 = r.y;
  const x1 = r.x + r.w;
  const y1 = r.y + r.h;
  const mx = r.x + r.w / 2;
  const my = r.y + r.h / 2;
  return {
    nw: { x: x0, y: y0 },
    n: { x: mx, y: y0 },
    ne: { x: x1, y: y0 },
    e: { x: x1, y: my },
    se: { x: x1, y: y1 },
    s: { x: mx, y: y1 },
    sw: { x: x0, y: y1 },
    w: { x: x0, y: my },
  };
}

/** @returns {Hit} */
function hitTest(clientX, clientY) {
  if (!clientRect || redrawArmed) return "draw";
  const handles = handlePoints(clientRect);
  for (const [name, p] of Object.entries(handles)) {
    if (
      Math.abs(clientX - p.x) <= HANDLE_HIT &&
      Math.abs(clientY - p.y) <= HANDLE_HIT
    ) {
      return /** @type {Hit} */ (name);
    }
  }
  if (
    clientX >= clientRect.x &&
    clientX <= clientRect.x + clientRect.w &&
    clientY >= clientRect.y &&
    clientY <= clientRect.y + clientRect.h
  ) {
    return "move";
  }
  return "draw";
}

function draw() {
  ctx.clearRect(0, 0, canvas.width, canvas.height);
  ctx.fillStyle = "rgba(8, 12, 9, 0.28)";
  ctx.fillRect(0, 0, canvas.width, canvas.height);

  if (!clientRect) {
    updateDims();
    return;
  }

  const { x, y, w, h } = clientRect;
  ctx.clearRect(x, y, w, h);

  ctx.strokeStyle = "#c4a35a";
  ctx.lineWidth = 2;
  ctx.strokeRect(x + 0.5, y + 0.5, w, h);

  ctx.strokeStyle = "rgba(196, 163, 90, 0.35)";
  ctx.lineWidth = 1;
  ctx.setLineDash([4, 4]);
  ctx.strokeRect(x + 3.5, y + 3.5, Math.max(0, w - 6), Math.max(0, h - 6));
  ctx.setLineDash([]);

  ctx.fillStyle = "#c4a35a";
  for (const p of Object.values(handlePoints(clientRect))) {
    ctx.fillRect(p.x - HANDLE / 2, p.y - HANDLE / 2, HANDLE, HANDLE);
  }

  updateDims();
}

function resizeCanvas() {
  canvas.width = window.innerWidth;
  canvas.height = window.innerHeight;
  if (clientRect) clientRect = clampClientRect(clientRect);
  draw();
}

function applyResize(mode, orig, dx, dy) {
  let { x, y, w, h } = orig;
  const right = x + w;
  const bottom = y + h;

  if (mode === "w" || mode === "nw" || mode === "sw") {
    x = Math.min(orig.x + dx, right - MIN_W);
    w = right - x;
  }
  if (mode === "e" || mode === "ne" || mode === "se") {
    w = Math.max(MIN_W, orig.w + dx);
  }
  if (mode === "n" || mode === "nw" || mode === "ne") {
    y = Math.min(orig.y + dy, bottom - MIN_H);
    h = bottom - y;
  }
  if (mode === "s" || mode === "sw" || mode === "se") {
    h = Math.max(MIN_H, orig.h + dy);
  }
  return clampClientRect({ x, y, w, h });
}

function setCursor(hit) {
  canvas.style.cursor = HANDLE_CURSORS[hit || "draw"] || "crosshair";
}

canvas.addEventListener("mousedown", (e) => {
  if (e.button !== 0) return;
  const hit = hitTest(e.clientX, e.clientY);
  const start = { x: e.clientX, y: e.clientY };

  if (hit === "draw") {
    redrawArmed = false;
    drag = {
      mode: "draw",
      start,
      orig: { x: start.x, y: start.y, w: 0, h: 0 },
    };
    clientRect = null;
    draw();
    return;
  }

  drag = {
    mode: hit,
    start,
    orig: { ...clientRect },
  };
});

canvas.addEventListener("mousemove", (e) => {
  if (!drag) {
    setCursor(hitTest(e.clientX, e.clientY));
    return;
  }

  const dx = e.clientX - drag.start.x;
  const dy = e.clientY - drag.start.y;

  if (drag.mode === "draw") {
    const x0 = drag.start.x;
    const y0 = drag.start.y;
    clientRect = {
      x: Math.min(x0, e.clientX),
      y: Math.min(y0, e.clientY),
      w: Math.max(1, Math.abs(e.clientX - x0)),
      h: Math.max(1, Math.abs(e.clientY - y0)),
    };
  } else if (drag.mode === "move") {
    clientRect = clampClientRect({
      x: drag.orig.x + dx,
      y: drag.orig.y + dy,
      w: drag.orig.w,
      h: drag.orig.h,
    });
  } else {
    clientRect = applyResize(drag.mode, drag.orig, dx, dy);
  }

  setCursor(drag.mode);
  draw();
});

function endDrag() {
  if (!drag) return;
  if (drag.mode === "draw" && clientRect) {
    clientRect = clampClientRect(clientRect);
  }
  drag = null;
  redrawArmed = false;
  draw();
}

canvas.addEventListener("mouseup", endDrag);
window.addEventListener("mouseup", endDrag);

async function save() {
  if (!clientRect) return;
  clientRect = clampClientRect(clientRect);
  // Refresh space right before save so position/scale match the live window.
  try {
    coordSpace = await invoke("get_picker_coord_space");
  } catch (err) {
    console.error(err);
  }
  const region = toClientPayload(clientRect);
  try {
    await invoke("save_ocr_region", { region });
  } catch (err) {
    console.error(err);
    return;
  }
  await invoke("close_region_picker");
}

async function cancel() {
  await invoke("close_region_picker");
}

document.getElementById("btn-save").addEventListener("click", () => {
  save();
});
document.getElementById("btn-cancel").addEventListener("click", () => {
  cancel();
});
document.getElementById("btn-redraw").addEventListener("click", () => {
  redrawArmed = true;
  clientRect = null;
  setCursor("draw");
  draw();
});

/** @type {{ startX: number, startY: number, origLeft: number, origTop: number } | null} */
let barDrag = null;

function placeBar(left, top) {
  const margin = 8;
  const rect = barEl.getBoundingClientRect();
  const maxLeft = Math.max(margin, window.innerWidth - rect.width - margin);
  const maxTop = Math.max(margin, window.innerHeight - rect.height - margin);
  const x = Math.min(maxLeft, Math.max(margin, left));
  const y = Math.min(maxTop, Math.max(margin, top));
  barEl.classList.add("picker-bar-placed");
  barEl.style.left = `${x}px`;
  barEl.style.top = `${y}px`;
}

function ensureBarPixelPosition() {
  if (barEl.classList.contains("picker-bar-placed")) {
    return barEl.getBoundingClientRect();
  }
  const rect = barEl.getBoundingClientRect();
  placeBar(rect.left, rect.top);
  return barEl.getBoundingClientRect();
}

function isBarDragTarget(target) {
  if (!(target instanceof Element)) return false;
  if (target.closest("button")) return false;
  return !!target.closest("#picker-bar");
}

barEl.addEventListener("mousedown", (e) => {
  e.stopPropagation();
  if (e.button !== 0 || !isBarDragTarget(e.target)) return;
  const rect = ensureBarPixelPosition();
  barDrag = {
    startX: e.clientX,
    startY: e.clientY,
    origLeft: rect.left,
    origTop: rect.top,
  };
  barEl.classList.add("picker-bar-dragging");
  e.preventDefault();
});

window.addEventListener("mousemove", (e) => {
  if (!barDrag) return;
  placeBar(
    barDrag.origLeft + (e.clientX - barDrag.startX),
    barDrag.origTop + (e.clientY - barDrag.startY)
  );
});

window.addEventListener("mouseup", () => {
  if (!barDrag) return;
  barDrag = null;
  barEl.classList.remove("picker-bar-dragging");
});

window.addEventListener("keydown", async (e) => {
  if (e.key === "Escape") {
    e.preventDefault();
    await cancel();
  } else if (e.key === "Enter") {
    e.preventDefault();
    await save();
  }
});

window.addEventListener("resize", () => {
  resizeCanvas();
  if (barEl.classList.contains("picker-bar-placed")) {
    const rect = barEl.getBoundingClientRect();
    placeBar(rect.left, rect.top);
  }
});

async function init() {
  resizeCanvas();
  try {
    coordSpace = await invoke("get_picker_coord_space");
    const kind = await invoke("get_picker_region_kind");
    const hint = document.querySelector(".picker-hint");
    if (hint) {
      hint.textContent =
        kind === "insight"
          ? "Pet Insight: cover the pet name and the Lv. N (…) line · drag toolbar to move it"
          : "Nameplate: cover the target name · drag toolbar to move it · box to move · handles to resize";
    }
    const existing = await invoke("get_picker_ocr_region");
    if (existing && existing.width > 0 && existing.height > 0) {
      clientRect = clampClientRect({
        x: existing.x,
        y: existing.y,
        w: existing.width,
        h: existing.height,
      });
    }
  } catch (err) {
    console.error(err);
  }
  draw();
}

init();
