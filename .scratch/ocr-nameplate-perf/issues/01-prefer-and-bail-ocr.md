# 01: Prefer-and-bail nameplate OCR

**What to build:** Hotkey/Test OCR tries the strong soft-yellow + single-line + whitelist path first and stops on a decisive catalog match; weak reads still fall back to the full ladder. With timings on, easy plates show few attempts.

**Blocked by:** None (can start immediately)

**Status:** resolved

- [x] Preferred path runs first (soft-yellow, single-line PSM, whitelist)
- [x] Early exit when catalog score is decisive (approx ≥ 0.95 or exact normalized match)
- [x] Full variant/PSM ladder still runs when best score stays weak (approx < 0.85)
- [x] With OCR timings enabled, easy fixture/lookup shows fewer attempts than the old always-12 path
- [x] Match quality on strong nameplates is not worse than before

## Answer

Nameplate OCR (`ocr_image_bytes`) now follows a prefer-and-bail ladder:

1. **Preferred first** — `01-yellow-soft` + PSM 7 + whitelist, via `attempt_plan()`.
2. **Early exit** — stop when best catalog score ≥ `0.95` (covers exact normalized matches at 1.0).
3. **Fallback** — any non-decisive score (including weak &lt; ~0.85 and middling) continues through the remaining 11 preprocess/PSM rows; variants are prepared lazily so unused preprocess work is skipped.

Unit tests cover plan order and decisive threshold. Live attempt-count wins show up under **Show OCR timings** when the preferred path hits ≥ 0.95 (1 attempt vs the old fixed 12).
