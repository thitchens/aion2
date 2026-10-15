mod matching;
mod models;
mod ocr;
mod storage;
mod tess_engine;

use matching::{
    build_lookup, looks_like_pet_insight, parse_insight_from_ocr, parse_progress_from_ocr,
};
use models::{Catalog, LookupResult, PetProgress, ProgressStore, RegionBox, Settings};
use serde::Serialize;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Mutex;
use std::time::Instant;
use tauri::window::Color;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

#[derive(Clone, Serialize)]
struct PopupPayload {
    result: LookupResult,
}

#[derive(Clone, Serialize)]
struct ImportResult {
    progress: ProgressStore,
    updated_names: Vec<String>,
}

#[derive(Clone, Serialize)]
struct RegionSavedPayload {
    kind: String,
    region: RegionBox,
}

/// Which saved rectangle the region picker is currently editing.
static PICKER_KIND: Mutex<&'static str> = Mutex::new("nameplate");

fn normalize_region_kind(kind: &str) -> Result<&'static str, String> {
    match kind {
        "nameplate" => Ok("nameplate"),
        "insight" => Ok("insight"),
        other => Err(format!(
            "Unknown region kind '{other}' (expected nameplate or insight)"
        )),
    }
}

fn set_picker_kind(kind: &str) -> Result<&'static str, String> {
    let kind = normalize_region_kind(kind)?;
    *PICKER_KIND.lock().map_err(|e| e.to_string())? = kind;
    Ok(kind)
}

fn get_picker_kind() -> Result<&'static str, String> {
    Ok(*PICKER_KIND.lock().map_err(|e| e.to_string())?)
}

fn region_for_kind<'a>(settings: &'a Settings, kind: &str) -> Result<&'a RegionBox, String> {
    match normalize_region_kind(kind)? {
        "insight" => Ok(&settings.insight_region),
        _ => Ok(&settings.ocr_region),
    }
}

fn region_for_kind_mut<'a>(settings: &'a mut Settings, kind: &str) -> Result<&'a mut RegionBox, String> {
    match normalize_region_kind(kind)? {
        "insight" => Ok(&mut settings.insight_region),
        _ => Ok(&mut settings.ocr_region),
    }
}

fn apply_progress_updates(
    catalog: &Catalog,
    updates: Vec<(String, PetProgress)>,
) -> Result<ImportResult, String> {
    if updates.is_empty() {
        return Err("No pets recognized.".into());
    }
    let mut progress = storage::load_progress()?;
    let mut updated_names = Vec::with_capacity(updates.len());
    for (id, pet_progress) in updates {
        let name = catalog
            .pets
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| id.clone());
        updated_names.push(format!("{name} (Lv {})", pet_progress.level));
        progress.pets.insert(id, pet_progress);
    }
    updated_names.sort();
    storage::save_progress(&progress)?;
    Ok(ImportResult {
        progress,
        updated_names,
    })
}

fn wire_popup_close_to_hide(popup: &WebviewWindow) {
    let hide_target = popup.clone();
    popup.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            // Closing would destroy the labeled window; hide so hotkeys keep working.
            api.prevent_close();
            let _ = hide_target.hide();
        }
    });
}

fn ensure_popup(app: &AppHandle) -> Result<WebviewWindow, String> {
    if let Some(popup) = app.get_webview_window("popup") {
        return Ok(popup);
    }
    let popup = WebviewWindowBuilder::new(app, "popup", WebviewUrl::App("popup.html".into()))
        .title("Pet Info")
        .inner_size(260.0, 160.0)
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .resizable(false)
        .build()
        .map_err(|e| e.to_string())?;
    let _ = popup.set_background_color(Some(Color(0, 0, 0, 0)));
    wire_popup_close_to_hide(&popup);
    Ok(popup)
}

fn show_popup(app: &AppHandle, result: LookupResult) -> Result<(), String> {
    let popup = ensure_popup(app)?;
    // Show without stealing focus so gameplay stays uninterrupted.
    popup.show().map_err(|e| e.to_string())?;
    let _ = popup.set_always_on_top(true);
    app.emit("lookup-result", PopupPayload { result })
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn lookup_ocr_region(settings: &Settings) -> Result<LookupResult, String> {
    let catalog = storage::load_catalog(settings)?;
    let progress = storage::load_progress()?;
    let total_start = Instant::now();

    let capture_start = Instant::now();
    let img = ocr::capture_region(&settings.ocr_region)?;
    let capture_ms = capture_start.elapsed().as_millis() as u64;

    let ocr_out = ocr::ocr_rgba_image(
        &img,
        &catalog,
        settings.ocr_debug_timings,
        settings.ocr_debug_images,
    )?;
    let mut result = build_lookup(
        &catalog,
        &progress,
        &settings.region_profile,
        &ocr_out.text,
    );

    if let Some(mut timings) = ocr_out.timings {
        timings.capture_ms = capture_ms;
        timings.total_ms = total_start.elapsed().as_millis() as u64;
        result.timings = Some(timings);
    }

    Ok(result)
}

fn popup_is_visible(app: &AppHandle) -> bool {
    app.get_webview_window("popup")
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false)
}

/// Same hotkey toggles: hide if the popup is already up, otherwise OCR + show.
fn run_hotkey_lookup(app: &AppHandle) -> Result<(), String> {
    if popup_is_visible(app) {
        if let Some(popup) = app.get_webview_window("popup") {
            popup.hide().map_err(|e| e.to_string())?;
        }
        return Ok(());
    }
    let settings = storage::load_settings()?;
    let result = lookup_ocr_region(&settings)?;
    show_popup(app, result)
}

fn run_hotkey_import(app: &AppHandle) {
    match import_insight_region() {
        Ok(result) => {
            let _ = app.emit("import-result", result);
        }
        Err(err) => {
            let _ = app.emit("import-error", err);
        }
    }
}

fn parse_hotkey(hotkey: &str) -> Result<Shortcut, String> {
    // Accept either Ctrl+Shift+P or CommandOrControl+Shift+P style.
    let normalized = hotkey
        .replace("Ctrl", "Control")
        .replace("cmdorcontrol", "CommandOrControl");
    Shortcut::from_str(&normalized)
        .or_else(|_| Shortcut::from_str(hotkey))
        .map_err(|e| format!("Invalid hotkey '{hotkey}': {e}"))
}

fn configured_hotkeys(settings: &Settings) -> Vec<(&'static str, &str)> {
    vec![
        ("lookup", settings.hotkey.as_str()),
        ("import", settings.hotkey_import.as_str()),
    ]
}

fn register_hotkeys(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let _ = app.global_shortcut().unregister_all();

    let mut seen: Vec<Shortcut> = Vec::new();
    for (action, hotkey) in configured_hotkeys(settings) {
        let hotkey = hotkey.trim();
        if hotkey.is_empty() {
            continue;
        }
        let shortcut = parse_hotkey(hotkey)?;
        if seen.iter().any(|s| s == &shortcut) {
            return Err(format!(
                "Hotkey '{hotkey}' is used more than once ({action}). Each action needs a unique shortcut."
            ));
        }
        app.global_shortcut()
            .register(shortcut)
            .map_err(|e| format!("Failed to register {action} hotkey '{hotkey}': {e}"))?;
        seen.push(shortcut);
    }
    Ok(())
}

fn dispatch_hotkey(app: &AppHandle, pressed: &Shortcut) {
    let settings = match storage::load_settings() {
        Ok(s) => s,
        Err(err) => {
            let _ = app.emit("lookup-error", err);
            return;
        }
    };

    for (action, hotkey) in configured_hotkeys(&settings) {
        let hotkey = hotkey.trim();
        if hotkey.is_empty() {
            continue;
        }
        let Ok(shortcut) = parse_hotkey(hotkey) else {
            continue;
        };
        if &shortcut != pressed {
            continue;
        }
        match action {
            "lookup" => {
                if let Err(err) = run_hotkey_lookup(app) {
                    let _ = app.emit("lookup-error", err);
                }
            }
            "import" => run_hotkey_import(app),
            _ => {}
        }
        return;
    }
}

#[tauri::command]
fn get_settings() -> Result<Settings, String> {
    storage::load_settings()
}

#[tauri::command]
fn save_settings(app: AppHandle, settings: Settings) -> Result<Settings, String> {
    storage::save_settings(&settings)?;
    register_hotkeys(&app, &settings)?;
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
fn set_pet_progress(pet_id: String, level: u32) -> Result<ProgressStore, String> {
    let settings = storage::load_settings()?;
    let catalog = storage::load_catalog(&settings)?;
    // Allow the highest max across profiles so Korea Lv5 survives while Global is selected.
    let abs_max = catalog
        .profiles
        .values()
        .map(|p| p.max_level)
        .max()
        .unwrap_or(5);
    let level = level.min(abs_max);
    let mut progress = storage::load_progress()?;
    progress.pets.insert(pet_id, PetProgress { level });
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
    lookup_ocr_region(&settings)
}

#[tauri::command]
fn trigger_lookup(app: AppHandle) -> Result<LookupResult, String> {
    let settings = storage::load_settings()?;
    let result = lookup_ocr_region(&settings)?;
    show_popup(&app, result.clone())?;
    Ok(result)
}

#[tauri::command]
fn show_lookup_popup(app: AppHandle, result: LookupResult) -> Result<(), String> {
    show_popup(&app, result)
}

#[derive(Clone, Serialize)]
struct PickerCoordSpace {
    /// Physical top-left of the picker client area (BitBlt / virtual-screen space).
    origin_x: i32,
    origin_y: i32,
    /// Logical CSS pixels → physical pixels (WebView scale factor).
    scale: f64,
}

fn picker_window(app: &AppHandle) -> Result<WebviewWindow, String> {
    app.get_webview_window("region-picker")
        .ok_or_else(|| "Region picker window missing".to_string())
}

fn picker_coord_space(picker: &WebviewWindow) -> Result<PickerCoordSpace, String> {
    // clientX/Y are relative to the inner (client) area, not outer chrome.
    let pos = picker.inner_position().map_err(|e| e.to_string())?;
    let scale = picker.scale_factor().map_err(|e| e.to_string())?;
    if scale <= 0.0 {
        return Err("invalid window scale factor".into());
    }
    Ok(PickerCoordSpace {
        origin_x: pos.x,
        origin_y: pos.y,
        scale,
    })
}

/// Convert picker logical/CSS client rect → physical screen rect for BitBlt.
fn client_region_to_screen(space: &PickerCoordSpace, client: &RegionBox) -> RegionBox {
    RegionBox {
        x: space.origin_x + (client.x as f64 * space.scale).round() as i32,
        y: space.origin_y + (client.y as f64 * space.scale).round() as i32,
        width: ((client.width as f64) * space.scale).round().max(1.0) as u32,
        height: ((client.height as f64) * space.scale).round().max(1.0) as u32,
    }
}

/// Convert saved physical screen rect → picker logical/CSS client rect for editing.
fn screen_region_to_client(space: &PickerCoordSpace, screen: &RegionBox) -> RegionBox {
    RegionBox {
        x: (((screen.x - space.origin_x) as f64) / space.scale).round() as i32,
        y: (((screen.y - space.origin_y) as f64) / space.scale).round() as i32,
        width: ((screen.width as f64) / space.scale).round().max(1.0) as u32,
        height: ((screen.height as f64) / space.scale).round().max(1.0) as u32,
    }
}

#[tauri::command]
fn open_region_picker(app: AppHandle, kind: String) -> Result<(), String> {
    let kind = set_picker_kind(&kind)?;
    let picker = picker_window(&app)?;

    let title = match kind {
        "insight" => "Select Pet Insight region",
        _ => "Select nameplate region",
    };
    let _ = picker.set_title(title);

    // Cover the monitor without exclusive fullscreen — fullscreen + transparent
    // often goes opaque black on Windows WebView2 / DWM.
    let _ = picker.set_fullscreen(false);
    let _ = picker.set_background_color(Some(Color(0, 0, 0, 0)));
    if let Ok(Some(monitor)) = picker.current_monitor() {
        let size = monitor.size();
        let pos = monitor.position();
        let _ = picker.set_position(PhysicalPosition::new(pos.x, pos.y));
        let _ = picker.set_size(PhysicalSize::new(size.width, size.height));
    } else {
        let _ = picker.maximize();
    }

    picker.show().map_err(|e| e.to_string())?;
    picker.set_focus().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
fn get_picker_coord_space(app: AppHandle) -> Result<PickerCoordSpace, String> {
    picker_coord_space(&picker_window(&app)?)
}

#[tauri::command]
fn get_picker_region_kind() -> Result<String, String> {
    Ok(get_picker_kind()?.to_string())
}

/// Current OCR region mapped into the picker window's logical client coordinates.
#[tauri::command]
fn get_picker_ocr_region(app: AppHandle) -> Result<Option<RegionBox>, String> {
    let kind = get_picker_kind()?;
    let settings = storage::load_settings()?;
    let r = region_for_kind(&settings, kind)?;
    if r.width == 0 || r.height == 0 {
        return Ok(None);
    }
    let space = picker_coord_space(&picker_window(&app)?)?;
    Ok(Some(screen_region_to_client(&space, r)))
}

/// `region` is in the region-picker's **logical client** coordinates (CSS pixels).
/// Converted to physical screen pixels using the picker window position + scale factor.
#[tauri::command]
fn save_ocr_region(app: AppHandle, region: RegionBox) -> Result<Settings, String> {
    if region.width == 0 || region.height == 0 {
        return Err("OCR region width/height must be > 0".into());
    }
    let kind = get_picker_kind()?;
    let space = picker_coord_space(&picker_window(&app)?)?;
    let screen = client_region_to_screen(&space, &region);

    let mut settings = storage::load_settings()?;
    *region_for_kind_mut(&mut settings, kind)? = screen.clone();
    storage::save_settings(&settings)?;
    let _ = app.emit(
        "region-saved",
        RegionSavedPayload {
            kind: kind.to_string(),
            region: screen.clone(),
        },
    );

    // When debug images are on, dump a full-monitor frame with the rectangle marked
    // so misaligned picker vs BitBlt coords are obvious.
    if settings.ocr_debug_images {
        if let Err(err) = ocr::dump_region_context(&screen) {
            eprintln!("region context dump failed: {err}");
        }
    }

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
fn import_screenshot(path: String) -> Result<ImportResult, String> {
    let settings = storage::load_settings()?;
    let catalog = storage::load_catalog(&settings)?;
    let raw = ocr::ocr_image_path(&PathBuf::from(&path))?;
    let updates = if looks_like_pet_insight(&raw) {
        parse_insight_from_ocr(&catalog, &raw)
            .map(|u| vec![u])
            .unwrap_or_default()
    } else {
        parse_progress_from_ocr(&catalog, &raw)
    };
    if updates.is_empty() {
        return Err(format!(
            "No pets recognized in screenshot OCR.\nRaw text:\n{raw}"
        ));
    }
    apply_progress_updates(&catalog, updates)
}

/// Capture the calibrated Pet Insight region and merge name + level into progress.
#[tauri::command]
fn import_insight_region() -> Result<ImportResult, String> {
    let settings = storage::load_settings()?;
    let catalog = storage::load_catalog(&settings)?;
    let region = &settings.insight_region;
    if region.width == 0 || region.height == 0 {
        return Err("Pet Insight region is not set — use Edit Pet Insight region first.".into());
    }
    let img = ocr::capture_region(region)?;
    let raw = ocr::ocr_rgba_general(&img, settings.ocr_debug_images)?;
    let Some(update) = parse_insight_from_ocr(&catalog, &raw) else {
        return Err(format!(
            "No pet recognized in Pet Insight OCR.\nRaw text:\n{raw}"
        ));
    };
    apply_progress_updates(&catalog, vec![update])
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
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        dispatch_hotkey(app, shortcut);
                    }
                })
                .build(),
        )
        .setup(|app| {
            if let Some(popup) = app.get_webview_window("popup") {
                let _ = popup.set_background_color(Some(Color(0, 0, 0, 0)));
                wire_popup_close_to_hide(&popup);
            }
            let settings = storage::load_settings().unwrap_or_default();
            if let Err(err) = register_hotkeys(app.handle(), &settings) {
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
            get_picker_coord_space,
            get_picker_region_kind,
            get_picker_ocr_region,
            save_ocr_region,
            close_region_picker,
            import_screenshot,
            import_insight_region,
            pick_image,
            pick_json,
            refresh_catalog,
            data_dir_path
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::{PickerCoordSpace, client_region_to_screen, screen_region_to_client};
    use crate::models::RegionBox;

    #[test]
    fn client_screen_roundtrip_at_125_scale() {
        let space = PickerCoordSpace {
            origin_x: 100,
            origin_y: 50,
            scale: 1.25,
        };
        let client = RegionBox {
            x: 80,
            y: 40,
            width: 200,
            height: 32,
        };
        let screen = client_region_to_screen(&space, &client);
        assert_eq!(screen.x, 100 + (80.0 * 1.25) as i32);
        assert_eq!(screen.y, 50 + (40.0 * 1.25) as i32);
        assert_eq!(screen.width, (200.0 * 1.25) as u32);
        assert_eq!(screen.height, (32.0 * 1.25) as u32);
        let back = screen_region_to_client(&space, &screen);
        assert_eq!(back.x, client.x);
        assert_eq!(back.y, client.y);
        assert_eq!(back.width, client.width);
        assert_eq!(back.height, client.height);
    }
}
