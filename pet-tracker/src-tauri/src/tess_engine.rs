//! Resident (in-process) Tesseract via dynamic loading, with CLI fallback.
//!
//! Loads `libtesseract` once and reuses a single `TessBaseAPI` across attempts/hotkeys
//! so model init cost isn't paid per ladder step. If the shared library cannot be loaded
//! (common when only the CLI is on PATH), falls back to spawning `tesseract` as before.

use image::GrayImage;
use libloading::{Library, Symbol};
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::io::Write;
use std::sync::{Mutex, OnceLock};

const OEM_LSTM_ONLY: c_int = 1;
const PSM_SINGLE_BLOCK: c_int = 6;
const PSM_SINGLE_LINE: c_int = 7;

type TessBaseApi = c_void;

struct TessFns {
    // Keep libraries alive for the process lifetime.
    _libs: Vec<Library>,
    create: unsafe extern "C" fn() -> *mut TessBaseApi,
    delete: unsafe extern "C" fn(*mut TessBaseApi),
    /// Init with explicit OEM when available (Init2); else None and use Init3.
    init2: Option<unsafe extern "C" fn(*mut TessBaseApi, *const c_char, *const c_char, c_int) -> c_int>,
    init3: unsafe extern "C" fn(*mut TessBaseApi, *const c_char, *const c_char) -> c_int,
    set_page_seg_mode: unsafe extern "C" fn(*mut TessBaseApi, c_int),
    set_variable: unsafe extern "C" fn(*mut TessBaseApi, *const c_char, *const c_char) -> c_int,
    set_image: unsafe extern "C" fn(*mut TessBaseApi, *const u8, c_int, c_int, c_int, c_int),
    set_source_resolution: unsafe extern "C" fn(*mut TessBaseApi, c_int),
    recognize: unsafe extern "C" fn(*mut TessBaseApi, *mut c_void) -> c_int,
    get_utf8_text: unsafe extern "C" fn(*mut TessBaseApi) -> *mut c_char,
    delete_text: unsafe extern "C" fn(*mut c_char),
}

struct ResidentApi {
    fns: TessFns,
    handle: *mut TessBaseApi,
}

// TessBaseAPI is used under a mutex; Tesseract documents multi-instance thread safety.
unsafe impl Send for ResidentApi {}

pub struct VocabPaths<'a> {
    pub words: &'a Path,
    pub patterns: &'a Path,
}

static RESIDENT: OnceLock<Mutex<Option<Result<ResidentApi, String>>>> = OnceLock::new();

fn resident_slot() -> &'static Mutex<Option<Result<ResidentApi, String>>> {
    RESIDENT.get_or_init(|| Mutex::new(None))
}

fn which_tesseract() -> Option<PathBuf> {
    let output = Command::new(if cfg!(windows) { "where" } else { "which" })
        .arg("tesseract")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let first = stdout.lines().next()?.trim();
    if first.is_empty() {
        None
    } else {
        Some(PathBuf::from(first))
    }
}

fn tessdata_dir(install_dir: &Path) -> Option<PathBuf> {
    if let Ok(prefix) = std::env::var("TESSDATA_PREFIX") {
        let p = PathBuf::from(prefix);
        if p.join("eng.traineddata").is_file() {
            return Some(p);
        }
        let nested = p.join("tessdata");
        if nested.join("eng.traineddata").is_file() {
            return Some(nested);
        }
    }
    let sibling = install_dir.join("tessdata");
    if sibling.join("eng.traineddata").is_file() {
        return Some(sibling);
    }
    for candidate in [
        "/usr/share/tesseract-ocr/5/tessdata",
        "/usr/share/tesseract-ocr/4.00/tessdata",
        "/usr/share/tessdata",
        "/usr/local/share/tessdata",
    ] {
        let p = PathBuf::from(candidate);
        if p.join("eng.traineddata").is_file() {
            return Some(p);
        }
    }
    None
}

fn candidate_lib_names() -> Vec<&'static str> {
    if cfg!(windows) {
        vec![
            "libtesseract-5.dll",
            "libtesseract54.dll",
            "libtesseract53.dll",
            "libtesseract.dll",
            "tesseract53.dll",
            "tesseract52.dll",
            "tesseract50.dll",
            "tesseract.dll",
        ]
    } else if cfg!(target_os = "macos") {
        vec![
            "libtesseract.dylib",
            "libtesseract.5.dylib",
            "libtesseract.4.dylib",
        ]
    } else {
        vec![
            "libtesseract.so",
            "libtesseract.so.5",
            "libtesseract.so.4",
        ]
    }
}

fn candidate_leptonica_names() -> Vec<&'static str> {
    if cfg!(windows) {
        vec![
            "leptonica-1.84.0.dll",
            "leptonica-1.83.0.dll",
            "leptonica-1.82.0.dll",
            "leptonica-1.80.0.dll",
            "libleptonica-5.dll",
            "liblept-5.dll",
            "liblept.dll",
        ]
    } else if cfg!(target_os = "macos") {
        vec!["liblept.dylib", "libleptonica.dylib"]
    } else {
        vec!["liblept.so", "liblept.so.5", "libleptonica.so"]
    }
}

fn try_load_library(dir: Option<&Path>, name: &str) -> Option<Library> {
    if let Some(dir) = dir {
        let path = dir.join(name);
        if let Ok(lib) = unsafe { Library::new(&path) } {
            return Some(lib);
        }
    }
    unsafe { Library::new(name) }.ok()
}

fn load_tess_fns(install_dir: Option<&Path>) -> Result<TessFns, String> {
    let mut libs = Vec::new();
    // Leptonica must be loadable before tesseract on Windows.
    for name in candidate_leptonica_names() {
        if let Some(lib) = try_load_library(install_dir, name) {
            libs.push(lib);
            break;
        }
    }
    let mut tess_lib = None;
    for name in candidate_lib_names() {
        if let Some(lib) = try_load_library(install_dir, name) {
            tess_lib = Some(lib);
            break;
        }
    }
    let tess_lib = tess_lib.ok_or_else(|| {
        "Could not load libtesseract (DLL/SO). Install Tesseract OCR or ensure it is on PATH."
            .to_string()
    })?;
    libs.push(tess_lib);

    // Copy function pointers out of Symbols so TessFns doesn't borrow `Library`.
    // SAFETY: `_libs` keeps the shared libraries mapped for the process lifetime.
    let (
        create,
        delete,
        init2,
        init3,
        set_page_seg_mode,
        set_variable,
        set_image,
        set_source_resolution,
        recognize,
        get_utf8_text,
        delete_text,
    ) = {
        let tess = libs.last().unwrap();
        unsafe {
            let create: Symbol<unsafe extern "C" fn() -> *mut TessBaseApi> = tess
                .get(b"TessBaseAPICreate\0")
                .map_err(|e| format!("TessBaseAPICreate missing: {e}"))?;
            let delete: Symbol<unsafe extern "C" fn(*mut TessBaseApi)> = tess
                .get(b"TessBaseAPIDelete\0")
                .map_err(|e| format!("TessBaseAPIDelete missing: {e}"))?;
            let init2_sym: Option<
                Symbol<
                    unsafe extern "C" fn(
                        *mut TessBaseApi,
                        *const c_char,
                        *const c_char,
                        c_int,
                    ) -> c_int,
                >,
            > = tess.get(b"TessBaseAPIInit2\0").ok();
            let init3: Symbol<
                unsafe extern "C" fn(*mut TessBaseApi, *const c_char, *const c_char) -> c_int,
            > = tess
                .get(b"TessBaseAPIInit3\0")
                .map_err(|e| format!("TessBaseAPIInit3 missing: {e}"))?;
            let set_page_seg_mode: Symbol<unsafe extern "C" fn(*mut TessBaseApi, c_int)> = tess
                .get(b"TessBaseAPISetPageSegMode\0")
                .map_err(|e| format!("TessBaseAPISetPageSegMode missing: {e}"))?;
            let set_variable: Symbol<
                unsafe extern "C" fn(*mut TessBaseApi, *const c_char, *const c_char) -> c_int,
            > = tess
                .get(b"TessBaseAPISetVariable\0")
                .map_err(|e| format!("TessBaseAPISetVariable missing: {e}"))?;
            let set_image: Symbol<
                unsafe extern "C" fn(*mut TessBaseApi, *const u8, c_int, c_int, c_int, c_int),
            > = tess
                .get(b"TessBaseAPISetImage\0")
                .map_err(|e| format!("TessBaseAPISetImage missing: {e}"))?;
            let set_source_resolution: Symbol<unsafe extern "C" fn(*mut TessBaseApi, c_int)> = tess
                .get(b"TessBaseAPISetSourceResolution\0")
                .map_err(|e| format!("TessBaseAPISetSourceResolution missing: {e}"))?;
            let recognize: Symbol<unsafe extern "C" fn(*mut TessBaseApi, *mut c_void) -> c_int> =
                tess.get(b"TessBaseAPIRecognize\0")
                    .map_err(|e| format!("TessBaseAPIRecognize missing: {e}"))?;
            let get_utf8_text: Symbol<unsafe extern "C" fn(*mut TessBaseApi) -> *mut c_char> = tess
                .get(b"TessBaseAPIGetUTF8Text\0")
                .map_err(|e| format!("TessBaseAPIGetUTF8Text missing: {e}"))?;
            let delete_text: Symbol<unsafe extern "C" fn(*mut c_char)> = tess
                .get(b"TessDeleteText\0")
                .map_err(|e| format!("TessDeleteText missing: {e}"))?;

            (
                *create,
                *delete,
                init2_sym.as_ref().map(|s| **s),
                *init3,
                *set_page_seg_mode,
                *set_variable,
                *set_image,
                *set_source_resolution,
                *recognize,
                *get_utf8_text,
                *delete_text,
            )
        }
    };

    Ok(TessFns {
        _libs: libs,
        create,
        delete,
        init2,
        init3,
        set_page_seg_mode,
        set_variable,
        set_image,
        set_source_resolution,
        recognize,
        get_utf8_text,
        delete_text,
    })
}

fn psm_to_int(psm: &str) -> c_int {
    match psm {
        "7" => PSM_SINGLE_LINE,
        "6" => PSM_SINGLE_BLOCK,
        other => other.parse().unwrap_or(PSM_SINGLE_LINE),
    }
}

fn init_resident() -> Result<ResidentApi, String> {
    let exe = which_tesseract();
    let install_dir = exe.as_ref().and_then(|p| p.parent().map(|d| d.to_path_buf()));
    let tessdata = install_dir
        .as_ref()
        .and_then(|d| tessdata_dir(d))
        .or_else(|| tessdata_dir(Path::new(".")))
        .ok_or_else(|| {
            "Tesseract tessdata (eng.traineddata) not found. Install Tesseract OCR with English data."
                .to_string()
        })?;

    let fns = load_tess_fns(install_dir.as_deref())?;
    let handle = unsafe { (fns.create)() };
    if handle.is_null() {
        return Err("TessBaseAPICreate returned null".into());
    }

    let data_c = CString::new(tessdata.to_string_lossy().as_bytes())
        .map_err(|_| "tessdata path contains NUL".to_string())?;
    let lang = CString::new("eng").unwrap();
    let rc = unsafe {
        if let Some(init2) = fns.init2 {
            init2(handle, data_c.as_ptr(), lang.as_ptr(), OEM_LSTM_ONLY)
        } else {
            (fns.init3)(handle, data_c.as_ptr(), lang.as_ptr())
        }
    };
    if rc != 0 {
        unsafe { (fns.delete)(handle) };
        return Err(format!(
            "Tesseract init failed (code {rc}). Install Tesseract OCR and ensure eng.traineddata is available."
        ));
    }

    Ok(ResidentApi { fns, handle })
}

fn ensure_resident() -> Result<(), String> {
    let slot = resident_slot();
    let mut guard = slot
        .lock()
        .map_err(|_| "Tesseract engine lock poisoned".to_string())?;
    if guard.is_none() {
        *guard = Some(init_resident());
    }
    match guard.as_ref().unwrap() {
        Ok(_) => Ok(()),
        Err(e) => Err(e.clone()),
    }
}

fn resident_ocr(
    api: &ResidentApi,
    img: &GrayImage,
    psm: &str,
    whitelist: bool,
    dpi: u32,
    vocab: Option<&VocabPaths<'_>>,
    char_whitelist: &str,
) -> Result<String, String> {
    let w = img.width() as c_int;
    let h = img.height() as c_int;
    if w <= 0 || h <= 0 {
        return Err("OCR image is empty".into());
    }

    unsafe {
        (api.fns.set_page_seg_mode)(api.handle, psm_to_int(psm));
        (api.fns.set_source_resolution)(api.handle, dpi as c_int);

        let wl_key = CString::new("tessedit_char_whitelist").unwrap();
        let wl_val = CString::new(if whitelist { char_whitelist } else { "" }).unwrap();
        (api.fns.set_variable)(api.handle, wl_key.as_ptr(), wl_val.as_ptr());

        if let Some(v) = vocab {
            let words_key = CString::new("user_words_file").unwrap();
            let words_val = CString::new(v.words.to_string_lossy().as_bytes())
                .map_err(|_| "user-words path contains NUL".to_string())?;
            (api.fns.set_variable)(api.handle, words_key.as_ptr(), words_val.as_ptr());

            let pat_key = CString::new("user_patterns_file").unwrap();
            let pat_val = CString::new(v.patterns.to_string_lossy().as_bytes())
                .map_err(|_| "user-patterns path contains NUL".to_string())?;
            (api.fns.set_variable)(api.handle, pat_key.as_ptr(), pat_val.as_ptr());
        }

        (api.fns.set_image)(api.handle, img.as_raw().as_ptr(), w, h, 1, w);
        let rc = (api.fns.recognize)(api.handle, std::ptr::null_mut());
        if rc != 0 {
            return Err(format!("Tesseract recognize failed (code {rc})"));
        }
        let text_ptr = (api.fns.get_utf8_text)(api.handle);
        if text_ptr.is_null() {
            return Err("Tesseract returned null text".into());
        }
        let text = CStr::from_ptr(text_ptr)
            .to_string_lossy()
            .trim()
            .to_string();
        (api.fns.delete_text)(text_ptr);
        Ok(text)
    }
}

fn run_tesseract_cli_png(
    png: &[u8],
    psm: &str,
    whitelist: bool,
    dpi: u32,
    vocab: Option<&VocabPaths<'_>>,
    char_whitelist: &str,
    oem: &str,
) -> Result<String, String> {
    let mut cmd = Command::new("tesseract");
    cmd.arg("-")
        .arg("stdout")
        .arg("-l")
        .arg("eng")
        .arg("--psm")
        .arg(psm)
        .arg("--oem")
        .arg(oem)
        .arg("--dpi")
        .arg(dpi.to_string());
    if whitelist {
        cmd.arg("-c")
            .arg(format!("tessedit_char_whitelist={char_whitelist}"));
    }
    if let Some(v) = vocab {
        cmd.arg("--user-words")
            .arg(v.words)
            .arg("--user-patterns")
            .arg(v.patterns);
    }

    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            format!(
                "Failed to run tesseract ({e}). Install Tesseract OCR and ensure it is on PATH."
            )
        })?;

    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| "Failed to open tesseract stdin".to_string())?;
        stdin
            .write_all(png)
            .map_err(|e| format!("Failed to write image to tesseract stdin: {e}"))?;
    }

    let output = child
        .wait_with_output()
        .map_err(|e| format!("tesseract wait failed: {e}"))?;

    if !output.status.success() {
        let err = String::from_utf8_lossy(&output.stderr);
        return Err(format!("tesseract failed: {err}"));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn encode_gray_png(img: &GrayImage) -> Result<Vec<u8>, String> {
    use image::ImageEncoder;
    use std::io::Cursor;
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

/// OCR a grayscale nameplate buffer. Prefers the resident in-process engine;
/// falls back to CLI `tesseract` when the shared library cannot be loaded.
pub fn ocr_gray(
    img: &GrayImage,
    psm: &str,
    whitelist: bool,
    dpi: u32,
    vocab: Option<&VocabPaths<'_>>,
    char_whitelist: &str,
    oem: &str,
) -> Result<String, String> {
    // Try resident first; cache init failure so we don't retry every attempt.
    let resident_ok = ensure_resident().is_ok();
    if resident_ok {
        let slot = resident_slot();
        let mut guard = slot
            .lock()
            .map_err(|_| "Tesseract engine lock poisoned".to_string())?;
        if let Some(Ok(api)) = guard.as_mut() {
            return resident_ocr(api, img, psm, whitelist, dpi, vocab, char_whitelist);
        }
    }

    // CLI fallback (also used when only the tesseract binary is installed).
    let png = encode_gray_png(img)?;
    run_tesseract_cli_png(png.as_slice(), psm, whitelist, dpi, vocab, char_whitelist, oem)
}

/// Human-readable status for debug/tests.
pub fn engine_kind() -> &'static str {
    match ensure_resident() {
        Ok(()) => "resident",
        Err(_) => "cli",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_kind_is_resident_or_cli() {
        let kind = engine_kind();
        assert!(kind == "resident" || kind == "cli", "unexpected {kind}");
    }

    #[test]
    fn missing_tesseract_error_is_actionable() {
        // When resident init fails, the error should mention install/PATH guidance.
        // On machines with a working install this still documents the message shape
        // via the CLI fallback path's spawn error if `tesseract` is absent.
        if which_tesseract().is_some() {
            return;
        }
        let img = GrayImage::new(4, 2);
        let err = ocr_gray(&img, "7", true, 192, None, "abc", "1").unwrap_err();
        let lower = err.to_lowercase();
        assert!(
            lower.contains("tesseract")
                && (lower.contains("install") || lower.contains("path") || lower.contains("failed")),
            "unclear error: {err}"
        );
    }

    #[test]
    fn psm_mapping() {
        assert_eq!(psm_to_int("7"), PSM_SINGLE_LINE);
        assert_eq!(psm_to_int("6"), PSM_SINGLE_BLOCK);
    }
}
