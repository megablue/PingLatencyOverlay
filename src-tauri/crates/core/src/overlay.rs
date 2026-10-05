use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::border::{border_frame_interval, BorderAnimator};
use crate::config::{
    smooth_frame_interval, Anchor, Config, DisplayMode, OverlayConfig, StickyTarget, StickyZOrder,
    TargetConfig,
};
use crate::monitors::{MonitorInfo, Rect};
use crate::probes::SampleStore;
use crate::render::{
    cosmetic_prefill_samples, line_glow_reserve_px, render_series_into_with_border,
    sample_cursor_reserve_px, sample_cursor_room_px, sample_gap_threshold, CursorAnimation,
    SamplePoint, Series,
};
use crate::rules::CompiledMatcher;
use crate::sticky;
use crate::winwatch;

#[cfg(test)]
use crate::render::cosmetic_prefill_values;

/// The Win32 surface this crate needs, declared here rather than taken from a
/// binding crate.
///
/// `pub(crate)` so `monitors` can share it: the display enumeration and the
/// layered-window code both need `RECT`, `BOOL` and `GetMonitorInfoW`, and a
/// second `#[repr(C)] RECT` in a sibling module is exactly the kind of thing
/// that drifts.
#[cfg(windows)]
#[allow(non_snake_case, clippy::upper_case_acronyms)]
pub(crate) mod win {
    use core::ffi::c_void;

    pub type BOOL = i32;
    pub type BYTE = u8;
    pub type DWORD = u32;
    pub type HANDLE = *mut c_void;
    pub type HDC = HANDLE;
    pub type HGDIOBJ = HANDLE;
    pub type HINSTANCE = HANDLE;
    pub type HMONITOR = HANDLE;
    pub type HWND = HANDLE;
    pub type HMENU = HANDLE;
    pub type LPARAM = isize;
    pub type LRESULT = isize;
    pub type UINT = u32;
    pub type WPARAM = usize;

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct RECT {
        pub left: i32,
        pub top: i32,
        pub right: i32,
        pub bottom: i32,
    }

    /// One entry of a thread's message queue.
    ///
    /// Declared here, with the rest of the Win32 surface, rather than taken
    /// from `windows-sys`: this crate must never gain that dependency, and the
    /// field order below is the one `PeekMessageW` writes into.
    #[repr(C)]
    pub struct MSG {
        pub hwnd: HWND,
        pub message: UINT,
        pub wParam: WPARAM,
        pub lParam: LPARAM,
        pub time: DWORD,
        pub pt: POINT,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct POINT {
        pub x: i32,
        pub y: i32,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct SIZE {
        pub cx: i32,
        pub cy: i32,
    }

    /// `MONITORINFO` with `szDevice` appended, so the display's own name can be
    /// read. Passing the larger struct to `GetMonitorInfoW` is correct and
    /// documented: `cbSize` is how the callee knows what it was handed.
    #[repr(C)]
    pub struct MONITORINFOEXW {
        pub cbSize: DWORD,
        pub rcMonitor: RECT,
        pub rcWork: RECT,
        pub dwFlags: DWORD,
        /// `CCHDEVICENAMEW`: 32 `WCHAR`s, null terminated.
        pub szDevice: [u16; 32],
    }

    /// The callback `EnumDisplayMonitors` calls once per display. The third
    /// parameter is the intersection of the monitor with the caller's `HDC`
    /// rect, which is null when the whole virtual desktop was asked for.
    pub type MONITORENUMPROC =
        Option<unsafe extern "system" fn(HMONITOR, HDC, *mut RECT, LPARAM) -> BOOL>;

    #[repr(C)]
    pub struct WNDCLASSW {
        pub style: UINT,
        pub lpfnWndProc: Option<unsafe extern "system" fn(HWND, UINT, WPARAM, LPARAM) -> LRESULT>,
        pub cbClsExtra: i32,
        pub cbWndExtra: i32,
        pub hInstance: HINSTANCE,
        pub hIcon: HANDLE,
        pub hCursor: HANDLE,
        pub hbrBackground: HANDLE,
        pub lpszMenuName: *const u16,
        pub lpszClassName: *const u16,
    }

    #[repr(C)]
    pub struct BITMAPINFOHEADER {
        pub biSize: DWORD,
        pub biWidth: i32,
        pub biHeight: i32,
        pub biPlanes: u16,
        pub biBitCount: u16,
        pub biCompression: DWORD,
        pub biSizeImage: DWORD,
        pub biXPelsPerMeter: i32,
        pub biYPelsPerMeter: i32,
        pub biClrUsed: DWORD,
        pub biClrImportant: DWORD,
    }

    #[repr(C)]
    pub struct BITMAPINFO {
        pub bmiHeader: BITMAPINFOHEADER,
        pub bmiColors: [u32; 3],
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct BLENDFUNCTION {
        pub BlendOp: BYTE,
        pub BlendFlags: BYTE,
        pub SourceConstantAlpha: BYTE,
        pub AlphaFormat: BYTE,
    }

    #[link(name = "user32")]
    unsafe extern "system" {
        pub fn CreateWindowExW(
            dwExStyle: DWORD,
            lpClassName: *const u16,
            lpWindowName: *const u16,
            dwStyle: DWORD,
            x: i32,
            y: i32,
            nWidth: i32,
            nHeight: i32,
            hWndParent: HWND,
            hMenu: HMENU,
            hInstance: HINSTANCE,
            lpParam: HANDLE,
        ) -> HWND;
        pub fn DefWindowProcW(hwnd: HWND, msg: UINT, wparam: WPARAM, lparam: LPARAM) -> LRESULT;
        pub fn DestroyWindow(hwnd: HWND) -> BOOL;
        pub fn GetDC(hwnd: HWND) -> HDC;
        pub fn ReleaseDC(hwnd: HWND, hdc: HDC) -> i32;
        pub fn RegisterClassW(lpwcx: *const WNDCLASSW) -> u16;
        pub fn UnregisterClassW(lpclassname: *const u16, hinstance: HINSTANCE) -> BOOL;
        pub fn GetModuleHandleW(lpmodname: *const u16) -> HINSTANCE;
        pub fn GetStockObject(id: i32) -> HANDLE;
        pub fn SetWindowPos(
            hwnd: HWND,
            hwndinsertafter: HWND,
            x: i32,
            y: i32,
            cx: i32,
            cy: i32,
            flags: DWORD,
        ) -> BOOL;
        pub fn ShowWindow(hwnd: HWND, ncmdshow: i32) -> BOOL;
        pub fn UpdateWindow(hwnd: HWND) -> BOOL;
        pub fn GetMonitorInfoW(hmonitor: HANDLE, monitorinfo: *mut MONITORINFOEXW) -> BOOL;
        /// The export name is `EnumDisplayMonitors`, with no `W`.
        ///
        /// The header calls this `EnumDisplayMonitorsW` and the SDK's
        /// `user32.lib` has no such member — `dumpbin /LINKERMEMBER` lists
        /// `EnumDisplayDevicesW`, `EnumDisplaySettingsW` and every other
        /// wide/narrow pair separately, and one bare `EnumDisplayMonitors`
        /// beside them. There is no A/W split to make here anyway: the
        /// function takes no string argument. So this links to the name that is
        /// actually there, and a `W` on the declaration would be a link error
        /// rather than a wrong answer.
        #[link_name = "EnumDisplayMonitors"]
        pub fn EnumDisplayMonitorsW(
            hmonitor: HANDLE,
            hdc: HDC,
            lpenumfunc: MONITORENUMPROC,
            dwdata: LPARAM,
        ) -> BOOL;
        pub fn SetProcessDpiAwarenessContext(value: HANDLE) -> BOOL;
        pub fn ValidateRect(hwnd: HWND, rect: *const RECT) -> BOOL;
        pub fn PeekMessageW(
            lpmsg: *mut MSG,
            hwnd: HWND,
            wmsgfiltermMin: UINT,
            wmsgfiltermMax: UINT,
            wremoveflag: UINT,
        ) -> BOOL;
        pub fn TranslateMessage(lpmsg: *const MSG) -> BOOL;
        pub fn DispatchMessageW(lpmsg: *const MSG) -> LRESULT;
        pub fn GetShellWindow() -> HWND;
        pub fn GetWindow(hwnd: HWND, ucmd: UINT) -> HWND;
        pub fn GetWindowLongPtrW(hwnd: HWND, nindex: i32) -> isize;
        /// Sets the owner of a top-level window (or a value on it, by index).
        pub fn SetWindowLongPtrW(hwnd: HWND, nindex: i32, value: isize) -> isize;
    }

    #[link(name = "gdi32")]
    unsafe extern "system" {
        pub fn CreateCompatibleDC(hdc: HDC) -> HDC;
        pub fn CreateDIBSection(
            hdc: HDC,
            pbmi: *const BITMAPINFO,
            usage: UINT,
            ppvbits: *mut HANDLE,
            hsection: HANDLE,
            offset: DWORD,
        ) -> HANDLE;
        pub fn SelectObject(hdc: HDC, obj: HGDIOBJ) -> HGDIOBJ;
        pub fn DeleteDC(hdc: HDC) -> BOOL;
        pub fn DeleteObject(obj: HGDIOBJ) -> BOOL;
        pub fn UpdateLayeredWindow(
            hwnd: HWND,
            hdcdst: HDC,
            pptdst: *const POINT,
            psize: *const SIZE,
            hdcsrc: HDC,
            pptsrc: *const POINT,
            crkey: DWORD,
            pblend: *const BLENDFUNCTION,
            dwflags: DWORD,
        ) -> BOOL;
    }
}

#[cfg(windows)]
use win::{
    CreateCompatibleDC, CreateDIBSection, CreateWindowExW, DefWindowProcW, DeleteDC, DeleteObject,
    DestroyWindow, DispatchMessageW, GetDC, GetModuleHandleW, GetShellWindow, GetStockObject,
    GetWindow, GetWindowLongPtrW, PeekMessageW, RegisterClassW, ReleaseDC, SelectObject,
    SetProcessDpiAwarenessContext, SetWindowLongPtrW, SetWindowPos, ShowWindow, TranslateMessage,
    UnregisterClassW, UpdateLayeredWindow, UpdateWindow, ValidateRect, BITMAPINFO, BLENDFUNCTION,
    HDC, HGDIOBJ, HINSTANCE, HWND, LPARAM, LRESULT, MSG, POINT, RECT, SIZE, UINT, WNDCLASSW,
    WPARAM,
};

#[cfg(windows)]
const WS_POPUP: u32 = 0x8000_0000;
#[cfg(windows)]
const WS_EX_TOPMOST: u32 = 0x0000_0008;
#[cfg(windows)]
const WS_EX_LAYERED: u32 = 0x0008_0000;
#[cfg(windows)]
const WS_EX_TRANSPARENT: u32 = 0x0000_0020;
#[cfg(windows)]
const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;
#[cfg(windows)]
const WS_EX_NOACTIVATE: u32 = 0x0800_0000;
#[cfg(windows)]
const HWND_TOPMOST: HWND = -1isize as HWND;
/// The z-order argument that takes a window out of the topmost band (and clears
/// `WS_EX_TOPMOST`), which is how wallpaper mode starts.
#[cfg(windows)]
const HWND_NOTOPMOST: HWND = -2isize as HWND;
#[cfg(windows)]
const GWL_EXSTYLE: i32 = -20;
/// `GWLP_HWNDPARENT`, the index `SetWindowLongPtrW` takes to set a top-level
/// window's owner — the relationship that puts a sticky overlay in its target's
/// z-order band instead of above every window.
#[cfg(windows)]
const GWLP_HWNDPARENT: i32 = -8;
/// `GetWindow` argument for the window directly below this one in z-order.
#[cfg(windows)]
const GW_HWNDNEXT: u32 = 3;
/// The system-command message and the minimize request within it.
///
/// Show Desktop minimizes every window; a wallpaper-mode overlay refuses that
/// instead of vanishing from the desktop it belongs to.
#[cfg(windows)]
const WM_SYSCOMMAND: u32 = 0x0112;
#[cfg(windows)]
const SC_MINIMIZE: usize = 0xF020;
#[cfg(windows)]
const SWP_NOSIZE: u32 = 0x0001;
#[cfg(windows)]
const SWP_NOMOVE: u32 = 0x0002;
#[cfg(windows)]
const SWP_NOACTIVATE: u32 = 0x0010;
#[cfg(windows)]
const SWP_SHOWWINDOW: u32 = 0x0040;
#[cfg(windows)]
const ULW_ALPHA: u32 = 0x0000_0002;
#[cfg(windows)]
const BI_RGB: u32 = 0;
/// Tells `PeekMessageW` to take the message out of the queue.
#[cfg(windows)]
const PM_REMOVE: u32 = 0x0001;
#[cfg(windows)]
const DIB_RGB_COLORS: u32 = 0;
#[cfg(windows)]
const CS_HREDRAW: u32 = 0x0002;
#[cfg(windows)]
const CS_VREDRAW: u32 = 0x0001;
#[cfg(windows)]
const WM_PAINT: u32 = 0x000F;
#[cfg(windows)]
const WM_ERASEBKGND: u32 = 0x0014;
#[cfg(windows)]
const WM_MOUSEACTIVATE: u32 = 0x0021;
#[cfg(windows)]
const WM_NCHITTEST: u32 = 0x0084;

/// A window that is asked to close, and a session that is being shut down.
///
/// Both mean the same thing to us: this process is finished.
const WM_CLOSE: u32 = 0x0010;
const WM_QUERYENDSESSION: u32 = 0x0011;

/// Set by the window procedure when Windows asks this process to close.
///
/// A static is the right shape rather than a convenience: the overlay windows
/// belong to the renderer's one loop thread, so the flag is written and read on
/// the same thread and needs no locking and no channel. The window procedure
/// cannot stop the loop by returning — it is called from inside the pump, which
/// the loop owns — so all it can do is record the request, and the loop looks.
static QUIT_REQUESTED: AtomicBool = AtomicBool::new(false);

/// Whether Windows has asked this process to close.
///
/// Checked once per pass of the renderer's loop. The loop is the only thing
/// that can act on it, because the loop is what has to finish: stop probing,
/// drop the overlays, release the mutex, exit.
pub fn quit_requested() -> bool {
    QUIT_REQUESTED.load(Ordering::Relaxed)
}
#[cfg(windows)]
const HTTRANSPARENT: isize = -1;
#[cfg(windows)]
const MA_NOACTIVATE: isize = 3;
#[cfg(windows)]
const AC_SRC_OVER: u8 = 0x00;
#[cfg(windows)]
const AC_SRC_ALPHA: u8 = 0x01;
#[cfg(windows)]
const SW_SHOWNOACTIVATE: i32 = 4;
/// Hides the window without destroying it, which is what a missing monitor
/// calls for: the overlay has to come back where it was when the monitor does.
#[cfg(windows)]
const SW_HIDE: i32 = 0;
#[cfg(windows)]
const MAX_RENDER_DIMENSION: f64 = 2048.0;

/// Let Windows give us physical coordinates for the layered windows.
pub fn enable_dpi_awareness() {
    #[cfg(windows)]
    unsafe {
        // DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2
        let _ = SetProcessDpiAwarenessContext(-4isize as *mut c_void);
    }
}

/// Drain this thread's Windows message queue.
///
/// **Any thread in any of our processes that creates a window must call this.**
/// It started here, in the renderer, and it was left out of the tray, which is
/// where the same mistake was made twice: the tray icon is a window too, and
/// Windows only delivers a window message to a thread that pumps its queue, so
/// a tray that sleeps instead of pumping looks completely fine — the icon is on
/// screen — and is completely dead. tray-icon says as much at the top of its own
/// documentation: "an event loop must be running on the thread."
///
/// The overlay windows are ordinary Win32 windows, so Windows posts messages to
/// the thread that owns them: the cursor changes when the pointer moves, hover
/// tracking ticks, and the window is asked to repaint itself. A thread that
/// blocks without pumping leaves all of those queued and unanswered, and after a
/// few seconds Windows decides the window is hung — which is what the user sees
/// as a spinning cursor and a "Not responding" process.
///
/// This only peeks, so it never blocks: a thread waiting for work calls it
/// between waits rather than instead of them. It is not a substitute for a real
/// message loop, and deliberately not one — the windows are click-through and
/// the tray is an icon, so none of these processes has input to receive.
pub fn pump_messages() {
    #[cfg(windows)]
    unsafe {
        let mut message: MSG = zeroed();
        // PM_NOREMOVE would report the same message forever, so take each one
        // out of the queue as it is handled.
        while PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_REMOVE) != 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}
// history after the reveal and are never inserted into SampleStore.
struct PrefillState {
    started_at: Instant,
    duration: Duration,
    /// One cosmetic curve per enabled target, in the overlay's target order, so
    /// it can be zipped with the live samples without a lookup.
    samples: Vec<Vec<SamplePoint>>,
    completed_rendered: bool,
}

impl PrefillState {
    fn new(config: &OverlayConfig, now: Instant) -> Self {
        Self {
            started_at: now,
            duration: Duration::from_secs(config.prefill_animation_sec.max(1) as u64),
            samples: config
                .targets
                .iter()
                .filter(|target| target.enabled)
                .map(|target| cosmetic_prefill_samples(config, &target.id, now))
                .collect(),
            completed_rendered: false,
        }
    }

    fn progress(&self, now: Instant) -> f32 {
        (now.saturating_duration_since(self.started_at).as_secs_f64() / self.duration.as_secs_f64())
            .clamp(0.0, 1.0) as f32
    }

    fn is_complete(&self, now: Instant) -> bool {
        self.progress(now) >= 1.0
    }
}

/// One target's line as the window currently holds it.
struct WindowSeries {
    /// The target id, so a reordered or removed target can be matched up.
    target_id: String,
    samples: Vec<SamplePoint>,
    /// The generation the samples were read at, so "did this change" is a
    /// comparison rather than a length check that misses an equal-length update.
    generation: u64,
    /// The prefill and the live samples merged in time order.
    history: Vec<SamplePoint>,
    /// Where the sample cursor was last drawn, and whether it is still easing.
    cursor: CursorAnimation,
}

impl WindowSeries {
    fn new(target_id: &str) -> Self {
        Self {
            target_id: target_id.to_string(),
            samples: Vec::new(),
            generation: 0,
            history: Vec::new(),
            cursor: CursorAnimation::default(),
        }
    }
}

struct OverlayWindow {
    hwnd: HWND,
    config: OverlayConfig,
    /// One entry per enabled target, in the overlay's target order. The order
    /// is the config's, so a reorder changes which line is drawn first and
    /// nothing else about the frame.
    series: Vec<WindowSeries>,
    prefill: Option<PrefillState>,
    /// Scratch for the in-progress reveal: one truncated curve per target,
    /// reused every frame so the reveal does not allocate at frame rate.
    prefill_points: Vec<Vec<SamplePoint>>,
    history_dirty: bool,
    border: BorderAnimator,
    border_selected: bool,
    pixels: Vec<u8>,
    /// Whether any target has ever produced a sample.
    ///
    /// Per overlay rather than per target, because it answers one question:
    /// has the real graph started? A group where one target has answered and
    /// another has not is past the prefill, and the one that has not draws an
    /// empty line rather than a fake one.
    sample_generation: u64,
    size: (i32, i32),
    position: (i32, i32),
    /// Set when the display this overlay is pinned to is not attached, or its
    /// sticky target is closed or minimized, so the window is off screen but
    /// kept.
    ///
    /// Kept rather than destroyed so the graph comes back at exactly the size
    /// and position the user chose when the display or target returns. The
    /// field exists because `ShowWindow(hwnd, SW_HIDE)` is not the only way a
    /// window ends up invisible, and the two repaint intervals and the z-order
    /// reassert have to all skip it rather than each guess.
    hidden: bool,
    dirty: bool,
    last_rendered: Instant,
    surface: LayeredSurface,
}

impl OverlayWindow {
    /// The prefill curve belonging to the target at `index`, if there is one.
    ///
    /// Indexed rather than keyed because the prefill is built from the same
    /// filtered list as `series`, in the same order, so the two cannot disagree
    /// — and a lookup that could come back empty would have to be handled as a
    /// state that this pair makes unrepresentable.
    fn prefill_samples(&self, index: usize) -> Option<&[SamplePoint]> {
        self.prefill.as_ref()?.samples.get(index).map(Vec::as_slice)
    }

    fn rebuild_history(&mut self) {
        // Borrow the two fields separately rather than the whole window, which
        // is what calling `self.prefill_samples` inside the loop would need.
        let prefill = self.prefill.as_ref();
        for (index, series) in self.series.iter_mut().enumerate() {
            series.history.clear();
            if let Some(samples) = prefill.and_then(|state| state.samples.get(index)) {
                series.history.extend(samples.iter().copied());
            }
            series.history.extend(series.samples.iter().copied());
            series.history.sort_by_key(|sample| sample.timestamp);
        }
        self.history_dirty = false;
    }

    /// Rebuild the per-target series list to match the configuration.
    fn sync_series(&mut self, targets: &[TargetConfig]) -> bool {
        sync_series(&mut self.series, targets)
    }
}

/// Bring a window's series list in line with the configured targets, keeping the
/// entries whose target id is still there.
///
/// Free rather than a method on `OverlayWindow` so it can be tested without one:
/// a window owns an `HWND` and a `LayeredSurface`, so a test that had to build
/// one to exercise a list operation would need a real window and would then be
/// skipped on anything but Windows.
///
/// The returned flag is what `apply` uses to ask for a frame. It matters more
/// than it looks: a host added while the samples and the rest of the config are
/// both unchanged is invisible to every other trigger in `apply`, so without
/// this the new line appears one frame late — or not at all, if nothing else
/// asks for a redraw.
fn sync_series(series: &mut Vec<WindowSeries>, targets: &[TargetConfig]) -> bool {
    let wanted: Vec<&TargetConfig> = targets.iter().filter(|t| t.enabled).collect();
    if series.len() == wanted.len()
        && series
            .iter()
            .zip(wanted.iter())
            .all(|(existing, target)| existing.target_id == target.id)
    {
        return false;
    }

    // Keyed by id rather than by position, so a reorder or a removal above a
    // target does not reset the samples of the ones below it. That would show as
    // every remaining line restarting from scratch.
    let mut previous: HashMap<String, WindowSeries> = std::mem::take(series)
        .into_iter()
        .map(|entry| (entry.target_id.clone(), entry))
        .collect();
    *series = wanted
        .iter()
        .map(|target| {
            previous
                .remove(&target.id)
                .unwrap_or_else(|| WindowSeries::new(&target.id))
        })
        .collect();
    true
}

// Keep the DIB, source DC, and BGRA staging buffer alive for the lifetime of
// an overlay. Smooth mode can call UpdateLayeredWindow at display frequency;
// recreating these GDI objects for every frame would needlessly stress the
// graphics subsystem.
struct LayeredSurface {
    size: (i32, i32),
    screen_dc: HDC,
    memory_dc: HDC,
    bitmap: HGDIOBJ,
    old_object: HGDIOBJ,
    bits: HGDIOBJ,
    bgra: Vec<u8>,
}

impl LayeredSurface {
    fn new(size: (i32, i32)) -> Result<Self, Box<dyn Error + Send + Sync>> {
        if size.0 <= 0 || size.1 <= 0 {
            return Err("layered surface has invalid dimensions".into());
        }

        unsafe {
            let screen_dc = GetDC(ptr::null_mut());
            if screen_dc.is_null() {
                return Err("GetDC failed for layered surface".into());
            }
            let memory_dc = CreateCompatibleDC(screen_dc);
            if memory_dc.is_null() {
                ReleaseDC(ptr::null_mut(), screen_dc);
                return Err("CreateCompatibleDC failed for layered surface".into());
            }

            let mut bitmap_info: BITMAPINFO = zeroed();
            bitmap_info.bmiHeader.biSize =
                size_of::<crate::overlay::win::BITMAPINFOHEADER>() as u32;
            bitmap_info.bmiHeader.biWidth = size.0;
            bitmap_info.bmiHeader.biHeight = -size.1;
            bitmap_info.bmiHeader.biPlanes = 1;
            bitmap_info.bmiHeader.biBitCount = 32;
            bitmap_info.bmiHeader.biCompression = BI_RGB;

            let mut bits: HGDIOBJ = ptr::null_mut();
            let bitmap = CreateDIBSection(
                screen_dc,
                &bitmap_info,
                DIB_RGB_COLORS,
                &mut bits,
                ptr::null_mut(),
                0,
            );
            if bitmap.is_null() {
                DeleteDC(memory_dc);
                ReleaseDC(ptr::null_mut(), screen_dc);
                return Err("CreateDIBSection failed for layered surface".into());
            }
            if bits.is_null() {
                DeleteObject(bitmap);
                DeleteDC(memory_dc);
                ReleaseDC(ptr::null_mut(), screen_dc);
                return Err("CreateDIBSection returned no pixels".into());
            }

            let old_object = SelectObject(memory_dc, bitmap);
            if old_object.is_null() {
                DeleteObject(bitmap);
                DeleteDC(memory_dc);
                ReleaseDC(ptr::null_mut(), screen_dc);
                return Err("SelectObject failed for layered surface".into());
            }

            let byte_len = (size.0 as usize)
                .saturating_mul(size.1 as usize)
                .saturating_mul(4);
            Ok(Self {
                size,
                screen_dc,
                memory_dc,
                bitmap,
                old_object,
                bits,
                bgra: vec![0; byte_len],
            })
        }
    }

    #[allow(clippy::chunks_exact_to_as_chunks)]
    fn update(&mut self, hwnd: HWND, position: (i32, i32), rgba: &[u8]) -> bool {
        if rgba.len() != self.bgra.len() {
            return false;
        }
        for (src, dst) in rgba.chunks_exact(4).zip(self.bgra.chunks_exact_mut(4)) {
            dst[0] = src[2];
            dst[1] = src[1];
            dst[2] = src[0];
            dst[3] = src[3];
        }

        unsafe {
            ptr::copy_nonoverlapping(self.bgra.as_ptr(), self.bits.cast::<u8>(), self.bgra.len());
            let destination = POINT {
                x: position.0,
                y: position.1,
            };
            let source = POINT { x: 0, y: 0 };
            let window_size = SIZE {
                cx: self.size.0,
                cy: self.size.1,
            };
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA,
            };
            UpdateLayeredWindow(
                hwnd,
                self.screen_dc,
                &destination,
                &window_size,
                self.memory_dc,
                &source,
                0,
                &blend,
                ULW_ALPHA,
            ) != 0
        }
    }
}

impl Drop for LayeredSurface {
    fn drop(&mut self) {
        unsafe {
            if !self.memory_dc.is_null() {
                if !self.old_object.is_null() {
                    SelectObject(self.memory_dc, self.old_object);
                }
                DeleteObject(self.bitmap);
                DeleteDC(self.memory_dc);
            }
            if !self.screen_dc.is_null() {
                ReleaseDC(ptr::null_mut(), self.screen_dc);
            }
        }
    }
}

/// What one sticky overlay is following, and what it last saw.
///
/// Parallel to `windows` rather than a field on [`OverlayWindow`]: the window
/// is destroyed and recreated — by a config edit, or by Windows itself when
/// the window an overlay was owned by closes — while the target the user chose
/// has to survive that. It is also the state that exists for an overlay that
/// has no window at all, so the overlay can come back when its window does.
#[derive(Default)]
struct StickyFollow {
    /// The conditions the handle was resolved with, rebuilt when the target is
    /// edited. Empty conditions can never match, so an unconfigured target
    /// resolves to nothing.
    matcher: CompiledMatcher,
    /// The target the matcher was built from, so an edit rebuilds it.
    target: Option<StickyTarget>,
    /// The window being followed, while it is still alive.
    hwnd: Option<HWND>,
    /// The target's client area in screen coordinates.
    client: Rect,
    /// The DPI scale of the display the target is on.
    scale: f32,
    /// The bounds of the display `scale` came from, so a window moving within
    /// one display does not re-enumerate displays on every frame.
    display: Option<Rect>,
    /// Whether the last poll found somewhere to put the overlay.
    placed: bool,
    /// When the desktop was last swept for a match. A target that is not
    /// running yet costs one sweep per second rather than one per frame.
    scanned_at: Option<Instant>,
}

pub struct OverlayManager {
    instance: HINSTANCE,
    class_name: Vec<u16>,
    windows: HashMap<String, OverlayWindow>,
    /// One entry per enabled sticky overlay, whether or not it has a window.
    sticky: HashMap<String, StickyFollow>,
    last_topmost: Instant,
}

impl OverlayManager {
    pub fn new() -> Result<Self, Box<dyn Error + Send + Sync>> {
        let instance = unsafe { GetModuleHandleW(ptr::null()) };
        if instance.is_null() {
            return Err("GetModuleHandleW failed".into());
        }
        let class_name = wide("PingLatencyOverlayLayeredWindow")?;
        let mut wc: WNDCLASSW = unsafe { zeroed() };
        wc.style = CS_HREDRAW | CS_VREDRAW;
        wc.lpfnWndProc = Some(overlay_wnd_proc);
        wc.hInstance = instance;
        wc.hbrBackground = unsafe { GetStockObject(4) };
        wc.lpszClassName = class_name.as_ptr();
        let atom = unsafe { RegisterClassW(&wc) };
        if atom == 0 {
            return Err("failed to register overlay window class".into());
        }
        Ok(Self {
            instance,
            class_name,
            windows: HashMap::new(),
            sticky: HashMap::new(),
            last_topmost: Instant::now(),
        })
    }

    /// Reconcile native windows and redraw when config, samples, smooth rendering,
    /// prefill animation, or border animation requires it.
    pub fn apply(
        &mut self,
        config: &Config,
        samples: &SampleStore,
        running: bool,
        selected_id: Option<&str>,
    ) {
        let wanted: HashSet<String> = config
            .overlays
            .iter()
            .filter(|overlay| overlay.enabled)
            .map(|overlay| overlay.id.clone())
            .collect();

        let removed: Vec<String> = self
            .windows
            .keys()
            .filter(|id| !wanted.contains(*id))
            .cloned()
            .collect();
        for id in removed {
            if let Some(window) = self.windows.remove(&id) {
                unsafe {
                    DestroyWindow(window.hwnd);
                }
            }
        }

        // Enumerated once per pass rather than per overlay: the answer is the
        // same for every overlay on screen, and `EnumDisplayMonitors` is a
        // round trip into the window manager.
        let connected = crate::monitors::enumerate();
        for overlay in config.overlays.iter().filter(|overlay| overlay.enabled) {
            let selected = selected_id == Some(overlay.id.as_str());
            // Where this overlay belongs, and — for a sticky overlay in its
            // target's z-order band — what owns it. Decided before the window
            // is touched, so an overlay with nowhere to go is hidden rather
            // than created.
            let (size, position, owner) = match overlay.display_mode {
                DisplayMode::Sticky => {
                    // `follow_sticky` runs before this on every renderer pass,
                    // so an enabled sticky overlay has a state here.
                    let Some(follow) = self.sticky.get(&overlay.id) else {
                        hide(&mut self.windows, &overlay.id);
                        continue;
                    };
                    if !follow.placed {
                        // The target is closed or minimized, or the conditions
                        // have never matched a window.
                        hide(&mut self.windows, &overlay.id);
                        continue;
                    }
                    let (size, position) = layout_in_rect(overlay, follow.client, follow.scale);
                    let owner = match overlay.sticky_z_order {
                        StickyZOrder::FollowWindow => follow.hwnd,
                        StickyZOrder::AboveEverything => None,
                    };
                    (size, position, owner)
                }
                DisplayMode::Global | DisplayMode::Wallpaper => {
                    let Some(monitor) =
                        crate::monitors::resolve(overlay.monitor_device.as_deref(), &connected)
                    else {
                        hide(&mut self.windows, &overlay.id);
                        continue;
                    };
                    let (size, position) = layout_for(overlay, monitor);
                    (size, position, None)
                }
            };

            // One lock for the whole overlay rather than one per target: the
            // renderer touches every target of this overlay on this frame, and a
            // per-target lock would make the cost of a group grow with the very
            // thing the lock protects.
            let stored: Vec<Option<(u64, Vec<SamplePoint>)>> = {
                let store = samples.lock().unwrap();
                let overlay_samples = store.get(&overlay.id);
                overlay
                    .targets
                    .iter()
                    .filter(|target| target.enabled)
                    .map(|target| {
                        overlay_samples
                            .and_then(|targets| targets.get(&target.id))
                            .map(|buffer| {
                                (
                                    buffer.generation,
                                    buffer.values.iter().copied().collect::<Vec<_>>(),
                                )
                            })
                    })
                    .collect()
            };

            // A window can die without this process asking: an overlay owned by
            // its sticky target is destroyed by Windows when that target
            // closes. Dropping the entry lets the creation path below rebuild
            // it, owned by whatever the follow state resolves to next.
            if self
                .windows
                .get(&overlay.id)
                .is_some_and(|window| !winwatch::is_window(window.hwnd))
            {
                self.windows.remove(&overlay.id);
            }

            if !self.windows.contains_key(&overlay.id) {
                match self.create_window(overlay, size, position, selected, owner) {
                    Ok(window) => {
                        self.windows.insert(overlay.id.clone(), window);
                    }
                    Err(error) => {
                        log::error!("failed to create overlay {}: {error}", overlay.id);
                        continue;
                    }
                }
            }

            let Some(window) = self.windows.get_mut(&overlay.id) else {
                continue;
            };
            if window.hidden {
                // Whatever took it away has come back — the pinned display, or
                // a sticky target that was minimized or closed. Show it and
                // force a redraw: a window that was hidden while a display
                // change also moved it may hold a surface sized for the old
                // one, and `changed` is computed from `size`/`position` rather
                // than from visibility, so nothing else here would ask for a
                // frame.
                window.hidden = false;
                window.dirty = true;
                unsafe {
                    ShowWindow(window.hwnd, SW_SHOWNOACTIVATE);
                }
            }
            let config_changed = window.config != *overlay;
            // Captured before `window.config` is replaced below, and only when
            // the placement actually moved: every other config edit is not a
            // reason to touch the desktop hierarchy.
            let placement_changed = window.config.display_mode != overlay.display_mode
                || (overlay.display_mode == DisplayMode::Sticky
                    && (window.config.sticky_z_order != overlay.sticky_z_order
                        || window.config.sticky_target != overlay.sticky_target));
            let border_selection_changed = window.border_selected != selected;
            window.border_selected = selected;
            // A target added, removed, disabled or reordered changes what the
            // window is holding, and nothing else in this pass would notice:
            // the samples may be identical and the config comparison may be
            // false because the window was rebuilt from it this same pass.
            let series_changed = window.sync_series(&overlay.targets);
            let mut samples_changed = series_changed;
            let mut sample_generation = window.sample_generation;
            for (series, stored) in window.series.iter_mut().zip(stored) {
                match stored {
                    Some((generation, buffer)) if series.generation != generation => {
                        series.samples = buffer;
                        series.generation = generation;
                        samples_changed = true;
                        sample_generation = sample_generation.max(generation);
                    }
                    Some((generation, _)) => {
                        sample_generation = sample_generation.max(generation);
                    }
                    // No buffer at all for a target: it has never answered, so
                    // its generation stays where it was and the line is empty.
                    None => {}
                }
            }
            let changed = window.dirty
                || window.size != size
                || window.position != position
                || config_changed
                || samples_changed
                || border_selection_changed;
            if config_changed {
                window.config = overlay.clone();
            }
            // Unconditional, and a maximum across the targets rather than a
            // value taken from one of them: it answers "has the real graph
            // started?", and a group where one host answered has started.
            window.sample_generation = sample_generation;
            if !overlay.cosmetic_startup_prefill {
                window.prefill = None;
            } else if sample_generation == 0 && (window.prefill.is_none() || config_changed) {
                window.prefill = Some(PrefillState::new(overlay, Instant::now()));
            }
            if sample_generation > 0 {
                // Restart the reveal against the first real sample, which for a
                // group is whichever target answered first.
                let first_real = window
                    .series
                    .iter()
                    .filter_map(|series| series.samples.first())
                    .map(|sample| sample.timestamp)
                    .min();
                if let Some(first_real) = first_real {
                    if let Some(prefill) = window.prefill.as_ref() {
                        if prefill.started_at > first_real {
                            window.prefill = Some(PrefillState::new(overlay, first_real));
                        }
                    }
                }
            }
            window.history_dirty |= samples_changed || config_changed;
            let surface_changed = window.surface.size != size;
            if surface_changed {
                match LayeredSurface::new(size) {
                    Ok(surface) => window.surface = surface,
                    Err(error) => {
                        log::error!("failed to resize overlay {}: {error}", overlay.id);
                        continue;
                    }
                }
            }
            let moved = window.size != size || window.position != position;
            window.size = size;
            window.position = position;
            if placement_changed || (moved && window.config.display_mode == DisplayMode::Wallpaper)
            {
                // A parked window keeps its screen coordinates, so a moved or
                // resized wallpaper overlay is re-checked here; the paint below
                // then uses the same rect.
                apply_placement(window, overlay, owner);
            }
            let now = Instant::now();
            let border_was_active = window.border.needs_animation();
            window.border.update(&window.config, selected, now);
            let border_active = window.border.needs_animation();
            let smooth = running
                && window.config.smooth_rendering
                && (window.sample_generation > 0
                    || window
                        .prefill
                        .as_ref()
                        .is_some_and(|prefill| prefill.completed_rendered));
            let smooth_due = smooth
                && window.last_rendered.elapsed()
                    >= smooth_frame_interval(window.config.smooth_fps);
            let border_due = (border_active
                && window.last_rendered.elapsed() >= border_frame_interval())
                || border_active != border_was_active;
            let prefill_due = window.prefill.as_ref().is_some_and(|prefill| {
                !prefill.completed_rendered
                    && (prefill.is_complete(now)
                        || window.last_rendered.elapsed()
                            >= smooth_frame_interval(window.config.smooth_fps))
            });
            // A cursor that is still easing needs frames of its own: index mode
            // has no clock, and the next sample is a second away.
            let cursor_due = window.config.sample_cursor
                && window.series.iter().any(|series| series.cursor.is_active())
                && window.last_rendered.elapsed() >= border_frame_interval();
            if changed || surface_changed || smooth_due || prefill_due || border_due || cursor_due {
                if Self::render_window(window, smooth) {
                    window.last_rendered = Instant::now();
                } else {
                    window.dirty = true;
                }
            }
        }

        if self.last_topmost.elapsed() >= Duration::from_secs(1) {
            for window in self.windows.values_mut() {
                // Hidden windows are skipped, and they have to be: the topmost
                // reassert passes `SWP_SHOWWINDOW`, so reasserting a hidden
                // overlay's z-order would put the graph back on screen and undo
                // the pin.
                if window.hidden {
                    continue;
                }
                match (window.config.display_mode, window.config.sticky_z_order) {
                    (DisplayMode::Wallpaper, _) => {
                        // The desktop's own windows come and go — Explorer
                        // restarts, wallpaper changes, Show Desktop — so the
                        // placement is re-checked on the same clock the topmost
                        // reassert uses. It is a check rather than a placement:
                        // re-ordering every second would be a flicker.
                        let _ = park_above_desktop(window);
                    }
                    (DisplayMode::Sticky, StickyZOrder::FollowWindow) => {
                        // Windows keeps an owned window in its owner's band;
                        // there is nothing to reassert, and reasserting topmost
                        // would undo the ownership.
                    }
                    _ => reassert_topmost(window.hwnd),
                }
            }
            self.last_topmost = Instant::now();
        }
    }

    /// Poll every sticky overlay's target, and report whether anything moved.
    ///
    /// Called on the renderer's fast wake-up (~16 ms, the same clock the
    /// message pump uses) *before* the repaint decision, so following a dragged
    /// window is smooth without a `SetWinEventHook` or a second thread. Kept
    /// apart from `apply` because `apply` is the expensive pass — a sample
    /// lock, a full config compare, a possible render — while this is a handful
    /// of window-manager reads. The return value is what tells the caller to
    /// bring its next layout pass forward; no overlay window is touched here.
    pub fn follow_sticky(&mut self, config: &Config) -> bool {
        self.sticky.retain(|id, _| {
            config.overlays.iter().any(|overlay| {
                overlay.enabled && overlay.id == *id && overlay.display_mode == DisplayMode::Sticky
            })
        });
        let mut changed = false;
        for overlay in config
            .overlays
            .iter()
            .filter(|overlay| overlay.enabled && overlay.display_mode == DisplayMode::Sticky)
        {
            let follow = self.sticky.entry(overlay.id.clone()).or_default();
            if follow.target != overlay.sticky_target {
                // The conditions are a target's identity, so an edit is a new
                // target rather than a tweak to the old one: two Chrome windows
                // are told apart by the title box, and keeping the handle a
                // title edit was meant to stop following would be wrong.
                follow.matcher = overlay
                    .sticky_target
                    .as_ref()
                    .map(|target| CompiledMatcher::compile(&target.when))
                    .unwrap_or_default();
                follow.target = overlay.sticky_target.clone();
                follow.hwnd = None;
                follow.placed = false;
                changed = true;
            }
            if follow.hwnd.is_some_and(|hwnd| !winwatch::is_window(hwnd)) {
                // The target closed. Windows also destroys an owned overlay
                // with it, which `apply` repairs; this is only the follow state
                // letting go.
                follow.hwnd = None;
                follow.placed = false;
                changed = true;
            }
            if let Some(hwnd) = follow.hwnd {
                // A minimized target has no client area on screen, and one an
                // app hid — Steam closing to the tray does this — has none
                // either. The overlay hides rather than following a window
                // nobody can see, and this is re-read on every wake-up so it
                // comes back the moment the window does.
                let placed = !winwatch::is_iconic(hwnd) && winwatch::is_visible(hwnd);
                if placed {
                    match winwatch::client_rect(hwnd) {
                        Some(client) => {
                            if client != follow.client {
                                follow.client = client;
                                changed = true;
                            }
                            // The scale follows the display the target is on,
                            // and the bounds are cached so a window that is only
                            // moving does not re-enumerate displays every
                            // frame.
                            let crossed = !follow
                                .display
                                .is_some_and(|bounds| contains(bounds, centre(client)));
                            if crossed {
                                let (scale, bounds) = scale_for_rect(client);
                                if follow.scale != scale || follow.display != bounds {
                                    follow.scale = scale;
                                    follow.display = bounds;
                                    changed = true;
                                }
                            }
                        }
                        None => {
                            // The client area went unreadable, which is a window
                            // on its way out. The sweep below finds whatever
                            // replaced it.
                            follow.hwnd = None;
                            follow.placed = false;
                            changed = true;
                        }
                    }
                }
                if follow.placed != placed {
                    follow.placed = placed;
                    changed = true;
                }
            } else if follow
                .scanned_at
                .is_none_or(|at| at.elapsed() >= Duration::from_secs(1))
            {
                // At most one sweep a second: a target that is not running yet
                // costs a desktop enumeration per second, not one per frame.
                follow.scanned_at = Some(Instant::now());
                let shapes = winwatch::shapes();
                if let Some(shape) = sticky::resolve(&shapes, &follow.matcher) {
                    let (scale, bounds) = scale_for_rect(shape.client);
                    follow.hwnd = Some(shape.hwnd);
                    follow.client = shape.client;
                    follow.scale = scale;
                    follow.display = bounds;
                    follow.placed = true;
                    changed = true;
                }
            }
        }
        changed
    }

    /// How fast the renderer must redraw for the startup prefill.
    ///
    /// A hidden window's prefill is frozen where it stood, so it is left out
    /// here for the same reason it is skipped above: including it would ask for
    /// display-rate redraws of a graph nobody can see.
    pub fn prefill_repaint_interval(&self) -> Option<Duration> {
        self.windows
            .values()
            .filter(|window| !window.hidden)
            .filter_map(|window| {
                let prefill = window.prefill.as_ref()?;
                if prefill.completed_rendered {
                    return None;
                }
                Some(smooth_frame_interval(window.config.smooth_fps))
            })
            .min()
    }

    pub fn border_repaint_interval(&self) -> Option<Duration> {
        self.windows
            .values()
            .filter(|window| !window.hidden)
            .any(|window| window.border.needs_animation())
            .then(border_frame_interval)
    }

    /// How fast the renderer must redraw while a sample cursor is easing.
    ///
    /// Index mode has no frame clock of its own, so without this the cursor
    /// would only move when a sample arrives — a jump, which is the easing's
    /// whole point to avoid.
    pub fn cursor_repaint_interval(&self) -> Option<Duration> {
        self.windows
            .values()
            .filter(|window| !window.hidden && window.config.sample_cursor)
            .any(|window| window.series.iter().any(|series| series.cursor.is_active()))
            .then(border_frame_interval)
    }

    fn create_window(
        &self,
        config: &OverlayConfig,
        size: (i32, i32),
        position: (i32, i32),
        selected: bool,
        owner: Option<HWND>,
    ) -> Result<OverlayWindow, Box<dyn Error + Send + Sync>> {
        let title = wide(&format!("PingLatencyOverlay::{}", config.id))?;
        // Wallpaper mode drops the topmost style at creation: it is about to be
        // parked above the desktop rather than layered over everything. A
        // sticky overlay owned by its target drops it too: the owner's band
        // decides where it sits.
        let mut ex_style = WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE;
        if config.display_mode != DisplayMode::Wallpaper && owner.is_none() {
            ex_style |= WS_EX_TOPMOST;
        }
        let hwnd = unsafe {
            CreateWindowExW(
                ex_style,
                self.class_name.as_ptr(),
                title.as_ptr(),
                WS_POPUP,
                position.0,
                position.1,
                size.0,
                size.1,
                ptr::null_mut(),
                ptr::null_mut(),
                self.instance,
                ptr::null_mut(),
            )
        };
        if hwnd.is_null() {
            return Err("CreateWindowExW failed".into());
        }
        let surface = match LayeredSurface::new(size) {
            Ok(surface) => surface,
            Err(error) => {
                unsafe {
                    DestroyWindow(hwnd);
                }
                return Err(error);
            }
        };

        let mut window = OverlayWindow {
            hwnd,
            config: config.clone(),
            series: Vec::new(),
            prefill: config
                .cosmetic_startup_prefill
                .then(|| PrefillState::new(config, Instant::now())),
            prefill_points: Vec::new(),
            history_dirty: true,
            border: BorderAnimator::new(),
            border_selected: selected,
            // A window is only ever created for an overlay whose monitor
            // resolved this pass, so it starts visible.
            hidden: false,
            pixels: Vec::new(),
            sample_generation: 0,
            size,
            position,
            dirty: true,
            last_rendered: Instant::now(),
            surface,
        };
        // Initialize the border before the first surface is rendered. This
        // makes the configured startup effect visible on the very first frame,
        // rather than only after the next overlay reconciliation pass.
        window.border.update(config, selected, Instant::now());
        // The series list has to exist before the first render, because that
        // render is what puts something on screen. `apply` would otherwise only
        // build it on its next pass, so the window would show one empty frame.
        window.sync_series(&config.targets);
        window.rebuild_history();
        // A wallpaper-mode window is parked directly above the shell's desktop
        // window; there is no host to look for before the first paint, because
        // the window stays an ordinary top-level popup that the once-a-second
        // check moves as soon as the shell is there. A sticky window in its
        // target's band is owned before it is shown instead.
        //
        // Give the layered window its first surface before making it visible.
        // Otherwise Windows can briefly retain the class background (white)
        // behind a fully transparent first frame.
        Self::render_window(&mut window, false);

        match owner {
            Some(owner) => unsafe {
                // Owned rather than topmost: the overlay shares the target's
                // z-order band, so switching to another app takes it off the
                // screen with the window it belongs to.
                SetWindowLongPtrW(hwnd, GWLP_HWNDPARENT, owner as isize);
                SetWindowPos(
                    hwnd,
                    owner,
                    position.0,
                    position.1,
                    size.0,
                    size.1,
                    SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
            },
            None if config.display_mode == DisplayMode::Wallpaper => {
                let _ = park_above_desktop(&window);
            }
            None => unsafe {
                SetWindowPos(
                    hwnd,
                    HWND_TOPMOST,
                    position.0,
                    position.1,
                    size.0,
                    size.1,
                    SWP_NOACTIVATE | SWP_SHOWWINDOW,
                );
            },
        }
        unsafe {
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            UpdateWindow(hwnd);
        }
        Ok(window)
    }

    fn render_window(window: &mut OverlayWindow, smooth: bool) -> bool {
        let now = Instant::now();
        let border = window.border.visual(now);
        let prefill_was_completed = window
            .prefill
            .as_ref()
            .is_some_and(|prefill| prefill.completed_rendered);
        let prefill_complete = window
            .prefill
            .as_ref()
            .is_some_and(|prefill| prefill.is_complete(now));
        if prefill_complete && (!prefill_was_completed || window.history_dirty) {
            window.rebuild_history();
        }
        let has_prefill = window.prefill.is_some();
        let prefill_progress = window
            .prefill
            .as_ref()
            .map(|prefill| prefill.progress(now))
            .unwrap_or(0.0);
        let render_smooth = smooth || has_prefill || border.is_some();
        let width = window.size.0.max(1) as u32;
        let height = window.size.1.max(1) as u32;
        // One borrow of the target list, taken once, because the colour of each
        // line is the target's and the renderer wants them all at once. Zipped
        // with the window's series, which `sync_series` keeps in the same order.
        let targets: Vec<&TargetConfig> = window
            .config
            .targets
            .iter()
            .filter(|target| target.enabled)
            .collect();
        let rendered = if has_prefill {
            if prefill_complete {
                let mut series: Vec<Series<'_>> = window
                    .series
                    .iter_mut()
                    .zip(targets.iter())
                    .map(|(entry, target)| Series {
                        line_color: &target.line_color,
                        timeout_color: &target.timeout_color,
                        samples: &entry.history,
                        max_sample_gap: sample_gap_threshold(target.timeout_ms),
                        cursor: Some(&mut entry.cursor),
                    })
                    .collect();
                render_series_into_with_border(
                    width,
                    height,
                    &window.config,
                    &mut series,
                    now,
                    render_smooth,
                    border.as_ref(),
                    &mut window.pixels,
                )
            } else {
                // The reveal draws the cosmetic curves only, and all of them
                // into one buffer: rendering them in a loop would clear the
                // pixels each pass and leave the last target's curve alone.
                // Every curve is truncated to the same progress so they reveal
                // left to right together.
                window.prefill_points.clear();
                for index in 0..targets.len() {
                    let Some(samples) = window.prefill_samples(index) else {
                        continue;
                    };
                    let count = (prefill_progress * samples.len() as f32).ceil() as usize;
                    window
                        .prefill_points
                        .push(samples.iter().take(count).copied().collect::<Vec<_>>());
                }
                let mut series: Vec<Series<'_>> = targets
                    .iter()
                    .zip(window.prefill_points.iter())
                    .map(|(target, samples)| Series {
                        line_color: &target.line_color,
                        timeout_color: &target.timeout_color,
                        samples,
                        max_sample_gap: sample_gap_threshold(target.timeout_ms),
                        cursor: None,
                    })
                    .collect();
                render_series_into_with_border(
                    width,
                    height,
                    &window.config,
                    &mut series,
                    now,
                    render_smooth,
                    border.as_ref(),
                    &mut window.pixels,
                )
            }
        } else {
            let mut series: Vec<Series<'_>> = window
                .series
                .iter_mut()
                .zip(targets.iter())
                .map(|(entry, target)| Series {
                    line_color: &target.line_color,
                    timeout_color: &target.timeout_color,
                    samples: &entry.samples,
                    max_sample_gap: sample_gap_threshold(target.timeout_ms),
                    cursor: Some(&mut entry.cursor),
                })
                .collect();
            render_series_into_with_border(
                width,
                height,
                &window.config,
                &mut series,
                now,
                smooth,
                border.as_ref(),
                &mut window.pixels,
            )
        };
        if !rendered {
            return false;
        }
        if window
            .surface
            .update(window.hwnd, window.position, &window.pixels)
        {
            window.dirty = false;
            if prefill_complete {
                if let Some(prefill) = window.prefill.as_mut() {
                    prefill.completed_rendered = true;
                }
            }
            true
        } else {
            false
        }
    }
}

impl Drop for OverlayManager {
    fn drop(&mut self) {
        for (_, window) in self.windows.drain() {
            unsafe {
                DestroyWindow(window.hwnd);
            }
        }
        unsafe {
            UnregisterClassW(self.class_name.as_ptr(), self.instance);
        }
    }
}

#[cfg(windows)]
unsafe extern "system" fn overlay_wnd_proc(
    hwnd: HWND,
    msg: UINT,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_NCHITTEST => HTTRANSPARENT,
        WM_MOUSEACTIVATE => MA_NOACTIVATE,
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            ValidateRect(hwnd, ptr::null());
            0
        }
        // Show Desktop asks every window to minimize, and a wallpaper-mode
        // overlay has to refuse: it would disappear from the desktop it belongs
        // to, and the user has no window to bring back. The absent topmost bit
        // is the mode's flag — an ordinary overlay always carries it, a parked
        // one never does — so a window procedure with no route to the manager
        // can still tell them apart.
        WM_SYSCOMMAND if (wparam & 0xFFF0) == SC_MINIMIZE => {
            let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
            if ex_style & WS_EX_TOPMOST == 0 {
                0
            } else {
                DefWindowProcW(hwnd, msg, wparam, lparam)
            }
        }
        // Windows is asking this process to close. Record it and return 0
        // WITHOUT calling DefWindowProcW, and that is the whole point.
        //
        // `DefWindowProc` on WM_CLOSE destroys the window, and the renderer's
        // loop would then see a missing handle on its next pass and rebuild it.
        // "End task" in Task Manager would close the windows, wait, find the
        // process alive with brand new windows, and conclude that its graceful
        // close had worked — so it would never escalate to a kill and the
        // process would simply never end. Letting the default handler run is
        // therefore worse than ignoring the message: it also defeats the very
        // escalation the user reached for.
        //
        // A quit the user asked for somewhere else — the tray's Exit, a logoff —
        // arrives the same way and gets the same answer, from one place.
        WM_CLOSE | WM_QUERYENDSESSION => {
            QUIT_REQUESTED.store(true, Ordering::Relaxed);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

#[cfg(windows)]
fn non_null(hwnd: HWND) -> Option<HWND> {
    (!hwnd.is_null()).then_some(hwnd)
}

/// The window a wallpaper-mode overlay is parked directly above.
///
/// The shell's desktop window is the one to sit on: it contains the wallpaper
/// and the desktop icons on both desktop shapes, so one host is enough — and,
/// crucially, a wallpaper window must stay an ordinary top-level one. A
/// layered child of the desktop presents nothing but a flash on the raised
/// desktop (measured on a 26100 build: the compositor does not hold its
/// content), and the only recommended alternative there is a GPU present
/// path, which this renderer deliberately does not have. Parking above this
/// window therefore leaves the overlay above the wallpaper and below every
/// ordinary window and the taskbar, with no GPU context.
#[cfg(windows)]
fn desktop_host() -> Option<HWND> {
    unsafe { non_null(GetShellWindow()) }
}

/// Keep a wallpaper-mode overlay directly above the desktop window.
///
/// Checked rather than unconditional: the re-assert runs every second, and
/// re-placing a window that is already in the right spot is churn the user
/// can see as a flicker. Returns whether the window ends up there.
#[cfg(windows)]
fn park_above_desktop(window: &OverlayWindow) -> bool {
    let Some(host) = desktop_host() else {
        return false;
    };
    unsafe {
        if GetWindow(window.hwnd, GW_HWNDNEXT) == host {
            return true;
        }
        SetWindowPos(
            window.hwnd,
            host,
            0,
            0,
            0,
            0,
            SWP_NOSIZE | SWP_NOMOVE | SWP_NOACTIVATE,
        );
        GetWindow(window.hwnd, GW_HWNDNEXT) == host
    }
}

#[cfg(windows)]
fn reassert_topmost(hwnd: HWND) {
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOSIZE | SWP_NOMOVE | SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
    }
}

/// Hide an overlay's window because there is nowhere to put it.
///
/// The two causes — a pinned display that is not attached, and a sticky target
/// that is closed or minimized — behave the same: the window is hidden rather
/// than destroyed, so the graph comes back at exactly the size and position the
/// user chose, and it is only hidden if it already exists, because a missing
/// target is not a reason to create a window nobody can see.
///
/// A hidden window is also excluded from the repaint intervals and from the
/// z-order reassert, for the same reason [`OverlayWindow::hidden`] exists:
/// leaving a frozen prefill in those lists would have the renderer spin at
/// display rate for a graph that is not on screen, and `SetWindowPos` carries
/// `SWP_SHOWWINDOW`, so reasserting would quietly unhide it.
#[cfg(windows)]
fn hide(windows: &mut HashMap<String, OverlayWindow>, id: &str) {
    if let Some(window) = windows.get_mut(id) {
        if !window.hidden {
            window.hidden = true;
            unsafe {
                ShowWindow(window.hwnd, SW_HIDE);
            }
        }
    }
}

/// Put a window where its display mode and z-order choice say it belongs.
///
/// Called only when that decision changed, so the Win32 calls here are not a
/// per-frame cost. The wallpaper arm is checked rather than unconditional — see
/// [`park_above_desktop`] — and ownership is idempotent but still only
/// re-applied when the mode, the choice or the target moved.
#[cfg(windows)]
fn apply_placement(window: &OverlayWindow, overlay: &OverlayConfig, owner: Option<HWND>) {
    match (overlay.display_mode, overlay.sticky_z_order, owner) {
        (DisplayMode::Wallpaper, _, _) => {
            unsafe {
                SetWindowPos(
                    window.hwnd,
                    HWND_NOTOPMOST,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOMOVE | SWP_NOACTIVATE,
                );
            }
            let _ = park_above_desktop(window);
        }
        (DisplayMode::Sticky, StickyZOrder::FollowWindow, Some(owner)) => unsafe {
            // Out of the topmost band first: a topmost window stays over
            // everything however it is owned, so the style has to go before the
            // band can take it.
            SetWindowPos(
                window.hwnd,
                HWND_NOTOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOSIZE | SWP_NOMOVE | SWP_NOACTIVATE,
            );
            SetWindowLongPtrW(window.hwnd, GWLP_HWNDPARENT, owner as isize);
            // Directly above the window it follows, in that window's band.
            SetWindowPos(
                window.hwnd,
                owner,
                0,
                0,
                0,
                0,
                SWP_NOSIZE | SWP_NOMOVE | SWP_NOACTIVATE,
            );
        },
        _ => unsafe {
            // Every other combination floats: clear any ownership a previous
            // target left behind, then take the topmost band again.
            SetWindowLongPtrW(window.hwnd, GWLP_HWNDPARENT, 0);
            reassert_topmost(window.hwnd);
        },
    }
}

/// The centre of a rectangle, which is the point a display is picked by.
fn centre(rect: Rect) -> (i32, i32) {
    (rect.left + rect.width() / 2, rect.top + rect.height() / 2)
}

/// Whether a point is inside a rectangle. Half-open, so a point on the far
/// edge belongs to the next display rather than to two.
fn contains(rect: Rect, point: (i32, i32)) -> bool {
    point.0 >= rect.left && point.0 < rect.right && point.1 >= rect.top && point.1 < rect.bottom
}

/// The DPI scale of the display a rectangle's centre sits on, and that
/// display's bounds.
///
/// Read from the display rather than from the system, because a sticky target
/// dragged between a 100% and a 150% panel must resize the graph as it crosses;
/// `GetDpiForSystem` answers for the primary whatever the caller does. Falls
/// back to the first display when the point is in the gap between two monitors.
fn scale_for_rect(rect: Rect) -> (f32, Option<Rect>) {
    let monitors = crate::monitors::enumerate();
    let point = centre(rect);
    let monitor = monitors
        .iter()
        .find(|monitor| contains(monitor.bounds, point))
        .or_else(|| monitors.first());
    match monitor {
        Some(monitor) => (monitor.scale(), Some(monitor.bounds)),
        None => (1.0, None),
    }
}

#[cfg(windows)]
fn position_for_anchor(
    anchor: Anchor,
    work: RECT,
    width: i64,
    height: i64,
    horizontal_margin: i64,
    vertical_margin: i64,
) -> (i64, i64) {
    let left = work.left as i64;
    let top = work.top as i64;
    let right = work.right as i64;
    let bottom = work.bottom as i64;
    let center_x = left + (right - left - width) / 2;
    let center_y = top + (bottom - top - height) / 2;
    match anchor {
        Anchor::TopLeft => (left + horizontal_margin, top + vertical_margin),
        Anchor::TopCenter => (center_x + horizontal_margin, top + vertical_margin),
        Anchor::TopRight => (right - width - horizontal_margin, top + vertical_margin),
        Anchor::CenterLeft => (left + horizontal_margin, center_y + vertical_margin),
        Anchor::Center => (center_x + horizontal_margin, center_y + vertical_margin),
        Anchor::CenterRight => (
            right - width - horizontal_margin,
            center_y + vertical_margin,
        ),
        Anchor::BottomLeft => (left + horizontal_margin, bottom - height - vertical_margin),
        Anchor::BottomCenter => (
            center_x + horizontal_margin,
            bottom - height - vertical_margin,
        ),
        Anchor::BottomRight => (
            right - width - horizontal_margin,
            bottom - height - vertical_margin,
        ),
    }
}

/// Size and place an overlay against one display.
///
/// Takes the display rather than a DPI number and a work area, because those
/// two are one decision: reading the scale from anywhere but the monitor the
/// graph is going on is how a 150% secondary panel ends up with a graph sized
/// for the primary. The caller has already resolved the display, so an
/// unreachable monitor never reaches here.
#[cfg(windows)]
fn layout_for(config: &OverlayConfig, monitor: &MonitorInfo) -> ((i32, i32), (i32, i32)) {
    layout_in_rect(config, monitor.work, monitor.scale())
}

/// Size and place an overlay inside one rectangle on one display.
///
/// The rectangle is a pinned display's work area for Global and Wallpaper
/// modes, and a target window's client area for Sticky mode — which is what
/// makes "stick to the window as if it were the screen" literal: every anchor
/// and margin applies inside it, and the scale comes from the display the
/// rectangle is actually on.
#[cfg(windows)]
fn layout_in_rect(config: &OverlayConfig, work: Rect, dpi_scale: f32) -> ((i32, i32), (i32, i32)) {
    let long_logical =
        (config.window_seconds.max(1) as f64 * config.scale.max(1) as f64).clamp(1.0, 8192.0);
    let short_logical = (config.graph_height_px.max(10) as f64).clamp(1.0, 8192.0);
    // Underglow is cast past the zero line, so the window grows on that side
    // instead of the axis shrinking: the short dimension is the axis plus the
    // cast's reserve, and `render_graph_into_internal` insets the axis by the
    // same amount. The reserve is not scaled by DPI because the glow radius it
    // mirrors is a physical pixel size.
    let reserve = line_glow_reserve_px(config) as i32;
    // The cursor owns a band past the newest sample on the long side and half
    // a triangle's height at each end of the axis. The window grows for both
    // and the renderer insets by the same amounts, so the axis keeps its
    // configured size and a cursor on the zero line or the ceiling stays
    // whole. Physical pixels, like the glow radius.
    let cursor_reserve = sample_cursor_reserve_px(config) as i32;
    let cursor_room = sample_cursor_room_px(config) as i32 * 2;
    let (long_px, short_px) = if matches!(config.orientation, 90 | 270) {
        (
            (short_logical * dpi_scale as f64).clamp(1.0, MAX_RENDER_DIMENSION) as i32
                + reserve
                + cursor_room,
            (long_logical * dpi_scale as f64).clamp(1.0, MAX_RENDER_DIMENSION) as i32
                + cursor_reserve,
        )
    } else {
        (
            (long_logical * dpi_scale as f64).clamp(1.0, MAX_RENDER_DIMENSION) as i32
                + cursor_reserve,
            (short_logical * dpi_scale as f64).clamp(1.0, MAX_RENDER_DIMENSION) as i32
                + reserve
                + cursor_room,
        )
    };
    let size = (long_px.max(1), short_px.max(1));
    let horizontal_margin = (config.horizontal_margin_px as f64 * dpi_scale as f64).round() as i64;
    let vertical_margin = (config.vertical_margin_px as f64 * dpi_scale as f64).round() as i64;
    // The portable `Rect` crosses into the Win32 shape here rather than
    // carrying it around: the overlay maths has always spoken `RECT` and
    // `position_for_anchor` is left exactly as it was.
    let work = RECT {
        left: work.left,
        top: work.top,
        right: work.right,
        bottom: work.bottom,
    };
    let width = size.0 as i64;
    let height = size.1 as i64;
    let (x, y) = position_for_anchor(
        config.position,
        work,
        width,
        height,
        horizontal_margin,
        vertical_margin,
    );
    (
        size,
        (
            x.clamp(i32::MIN as i64, i32::MAX as i64) as i32,
            y.clamp(i32::MIN as i64, i32::MAX as i64) as i32,
        ),
    )
}

#[cfg(windows)]
fn wide(value: &str) -> Result<Vec<u16>, Box<dyn Error + Send + Sync>> {
    Ok(value.encode_utf16().chain(std::iter::once(0)).collect())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    /// The two cosmetic curves a group draws at startup must not be identical.
    ///
    /// The seed is the target's id, so this is what says the seed is actually
    /// read. Two identical curves look like one host with a fat line, which is
    /// the exact thing grouping exists to disambiguate.
    #[test]
    fn two_targets_get_different_startup_curves() {
        let config = OverlayConfig::new();
        let first = cosmetic_prefill_values(&config, "target-one");
        let second = cosmetic_prefill_values(&config, "target-two");
        assert_ne!(
            first, second,
            "two hosts of one group have the same fake latency history"
        );

        // Deterministic, so a restart does not reshuffle the reveal.
        assert_eq!(first, cosmetic_prefill_values(&config, "target-one"));
    }

    /// A group's reveal holds one curve per **enabled** target, and the count
    /// has to match the series the window draws, because `rebuild_history` and
    /// the reveal index the two in step.
    #[test]
    fn the_startup_curves_match_the_enabled_targets() {
        let mut config = OverlayConfig::new();
        config.add_target();
        assert_eq!(config.targets.len(), 2);

        let before = PrefillState::new(&config, Instant::now());
        assert_eq!(
            before.samples.len(),
            2,
            "the reveal drew {} curves for 2 targets",
            before.samples.len()
        );

        config.targets[1].enabled = false;
        let after = PrefillState::new(&config, Instant::now());
        assert_eq!(
            after.samples.len(),
            1,
            "a disabled host still got a startup curve"
        );
    }

    /// A target's series survives a change that is not about it.
    ///
    /// `sync_series` matches on the target id, so reordering, disabling a
    /// sibling and editing a colour all have to leave the other targets' samples
    /// and generations alone. Matching on position instead would silently reset
    /// a line's history whenever a host above it in the list was removed.
    #[test]
    fn a_series_keeps_its_samples_across_a_reorder() {
        let mut config = OverlayConfig::new();
        config.add_target();
        let mut series = Vec::new();
        assert!(sync_series(&mut series, &config.targets));
        assert_eq!(series.len(), 2);
        series[0].generation = 7;
        series[0].samples.push(SamplePoint {
            value: Some(42),
            timestamp: Instant::now(),
            is_prefill: false,
        });
        // The target that has history, by id rather than by position: after the
        // swap it is second, so anything checked by index would be looking at
        // the other host's line.
        let with_history = config.targets[0].id.clone();

        config.targets.swap(0, 1);
        assert!(sync_series(&mut series, &config.targets));

        assert_eq!(
            series
                .iter()
                .position(|entry| entry.target_id == with_history),
            Some(1),
            "the series did not follow the target through the reorder"
        );
        let moved = series.last().expect("two series");
        assert_eq!(
            moved.generation, 7,
            "the target that moved lost its generation"
        );
        assert_eq!(
            moved.samples.len(),
            1,
            "the target that moved lost its samples"
        );
    }

    /// Adding a host grows the series list and asks for a frame.
    ///
    /// The "asks for a frame" half is the part that is easy to lose: a target
    /// added while the samples and the config are both unchanged is invisible to
    /// every other trigger in `apply`.
    #[test]
    fn adding_a_host_grows_the_series_list() {
        let mut config = OverlayConfig::new();
        let mut series = Vec::new();
        assert!(sync_series(&mut series, &config.targets));
        assert_eq!(series.len(), 1);

        config.add_target();
        assert!(
            sync_series(&mut series, &config.targets),
            "adding a host did not report a change, so no frame would be drawn"
        );
        assert_eq!(series.len(), 2);

        // Idempotent: a pass with nothing changed reports nothing, or the
        // renderer would redraw every frame for the rest of the session.
        assert!(
            !sync_series(&mut series, &config.targets),
            "an unchanged pass reported a change and redraws forever"
        );
    }

    /// Disabling one host removes its line and leaves the rest in place.
    #[test]
    fn disabling_a_host_drops_only_its_line() {
        let mut config = OverlayConfig::new();
        config.add_target();
        let mut series = Vec::new();
        sync_series(&mut series, &config.targets);
        series[0].generation = 3;
        series[1].generation = 9;
        let first_generation = series[0].generation;

        config.targets[1].enabled = false;
        assert!(sync_series(&mut series, &config.targets));

        assert_eq!(series.len(), 1);
        assert_eq!(
            series[0].generation, first_generation,
            "the remaining host's samples were reset"
        );
    }

    fn work_area() -> RECT {
        RECT {
            left: 0,
            top: 0,
            right: 1000,
            bottom: 800,
        }
    }

    #[test]
    fn signed_margins_follow_anchor_reference() {
        assert_eq!(
            position_for_anchor(Anchor::TopCenter, work_area(), 100, 50, 10, 20),
            (460, 20)
        );
        assert_eq!(
            position_for_anchor(Anchor::CenterLeft, work_area(), 100, 50, 0, 0),
            (0, 375)
        );
        assert_eq!(
            position_for_anchor(Anchor::TopRight, work_area(), 100, 50, -10, 5),
            (910, 5)
        );
        assert_eq!(
            position_for_anchor(Anchor::Center, work_area(), 100, 50, -10, 20),
            (440, 395)
        );
        assert_eq!(
            position_for_anchor(Anchor::BottomCenter, work_area(), 100, 50, -10, 20),
            (440, 730)
        );
    }

    /// The underglow's reserve grows the window on the short side and leaves
    /// the axis the height `graphHeightPx` names; a rotated overlay carries it
    /// on its width. Zero without the glow, so existing sizes do not move.
    #[test]
    fn the_underglow_reserves_room_on_the_short_side() {
        let work = Rect {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        let mut overlay = OverlayConfig::new();
        overlay.line_glow = false;
        overlay.sample_cursor = false;
        let (size, _) = layout_in_rect(&overlay, work, 1.0);
        assert_eq!(size, (120, 60));

        overlay.line_glow = true;
        overlay.line_glow_radius_px = 4;
        let (size, _) = layout_in_rect(&overlay, work, 1.0);
        assert_eq!(size, (120, 65));
        assert_eq!(
            line_glow_reserve_px(&overlay),
            5,
            "the reserve must match the window's growth"
        );

        overlay.orientation = 90;
        let (rotated, _) = layout_in_rect(&overlay, work, 1.0);
        assert_eq!(rotated, (65, 120));

        // The reserve is a physical size and is not scaled with the display.
        let (scaled, _) = layout_in_rect(&overlay, work, 2.0);
        assert_eq!(scaled, (125, 240));
    }

    /// The cursor reserves a constant 2px gutter on the leading side of the
    /// long axis — the base sits flush at the edge, and the apex grows back
    /// along the line as the cursor grows — plus room at both ends of the axis
    /// so the triangle is never sliced and the axis keeps the size
    /// `windowSeconds`/`graphHeightPx` name. Both reserves are physical sizes,
    /// not scaled with the display.
    #[test]
    fn the_sample_cursor_reserves_room_on_both_axes() {
        let work = Rect {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        };
        let mut overlay = OverlayConfig::new();
        overlay.line_glow = false;
        overlay.sample_cursor = false;
        let (size, _) = layout_in_rect(&overlay, work, 1.0);
        assert_eq!(size, (120, 60));

        overlay.sample_cursor = true;
        assert_eq!(
            sample_cursor_reserve_px(&overlay),
            2,
            "a constant gutter: the edge margin plus half the rim"
        );
        assert_eq!(
            sample_cursor_room_px(&overlay),
            9,
            "half the triangle (0.7 * 10) plus half the rim and a pixel"
        );
        let (size, _) = layout_in_rect(&overlay, work, 1.0);
        assert_eq!(size, (120 + 2, 60 + 2 * 9));

        overlay.orientation = 90;
        let (rotated, _) = layout_in_rect(&overlay, work, 1.0);
        assert_eq!(rotated, (60 + 2 * 9, 120 + 2));

        overlay.orientation = 0;
        overlay.sample_cursor_size_px = 20;
        let (large, _) = layout_in_rect(&overlay, work, 1.0);
        assert_eq!(large, (120 + 2, 60 + 2 * 16));

        let (scaled, _) = layout_in_rect(&overlay, work, 2.0);
        assert_eq!(scaled, (2 * 120 + 2, 2 * 60 + 2 * 16));
    }
}
