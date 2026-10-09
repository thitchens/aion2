use crate::models::{
    Catalog, LookupResult, MatchCandidate, Pet, PetProgress, ProgressStore, RegionProfile,
};
use regex::Regex;
use strsim::jaro_winkler;

fn normalize(s: &str) -> String {
    let lower = s.to_lowercase();
    let re = Regex::new(r"[^a-z0-9\s]").unwrap();
    let cleaned = re.replace_all(&lower, " ");
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Pull a plausible mob/pet name from noisy OCR (drop levels like "Lv.27").
pub fn clean_ocr_text(raw: &str) -> String {
    let mut lines: Vec<String> = raw
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .map(|l| l.to_string())
        .collect();

    if lines.is_empty() {
        return normalize(raw);
    }

    let level_re = Regex::new(r"(?i)^(?:lv\.?|level)\s*\d+$").unwrap();
    lines.retain(|l| !level_re.is_match(l));
    if lines.is_empty() {
        return normalize(raw);
    }

    let best = lines
        .into_iter()
        .max_by_key(|l| l.chars().count())
        .unwrap_or_default();

    let stripped = Regex::new(r"(?i)\b(?:lv\.?|level)\s*\d+\b")
        .unwrap()
        .replace_all(&best, " ");
    normalize(&stripped)
}

fn score_against(query: &str, candidate: &str) -> f64 {
    let q = normalize(query);
    let c = normalize(candidate);
    if q.is_empty() || c.is_empty() {
        return 0.0;
    }
    if q == c {
        return 1.0;
    }
    if c.contains(&q) || q.contains(&c) {
        return 0.92_f64.max(jaro_winkler(&q, &c));
    }
    jaro_winkler(&q, &c)
}

pub fn match_pets(catalog: &Catalog, query: &str, limit: usize) -> Vec<MatchCandidate> {
    let mut scored: Vec<MatchCandidate> = Vec::new();

    for pet in &catalog.pets {
        let mut best_score = score_against(query, &pet.name);
        let mut matched_on = pet.name.clone();

        for mob in &pet.source_mobs {
            let s = score_against(query, mob);
            if s > best_score {
                best_score = s;
                matched_on = mob.clone();
            }
        }

        if best_score >= 0.72 {
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
            soul_thresholds: vec![5, 25, 75],
        })
}

fn souls_to_next(progress: &PetProgress, profile: &RegionProfile) -> (bool, Option<u32>) {
    if progress.level >= profile.max_level {
        return (true, None);
    }
    let idx = progress.level as usize;
    let target = profile.soul_thresholds.get(idx).copied();
    match target {
        Some(t) if t > progress.souls => (false, Some(t - progress.souls)),
        Some(_) => (false, Some(0)),
        None => (true, None),
    }
}

pub fn build_lookup(
    catalog: &Catalog,
    progress: &ProgressStore,
    profile_key: &str,
    raw_ocr: &str,
) -> LookupResult {
    let cleaned = clean_ocr_text(raw_ocr);
    let suggestions = match_pets(catalog, &cleaned, 5);
    let best = suggestions.first().cloned();
    let profile = profile_for(catalog, profile_key);

    let pet: Option<Pet> = best
        .as_ref()
        .and_then(|b| catalog.pets.iter().find(|p| p.id == b.pet_id).cloned());

    let pet_progress = best
        .as_ref()
        .and_then(|b| progress.pets.get(&b.pet_id).cloned());

    let (at_max, souls_to_next) = match &pet_progress {
        Some(p) => souls_to_next(p, &profile),
        None => {
            let unowned = PetProgress { level: 0, souls: 0 };
            souls_to_next(&unowned, &profile)
        }
    };

    LookupResult {
        raw_ocr: raw_ocr.to_string(),
        cleaned_query: cleaned,
        best,
        suggestions,
        pet,
        progress: pet_progress,
        profile,
        souls_to_next,
        at_max,
    }
}

/// Best-effort Pets-page screenshot import: find catalog names in OCR text and nearby level/soul numbers.
pub fn parse_progress_from_ocr(catalog: &Catalog, raw: &str) -> Vec<(String, PetProgress)> {
    let level_re = Regex::new(r"(?i)\b(?:lv\.?|level)\s*(\d+)\b").unwrap();
    let souls_re = Regex::new(r"(?i)\b(\d+)\s*(?:souls?)\b").unwrap();
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
            let souls = souls_re
                .captures(&block)
                .and_then(|c| c[1].parse().ok())
                .unwrap_or(0);

            if level > 0 || souls > 0 {
                updates.push((pet.id.clone(), PetProgress { level, souls }));
            }
            break;
        }
    }

    updates
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Pet;
    use std::collections::HashMap;

    fn sample_catalog() -> Catalog {
        Catalog {
            version: 1,
            attribution: String::new(),
            profiles: HashMap::from([(
                "global".into(),
                RegionProfile {
                    label: "Global".into(),
                    max_level: 3,
                    soul_thresholds: vec![5, 25, 75],
                },
            )]),
            pets: vec![Pet {
                id: "manduri-fighter".into(),
                name: "Manduri Fighter".into(),
                genus: "Cogni".into(),
                source_mobs: vec!["Wandering Manduri Fighter".into()],
                source_level: Some(27),
                zone: None,
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
        assert_eq!(result.souls_to_next, Some(5));
    }
}
