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

    #[cfg(windows)]
    pub fn make_overlay(&self) {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{
            GetWindowLongPtrW, SetWindowDisplayAffinity, SetWindowLongPtrW, GWL_EXSTYLE, WDA_EXCLUDEFROMCAPTURE,
            WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
        };
        if self.0 == 0 {
            return;
        }
        let hwnd = HWND(self.0 as *mut core::ffi::c_void);
        unsafe {
            let ex = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            let add = (WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0 | WS_EX_TRANSPARENT.0 | WS_EX_LAYERED.0) as isize;
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, ex | add);
            let _ = SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE);
        }
    }

    #[cfg(windows)]
    pub fn set_visible(&self, visible: bool) {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{ShowWindowAsync, SW_HIDE, SW_SHOWNOACTIVATE};
        if self.0 == 0 {
            return;
        }
        let hwnd = HWND(self.0 as *mut core::ffi::c_void);
        unsafe {
            let _ = ShowWindowAsync(hwnd, if visible { SW_SHOWNOACTIVATE } else { SW_HIDE });
        }
    }

    #[cfg(not(windows))]
    pub fn make_overlay(&self) {}

    #[cfg(not(windows))]
    pub fn set_visible(&self, _visible: bool) {
        let _ = self.0;
    }
}
