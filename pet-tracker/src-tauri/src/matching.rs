use crate::models::{
    Catalog, LookupResult, MatchCandidate, Pet, PetProgress, ProgressStore, RegionProfile,
};
use regex::Regex;
use std::sync::LazyLock;
use strsim::jaro_winkler;

/// Minimum catalog score to accept a pet suggestion (also used for confusion accept).
const MATCH_THRESHOLD: f64 = 0.72;

static LEVEL_TOKEN_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\b(?:lv\.?|level)\s*\d+\b").unwrap());
static CRUMB_ONLY_LINE_RE: LazyLock<Regex> = LazyLock::new(|| {
    // Standalone UI chrome / level chips / pure punctuation.
    Regex::new(r"(?i)^(?:(?:lv\.?|level)\s*\d+|hp|mp|exp|ok|cancel|[\W_]+|\d+)$").unwrap()
});

/// Lowercase, keep `[a-z0-9]`, collapse other chars to single spaces.
/// Hot path: no regex compile (was re-compiled on every catalog compare).
fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut pending_space = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            if pending_space && !out.is_empty() {
                out.push(' ');
            }
            pending_space = false;
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() {
            pending_space = true;
        }
    }
    out
}

/// Strip level fragments and collapse whitespace; empty if the line is UI chrome only.
fn strip_ui_crumbs(line: &str) -> String {
    let trimmed = line.trim();
    if trimmed.is_empty() || CRUMB_ONLY_LINE_RE.is_match(trimmed) {
        return String::new();
    }
    let stripped = LEVEL_TOKEN_RE.replace_all(trimmed, " ");
    let normalized = normalize(&stripped);
    if normalized.is_empty() || CRUMB_ONLY_LINE_RE.is_match(&normalized) {
        String::new()
    } else {
        normalized
    }
}

fn score_against_normalized(q: &str, candidate: &str) -> f64 {
    let c = normalize(candidate);
    if q.is_empty() || c.is_empty() {
        return 0.0;
    }
    if q == c {
        return 1.0;
    }
    if c.contains(q) || q.contains(&c) {
        return 0.92_f64.max(jaro_winkler(q, &c));
    }
    jaro_winkler(q, &c)
}

fn score_against(query: &str, candidate: &str) -> f64 {
    score_against_normalized(&normalize(query), candidate)
}

fn nearest_catalog_score(catalog: &Catalog, query: &str) -> f64 {
    let q = normalize(query);
    if q.is_empty() {
        return 0.0;
    }
    catalog
        .pets
        .iter()
        .map(|pet| {
            let mut best = score_against_normalized(&q, &pet.name);
            for mob in &pet.source_mobs {
                best = best.max(score_against_normalized(&q, mob));
            }
            best
        })
        .fold(0.0_f64, f64::max)
}

fn map_confusions(query: &str, digit_one: char, pipe: char) -> String {
    query
        .chars()
        .map(|c| match c {
            '0' => 'o',
            '1' => digit_one,
            '3' => 'e',
            '4' => 'a',
            '5' => 's',
            '7' => 't',
            '8' => 'b',
            '|' => pipe,
            other => other,
        })
        .collect()
}

/// Limited OCR confusion variants (digits/pipes/`rn`→letter shapes).
fn confusion_variants(query: &str) -> Vec<String> {
    let mut out = Vec::new();
    // `1`/`|` may be i or l depending on the glyph — try both.
    for (one, pipe) in [('l', 'l'), ('i', 'i'), ('l', 'i'), ('i', 'l')] {
        let fixed = map_confusions(query, one, pipe);
        if fixed != query && !out.iter().any(|v| v == &fixed) {
            out.push(fixed);
        }
    }

    let rn_fixed = query.replace("rn", "m").replace("Rn", "M");
    if rn_fixed != query && !out.iter().any(|v| v == &rn_fixed) {
        out.push(rn_fixed);
    }

    out
}

/// Keep a confusion variant only when it improves nearest catalog score.
fn best_query_variant(catalog: &Catalog, query: &str) -> (String, f64) {
    let base = normalize(query);
    if base.is_empty() {
        return (String::new(), 0.0);
    }
    let mut best_q = base.clone();
    let mut best_score = nearest_catalog_score(catalog, &base);

    for variant in confusion_variants(&base) {
        let v = normalize(&variant);
        if v.is_empty() || v == best_q {
            continue;
        }
        let score = nearest_catalog_score(catalog, &v);
        if score > best_score {
            best_score = score;
            best_q = v;
        }
    }

    (best_q, best_score)
}

/// Pick a catalog query from noisy multi-line OCR: score each line (not longest-only),
/// strip UI crumbs, and apply confusion fixes only when they improve catalog score.
pub fn clean_ocr_text(catalog: &Catalog, raw: &str) -> String {
    let mut candidates: Vec<String> = raw
        .lines()
        .map(strip_ui_crumbs)
        .filter(|l| !l.is_empty())
        .collect();

    // Also consider the joined remaining lines when a name was split across wraps.
    if candidates.len() > 1 {
        let joined = candidates.join(" ");
        if !joined.is_empty() {
            candidates.push(joined);
        }
    }

    if candidates.is_empty() {
        // Fail closed: no usable name line.
        return String::new();
    }

    let mut best_q = String::new();
    let mut best_score = -1.0_f64;

    for line in candidates {
        let (q, score) = best_query_variant(catalog, &line);
        if q.is_empty() {
            continue;
        }
        if score > best_score
            || (score == best_score && q.chars().count() > best_q.chars().count())
        {
            best_score = score;
            best_q = q;
        }
    }

    best_q
}

pub fn match_pets(catalog: &Catalog, query: &str, limit: usize) -> Vec<MatchCandidate> {
    let q = normalize(query);
    if q.is_empty() || limit == 0 {
        return Vec::new();
    }

    let mut scored: Vec<MatchCandidate> = Vec::new();

    for pet in &catalog.pets {
        let mut best_score = score_against_normalized(&q, &pet.name);
        let mut matched_on = pet.name.clone();

        for mob in &pet.source_mobs {
            let s = score_against_normalized(&q, mob);
            if s > best_score {
                best_score = s;
                matched_on = mob.clone();
            }
        }

        if best_score >= MATCH_THRESHOLD {
            scored.push(MatchCandidate {
                pet_id: pet.id.clone(),
                pet_name: pet.name.clone(),
                genus: pet.genus.clone(),
                score: best_score,
                matched_on,
            });
        }
    }

    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    scored.truncate(limit);
    scored
}

fn profile_for(catalog: &Catalog, key: &str) -> RegionProfile {
    catalog
        .profiles
        .get(key)
        .cloned()
        .or_else(|| catalog.profiles.get("global").cloned())
        .unwrap_or(RegionProfile {
            label: "Global".into(),
            max_level: 3,
        })
}

pub fn build_lookup(
    catalog: &Catalog,
    progress: &ProgressStore,
    profile_key: &str,
    raw_ocr: &str,
) -> LookupResult {
    let cleaned = clean_ocr_text(catalog, raw_ocr);
    let suggestions = match_pets(catalog, &cleaned, 5);
    let best = suggestions.first().cloned();
    let profile = profile_for(catalog, profile_key);

    let pet: Option<Pet> = best
        .as_ref()
        .and_then(|b| catalog.pets.iter().find(|p| p.id == b.pet_id).cloned());

    let pet_progress = best
        .as_ref()
        .and_then(|b| progress.pets.get(&b.pet_id).cloned());

    let at_max = pet_progress
        .as_ref()
        .map(|p| p.level >= profile.max_level)
        .unwrap_or(false);

    LookupResult {
        raw_ocr: raw_ocr.to_string(),
        cleaned_query: cleaned,
        best,
        suggestions,
        pet,
        progress: pet_progress,
        profile,
        at_max,
        timings: None,
    }
}

/// Best-effort Pets-page screenshot import: find catalog names in OCR text and nearby levels.
pub fn parse_progress_from_ocr(catalog: &Catalog, raw: &str) -> Vec<(String, PetProgress)> {
    let level_re = Regex::new(r"(?i)\b(?:lv\.?|level)\s*(\d+)\b").unwrap();
    let lines: Vec<&str> = raw.lines().collect();
    let mut updates = Vec::new();

    for pet in &catalog.pets {
        let aliases: Vec<&str> = std::iter::once(pet.name.as_str())
            .chain(pet.source_mobs.iter().map(|s| s.as_str()))
            .collect();

        for (idx, line) in lines.iter().enumerate() {
            let hit = aliases.iter().any(|alias| score_against(line, alias) >= 0.88);
            if !hit {
                continue;
            }
            let block = lines
                .iter()
                .skip(idx.saturating_sub(1))
                .take(5)
                .copied()
                .collect::<Vec<_>>()
                .join("\n");

            let level = level_re
                .captures(&block)
                .and_then(|c| c[1].parse().ok())
                .unwrap_or(0);

            if level > 0 {
                updates.push((pet.id.clone(), PetProgress { level }));
            }
            break;
        }
    }

    updates
}

/// Pet Insight panel: `Lv. N (cur/max)` progress line (distinct from the circular Lv chip).
/// `Ly.` is a common OCR misread of `Lv.` on the insight progress line.
static INSIGHT_LEVEL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bl[vy]\.?\s*(\d+)\s*\(").unwrap());
static ANY_LEVEL_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)\bl[vy]\.?\s*(\d+)\b").unwrap());

/// True when OCR looks like a Pet Insight card (`Lv. N (x/y)`), not a pets list page.
pub fn looks_like_pet_insight(raw: &str) -> bool {
    INSIGHT_LEVEL_RE.is_match(raw)
}

fn extract_insight_level(raw: &str) -> Option<u32> {
    if let Some(c) = INSIGHT_LEVEL_RE.captures(raw) {
        return c[1].parse().ok();
    }
    // Fallback: last bare `Lv. N` — insight line sits below the circular chip.
    ANY_LEVEL_RE
        .captures_iter(raw)
        .last()
        .and_then(|c| c[1].parse().ok())
}

/// Single-pet import from a Pet Insight OCR region / screenshot.
/// Prefers `Lv. N (…)` so the top-right circular level chip is ignored.
pub fn parse_insight_from_ocr(catalog: &Catalog, raw: &str) -> Option<(String, PetProgress)> {
    let level = extract_insight_level(raw)?;
    if level == 0 {
        return None;
    }

    let mut best: Option<(String, f64)> = None;
    for pet in &catalog.pets {
        let aliases = std::iter::once(pet.name.as_str())
            .chain(pet.source_mobs.iter().map(|s| s.as_str()));
        for alias in aliases {
            for line in raw.lines() {
                let score = score_against(line, alias);
                if score < 0.88 {
                    continue;
                }
                let replace = match &best {
                    None => true,
                    Some((_, prev)) => score > *prev,
                };
                if replace {
                    best = Some((pet.id.clone(), score));
                }
            }
        }
    }

    let (pet_id, _) = best?;
    Some((pet_id, PetProgress { level }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Pet;
    use std::collections::HashMap;

    fn sample_catalog() -> Catalog {
        Catalog {
            version: 2,
            attribution: String::new(),
            source_url: String::new(),
            profiles: HashMap::from([(
                "global".into(),
                RegionProfile {
                    label: "Global".into(),
                    max_level: 3,
                },
            )]),
            genera: vec![],
            pets: vec![Pet {
                id: "manduri-fighter".into(),
                name: "Manduri Fighter".into(),
                genus: "Cogni".into(),
                source_mobs: vec!["Wandering Manduri Fighter".into()],
                source_level: Some(27),
                zone: None,
                spawn_areas: vec![],
                owned_effects: HashMap::new(),
                notes: String::new(),
            }],
        }
    }

    #[test]
    fn matches_noisy_ocr() {
        let catalog = sample_catalog();
        let progress = ProgressStore::default();
        let result = build_lookup(
            &catalog,
            &progress,
            "global",
            "Lv.27\nWandering Manduri Fighter",
        );
        assert_eq!(result.best.unwrap().pet_id, "manduri-fighter");
        assert!(!result.at_max);
    }

    #[test]
    fn insight_prefers_parenthetical_level_over_circular_chip() {
        // PetInsight.png layout: name, circular Lv.6 chip, then Lv.1 (19/25).
        let raw = "Lesser Earth Spirit\nLv. 6\nPet Insight\nLv. 1 (19/25)";
        let mut catalog = sample_catalog();
        catalog.pets.push(Pet {
            id: "lesser-earth-spirit".into(),
            name: "Lesser Earth Spirit".into(),
            genus: "Natura".into(),
            source_mobs: vec!["Lesser Earth Spirit".into()],
            source_level: None,
            zone: None,
            spawn_areas: vec![],
            owned_effects: HashMap::new(),
            notes: String::new(),
        });
        assert!(looks_like_pet_insight(raw));
        let (id, progress) = parse_insight_from_ocr(&catalog, raw).unwrap();
        assert_eq!(id, "lesser-earth-spirit");
        assert_eq!(progress.level, 1);
    }

    #[test]
    fn insight_falls_back_to_last_bare_level() {
        let catalog = sample_catalog();
        let raw = "Manduri Fighter\nLv. 2\nPet Insight\nLv. 3";
        let (id, progress) = parse_insight_from_ocr(&catalog, raw).unwrap();
        assert_eq!(id, "manduri-fighter");
        assert_eq!(progress.level, 3);
    }

    #[test]
    fn insight_accepts_ocr_ly_for_lv() {
        // Real capture: Tesseract often reads Insight "Lv." as "Ly." and drops "Pet".
        let raw = "Nepenthes\nInsight Ly. 1 (14/25)";
        let mut catalog = sample_catalog();
        catalog.pets.push(Pet {
            id: "nepenthes".into(),
            name: "Nepenthes".into(),
            genus: "Natura".into(),
            source_mobs: vec!["Nepenthes".into()],
            source_level: None,
            zone: None,
            spawn_areas: vec![],
            owned_effects: HashMap::new(),
            notes: String::new(),
        });
        assert!(looks_like_pet_insight(raw));
        let (id, progress) = parse_insight_from_ocr(&catalog, raw).unwrap();
        assert_eq!(id, "nepenthes");
        assert_eq!(progress.level, 1);
    }

    #[test]
    fn scores_each_line_not_only_longest() {
        let catalog = sample_catalog();
        // Longest line is chrome junk; the real name is shorter.
        let raw = "HP BAR OVERLAY CHROME BANNER EXTRA TEXT HERE\nManduri Fighter\nLv.27";
        let cleaned = clean_ocr_text(&catalog, raw);
        assert_eq!(cleaned, "manduri fighter");
        let result = build_lookup(&catalog, &ProgressStore::default(), "global", raw);
        assert_eq!(result.best.unwrap().pet_id, "manduri-fighter");
    }

    #[test]
    fn strips_inline_level_crumbs() {
        let catalog = sample_catalog();
        let cleaned = clean_ocr_text(&catalog, "Lv.27 Wandering Manduri Fighter");
        assert_eq!(cleaned, "wandering manduri fighter");
    }

    #[test]
    fn confusion_fix_applies_only_when_score_improves() {
        let catalog = sample_catalog();
        // Digit/letter confusion: F1ghter → Fighter improves match.
        let improved = clean_ocr_text(&catalog, "Wandering Manduri F1ghter");
        assert_eq!(improved, "wandering manduri fighter");
        assert!(
            nearest_catalog_score(&catalog, &improved)
                > nearest_catalog_score(&catalog, "wandering manduri f1ghter")
        );

        // Already-correct text must not be mangled by confusion passes.
        let exact = clean_ocr_text(&catalog, "Wandering Manduri Fighter");
        assert_eq!(exact, "wandering manduri fighter");
    }

    #[test]
    fn garbage_ocr_fails_closed() {
        let catalog = sample_catalog();
        let cleaned = clean_ocr_text(&catalog, "|||\n###\n$$$\nLv.12");
        assert!(cleaned.is_empty(), "expected empty, got {cleaned:?}");
        let result = build_lookup(
            &catalog,
            &ProgressStore::default(),
            "global",
            "|||\n###\n$$$\nLv.12",
        );
        assert!(result.best.is_none());
        assert!(result.suggestions.is_empty());
    }

    #[test]
    fn catalog_scoring_stays_fast_on_full_seed() {
        // Regresses the Regex::new-per-compare blowup that made OCR score_* ~20s.
        let catalog: Catalog = serde_json::from_str(include_str!("../../data/pets.json"))
            .expect("bundled pets.json");
        assert!(catalog.pets.len() > 100);

        let start = std::time::Instant::now();
        for raw in [
            "",
            "Ma Ui\nPace",
            "Pel a",
            "Wandering Manduri Fighter",
            "Lv.27\nWandering Manduri Fighter",
        ] {
            for _ in 0..12 {
                let _ = clean_ocr_text(&catalog, raw);
                let _ = match_pets(&catalog, &clean_ocr_text(&catalog, raw), 1);
            }
        }
        let ms = start.elapsed().as_millis();
        assert!(
            ms < 2_000,
            "full-catalog scoring loop took {ms}ms; expected <2000ms"
        );
    }

    #[test]
    fn confusion_does_not_create_false_high_confidence_from_noise() {
        let catalog = sample_catalog();
        // Random letters with a digit — must not invent a decisive Manduri match.
        let raw = "xqz9plm wvtkb";
        let result = build_lookup(&catalog, &ProgressStore::default(), "global", raw);
        assert!(
            result
                .best
                .as_ref()
                .map(|b| b.score < 0.95)
                .unwrap_or(true),
            "unexpected high-confidence match: {:?}",
            result.best
        );
    }
}
