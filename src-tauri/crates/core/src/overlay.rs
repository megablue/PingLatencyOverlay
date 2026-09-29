use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::border::{border_frame_interval, BorderAnimator};
use crate::config::{smooth_frame_interval, Anchor, Config, OverlayConfig, TargetConfig};
#[cfg(windows)]
use crate::monitors::MonitorInfo;
use crate::probes::SampleStore;
use crate::render::{
    cosmetic_prefill_samples, render_series_into_with_border, SamplePoint, Series,
};

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
    DestroyWindow, DispatchMessageW, GetDC, GetModuleHandleW, GetStockObject, PeekMessageW,
    RegisterClassW, ReleaseDC, SelectObject, SetProcessDpiAwarenessContext, SetWindowPos,
    ShowWindow, TranslateMessage, UnregisterClassW, UpdateLayeredWindow, UpdateWindow,
    ValidateRect, BITMAPINFO, BLENDFUNCTION, HDC, HGDIOBJ, HINSTANCE, HWND, LPARAM, LRESULT, MSG,
    POINT, RECT, SIZE, UINT, WNDCLASSW, WPARAM,
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
}

impl WindowSeries {
    fn new(target_id: &str) -> Self {
        Self {
            target_id: target_id.to_string(),
            samples: Vec::new(),
            generation: 0,
            history: Vec::new(),
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
    /// Set when the display this overlay is pinned to is not attached, so the
    /// window is off screen but kept.
    ///
    /// Kept rather than destroyed so the graph comes back at exactly the size
    /// and position the user chose when the monitor returns. The field exists
    /// because `ShowWindow(hwnd, SW_HIDE)` is not the only way a window ends up
    /// invisible, and the two repaint intervals and the topmost reassert have to
    /// all skip it rather than each guess.
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

pub struct OverlayManager {
    instance: HINSTANCE,
    class_name: Vec<u16>,
    windows: HashMap<String, OverlayWindow>,
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
            let Some(monitor) =
                crate::monitors::resolve(overlay.monitor_device.as_deref(), &connected)
            else {
                // Pinned to a display that is not attached right now.
                //
                // The window is hidden rather than destroyed, and it is only
                // hidden if it already exists: a monitor that is missing is not
                // a reason to create a window nobody can see, and creating it
                // would cost a layered surface and a DIB for the whole time the
                // panel is unplugged.
                //
                // A hidden window is also excluded from the repaint intervals
                // below and from the topmost reassert. Leaving a frozen prefill
                // in those lists would have the renderer spin at display rate
                // for a graph that is not on screen, and `SetWindowPos` carries
                // `SWP_SHOWWINDOW`, so reasserting would quietly unhide it.
                if let Some(window) = self.windows.get_mut(&overlay.id) {
                    if !window.hidden {
                        window.hidden = true;
                        unsafe {
                            ShowWindow(window.hwnd, SW_HIDE);
                        }
                    }
                }
                continue;
            };
            let (size, position) = layout_for(overlay, monitor);

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

            if !self.windows.contains_key(&overlay.id) {
                match self.create_window(overlay, size, position, selected) {
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
                // The monitor came back. Show it and force a redraw: a window
                // that was hidden while a display change also moved it may hold
                // a surface sized for the old one, and `changed` is computed
                // from `size`/`position` rather than from visibility, so
                // nothing else here would ask for a frame.
                window.hidden = false;
                window.dirty = true;
                unsafe {
                    ShowWindow(window.hwnd, SW_SHOWNOACTIVATE);
                }
            }
            let config_changed = window.config != *overlay;
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
            window.size = size;
            window.position = position;
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
            if changed || surface_changed || smooth_due || prefill_due || border_due {
                if Self::render_window(window, smooth) {
                    window.last_rendered = Instant::now();
                } else {
                    window.dirty = true;
                }
            }
        }

        if self.last_topmost.elapsed() >= Duration::from_secs(1) {
            for window in self.windows.values() {
                // Hidden windows are skipped, and they have to be: this passes
                // `SWP_SHOWWINDOW`, so reasserting a hidden overlay's z-order
                // would put the graph back on screen and undo the pin.
                if !window.hidden {
                    reassert_topmost(window.hwnd);
                }
            }
            self.last_topmost = Instant::now();
        }
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

    fn create_window(
        &self,
        config: &OverlayConfig,
        size: (i32, i32),
        position: (i32, i32),
        selected: bool,
    ) -> Result<OverlayWindow, Box<dyn Error + Send + Sync>> {
        let title = wide(&format!("PingLatencyOverlay::{}", config.id))?;
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST
                    | WS_EX_LAYERED
                    | WS_EX_TRANSPARENT
                    | WS_EX_TOOLWINDOW
                    | WS_EX_NOACTIVATE,
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
        // Give the layered window its first surface before making it visible.
        // Otherwise Windows can briefly retain the class background (white)
        // behind a fully transparent first frame.
        Self::render_window(&mut window, false);

        unsafe {
            SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                position.0,
                position.1,
                size.0,
                size.1,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
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
                let series: Vec<Series<'_>> = window
                    .series
                    .iter()
                    .zip(targets.iter())
                    .map(|(entry, target)| Series {
                        line_color: &target.line_color,
                        timeout_color: &target.timeout_color,
                        samples: &entry.history,
                    })
                    .collect();
                render_series_into_with_border(
                    width,
                    height,
                    &window.config,
                    &series,
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
                let series: Vec<Series<'_>> = targets
                    .iter()
                    .zip(window.prefill_points.iter())
                    .map(|(target, samples)| Series {
                        line_color: &target.line_color,
                        timeout_color: &target.timeout_color,
                        samples,
                    })
                    .collect();
                render_series_into_with_border(
                    width,
                    height,
                    &window.config,
                    &series,
                    now,
                    render_smooth,
                    border.as_ref(),
                    &mut window.pixels,
                )
            }
        } else {
            let series: Vec<Series<'_>> = window
                .series
                .iter()
                .zip(targets.iter())
                .map(|(entry, target)| Series {
                    line_color: &target.line_color,
                    timeout_color: &target.timeout_color,
                    samples: &entry.samples,
                })
                .collect();
            render_series_into_with_border(
                width,
                height,
                &window.config,
                &series,
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
    let dpi_scale = monitor.scale();
    let long_logical =
        (config.window_seconds.max(1) as f64 * config.scale.max(1) as f64).clamp(1.0, 8192.0);
    let short_logical = (config.graph_height_px.max(10) as f64).clamp(1.0, 8192.0);
    let (long_px, short_px) = if matches!(config.orientation, 90 | 270) {
        (
            (short_logical * dpi_scale as f64).clamp(1.0, MAX_RENDER_DIMENSION) as i32,
            (long_logical * dpi_scale as f64).clamp(1.0, MAX_RENDER_DIMENSION) as i32,
        )
    } else {
        (
            (long_logical * dpi_scale as f64).clamp(1.0, MAX_RENDER_DIMENSION) as i32,
            (short_logical * dpi_scale as f64).clamp(1.0, MAX_RENDER_DIMENSION) as i32,
        )
    };
    let size = (long_px.max(1), short_px.max(1));
    let horizontal_margin = (config.horizontal_margin_px as f64 * dpi_scale as f64).round() as i64;
    let vertical_margin = (config.vertical_margin_px as f64 * dpi_scale as f64).round() as i64;
    // The portable `Rect` crosses into the Win32 shape here rather than
    // carrying it around: the overlay maths has always spoken `RECT` and
    // `position_for_anchor` is left exactly as it was.
    let work = RECT {
        left: monitor.work.left,
        top: monitor.work.top,
        right: monitor.work.right,
        bottom: monitor.work.bottom,
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
}
