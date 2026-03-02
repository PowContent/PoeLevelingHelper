use std::sync::mpsc;
use std::time::{Duration, Instant};
use log::{info, warn, debug};

#[derive(Debug)]
#[allow(dead_code)]
pub enum OcrError {
    WindowNotFound,
    CaptureError(String),
    OcrFailed(String),
    NoLevelFound,
    NoEngine,
    NotSupported,
    #[cfg(windows)]
    Windows(windows::core::Error),
}

impl std::fmt::Display for OcrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OcrError::WindowNotFound => write!(f, "PoE window not found"),
            OcrError::CaptureError(msg) => write!(f, "Capture error: {}", msg),
            OcrError::OcrFailed(msg) => write!(f, "OCR failed: {}", msg),
            OcrError::NoLevelFound => write!(f, "No level number found in OCR text"),
            OcrError::NoEngine => write!(f, "No OCR engine available (missing language pack?)"),
            OcrError::NotSupported => write!(f, "OCR not supported on this platform"),
            #[cfg(windows)]
            OcrError::Windows(e) => write!(f, "Windows error: {}", e),
        }
    }
}

#[cfg(windows)]
impl From<windows::core::Error> for OcrError {
    fn from(e: windows::core::Error) -> Self {
        OcrError::Windows(e)
    }
}

enum OcrCommand {
    TriggerOcr,
    Shutdown,
}

pub struct OcrWorker {
    cmd_tx: mpsc::Sender<OcrCommand>,
    result_rx: mpsc::Receiver<u32>,
}

impl OcrWorker {
    pub fn spawn() -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (result_tx, result_rx) = mpsc::channel();

        std::thread::Builder::new()
            .name("ocr-worker".to_string())
            .spawn(move || {
                worker_loop(cmd_rx, result_tx);
            })
            .expect("Failed to spawn OCR worker thread");

        Self { cmd_tx, result_rx }
    }

    /// Trigger an OCR attempt. Cancels any pending retry.
    pub fn trigger(&self) {
        let _ = self.cmd_tx.send(OcrCommand::TriggerOcr);
    }

    /// Non-blocking poll for the latest OCR result.
    pub fn poll_result(&self) -> Option<u32> {
        let mut last = None;
        // Drain all pending results, keep the latest
        while let Ok(level) = self.result_rx.try_recv() {
            last = Some(level);
        }
        last
    }

    /// Signal the background thread to shut down.
    pub fn shutdown(&self) {
        let _ = self.cmd_tx.send(OcrCommand::Shutdown);
    }
}

/// Background thread state machine.
/// States:
///   Idle — waiting indefinitely for a TriggerOcr command
///   WaitingToAttempt — waiting N seconds before attempting OCR
///     (1s on first trigger, 5s on retry after failure)
///   Any incoming TriggerOcr resets the wait to 1s (prevents stacking)
fn worker_loop(cmd_rx: mpsc::Receiver<OcrCommand>, result_tx: mpsc::Sender<u32>) {
    let initial_delay = Duration::from_secs(1);
    let retry_delay = Duration::from_secs(5);

    loop {
        // === Idle: wait forever for a command ===
        match cmd_rx.recv() {
            Ok(OcrCommand::TriggerOcr) => {
                debug!("OCR: Zone change detected, will attempt OCR in 1s");
            }
            Ok(OcrCommand::Shutdown) | Err(_) => {
                info!("OCR worker shutting down");
                return;
            }
        }

        // === WaitingToAttempt state ===
        let mut deadline = Instant::now() + initial_delay;

        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                // Time to attempt OCR
                match do_ocr() {
                    Ok(level) => {
                        info!("OCR detected area level: {}", level);
                        let _ = result_tx.send(level);
                        break; // Back to Idle
                    }
                    Err(e) => {
                        warn!("OCR attempt failed: {}, retrying in 5s", e);
                        deadline = Instant::now() + retry_delay;
                        // Continue waiting loop
                    }
                }
            } else {
                // Wait with timeout, listening for new commands
                match cmd_rx.recv_timeout(remaining) {
                    Ok(OcrCommand::TriggerOcr) => {
                        debug!("OCR: New zone change, resetting timer to 1s");
                        deadline = Instant::now() + initial_delay;
                        // Continue waiting loop with reset deadline
                    }
                    Ok(OcrCommand::Shutdown) => {
                        info!("OCR worker shutting down");
                        return;
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        // Timer elapsed, loop back to attempt OCR
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => {
                        info!("OCR worker channel disconnected, shutting down");
                        return;
                    }
                }
            }
        }
    }
}

// ============================================================
// Windows implementation
// ============================================================

#[cfg(windows)]
fn do_ocr() -> Result<u32, OcrError> {
    let hwnd = find_poe_window()?;
    let (pixels, width, height) = capture_top_right(hwnd)?;

    let text = match run_windows_ocr(&pixels, width, height) {
        Ok(t) => t,
        Err(e) => {
            warn!("OCR engine failed: {}", e);
            save_debug_image(&pixels, width, height, "ocr_debug_engine_fail");
            return Err(e);
        }
    };

    info!("OCR raw text: {:?}", text);

    match parse_level_from_text(&text) {
        Ok(level) => Ok(level),
        Err(e) => {
            warn!("OCR text parse failed — text was: {:?}", text);
            save_debug_image(&pixels, width, height, "ocr_debug");
            Err(e)
        }
    }
}

/// Save RGBA pixel data as a PNG next to the executable for debugging.
#[cfg(windows)]
fn save_debug_image(rgba_pixels: &[u8], w: u32, h: u32, name: &str) {
    let path = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(format!("{}.png", name));

    match image::ImageBuffer::<image::Rgba<u8>, _>::from_raw(w, h, rgba_pixels.to_vec()) {
        Some(img) => {
            match img.save(&path) {
                Ok(_) => info!("Saved debug image to {:?}", path),
                Err(e) => warn!("Failed to save debug image: {}", e),
            }
        }
        None => warn!("Failed to create image buffer for debug save"),
    }
}

#[cfg(windows)]
fn find_poe_window() -> Result<winapi::shared::windef::HWND, OcrError> {
    use winapi::um::winuser::{EnumWindows, GetWindowTextW, IsWindowVisible};
    use winapi::shared::minwindef::{BOOL, LPARAM, TRUE, FALSE};
    use winapi::shared::windef::HWND;

    // We pass a mutable pointer to HWND through LPARAM
    let mut found_hwnd: HWND = std::ptr::null_mut();

    unsafe extern "system" fn callback(hwnd: HWND, lparam: LPARAM) -> BOOL {
        unsafe {
            if IsWindowVisible(hwnd) == 0 {
                return TRUE;
            }
            let mut title = [0u16; 256];
            let len = GetWindowTextW(hwnd, title.as_mut_ptr(), 256);
            if len > 0 {
                let title_str = String::from_utf16_lossy(&title[..len as usize]);
                if title_str.contains("Path of Exile") {
                    let out = &mut *(lparam as *mut HWND);
                    *out = hwnd;
                    return FALSE; // Stop enumeration
                }
            }
            TRUE
        }
    }

    unsafe {
        EnumWindows(Some(callback), &mut found_hwnd as *mut HWND as LPARAM);
    }

    if found_hwnd.is_null() {
        Err(OcrError::WindowNotFound)
    } else {
        Ok(found_hwnd)
    }
}

/// Capture the top-right region of the PoE window.
/// Returns (RGBA pixels, width, height).
#[cfg(windows)]
fn capture_top_right(
    hwnd: winapi::shared::windef::HWND,
) -> Result<(Vec<u8>, u32, u32), OcrError> {
    use winapi::um::wingdi::*;
    use winapi::um::winuser::*;
    use winapi::shared::windef::RECT;

    unsafe {
        // Get the window rect (includes title bar/frame) and client rect
        let mut window_rect: RECT = std::mem::zeroed();
        let mut client_rect: RECT = std::mem::zeroed();
        if GetWindowRect(hwnd, &mut window_rect) == 0 {
            return Err(OcrError::CaptureError("GetWindowRect failed".into()));
        }
        if GetClientRect(hwnd, &mut client_rect) == 0 {
            return Err(OcrError::CaptureError("GetClientRect failed".into()));
        }

        let window_w = window_rect.right - window_rect.left;
        let window_h = window_rect.bottom - window_rect.top;
        let client_w = client_rect.right;
        let client_h = client_rect.bottom;

        // The frame/title bar height is the difference between window and client height
        let frame_top = window_h - client_h; // includes title bar + top border
        let frame_side = (window_w - client_w) / 2; // left/right border

        info!(
            "Window: {}x{}, Client: {}x{}, Frame top: {}, Frame side: {}",
            window_w, window_h, client_w, client_h, frame_top, frame_side
        );

        if client_w <= 0 || client_h <= 0 {
            return Err(OcrError::CaptureError("Window has zero size".into()));
        }

        // PrintWindow captures the FULL window including frame.
        // We need to offset our capture region by the frame dimensions.
        let full_w = window_w; // Full capture width (entire window)
        let full_h = window_h; // Full capture height (entire window)

        // Define capture region: top-right corner of the window
        // Static size: 20% of width or 200px, whichever is higher
        let cap_size = ((window_w as f32 * 0.20) as i32).max(200);
        let cap_w = cap_size.min(full_w);
        let cap_h = cap_size.min(full_h);
        let cap_x = (full_w - cap_w).max(0);
        let cap_y = 0;

        info!(
            "Capture region: x={}, y={}, w={}, h={}",
            cap_x, cap_y, cap_w, cap_h
        );

        if cap_w <= 0 || cap_h <= 0 {
            return Err(OcrError::CaptureError("Capture region has zero size".into()));
        }

        // Create DCs and bitmaps for full window capture
        let screen_dc = GetDC(std::ptr::null_mut());
        if screen_dc.is_null() {
            return Err(OcrError::CaptureError("GetDC failed".into()));
        }

        let mem_dc = CreateCompatibleDC(screen_dc);
        let full_bmp = CreateCompatibleBitmap(screen_dc, full_w, full_h);
        let old_bmp = SelectObject(mem_dc, full_bmp as *mut _);

        // PW_RENDERFULLCONTENT = 0x00000002 — captures DirectX content
        let pw_result = PrintWindow(hwnd, mem_dc, 0x00000002);
        if pw_result == 0 {
            // Cleanup and report error
            SelectObject(mem_dc, old_bmp);
            DeleteObject(full_bmp as *mut _);
            DeleteDC(mem_dc);
            ReleaseDC(std::ptr::null_mut(), screen_dc);
            return Err(OcrError::CaptureError("PrintWindow failed".into()));
        }

        // BitBlt the sub-region into a smaller bitmap
        let region_dc = CreateCompatibleDC(screen_dc);
        let region_bmp = CreateCompatibleBitmap(screen_dc, cap_w, cap_h);
        let old_region_bmp = SelectObject(region_dc, region_bmp as *mut _);
        BitBlt(region_dc, 0, 0, cap_w, cap_h, mem_dc, cap_x, cap_y, SRCCOPY);

        // Read pixels as BGRA via GetDIBits
        let mut bi: BITMAPINFOHEADER = std::mem::zeroed();
        bi.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
        bi.biWidth = cap_w;
        bi.biHeight = -cap_h; // Negative = top-down
        bi.biPlanes = 1;
        bi.biBitCount = 32;
        bi.biCompression = BI_RGB;

        let pixel_count = (cap_w * cap_h * 4) as usize;
        let mut pixels = vec![0u8; pixel_count];

        let rows = GetDIBits(
            region_dc,
            region_bmp,
            0,
            cap_h as u32,
            pixels.as_mut_ptr() as *mut _,
            &mut bi as *mut BITMAPINFOHEADER as *mut BITMAPINFO,
            DIB_RGB_COLORS,
        );

        // Cleanup GDI resources
        SelectObject(region_dc, old_region_bmp);
        DeleteObject(region_bmp as *mut _);
        DeleteDC(region_dc);
        SelectObject(mem_dc, old_bmp);
        DeleteObject(full_bmp as *mut _);
        DeleteDC(mem_dc);
        ReleaseDC(std::ptr::null_mut(), screen_dc);

        if rows == 0 {
            return Err(OcrError::CaptureError("GetDIBits returned 0 rows".into()));
        }

        // Convert BGRA → RGBA
        for chunk in pixels.chunks_exact_mut(4) {
            chunk.swap(0, 2);
        }

        // Check for all-black capture (DX capture failure)
        let non_black = pixels.chunks_exact(4).any(|px| px[0] > 10 || px[1] > 10 || px[2] > 10);
        if !non_black {
            return Err(OcrError::CaptureError(
                "Captured image is all black — PrintWindow may not work with this PoE renderer".into(),
            ));
        }

        Ok((pixels, cap_w as u32, cap_h as u32))
    }
}

/// Run Windows OCR (WinRT) on RGBA pixel data.
#[cfg(windows)]
fn run_windows_ocr(rgba_pixels: &[u8], w: u32, h: u32) -> Result<String, OcrError> {
    use windows::Graphics::Imaging::{
        BitmapBufferAccessMode, BitmapPixelFormat, SoftwareBitmap,
    };
    use windows::Media::Ocr::OcrEngine;
    use windows::Foundation::IMemoryBufferReference;

    // Create a SoftwareBitmap
    let bitmap = SoftwareBitmap::Create(
        BitmapPixelFormat::Rgba8,
        w as i32,
        h as i32,
    )?;

    // Copy pixel data into the bitmap buffer
    {
        let buffer = bitmap.LockBuffer(BitmapBufferAccessMode::Write)?;
        let reference: IMemoryBufferReference = buffer.CreateReference()?;

        // Use the IMemoryBufferByteAccess interface to get a raw pointer
        use windows::core::Interface;
        let byte_access: windows::Win32::System::WinRT::IMemoryBufferByteAccess =
            reference.cast()?;

        let mut data_ptr: *mut u8 = std::ptr::null_mut();
        let mut capacity: u32 = 0;
        unsafe {
            byte_access.GetBuffer(&mut data_ptr, &mut capacity)?;

            let dest = std::slice::from_raw_parts_mut(data_ptr, capacity as usize);
            let copy_len = rgba_pixels.len().min(dest.len());
            dest[..copy_len].copy_from_slice(&rgba_pixels[..copy_len]);
        }
    }

    // Create OCR engine from user profile languages
    let engine = OcrEngine::TryCreateFromUserProfileLanguages()
        .map_err(|e| OcrError::OcrFailed(format!("Failed to create OCR engine: {}", e)))?;

    // Run recognition (blocking on the async operation)
    let result = engine.RecognizeAsync(&bitmap)?.get()?;
    let text = result.Text()?.to_string();

    Ok(text)
}

/// Parse the area/monster level number from OCR text.
/// Looks for patterns like "Area Level: 23", "Monster Level: 45",
/// or falls back to any standalone number in range 1-84.
fn parse_level_from_text(text: &str) -> Result<u32, OcrError> {
    // Find "level" keyword preceded by "monster" or "area", then take the number after it
    let words: Vec<&str> = text.split_whitespace().collect();
    let lower_words: Vec<String> = words.iter().map(|w| w.to_lowercase()).collect();

    // Pass 1: look for "monster level" or "area level" pattern
    for (i, word) in lower_words.iter().enumerate() {
        if word.contains("level") && i > 0 {
            let prev = &lower_words[i - 1];
            if prev.contains("monster") || prev.contains("area") {
                for j in (i + 1)..words.len().min(i + 3) {
                    let cleaned: String = words[j].chars().filter(|c| c.is_ascii_digit()).collect();
                    if let Ok(n) = cleaned.parse::<u32>() {
                        if (1..=100).contains(&n) {
                            info!("Parsed level {} from '{} {}'", n, words[i - 1], words[i]);
                            return Ok(n);
                        }
                    }
                    if words[j].chars().any(|c| c.is_alphabetic()) {
                        break;
                    }
                }
            }
        }
    }

    // Pass 2: any "level" keyword followed by a number (less strict)
    for (i, word) in lower_words.iter().enumerate() {
        if word.contains("level") {
            for j in (i + 1)..words.len().min(i + 3) {
                let cleaned: String = words[j].chars().filter(|c| c.is_ascii_digit()).collect();
                if let Ok(n) = cleaned.parse::<u32>() {
                    if (1..=100).contains(&n) {
                        info!("Parsed level {} from generic 'level' match at word '{}'", n, words[i]);
                        return Ok(n);
                    }
                }
                if words[j].chars().any(|c| c.is_alphabetic()) {
                    break;
                }
            }
        }
    }

    Err(OcrError::NoLevelFound)
}

// ============================================================
// Non-Windows stub
// ============================================================

#[cfg(not(windows))]
fn do_ocr() -> Result<u32, OcrError> {
    Err(OcrError::NotSupported)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_level_basic() {
        assert_eq!(parse_level_from_text("Area Level: 23").unwrap(), 23);
        assert_eq!(parse_level_from_text("Monster Level: 45").unwrap(), 45);
        assert_eq!(parse_level_from_text("Level 1").unwrap(), 1);
    }

    #[test]
    fn test_parse_level_multiline() {
        let text = "The Coast\nArea Level: 12\nMonsters remaining: 0";
        assert_eq!(parse_level_from_text(text).unwrap(), 12);
    }

    #[test]
    fn test_parse_level_no_keyword() {
        // No "level" keyword — should fail (no false positives)
        assert!(parse_level_from_text("42").is_err());
    }

    #[test]
    fn test_parse_level_no_match() {
        assert!(parse_level_from_text("no numbers here").is_err());
        assert!(parse_level_from_text("").is_err());
    }

    #[test]
    fn test_parse_level_out_of_range() {
        // 999 is out of range for both primary and fallback
        assert!(parse_level_from_text("999").is_err());
    }

    #[test]
    fn test_parse_level_ocr_real_output() {
        let text = "The vaal City Monster Level: 54 Long Live Octavian (PL78528) League Free for All Canada (East) Realm TH+4AST 8 OF A MILLION FACES";
        assert_eq!(parse_level_from_text(text).unwrap(), 54);
    }
}
