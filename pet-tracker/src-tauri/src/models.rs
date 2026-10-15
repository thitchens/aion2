use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegionBox {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Default for RegionBox {
    fn default() -> Self {
        Self {
            x: 0,
            y: 0,
            width: 420,
            height: 64,
        }
    }
}

fn default_insight_region() -> RegionBox {
    RegionBox {
        x: 0,
        y: 0,
        width: 520,
        height: 140,
    }
}

fn default_hotkey() -> String {
    "Ctrl+Shift+P".into()
}

fn default_hotkey_import() -> String {
    "Ctrl+Shift+I".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// Global shortcut: OCR nameplate + show Pet Info popup; press again to hide.
    #[serde(default = "default_hotkey")]
    pub hotkey: String,
    /// Global shortcut: import from the calibrated Pet Insight region.
    #[serde(default = "default_hotkey_import")]
    pub hotkey_import: String,
    pub ocr_region: RegionBox,
    /// Screen region covering Pet Insight: pet name + `Lv. N (…)` progress line.
    #[serde(default = "default_insight_region")]
    pub insight_region: RegionBox,
    pub region_profile: String,
    pub catalog_update_url: String,
    pub custom_catalog_path: String,
    pub use_custom_catalog: bool,
    /// When true, OCR lookups include stage timings in the result.
    #[serde(default)]
    pub ocr_debug_timings: bool,
    /// When true, write raw/variant PNGs under the app-data `ocr-debug` folder.
    /// Independent of `ocr_debug_timings` (default off so hotkey latency stays lean).
    #[serde(default)]
    pub ocr_debug_images: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hotkey: default_hotkey(),
            hotkey_import: default_hotkey_import(),
            ocr_region: RegionBox::default(),
            insight_region: default_insight_region(),
            region_profile: "global".into(),
            catalog_update_url: String::new(),
            custom_catalog_path: String::new(),
            use_custom_catalog: false,
            ocr_debug_timings: false,
            ocr_debug_images: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegionProfile {
    pub label: String,
    pub max_level: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnArea {
    pub zone: String,
    pub area: String,
    #[serde(default)]
    pub source_mobs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pet {
    pub id: String,
    pub name: String,
    pub genus: String,
    #[serde(default)]
    pub source_mobs: Vec<String>,
    #[serde(default)]
    pub source_level: Option<u32>,
    #[serde(default)]
    pub zone: Option<String>,
    /// Named places inside zones where associated mobs spawn (from Wikily monsters).
    #[serde(default)]
    pub spawn_areas: Vec<SpawnArea>,
    #[serde(default)]
    pub owned_effects: HashMap<String, String>,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenusInfo {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub pet_count: u32,
    #[serde(default)]
    pub owned_effects: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Catalog {
    pub version: u32,
    #[serde(default)]
    pub attribution: String,
    #[serde(default)]
    pub source_url: String,
    #[serde(default)]
    pub profiles: HashMap<String, RegionProfile>,
    /// Canonical genus categories for filtering/grouping (Cogni, Fera, …).
    #[serde(default)]
    pub genera: Vec<GenusInfo>,
    pub pets: Vec<Pet>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PetProgress {
    pub level: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProgressStore {
    pub pets: HashMap<String, PetProgress>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchCandidate {
    pub pet_id: String,
    pub pet_name: String,
    pub genus: String,
    pub score: f64,
    pub matched_on: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrAttemptTiming {
    pub variant: String,
    pub psm: String,
    pub whitelist: bool,
    /// Preprocess upscale factor used for this attempt (2 = fast path, 3 = escalated).
    #[serde(default)]
    pub upscale: u32,
    pub tesseract_ms: u64,
    pub score: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OcrTimings {
    pub capture_ms: u64,
    pub decode_ms: u64,
    pub preprocess_ms: u64,
    pub tesseract_ms: u64,
    pub score_ms: u64,
    pub total_ms: u64,
    pub attempts: Vec<OcrAttemptTiming>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LookupResult {
    pub raw_ocr: String,
    pub cleaned_query: String,
    pub best: Option<MatchCandidate>,
    pub suggestions: Vec<MatchCandidate>,
    pub pet: Option<Pet>,
    pub progress: Option<PetProgress>,
    pub profile: RegionProfile,
    pub at_max: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timings: Option<OcrTimings>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_image_setting_defaults_off_and_independent() {
        let defaults = Settings::default();
        assert!(!defaults.ocr_debug_images);
        assert!(!defaults.ocr_debug_timings);

        // Older settings.json without the new field still deserializes (images off).
        let legacy = r#"{
            "hotkey": "Ctrl+Shift+P",
            "ocr_region": {"x":0,"y":0,"width":420,"height":64},
            "region_profile": "global",
            "catalog_update_url": "",
            "custom_catalog_path": "",
            "use_custom_catalog": false,
            "ocr_debug_timings": true
        }"#;
        let parsed: Settings = serde_json::from_str(legacy).unwrap();
        assert!(parsed.ocr_debug_timings);
        assert!(!parsed.ocr_debug_images);
        assert_eq!(parsed.insight_region.width, 520);
        assert_eq!(parsed.insight_region.height, 140);
        assert_eq!(parsed.hotkey_import, "Ctrl+Shift+I");
    }
}
