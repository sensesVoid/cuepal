//! Win32 flags for the overlay window. This module is the single source of
//! truth: eframe/winit options are only a best-effort hint, and these calls
//! are re-applied on every frame (see `ensure_flags`).
//!
//! Modes:
//! - Overlay/click-through: no activation, toolwindow, layered, transparent click-through.
//! - Interactive/notepad-like: activatable/clickable/draggable (no NOACTIVATE/TRANSPARENT).

use std::sync::Mutex;
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetWindowLongPtrW, SetWindowDisplayAffinity, SetWindowLongPtrW, GWL_EXSTYLE,
    WDA_EXCLUDEFROMCAPTURE, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
};

/// Unique title — `FindWindowW` discovers the HWND without depending on
/// eframe internals (version-proof across egui 0.37+).
pub const OVERLAY_TITLE: &str = "CuePal Overlay (spike)";

static OVERLAY_HWND: Mutex<Option<isize>> = Mutex::new(None);

/// Discover (and cache) the overlay HWND. The cache is invalidated if the
/// handle is no longer a live window (re-creation after a DPI change, etc.),
/// so flags are re-applied to the new window instead of a dead handle.
pub fn find_overlay_hwnd() -> Option<HWND> {
    if let Ok(mut g) = OVERLAY_HWND.lock() {
        if let Some(v) = *g {
            if is_window_live(v) {
                return Some(HWND(v as *mut _));
            }
            *g = None; // stale handle — re-query below
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

/// Pure-ish check used to invalidate the cached handle: `IsWindow(0)` is
/// always false, so a freshly-initialized cache can be tested without a
/// real window.
pub fn is_window_live(hwnd: isize) -> bool {
    if hwnd == 0 {
        return false;
    }
    unsafe {
        windows::Win32::UI::WindowsAndMessaging::IsWindow(Some(HWND(hwnd as *mut _))).as_bool()
    }
}

/// Apply flags for the current mode. In overlay mode we prevent activation and
/// (optionally) pass clicks through; in interactive mode we allow normal focus/click/drag.
pub unsafe fn apply_window_mode(hwnd: HWND, click_through: bool) -> Result<(), String> {
    SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE)
        .map_err(|e| format!("SetWindowDisplayAffinity: {e}"))?;

    let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
    let mut wanted = style | WS_EX_LAYERED.0 as isize;

    if click_through {
        wanted |= WS_EX_NOACTIVATE.0 as isize;
        wanted |= WS_EX_TOOLWINDOW.0 as isize;
        wanted |= WS_EX_TRANSPARENT.0 as isize;
    } else {
        wanted &= !(WS_EX_NOACTIVATE.0 as isize);
        wanted &= !(WS_EX_TRANSPARENT.0 as isize);
        // Keep TOOLWINDOW off in interactive mode for Notepad-like taskbar/activation feel.
        wanted &= !(WS_EX_TOOLWINDOW.0 as isize);
    }

    SetWindowLongPtrW(hwnd, GWL_EXSTYLE, wanted);
    Ok(())
}

/// Back-compat: apply overlay flags (click-through off by default; caller may set).
#[allow(dead_code)]
pub unsafe fn apply_overlay_flags(hwnd: HWND) -> Result<(), String> {
    apply_window_mode(hwnd, false)
}

/// Toggle click-through mode (and related activation behavior).
pub unsafe fn set_click_through(hwnd: HWND, on: bool) {
    let _ = apply_window_mode(hwnd, on);
}

/// Idempotent re-apply for the current desired mode.
pub unsafe fn ensure_flags(hwnd: HWND, click_through: bool) {
    let _ = apply_window_mode(hwnd, click_through);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn zero_handle_is_not_live() {
        assert!(!is_window_live(0));
    }
    #[test]
    fn bogus_cached_handle_is_not_returned() {
        if let Ok(mut g) = OVERLAY_HWND.lock() {
            *g = Some(1); // 1 is never a real window
        }
        let found = find_overlay_hwnd();
        if let Some(h) = found {
            assert_ne!(h.0 as isize, 1);
        }
        if let Ok(g) = OVERLAY_HWND.lock() {
            assert_ne!(g.unwrap_or(0), 1);
        }
    }
}
