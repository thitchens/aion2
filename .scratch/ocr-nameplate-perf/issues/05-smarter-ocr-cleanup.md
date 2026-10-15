# 05: Smarter OCR text cleanup and line picking

**What to build:** Lookup scores each OCR line (not only the longest), strips common UI crumbs, and applies confusion fixes only when they improve catalog score — better matches on multi-line/noisy plates.

**Blocked by:** 04: Catalog-biased Tesseract for nameplates

**Status:** resolved

- [x] Each non-empty OCR line is considered for catalog scoring, not only the longest line
- [x] Common UI crumbs (e.g. level fragments / chrome words) are stripped before or during match
- [x] Letter-confusion normalization applies only when it improves nearest catalog score
- [x] Multi-line/noisy fixtures resolve to the correct pet more often than longest-line-only behavior
- [x] Empty or garbage OCR still fails closed (no false high-confidence match)

## Answer

Post-OCR cleanup in `matching::clean_ocr_text` is now catalog-aware:

1. **Per-line scoring** — every non-empty line (plus joined wrap) is stripped and scored against pet names/source mobs; the best-scoring query wins instead of longest-line-only.
2. **UI crumbs** — drops standalone level chips / chrome (`Lv.N`, `HP`, `MP`, punctuation-only) and strips inline `Lv./Level N` tokens before normalize.
3. **Confusion fixes** — digit/pipe/`rn`→letter variants are tried only when they raise nearest catalog score (e.g. `F1ghter` → `fighter`); already-correct text is left alone.
4. **Fail closed** — crumb-only / empty OCR yields an empty query and no suggestions.

Lookup and OCR early-exit both use the new `clean_ocr_text(catalog, raw)` signature. Unit tests cover longest-junk-vs-short-name, inline levels, confusion improve-only, and garbage fail-closed.
