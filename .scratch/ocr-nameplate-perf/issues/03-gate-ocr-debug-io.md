# 03: Gate OCR debug image writes

**What to build:** Hot path does not write `ocr-debug` PNGs unless an explicit debug setting is on (timings can stay independent). Faster default lookups; debug still available when needed.

**Blocked by:** None (can start immediately)

**Status:** resolved

- [x] Default settings do not write raw/variant debug PNGs on hotkey or Test OCR
- [x] An explicit debug setting (separate from timings if needed) re-enables those writes
- [x] Show OCR timings still works when image debug is off
- [x] Enabling image debug still produces inspectable artifacts under the app data OCR debug dir

## Answer

Debug image I/O is gated behind a new setting, independent of timings:

- **`ocr_debug_images`** (default `false`, serde-default for older `settings.json`) — when off, `ocr_rgba_image` does not create/write under `ocr-debug/` (no `00-raw.png`, variant PNGs, or `last-run.txt`).
- **`ocr_debug_timings`** unchanged — still attaches stage/attempt timings with image debug off.
- Settings UI checkbox: **Write OCR debug images (raw/variants)**.

Hot path: `lookup_ocr_region` passes both flags into `ocr_rgba_image`.
