mod matching;
mod models;
mod ocr;
mod storage;

use matching::{build_lookup, parse_progress_from_ocr};
use models::{LookupResult, PetProgress, ProgressStore, RegionBox, Settings};
use serde::Serialize;
use std::path::PathBuf;
use std::str::FromStr;
use tauri::{AppHandle, Emitter, Manager, WebviewWindow};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

#[derive(Clone, Serialize)]
struct PopupPayload {
    result: LookupResult,
}

fn show_popup(app: &AppHandle, result: LookupResult) -> Result<(), String> {
    if let Some(popup) = app.get_webview_window("popup") {
        popup.show().map_err(|e| e.to_string())?;
        popup.set_focus().map_err(|e| e.to_string())?;
        app.emit("lookup-result", PopupPayload { result })
            .map_err(|e| e.to_string())?;
        Ok(())
    } else {
        Err("Popup window missing".into())
    }
}

fn run_hotkey_lookup(app: &AppHandle) -> Result<(), String> {
    let settings = storage::load_settings()?;
    let catalog = storage::load_catalog(&settings)?;
    let progress = storage::load_progress()?;
    let png = ocr::capture_region_png(&settings.ocr_region)?;
    let raw = ocr::ocr_image_bytes(&png)?;
    let result = build_lookup(&catalog, &progress, &settings.region_profile, &raw);
    show_popup(app, result)
}

fn parse_hotkey(hotkey: &str) -> Result<Shortcut, String> {
    // Accept either Ctrl+Shift+P or CommandOrControl+Shift+P style.
    let normalized = hotkey
        .replace("Ctrl", "Control")
        .replace("cmdorcontrol", "CommandOrControl");
    Shortcut::from_str(&normalized).or_else(|_| Shortcut::from_str(hotkey))
        .map_err(|e| format!("Invalid hotkey '{hotkey}': {e}"))
}

fn register_hotkey(app: &AppHandle, hotkey: &str) -> Result<(), String> {
    let shortcut = parse_hotkey(hotkey)?;
    let _ = app.global_shortcut().unregister_all();
    app.global_shortcut()
        .register(shortcut)
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn get_settings() -> Result<Settings, String> {
    storage::load_settings()
}

#[tauri::command]
fn save_settings(app: AppHandle, settings: Settings) -> Result<Settings, String> {
    storage::save_settings(&settings)?;
    register_hotkey(&app, &settings.hotkey)?;
    Ok(settings)
}

#[tauri::command]
fn get_catalog() -> Result<serde_json::Value, String> {
    let settings = storage::load_settings()?;
    let catalog = storage::load_catalog(&settings)?;
    serde_json::to_value(catalog).map_err(|e| e.to_string())
}

#[tauri::command]
fn get_progress() -> Result<ProgressStore, String> {
    storage::load_progress()
}

#[tauri::command]
fn set_pet_progress(pet_id: String, level: u32, souls: u32) -> Result<ProgressStore, String> {
    let mut progress = storage::load_progress()?;
    progress.pets.insert(pet_id, PetProgress { level, souls });
    storage::save_progress(&progress)?;
    Ok(progress)
}

#[tauri::command]
fn lookup_text(query: String) -> Result<LookupResult, String> {
    let settings = storage::load_settings()?;
    let catalog = storage::load_catalog(&settings)?;
    let progress = storage::load_progress()?;
    Ok(build_lookup(
        &catalog,
        &progress,
        &settings.region_profile,
        &query,
    ))
}

#[tauri::command]
fn lookup_pet_id(pet_id: String) -> Result<LookupResult, String> {
    let settings = storage::load_settings()?;
    let catalog = storage::load_catalog(&settings)?;
    let progress = storage::load_progress()?;
    let name = catalog
        .pets
        .iter()
        .find(|p| p.id == pet_id)
        .map(|p| p.name.clone())
        .ok_or_else(|| format!("Unknown pet id {pet_id}"))?;
    Ok(build_lookup(
        &catalog,
        &progress,
        &settings.region_profile,
        &name,
    ))
}

#[tauri::command]
fn test_ocr_region() -> Result<LookupResult, String> {
    let settings = storage::load_settings()?;
    let catalog = storage::load_catalog(&settings)?;
    let progress = storage::load_progress()?;
    let png = ocr::capture_region_png(&settings.ocr_region)?;
    let raw = ocr::ocr_image_bytes(&png)?;
    Ok(build_lookup(
        &catalog,
        &progress,
        &settings.region_profile,
        &raw,
    ))
}

#[tauri::command]
fn trigger_lookup(app: AppHandle) -> Result<LookupResult, String> {
    let settings = storage::load_settings()?;
    let catalog = storage::load_catalog(&settings)?;
    let progress = storage::load_progress()?;
    let png = ocr::capture_region_png(&settings.ocr_region)?;
    let raw = ocr::ocr_image_bytes(&png)?;
    let result = build_lookup(&catalog, &progress, &settings.region_profile, &raw);
    show_popup(&app, result.clone())?;
    Ok(result)
}

#[tauri::command]
fn show_lookup_popup(app: AppHandle, result: LookupResult) -> Result<(), String> {
    show_popup(&app, result)
}

#[tauri::command]
fn open_region_picker(app: AppHandle) -> Result<(), String> {
    let picker: WebviewWindow = app
        .get_webview_window("region-picker")
        .ok_or_else(|| "Region picker window missing".to_string())?;
    picker.show().map_err(|e| e.to_string())?;
    picker.set_focus().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn save_ocr_region(app: AppHandle, region: RegionBox) -> Result<Settings, String> {
    let mut settings = storage::load_settings()?;
    settings.ocr_region = region;
    storage::save_settings(&settings)?;
    let _ = app.emit("region-saved", &settings.ocr_region);
    Ok(settings)
}

#[tauri::command]
fn close_region_picker(app: AppHandle) -> Result<(), String> {
    if let Some(picker) = app.get_webview_window("region-picker") {
        picker.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
fn import_screenshot(path: String) -> Result<ProgressStore, String> {
    let settings = storage::load_settings()?;
    let catalog = storage::load_catalog(&settings)?;
    let raw = ocr::ocr_image_path(&PathBuf::from(&path))?;
    let updates = parse_progress_from_ocr(&catalog, &raw);
    if updates.is_empty() {
        return Err(format!(
            "No pets recognized in screenshot OCR.\nRaw text:\n{raw}"
        ));
    }
    let mut progress = storage::load_progress()?;
    for (id, pet_progress) in updates {
        progress.pets.insert(id, pet_progress);
    }
    storage::save_progress(&progress)?;
    Ok(progress)
}

#[tauri::command]
fn pick_image(app: AppHandle) -> Result<Option<String>, String> {
    let path = app
        .dialog()
        .file()
        .add_filter("Images", &["png", "jpg", "jpeg", "webp", "bmp"])
        .blocking_pick_file();
    Ok(path.map(|p| p.to_string()))
}

#[tauri::command]
fn pick_json(app: AppHandle) -> Result<Option<String>, String> {
    let path = app
        .dialog()
        .file()
        .add_filter("JSON", &["json"])
        .blocking_pick_file();
    Ok(path.map(|p| p.to_string()))
}

#[tauri::command]
fn refresh_catalog() -> Result<serde_json::Value, String> {
    let settings = storage::load_settings()?;
    let catalog = if !settings.catalog_update_url.trim().is_empty() {
        storage::refresh_catalog_from_url(&settings.catalog_update_url)?
    } else {
        storage::reset_catalog_cache()?;
        storage::bundled_catalog()?
    };
    serde_json::to_value(catalog).map_err(|e| e.to_string())
}

#[tauri::command]
fn data_dir_path() -> Result<String, String> {
    storage::app_data_dir().map(|p| p.display().to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        if let Err(err) = run_hotkey_lookup(app) {
                            let _ = app.emit("lookup-error", err);
                        }
                    }
                })
                .build(),
        )
        .setup(|app| {
            let settings = storage::load_settings().unwrap_or_default();
            if let Err(err) = register_hotkey(app.handle(), &settings.hotkey) {
                eprintln!("hotkey registration failed: {err}");
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_settings,
            save_settings,
            get_catalog,
            get_progress,
            set_pet_progress,
            lookup_text,
            lookup_pet_id,
            test_ocr_region,
            trigger_lookup,
            show_lookup_popup,
            open_region_picker,
            save_ocr_region,
            close_region_picker,
            import_screenshot,
            pick_image,
            pick_json,
            refresh_catalog,
            data_dir_path
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
