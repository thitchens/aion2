## Problem Statement

Hotkey nameplate OCR is too slow and sometimes misreads the gold target name, so pet lookup feels laggy or wrong. The pipeline always runs many preprocess/Tesseract variants, captures a full monitor, and round-trips PNGs, while recognition still depends on post-hoc fuzzy matching instead of guiding OCR toward catalog names.

## Solution

Make nameplate OCR fast in the common case and more accurate on AION 2 gold nameplates: short-circuit to the best preprocess/PSM path, avoid unnecessary capture/I/O work, tune Tesseract for a single name line, and bias recognition toward the pet catalog — still OCR + local data only. Use the existing **Show OCR timings** setting to verify wins.

## User Stories

1. As a player, I want hotkey lookup to finish quickly, so that I can check pet progress without breaking combat flow.
2. As a player, I want the nameplate name read correctly on the first try, so that the right pet shows up.
3. As a player, I want rare/hard nameplates to still resolve after a fallback path, so that speed gains don’t break edge cases.
4. As a player, I want gold nameplate text preferred over UI chrome, so that level chips and bars don’t poison the match.
5. As a player, I want OCR biased toward real mob/pet names from my catalog, so that garbage strings are less likely.
6. As a player, I want common OCR letter confusions corrected when they improve a catalog match, so that near-misses still land.
7. As a player, I want multi-line OCR noise handled by scoring each line, so that the longest junk line doesn’t win.
8. As a player, I want a tight nameplate region to be enough, so that I don’t need a huge capture box for accuracy.
9. As a developer, I want typical lookups to run one strong variant first, so that I don’t pay for twelve Tesseract starts every time.
10. As a developer, I want early exit when catalog score is already excellent, so that further variants are skipped.
11. As a developer, I want historically winning preprocess/PSM remembered, so that the hot path stays warm for this machine/UI.
12. As a developer, I want redundant whitelist twins dropped unless score is weak, so that duplicate work disappears.
13. As a developer, I want region-sized capture instead of full-monitor grab+crop, so that capture time drops.
14. As a developer, I want in-memory image handoff into OCR (no PNG disk round-trip on the hot path), so that encode/decode overhead shrinks.
15. As a developer, I want debug image writes off by default, so that hotkey latency isn’t dominated by disk I/O.
16. As a developer, I want optional in-process or reused Tesseract, so that process-spawn cost isn’t paid per attempt.
17. As a developer, I want upscale to start milder and escalate only on weak scores, so that OCR work scales with difficulty.
18. As a developer, I want PSM/DPI/OEM tuned for a single English nameplate line, so that Tesseract assumptions match the crop.
19. As a developer, I want soft-yellow (and learned fallbacks) plus adaptive thresholding when needed, so that gold ink stays readable.
20. As a developer, I want the name band cropped inside the OCR region when level UI is included, so that “Lv.N” doesn’t enter OCR.
21. As a developer, I want Tesseract confidence considered when scores tie, so that longer garbage doesn’t beat shorter correct text.
22. As a developer, I want OCR timings (already in settings) to show capture vs preprocess vs Tesseract, so that I can prove which optimization mattered.
23. As a developer, I want fixture-based tests at the lookup seam, so that speed/accuracy changes don’t require a live game client.
24. As a user of the Track tab, I want Test OCR to still show match details plus timings when enabled, so that calibration stays inspectable.
25. As a maintainer, I want guardrails preserved (no memory/hooks/packets), so that the app stays OCR-and-data-only.

## Implementation Decisions

- **Primary seam**: OCR region lookup result (capture → OCR → catalog match → lookup result). One seam for behavior tests; no new lower seams unless early-exit attempt counts can’t be observed via timings/attempts on that result.
- **Keep** multi-variant OCR as a **fallback ladder**, not the default hot path. Order: preferred variant (soft-yellow + single-line PSM + whitelist) → early exit on strong catalog score → remaining variants/PSMs only if weak.
- **Early-exit threshold**: stop when best catalog score is clearly decisive (approx ≥ 0.95 or exact normalized catalog/source-mob match). If best remains weak (approx < 0.85), run the fuller ladder.
- **Persist winner hint** (last successful preprocess + PSM + whitelist) in settings or a small local debug sidecar so the next hotkey tries that path first.
- **Capture**: prefer capturing only the configured OCR region; avoid full-monitor capture+crop when the capture API allows.
- **Image pipeline**: keep RGBA/gray buffers in memory through preprocess; feed Tesseract via stdin or an in-process binding; avoid temp PNG files on the success path.
- **Debug artifacts**: gate writing raw/variant PNGs behind an explicit debug setting (timings setting may remain independent); default off for hotkey path.
- **Tesseract config for nameplates**: prefer single-line/single-word page segmentation, explicit DPI after upscale, LSTM-oriented OEM, keep character whitelist aligned to English mob names.
- **Catalog vocabulary**: generate user-words (and optionally user-patterns) from pet names + source mobs for Tesseract.
- **Preprocess**: keep soft-yellow as primary; add adaptive/Otsu-style thresholding and light stroke enhancement only on fallback; optional inner crop to the name band when the region includes level UI.
- **Post-OCR**: score each OCR line against the catalog (not only the longest); strip common UI crumbs; apply confusion normalization only when it improves nearest catalog score.
- **Upscale policy**: try 2× first; escalate to 3× only on weak scores.
- **In-process Tesseract**: preferred performance end-state; CLI subprocess remains acceptable interim if bindings are painful on Windows packaging — but attempt count / early exit still required either way.
- **Timings**: existing OCR debug timings on the lookup result stays the measurement surface (capture, decode, preprocess, tesseract, score, total, per-attempt). New work should keep that contract stable or extend it additively.
- **Guardrails**: screen OCR + bundled/user catalog + local progress only; no process memory, hooks, or packet capture.

## Testing Decisions

- Test **external behavior** at the OCR region lookup seam: given a fixture nameplate image (or injected capture bytes) + catalog + settings, assert matched pet/query and, when timings are enabled, timing/attempt shape.
- Good tests: clear fixture → correct pet; noisy/garbage OCR → no false high-confidence match; strong first-attempt case → few attempts / early exit observable via timings; timings off → no timings payload.
- Prefer fixture PNGs under test data over live monitor capture.
- Prior art: existing yellow-affinity/preprocess unit tests and noisy-OCR matching tests in the Rust side — extend that style rather than UI snapshot tests.
- Do not assert internal preprocess function names or exact Tesseract CLI flags unless they are part of a documented settings/debug contract.

## Out of Scope

- Polling/continuous OCR
- Multi-language OCR / non-English UI
- Exclusive-fullscreen capture polish
- Pets-page live region import (screenshot import path may stay as-is)
- Replacing fuzzy catalog matching entirely
- UI redesign beyond what’s needed to expose any new debug toggles
- Changing Flight Log or coupling pet-tracker to it

## Further Notes

- **Already shipped**: Settings checkbox **Show OCR timings (debug)** with per-stage and per-attempt timings on lookup results and in the OCR debug log. Treat that as the measurement hook for this work, not as unfinished scope unless gaps appear.
- Suggested implementation order: early exit + preferred variant → gate debug I/O → region capture + no PNG round-trip → catalog user-words + PSM/DPI → in-process Tesseract → adaptive threshold / name-band crop.
- Domain terms: nameplate, OCR region, soft-yellow, pet catalog, source mob, soul progress, region profile.
- **Primary test seam**: OCR region lookup → lookup result (including optional timings/attempts).
