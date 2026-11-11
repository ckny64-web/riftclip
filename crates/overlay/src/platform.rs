//! Window tweaks that only exist on Windows: never take focus, stay out of
//! the taskbar and Alt+Tab, stay out of screen captures, and show/hide from
//! any thread.

use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// The overlay's native window handle (0 if unavailable).
#[derive(Clone, Copy)]
pub struct Window(isize);

impl Window {
    pub fn from_creation(cc: &eframe::CreationContext<'_>) -> Self {
        let hwnd = match cc.window_handle().map(|h| h.as_raw()) {
            Ok(RawWindowHandle::Win32(h)) => h.hwnd.get(),
            _ => 0,
        };
        Window(hwnd)
    }

    pub fn is_valid(&self) -> bool {
        self.0 != 0
    }

    #[cfg(windows)]
    pub fn make_overlay(&self) {
        use windows::Win32::Foundation::{COLORREF, HWND};
        use windows::Win32::UI::WindowsAndMessaging::{
            GetWindowLongPtrW, SetLayeredWindowAttributes, SetWindowDisplayAffinity, SetWindowLongPtrW, GWL_EXSTYLE,
            LWA_ALPHA, WDA_EXCLUDEFROMCAPTURE, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
        };
        if self.0 == 0 {
            return;
        }
        let hwnd = HWND(self.0 as *mut core::ffi::c_void);
        unsafe {
            // Never take focus, stay out of the taskbar and Alt+Tab. Click-through
            // is left to the windowing library (it sets the layered and
            // transparent styles itself).
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex | (WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0) as isize);
            // A layered window draws nothing until its opacity has been set.
            if GetWindowLongPtrW(hwnd, GWL_EXSTYLE) & WS_EX_LAYERED.0 as isize != 0 {
                let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);
            }
            let _ = SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE);
        }
    }

    /// Window facts for the diagnostic log.
    #[cfg(windows)]
    pub fn describe(&self) -> String {
        use windows::Win32::Foundation::{HWND, RECT};
        use windows::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowLongPtrW, GetWindowRect, IsWindowVisible, GWL_EXSTYLE, GWL_STYLE,
        };
        if self.0 == 0 {
            return "no native window handle".into();
        }
        let hwnd = HWND(self.0 as *mut core::ffi::c_void);
        unsafe {
            let mut r = RECT::default();
            let _ = GetWindowRect(hwnd, &mut r);
            format!(
                "visible {}, rect ({}, {}) to ({}, {}), style {:#x}, ex-style {:#x}, is foreground {}",
                IsWindowVisible(hwnd).as_bool(),
                r.left,
                r.top,
                r.right,
                r.bottom,
                GetWindowLongPtrW(hwnd, GWL_STYLE),
                GetWindowLongPtrW(hwnd, GWL_EXSTYLE),
                GetForegroundWindow() == hwnd
            )
        }
    }

    #[cfg(not(windows))]
    pub fn describe(&self) -> String {
        "not on Windows".into()
    }

    #[cfg(windows)]
    pub fn set_visible(&self, visible: bool) {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{ShowWindowAsync, SW_HIDE, SW_SHOWNOACTIVATE};
        if self.0 == 0 {
            return;
        }
        if visible {
            // Styles may have changed since creation; a layered window whose
            // opacity was never set would stay invisible.
            self.make_overlay();
        }
        let hwnd = HWND(self.0 as *mut core::ffi::c_void);
        unsafe {
            let _ = ShowWindowAsync(hwnd, if visible { SW_SHOWNOACTIVATE } else { SW_HIDE });
        }
        crate::log(&format!("window visibility set to {visible}"));
    }

    #[cfg(not(windows))]
    pub fn make_overlay(&self) {}

    #[cfg(not(windows))]
    pub fn set_visible(&self, visible: bool) {
        crate::log(&format!("window visibility set to {visible} (no-op on this platform)"));
    }
}
