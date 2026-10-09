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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    pub hotkey: String,
    pub ocr_region: RegionBox,
    pub region_profile: String,
    pub catalog_update_url: String,
    pub custom_catalog_path: String,
    pub use_custom_catalog: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hotkey: "Ctrl+Shift+P".into(),
            ocr_region: RegionBox::default(),
            region_profile: "global".into(),
            catalog_update_url: String::new(),
            custom_catalog_path: String::new(),
            use_custom_catalog: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegionProfile {
    pub label: String,
    pub max_level: u32,
    pub soul_thresholds: Vec<u32>,
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
    #[serde(default)]
    pub owned_effects: HashMap<String, String>,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Catalog {
    pub version: u32,
    #[serde(default)]
    pub attribution: String,
    #[serde(default)]
    pub profiles: HashMap<String, RegionProfile>,
    pub pets: Vec<Pet>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PetProgress {
    pub level: u32,
    pub souls: u32,
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
pub struct LookupResult {
    pub raw_ocr: String,
    pub cleaned_query: String,
    pub best: Option<MatchCandidate>,
    pub suggestions: Vec<MatchCandidate>,
    pub pet: Option<Pet>,
    pub progress: Option<PetProgress>,
    pub profile: RegionProfile,
    pub souls_to_next: Option<u32>,
    pub at_max: bool,
}
