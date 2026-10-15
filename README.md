# aion2

Unofficial helper tools for **AION 2**. Two separate products live in this repo:

| Folder | What it is |
| --- | --- |
| [`flight-log/`](flight-log/) | Browser checklist for dailies, weeklies, and shop/craft limits (GitHub Pages) |
| [`pet-tracker/`](pet-tracker/) | Windows app: OCR the target nameplate → show associated pet + your pet level |

They do not share runtime code. Prefer borderless windowed + English UI when using the pet tracker.

## GitHub Pages

Point Pages at the **`/flight-log`** folder on `main`. See [`flight-log/README.md`](flight-log/README.md).

## Pet tracker

See [`pet-tracker/README.md`](pet-tracker/README.md). Build on Windows (`npm run tauri build`). **OCR only — never memory, hooks, or packets** (also in [`AGENTS.md`](AGENTS.md)).
