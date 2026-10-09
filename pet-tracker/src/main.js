const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const trackOutput = document.getElementById("track-output");
const importOutput = document.getElementById("import-output");
const settingsForm = document.getElementById("settings-form");
const settingsStatus = document.getElementById("settings-status");
const progressList = document.getElementById("progress-list");
const dataDirEl = document.getElementById("data-dir");

let catalog = { pets: [], profiles: {} };
let progress = { pets: {} };

function formatLookup(result) {
  const lines = [];
  lines.push(`OCR: ${result.raw_ocr || "(empty)"}`);
  lines.push(`Query: ${result.cleaned_query || "(empty)"}`);
  if (!result.best) {
    lines.push("No match.");
    if (result.suggestions?.length) {
      lines.push("Suggestions:");
      for (const s of result.suggestions) {
        lines.push(`  - ${s.pet_name} (${s.genus}) score=${s.score.toFixed(2)}`);
      }
    }
    return lines.join("\n");
  }
  const pet = result.pet;
  const prog = result.progress;
  lines.push(`Match: ${pet?.name} [${pet?.genus}] via "${result.best.matched_on}"`);
  if (pet?.source_level != null) lines.push(`Source mob level: ${pet.source_level}`);
  if (pet?.zone) lines.push(`Zone: ${pet.zone}`);
  if (prog) {
    lines.push(`Your pet level: ${prog.level}`);
    lines.push(`Souls: ${prog.souls}`);
  } else {
    lines.push("Your progress: not recorded (import or edit Progress)");
  }
  if (result.at_max) {
    lines.push("At max pet level for this profile.");
  } else if (result.souls_to_next != null) {
    lines.push(`Souls to next level: ${result.souls_to_next}`);
  }
  if (pet?.owned_effects) {
    lines.push("Owned effects:");
    for (const [lvl, text] of Object.entries(pet.owned_effects)) {
      lines.push(`  Lv ${lvl}: ${text}`);
    }
  }
  if (pet?.notes) lines.push(`Notes: ${pet.notes}`);
  return lines.join("\n");
}

function fillSettings(settings) {
  settingsForm.hotkey.value = settings.hotkey ?? "Ctrl+Shift+P";
  settingsForm.region_profile.value = settings.region_profile ?? "global";
  settingsForm.catalog_update_url.value = settings.catalog_update_url ?? "";
  settingsForm.use_custom_catalog.checked = !!settings.use_custom_catalog;
  settingsForm.custom_catalog_path.value = settings.custom_catalog_path ?? "";
  settingsForm.x.value = settings.ocr_region?.x ?? 0;
  settingsForm.y.value = settings.ocr_region?.y ?? 0;
  settingsForm.width.value = settings.ocr_region?.width ?? 420;
  settingsForm.height.value = settings.ocr_region?.height ?? 64;
}

function readSettingsFromForm() {
  return {
    hotkey: settingsForm.hotkey.value.trim() || "Ctrl+Shift+P",
    region_profile: settingsForm.region_profile.value,
    catalog_update_url: settingsForm.catalog_update_url.value.trim(),
    use_custom_catalog: settingsForm.use_custom_catalog.checked,
    custom_catalog_path: settingsForm.custom_catalog_path.value.trim(),
    ocr_region: {
      x: Number(settingsForm.x.value) || 0,
      y: Number(settingsForm.y.value) || 0,
      width: Number(settingsForm.width.value) || 1,
      height: Number(settingsForm.height.value) || 1,
    },
  };
}

function renderProgress() {
  const pets = catalog.pets || [];
  progressList.innerHTML = "";
  if (!pets.length) {
    progressList.textContent = "Catalog empty.";
    return;
  }
  for (const pet of pets) {
    const p = progress.pets?.[pet.id] || { level: 0, souls: 0 };
    const row = document.createElement("div");
    row.className = "progress-card";
    row.innerHTML = `
      <div>
        <strong>${pet.name}</strong>
        <div class="muted">${pet.genus}</div>
      </div>
      <label class="muted">Lv <input data-id="${pet.id}" data-field="level" type="number" min="0" value="${p.level}" /></label>
      <label class="muted">Souls <input data-id="${pet.id}" data-field="souls" type="number" min="0" value="${p.souls}" /></label>
      <button type="button" data-save="${pet.id}">Save</button>
    `;
    progressList.appendChild(row);
  }
}

async function reloadAll() {
  const [settings, cat, prog, dataDir] = await Promise.all([
    invoke("get_settings"),
    invoke("get_catalog"),
    invoke("get_progress"),
    invoke("data_dir_path"),
  ]);
  catalog = cat;
  progress = prog;
  fillSettings(settings);
  dataDirEl.textContent = dataDir;
  renderProgress();
}

document.querySelectorAll(".tab").forEach((btn) => {
  btn.addEventListener("click", () => {
    document.querySelectorAll(".tab").forEach((b) => b.classList.remove("active"));
    document.querySelectorAll(".panel").forEach((p) => p.classList.remove("active"));
    btn.classList.add("active");
    document.getElementById(`tab-${btn.dataset.tab}`).classList.add("active");
  });
});

document.getElementById("btn-calibrate").addEventListener("click", async () => {
  try {
    await invoke("open_region_picker");
  } catch (err) {
    trackOutput.textContent = String(err);
  }
});

document.getElementById("btn-test-ocr").addEventListener("click", async () => {
  trackOutput.textContent = "Capturing…";
  try {
    const result = await invoke("test_ocr_region");
    trackOutput.textContent = formatLookup(result);
  } catch (err) {
    trackOutput.textContent = String(err);
  }
});

document.getElementById("btn-hotkey-lookup").addEventListener("click", async () => {
  trackOutput.textContent = "Looking up…";
  try {
    const result = await invoke("trigger_lookup");
    trackOutput.textContent = formatLookup(result);
  } catch (err) {
    trackOutput.textContent = String(err);
  }
});

document.getElementById("btn-manual").addEventListener("click", async () => {
  const query = document.getElementById("manual-query").value.trim();
  if (!query) return;
  try {
    const result = await invoke("lookup_text", { query });
    trackOutput.textContent = formatLookup(result);
    await invoke("show_lookup_popup", { result });
  } catch (err) {
    trackOutput.textContent = String(err);
  }
});

document.getElementById("btn-import").addEventListener("click", async () => {
  try {
    const selected = await invoke("pick_image");
    if (!selected) return;
    importOutput.textContent = "Importing…";
    progress = await invoke("import_screenshot", { path: selected });
    importOutput.textContent = `Updated ${Object.keys(progress.pets || {}).length} pet progress entries.`;
    renderProgress();
  } catch (err) {
    importOutput.textContent = String(err);
  }
});

document.getElementById("btn-pick-catalog").addEventListener("click", async () => {
  const selected = await invoke("pick_json");
  if (selected) {
    settingsForm.custom_catalog_path.value = selected;
    settingsForm.use_custom_catalog.checked = true;
  }
});

settingsForm.addEventListener("submit", async (e) => {
  e.preventDefault();
  try {
    const settings = await invoke("save_settings", { settings: readSettingsFromForm() });
    fillSettings(settings);
    catalog = await invoke("get_catalog");
    renderProgress();
    settingsStatus.textContent = "Saved.";
  } catch (err) {
    settingsStatus.textContent = String(err);
  }
});

document.getElementById("btn-refresh-catalog").addEventListener("click", async () => {
  try {
    await invoke("save_settings", { settings: readSettingsFromForm() });
    catalog = await invoke("refresh_catalog");
    renderProgress();
    settingsStatus.textContent = `Catalog loaded (${catalog.pets?.length ?? 0} pets).`;
  } catch (err) {
    settingsStatus.textContent = String(err);
  }
});

progressList.addEventListener("click", async (e) => {
  const btn = e.target.closest("[data-save]");
  if (!btn) return;
  const id = btn.getAttribute("data-save");
  const levelInput = progressList.querySelector(`input[data-id="${id}"][data-field="level"]`);
  const soulsInput = progressList.querySelector(`input[data-id="${id}"][data-field="souls"]`);
  progress = await invoke("set_pet_progress", {
    petId: id,
    level: Number(levelInput.value) || 0,
    souls: Number(soulsInput.value) || 0,
  });
  renderProgress();
});

listen("lookup-error", (event) => {
  trackOutput.textContent = `Hotkey lookup failed: ${event.payload}`;
});

listen("region-saved", async (event) => {
  const settings = await invoke("get_settings");
  fillSettings(settings);
  const r = event.payload || settings.ocr_region;
  trackOutput.textContent = `OCR region saved: ${r.width}×${r.height} at (${r.x}, ${r.y})`;
});

reloadAll().catch((err) => {
  trackOutput.textContent = String(err);
});
