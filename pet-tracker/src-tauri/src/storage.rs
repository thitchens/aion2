use crate::models::{Catalog, ProgressStore, Settings};
use std::fs;
use std::path::PathBuf;

const BUNDLED_CATALOG: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../data/pets.json"));

pub fn app_data_dir() -> Result<PathBuf, String> {
    let base = dirs::data_dir().ok_or_else(|| "Could not resolve app data directory".to_string())?;
    let dir = base.join("aion2-pet-tracker");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn settings_path() -> Result<PathBuf, String> {
    Ok(app_data_dir()?.join("settings.json"))
}

fn progress_path() -> Result<PathBuf, String> {
    Ok(app_data_dir()?.join("progress.json"))
}

fn cached_catalog_path() -> Result<PathBuf, String> {
    Ok(app_data_dir()?.join("catalog-cache.json"))
}

pub fn load_settings() -> Result<Settings, String> {
    let path = settings_path()?;
    if !path.exists() {
        let defaults = Settings::default();
        save_settings(&defaults)?;
        return Ok(defaults);
    }
    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| e.to_string())
}

pub fn save_settings(settings: &Settings) -> Result<(), String> {
    let path = settings_path()?;
    let raw = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    fs::write(path, raw).map_err(|e| e.to_string())
}

pub fn load_progress() -> Result<ProgressStore, String> {
    let path = progress_path()?;
    if !path.exists() {
        return Ok(ProgressStore::default());
    }
    let raw = fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&raw).map_err(|e| e.to_string())
}

pub fn save_progress(progress: &ProgressStore) -> Result<(), String> {
    let path = progress_path()?;
    let raw = serde_json::to_string_pretty(progress).map_err(|e| e.to_string())?;
    fs::write(path, raw).map_err(|e| e.to_string())
}

pub fn bundled_catalog() -> Result<Catalog, String> {
    serde_json::from_str(BUNDLED_CATALOG).map_err(|e| format!("Bundled catalog invalid: {e}"))
}

pub fn load_catalog(settings: &Settings) -> Result<Catalog, String> {
    if settings.use_custom_catalog && !settings.custom_catalog_path.is_empty() {
        let raw = fs::read_to_string(&settings.custom_catalog_path).map_err(|e| {
            format!(
                "Failed to read custom catalog {}: {e}",
                settings.custom_catalog_path
            )
        })?;
        return serde_json::from_str(&raw).map_err(|e| format!("Custom catalog invalid: {e}"));
    }

    let cache = cached_catalog_path()?;
    if cache.exists() {
        let raw = fs::read_to_string(&cache).map_err(|e| e.to_string())?;
        if let Ok(catalog) = serde_json::from_str::<Catalog>(&raw) {
            return Ok(catalog);
        }
    }

    bundled_catalog()
}

pub fn refresh_catalog_from_url(url: &str) -> Result<Catalog, String> {
    if url.trim().is_empty() {
        return Err("Catalog update URL is empty".into());
    }
    let response = reqwest::blocking::get(url).map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("Catalog URL returned HTTP {}", response.status()));
    }
    let raw = response.text().map_err(|e| e.to_string())?;
    let catalog: Catalog =
        serde_json::from_str(&raw).map_err(|e| format!("Downloaded catalog invalid: {e}"))?;
    let path = cached_catalog_path()?;
    fs::write(&path, serde_json::to_string_pretty(&catalog).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    Ok(catalog)
}

pub fn reset_catalog_cache() -> Result<(), String> {
    let path = cached_catalog_path()?;
    if path.exists() {
        fs::remove_file(path).map_err(|e| e.to_string())?;
    }
    Ok(())
}
