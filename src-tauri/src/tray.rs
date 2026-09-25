use std::error::Error;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};

use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

#[cfg(windows)]
#[allow(non_snake_case, clippy::upper_case_acronyms)]
mod win {
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
    Exit,
}

/// Owns the tray icon and relays menu actions into the eframe thread.
pub struct TrayState {
    _tray: TrayIcon,
    action_tx: mpsc::Sender<TrayAction>,
    action_rx: mpsc::Receiver<TrayAction>,
    running: AtomicBool,
    #[cfg(windows)]
    tray_hwnd: isize,
    #[cfg(windows)]
    menu_open: Arc<AtomicBool>,
}

impl TrayState {
    pub fn set_running(&self, running: bool) {
        self.running.store(running, Ordering::Release);
    }

    /// Show the native menu away from the eframe thread. `TrackPopupMenu` runs
    /// its own modal loop, so calling it from the tray window procedure would
    /// pause all overlay redraws until the menu closes.
    #[cfg(windows)]
    fn show_menu(&self, x: i32, y: i32) {
        if self.menu_open.swap(true, Ordering::AcqRel) {
            return;
        }

        let action_tx = self.action_tx.clone();
        let running = self.running.load(Ordering::Acquire);
        let tray_hwnd = self.tray_hwnd as win::HWND;
        unsafe {
            win::SetForegroundWindow(tray_hwnd);
        }
        let menu_open = Arc::clone(&self.menu_open);
        std::thread::spawn(move || {
            show_native_menu(x, y, running, action_tx);
            menu_open.store(false, Ordering::Release);
        });
    }

    /// Drain tray actions and icon events without blocking the egui event loop.
    pub fn poll(&self) -> Vec<TrayAction> {
        let mut actions: Vec<TrayAction> = self.action_rx.try_iter().collect();

        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
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
fn show_native_menu(x: i32, y: i32, running: bool, action_tx: mpsc::Sender<TrayAction>) {
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
        return;
    }

    let menu = unsafe { win::CreatePopupMenu() };
    if menu.is_null() {
        unsafe {
            win::DestroyWindow(owner);
        }
        return;
    }

    let toggle_text = wide(if running { "Pause" } else { "Resume" });
    let config_text = wide("Config");
    let exit_text = wide("Exit");
    let appended = unsafe {
        win::AppendMenuW(menu, win::MF_STRING, 1, toggle_text.as_ptr()) != 0
            && win::AppendMenuW(menu, win::MF_STRING, 2, config_text.as_ptr()) != 0
            && win::AppendMenuW(menu, win::MF_STRING, 3, exit_text.as_ptr()) != 0
    };

    if appended {
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
            match command {
                1 => {
                    let _ = action_tx.send(TrayAction::ToggleRunning);
                }
                2 => {
                    let _ = action_tx.send(TrayAction::Config);
                }
                3 => {
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

/// Load the bundled artwork for the native window and taskbar.
pub fn app_icon() -> eframe::egui::IconData {
    let image = image::load_from_memory(include_bytes!("../icons/icon.png"))
        .expect("bundled icon must be valid")
        .to_rgba8();
    let width = image.width();
    let height = image.height();
    eframe::egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    }
}

pub fn create() -> Result<TrayState, Box<dyn Error + Send + Sync>> {
    let image = image::load_from_memory(include_bytes!("../icons/icon.png"))?.to_rgba8();
    let width = image.width();
    let height = image.height();
    let icon = Icon::from_rgba(image.into_raw(), width, height)?;
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
        tray_hwnd,
        #[cfg(windows)]
        menu_open: Arc::new(AtomicBool::new(false)),
    })
}
