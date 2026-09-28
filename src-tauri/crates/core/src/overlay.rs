use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::ffi::c_void;
use std::mem::{size_of, zeroed};
use std::ptr;
use std::time::{Duration, Instant};

use crate::border::{border_frame_interval, BorderAnimator};
use crate::config::{smooth_frame_interval, Anchor, Config, OverlayConfig};
use crate::probes::SampleStore;
use crate::render::{
    cosmetic_prefill_samples, render_graph_into_with_border, render_prefill_into, SamplePoint,
};

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
    BITMAPINFO, BLENDFUNCTION, HDC, HGDIOBJ, HINSTANCE, HWND, LPARAM, LRESULT, MONITORINFO, POINT,
    RECT, SIZE, UINT, WNDCLASSW, WPARAM,
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

// Fake samples live only in the overlay renderer; they are retained as visual
// history after the reveal and are never inserted into SampleStore.
struct PrefillState {
    started_at: Instant,
    duration: Duration,
    samples: Vec<SamplePoint>,
    completed_rendered: bool,
}

impl PrefillState {
    fn new(config: &OverlayConfig, now: Instant) -> Self {
        Self {
            started_at: now,
            duration: Duration::from_secs(config.prefill_animation_sec.max(1) as u64),
            samples: cosmetic_prefill_samples(config, now),
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

struct OverlayWindow {
    hwnd: HWND,
    config: OverlayConfig,
    samples: Vec<SamplePoint>,
    prefill: Option<PrefillState>,
    render_points: Vec<SamplePoint>,
    history_points: Vec<SamplePoint>,
    history_dirty: bool,
    border: BorderAnimator,
    border_selected: bool,
    pixels: Vec<u8>,
    sample_generation: u64,
    size: (i32, i32),
    position: (i32, i32),
    dirty: bool,
    last_rendered: Instant,
    surface: LayeredSurface,
}

impl OverlayWindow {
    fn rebuild_history(&mut self) {
        self.history_points.clear();
        if let Some(prefill) = &self.prefill {
            self.history_points.extend(prefill.samples.iter().copied());
        }
        self.history_points.extend(self.samples.iter().copied());
        self.history_points.sort_by_key(|sample| sample.timestamp);
        self.history_dirty = false;
    }
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
            let selected = selected_id == Some(overlay.id.as_str());

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
            let config_changed = window.config != *overlay;
            let border_selection_changed = window.border_selected != selected;
            window.border_selected = selected;
            let changed = window.dirty
                || window.size != size
                || window.position != position
                || config_changed
                || samples_changed
                || border_selection_changed;
            if config_changed {
                window.config = overlay.clone();
            }
            if samples_changed {
                window.samples = sample_buffer;
                window.sample_generation = sample_generation;
            }
            if !overlay.cosmetic_startup_prefill {
                window.prefill = None;
            } else if sample_generation == 0 && (window.prefill.is_none() || config_changed) {
                window.prefill = Some(PrefillState::new(overlay, Instant::now()));
            }
            if sample_generation > 0 {
                if let Some(first_real) = window.samples.first() {
                    if let Some(prefill) = window.prefill.as_ref() {
                        if prefill.started_at > first_real.timestamp {
                            window.prefill = Some(PrefillState::new(overlay, first_real.timestamp));
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
                reassert_topmost(window.hwnd);
            }
            self.last_topmost = Instant::now();
        }
    }

    pub fn prefill_repaint_interval(&self) -> Option<Duration> {
        self.windows
            .values()
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
            samples: Vec::new(),
            prefill: config
                .cosmetic_startup_prefill
                .then(|| PrefillState::new(config, Instant::now())),
            render_points: Vec::new(),
            history_points: Vec::new(),
            history_dirty: true,
            border: BorderAnimator::new(),
            border_selected: selected,
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
        let prefill_samples = window
            .prefill
            .as_ref()
            .map(|prefill| prefill.samples.as_slice());
        let prefill_progress = window
            .prefill
            .as_ref()
            .map(|prefill| prefill.progress(now))
            .unwrap_or(0.0);
        let render_smooth = smooth || prefill_samples.is_some() || border.is_some();
        let rendered = if let Some(samples) = prefill_samples {
            if prefill_complete {
                render_graph_into_with_border(
                    window.size.0.max(1) as u32,
                    window.size.1.max(1) as u32,
                    &window.config,
                    &window.history_points,
                    now,
                    render_smooth,
                    border.as_ref(),
                    &mut window.pixels,
                )
            } else {
                render_prefill_into(
                    window.size.0.max(1) as u32,
                    window.size.1.max(1) as u32,
                    &window.config,
                    samples,
                    now,
                    prefill_progress,
                    border.as_ref(),
                    &mut window.render_points,
                    &mut window.pixels,
                )
            }
        } else {
            render_graph_into_with_border(
                window.size.0.max(1) as u32,
                window.size.1.max(1) as u32,
                &window.config,
                &window.samples,
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
fn dpi_scale() -> f32 {
    let dpi = unsafe { GetDpiForSystem() };
    if dpi == 0 {
        1.0
    } else {
        dpi as f32 / 96.0
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
    let horizontal_margin = (config.horizontal_margin_px as f64 * dpi_scale as f64).round() as i64;
    let vertical_margin = (config.vertical_margin_px as f64 * dpi_scale as f64).round() as i64;
    let work = primary_work_area();
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

#[cfg(all(test, windows))]
mod tests {
    use super::*;

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
