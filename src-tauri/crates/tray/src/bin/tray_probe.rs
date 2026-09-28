//! The smallest tray that can still fail, on purpose.
//!
//! Everything else in this crate does four jobs at once: attaches to a
//! renderer over a pipe, supervises it with a storm guard, spawns the Config
//! window, and draws a menu. When the menu does not appear, any of those could
//! be responsible, and reading four hundred lines to decide between them is how
//! two rounds of guessing went wrong. This binary removes all of them.
//!
//! It creates an icon, shows a one-item menu on right-click, and logs each
//! step. It shares `win` and `show_native_menu` with the real tray, so the only
//! thing it does NOT do is the four jobs listed above. That is deliberate: if
//! the menu fails here, it is tray-icon or the hand-written plumbing and the
//! other three are innocent. If it works here, they go back on one at a time.
//!
//! Each log line answers one question:
//!   `started`                        the process is alive
//!   `icon created`                   the icon is real, not a stale one Windows
//!                                    is still showing from a dead process
//!   `right-click at x,y`             the event reached us, so the loop polls
//!   `menu handed to Windows`         we built it and called `TrackPopupMenu`
//!   `item chosen: N`                 the menu worked end to end
//!   `menu gave up: <reason>`         one of the silent paths fired, and says which
//!
//! No `right-click` line means the event loop is not delivering, which is above
//! this file. A `right-click` with no `menu handed to Windows` and a give-up
//! reason means this file. A `menu handed to Windows` and no `item chosen` means
//! Windows took the menu and dismissed it — the classic symptom of a popup
//! whose owner is not the foreground window.
//!
//! It has no pipe, no renderer, no storm guard, no window and no Config process,
//! and it is deliberately not in the installer.

#![cfg_attr(windows, windows_subsystem = "windows")]

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tray_icon::TrayIconBuilder;
use tray_icon::{MouseButton, MouseButtonState, TrayIconEvent};

use ping_latency_overlay_core::diagnostics;
use ping_latency_overlay_tray::win;

/// Marks a menu as open so a second right-click is reported rather than silently
/// dropped, which is one of the ways a working tray can look like a dead one.
static MENU_OPEN: AtomicBool = AtomicBool::new(false);

fn log(message: &str) {
    diagnostics::log_line("tray_probe", message);
}

fn main() {
    log("started");

    let Some(icon) = icon() else {
        return;
    };

    // Both automatic menus are off, so the only thing that can put a menu on
    // screen is our own `TrackPopupMenu` call below. If the probe shows a menu
    // the real tray would have shown one too, and if it does not, tray-icon's
    // own menu machinery is out of the picture entirely. Built through
    // `TrayIconBuilder` exactly as the real tray builds it, because a probe
    // that differs from the shipping path could fail for a reason the shipping
    // path does not have.
    // Bound to `_tray` on purpose: dropping a `TrayIcon` removes the icon from
    // the notification area, and an icon that vanishes on its own would be a
    // red herring the size of this whole exercise. Holding it for the life of
    // the loop is what keeps it on screen.
    let _tray = match TrayIconBuilder::new()
        .with_menu_on_left_click(false)
        .with_menu_on_right_click(false)
        .with_tooltip("PingLatencyOverlay probe")
        .with_icon(icon)
        .build()
    {
        Ok(tray) => tray,
        Err(error) => {
            log(&format!("icon could not be created: {error}"));
            return;
        }
    };
    log("icon created");

    loop {
        // The same call the real tray makes, and the whole point of the probe:
        // tray-icon's icon is a window on THIS thread, and Windows only delivers
        // a window message to a thread that pumps its queue. Without this the
        // icon appears and every click is dispatched into a queue nobody reads.
        ping_latency_overlay_core::overlay::pump_messages();

        if let Ok(event) = TrayIconEvent::receiver().try_recv() {
            match &event {
                TrayIconEvent::Click {
                    button: MouseButton::Right,
                    button_state: MouseButtonState::Up,
                    position,
                    ..
                } => {
                    log(&format!("right-click at {},{}", position.x, position.y));
                    show_native_menu(position.x as i32, position.y as i32);
                }
                // Logged because "the click never arrived" and "the click
                // arrived and did not match" are indistinguishable from outside,
                // and that ambiguity is what cost two rounds of guessing.
                other => log(&format!("other event: {other:?}")),
            }
        }
        std::thread::sleep(Duration::from_millis(16));
    }
}

/// The one copy of the artwork the whole build shares, decoded the same way the
/// real tray decodes it.
fn icon() -> Option<tray_icon::Icon> {
    match image::load_from_memory(ping_latency_overlay_core::icon_png()) {
        Ok(image) => {
            let image = image.to_rgba8();
            let width = image.width();
            let height = image.height();
            match tray_icon::Icon::from_rgba(image.into_raw(), width, height) {
                Ok(icon) => Some(icon),
                Err(error) => {
                    log(&format!("the icon was rejected: {error}"));
                    None
                }
            }
        }
        Err(error) => {
            log(&format!("the app icon could not be decoded: {error}"));
            None
        }
    }
}

/// `show_native_menu` from the real tray, with one item instead of five and a
/// log instead of a channel. The Win32 sequence is otherwise identical, which
/// is the point: if this differs anywhere from the shipping path, that
/// difference is a candidate for the bug.
fn show_native_menu(x: i32, y: i32) {
    if MENU_OPEN.swap(true, Ordering::AcqRel) {
        log("a menu is already open; this request is ignored");
        return;
    }
    // Clears the flag however the menu ends, including a panic.
    let _guard = OpenGuard;

    let class_name = wide("STATIC");
    let owner = unsafe {
        win::CreateWindowExW(
            0,
            class_name.as_ptr(),
            std::ptr::null(),
            win::WS_POPUP,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if owner.is_null() {
        log("menu gave up: the owner window was not created");
        return;
    }

    let menu = unsafe { win::CreatePopupMenu() };
    if menu.is_null() {
        log("menu gave up: CreatePopupMenu returned nothing");
        unsafe {
            win::DestroyWindow(owner);
        }
        return;
    }

    let text = wide("Test");
    let appended = unsafe { win::AppendMenuW(menu, win::MF_STRING, 1, text.as_ptr()) != 0 };
    if !appended {
        log("menu gave up: AppendMenuW refused the item");
    }

    if appended {
        log("menu handed to Windows");
        unsafe {
            win::SetForegroundWindow(owner);
            let command = win::TrackPopupMenu(
                menu,
                win::TPM_BOTTOMALIGN
                    | win::TPM_LEFTALIGN
                    | win::TPM_RETURNCMD
                    | win::TPM_RIGHTBUTTON,
                x,
                y,
                0,
                owner,
                std::ptr::null(),
            );
            // The documented goodbye for a menu that took the foreground, so the
            // next right-click is not swallowed by a leftover one.
            win::PostMessageW(owner, win::WM_NULL, 0, 0);
            if command == 1 {
                log("item chosen: 1");
            } else {
                log(&format!("menu closed without a choice (command {command})"));
            }
        }
    }

    unsafe {
        win::DestroyMenu(menu);
        win::DestroyWindow(owner);
    }
}

struct OpenGuard;

impl Drop for OpenGuard {
    fn drop(&mut self) {
        MENU_OPEN.store(false, Ordering::Release);
    }
}

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
