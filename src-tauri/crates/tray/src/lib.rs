use std::error::Error;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};

use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

#[cfg(windows)]
#[allow(non_snake_case, clippy::upper_case_acronyms)]
pub mod win {
    use core::ffi::c_void;

    pub type BOOL = i32;
    pub type HANDLE = *mut c_void;
    pub type HMENU = *mut c_void;
    pub type HWND = *mut c_void;
    pub type LPARAM = isize;
    pub type UINT = u32;
    pub type WPARAM = usize;

    #[link(name = "user32")]
    unsafe extern "system" {
        pub fn AppendMenuW(hmenu: HMENU, flags: UINT, id: WPARAM, text: *const u16) -> BOOL;
        pub fn CreatePopupMenu() -> HMENU;
        pub fn CreateWindowExW(
            ex_style: u32,
            class_name: *const u16,
            window_name: *const u16,
            style: u32,
            x: i32,
            y: i32,
            width: i32,
            height: i32,
            parent: HWND,
            menu: HMENU,
            instance: HANDLE,
            param: *mut c_void,
        ) -> HWND;
        pub fn DestroyMenu(menu: HMENU);
        pub fn DestroyWindow(hwnd: HWND) -> BOOL;
        pub fn PostMessageW(hwnd: HWND, msg: UINT, wparam: WPARAM, lparam: LPARAM) -> BOOL;
        pub fn SetForegroundWindow(hwnd: HWND) -> BOOL;
        pub fn TrackPopupMenu(
            menu: HMENU,
            flags: UINT,
            x: i32,
            y: i32,
            reserved: i32,
            owner: HWND,
            rect: *const c_void,
        ) -> u32;
    }

    pub const MF_STRING: u32 = 0x0000;
    /// A horizontal rule, used to group the two exit commands.
    pub const MF_SEPARATOR: u32 = 0x0800;
    pub const TPM_BOTTOMALIGN: u32 = 0x0020;
    pub const TPM_LEFTALIGN: u32 = 0x0000;
    pub const TPM_RETURNCMD: u32 = 0x0100;
    pub const TPM_RIGHTBUTTON: u32 = 0x0002;
    pub const WS_POPUP: u32 = 0x8000_0000;
    pub const WM_NULL: u32 = 0x0000;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayAction {
    ToggleRunning,
    Config,
    /// Close the tray, leave the renderer and its overlays running.
    Detach,
    Exit,
}

/// Owns the tray icon and relays menu actions into the eframe thread.
pub struct TrayState {
    _tray: TrayIcon,
    action_tx: mpsc::Sender<TrayAction>,
    action_rx: mpsc::Receiver<TrayAction>,
    running: AtomicBool,
    /// Whether a renderer is currently attached to the pipe.
    ///
    /// Read when the menu is built, so the pause item can say so itself rather
    /// than accepting a click it cannot honour.
    #[cfg(windows)]
    connected: AtomicBool,
    #[cfg(windows)]
    tray_hwnd: isize,
    #[cfg(windows)]
    menu_open: Arc<AtomicBool>,
}

impl TrayState {
    pub fn set_running(&self, running: bool) {
        self.running.store(running, Ordering::Release);
    }

    /// Say, in the menu itself, whether there is a renderer to talk to.
    ///
    /// A pause item that silently does nothing is indistinguishable from one
    /// that works, which is the whole reason this bug was hard to see: the tray
    /// is a background process, the log is a file nobody has open, and the only
    /// thing in front of the user is a menu. So the menu carries the fact. When
    /// there is no renderer the caption says so and the item is drawn greyed
    /// out, and clicking it is refused rather than accepted and dropped.
    #[cfg(windows)]
    pub fn set_renderer_connected(&self, connected: bool) {
        self.connected.store(connected, Ordering::Release);
        // Rebuilding is the only way to change a caption, and the next
        // right-click builds it again anyway, so this only has to be recorded.
    }

    #[cfg(windows)]
    pub fn renderer_connected(&self) -> bool {
        self.connected.load(Ordering::Acquire)
    }

    /// Show the native menu away from the eframe thread. `TrackPopupMenu` runs
    /// its own modal loop, so calling it from the tray window procedure would
    /// pause all overlay redraws until the menu closes.
    #[cfg(windows)]
    fn show_menu(&self, x: i32, y: i32) {
        // The one line that distinguishes "the click never arrived" from "the
        // menu was built and then did not appear". Every give-up path below
        // also logs, so a log with no entry here means the event was never
        // delivered, and a log with the entry but none of those means the menu
        // was handed to Windows and Windows did not show it.
        crate::diagnostics::log_line("tray", &format!("menu requested at {x},{y}"));
        if self.menu_open.swap(true, Ordering::AcqRel) {
            crate::diagnostics::log_line("tray", "menu already open, ignoring the request");
            return;
        }

        let action_tx = self.action_tx.clone();
        let running = self.running.load(Ordering::Acquire);
        let tray_hwnd = self.tray_hwnd as win::HWND;
        unsafe {
            win::SetForegroundWindow(tray_hwnd);
        }
        let menu_open = Arc::clone(&self.menu_open);
        let connected = self.renderer_connected();
        std::thread::spawn(move || {
            show_native_menu(x, y, running, connected, action_tx);
            menu_open.store(false, Ordering::Release);
        });
    }

    /// Drain tray actions and icon events without blocking the egui event loop.
    pub fn poll(&self) -> Vec<TrayAction> {
        let mut actions: Vec<TrayAction> = self.action_rx.try_iter().collect();

        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            // Every event that reaches here is named, whether or not it is one
            // this function acts on. The reason is that "the click never
            // arrived" and "the click arrived and did not match" look exactly
            // the same from outside, and two rounds of guessing at which it was
            // produced two wrong answers. A log with no event lines at all means
            // the event loop is not delivering; a line naming an event nobody
            // matched means the pattern is wrong.
            #[cfg(windows)]
            crate::diagnostics::log_line("tray", &format!("tray event: {event:?}"));

            #[cfg(windows)]
            if let TrayIconEvent::Click {
                button: MouseButton::Right,
                button_state: MouseButtonState::Up,
                position,
                ..
            } = &event
            {
                self.show_menu(position.x as i32, position.y as i32);
                continue;
            }

            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                actions.push(TrayAction::Config);
            }
        }

        actions
    }
}

#[cfg(windows)]
use ping_latency_overlay_core::diagnostics;

fn show_native_menu(
    x: i32,
    y: i32,
    running: bool,
    connected: bool,
    action_tx: mpsc::Sender<TrayAction>,
) {
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
        // This used to be a bare `return`, and it is the reason a right-click
        // that did nothing was indistinguishable from a right-click that was
        // never delivered: this process has no window, no console and no
        // stderr, so a silent return here is silent forever. Everything that
        // gives up in this function says which step gave up.
        diagnostics::log_line(
            "tray",
            "the menu could not be shown: the owner window was not created",
        );
        return;
    }

    let menu = unsafe { win::CreatePopupMenu() };
    if menu.is_null() {
        diagnostics::log_line(
            "tray",
            "the menu could not be shown: CreatePopupMenu returned nothing",
        );
        unsafe {
            win::DestroyWindow(owner);
        }
        return;
    }

    let toggle_text = wide(&if !connected {
        format!("{} (no renderer)", if running { "Pause" } else { "Resume" })
    } else if running {
        "Pause".to_string()
    } else {
        "Resume".to_string()
    });
    let config_text = wide("Config");
    // `MF_GRAYED` is 0x0001; declared here rather than in the shared `win`
    // module, which is the renderer's and the probe's and has no menu in it.
    #[allow(non_upper_case_globals)]
    const MF_GRAYED: u32 = 0x0001;
    // Two exits, because the two things a user might want are different: one
    // takes the overlays off the screen, the other just gets the icon out of
    // the notification area and leaves them running. A separator between them
    // so the pair reads as a group rather than as three unrelated commands.
    let detach_text = wide("Close tray, keep overlays running");
    let exit_text = wide("Exit");
    // `MF_GRAYED` so the item is visible but not pressable, rather than
    // missing: a caption that changes to explain itself and then does nothing
    // when clicked is the honest version, and a caption that lies is the bug.
    let toggle_flags = if connected {
        win::MF_STRING
    } else {
        win::MF_STRING | MF_GRAYED
    };
    let appended = unsafe {
        win::AppendMenuW(menu, toggle_flags, 1, toggle_text.as_ptr()) != 0
            && win::AppendMenuW(menu, win::MF_STRING, 2, config_text.as_ptr()) != 0
            && win::AppendMenuW(menu, win::MF_SEPARATOR, 0, std::ptr::null()) != 0
            && win::AppendMenuW(menu, win::MF_STRING, 3, detach_text.as_ptr()) != 0
            && win::AppendMenuW(menu, win::MF_STRING, 4, exit_text.as_ptr()) != 0
    };
    if !appended {
        diagnostics::log_line(
            "tray",
            "the menu could not be shown: AppendMenuW refused at least one item",
        );
    }

    if appended {
        // Handed to Windows. If the log stops here and comes back with no
        // command, then Windows took the menu and closed it without the user
        // choosing anything -- the classic symptom of a popup whose owner is
        // not the foreground window, which `SetForegroundWindow` is not
        // allowed to fix on its own. That is a different bug from the ones
        // above and this line is what tells them apart.
        diagnostics::log_line("tray", "the menu was handed to Windows");
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
            win::PostMessageW(owner, win::WM_NULL, 0, 0);
            // Only the pause item depends on there being a renderer. Config,
            // Close tray and Exit must work whether or not one is attached:
            // Config starts whatever is missing, and the two exits are how you
            // get out of a state with no renderer at all. An earlier version put
            // this check around the WHOLE match, which discarded all four the
            // moment the pipe went, and the symptom was a tray whose every
            // command had stopped working at once.
            let chosen = if command == 1 && !connected {
                crate::diagnostics::log_line(
                    "tray",
                    "pause was chosen with no renderer attached, so there is nothing to tell",
                );
                None
            } else {
                Some(command)
            };
            match chosen {
                Some(1) => {
                    let _ = action_tx.send(TrayAction::ToggleRunning);
                }
                Some(2) => {
                    let _ = action_tx.send(TrayAction::Config);
                }
                Some(3) => {
                    let _ = action_tx.send(TrayAction::Detach);
                }
                Some(4) => {
                    let _ = action_tx.send(TrayAction::Exit);
                }
                _ => {}
            }
        }
    }

    unsafe {
        win::DestroyMenu(menu);
        win::DestroyWindow(owner);
    }
}

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Decode the bundled artwork for the tray icon.
///
/// The PNG bytes come from `core::icon_png()` rather than a second
/// `include_bytes!` here, so there is one copy of the artwork in the build and
/// one place that names it. The configuration window decodes the same bytes
/// into an `egui::IconData` for itself, which is why this returns `Icon` and
/// not the window's type: this crate has no eframe and must not acquire one.
pub fn tray_icon() -> Result<Icon, Box<dyn Error + Send + Sync>> {
    let image = image::load_from_memory(ping_latency_overlay_core::icon_png())?.to_rgba8();
    let width = image.width();
    let height = image.height();
    Ok(Icon::from_rgba(image.into_raw(), width, height)?)
}

pub fn create() -> Result<TrayState, Box<dyn Error + Send + Sync>> {
    let icon = tray_icon()?;
    let tray = TrayIconBuilder::new()
        .with_menu_on_left_click(false)
        // The menu is rendered by a worker thread so TrackPopupMenu's modal
        // loop cannot pause the eframe redraw loop.
        .with_menu_on_right_click(false)
        .with_tooltip("PingLatencyOverlay")
        .with_icon(icon)
        .build()?;

    let (action_tx, action_rx) = mpsc::channel();
    #[cfg(windows)]
    let tray_hwnd = tray.window_handle() as isize;
    Ok(TrayState {
        _tray: tray,
        action_tx,
        action_rx,
        running: AtomicBool::new(true),
        #[cfg(windows)]
        connected: AtomicBool::new(false),
        #[cfg(windows)]
        tray_hwnd,
        #[cfg(windows)]
        menu_open: Arc::new(AtomicBool::new(false)),
    })
}
