const { listen } = window.__TAURI__.event;
const { invoke } = window.__TAURI__.core;
const { getCurrentWindow, LogicalSize } = window.__TAURI__.window;

const win = getCurrentWindow();
const content = document.getElementById("popup-content");
const hudName = document.getElementById("hud-name");
const hudSub = document.getElementById("hud-sub");
const hideBtn = document.getElementById("btn-hide");

const POPUP_WIDTH = 260;

hideBtn.addEventListener("click", async () => {
  await win.hide();
});

async function fitWindow() {
  const hud = document.querySelector(".hud");
  if (!hud) return;
  const height = Math.ceil(hud.getBoundingClientRect().height) + 16;
  try {
    await win.setSize(new LogicalSize(POPUP_WIDTH, Math.max(120, height)));
  } catch (_) {
    /* size permission missing in some hosts — ignore */
  }
}

function render(result) {
  if (!result?.best || !result.pet) {
    hudName.textContent = "No match";
    hudSub.textContent = result?.cleaned_query
      ? `Heard “${result.cleaned_query}”`
      : "Could not read nameplate";
    const suggestions = (result?.suggestions || [])
      .slice(0, 3)
      .map(
        (s) =>
          `<li><button type="button" data-pet="${s.pet_id}">${escapeHtml(s.pet_name)}</button></li>`
      )
      .join("");
    content.innerHTML = suggestions
      ? `<ul class="hud-suggestions">${suggestions}</ul>`
      : `<p class="hud-idle">Try the hotkey again.</p>`;
    requestAnimationFrame(fitWindow);
    return;
  }

  const pet = result.pet;
  const prog = result.progress;
  hudName.textContent = pet.name;
  hudSub.textContent = pet.genus;

  if (prog) {
    const maxLevel = result.profile?.max_level ?? 3;
    const shownLevel = Math.min(prog.level ?? 0, maxLevel);
    content.innerHTML = `
      <div class="hud-metrics">
        <div><span class="hud-label">Level</span><strong>${shownLevel}</strong></div>
      </div>
      ${
        result.at_max
          ? `<div class="hud-next"><span class="hud-pill max">Max</span></div>`
          : ""
      }
    `;
  } else {
    content.innerHTML = `<p class="hud-idle">No progress saved yet.</p>`;
  }
  requestAnimationFrame(fitWindow);
}

function escapeHtml(s) {
  return String(s)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

content.addEventListener("click", async (e) => {
  const btn = e.target.closest("[data-pet]");
  if (!btn) return;
  const result = await invoke("lookup_pet_id", {
    petId: btn.getAttribute("data-pet"),
  });
  render(result);
});

listen("lookup-result", (event) => {
  render(event.payload?.result ?? event.payload);
});
