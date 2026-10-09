//! OS-level anti-capture shield.
//!
//! Windows: `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)` — the window
//! vanishes from screenshots, screen recordings, and most capture pipelines
//! while armed. This is the same primitive tao applies internally, applied
//! directly on the HWND tauri exposes.
//!
//! Other platforms: the call is a no-op there today (tauri does not surface
//! the tao API on macOS/Linux); the UI reports the shield honestly per OS.

/// Arm or disarm the capture shield for a window.
#[cfg(target_os = "windows")]
pub fn apply(window: &tauri::WebviewWindow, enabled: bool) -> Result<(), String> {
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE, WDA_NONE,
    };

    let hwnd = window
        .hwnd()
        .map_err(|e| format!("hwnd unavailable: {e}"))?;
    let affinity = if enabled {
        WDA_EXCLUDEFROMCAPTURE
    } else {
        WDA_NONE
    };
    // SAFETY: hwnd comes live from the window; the call only toggles the
    // display-affinity flag and returns a Result.
    unsafe { SetWindowDisplayAffinity(hwnd, affinity) }
        .map_err(|e| format!("SetWindowDisplayAffinity failed: {e}"))
}

/// Non-Windows: documented no-op until tauri surfaces the tao API.
#[cfg(not(target_os = "windows"))]
pub fn apply(_window: &tauri::WebviewWindow, _enabled: bool) -> Result<(), String> {
    Ok(())
}
