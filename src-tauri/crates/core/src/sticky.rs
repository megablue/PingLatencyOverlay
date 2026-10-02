//! Picking the window a sticky overlay follows.
//!
//! The pure half of sticky mode: [`crate::winwatch::shapes`] reads the desktop
//! and this decides, the same split as `monitors::enumerate` against
//! `monitors::resolve`. The interesting cases — several windows of one process,
//! the target minimized or closed — are therefore testable with synthetic
//! lists on a machine that cannot produce them.

use crate::rules::CompiledMatcher;
use crate::winwatch::WindowShape;

/// The window a sticky overlay should follow right now.
///
/// The focused candidate wins when it matches, so a target with several
/// windows resolves to the one the user is looking at; otherwise the first
/// match wins, and because the sweep arrives in z-order that is the topmost
/// one. A minimized window is never chosen: sticky mode hides while its target
/// is minimized, so a second matching window the user *is* looking at is the
/// better answer, and when there is none the overlay hides.
///
/// A matcher that cannot mean anything — empty, or carrying an error —
/// resolves to nothing, which is the same answer as "no window matched".
pub fn resolve<'a>(
    windows: &'a [WindowShape],
    matcher: &CompiledMatcher,
) -> Option<&'a WindowShape> {
    let usable = |shape: &WindowShape| !shape.iconic && matcher.matches(&shape.window);
    windows
        .iter()
        .find(|shape| shape.focused && usable(shape))
        .or_else(|| windows.iter().find(|shape| usable(shape)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::monitors::Rect;
    use crate::overlay::win::HWND;
    use crate::rules::{Condition, MatchMode, Part, WindowInfo};

    fn shape(handle: isize, process: &str, iconic: bool, focused: bool) -> WindowShape {
        WindowShape {
            hwnd: handle as HWND,
            window: WindowInfo {
                process: process.to_string(),
                title: "A window".to_string(),
                class_name: "AWindowClass".to_string(),
            },
            client: Rect {
                left: 100,
                top: 200,
                right: 500,
                bottom: 400,
            },
            iconic,
            focused,
        }
    }

    fn matcher(conditions: &[Condition]) -> CompiledMatcher {
        CompiledMatcher::compile(conditions)
    }

    fn process_is(name: &str) -> Condition {
        Condition {
            part: Part::ProcessName,
            matcher: MatchMode::Exact,
            value: name.to_string(),
        }
    }

    #[test]
    fn the_focused_match_wins_over_the_topmost_one() {
        let windows = vec![
            shape(1, "chrome.exe", false, false),
            shape(2, "explorer.exe", false, false),
            shape(3, "chrome.exe", false, true),
        ];
        let resolved =
            resolve(&windows, &matcher(&[process_is("chrome.exe")])).expect("a matching window");
        assert_eq!(resolved.hwnd, 3 as HWND, "the focused window is the target");
    }

    #[test]
    fn the_topmost_match_wins_when_none_is_focused() {
        let windows = vec![
            shape(1, "chrome.exe", false, false),
            shape(2, "chrome.exe", false, false),
        ];
        let resolved =
            resolve(&windows, &matcher(&[process_is("chrome.exe")])).expect("a matching window");
        assert_eq!(
            resolved.hwnd, 1 as HWND,
            "with no focused match the first window in z-order is the target"
        );
    }

    #[test]
    fn a_minimized_window_is_not_a_target() {
        let windows = vec![
            shape(1, "chrome.exe", true, true),
            shape(2, "chrome.exe", false, false),
        ];
        let resolved = resolve(&windows, &matcher(&[process_is("chrome.exe")]))
            .expect("the restored window is the target");
        assert_eq!(
            resolved.hwnd, 2 as HWND,
            "a minimized window cannot be followed, even when it is focused"
        );

        let only_minimized = vec![shape(1, "chrome.exe", true, true)];
        assert!(
            resolve(&only_minimized, &matcher(&[process_is("chrome.exe")])).is_none(),
            "a target that is only open minimized has nowhere to go"
        );
    }

    #[test]
    fn a_desktop_without_a_match_resolves_to_nothing() {
        let windows = vec![shape(1, "explorer.exe", false, false)];
        assert!(resolve(&windows, &matcher(&[process_is("chrome.exe")])).is_none());
        assert!(
            resolve(&windows, &matcher(&[])).is_none(),
            "an empty matcher must not mean every window"
        );
    }
}
