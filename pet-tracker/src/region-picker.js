const { invoke } = window.__TAURI__.core;

const canvas = document.getElementById("overlay");
const ctx = canvas.getContext("2d");

let start = null;
let current = null;

function resize() {
  canvas.width = window.innerWidth;
  canvas.height = window.innerHeight;
  draw();
}

function draw() {
  ctx.clearRect(0, 0, canvas.width, canvas.height);
  ctx.fillStyle = "rgba(8, 12, 9, 0.35)";
  ctx.fillRect(0, 0, canvas.width, canvas.height);
  if (!start || !current) return;
  const x = Math.min(start.x, current.x);
  const y = Math.min(start.y, current.y);
  const w = Math.abs(current.x - start.x);
  const h = Math.abs(current.y - start.y);
  ctx.clearRect(x, y, w, h);
  ctx.strokeStyle = "#c4a35a";
  ctx.lineWidth = 2;
  ctx.strokeRect(x + 0.5, y + 0.5, w, h);
}

canvas.addEventListener("mousedown", (e) => {
  start = {
    x: e.clientX,
    y: e.clientY,
    screenX: e.screenX,
    screenY: e.screenY,
  };
  current = { ...start };
  draw();
});

canvas.addEventListener("mousemove", (e) => {
  if (!start) return;
  current = {
    x: e.clientX,
    y: e.clientY,
    screenX: e.screenX,
    screenY: e.screenY,
  };
  draw();
});

async function finish() {
  if (!start || !current) return;
  const x = Math.min(start.screenX, current.screenX);
  const y = Math.min(start.screenY, current.screenY);
  const width = Math.max(1, Math.abs(current.screenX - start.screenX));
  const height = Math.max(1, Math.abs(current.screenY - start.screenY));
  start = null;
  current = null;
  try {
    await invoke("save_ocr_region", { region: { x, y, width, height } });
  } catch (err) {
    console.error(err);
  }
  await invoke("close_region_picker");
}

canvas.addEventListener("mouseup", () => {
  finish();
});

window.addEventListener("keydown", async (e) => {
  if (e.key === "Escape") {
    await invoke("close_region_picker");
  }
});

window.addEventListener("resize", resize);
resize();
