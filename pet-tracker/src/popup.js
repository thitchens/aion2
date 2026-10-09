const { listen } = window.__TAURI__.event;
const { invoke } = window.__TAURI__.core;

const content = document.getElementById("popup-content");

function render(result) {
  if (!result?.best || !result.pet) {
    const suggestions = (result?.suggestions || [])
      .map(
        (s) =>
          `<li><button type="button" data-pet="${s.pet_id}">${s.pet_name} · ${s.genus} (${s.score.toFixed(2)})</button></li>`
      )
      .join("");
    content.innerHTML = `
      <h2>No match</h2>
      <p class="muted">OCR: ${escapeHtml(result?.raw_ocr || "(empty)")}</p>
      <p class="muted">Query: ${escapeHtml(result?.cleaned_query || "")}</p>
      <ul class="suggestions">${suggestions || "<li class='muted'>No suggestions</li>"}</ul>
    `;
    return;
  }

  const pet = result.pet;
  const prog = result.progress;
  const effects = Object.entries(pet.owned_effects || {})
    .map(([lvl, text]) => `<div>Lv ${lvl}: ${escapeHtml(text)}</div>`)
    .join("");

  let progressBlock = `<dt>Your progress</dt><dd>Not recorded — import a Pets screenshot or edit in Progress</dd>`;
  if (prog) {
    progressBlock = `
      <dt>Your pet level</dt><dd>${prog.level}</dd>
      <dt>Souls</dt><dd>${prog.souls}</dd>
    `;
    if (result.at_max) {
      progressBlock += `<dt>Status</dt><dd>Max level (${result.profile.label})</dd>`;
    } else if (result.souls_to_next != null) {
      progressBlock += `<dt>To next level</dt><dd>${result.souls_to_next} souls</dd>`;
    }
  }

  content.innerHTML = `
    <h2>${escapeHtml(pet.name)}</h2>
    <p class="muted">${escapeHtml(pet.genus)} · matched “${escapeHtml(result.best.matched_on)}”</p>
    <dl>
      ${pet.source_level != null ? `<dt>Source level</dt><dd>${pet.source_level}</dd>` : ""}
      ${pet.zone ? `<dt>Zone</dt><dd>${escapeHtml(pet.zone)}</dd>` : ""}
      ${progressBlock}
    </dl>
    <h3>Owned effects</h3>
    ${effects || "<p class='muted'>None listed</p>"}
    ${pet.notes ? `<p class="muted">${escapeHtml(pet.notes)}</p>` : ""}
    <p class="muted">Raw OCR: ${escapeHtml(result.raw_ocr || "")}</p>
    ${(result.suggestions || []).length > 1
      ? `<h3>Other suggestions</h3><ul class="suggestions">${result.suggestions
          .slice(1)
          .map(
            (s) =>
              `<li><button type="button" data-pet="${s.pet_id}">${escapeHtml(s.pet_name)} (${s.score.toFixed(2)})</button></li>`
          )
          .join("")}</ul>`
      : ""}
  `;
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
  const result = await invoke("lookup_pet_id", { petId: btn.getAttribute("data-pet") });
  render(result);
});

listen("lookup-result", (event) => {
  render(event.payload?.result ?? event.payload);
});
