use crate::models::RegionBox;
use image::{GenericImageView, ImageEncoder, RgbaImage};
use std::io::Cursor;
use std::process::Command;
use xcap::Monitor;

pub fn capture_region_png(region: &RegionBox) -> Result<Vec<u8>, String> {
    if region.width == 0 || region.height == 0 {
        return Err("OCR region width/height must be > 0".into());
    }

    let monitors = Monitor::all().map_err(|e| format!("Monitor enumeration failed: {e}"))?;
    if monitors.is_empty() {
        return Err("No monitors found".into());
    }

    let monitor = monitors
        .iter()
        .find(|m| {
            let mx = m.x();
            let my = m.y();
            let mw = m.width() as i32;
            let mh = m.height() as i32;
            region.x >= mx && region.y >= my && region.x < mx + mw && region.y < my + mh
        })
        .or_else(|| monitors.iter().find(|m| m.is_primary()))
        .unwrap_or(&monitors[0]);

    let mx = monitor.x();
    let my = monitor.y();
    let local_x = (region.x - mx).max(0) as u32;
    let local_y = (region.y - my).max(0) as u32;

    let full = monitor
        .capture_image()
        .map_err(|e| format!("Screen capture failed: {e}"))?;

    let max_w = full.width().saturating_sub(local_x);
    let max_h = full.height().saturating_sub(local_y);
    let w = region.width.min(max_w);
    let h = region.height.min(max_h);
    if w == 0 || h == 0 {
        return Err("OCR region is outside the selected monitor".into());
    }

    let cropped: RgbaImage = full
        .view(local_x, local_y, w, h)
        .to_image();

    encode_rgba_png(cropped.width(), cropped.height(), cropped.as_raw())
}

fn encode_rgba_png(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut buf = Cursor::new(Vec::new());
    let encoder = image::codecs::png::PngEncoder::new(&mut buf);
    encoder
        .write_image(rgba, width, height, image::ExtendedColorType::Rgba8)
        .map_err(|e| format!("PNG encode failed: {e}"))?;
    Ok(buf.into_inner())
}

pub fn ocr_image_bytes(png: &[u8]) -> Result<String, String> {
    let dir = std::env::temp_dir();
    let path = dir.join(format!("aion2-pet-ocr-{}.png", std::process::id()));
    std::fs::write(&path, png).map_err(|e| e.to_string())?;

    let result = ocr_image_path(&path);
    let _ = std::fs::remove_file(&path);
    result
}

pub fn ocr_image_path(path: &std::path::Path) -> Result<String, String> {
    let output = Command::new("tesseract")
        .arg(path)
        .arg("stdout")
        .arg("-l")
        .arg("eng")
        .arg("--psm")
        .arg("6")
        .output()
        .map_err(|e| {
            format!(
                "Failed to run tesseract ({e}). Install Tesseract OCR and ensure it is on PATH."
            )
        })?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("tesseract failed: {err}"));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
