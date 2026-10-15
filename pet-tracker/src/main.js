const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const trackOutput = document.getElementById("track-output");
const importOutput = document.getElementById("import-output");
const settingsForm = document.getElementById("settings-form");
const settingsStatus = document.getElementById("settings-status");
const progressList = document.getElementById("progress-list");
const dataDirEl = document.getElementById("data-dir");

let catalog = { pets: [], profiles: {}, genera: [] };
let progress = { pets: {} };
let settings = {};
let activeGenus = "all";

function formatTimings(timings) {
  if (!timings) return [];
  const t = timings;
  const lines = ["", "OCR timings:"];
  lines.push(`  capture: ${t.capture_ms}ms`);
  lines.push(`  decode: ${t.decode_ms}ms`);
  lines.push(`  preprocess: ${t.preprocess_ms}ms`);
  lines.push(`  tesseract: ${t.tesseract_ms}ms`);
  lines.push(`  score: ${t.score_ms}ms`);
  lines.push(`  total: ${t.total_ms}ms`);
  if (t.attempts?.length) {
    lines.push("  attempts:");
    for (const a of t.attempts) {
      const score = a.score < 0 ? "ERR" : a.score.toFixed(3);
      lines.push(
        `    ${a.variant} psm${a.psm} wl=${a.whitelist} x${a.upscale || "?"}: ${a.tesseract_ms}ms score=${score}`
      );
    }
  }
  return lines;
}

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
    lines.push(...formatTimings(result.timings));
    return lines.join("\n");
  }
  const pet = result.pet;
  const prog = result.progress;
  lines.push(`Match: ${pet?.name} [${pet?.genus}] via "${result.best.matched_on}"`);
  if (pet?.source_level != null) lines.push(`Source mob level: ${pet.source_level}`);
  if (pet?.zone) lines.push(`Zone: ${pet.zone}`);
  if (prog) {
    const maxLevel = result.profile?.max_level ?? profileMaxLevel();
    const shownLevel = Math.min(prog.level ?? 0, maxLevel);
    lines.push(`Your pet level: ${shownLevel}`);
    if ((prog.level ?? 0) > maxLevel) {
      lines.push(`(Stored Lv ${prog.level} for a higher-max region)`);
    }
  } else {
    lines.push("Your progress: not recorded (import or edit Progress)");
  }
  if (result.at_max) {
    lines.push("At max pet level for this profile.");
  }
  if (pet?.owned_effects) {
    lines.push("Owned effects:");
    for (const [lvl, text] of Object.entries(pet.owned_effects)) {
      lines.push(`  Lv ${lvl}: ${text}`);
    }
  }
  if (pet?.notes) lines.push(`Notes: ${pet.notes}`);
  lines.push(...formatTimings(result.timings));
  return lines.join("\n");
}

function fillSettings(settings) {
  settingsForm.hotkey.value = settings.hotkey ?? "Ctrl+Shift+P";
  settingsForm.hotkey_import.value = settings.hotkey_import ?? "Ctrl+Shift+I";
  settingsForm.region_profile.value = settings.region_profile ?? "global";
  settingsForm.catalog_update_url.value = settings.catalog_update_url ?? "";
  settingsForm.use_custom_catalog.checked = !!settings.use_custom_catalog;
  settingsForm.ocr_debug_timings.checked = !!settings.ocr_debug_timings;
  settingsForm.ocr_debug_images.checked = !!settings.ocr_debug_images;
  settingsForm.custom_catalog_path.value = settings.custom_catalog_path ?? "";
  settingsForm.x.value = settings.ocr_region?.x ?? 0;
  settingsForm.y.value = settings.ocr_region?.y ?? 0;
  settingsForm.width.value = settings.ocr_region?.width ?? 420;
  settingsForm.height.value = settings.ocr_region?.height ?? 64;
  settingsForm.insight_x.value = settings.insight_region?.x ?? 0;
  settingsForm.insight_y.value = settings.insight_region?.y ?? 0;
  settingsForm.insight_width.value = settings.insight_region?.width ?? 520;
  settingsForm.insight_height.value = settings.insight_region?.height ?? 140;
}

function readSettingsFromForm() {
  return {
    hotkey: settingsForm.hotkey.value.trim() || "Ctrl+Shift+P",
    hotkey_import: settingsForm.hotkey_import.value.trim() || "Ctrl+Shift+I",
    region_profile: settingsForm.region_profile.value,
    catalog_update_url: settingsForm.catalog_update_url.value.trim(),
    use_custom_catalog: settingsForm.use_custom_catalog.checked,
    ocr_debug_timings: settingsForm.ocr_debug_timings.checked,
    ocr_debug_images: settingsForm.ocr_debug_images.checked,
    custom_catalog_path: settingsForm.custom_catalog_path.value.trim(),
    ocr_region: {
      x: Number(settingsForm.x.value) || 0,
      y: Number(settingsForm.y.value) || 0,
      width: Number(settingsForm.width.value) || 1,
      height: Number(settingsForm.height.value) || 1,
    },
    insight_region: {
      x: Number(settingsForm.insight_x.value) || 0,
      y: Number(settingsForm.insight_y.value) || 0,
      width: Number(settingsForm.insight_width.value) || 1,
      height: Number(settingsForm.insight_height.value) || 1,
    },
  };
}

function genusList() {
  if (catalog.genera?.length) {
    return catalog.genera.map((g) => g.name);
  }
  return [...new Set((catalog.pets || []).map((p) => p.genus).filter(Boolean))].sort();
}

function profileMaxLevel() {
  const key = settings.region_profile || "global";
  return catalog.profiles?.[key]?.max_level ?? 3;
}

function petProgress(petId) {
  return progress.pets?.[petId] || { level: 0 };
}

function isOwned(p) {
  return (p.level || 0) > 0;
}

function escapeHtml(s) {
  return String(s)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

function findPet(petId) {
  return (catalog.pets || []).find((p) => p.id === petId) || null;
}

function openPetDetail(petId) {
  const dialog = document.getElementById("pet-detail");
  const title = document.getElementById("pet-detail-title");
  const sub = document.getElementById("pet-detail-sub");
  const body = document.getElementById("pet-detail-body");
  if (!dialog || !title || !sub || !body) return;

  const pet = findPet(petId);
  if (!pet) return;

  const p = petProgress(pet.id);
  const maxLevel = profileMaxLevel();
  const shownLevel = Math.min(p.level || 0, maxLevel);
  const zones = zoneTokens(pet.zone);
  const mobs = pet.source_mobs || [];
  const spawnAreas = pet.spawn_areas || [];

  title.textContent = pet.name;
  sub.textContent = pet.genus || "";

  const sections = [];
  sections.push(`
    <section class="pet-detail-section">
      <h4>Your level</h4>
      <p>${shownLevel}${
        (p.level || 0) > maxLevel ? ` <span class="muted">(stored Lv ${p.level})</span>` : ""
      } / ${maxLevel}${isOwned(p) ? "" : ' <span class="muted">· unowned</span>'}</p>
    </section>
  `);

  if (spawnAreas.length) {
    const areaRows = spawnAreas
      .map((sa) => {
        const mobNote =
          sa.source_mobs?.length
            ? ` <span class="muted">(${sa.source_mobs.map(escapeHtml).join(", ")})</span>`
            : "";
        return `<li><strong>${escapeHtml(sa.area)}</strong> · ${escapeHtml(sa.zone)}${mobNote}</li>`;
      })
      .join("");
    sections.push(`
      <section class="pet-detail-section">
        <h4>Spawn areas</h4>
        <ul>${areaRows}</ul>
      </section>
    `);
  } else {
    sections.push(`
      <section class="pet-detail-section">
        <h4>Zones</h4>
        ${
          zones.length
            ? `<ul>${zones.map((z) => `<li>${escapeHtml(z)}</li>`).join("")}</ul>`
            : `<p class="muted">No spawn area listed</p>`
        }
      </section>
    `);
  }

  sections.push(`
    <section class="pet-detail-section">
      <h4>Source mobs</h4>
      ${
        mobs.length
          ? `<ul>${mobs.map((m) => `<li>${escapeHtml(m)}</li>`).join("")}</ul>`
          : `<p class="muted">No source mobs listed</p>`
      }
      ${
        pet.source_level != null
          ? `<p class="muted">Mob level ${pet.source_level}</p>`
          : ""
      }
    </section>
  `);

  if (pet.owned_effects && Object.keys(pet.owned_effects).length) {
    const effectRows = Object.entries(pet.owned_effects)
      .sort(([a], [b]) => Number(a) - Number(b))
      .map(
        ([lvl, text]) =>
          `<li><span class="muted">Lv ${escapeHtml(lvl)}:</span> ${escapeHtml(text)}</li>`
      )
      .join("");
    sections.push(`
      <section class="pet-detail-section">
        <h4>Owned effects</h4>
        <ul>${effectRows}</ul>
      </section>
    `);
  }

  if (pet.notes) {
    sections.push(`
      <section class="pet-detail-section">
        <h4>Notes</h4>
        <p>${escapeHtml(pet.notes)}</p>
      </section>
    `);
  }

  body.innerHTML = sections.join("");
  if (typeof dialog.showModal === "function") {
    dialog.showModal();
  } else {
    dialog.setAttribute("open", "");
  }
}

/** Split messy zone strings into place-name tokens (drops drop-rate noise). */
function zoneTokens(zone) {
  if (!zone) return [];
  return zone
    .split(",")
    .map((part) => part.trim())
    .filter((part) => part && !part.includes("%") && !/^100\b/i.test(part));
}

function zoneOptions() {
  const counts = new Map();
  for (const pet of catalog.pets || []) {
    for (const token of zoneTokens(pet.zone)) {
      counts.set(token, (counts.get(token) || 0) + 1);
    }
  }
  return [...counts.entries()].sort((a, b) => a[0].localeCompare(b[0]));
}

function renderGenusFilters() {
  const el = document.getElementById("genus-filters");
  if (!el) return;
  const genera = genusList();
  const buttons = [
    { id: "all", label: `All (${catalog.pets?.length || 0})` },
    ...genera.map((name) => {
      const count =
        catalog.genera?.find((g) => g.name === name)?.pet_count ??
        (catalog.pets || []).filter((p) => p.genus === name).length;
      return { id: name, label: `${name} (${count})` };
    }),
  ];
  el.innerHTML = buttons
    .map(
      (b) =>
        `<button type="button" class="genus-chip${activeGenus === b.id ? " active" : ""}" data-genus="${b.id}">${b.label}</button>`
    )
    .join("");
}

function renderZoneFilter() {
  const el = document.getElementById("progress-zone");
  if (!el) return;
  const selected = el.value || "all";
  const options = [
    `<option value="all">All zones</option>`,
    `<option value="none">No zone listed</option>`,
    ...zoneOptions().map(
      ([name, count]) =>
        `<option value="${name.replace(/"/g, "&quot;")}">${name} (${count})</option>`
    ),
  ];
  el.innerHTML = options.join("");
  if ([...el.options].some((o) => o.value === selected)) {
    el.value = selected;
  } else {
    el.value = "all";
  }
}

function matchesOwnership(pet) {
  const ownership = document.getElementById("progress-ownership")?.value || "all";
  if (ownership === "all") return true;
  const p = petProgress(pet.id);
  const owned = isOwned(p);
  const maxLevel = profileMaxLevel();
  switch (ownership) {
    case "owned":
      return owned;
    case "unowned":
      return !owned;
    case "in-progress":
      return owned && (p.level || 0) < maxLevel;
    case "maxed":
      return (p.level || 0) >= maxLevel;
    default:
      return true;
  }
}

function matchesZone(pet) {
  const zone = document.getElementById("progress-zone")?.value || "all";
  if (zone === "all") return true;
  const tokens = zoneTokens(pet.zone);
  if (zone === "none") return tokens.length === 0;
  return tokens.some((t) => t === zone);
}

function matchesPetLevel(pet) {
  const minRaw = document.getElementById("progress-level-min")?.value;
  const maxRaw = document.getElementById("progress-level-max")?.value;
  const min = minRaw === "" || minRaw == null ? null : Number(minRaw);
  const max = maxRaw === "" || maxRaw == null ? null : Number(maxRaw);
  if (min == null && max == null) return true;
  const level = petProgress(pet.id).level || 0;
  if (min != null && !Number.isNaN(min) && level < min) return false;
  if (max != null && !Number.isNaN(max) && level > max) return false;
  return true;
}

function comparePets(a, b, sort) {
  const pa = petProgress(a.id);
  const pb = petProgress(b.id);
  const nameCmp = () => a.name.localeCompare(b.name);
  const genusCmp = () => (a.genus || "").localeCompare(b.genus || "") || nameCmp();
  const zoneKey = (pet) => zoneTokens(pet.zone)[0] || "\uffff";

  switch (sort) {
    case "name-asc":
      return nameCmp();
    case "name-desc":
      return b.name.localeCompare(a.name);
    case "pet-level-asc":
      return (pa.level || 0) - (pb.level || 0) || nameCmp();
    case "pet-level-desc":
      return (pb.level || 0) - (pa.level || 0) || nameCmp();
    case "zone-asc":
      return zoneKey(a).localeCompare(zoneKey(b)) || nameCmp();
    case "genus-name":
    default:
      return genusCmp();
  }
}

function filteredPets() {
  const q = (document.getElementById("progress-search")?.value || "")
    .trim()
    .toLowerCase();
  const sort = document.getElementById("progress-sort")?.value || "genus-name";
  const pets = (catalog.pets || []).filter((pet) => {
    if (activeGenus !== "all" && pet.genus !== activeGenus) return false;
    if (!matchesOwnership(pet)) return false;
    if (!matchesZone(pet)) return false;
    if (!matchesPetLevel(pet)) return false;
    if (!q) return true;
    const hay = [
      pet.name,
      pet.genus,
      pet.zone || "",
      ...(pet.source_mobs || []),
      ...((pet.spawn_areas || []).flatMap((sa) => [sa.zone, sa.area])),
    ]
      .join(" ")
      .toLowerCase();
    return hay.includes(q);
  });
  pets.sort((a, b) => comparePets(a, b, sort));
  return pets;
}

function progressField(petId, field, label, value, { max = null, stored = null } = {}) {
  const maxAttr = max != null ? ` max="${max}"` : "";
  const storedAttr = stored != null ? ` data-stored="${stored}"` : "";
  const title =
    stored != null && max != null && stored > max
      ? ` title="Stored Lv ${stored} (kept for higher-max regions)"`
      : "";
  return `
    <label class="progress-field muted">
      <span>${label}</span>
      <span class="num-stepper">
        <input data-id="${petId}" data-field="${field}" type="number" min="0"${maxAttr}${storedAttr}${title} value="${value}" />
        <span class="num-stepper-btns">
          <button type="button" class="num-step" data-step="1" data-id="${petId}" data-field="${field}" aria-label="Increase ${label}"></button>
          <button type="button" class="num-step" data-step="-1" data-id="${petId}" data-field="${field}" aria-label="Decrease ${label}"></button>
        </span>
      </span>
    </label>
  `;
}

/** Level to persist: clamp to region max, but keep a higher stored level if the user didn't edit. */
function levelToSave(levelInput) {
  const maxLevel = profileMaxLevel();
  const stored = Number(levelInput.dataset.stored) || 0;
  let level = Number(levelInput.value) || 0;
  level = Math.min(Math.max(0, level), maxLevel);
  if (stored > maxLevel && levelInput.dataset.dirty !== "1") {
    return stored;
  }
  return level;
}

function renderProgress() {
  renderGenusFilters();
  renderZoneFilter();
  const pets = filteredPets();
  const sort = document.getElementById("progress-sort")?.value || "genus-name";
  const groupByGenus = activeGenus === "all" && sort === "genus-name";
  const countEl = document.getElementById("progress-count");
  progressList.innerHTML = "";
  if (!(catalog.pets || []).length) {
    if (countEl) countEl.textContent = "";
    progressList.textContent = "Catalog empty.";
    return;
  }
  if (countEl) {
    countEl.textContent = `Showing ${pets.length} of ${catalog.pets.length} pets`;
  }
  if (!pets.length) {
    progressList.textContent = "No pets match this filter.";
    return;
  }

  let lastGenus = null;
  for (const pet of pets) {
    if (groupByGenus && pet.genus !== lastGenus) {
      lastGenus = pet.genus;
      const heading = document.createElement("h3");
      heading.className = "genus-heading";
      heading.textContent = pet.genus;
      progressList.appendChild(heading);
    }
    const p = petProgress(pet.id);
    const maxLevel = profileMaxLevel();
    const shownLevel = Math.min(p.level || 0, maxLevel);
    const row = document.createElement("div");
    row.className = "progress-card";
    row.innerHTML = `
      <button type="button" class="progress-pet" data-detail="${escapeHtml(pet.id)}" aria-label="Details for ${escapeHtml(pet.name)}">
        <strong>${escapeHtml(pet.name)}</strong>
        <div class="muted">${escapeHtml(pet.genus)}${pet.source_level != null ? ` · mob Lv ${pet.source_level}` : ""}${pet.zone ? ` · ${escapeHtml(pet.zone)}` : ""}${(p.level || 0) > maxLevel ? ` · stored Lv ${p.level}` : ""}</div>
      </button>
      ${progressField(pet.id, "level", "Lv", shownLevel, { max: maxLevel, stored: p.level || 0 })}
      <button type="button" data-save="${pet.id}">Save</button>
    `;
    progressList.appendChild(row);
  }
}

async function reloadAll() {
  const [nextSettings, cat, prog, dataDir] = await Promise.all([
    invoke("get_settings"),
    invoke("get_catalog"),
    invoke("get_progress"),
    invoke("data_dir_path"),
  ]);
  settings = nextSettings;
  catalog = cat;
  progress = prog;
  fillSettings(nextSettings);
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

document.getElementById("genus-filters")?.addEventListener("click", (e) => {
  const btn = e.target.closest("[data-genus]");
  if (!btn) return;
  activeGenus = btn.getAttribute("data-genus");
  renderProgress();
});

document.getElementById("progress-search")?.addEventListener("input", () => {
  renderProgress();
});

for (const id of ["progress-ownership", "progress-zone", "progress-sort"]) {
  document.getElementById(id)?.addEventListener("change", () => renderProgress());
}
for (const id of ["progress-level-min", "progress-level-max"]) {
  document.getElementById(id)?.addEventListener("input", () => renderProgress());
}

document.getElementById("btn-calibrate").addEventListener("click", async () => {
  try {
    await invoke("open_region_picker", { kind: "nameplate" });
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

function formatImportResult(result) {
  const names = result.updated_names || [];
  const header = `Updated ${names.length} pet${names.length === 1 ? "" : "s"}:`;
  return names.length
    ? `${header}\n${names.map((n) => `• ${n}`).join("\n")}`
    : header;
}

document.getElementById("btn-calibrate-insight").addEventListener("click", async () => {
  try {
    await invoke("open_region_picker", { kind: "insight" });
  } catch (err) {
    importOutput.textContent = String(err);
  }
});

document.getElementById("btn-import-insight").addEventListener("click", async () => {
  importOutput.textContent = "Capturing Pet Insight…";
  try {
    const result = await invoke("import_insight_region");
    progress = result.progress;
    importOutput.textContent = formatImportResult(result);
    renderProgress();
  } catch (err) {
    importOutput.textContent = String(err);
  }
});

document.getElementById("btn-import").addEventListener("click", async () => {
  try {
    const selected = await invoke("pick_image");
    if (!selected) return;
    importOutput.textContent = "Importing…";
    const result = await invoke("import_screenshot", { path: selected });
    progress = result.progress;
    importOutput.textContent = formatImportResult(result);
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
    settings = await invoke("save_settings", { settings: readSettingsFromForm() });
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

progressList.addEventListener("input", (e) => {
  const input = e.target.closest("input[data-field]");
  if (!input) return;
  input.dataset.dirty = "1";
  if (input.dataset.field === "level") {
    const maxLevel = profileMaxLevel();
    const clamped = Math.min(Math.max(0, Number(input.value) || 0), maxLevel);
    if (String(clamped) !== input.value) input.value = String(clamped);
  }
});

progressList.addEventListener("click", async (e) => {
  const detailBtn = e.target.closest("[data-detail]");
  if (detailBtn) {
    openPetDetail(detailBtn.getAttribute("data-detail"));
    return;
  }

  const stepBtn = e.target.closest(".num-step");
  if (stepBtn) {
    const id = stepBtn.getAttribute("data-id");
    const field = stepBtn.getAttribute("data-field");
    const delta = Number(stepBtn.getAttribute("data-step")) || 0;
    const input = progressList.querySelector(`input[data-id="${id}"][data-field="${field}"]`);
    if (!input) return;
    const max =
      field === "level"
        ? profileMaxLevel()
        : Number(input.max) || Number.POSITIVE_INFINITY;
    const next = Math.min(max, Math.max(0, (Number(input.value) || 0) + delta));
    input.value = String(next);
    input.dispatchEvent(new Event("input", { bubbles: true }));
    return;
  }

  const btn = e.target.closest("[data-save]");
  if (!btn) return;
  const id = btn.getAttribute("data-save");
  const levelInput = progressList.querySelector(`input[data-id="${id}"][data-field="level"]`);
  progress = await invoke("set_pet_progress", {
    petId: id,
    level: levelToSave(levelInput),
  });
  renderProgress();
});

listen("lookup-error", (event) => {
  trackOutput.textContent = `Hotkey lookup failed: ${event.payload}`;
});

listen("import-result", (event) => {
  const result = event.payload;
  if (result?.progress) progress = result.progress;
  importOutput.textContent = formatImportResult(result || {});
  renderProgress();
});

listen("import-error", (event) => {
  importOutput.textContent = `Hotkey import failed: ${event.payload}`;
});

listen("region-saved", async (event) => {
  settings = await invoke("get_settings");
  fillSettings(settings);
  const payload = event.payload || {};
  const kind = payload.kind || "nameplate";
  const r =
    payload.region ||
    (kind === "insight" ? settings.insight_region : settings.ocr_region) ||
    {};
  const label = kind === "insight" ? "Pet Insight" : "Nameplate";
  const msg = `${label} region saved: ${r.width}×${r.height} at (${r.x}, ${r.y})`;
  if (kind === "insight") {
    importOutput.textContent = msg;
  } else {
    trackOutput.textContent = msg;
  }
});

reloadAll().catch((err) => {
  trackOutput.textContent = String(err);
});
