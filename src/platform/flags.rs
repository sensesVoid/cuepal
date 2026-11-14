//! Win32 flags for the overlay window. This module is the single source of
//! truth: eframe/winit options are only a best-effort hint, and these calls
//! are re-applied on every frame (see `ensure_flags`).

use std::sync::Mutex;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetWindowDisplayAffinity, GetWindowLongPtrW, SetWindowDisplayAffinity,
    SetWindowLongPtrW, GWL_EXSTYLE, WDA_EXCLUDEFROMCAPTURE, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
};

/// Unique title — `FindWindowW` discovers the HWND without depending on
/// eframe internals (version-proof across egui 0.37+).
pub const OVERLAY_TITLE: &str = "CuePal Overlay (spike)";

static OVERLAY_HWND: Mutex<Option<isize>> = Mutex::new(None);

/// Discover (and cache) the overlay HWND.
pub fn find_overlay_hwnd() -> Option<HWND> {
    if let Ok(g) = OVERLAY_HWND.lock() {
        if let Some(v) = *g {
            return Some(HWND(v as *mut _));
        }
    }
    let h = unsafe { FindWindowW(None, windows::core::w!("CuePal Overlay (spike)")).ok()? };
    if h.0.is_null() {
        return None;
    }
    if let Ok(mut g) = OVERLAY_HWND.lock() {
        *g = Some(h.0 as isize);
    }
    Some(h)
}

/// Cached HWND handle for other modules (tray, app).
pub fn cached_hwnd() -> Option<HWND> {
    find_overlay_hwnd()
}

/// Apply every flag the live overlay needs. Single source of truth.
pub unsafe fn apply_overlay_flags(hwnd: HWND) -> Result<(), String> {
    SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE)
        .map_err(|e| format!("SetWindowDisplayAffinity: {e}"))?;

    let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
    let wanted = style
        | WS_EX_NOACTIVATE.0 as isize
        | WS_EX_TOOLWINDOW.0 as isize
        | WS_EX_LAYERED.0 as isize;
    SetWindowLongPtrW(hwnd, GWL_EXSTYLE, wanted);
    Ok(())
}

/// Toggle click-through by setting/clearing WS_EX_TRANSPARENT only.
pub unsafe fn set_click_through(hwnd: HWND, on: bool) {
    let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
    let new = if on {
        style | WS_EX_TRANSPARENT.0 as isize
    } else {
        style & !(WS_EX_TRANSPARENT.0 as isize)
    };
    SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new);
}

/// Idempotent re-apply: re-read the style/affinity and re-apply if drifted.
pub unsafe fn ensure_flags(hwnd: HWND) {
    let mut affinity: u32 = 0;
    let _ = GetWindowDisplayAffinity(hwnd, &mut affinity);
    if affinity != WDA_EXCLUDEFROMCAPTURE.0 {
        let _ = apply_overlay_flags(hwnd);
        return;
    }
    let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
    if style & (WS_EX_NOACTIVATE.0 as isize) == 0
        || style & (WS_EX_TOOLWINDOW.0 as isize) == 0
        || style & (WS_EX_LAYERED.0 as isize) == 0
    {
        let _ = apply_overlay_flags(hwnd);
    }
}
