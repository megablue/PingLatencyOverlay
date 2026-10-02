//! A snapshot of the desktop's windows, for auto profile switching.
//!
//! The only impure part of the feature. [`crate::rules`] is the matching and
//! this is the reading, kept apart so the matching is testable with synthetic
//! window lists — the same split as `monitors::enumerate` vs
//! `monitors::resolve`.
//!
//! The Win32 entry points are declared here rather than pulled from a binding
//! crate, which is the crate-wide rule: `tests/no_gui_dependencies.rs` fails
//! the suite if `windows-sys` appears anywhere in this crate's graph.
//!
//! Two exclusions shape what a rule can see. Only **visible** top-level
//! windows are candidates, and windows with the `WS_EX_TOOLWINDOW` style are
//! skipped, so a rule naming a class does not match some invisible helper
//! window that happens to share it. The title is read with `GetWindowTextW`,
//! which Windows documents as safe against a hung process: it returns the
//! cached caption rather than sending `WM_GETTEXT`, so a frozen game cannot
//! freeze this process with it.

#[cfg(windows)]
mod win32 {
    use std::collections::HashMap;

    use crate::monitors::Rect;
    use crate::overlay::win::{BOOL, DWORD, HANDLE, HWND, LPARAM, POINT, RECT};
    use crate::rules::{Snapshot, WindowInfo};

    /// `WS_EX_TOOLWINDOW`, the style that marks a window as a helper rather
    /// than something a user switches to.
    const WS_EX_TOOLWINDOW: i32 = 0x0000_0080;
    /// `GWL_EXSTYLE`.
    const GWL_EXSTYLE: i32 = -20;
    /// `TH32CS_SNAPPROCESS`.
    const TH32CS_SNAPPROCESS: DWORD = 0x0000_0002;
    /// `MAX_PATH`, which is what `PROCESSENTRY32W::szExeFile` holds.
    const MAX_PATH: usize = 260;
    /// The traditional ceiling for a window class name, `MAX_CLASS_NAME`.
    const MAX_CLASS_NAME: usize = 256;
    /// The value `CreateToolhelp32Snapshot` returns on failure.
    const INVALID_HANDLE_VALUE: HANDLE = -1isize as HANDLE;
    /// `DWMWA_CLOAKED`, which reports a window its own app has hidden — the
    /// ghost windows UWP apps keep around, which would otherwise match a rule
    /// by process or class while being invisible to the user.
    const DWMWA_CLOAKED: DWORD = 14;

    /// `PROCESSENTRY32W`, laid out field by field rather than named like the
    /// header: only the offsets matter, and the SDK's spelling would need a
    /// `non_snake_case` allow to say nothing extra.
    #[repr(C)]
    struct ProcessEntry32W {
        /// `dwSize`; must be set to the size of this struct before the first
        /// call or `Process32FirstW` fails.
        size: DWORD,
        /// `cntUsage`, unused.
        _usage: DWORD,
        /// `th32ProcessID`.
        process_id: DWORD,
        /// `th32DefaultHeapID`, unused.
        _default_heap_id: usize,
        /// `th32ModuleID`, unused.
        _module_id: DWORD,
        /// `cntThreads`, unused.
        _threads: DWORD,
        /// `th32ParentProcessID`, unused.
        _parent_process_id: DWORD,
        /// `pcPriClassBase`, unused.
        _priority_class: i32,
        /// `dwFlags`, unused.
        _flags: DWORD,
        /// `szExeFile`, the executable's file name.
        exe_file: [u16; MAX_PATH],
    }

    type WndEnumProc = Option<unsafe extern "system" fn(HWND, LPARAM) -> BOOL>;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn EnumWindows(enumerator: WndEnumProc, data: LPARAM) -> BOOL;
        fn IsWindowVisible(hwnd: HWND) -> BOOL;
        fn IsIconic(hwnd: HWND) -> BOOL;
        fn GetWindowLongW(hwnd: HWND, index: i32) -> i32;
        fn GetWindowTextLengthW(hwnd: HWND) -> i32;
        fn GetWindowTextW(hwnd: HWND, text: *mut u16, max_count: i32) -> i32;
        fn GetClassNameW(hwnd: HWND, class: *mut u16, max_count: i32) -> i32;
        fn GetWindowThreadProcessId(hwnd: HWND, process_id: *mut DWORD) -> DWORD;
        fn GetForegroundWindow() -> HWND;
        fn IsWindow(hwnd: HWND) -> BOOL;
        fn GetClientRect(hwnd: HWND, rect: *mut RECT) -> BOOL;
        fn ClientToScreen(hwnd: HWND, point: *mut POINT) -> BOOL;
    }

    #[link(name = "dwmapi")]
    unsafe extern "system" {
        fn DwmGetWindowAttribute(
            hwnd: HWND,
            attribute: DWORD,
            data: *mut core::ffi::c_void,
            size: DWORD,
        ) -> i32;
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateToolhelp32Snapshot(flags: DWORD, process_id: DWORD) -> HANDLE;
        fn Process32FirstW(snapshot: HANDLE, entry: *mut ProcessEntry32W) -> BOOL;
        fn Process32NextW(snapshot: HANDLE, entry: *mut ProcessEntry32W) -> BOOL;
        fn CloseHandle(handle: HANDLE) -> BOOL;
    }

    /// What the enumeration collects: one process-name map for the whole
    /// sweep, so describing N windows costs one snapshot rather than N handle
    /// opens.
    struct Collect {
        windows: Vec<WindowInfo>,
        processes: HashMap<DWORD, String>,
    }

    /// Every process's id and executable file name right now.
    ///
    /// A Toolhelp snapshot needs no handles into the target processes, which
    /// is why this works for a game running elevated or behind an anti-cheat
    /// while a non-elevated `OpenProcess` would fail.
    unsafe fn process_names() -> HashMap<DWORD, String> {
        let mut names = HashMap::new();
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snapshot == INVALID_HANDLE_VALUE {
            return names;
        }
        let mut entry: ProcessEntry32W = std::mem::zeroed();
        entry.size = std::mem::size_of::<ProcessEntry32W>() as DWORD;
        if Process32FirstW(snapshot, &mut entry) != 0 {
            loop {
                let name = utf16_to_string(&entry.exe_file);
                if !name.is_empty() {
                    names.insert(entry.process_id, name);
                }
                if Process32NextW(snapshot, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snapshot);
        names
    }

    /// The window's title, from the cached caption.
    ///
    /// `GetWindowTextW` is documented not to send `WM_GETTEXT` to another
    /// process's window, so this cannot block on an unresponsive game.
    unsafe fn window_title(hwnd: HWND) -> String {
        let length = GetWindowTextLengthW(hwnd);
        if length <= 0 {
            return String::new();
        }
        let mut buffer = vec![0u16; length as usize + 1];
        let copied = GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
        if copied <= 0 {
            return String::new();
        }
        String::from_utf16_lossy(&buffer[..copied as usize])
    }

    unsafe fn window_class(hwnd: HWND) -> String {
        let mut buffer = [0u16; MAX_CLASS_NAME];
        let copied = GetClassNameW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
        if copied <= 0 {
            return String::new();
        }
        String::from_utf16_lossy(&buffer[..copied as usize])
    }

    /// A complete `WindowInfo`, or `None` when the owning process is unknown
    /// (it exited between the process sweep and this call).
    unsafe fn describe(hwnd: HWND, processes: &HashMap<DWORD, String>) -> Option<WindowInfo> {
        let mut process_id: DWORD = 0;
        GetWindowThreadProcessId(hwnd, &mut process_id);
        let process = processes.get(&process_id)?.clone();
        Some(WindowInfo {
            process,
            title: window_title(hwnd),
            class_name: window_class(hwnd),
        })
    }

    /// The visibility and style gate every enumeration shares.
    ///
    /// One function rather than the same tests in two callbacks: a window that
    /// `snapshot()` reports and `shapes()` does not (or the other way round)
    /// would be a rule that matches while sticky mode cannot follow it.
    unsafe fn candidate(hwnd: HWND) -> bool {
        if IsWindowVisible(hwnd) == 0 {
            return false;
        }
        if GetWindowLongW(hwnd, GWL_EXSTYLE) & WS_EX_TOOLWINDOW != 0 {
            return false;
        }
        !is_cloaked(hwnd)
    }

    /// Whether the app has cloaked the window — the ghosts UWP apps keep.
    unsafe fn is_cloaked(hwnd: HWND) -> bool {
        let mut cloaked: DWORD = 0;
        let result = DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            &mut cloaked as *mut DWORD as *mut core::ffi::c_void,
            std::mem::size_of::<DWORD>() as DWORD,
        );
        result == 0 && cloaked != 0
    }

    /// A window for sticky mode: what a matcher reads, and where it is.
    #[derive(Clone, Debug)]
    pub struct WindowShape {
        /// The window itself, for following and for owning.
        pub hwnd: HWND,
        /// What a sticky target matches against.
        pub window: WindowInfo,
        /// The client area in screen coordinates — the rectangle sticky mode
        /// treats as the screen.
        pub client: Rect,
        /// A minimized window has no usable client rectangle, so it is left
        /// out of a resolution.
        pub iconic: bool,
        /// Whether this was the focused window when the sweep ran, so a target
        /// with several windows picks the one the user is looking at.
        pub focused: bool,
    }

    /// The client area in screen coordinates, or `None` while it cannot be
    /// read.
    unsafe fn read_client_rect(hwnd: HWND) -> Option<Rect> {
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        if GetClientRect(hwnd, &mut rect) == 0 {
            return None;
        }
        let mut origin = POINT {
            x: rect.left,
            y: rect.top,
        };
        if ClientToScreen(hwnd, &mut origin) == 0 {
            return None;
        }
        Some(Rect {
            left: origin.x,
            top: origin.y,
            right: origin.x + (rect.right - rect.left),
            bottom: origin.y + (rect.bottom - rect.top),
        })
    }

    /// Whether the handle still names a window.
    ///
    /// A sticky overlay can lose its window without anything in this process
    /// asking: owning the target means Windows destroys it when the target
    /// closes, and a dead handle is what this answers.
    ///
    /// `pub(crate)` because the handle type is a raw pointer: the functions are
    /// for the overlay manager, which already lives in that world, not for a
    /// caller outside the crate to hand arbitrary pointers to.
    pub(crate) fn is_window(hwnd: HWND) -> bool {
        unsafe { IsWindow(hwnd) != 0 }
    }

    /// Whether the window is minimized.
    ///
    /// A minimized window has no client area on screen, so a sticky overlay
    /// hides rather than following its window somewhere off screen.
    pub(crate) fn is_iconic(hwnd: HWND) -> bool {
        unsafe { IsIconic(hwnd) != 0 }
    }

    /// Whether Windows considers the window visible.
    ///
    /// An app that closes to the tray — Steam is the one that made this
    /// necessary — *hides* its window rather than destroying or minimizing it,
    /// so a sticky overlay must treat "hidden" the same way it treats
    /// "minimized": hide until the window is shown again.
    pub(crate) fn is_visible(hwnd: HWND) -> bool {
        unsafe { IsWindowVisible(hwnd) != 0 }
    }

    /// The client area in screen coordinates, or `None` while it cannot be
    /// read.
    pub(crate) fn client_rect(hwnd: HWND) -> Option<Rect> {
        unsafe { read_client_rect(hwnd) }
    }

    /// What the shape sweep collects: the process map and the foreground
    /// window, so each shape can say whether it was the focused one.
    struct Shapes {
        shapes: Vec<WindowShape>,
        processes: HashMap<DWORD, String>,
        foreground: HWND,
    }

    unsafe extern "system" fn collect_shape(hwnd: HWND, data: LPARAM) -> BOOL {
        let collect = &mut *(data as *mut Shapes);
        if !candidate(hwnd) {
            return 1;
        }
        let Some(info) = describe(hwnd, &collect.processes) else {
            return 1;
        };
        let Some(client) = read_client_rect(hwnd) else {
            return 1;
        };
        collect.shapes.push(WindowShape {
            hwnd,
            window: info,
            client,
            iconic: IsIconic(hwnd) != 0,
            focused: hwnd == collect.foreground,
        });
        1 // Keep enumerating.
    }

    /// Every visible top-level window with its client area, topmost first.
    ///
    /// The same candidates `snapshot()` reports and the same ordering
    /// `EnumWindows` gives, which is z-order from the top, so "the first match"
    /// is the one a user can actually see over the others.
    pub fn shapes() -> Vec<WindowShape> {
        let processes = unsafe { process_names() };
        let foreground = unsafe { GetForegroundWindow() };
        let mut collect = Shapes {
            shapes: Vec::new(),
            processes,
            foreground,
        };
        unsafe {
            EnumWindows(Some(collect_shape), &mut collect as *mut Shapes as LPARAM);
        }
        collect.shapes
    }

    unsafe extern "system" fn collect_window(hwnd: HWND, data: LPARAM) -> BOOL {
        let collect = &mut *(data as *mut Collect);
        if !candidate(hwnd) {
            return 1;
        }
        if let Some(info) = describe(hwnd, &collect.processes) {
            collect.windows.push(info);
        }
        1 // Keep enumerating.
    }

    fn utf16_to_string(buffer: &[u16]) -> String {
        let end = buffer
            .iter()
            .position(|unit| *unit == 0)
            .unwrap_or(buffer.len());
        String::from_utf16_lossy(&buffer[..end])
    }

    pub fn snapshot() -> Snapshot {
        let processes = unsafe { process_names() };
        let mut collect = Collect {
            windows: Vec::new(),
            processes,
        };
        unsafe {
            EnumWindows(Some(collect_window), &mut collect as *mut Collect as LPARAM);
        }
        let foreground = unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.is_null() {
                None
            } else {
                describe(hwnd, &collect.processes)
            }
        };
        Snapshot {
            windows: collect.windows,
            foreground,
        }
    }
}

#[cfg(windows)]
pub(crate) use win32::{client_rect, is_iconic, is_visible, is_window};
#[cfg(windows)]
pub use win32::{shapes, snapshot, WindowShape};

/// On a platform without these windows there are none to report.
#[cfg(not(windows))]
pub fn snapshot() -> crate::rules::Snapshot {
    crate::rules::Snapshot::default()
}

/// On a platform without these windows there is nothing to follow.
#[cfg(not(windows))]
pub fn shapes() -> Vec<WindowShape> {
    Vec::new()
}

/// There are no handles away from Windows; these exist so the renderer's
/// resolver still compiles.
#[cfg(not(windows))]
pub(crate) fn is_window(_hwnd: isize) -> bool {
    false
}

#[cfg(not(windows))]
pub(crate) fn is_iconic(_hwnd: isize) -> bool {
    false
}

#[cfg(not(windows))]
pub(crate) fn is_visible(_hwnd: isize) -> bool {
    false
}

#[cfg(not(windows))]
pub(crate) fn client_rect(_hwnd: isize) -> Option<crate::monitors::Rect> {
    None
}

/// On a platform without these windows there is nothing to follow; the type
/// exists so the resolver still compiles.
#[cfg(not(windows))]
#[derive(Clone, Debug)]
pub struct WindowShape {
    pub hwnd: isize,
    pub window: crate::rules::WindowInfo,
    pub client: crate::monitors::Rect,
    pub iconic: bool,
    pub focused: bool,
}
