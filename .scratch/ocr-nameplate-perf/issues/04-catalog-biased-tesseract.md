# 04: Catalog-biased Tesseract for nameplates

**What to build:** Tesseract is tuned for a single English name line (PSM/DPI/OEM/whitelist) and biased with catalog pet + source-mob words. Fixture/noisy nameplates match the right pet more often.

**Blocked by:** None (can start immediately)

**Status:** resolved

- [x] Nameplate OCR uses single-line-oriented page segmentation and explicit DPI after upscale
- [x] OEM/whitelist choices match English mob-name characters
- [x] Catalog pet names and source mobs feed Tesseract user-words (and patterns if useful)
- [x] Noisy or previously weak fixtures match the correct pet more reliably
- [x] Guardrails unchanged: OCR + local catalog only

## Answer

Nameplate Tesseract is now single-line + catalog-biased:

1. **PSM / DPI / OEM** — preferred path stays PSM `7`; every nameplate attempt uses `--oem 1` (LSTM) and `--dpi 288` (`96 ×` current `3×` upscale). Whitelist is the English mob-name charset (`A–Z a–z 0–9 .'-`), covering hyphen/apostrophe catalog names.
2. **Catalog vocabulary** — `ensure_catalog_vocab` writes unique tokens from pet names + source mobs to app-data `ocr-vocab/user-words.txt`, plus a small Title Case / hyphen / apostrophe `user-patterns.txt`. Files are reused when contents unchanged; passed as `--user-words` / `--user-patterns` on the nameplate stdin path only (Pets-page OCR stays unbiased).
3. **Guardrails** — still OCR + local catalog only; vocab is derived from the same bundled/custom catalog JSON.

Unit tests cover config constants, word/pattern extraction, vocab file reuse, and CLI flag wiring. Confirm noisier live plates under **Test OCR** / **Show OCR timings** (Tesseract not available in this WSL environment).
