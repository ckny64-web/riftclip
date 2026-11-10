//! Finds the main window of the process to record.

use crate::{Error, Result, Target};
use windows::core::{BOOL, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, RECT};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetClientRect, GetWindowThreadProcessId, IsWindowVisible};

pub(crate) struct Found {
    pub hwnd: HWND,
    pub pid: u32,
}

struct Search<'a> {
    target: &'a Target,
    best: Option<(HWND, u32, i64)>,
}

fn exe_name(pid: u32) -> Option<String> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(handle);
        ok.ok()?;
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        path.rsplit(['\\', '/']).next().map(str::to_string)
    }
}

unsafe extern "system" fn on_window(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let search = &mut *(lparam.0 as *mut Search);
    if !IsWindowVisible(hwnd).as_bool() {
        return true.into();
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    let matches = match search.target {
        Target::Pid(p) => *p == pid,
        Target::ProcessName(name) => exe_name(pid).is_some_and(|n| n.eq_ignore_ascii_case(name)),
    };
    if matches {
        let mut rect = RECT::default();
        let _ = GetClientRect(hwnd, &mut rect);
        let area = (rect.right - rect.left) as i64 * (rect.bottom - rect.top) as i64;
        // A process can own several windows; the game view is the largest.
        if area > 0 && search.best.is_none_or(|(_, _, a)| area > a) {
            search.best = Some((hwnd, pid, area));
        }
    }
    true.into()
}

pub(crate) fn find(target: &Target) -> Result<Found> {
    let mut search = Search { target, best: None };
    unsafe {
        let _ = EnumWindows(Some(on_window), LPARAM(&mut search as *mut Search as isize));
    }
    match search.best {
        Some((hwnd, pid, _)) => Ok(Found { hwnd, pid }),
        None => Err(Error::WindowNotFound(match target {
            Target::Pid(p) => format!("process {p}"),
            Target::ProcessName(n) => n.clone(),
        })),
    }
}
