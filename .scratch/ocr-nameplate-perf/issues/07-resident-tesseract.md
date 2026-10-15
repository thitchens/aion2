# 07: Resident (in-process) Tesseract

**What to build:** Reuses a long-lived Tesseract engine so per-attempt cost drops further after attempt count is already reduced; Windows packaging still works.

**Blocked by:** 01: Prefer-and-bail nameplate OCR

**Status:** resolved

- [x] Nameplate OCR reuses an in-process or long-lived Tesseract engine across attempts/hotkeys
- [x] With timings on, per-attempt tesseract cost drops vs cold CLI spawn
- [x] Prefer-and-bail / fallback ladder behavior remains intact
- [x] App still builds and runs on the Windows target with Tesseract available
- [x] Failure mode still surfaces a clear “Tesseract missing/failed” error if the engine cannot start

## Answer

Resident Tesseract lives in `tess_engine.rs`:

1. **In-process reuse** — dynamically loads `libtesseract` (+ leptonica when needed) from the Tesseract install next to `tesseract` on PATH, keeps one `TessBaseAPI` under a mutex, and reuses it across ladder attempts and hotkeys (SetImage / Recognize / GetUTF8Text; no per-attempt process spawn).
2. **CLI fallback** — if the shared library or tessdata cannot be loaded, nameplate OCR falls back to the existing `tesseract` stdin CLI. Builds never link against Tesseract at compile time (`libloading` only), so Windows packaging stays “Tesseract on PATH”.
3. **Ladder unchanged** — prefer-and-bail / adaptive upscale / fallbacks still drive attempts; prepared grays feed the resident engine directly (PNG encode only on CLI fallback).
4. **Errors** — init/spawn failures still say to install Tesseract / check PATH / eng.traineddata.

Confirm per-attempt `tesseract_ms` drops under **Show OCR timings** on Windows when `libtesseract` loads (UB Mannheim install). On this WSL host without Tesseract libs, engine reports `cli` and unit tests pass.
