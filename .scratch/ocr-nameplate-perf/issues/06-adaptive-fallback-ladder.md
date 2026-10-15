# 06: Adaptive fallback ladder (learned winner, upscale, name band)

**What to build:** Remembers the last winning preprocess/PSM, escalates upscale only when weak, and improves hard plates via adaptive threshold / name-band crop without slowing the fast path.

**Blocked by:** 01: Prefer-and-bail nameplate OCR

**Status:** resolved

- [x] Last successful preprocess + PSM + whitelist is persisted and tried first on the next lookup
- [x] Upscale starts at 2× and escalates to 3× only when the score stays weak
- [x] Fallback path can apply adaptive/Otsu-style thresholding and/or inner name-band crop
- [x] Easy plates still early-exit quickly (timings/attempts stay low)
- [x] Hard plates improve match rate vs prefer-and-bail alone

## Answer

Adaptive nameplate ladder in `ocr.rs`:

1. **Learned winner** — successful attempts (score ≥ 0.85) persist to app-data `ocr-winner.json` and lead the next base plan.
2. **Upscale** — hot path uses **2×** (`dpi 192`); **3×** (`dpi 288`) only when best score stays **weak (&lt; 0.85)** after the 2× ladder. Decisive (≥ 0.95) still early-exits.
3. **Fallback variants** (only while weak at 2×): `05-yellow-otsu`, `06-yellow-stroke` (light ink thicken), `07-nameband-soft` (yellow-energy vertical crop then soft-yellow).
4. **Timings** — each attempt records `upscale`; UI shows `x2` / `x3`.

Easy plates: winner/preferred at 2× → bail. Hard plates: base → fallbacks → 3× escalation. Unit tests cover plan order, winner round-trip, thresholds, name-band crop, and Otsu/stroke prepare.
