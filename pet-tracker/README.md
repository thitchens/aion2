# AION 2 — Pet Tracker

Windows Tauri app that OCRs the **target nameplate**, looks up the associated **pet** in a local catalog, and shows **your** pet level in a movable popup.

Flight Log (the checklist) lives separately in [`../flight-log/`](../flight-log/).

## Guardrails

**OCR and local data only.** This app must never:

- read the game process memory
- inject into or hook the game client
- capture or intercept network packets

See also the repo root [`AGENTS.md`](../AGENTS.md).

## Requirements

- **Windows** (primary target; develop/build there for global hotkeys + capture)
- [Rust](https://rustup.rs/) + [Node.js](https://nodejs.org/)
- [Tesseract OCR](https://github.com/tesseract-ocr/tesseract) on `PATH` (`tesseract --version`). The app prefers a resident in-process engine (loads `libtesseract` next to the install) and falls back to the CLI if the shared library is unavailable.
- AION 2 in **borderless windowed**, **English** UI preferred

## Quick start

```bash
cd pet-tracker
npm install
npm run dev
```

Release build:

```bash
npm run build
```

## v1 usage

1. **Settings** — confirm hotkey (`Ctrl+Shift+P`), region profile (Global Lv3 default), catalog options.
2. **Track → Edit nameplate region** — draw/move/resize the box over the **top-of-screen static target nameplate**, then **Save** (Enter). Esc cancels. With OCR debug images on, saving also writes `region-context.png` so you can verify the capture rectangle.
3. **Import → Pet Insight** — calibrate a region over the pet name + `Lv. N (…)` line in the Insight panel, then **Import from region** while that UI is open. Or import a Pets-list / Insight screenshot.
4. In game, target an NPC and press the hotkey — a **Pet Info** popup opens/focuses with the match.

Manual search works without capture. Progress can always be edited under **Progress**.

## Catalog

Bundled seed: [`data/pets.json`](data/pets.json) — **206 pets** across Cogni / Fera / Natura / Varian / Special, seeded from the unofficial [Wikily](https://wikily.gg/aion-2/pets) dataset (may drift).

- Progress tab filters by **genus**, ownership, zone, pet level, and search; sortable by name, levels, or zone. Click a pet for spawn areas, source mobs, and effects.
- Optional **catalog update URL** downloads JSON into the app data cache (same schema; used when its `version` ≥ bundled).
- Or enable **custom catalog** and point at a local JSON file.
- Re-join spawn areas from Wikily monsters: `python3 scripts/enrich_spawn_areas.py`.

Schema: `version`, `genera[]`, `profiles`, `pets[]` with `id`, `name`, `genus`, `source_mobs`, optional `source_level` / `zone` / `spawn_areas[]` (`zone`, `area`, `source_mobs`) / `owned_effects` / `notes`.

## Out of v1

Polling OCR, bulk Pets-page live region import, multi-language OCR, exclusive-fullscreen polish, full kitchen-sink completeness.
