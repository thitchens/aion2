# 02: Lean capture and in-memory OCR feed

**What to build:** Lookup captures only the OCR region and feeds preprocess/Tesseract without hot-path PNG disk round-trips. Timings show lower capture/decode cost; match behavior unchanged on fixtures.

**Blocked by:** None (can start immediately)

**Status:** resolved

- [x] Capture prefers the configured OCR region over full-monitor grab+crop when the API allows
- [x] Hot path keeps image buffers in memory through preprocess into Tesseract (stdin or equivalent)
- [x] No temp PNG write required for a successful nameplate OCR
- [x] With timings on, capture/decode costs drop vs prior full-monitor + PNG round-trip behavior
- [x] Fixture/catalog match behavior stays equivalent for the same pixels

## Answer

Lean hot path is in place:

1. **Region capture** — `ocr::capture_region` returns an in-memory `RgbaImage`. On Windows it BitBlts only the OCR rectangle (`windows` GDI); non-Windows keeps full-monitor+crop via xcap 0.0.14 (newer xcap with `capture_region` pulls PipeWire and breaks Linux/WSL builds).
2. **No capture PNG round-trip** — `lookup_ocr_region` calls `ocr_rgba_image` directly (`decode_ms` stays 0 on the live path).
3. **Tesseract stdin** — prepared gray variants are encoded to PNG bytes in memory and fed with `tesseract - stdout` (no temp file). `ocr_image_bytes` remains as a fixture/decode seam.

Unit tests cover RGBA↔PNG pixel equivalence and PNG magic for the stdin feed. Confirm capture/decode drops on Windows with **Show OCR timings**.
