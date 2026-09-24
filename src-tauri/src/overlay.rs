use std::collections::HashSet;

use tauri::{
    AppHandle, LogicalPosition, LogicalSize, Manager, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

use crate::config::{Anchor, Config, OverlayConfig};

pub fn overlay_label(id: &str) -> String {
    format!("overlay-{id}")
}

/// Logical window size for an overlay. Rotated graphs swap the long/short axes.
fn window_size(cfg: &OverlayConfig) -> (f64, f64) {
    let long = (cfg.window_seconds as f64) * (cfg.scale as f64);
    let short = cfg.graph_height_px as f64;
    if cfg.orientation == 90 || cfg.orientation == 270 {
        (short, long)
    } else {
        (long, short)
    }
}

/// Create, resize, reposition or close overlay windows so they match `config`.
pub fn reconcile(app: &AppHandle, config: &Config) -> tauri::Result<()> {
    let wanted: HashSet<&str> = config
        .overlays
        .iter()
        .filter(|o| o.enabled)
        .map(|o| o.id.as_str())
        .collect();

    // Close windows whose overlay was removed or disabled.
    for (label, window) in app.webview_windows() {
        if let Some(id) = label.strip_prefix("overlay-") {
            if !wanted.contains(id) {
                let _ = window.close();
            }
        }
    }

    for overlay in config.overlays.iter().filter(|o| o.enabled) {
        let label = overlay_label(&overlay.id);
        if let Some(window) = app.get_webview_window(label.as_str()) {
            let (w, h) = window_size(overlay);
            let _ = window.set_size(LogicalSize::new(w, h));
            let _ = position(&window, overlay, w, h);
        } else if let Err(err) = create(app, overlay) {
            // Log and keep going so one bad overlay doesn't block the others.
            log::error!("failed to create overlay window '{}': {err}", overlay.id);
        }
    }

    Ok(())
}

fn create(app: &AppHandle, cfg: &OverlayConfig) -> tauri::Result<()> {
    let (w, h) = window_size(cfg);
    let label = overlay_label(&cfg.id);
    let window =
        WebviewWindowBuilder::new(app, label.as_str(), WebviewUrl::App("index.html".into()))
            .title(cfg.name.clone())
            .inner_size(w, h)
            .decorations(false)
            .transparent(true)
            .always_on_top(true)
            .skip_taskbar(true)
            .resizable(false)
            .shadow(false)
            .focused(false)
            .build()?;

    // Make the overlay click-through: mouse events reach whatever is beneath it.
    window.set_ignore_cursor_events(true)?;
    position(&window, cfg, w, h)?;
    Ok(())
}

fn position(window: &WebviewWindow, cfg: &OverlayConfig, w: f64, h: f64) -> tauri::Result<()> {
    let monitor = window.current_monitor()?.or(window.primary_monitor()?);

    if let Some(monitor) = monitor {
        let scale = monitor.scale_factor();
        // Anchor within the work area (excludes the taskbar / other appbars), so
        // bottom and right overlays don't end up behind the taskbar.
        let work = monitor.work_area();
        let area_x = work.position.x as f64 / scale;
        let area_y = work.position.y as f64 / scale;
        let area_w = work.size.width as f64 / scale;
        let area_h = work.size.height as f64 / scale;

        let margin = cfg.margin_px as f64;

        let (x, y) = match cfg.position {
            Anchor::TopLeft => (area_x + margin, area_y + margin),
            Anchor::TopCenter => (area_x + (area_w - w) / 2.0, area_y + margin),
            Anchor::TopRight => (area_x + area_w - w - margin, area_y + margin),
            Anchor::CenterLeft => (area_x + margin, area_y + (area_h - h) / 2.0),
            Anchor::Center => (area_x + (area_w - w) / 2.0, area_y + (area_h - h) / 2.0),
            Anchor::CenterRight => (area_x + area_w - w - margin, area_y + (area_h - h) / 2.0),
            Anchor::BottomLeft => (area_x + margin, area_y + area_h - h - margin),
            Anchor::BottomCenter => (area_x + (area_w - w) / 2.0, area_y + area_h - h - margin),
            Anchor::BottomRight => (area_x + area_w - w - margin, area_y + area_h - h - margin),
        };

        window.set_position(LogicalPosition::new(x, y))?;
    }

    Ok(())
}

/// Re-insert every overlay window at the top of the topmost band. The taskbar
/// is topmost too and otherwise wins the z-order when it auto-shows, hiding
/// bottom-anchored overlays. Overlays are click-through, so this doesn't block
/// taskbar interaction. Call this periodically.
pub fn reassert_topmost(app: &AppHandle) {
    for (label, window) in app.webview_windows() {
        if label.starts_with("overlay-") {
            if let Ok(hwnd) = window.hwnd() {
                set_topmost(hwnd.0);
            }
        }
    }
}

#[cfg(windows)]
#[link(name = "user32")]
extern "system" {
    fn SetWindowPos(
        hwnd: *mut core::ffi::c_void,
        hwnd_insert_after: *mut core::ffi::c_void,
        x: i32,
        y: i32,
        cx: i32,
        cy: i32,
        flags: u32,
    ) -> i32;
}

#[cfg(windows)]
fn set_topmost(hwnd: *mut core::ffi::c_void) {
    const HWND_TOPMOST: *mut core::ffi::c_void = -1isize as *mut core::ffi::c_void;
    const SWP_NOSIZE: u32 = 0x0001;
    const SWP_NOMOVE: u32 = 0x0002;
    const SWP_NOACTIVATE: u32 = 0x0010;
    // SAFETY: `hwnd` comes from a live Tauri window; flags keep size/pos/focus.
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOSIZE | SWP_NOMOVE | SWP_NOACTIVATE,
        );
    }
}

#[cfg(not(windows))]
fn set_topmost(_hwnd: *mut core::ffi::c_void) {}
