use crate::matching::{clean_ocr_text, match_pets};
use crate::models::{Catalog, OcrAttemptTiming, OcrTimings, RegionBox};
use crate::storage;
use image::{GrayImage, ImageEncoder, Luma, Rgba, RgbaImage, imageops};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;
use xcap::Monitor;

/// OCR text plus optional stage timings (when debug timings are enabled).
pub struct OcrOutput {
    pub text: String,
    pub timings: Option<OcrTimings>,
}

fn elapsed_ms(start: Instant) -> u64 {
    start.elapsed().as_millis() as u64
}

/// Fast-path upscale; escalate to [`UPSCALE_ESCALATED`] only when score stays weak.
const UPSCALE_FAST: u32 = 2;
const UPSCALE_ESCALATED: u32 = 3;

/// Assumed screen DPI before upscale; Tesseract gets `SCREEN_DPI * scale`.
const SCREEN_DPI: u32 = 96;

fn tesseract_dpi(scale: u32) -> u32 {
    SCREEN_DPI.saturating_mul(scale.max(1))
}

/// LSTM-only engine — nameplates are a single English line, not legacy layout.
const TESSERACT_OEM: &str = "1";

/// Characters that appear in English mob/pet nameplates (matches catalog charset).
const CHAR_WHITELIST: &str =
    "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789 .'-";

/// Paths to catalog-derived Tesseract vocabulary files (user-words / patterns).
struct CatalogVocab {
    words_path: PathBuf,
    patterns_path: PathBuf,
}

/// Last successful nameplate attempt — tried first on the next lookup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct OcrWinnerHint {
    variant: String,
    psm: String,
    whitelist: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct AttemptSpec {
    variant: String,
    psm: String,
    whitelist: bool,
}

/// Capture the configured OCR region as an in-memory RGBA buffer.
///
/// On Windows, captures only that rectangle (BitBlt). Elsewhere, falls back to
/// full-monitor capture + crop when the capture API has no region primitive.
pub fn capture_region(region: &RegionBox) -> Result<RgbaImage, String> {
    if region.width == 0 || region.height == 0 {
        return Err("OCR region width/height must be > 0".into());
    }

    #[cfg(windows)]
    {
        return capture_region_windows(region);
    }

    #[cfg(not(windows))]
    {
        capture_region_full_monitor_crop(region)
    }
}

#[cfg(windows)]
fn capture_region_windows(region: &RegionBox) -> Result<RgbaImage, String> {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC,
        GetDIBits, ReleaseDC, SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
        HGDIOBJ, SRCCOPY,
    };
    use windows::Win32::UI::WindowsAndMessaging::GetDesktopWindow;

    let width = region.width as i32;
    let height = region.height as i32;
    if width <= 0 || height <= 0 {
        return Err("OCR region width/height must be > 0".into());
    }

    unsafe {
        let hwnd: HWND = GetDesktopWindow();
        let hdc_screen = GetDC(hwnd);
        if hdc_screen.is_invalid() {
            return Err("Screen capture failed: GetDC".into());
        }

        let hdc_mem = CreateCompatibleDC(hdc_screen);
        if hdc_mem.is_invalid() {
            ReleaseDC(hwnd, hdc_screen);
            return Err("Screen capture failed: CreateCompatibleDC".into());
        }

        let hbmp = CreateCompatibleBitmap(hdc_screen, width, height);
        if hbmp.is_invalid() {
            let _ = DeleteDC(hdc_mem);
            ReleaseDC(hwnd, hdc_screen);
            return Err("Screen capture failed: CreateCompatibleBitmap".into());
        }

        let prev = SelectObject(hdc_mem, HGDIOBJ(hbmp.0));
        let blit_ok = BitBlt(
            hdc_mem,
            0,
            0,
            width,
            height,
            hdc_screen,
            region.x,
            region.y,
            SRCCOPY,
        )
        .is_ok();
        SelectObject(hdc_mem, prev);

        if !blit_ok {
            let _ = DeleteObject(hbmp);
            let _ = DeleteDC(hdc_mem);
            ReleaseDC(hwnd, hdc_screen);
            return Err("Screen capture failed: BitBlt".into());
        }

        let buffer_size = (width * height * 4) as usize;
        let mut buffer = vec![0u8; buffer_size];
        let mut bitmap_info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height, // top-down
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0 as u32,
                biSizeImage: buffer_size as u32,
                ..Default::default()
            },
            ..Default::default()
        };

        let lines = GetDIBits(
            hdc_mem,
            hbmp,
            0,
            height as u32,
            Some(buffer.as_mut_ptr().cast()),
            &mut bitmap_info,
            DIB_RGB_COLORS,
        );

        let _ = DeleteObject(hbmp);
        let _ = DeleteDC(hdc_mem);
        ReleaseDC(hwnd, hdc_screen);

        if lines == 0 {
            return Err("Screen capture failed: GetDIBits".into());
        }

        // BGRA → RGBA
        for px in buffer.chunks_exact_mut(4) {
            px.swap(0, 2);
            if px[3] == 0 {
                px[3] = 255;
            }
        }

        RgbaImage::from_raw(region.width, region.height, buffer)
            .ok_or_else(|| "Screen capture failed: invalid RGBA buffer".into())
    }
}

#[cfg(not(windows))]
fn capture_region_full_monitor_crop(region: &RegionBox) -> Result<RgbaImage, String> {
    use image::GenericImageView;

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

    Ok(full.view(local_x, local_y, w, h).to_image())
}

fn encode_rgba_png(img: &RgbaImage) -> Result<Vec<u8>, String> {
    let mut buf = Cursor::new(Vec::new());
    let encoder = image::codecs::png::PngEncoder::new(&mut buf);
    encoder
        .write_image(
            img.as_raw(),
            img.width(),
            img.height(),
            image::ExtendedColorType::Rgba8,
        )
        .map_err(|e| format!("PNG encode failed: {e}"))?;
    Ok(buf.into_inner())
}

fn encode_gray_png(img: &GrayImage) -> Result<Vec<u8>, String> {
    let mut buf = Cursor::new(Vec::new());
    let encoder = image::codecs::png::PngEncoder::new(&mut buf);
    encoder
        .write_image(
            img.as_raw(),
            img.width(),
            img.height(),
            image::ExtendedColorType::L8,
        )
        .map_err(|e| format!("PNG encode failed: {e}"))?;
    Ok(buf.into_inner())
}

fn rgb_to_hsv(r: u8, g: u8, b: u8) -> (f32, f32, f32) {
    let r = r as f32 / 255.0;
    let g = g as f32 / 255.0;
    let b = b as f32 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let d = max - min;
    let h = if d < 1e-6 {
        0.0
    } else if (max - r).abs() < 1e-6 {
        60.0 * (((g - b) / d) % 6.0)
    } else if (max - g).abs() < 1e-6 {
        60.0 * (((b - r) / d) + 2.0)
    } else {
        60.0 * (((r - g) / d) + 4.0)
    };
    let h = if h < 0.0 { h + 360.0 } else { h };
    let s = if max < 1e-6 { 0.0 } else { d / max };
    (h, s, max)
}

/// Continuous 0..1 score for AION 2 gold/yellow nameplate ink.
fn yellow_affinity(r: u8, g: u8, b: u8) -> f32 {
    let (h, s, v) = rgb_to_hsv(r, g, b);
    // Also accept bright near-white-yellow (low sat, high value, R≈G≫B).
    let rgb_gold = {
        let r = r as f32;
        let g = g as f32;
        let b = b as f32;
        let mean_rg = (r + g) * 0.5;
        // Gold needs both R and G high and similar; hostile red must not qualify here.
        if mean_rg < 90.0 || g < 80.0 || (r - g).abs() > 70.0 || b >= mean_rg - 15.0 {
            0.0
        } else {
            ((mean_rg - b) / 180.0).clamp(0.0, 1.0) * (mean_rg / 255.0)
        }
    };
    let hue_ok = if (25.0..=85.0).contains(&h) {
        1.0 - ((h - 50.0).abs() / 35.0).min(1.0)
    } else {
        0.0
    };
    let hsv = hue_ok * s.clamp(0.0, 1.0) * v.clamp(0.0, 1.0);
    hsv.max(rgb_gold)
}

/// Continuous 0..1 score for hostile/enemy red nameplate ink (sans-serif UI text).
fn red_affinity(r: u8, g: u8, b: u8) -> f32 {
    let (h, s, v) = rgb_to_hsv(r, g, b);
    let rf = r as f32;
    let gf = g as f32;
    let bf = b as f32;
    let rgb_red = {
        // Strong R with G/B clearly lower — typical attackable-target red.
        if rf < 120.0 || rf < gf + 35.0 || rf < bf + 35.0 || gf > 140.0 {
            0.0
        } else {
            let dominance = ((rf - gf.max(bf)) / 160.0).clamp(0.0, 1.0);
            dominance * (rf / 255.0) * s.clamp(0.15, 1.0)
        }
    };
    // HSV red wraps across 0° / 360°.
    let hue_dist = if h <= 180.0 { h } else { 360.0 - h };
    let hue_ok = if hue_dist <= 22.0 {
        1.0 - (hue_dist / 22.0)
    } else {
        0.0
    };
    let hsv = hue_ok * s.clamp(0.0, 1.0) * v.clamp(0.0, 1.0);
    hsv.max(rgb_red)
}

/// Nameplate ink affinity: yellow (neutral/friendly) or red (hostile).
fn nameplate_ink_affinity(r: u8, g: u8, b: u8) -> f32 {
    yellow_affinity(r, g, b).max(red_affinity(r, g, b))
}

fn is_nameplate_ink(r: u8, g: u8, b: u8) -> bool {
    nameplate_ink_affinity(r, g, b) >= 0.18
}

fn upscale(img: &GrayImage, filter: imageops::FilterType, scale: u32) -> GrayImage {
    let scale = scale.max(1);
    imageops::resize(
        img,
        img.width().saturating_mul(scale).max(1),
        img.height().saturating_mul(scale).max(1),
        filter,
    )
}

/// Soft ink extract: yellow/red nameplate pixels → dark ink, rest white. Keeps AA edges.
/// Variant id stays `*-yellow-*` for winner-hint compatibility.
fn soft_yellow(img: &RgbaImage) -> GrayImage {
    let (w, h) = img.dimensions();
    let mut out = GrayImage::new(w, h);
    for (x, y, pixel) in img.enumerate_pixels() {
        let image::Rgba([r, g, b, _a]) = *pixel;
        let a = nameplate_ink_affinity(r, g, b);
        // Strong nameplate color → black, weak → gray, none → white.
        let ink = if a < 0.08 {
            255
        } else {
            (255.0 * (1.0 - a.clamp(0.0, 1.0))).round() as u8
        };
        out.put_pixel(x, y, Luma([ink]));
    }
    out
}

fn hard_yellow(img: &RgbaImage) -> GrayImage {
    let (w, h) = img.dimensions();
    let mut mask = GrayImage::new(w, h);
    for (x, y, pixel) in img.enumerate_pixels() {
        let image::Rgba([r, g, b, _a]) = *pixel;
        let v = if is_nameplate_ink(r, g, b) { 0 } else { 255 };
        mask.put_pixel(x, y, Luma([v]));
    }
    mask
}

fn grayscale_luma(img: &RgbaImage) -> GrayImage {
    let (w, h) = img.dimensions();
    let mut out = GrayImage::new(w, h);
    for (x, y, pixel) in img.enumerate_pixels() {
        let image::Rgba([r, g, b, _a]) = *pixel;
        let gray = ((r as u32 * 77 + g as u32 * 150 + b as u32 * 29) >> 8) as u8;
        // Invert so bright nameplate text becomes dark ink.
        out.put_pixel(x, y, Luma([255 - gray]));
    }
    out
}

fn grayscale_bright_bin(img: &RgbaImage) -> GrayImage {
    let (w, h) = img.dimensions();
    let mut out = GrayImage::new(w, h);
    for (x, y, pixel) in img.enumerate_pixels() {
        let image::Rgba([r, g, b, _a]) = *pixel;
        let gray = ((r as u32 * 77 + g as u32 * 150 + b as u32 * 29) >> 8) as u8;
        let v = if gray >= 150 { 0 } else { 255 };
        out.put_pixel(x, y, Luma([v]));
    }
    out
}

/// Otsu threshold for an 8-bit grayscale image (maximizes between-class variance).
fn otsu_threshold(img: &GrayImage) -> u8 {
    let mut hist = [0u32; 256];
    for p in img.pixels() {
        hist[p.0[0] as usize] += 1;
    }
    let total = img.width().saturating_mul(img.height()) as f64;
    if total <= 0.0 {
        return 128;
    }
    let mut sum_all = 0.0_f64;
    for (i, &c) in hist.iter().enumerate() {
        sum_all += i as f64 * c as f64;
    }
    let mut sum_bg = 0.0_f64;
    let mut weight_bg = 0.0_f64;
    let mut best_var = -1.0_f64;
    let mut best_t = 128u8;
    for t in 0..256 {
        weight_bg += hist[t] as f64;
        if weight_bg <= 0.0 {
            continue;
        }
        let weight_fg = total - weight_bg;
        if weight_fg <= 0.0 {
            break;
        }
        sum_bg += t as f64 * hist[t] as f64;
        let mean_bg = sum_bg / weight_bg;
        let mean_fg = (sum_all - sum_bg) / weight_fg;
        let var = weight_bg * weight_fg * (mean_bg - mean_fg).powi(2);
        if var > best_var {
            best_var = var;
            best_t = t as u8;
        }
    }
    best_t
}

/// Soft-yellow then Otsu binarize (fallback ladder).
fn yellow_otsu(img: &RgbaImage) -> GrayImage {
    let soft = soft_yellow(img);
    let t = otsu_threshold(&soft);
    let (w, h) = soft.dimensions();
    let mut out = GrayImage::new(w, h);
    for (x, y, pixel) in soft.enumerate_pixels() {
        // Soft-yellow uses dark ink on white; keep ink (below threshold) black.
        let v = if pixel.0[0] <= t { 0 } else { 255 };
        out.put_pixel(x, y, Luma([v]));
    }
    out
}

/// Light 4-connected dilate of dark ink to thicken thin strokes (fallback).
fn thicken_dark_ink(img: &GrayImage) -> GrayImage {
    let (w, h) = img.dimensions();
    let mut out = img.clone();
    let is_ink = |x: u32, y: u32| img.get_pixel(x, y).0[0] < 160;
    for y in 0..h {
        for x in 0..w {
            if is_ink(x, y) {
                continue;
            }
            let mut neighbor = false;
            if x > 0 && is_ink(x - 1, y) {
                neighbor = true;
            } else if x + 1 < w && is_ink(x + 1, y) {
                neighbor = true;
            } else if y > 0 && is_ink(x, y - 1) {
                neighbor = true;
            } else if y + 1 < h && is_ink(x, y + 1) {
                neighbor = true;
            }
            if neighbor {
                out.put_pixel(x, y, Luma([0]));
            }
        }
    }
    out
}

fn yellow_stroke(img: &RgbaImage) -> GrayImage {
    thicken_dark_ink(&soft_yellow(img))
}

/// Crop vertically to the colored name-ink band (drops level chrome above/below).
fn crop_name_band(img: &RgbaImage) -> RgbaImage {
    let (w, h) = img.dimensions();
    if h <= 4 {
        return img.clone();
    }
    let mut row_energy = vec![0.0_f32; h as usize];
    for y in 0..h {
        let mut e = 0.0_f32;
        for x in 0..w {
            let image::Rgba([r, g, b, _]) = *img.get_pixel(x, y);
            e += nameplate_ink_affinity(r, g, b);
        }
        row_energy[y as usize] = e;
    }
    let max_e = row_energy.iter().cloned().fold(0.0_f32, f32::max);
    let (y0, y1) = if max_e < 1e-3 {
        let y0 = h / 4;
        let y1 = (h - h / 4).max(y0 + 1);
        (y0, y1)
    } else {
        let thresh = max_e * 0.15;
        let first = row_energy
            .iter()
            .position(|&e| e >= thresh)
            .unwrap_or(0);
        let last = row_energy
            .iter()
            .rposition(|&e| e >= thresh)
            .unwrap_or(h as usize - 1);
        let pad = 1usize;
        let y0 = first.saturating_sub(pad) as u32;
        let y1 = ((last + 1 + pad) as u32).min(h).max(y0 + 1);
        (y0, y1)
    };
    imageops::crop_imm(img, 0, y0, w, y1 - y0).to_image()
}

fn nameband_soft(img: &RgbaImage) -> GrayImage {
    soft_yellow(&crop_name_band(img))
}

fn debug_dir() -> Result<std::path::PathBuf, String> {
    let dir = storage::app_data_dir()?.join("ocr-debug");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// Only resolve/create the OCR debug dir when image dumps are explicitly enabled.
fn debug_images_dir(enabled: bool) -> Option<std::path::PathBuf> {
    if !enabled {
        return None;
    }
    debug_dir().ok()
}

/// Capture the monitor containing `region` and draw the OCR rectangle for alignment checks.
/// Writes `ocr-debug/region-context.png` (top band + full annotated frame when small enough).
pub fn dump_region_context(region: &RegionBox) -> Result<PathBuf, String> {
    use image::GenericImageView;

    let dir = debug_dir()?;
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
            region.x + region.width as i32 > mx
                && region.y + region.height as i32 > my
                && region.x < mx + mw
                && region.y < my + mh
        })
        .or_else(|| monitors.iter().find(|m| m.is_primary()))
        .unwrap_or(&monitors[0]);

    let mx = monitor.x();
    let my = monitor.y();
    let mut full = monitor
        .capture_image()
        .map_err(|e| format!("Context capture failed: {e}"))?;

    let local_x = (region.x - mx).clamp(0, full.width().saturating_sub(1) as i32) as u32;
    let local_y = (region.y - my).clamp(0, full.height().saturating_sub(1) as i32) as u32;
    let rw = region.width.min(full.width().saturating_sub(local_x)).max(1);
    let rh = region.height.min(full.height().saturating_sub(local_y)).max(1);

    stroke_rect(
        &mut full,
        local_x,
        local_y,
        rw,
        rh,
        Rgba([255, 64, 64, 255]),
        3,
    );

    // Top strip around the region for a quick glance without opening a 1440p PNG.
    let band_y0 = local_y.saturating_sub(40);
    let band_y1 = (local_y + rh + 40).min(full.height());
    let band = full.view(0, band_y0, full.width(), band_y1.saturating_sub(band_y0).max(1));
    let band_png = encode_rgba_png(&band.to_image())?;
    let band_path = dir.join("region-context-top.png");
    std::fs::write(&band_path, band_png).map_err(|e| e.to_string())?;

    let full_png = encode_rgba_png(&full)?;
    let full_path = dir.join("region-context.png");
    std::fs::write(&full_path, full_png).map_err(|e| e.to_string())?;
    Ok(full_path)
}

fn stroke_rect(img: &mut RgbaImage, x: u32, y: u32, w: u32, h: u32, color: Rgba<u8>, thickness: u32) {
    if w == 0 || h == 0 {
        return;
    }
    let x1 = x.saturating_add(w).min(img.width());
    let y1 = y.saturating_add(h).min(img.height());
    let t = thickness.max(1);
    for dy in 0..t {
        let yt = y.saturating_add(dy);
        let yb = y1.saturating_sub(1).saturating_sub(dy);
        if yt < img.height() {
            for xi in x..x1 {
                img.put_pixel(xi, yt, color);
            }
        }
        if yb < img.height() {
            for xi in x..x1 {
                img.put_pixel(xi, yb, color);
            }
        }
    }
    for dx in 0..t {
        let xl = x.saturating_add(dx);
        let xr = x1.saturating_sub(1).saturating_sub(dx);
        if xl < img.width() {
            for yi in y..y1 {
                img.put_pixel(xl, yi, color);
            }
        }
        if xr < img.width() {
            for yi in y..y1 {
                img.put_pixel(xr, yi, color);
            }
        }
    }
}

fn write_debug_gray(dir: &std::path::Path, name: &str, img: &GrayImage) {
    if let Ok(png) = encode_gray_png(img) {
        let _ = std::fs::write(dir.join(name), png);
    }
}

/// Unique space-free tokens from pet names + source mobs (Tesseract user-words).
fn catalog_user_words(catalog: &Catalog) -> String {
    let mut words = BTreeSet::new();
    for pet in &catalog.pets {
        for phrase in std::iter::once(pet.name.as_str()).chain(pet.source_mobs.iter().map(String::as_str))
        {
            for token in phrase.split_whitespace() {
                if !token.is_empty() {
                    words.insert(token.to_string());
                }
            }
        }
    }
    let mut out = words.into_iter().collect::<Vec<_>>().join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

/// Lightweight patterns for Title Case / hyphenated English mob names.
fn catalog_user_patterns() -> &'static str {
    // Keep the set small — overly generic patterns slow LSTM decoding.
    "\\A\\a+\n\\A\\a+-\\A\\a+\n\\A\\a+'\\a+\n"
}

fn vocab_dir() -> Result<PathBuf, String> {
    let dir = storage::app_data_dir()?.join("ocr-vocab");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

/// Write catalog vocab files when contents change; reuse paths across ladder attempts.
fn ensure_catalog_vocab(catalog: &Catalog) -> Result<CatalogVocab, String> {
    let dir = vocab_dir()?;
    let words_path = dir.join("user-words.txt");
    let patterns_path = dir.join("user-patterns.txt");
    let words = catalog_user_words(catalog);
    let patterns = catalog_user_patterns();

    write_if_changed(&words_path, words.as_bytes())?;
    write_if_changed(&patterns_path, patterns.as_bytes())?;

    Ok(CatalogVocab {
        words_path,
        patterns_path,
    })
}

fn write_if_changed(path: &Path, contents: &[u8]) -> Result<(), String> {
    let needs_write = match std::fs::read(path) {
        Ok(existing) => existing != contents,
        Err(_) => true,
    };
    if needs_write {
        std::fs::write(path, contents).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn vocab_paths(vocab: Option<&CatalogVocab>) -> Option<crate::tess_engine::VocabPaths<'_>> {
    vocab.map(|v| crate::tess_engine::VocabPaths {
        words: &v.words_path,
        patterns: &v.patterns_path,
    })
}

/// Nameplate OCR via resident in-process Tesseract when available, else CLI.
fn run_nameplate_ocr(
    img: &GrayImage,
    psm: &str,
    whitelist: bool,
    dpi: u32,
    vocab: Option<&CatalogVocab>,
) -> Result<String, String> {
    crate::tess_engine::ocr_gray(
        img,
        psm,
        whitelist,
        dpi,
        vocab_paths(vocab).as_ref(),
        CHAR_WHITELIST,
        TESSERACT_OEM,
    )
}

fn run_tesseract_path(path: &Path, psm: &str, whitelist: bool) -> Result<String, String> {
    // Pets-page screenshots: no catalog bias, no nameplate DPI (image is not upscaled).
    let mut cmd = Command::new("tesseract");
    cmd.arg(path)
        .arg("stdout")
        .arg("-l")
        .arg("eng")
        .arg("--psm")
        .arg(psm)
        .arg("--oem")
        .arg(TESSERACT_OEM);
    if whitelist {
        cmd.arg("-c")
            .arg(format!("tessedit_char_whitelist={CHAR_WHITELIST}"));
    }
    let output = cmd.output().map_err(|e| {
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

/// Stop the ladder when catalog match is clearly decisive (≥ 0.95, including
/// exact normalized name/source-mob matches at 1.0). Non-decisive scores —
/// including weak reads below ~0.85 — continue through the full ladder.
const DECISIVE_SCORE: f64 = 0.95;

/// Below this after a scale phase, escalate (fallbacks / 3× upscale).
const WEAK_SCORE: f64 = 0.85;

/// Preferred hot path: soft yellow/red ink + single-line PSM + whitelist.
/// AION 2 nameplates are sans-serif; color is yellow or hostile red.
const PREFERRED_VARIANT: &str = "01-yellow-soft";
const PREFERRED_PSM: &str = "7";
const PREFERRED_WHITELIST: bool = true;

const BASE_VARIANTS: &[&str] = &[
    "01-yellow-soft", // yellow + red ink extract
    "02-yellow-hard",
    "03-gray-invert", // color-agnostic bright sans-serif
    "04-bright-bin",
];

/// Hard-plate fallbacks — only after the base ladder stays weak.
const FALLBACK_VARIANTS: &[&str] = &[
    "05-yellow-otsu",
    "06-yellow-stroke",
    "07-nameband-soft",
];

const PSM_CONFIGS: &[(&str, bool)] = &[("7", true), ("7", false), ("6", false)];

fn catalog_score(catalog: &Catalog, raw: &str) -> f64 {
    if raw.trim().is_empty() {
        return 0.0;
    }
    let cleaned = clean_ocr_text(catalog, raw);
    if cleaned.is_empty() {
        return 0.0;
    }
    match_pets(catalog, &cleaned, 1)
        .first()
        .map(|c| c.score)
        .unwrap_or(0.0)
}

/// Decisive = strong catalog score or exact normalized name/source-mob match (score 1.0).
fn is_decisive_score(score: f64) -> bool {
    score >= DECISIVE_SCORE
}

fn is_weak_score(score: f64) -> bool {
    score < WEAK_SCORE
}

fn is_known_variant(name: &str) -> bool {
    BASE_VARIANTS.contains(&name) || FALLBACK_VARIANTS.contains(&name)
}

fn winner_hint_path() -> Result<PathBuf, String> {
    Ok(storage::app_data_dir()?.join("ocr-winner.json"))
}

fn load_winner_hint() -> Option<OcrWinnerHint> {
    let path = winner_hint_path().ok()?;
    let raw = std::fs::read_to_string(path).ok()?;
    let hint: OcrWinnerHint = serde_json::from_str(&raw).ok()?;
    if is_known_variant(&hint.variant) && PSM_CONFIGS.iter().any(|(p, w)| *p == hint.psm && *w == hint.whitelist)
    {
        Some(hint)
    } else {
        None
    }
}

fn save_winner_hint(hint: &OcrWinnerHint) {
    if let Ok(path) = winner_hint_path() {
        if let Ok(raw) = serde_json::to_string_pretty(hint) {
            let _ = std::fs::write(path, raw);
        }
    }
}

fn push_unique_attempt(plan: &mut Vec<AttemptSpec>, spec: AttemptSpec) {
    if !plan.iter().any(|s| s == &spec) {
        plan.push(spec);
    }
}

fn expand_variants(variants: &[&str]) -> Vec<AttemptSpec> {
    let mut plan = Vec::new();
    for variant in variants {
        for &(psm, whitelist) in PSM_CONFIGS {
            push_unique_attempt(
                &mut plan,
                AttemptSpec {
                    variant: (*variant).into(),
                    psm: psm.into(),
                    whitelist,
                },
            );
        }
    }
    plan
}

/// Base ladder: learned winner (if any), then preferred, then remaining base configs.
fn attempt_plan_base(hint: Option<&OcrWinnerHint>) -> Vec<AttemptSpec> {
    let mut plan = Vec::new();
    if let Some(h) = hint {
        push_unique_attempt(
            &mut plan,
            AttemptSpec {
                variant: h.variant.clone(),
                psm: h.psm.clone(),
                whitelist: h.whitelist,
            },
        );
    }
    push_unique_attempt(
        &mut plan,
        AttemptSpec {
            variant: PREFERRED_VARIANT.into(),
            psm: PREFERRED_PSM.into(),
            whitelist: PREFERRED_WHITELIST,
        },
    );
    for spec in expand_variants(BASE_VARIANTS) {
        push_unique_attempt(&mut plan, spec);
    }
    plan
}

fn attempt_plan_fallback() -> Vec<AttemptSpec> {
    expand_variants(FALLBACK_VARIANTS)
}

fn prepare_variant(
    img: &RgbaImage,
    name: &str,
) -> Result<(GrayImage, imageops::FilterType), String> {
    match name {
        "01-yellow-soft" => Ok((soft_yellow(img), imageops::FilterType::Triangle)),
        "02-yellow-hard" => Ok((hard_yellow(img), imageops::FilterType::Nearest)),
        "03-gray-invert" => Ok((grayscale_luma(img), imageops::FilterType::Triangle)),
        "04-bright-bin" => Ok((grayscale_bright_bin(img), imageops::FilterType::Nearest)),
        "05-yellow-otsu" => Ok((yellow_otsu(img), imageops::FilterType::Nearest)),
        "06-yellow-stroke" => Ok((yellow_stroke(img), imageops::FilterType::Triangle)),
        "07-nameband-soft" => Ok((nameband_soft(img), imageops::FilterType::Triangle)),
        other => Err(format!("unknown OCR variant: {other}")),
    }
}

/// OCR an in-memory nameplate RGBA buffer (hot path — no PNG decode).
///
/// Adaptive ladder: learned winner → preferred soft yellow/red ink at 2× → base →
/// (if still weak) Otsu/stroke/name-band fallbacks → (if still weak) escalate to 3×.
/// Early-exits on a decisive catalog match. Uses a resident in-process Tesseract when
/// loadable (else CLI), with LSTM OEM, scale-matched DPI, and catalog user-words.
///
/// `debug_timings` and `debug_images` are independent: timings never force disk writes.
pub fn ocr_rgba_image(
    img: &RgbaImage,
    catalog: &Catalog,
    debug_timings: bool,
    debug_images: bool,
) -> Result<OcrOutput, String> {
    ocr_rgba_image_inner(img, catalog, debug_timings, debug_images, 0)
}

/// Decode a PNG then OCR (fixture / legacy seam). Prefer [`ocr_rgba_image`] on the hot path.
#[allow(dead_code)]
pub fn ocr_image_bytes(
    png: &[u8],
    catalog: &Catalog,
    debug_timings: bool,
    debug_images: bool,
) -> Result<OcrOutput, String> {
    let decode_start = Instant::now();
    let img = image::load_from_memory(png)
        .map_err(|e| format!("Failed to decode capture: {e}"))?
        .to_rgba8();
    let decode_ms = elapsed_ms(decode_start);
    ocr_rgba_image_inner(&img, catalog, debug_timings, debug_images, decode_ms)
}

struct LadderState<'a> {
    img: &'a RgbaImage,
    catalog: &'a Catalog,
    debug_timings: bool,
    dbg: &'a Option<PathBuf>,
    vocab: Option<&'a CatalogVocab>,
    best_text: String,
    best_score: f64,
    best_spec: Option<AttemptSpec>,
    log: String,
    attempts: Vec<OcrAttemptTiming>,
    tesseract_ms_total: u64,
    score_ms_total: u64,
    preprocess_ms: u64,
    /// Cache key: "variant@scale" → upscaled gray (fed to resident/CLI Tesseract).
    prepared_cache: std::collections::HashMap<String, GrayImage>,
}

impl LadderState<'_> {
    fn run_plan(&mut self, plan: &[AttemptSpec], scale: u32) -> Result<bool, String> {
        let dpi = tesseract_dpi(scale);
        for spec in plan {
            let cache_key = format!("{}@{scale}", spec.variant);
            if !self.prepared_cache.contains_key(&cache_key) {
                let prep_start = Instant::now();
                let (prepared, filter) = prepare_variant(self.img, &spec.variant)?;
                let up = upscale(&prepared, filter, scale);
                self.preprocess_ms += elapsed_ms(prep_start);
                if let Some(d) = self.dbg {
                    write_debug_gray(d, &format!("{cache_key}.png"), &up);
                }
                self.prepared_cache.insert(cache_key.clone(), up);
            }
            let prepared_gray = self.prepared_cache.get(&cache_key).unwrap();

            let tess_start = Instant::now();
            let text = match run_nameplate_ocr(
                prepared_gray,
                &spec.psm,
                spec.whitelist,
                dpi,
                self.vocab,
            ) {
                Ok(t) => t,
                Err(e) => {
                    let ms = elapsed_ms(tess_start);
                    self.tesseract_ms_total += ms;
                    self.log.push_str(&format!(
                        "{} psm{} wl={} x{scale}: ERR {e} ({ms}ms)\n",
                        spec.variant, spec.psm, spec.whitelist
                    ));
                    if self.debug_timings {
                        self.attempts.push(OcrAttemptTiming {
                            variant: spec.variant.clone(),
                            psm: spec.psm.clone(),
                            whitelist: spec.whitelist,
                            upscale: scale,
                            tesseract_ms: ms,
                            score: -1.0,
                        });
                    }
                    continue;
                }
            };
            let ms = elapsed_ms(tess_start);
            self.tesseract_ms_total += ms;

            let score_start = Instant::now();
            let score = catalog_score(self.catalog, &text);
            let score_ms = elapsed_ms(score_start);
            self.score_ms_total += score_ms;

            self.log.push_str(&format!(
                "{} psm{} wl={} x{scale}: score={score:.3} text={text:?} ({ms}ms)\n",
                spec.variant, spec.psm, spec.whitelist
            ));
            if self.debug_timings {
                self.attempts.push(OcrAttemptTiming {
                    variant: spec.variant.clone(),
                    psm: spec.psm.clone(),
                    whitelist: spec.whitelist,
                    upscale: scale,
                    tesseract_ms: ms,
                    score,
                });
            }
            if score > self.best_score
                || (score == self.best_score
                    && text.chars().count() > self.best_text.chars().count())
            {
                self.best_score = score;
                self.best_text = text;
                self.best_spec = Some(spec.clone());
            }

            if is_decisive_score(self.best_score) {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

fn ocr_rgba_image_inner(
    img: &RgbaImage,
    catalog: &Catalog,
    debug_timings: bool,
    debug_images: bool,
    decode_ms: u64,
) -> Result<OcrOutput, String> {
    let total_start = Instant::now();

    let dbg = debug_images_dir(debug_images);
    if let Some(dir) = &dbg {
        if let Ok(raw) = encode_rgba_png(img) {
            let _ = std::fs::write(dir.join("00-raw.png"), raw);
        }
    }

    let vocab = ensure_catalog_vocab(catalog).ok();
    let hint = load_winner_hint();
    let base_plan = attempt_plan_base(hint.as_ref());
    let fallback_plan = attempt_plan_fallback();

    let mut state = LadderState {
        img,
        catalog,
        debug_timings,
        dbg: &dbg,
        vocab: vocab.as_ref(),
        best_text: String::new(),
        best_score: -1.0_f64,
        best_spec: None,
        log: String::new(),
        attempts: Vec::new(),
        tesseract_ms_total: 0,
        score_ms_total: 0,
        preprocess_ms: 0,
        prepared_cache: std::collections::HashMap::new(),
    };

    // Phase 1: base ladder at 2× (winner → preferred → rest).
    let mut done = state.run_plan(&base_plan, UPSCALE_FAST)?;
    // Phase 2: adaptive fallbacks at 2× only while still weak.
    if !done && is_weak_score(state.best_score) {
        done = state.run_plan(&fallback_plan, UPSCALE_FAST)?;
    }
    // Phase 3: escalate to 3× when still weak after the 2× ladder.
    if !done && is_weak_score(state.best_score) {
        let mut escalated = base_plan.clone();
        for spec in &fallback_plan {
            push_unique_attempt(&mut escalated, spec.clone());
        }
        let _ = state.run_plan(&escalated, UPSCALE_ESCALATED)?;
    }

    if let Some(spec) = &state.best_spec {
        if state.best_score >= WEAK_SCORE {
            save_winner_hint(&OcrWinnerHint {
                variant: spec.variant.clone(),
                psm: spec.psm.clone(),
                whitelist: spec.whitelist,
            });
        }
    }

    let timings = if debug_timings {
        Some(OcrTimings {
            capture_ms: 0, // filled by caller
            decode_ms,
            preprocess_ms: state.preprocess_ms,
            tesseract_ms: state.tesseract_ms_total,
            score_ms: state.score_ms_total,
            total_ms: elapsed_ms(total_start),
            attempts: state.attempts,
        })
    } else {
        None
    };

    if let Some(d) = &dbg {
        let mut report = format!(
            "best_score={:.3}\nbest={:?}\n\n{}",
            state.best_score, state.best_text, state.log
        );
        if let Some(t) = &timings {
            report.push_str(&format!(
                "\ntimings: decode={}ms preprocess={}ms tesseract={}ms score={}ms ocr_total={}ms\n",
                t.decode_ms, t.preprocess_ms, t.tesseract_ms, t.score_ms, t.total_ms
            ));
        }
        let _ = std::fs::write(d.join("last-run.txt"), report);
    }

    if state.best_text.is_empty() {
        return Err("OCR produced empty text for all variants".into());
    }
    Ok(OcrOutput {
        text: state.best_text,
        timings,
    })
}

/// OCR an arbitrary image path (Pets-page / Pet Insight — no yellow filter).
pub fn ocr_image_path(path: &std::path::Path) -> Result<String, String> {
    run_tesseract_path(path, "6", false)
}

/// OCR a captured screen region with general (non-nameplate) settings.
/// Used for Pet Insight: white UI text on dark panels, multi-line name + level.
pub fn ocr_rgba_general(img: &RgbaImage, debug_images: bool) -> Result<String, String> {
    let dir = crate::storage::app_data_dir()?.join("ocr-debug");
    if debug_images {
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let raw_path = dir.join("insight-raw.png");
        img.save(&raw_path)
            .map_err(|e| format!("Failed to write insight debug image: {e}"))?;
    }

    let tmp = std::env::temp_dir().join(format!(
        "pet-tracker-insight-{}.png",
        std::process::id()
    ));
    img.save(&tmp)
        .map_err(|e| format!("Failed to write temporary insight capture: {e}"))?;
    let result = ocr_image_path(&tmp);
    let _ = std::fs::remove_file(&tmp);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Pet, RegionProfile};
    use image::Rgba;
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
            pets: vec![
                Pet {
                    id: "manduri-fighter".into(),
                    name: "Manduri Fighter".into(),
                    genus: "Cogni".into(),
                    source_mobs: vec!["Wandering Manduri Fighter".into()],
                    source_level: Some(27),
                    zone: None,
                    spawn_areas: vec![],
                    owned_effects: HashMap::new(),
                    notes: String::new(),
                },
                Pet {
                    id: "yellow-tailed-skama".into(),
                    name: "Yellow-Tailed Skama".into(),
                    genus: "Fera".into(),
                    source_mobs: vec!["Sentinel K'nash".into()],
                    source_level: None,
                    zone: None,
                    spawn_areas: vec![],
                    owned_effects: HashMap::new(),
                    notes: String::new(),
                },
            ],
        }
    }

    #[test]
    fn detects_nameplate_yellow_and_red_ink() {
        assert!(is_nameplate_ink(240, 200, 40));
        assert!(is_nameplate_ink(220, 180, 60));
        assert!(yellow_affinity(250, 240, 180) > 0.1);
        // Hostile red nameplate text (previously rejected as "not yellow").
        assert!(is_nameplate_ink(220, 40, 40));
        assert!(is_nameplate_ink(200, 50, 45));
        assert!(red_affinity(220, 40, 40) >= 0.18);
        assert!(!is_nameplate_ink(20, 20, 20));
        assert!(!is_nameplate_ink(40, 80, 220));
        // Muddy brown / low-value red-ish scenery should stay weak.
        assert!(red_affinity(90, 45, 30) < 0.18);
    }

    #[test]
    fn soft_ink_keeps_yellow_and_red() {
        let mut img = RgbaImage::new(4, 4);
        for pixel in img.pixels_mut() {
            *pixel = Rgba([10, 10, 10, 255]);
        }
        img.put_pixel(1, 1, Rgba([235, 195, 45, 255]));
        img.put_pixel(2, 1, Rgba([180, 160, 90, 255])); // AA edge
        img.put_pixel(1, 2, Rgba([220, 40, 40, 255])); // hostile red

        let out = soft_yellow(&img);
        assert!(out.get_pixel(1, 1).0[0] < 80);
        assert!(out.get_pixel(2, 1).0[0] < 220);
        assert!(out.get_pixel(1, 2).0[0] < 100);
        assert_eq!(out.get_pixel(0, 0).0[0], 255);
    }

    #[test]
    fn attempt_plan_prefers_soft_ink_single_line_whitelist() {
        let plan = attempt_plan_base(None);
        assert_eq!(
            plan[0],
            AttemptSpec {
                variant: PREFERRED_VARIANT.into(),
                psm: PREFERRED_PSM.into(),
                whitelist: PREFERRED_WHITELIST,
            }
        );
        // Base ladder: 4 variants × 3 configs.
        assert_eq!(plan.len(), 12);
        assert!(plan[1..].iter().all(|row| row != &plan[0]));
    }

    #[test]
    fn attempt_plan_tries_learned_winner_first() {
        let hint = OcrWinnerHint {
            variant: "02-yellow-hard".into(),
            psm: "6".into(),
            whitelist: false,
        };
        let plan = attempt_plan_base(Some(&hint));
        assert_eq!(plan[0].variant, "02-yellow-hard");
        assert_eq!(plan[0].psm, "6");
        assert!(!plan[0].whitelist);
        // Preferred still present second when it differs from the winner.
        assert_eq!(plan[1].variant, PREFERRED_VARIANT);
        assert_eq!(plan.len(), 12); // winner replaces duplicate, not additive
    }

    #[test]
    fn fallback_plan_has_otsu_stroke_nameband() {
        let plan = attempt_plan_fallback();
        assert_eq!(plan.len(), FALLBACK_VARIANTS.len() * PSM_CONFIGS.len());
        for name in FALLBACK_VARIANTS {
            assert!(plan.iter().any(|s| s.variant == *name));
        }
    }

    #[test]
    fn decisive_and_weak_score_thresholds() {
        assert!(is_decisive_score(1.0));
        assert!(is_decisive_score(0.95));
        assert!(!is_decisive_score(0.949));
        // Middling / weak scores are not decisive — ladder continues.
        assert!(!is_decisive_score(0.90));
        assert!(!is_decisive_score(0.84));
        assert!(is_weak_score(0.84));
        assert!(!is_weak_score(0.85));
    }

    #[test]
    fn winner_hint_roundtrips() {
        let hint = OcrWinnerHint {
            variant: "01-yellow-soft".into(),
            psm: "7".into(),
            whitelist: true,
        };
        save_winner_hint(&hint);
        let loaded = load_winner_hint().expect("hint");
        assert_eq!(loaded, hint);
    }

    #[test]
    fn rgba_hot_path_matches_png_roundtrip_pixels() {
        // Same pixels → same preprocess whether fed as RGBA or via PNG decode seam.
        let mut img = RgbaImage::new(8, 4);
        for pixel in img.pixels_mut() {
            *pixel = Rgba([12, 12, 12, 255]);
        }
        img.put_pixel(2, 1, Rgba([235, 195, 45, 255]));
        img.put_pixel(3, 1, Rgba([220, 180, 60, 255]));

        let png = encode_rgba_png(&img).unwrap();
        let decoded = image::load_from_memory(&png).unwrap().to_rgba8();
        assert_eq!(soft_yellow(&img).as_raw(), soft_yellow(&decoded).as_raw());
        assert_eq!(
            encode_gray_png(&upscale(
                &soft_yellow(&img),
                imageops::FilterType::Triangle,
                UPSCALE_FAST
            ))
            .unwrap(),
            encode_gray_png(&upscale(
                &soft_yellow(&decoded),
                imageops::FilterType::Triangle,
                UPSCALE_FAST
            ))
            .unwrap()
        );
    }

    #[test]
    fn prepared_gray_png_is_valid_for_stdin_feed() {
        let mut img = RgbaImage::new(4, 2);
        for pixel in img.pixels_mut() {
            *pixel = Rgba([10, 10, 10, 255]);
        }
        img.put_pixel(1, 0, Rgba([240, 200, 40, 255]));
        let gray = upscale(
            &soft_yellow(&img),
            imageops::FilterType::Triangle,
            UPSCALE_FAST,
        );
        let png = encode_gray_png(&gray).unwrap();
        // PNG magic — Tesseract stdin expects an encoded image, not raw pixels.
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n']);
    }

    #[test]
    fn name_band_crop_keeps_colored_ink_rows() {
        let mut img = RgbaImage::new(6, 12);
        for pixel in img.pixels_mut() {
            *pixel = Rgba([5, 5, 5, 255]);
        }
        // Red name band in the middle rows.
        for y in 4..8 {
            for x in 1..5 {
                img.put_pixel(x, y, Rgba([220, 40, 40, 255]));
            }
        }
        let cropped = crop_name_band(&img);
        assert!(cropped.height() < img.height());
        assert!(cropped.height() >= 4);
        assert!(soft_yellow(&cropped).pixels().any(|p| p.0[0] < 80));
    }

    #[test]
    fn otsu_and_stroke_fallback_variants_prepare() {
        let mut img = RgbaImage::new(8, 4);
        for pixel in img.pixels_mut() {
            *pixel = Rgba([12, 12, 12, 255]);
        }
        img.put_pixel(2, 1, Rgba([235, 195, 45, 255]));
        img.put_pixel(3, 1, Rgba([220, 180, 60, 255]));
        let (otsu, _) = prepare_variant(&img, "05-yellow-otsu").unwrap();
        assert!(otsu.pixels().any(|p| p.0[0] == 0));
        let (stroke, _) = prepare_variant(&img, "06-yellow-stroke").unwrap();
        // Stroke thicken should have at least as many ink pixels as soft yellow.
        let soft_ink = soft_yellow(&img).pixels().filter(|p| p.0[0] < 160).count();
        let stroke_ink = stroke.pixels().filter(|p| p.0[0] < 160).count();
        assert!(stroke_ink >= soft_ink);
    }

    #[test]
    fn debug_images_dir_off_skips_disk() {
        assert!(debug_images_dir(false).is_none());
    }

    #[test]
    fn debug_images_dir_on_resolves_under_app_data() {
        let dir = debug_images_dir(true).expect("debug dir when enabled");
        assert!(dir.ends_with("ocr-debug"));
        assert!(dir.is_dir());
    }

    #[test]
    fn nameplate_tess_config_is_single_line_lstm_with_dpi() {
        assert_eq!(PREFERRED_PSM, "7");
        assert_eq!(TESSERACT_OEM, "1");
        assert_eq!(UPSCALE_FAST, 2);
        assert_eq!(UPSCALE_ESCALATED, 3);
        assert_eq!(tesseract_dpi(UPSCALE_FAST), 192);
        assert_eq!(tesseract_dpi(UPSCALE_ESCALATED), 288);
        // Whitelist covers English mob-name chars used in the catalog.
        for ch in "Yellow-Tailed Skama K'nash".chars() {
            assert!(
                CHAR_WHITELIST.contains(ch),
                "whitelist missing {ch:?}"
            );
        }
    }

    #[test]
    fn catalog_user_words_include_pet_and_source_mob_tokens() {
        let words = catalog_user_words(&sample_catalog());
        for token in [
            "Manduri",
            "Fighter",
            "Wandering",
            "Yellow-Tailed",
            "Skama",
            "Sentinel",
            "K'nash",
        ] {
            assert!(
                words.lines().any(|l| l == token),
                "missing user-word {token:?} in:\n{words}"
            );
        }
        // Space-free tokens only (Tesseract dawg entries).
        assert!(words.lines().all(|l| !l.contains(' ')));
    }

    #[test]
    fn catalog_user_patterns_cover_title_case_and_hyphen_apostrophe() {
        let patterns = catalog_user_patterns();
        assert!(patterns.contains("\\A\\a+"));
        assert!(patterns.contains("\\A\\a+-\\A\\a+"));
        assert!(patterns.contains("\\A\\a+'\\a+"));
    }

    #[test]
    fn ensure_catalog_vocab_writes_reusable_files() {
        let catalog = sample_catalog();
        let first = ensure_catalog_vocab(&catalog).expect("vocab write");
        assert!(first.words_path.is_file());
        assert!(first.patterns_path.is_file());
        let words = std::fs::read_to_string(&first.words_path).unwrap();
        assert!(words.contains("Manduri"));
        assert!(words.contains("K'nash"));
        let patterns = std::fs::read_to_string(&first.patterns_path).unwrap();
        assert_eq!(patterns, catalog_user_patterns());

        let mtime_before = std::fs::metadata(&first.words_path)
            .unwrap()
            .modified()
            .unwrap();
        let second = ensure_catalog_vocab(&catalog).expect("vocab reuse");
        assert_eq!(first.words_path, second.words_path);
        let mtime_after = std::fs::metadata(&second.words_path)
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(mtime_before, mtime_after, "unchanged catalog should not rewrite");
    }

    #[test]
    fn tess_engine_reports_resident_or_cli() {
        let kind = crate::tess_engine::engine_kind();
        assert!(kind == "resident" || kind == "cli", "unexpected engine {kind}");
    }
}
