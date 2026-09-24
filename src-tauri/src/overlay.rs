use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::ptr;
use std::time::{Duration, Instant};

use crate::config::{Anchor, Config, OverlayConfig};
use crate::probes::SampleStore;
use crate::render::render_graph;

#[cfg(windows)]
#[allow(non_snake_case, clippy::upper_case_acronyms)]
mod win {
    use core::ffi::c_void;

    pub type BOOL = i32;
    pub type BYTE = u8;
    pub type DWORD = u32;
    pub type HANDLE = *mut c_void;
    pub type HDC = HANDLE;
    pub type HGDIOBJ = HANDLE;
    pub type HINSTANCE = HANDLE;
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

    #[repr(C)]
    pub struct MONITORINFO {
        pub cbSize: DWORD,
        pub rcMonitor: RECT,
        pub rcWork: RECT,
        pub dwFlags: DWORD,
    }

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
        pub fn GetMonitorInfoW(hmonitor: HANDLE, monitorinfo: *mut MONITORINFO) -> BOOL;
        pub fn MonitorFromPoint(point: POINT, dwflags: DWORD) -> HANDLE;
        pub fn GetDpiForSystem() -> u32;
        pub fn SetProcessDpiAwarenessContext(value: HANDLE) -> BOOL;
        pub fn ValidateRect(hwnd: HWND, rect: *const RECT) -> BOOL;
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
    DestroyWindow, GetDC, GetDpiForSystem, GetModuleHandleW, GetMonitorInfoW, GetStockObject,
    MonitorFromPoint, RegisterClassW, ReleaseDC, SelectObject, SetProcessDpiAwarenessContext,
    SetWindowPos, ShowWindow, UnregisterClassW, UpdateLayeredWindow, UpdateWindow, ValidateRect,
    BITMAPINFO, BLENDFUNCTION, HGDIOBJ, HINSTANCE, HWND, LPARAM, LRESULT, MONITORINFO, POINT, RECT,
    SIZE, UINT, WNDCLASSW, WPARAM,
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
#[cfg(windows)]
const HTTRANSPARENT: isize = -1;
#[cfg(windows)]
const MA_NOACTIVATE: isize = 3;
#[cfg(windows)]
const MONITOR_DEFAULTTOPRIMARY: u32 = 1;
#[cfg(windows)]
const AC_SRC_OVER: u8 = 0x00;
#[cfg(windows)]
const AC_SRC_ALPHA: u8 = 0x01;
#[cfg(windows)]
const SW_SHOWNOACTIVATE: i32 = 4;
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

struct OverlayWindow {
    hwnd: HWND,
    config: OverlayConfig,
    samples: Vec<Option<u32>>,
    sample_generation: u64,
    size: (i32, i32),
    position: (i32, i32),
    dirty: bool,
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

    /// Reconcile native windows and redraw only when config or samples changed.
    pub fn apply(&mut self, config: &Config, samples: &SampleStore) {
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

        let dpi = dpi_scale();
        for overlay in config.overlays.iter().filter(|overlay| overlay.enabled) {
            let (sample_generation, samples_changed, sample_buffer) = {
                let store = samples.lock().unwrap();
                let generation = store
                    .get(&overlay.id)
                    .map(|buffer| buffer.generation)
                    .unwrap_or(0);
                let changed = self
                    .windows
                    .get(&overlay.id)
                    .map(|window| window.sample_generation != generation)
                    .unwrap_or(true);
                let buffer = if changed {
                    store
                        .get(&overlay.id)
                        .map(|buffer| buffer.values.iter().copied().collect::<Vec<_>>())
                        .unwrap_or_default()
                } else {
                    Vec::new()
                };
                (generation, changed, buffer)
            };
            let (size, position) = layout_for(overlay, dpi);

            if !self.windows.contains_key(&overlay.id) {
                match self.create_window(overlay, size, position) {
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
            let config_changed = window.config != *overlay;
            let changed = window.dirty
                || window.size != size
                || window.position != position
                || config_changed
                || samples_changed;
            if config_changed {
                window.config = overlay.clone();
            }
            if samples_changed {
                window.samples = sample_buffer;
                window.sample_generation = sample_generation;
            }
            window.size = size;
            window.position = position;
            if changed {
                if let Some(window) = self.windows.get_mut(&overlay.id) {
                    Self::render_window(window);
                }
            }
        }

        if self.last_topmost.elapsed() >= Duration::from_secs(1) {
            for window in self.windows.values() {
                reassert_topmost(window.hwnd);
            }
            self.last_topmost = Instant::now();
        }
    }

    fn create_window(
        &self,
        config: &OverlayConfig,
        size: (i32, i32),
        position: (i32, i32),
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

        let mut window = OverlayWindow {
            hwnd,
            config: config.clone(),
            samples: Vec::new(),
            sample_generation: 0,
            size,
            position,
            dirty: true,
        };
        // Give the layered window its first surface before making it visible.
        // Otherwise Windows can briefly retain the class background (white)
        // behind a fully transparent first frame.
        Self::render_window(&mut window);

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

    fn render_window(window: &mut OverlayWindow) {
        let Some(rgba) = render_graph(
            window.size.0.max(1) as u32,
            window.size.1.max(1) as u32,
            &window.config,
            &window.samples,
        ) else {
            return;
        };
        if update_layered_window(window.hwnd, window.position, window.size, &rgba) {
            window.dirty = false;
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
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}

#[cfg(windows)]
#[allow(clippy::chunks_exact_to_as_chunks)]
fn update_layered_window(hwnd: HWND, position: (i32, i32), size: (i32, i32), rgba: &[u8]) -> bool {
    if size.0 <= 0 || size.1 <= 0 {
        return false;
    }
    let mut bgra = vec![0u8; rgba.len()];
    for (src, dst) in rgba.chunks_exact(4).zip(bgra.chunks_exact_mut(4)) {
        dst[0] = src[2];
        dst[1] = src[1];
        dst[2] = src[0];
        dst[3] = src[3];
    }

    unsafe {
        let screen_dc = GetDC(ptr::null_mut());
        if screen_dc.is_null() {
            return false;
        }
        let memory_dc = CreateCompatibleDC(screen_dc);
        if memory_dc.is_null() {
            ReleaseDC(ptr::null_mut(), screen_dc);
            return false;
        }

        let mut bitmap_info: BITMAPINFO = zeroed();
        bitmap_info.bmiHeader.biSize = size_of::<crate::overlay::win::BITMAPINFOHEADER>() as u32;
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
        if bitmap.is_null() || bits.is_null() {
            DeleteDC(memory_dc);
            ReleaseDC(ptr::null_mut(), screen_dc);
            return false;
        }

        let old_object = SelectObject(memory_dc, bitmap);
        if old_object.is_null() {
            DeleteObject(bitmap);
            DeleteDC(memory_dc);
            ReleaseDC(ptr::null_mut(), screen_dc);
            return false;
        }
        let byte_len = (size.0 as usize)
            .saturating_mul(size.1 as usize)
            .saturating_mul(4);
        ptr::copy_nonoverlapping(bgra.as_ptr(), bits.cast::<u8>(), byte_len);

        let destination = POINT {
            x: position.0,
            y: position.1,
        };
        let source = POINT { x: 0, y: 0 };
        let window_size = SIZE {
            cx: size.0,
            cy: size.1,
        };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA,
        };
        let result = UpdateLayeredWindow(
            hwnd,
            screen_dc,
            &destination,
            &window_size,
            memory_dc,
            &source,
            0,
            &blend,
            ULW_ALPHA,
        );

        SelectObject(memory_dc, old_object);
        DeleteObject(bitmap);
        DeleteDC(memory_dc);
        ReleaseDC(ptr::null_mut(), screen_dc);
        result != 0
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
fn dpi_scale() -> f32 {
    let dpi = unsafe { GetDpiForSystem() };
    if dpi == 0 {
        1.0
    } else {
        dpi as f32 / 96.0
    }
}

#[cfg(windows)]
fn layout_for(config: &OverlayConfig, dpi_scale: f32) -> ((i32, i32), (i32, i32)) {
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
    let margin = (config.margin_px as f64 * dpi_scale as f64) as i32;
    let work = primary_work_area();
    let left = work.left as i64;
    let top = work.top as i64;
    let right = work.right as i64;
    let bottom = work.bottom as i64;
    let width = size.0 as i64;
    let height = size.1 as i64;
    let (x, y) = match config.position {
        Anchor::TopLeft => (left + margin as i64, top + margin as i64),
        Anchor::TopCenter => (left + (right - left - width) / 2, top + margin as i64),
        Anchor::TopRight => (right - width - margin as i64, top + margin as i64),
        Anchor::CenterLeft => (left + margin as i64, top + (bottom - top - height) / 2),
        Anchor::Center => (
            left + (right - left - width) / 2,
            top + (bottom - top - height) / 2,
        ),
        Anchor::CenterRight => (
            right - width - margin as i64,
            top + (bottom - top - height) / 2,
        ),
        Anchor::BottomLeft => (left + margin as i64, bottom - height - margin as i64),
        Anchor::BottomCenter => (
            left + (right - left - width) / 2,
            bottom - height - margin as i64,
        ),
        Anchor::BottomRight => (
            right - width - margin as i64,
            bottom - height - margin as i64,
        ),
    };
    (
        size,
        (
            x.clamp(i32::MIN as i64, i32::MAX as i64) as i32,
            y.clamp(i32::MIN as i64, i32::MAX as i64) as i32,
        ),
    )
}

#[cfg(windows)]
fn primary_work_area() -> RECT {
    unsafe {
        let monitor = MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY);
        let mut info: MONITORINFO = zeroed();
        info.cbSize = size_of::<MONITORINFO>() as u32;
        if !monitor.is_null() && GetMonitorInfoW(monitor, &mut info) != 0 {
            info.rcWork
        } else {
            RECT {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1080,
            }
        }
    }
}

#[cfg(windows)]
fn wide(value: &str) -> Result<Vec<u16>, Box<dyn Error + Send + Sync>> {
    Ok(value.encode_utf16().chain(std::iter::once(0)).collect())
}
