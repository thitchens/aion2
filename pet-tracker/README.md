# AION 2 — Pet Tracker

Windows Tauri app that OCRs the **target nameplate**, looks up the associated **pet** in a local catalog, and shows **your** pet level / soul progress in a movable popup.

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
- [Tesseract OCR](https://github.com/tesseract-ocr/tesseract) on `PATH` (`tesseract --version`)
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
2. **Track → Draw nameplate region** — drag a box over the in-game target nameplate.
3. **Import** — optionally import a Pets UI screenshot so your levels/souls are known.
4. In game, target an NPC and press the hotkey — a **Pet Info** popup opens/focuses with the match.

Manual search works without capture. Progress can always be edited under **Progress**.

## Catalog

Bundled seed: [`data/pets.json`](data/pets.json) (unofficial, incomplete, may drift).

- Optional **catalog update URL** downloads a JSON file into the app data cache (same schema).
- Or enable **custom catalog** and point at a local JSON file.

Schema: `version`, `profiles` (soul thresholds / max level), `pets[]` with `id`, `name`, `genus`, `source_mobs`, optional `source_level` / `zone` / `owned_effects` / `notes`.

## Out of v1

Polling OCR, live Pets-page region import, multi-language OCR, exclusive-fullscreen polish, full kitchen-sink completeness.
