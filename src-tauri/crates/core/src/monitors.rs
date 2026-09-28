//! Which displays the machine has, and which one an overlay belongs on.
//!
//! Three things live here, and the order is the point:
//!
//! - [`MonitorInfo`], a description of one display with no Win32 in it, so the
//!   layout maths and the picker's labels can be tested on a machine that does
//!   not have the arrangement in question. Every test below runs against
//!   hand-written `MonitorInfo` values rather than the real desktop, because
//!   the states worth testing are exactly the ones a single-monitor developer
//!   cannot produce by unplugging something.
//! - [`enumerate`], the only function here that asks Windows anything.
//! - [`resolve`], the entire "which monitor does this overlay go on" rule, as a
//!   pure function over a list.
//!
//! ## Why the device name
//!
//! An index into the enumeration order is not an identity. Windows assigns
//! those by whatever order it brings adapters up, so rebooting with the
//! monitors in different ports silently moves an overlay to a different
//! physical panel, and the user has no way to tell from the graph. The device
//! name is more stable, though not perfectly so — which is why the picker also
//! shows the resolution, the scaling and where the monitor sits relative to the
//! primary. Between them, a wrong pick is visible.
//!
//! A fully stable identity (the EDID-derived instance path from
//! `QueryDisplayConfig`) survives port changes as well, and is a great deal more
//! code for a property a latency graph does not much care about.

use std::fmt;

/// A rectangle in virtual-desktop coordinates.
///
/// Deliberately not a Win32 `RECT`. This is the type the layout maths and the
/// tests speak, and it has to exist on whichever machine is running the tests
/// rather than only on the one with the monitors.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

impl Rect {
    pub fn width(&self) -> i32 {
        self.right - self.left
    }

    pub fn height(&self) -> i32 {
        self.bottom - self.top
    }
}

impl fmt::Display for Rect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}x{}", self.width(), self.height())
    }
}

/// One display, as far as anything here is concerned.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MonitorInfo {
    /// `\\.\DISPLAY1` and so on. This is what a profile stores.
    pub device: String,
    /// The whole panel, taskbar and all.
    pub bounds: Rect,
    /// The part of it not covered by an appbar.
    ///
    /// Overlays are placed against this rather than `bounds`, which is what
    /// keeps a bottom-anchored graph from sitting behind the taskbar. The two
    /// differ on any machine with a taskbar, so a test that conflates them is
    /// testing the wrong rectangle.
    pub work: Rect,
    /// Physical pixels per 96-logical-pixel inch, for this display.
    pub dpi: u32,
    pub primary: bool,
}

impl MonitorInfo {
    /// Logical pixels to physical pixels on this display.
    ///
    /// Per monitor, not per system. `GetDpiForSystem` is the primary monitor's
    /// DPI whatever the caller is doing, and this process is already
    /// `PER_MONITOR_AWARE_V2`, so asking for the system value is a way of
    /// saying "size the graph on the secondary panel as if it were the primary",
    /// which is wrong on exactly the mixed-DPI setups this feature exists for.
    pub fn scale(&self) -> f32 {
        if self.dpi == 0 {
            1.0
        } else {
            self.dpi as f32 / 96.0
        }
    }
}

/// The monitor an overlay belongs on, or `None` when it cannot be shown.
///
/// `requested` of `None` means "follow the primary", which is what every
/// profile written before monitors were selectable says. It cannot fail: if
/// Windows reports a primary monitor, there is one.
///
/// An empty or blank string is treated as `None` rather than as a device name,
/// because a hand-edited `"monitorDevice": ""` would otherwise name a monitor
/// that does not exist and hide the overlay forever with nothing on screen to
/// explain it.
///
/// A requested device that is **not connected** is `None`, and that is
/// deliberate: the pin stays and the overlay returns with the monitor, rather
/// than the graph reappearing on a display the user never chose. The cost of
/// that choice is that a hidden overlay explains itself nowhere except the
/// picker, which is why a disconnected pin is listed there as an entry rather
/// than dropped.
pub fn resolve<'a>(
    requested: Option<&str>,
    monitors: &'a [MonitorInfo],
) -> Option<&'a MonitorInfo> {
    match requested.map(str::trim).filter(|name| !name.is_empty()) {
        None => primary(monitors),
        Some(device) => monitors
            .iter()
            .find(|monitor| monitor.device.eq_ignore_ascii_case(device)),
    }
}

/// The primary display, when there is one.
pub fn primary(monitors: &[MonitorInfo]) -> Option<&MonitorInfo> {
    monitors.iter().find(|monitor| monitor.primary)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Left,
    Right,
    Aligned,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Row {
    Above,
    Below,
    Aligned,
}

fn horizontal_side(monitor: &MonitorInfo, primary: &MonitorInfo) -> Side {
    if monitor.bounds.right <= primary.bounds.left {
        Side::Left
    } else if monitor.bounds.left >= primary.bounds.right {
        Side::Right
    } else {
        Side::Aligned
    }
}

fn vertical_row(monitor: &MonitorInfo, primary: &MonitorInfo) -> Row {
    if monitor.bounds.bottom <= primary.bounds.top {
        Row::Above
    } else if monitor.bounds.top >= primary.bounds.bottom {
        Row::Below
    } else {
        Row::Aligned
    }
}

/// Where a display sits in the virtual desktop, relative to the primary.
///
/// For the picker's label only. A device name on its own tells a user nothing
/// they can check without leaving the app, and the whole reason to name the
/// display instead of numbering it is that the name can move — so the label
/// has to be the part that stays recognisable.
pub fn placement(monitor: &MonitorInfo, primary: &MonitorInfo) -> &'static str {
    if monitor.primary {
        return "primary";
    }
    match (
        horizontal_side(monitor, primary),
        vertical_row(monitor, primary),
    ) {
        (Side::Left, Row::Aligned) => "left of the primary",
        (Side::Right, Row::Aligned) => "right of the primary",
        (Side::Aligned, Row::Above) => "above the primary",
        (Side::Aligned, Row::Below) => "below the primary",
        (Side::Left, Row::Above) => "above and left of the primary",
        (Side::Right, Row::Above) => "above and right of the primary",
        (Side::Left, Row::Below) => "below and left of the primary",
        (Side::Right, Row::Below) => "below and right of the primary",
        // Duplicate-display mode reports a second monitor stacked on the
        // first, which is a real answer even though it is not a real position.
        (Side::Aligned, Row::Aligned) => "overlapping the primary",
    }
}

/// The display's scaling as a percentage, the way Windows shows it.
pub fn scale_percent(monitor: &MonitorInfo) -> u32 {
    (monitor.dpi.max(96) * 100) / 96
}

#[cfg(windows)]
mod shcore {
    use crate::overlay::win::{HMONITOR, UINT};

    /// `MDT_EFFECTIVE_DPI`.
    const MDT_EFFECTIVE_DPI: i32 = 0;

    // Present on Windows 8.1 and later, on both architectures, so this links
    // unconditionally.
    #[link(name = "shcore")]
    unsafe extern "system" {
        pub fn GetDpiForMonitor(
            hmonitor: HMONITOR,
            dpi_type: i32,
            dpi_x: *mut UINT,
            dpi_y: *mut UINT,
        ) -> i32;
    }

    pub const EFFECTIVE_DPI: i32 = MDT_EFFECTIVE_DPI;
}

#[cfg(windows)]
use crate::overlay::win::{
    EnumDisplayMonitorsW, GetMonitorInfoW, BOOL, DWORD, HDC, HMONITOR, LPARAM, MONITORINFOEXW,
    RECT, UINT,
};

/// `MONITORINFOF_PRIMARY`.
#[cfg(windows)]
const MONITORINFOF_PRIMARY: DWORD = 1;

/// Every display attached right now.
///
/// The order is whatever Windows enumerates in and carries no meaning, which is
/// why nothing outside this module may depend on it — see the note on device
/// names above.
#[cfg(windows)]
pub fn enumerate() -> Vec<MonitorInfo> {
    let mut found: Vec<MonitorInfo> = Vec::new();
    unsafe {
        EnumDisplayMonitorsW(
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            Some(collect_monitor),
            (&mut found as *mut Vec<MonitorInfo>) as LPARAM,
        );
    }
    found
}

#[cfg(not(windows))]
pub fn enumerate() -> Vec<MonitorInfo> {
    Vec::new()
}

/// One call of `EnumDisplayMonitors`.
///
/// Returns non-zero always, including for a monitor it could not read, because
/// returning zero would tell Windows to stop enumerating and silently drop
/// every display after the first oddity.
#[cfg(windows)]
unsafe extern "system" fn collect_monitor(
    monitor: HMONITOR,
    _device_context: HDC,
    _bounds: *mut RECT,
    data: LPARAM,
) -> BOOL {
    use std::mem::{size_of, zeroed};

    unsafe {
        let found = &mut *(data as *mut Vec<MonitorInfo>);
        let mut info: MONITORINFOEXW = zeroed();
        info.cbSize = size_of::<MONITORINFOEXW>() as u32;
        if GetMonitorInfoW(monitor, &mut info) == 0 {
            return 1;
        }
        let mut dpi_x: UINT = 0;
        let mut dpi_y: UINT = 0;
        let queried =
            shcore::GetDpiForMonitor(monitor, shcore::EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) >= 0;
        found.push(MonitorInfo {
            device: device_name(&info.szDevice),
            bounds: rect_of(info.rcMonitor),
            work: rect_of(info.rcWork),
            dpi: if queried && dpi_x > 0 { dpi_x } else { 96 },
            primary: info.dwFlags & MONITORINFOF_PRIMARY != 0,
        });
        1
    }
}

#[cfg(windows)]
fn device_name(chars: &[u16; 32]) -> String {
    let end = chars
        .iter()
        .position(|unit| *unit == 0)
        .unwrap_or(chars.len());
    String::from_utf16_lossy(&chars[..end])
}

#[cfg(windows)]
fn rect_of(rect: RECT) -> Rect {
    Rect {
        left: rect.left,
        top: rect.top,
        right: rect.right,
        bottom: rect.bottom,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A display with no appbar on it, so `bounds` and `work` agree unless a
    /// test says otherwise.
    fn panel(device: &str, x: i32, y: i32, width: i32, height: i32, dpi: u32) -> MonitorInfo {
        let rect = Rect {
            left: x,
            top: y,
            right: x + width,
            bottom: y + height,
        };
        MonitorInfo {
            device: device.to_string(),
            bounds: rect,
            work: rect,
            dpi,
            primary: false,
        }
    }

    fn primary_monitor() -> MonitorInfo {
        let mut monitor = panel("\\\\.\\DISPLAY1", 0, 0, 1920, 1080, 96);
        monitor.primary = true;
        monitor
    }

    /// The common two-monitor arrangement: a scaled panel to the right of the
    /// primary, each with a taskbar.
    fn mixed_dpi_desk() -> Vec<MonitorInfo> {
        let mut left = primary_monitor();
        left.work.bottom -= 40;
        let mut right = panel("\\\\.\\DISPLAY2", 1920, -200, 2560, 1440, 144);
        right.work.top += 60;
        vec![left, right]
    }

    #[test]
    fn no_request_follows_the_primary() {
        let desk = mixed_dpi_desk();
        assert_eq!(
            resolve(None, &desk).map(|m| m.device.as_str()),
            Some("\\\\.\\DISPLAY1")
        );
        assert_eq!(
            resolve(Some("  "), &desk).map(|m| m.device.as_str()),
            Some("\\\\.\\DISPLAY1")
        );
        assert_eq!(
            resolve(Some(""), &desk).map(|m| m.device.as_str()),
            Some("\\\\.\\DISPLAY1")
        );
    }

    #[test]
    fn a_requested_monitor_is_matched_ignoring_case() {
        let desk = mixed_dpi_desk();
        assert_eq!(
            resolve(Some("\\\\.\\display2"), &desk).map(|m| m.dpi),
            Some(144)
        );
    }

    #[test]
    fn a_disconnected_pin_resolves_to_nothing_rather_than_falling_back() {
        let mut desk = mixed_dpi_desk();
        // Undock the panel the overlay is pinned to.
        desk.pop();
        assert_eq!(resolve(Some("\\\\.\\DISPLAY2"), &desk), None);
        // And the shape this must never become: silently re-homing the graph
        // on the primary so the user sees it somewhere they did not ask for.
        assert_eq!(
            resolve(Some("\\\\.\\DISPLAY2"), &desk)
                .or_else(|| primary(&desk))
                .map(|m| m.device.as_str()),
            Some("\\\\.\\DISPLAY1")
        );
    }

    #[test]
    fn a_pin_returns_to_the_same_place_when_the_monitor_comes_back() {
        let with = mixed_dpi_desk();
        let without: Vec<MonitorInfo> = with.iter().filter(|m| m.primary).cloned().collect();
        assert_eq!(
            resolve(Some("\\\\.\\DISPLAY2"), &with).map(|m| m.device.as_str()),
            Some("\\\\.\\DISPLAY2")
        );
        assert_eq!(resolve(Some("\\\\.\\DISPLAY2"), &without), None);
        // Re-docking restores the identical entry, which is what puts the graph
        // back where it was rather than at a default.
        assert_eq!(
            resolve(Some("\\\\.\\DISPLAY2"), &with),
            resolve(Some("\\\\.\\DISPLAY2"), &with)
        );
    }

    #[test]
    fn no_monitors_at_all_hides_the_overlay() {
        // A headless session is a real state, and it is not the same as
        // "follow the primary", which cannot be satisfied.
        assert_eq!(resolve(None, &[]), None);
        assert_eq!(resolve(Some("\\\\.\\DISPLAY1"), &[]), None);
    }

    #[test]
    fn a_secondary_display_left_of_the_primary_has_negative_coordinates() {
        let mut right = primary_monitor();
        right.device = "\\\\.\\DISPLAY2".to_string();
        right.bounds = Rect {
            left: 1920,
            top: 0,
            right: 3840,
            bottom: 1080,
        };
        right.work = right.bounds;
        let mut left = panel("\\\\.\\DISPLAY3", -1920, 0, 1920, 1080, 96);
        left.primary = false;
        let desk = vec![right, left];
        let monitor = resolve(Some("\\\\.\\DISPLAY3"), &desk).expect("connected");
        assert_eq!(monitor.bounds.left, -1920);
        assert_eq!(monitor.work.width(), 1920);
    }

    #[test]
    fn work_area_and_bounds_are_different_rectangles() {
        let desk = mixed_dpi_desk();
        let secondary = resolve(Some("\\\\.\\DISPLAY2"), &desk).expect("connected");
        // A taskbar at the top, so the work area starts below it.
        assert_eq!(secondary.bounds.top, -200);
        assert_eq!(secondary.work.top, -140);
        assert_eq!(secondary.work.bottom, 1240);
    }

    #[test]
    fn scale_comes_from_the_display_not_the_system() {
        let desk = mixed_dpi_desk();
        let primary = resolve(None, &desk).expect("primary");
        let secondary = resolve(Some("\\\\.\\DISPLAY2"), &desk).expect("secondary");
        assert_eq!(primary.scale(), 1.0);
        assert_eq!(secondary.scale(), 1.5);
        assert_eq!(scale_percent(secondary), 150);
        // The failure this replaces: one scale read once per pass from the
        // system, so both displays came out identical and the 150% panel was
        // laid out at 100%. The assertion is `ne`, not `eq`, precisely so
        // reintroducing a single system-wide scale fails here.
        assert_ne!(primary.scale(), secondary.scale());
    }

    #[test]
    fn a_monitor_reporting_no_dpi_is_treated_as_100_percent() {
        let mut monitor = panel("\\\\.\\DISPLAY1", 0, 0, 800, 600, 0);
        monitor.primary = true;
        assert_eq!(monitor.scale(), 1.0);
        assert_eq!(scale_percent(&monitor), 100);
    }

    #[test]
    fn placement_names_every_direction() {
        let primary = primary_monitor();
        let mut cases = Vec::new();
        for (device, x, y, w, h, expected) in [
            ("L", -1920, 0, 1920, 1080, "left of the primary"),
            ("R", 1920, 0, 1920, 1080, "right of the primary"),
            ("A", 0, -1080, 1920, 1080, "above the primary"),
            ("B", 0, 1080, 1920, 1080, "below the primary"),
            (
                "LA",
                -1920,
                -1080,
                1920,
                1080,
                "above and left of the primary",
            ),
            (
                "RA",
                1920,
                -1080,
                1920,
                1080,
                "above and right of the primary",
            ),
            (
                "LB",
                -1920,
                1080,
                1920,
                1080,
                "below and left of the primary",
            ),
            (
                "RB",
                1920,
                1080,
                1920,
                1080,
                "below and right of the primary",
            ),
            ("O", 0, 0, 1920, 1080, "overlapping the primary"),
        ] {
            let monitor = panel(device, x, y, w, h, 96);
            cases.push((device, placement(&monitor, &primary), expected));
        }
        for (device, got, expected) in cases {
            assert_eq!(got, expected, "placement for {device}");
        }
        assert_eq!(placement(&primary, &primary), "primary");
    }

    #[test]
    fn a_monitor_only_half_aligned_is_aligned_on_that_axis() {
        let primary = primary_monitor();
        // Shares the primary's left edge but is above it: "above", not
        // "above and left", because there is nothing to the left of anything.
        let straddling = panel("\\\\.\\DISPLAY2", 0, -1080, 2560, 1080, 96);
        assert_eq!(placement(&straddling, &primary), "above the primary");
    }
}
