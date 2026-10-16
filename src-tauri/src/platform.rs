//! OS integration: idle time, lock detection, the monitor under the cursor, the foreground app,
//! and secret storage. Windows uses Win32 directly; other platforms get neutral fallbacks so the
//! app can be developed and tested on Linux.

#[derive(Debug, Clone, Default)]
pub struct ForegroundApp {
    /// Executable stem, e.g. `chrome`, `Code`, `1Password`.
    pub app: String,
    pub title: String,
}

#[cfg(windows)]
mod imp {
    use super::ForegroundApp;
    use windows::Win32::Foundation::{CloseHandle, POINT};
    use windows::Win32::Security::Credentials::{
        CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW, CredFree, CredReadW, CredWriteW,
    };
    use windows::Win32::System::StationsAndDesktops::{
        CloseDesktop, DESKTOP_CONTROL_FLAGS, DESKTOP_SWITCHDESKTOP, OpenInputDesktop, SwitchDesktop,
    };
    use windows::Win32::System::SystemInformation::GetTickCount;
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetCursorPos, GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId, SPI_GETSCREENSAVERRUNNING,
        SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
    };
    use windows::core::{HSTRING, PWSTR};

    pub fn idle_seconds() -> u64 {
        unsafe {
            let mut lii = LASTINPUTINFO {
                cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
                dwTime: 0,
            };
            if !GetLastInputInfo(&mut lii).as_bool() {
                return 0;
            }
            let now = GetTickCount();
            (now.wrapping_sub(lii.dwTime) / 1000) as u64
        }
    }

    /// True when the workstation is locked (input desktop is the secure desktop) or the
    /// screensaver is running.
    pub fn is_locked() -> bool {
        unsafe {
            let mut running: i32 = 0;
            if SystemParametersInfoW(
                SPI_GETSCREENSAVERRUNNING,
                0,
                Some(&mut running as *mut i32 as *mut core::ffi::c_void),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )
            .is_ok()
                && running != 0
            {
                return true;
            }
            match OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_SWITCHDESKTOP) {
                Ok(desk) => {
                    // Switching to the current input desktop is a no-op when unlocked and fails
                    // while the secure (lock) desktop is active.
                    let ok = SwitchDesktop(desk).is_ok();
                    let _ = CloseDesktop(desk);
                    !ok
                }
                Err(_) => true,
            }
        }
    }

    pub fn cursor_position() -> Option<(i32, i32)> {
        unsafe {
            let mut p = POINT::default();
            GetCursorPos(&mut p).ok()?;
            Some((p.x, p.y))
        }
    }

    fn process_name(hwnd: windows::Win32::Foundation::HWND) -> String {
        unsafe { process_name_inner(hwnd) }
    }

    unsafe fn process_name_inner(hwnd: windows::Win32::Foundation::HWND) -> String {
        unsafe {
            let mut pid: u32 = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == 0 {
                return String::new();
            }
            let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                return String::new();
            };
            let mut path = [0u16; 1024];
            let mut size = path.len() as u32;
            let mut name = String::new();
            if QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(path.as_mut_ptr()), &mut size).is_ok() {
                let full = String::from_utf16_lossy(&path[..size as usize]);
                name = std::path::Path::new(&full)
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default();
            }
            let _ = CloseHandle(h);
            name
        }
    }

    /// Screen rectangles (left, top, right, bottom; physical pixels) of visible top-level windows
    /// whose process matches the blocklist.
    pub fn blocked_window_rects(blocked: &[String]) -> Vec<(i32, i32, i32, i32)> {
        use windows::Win32::Foundation::{HWND, LPARAM, RECT};
        use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
        use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowRect, IsIconic, IsWindowVisible};

        unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
            let out = unsafe { &mut *(lparam.0 as *mut Vec<HWND>) };
            out.push(hwnd);
            true.into()
        }
        let mut hwnds: Vec<HWND> = Vec::new();
        unsafe {
            let _ = EnumWindows(Some(collect), LPARAM(&mut hwnds as *mut Vec<HWND> as isize));
        }
        let mut rects = Vec::new();
        for hwnd in hwnds {
            unsafe {
                if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
                    continue;
                }
                let mut cloaked: u32 = 0;
                if DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut core::ffi::c_void, 4).is_ok() && cloaked != 0
                {
                    continue; // on another virtual desktop or hidden by the shell
                }
                let name = process_name(hwnd);
                if super::blocked_match(&name, blocked).is_none() {
                    continue;
                }
                let mut r = RECT::default();
                if GetWindowRect(hwnd, &mut r).is_ok() && r.right > r.left && r.bottom > r.top {
                    rects.push((r.left, r.top, r.right, r.bottom));
                }
            }
        }
        rects
    }

    pub fn foreground_app() -> Option<ForegroundApp> {
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.is_invalid() {
                return None;
            }
            let mut buf = [0u16; 512];
            let n = GetWindowTextW(hwnd, &mut buf);
            let title = String::from_utf16_lossy(&buf[..n.max(0) as usize]);

            Some(ForegroundApp {
                app: process_name(hwnd),
                title,
            })
        }
    }

    fn target(slot: &str) -> String {
        format!("Daytrace/{slot}")
    }

    pub fn secret_get(slot: &str) -> Option<String> {
        unsafe {
            let mut ptr: *mut CREDENTIALW = std::ptr::null_mut();
            CredReadW(&HSTRING::from(target(slot)), CRED_TYPE_GENERIC, None, &mut ptr).ok()?;
            let cred = &*ptr;
            let bytes = std::slice::from_raw_parts(cred.CredentialBlob, cred.CredentialBlobSize as usize);
            let value = String::from_utf8(bytes.to_vec()).ok();
            CredFree(ptr as *const core::ffi::c_void);
            value
        }
    }

    pub fn secret_set(slot: &str, value: &str) -> anyhow::Result<()> {
        unsafe {
            if value.is_empty() {
                let _ = CredDeleteW(&HSTRING::from(target(slot)), CRED_TYPE_GENERIC, None);
                return Ok(());
            }
            let mut target: Vec<u16> = target(slot).encode_utf16().chain(std::iter::once(0)).collect();
            let mut user: Vec<u16> = "daytrace".encode_utf16().chain(std::iter::once(0)).collect();
            let mut blob = value.as_bytes().to_vec();
            let cred = CREDENTIALW {
                Type: CRED_TYPE_GENERIC,
                TargetName: PWSTR(target.as_mut_ptr()),
                CredentialBlobSize: blob.len() as u32,
                CredentialBlob: blob.as_mut_ptr(),
                Persist: CRED_PERSIST_LOCAL_MACHINE,
                UserName: PWSTR(user.as_mut_ptr()),
                ..Default::default()
            };
            CredWriteW(&cred, 0)?;
            Ok(())
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::ForegroundApp;
    use std::sync::Mutex;

    pub fn idle_seconds() -> u64 {
        0
    }
    pub fn is_locked() -> bool {
        false
    }
    pub fn cursor_position() -> Option<(i32, i32)> {
        None
    }
    pub fn foreground_app() -> Option<ForegroundApp> {
        None
    }
    pub fn blocked_window_rects(_blocked: &[String]) -> Vec<(i32, i32, i32, i32)> {
        Vec::new()
    }

    // Development fallback: keys live in memory, seeded from DAYTRACE_API_KEY.
    static SECRETS: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

    pub fn secret_get(slot: &str) -> Option<String> {
        let guard = SECRETS.lock().unwrap();
        guard
            .iter()
            .find(|(s, _)| s == slot)
            .map(|(_, v)| v.clone())
            .or_else(|| std::env::var("DAYTRACE_API_KEY").ok())
            .filter(|s| !s.is_empty())
    }
    pub fn secret_set(slot: &str, value: &str) -> anyhow::Result<()> {
        let mut g = SECRETS.lock().unwrap();
        g.retain(|(s, _)| s != slot);
        g.push((slot.to_string(), value.to_string()));
        Ok(())
    }
}

pub use imp::*;

/// Password managers and authenticators hidden by default (Dayflow's seed list, adapted to
/// Windows executable names). Matching is on the normalized executable name.
pub const DEFAULT_BLOCKED_APPS: &[&str] = &[
    "1password",
    "authy",
    "bitwarden",
    "dashlane",
    "enpass",
    "keeper",
    "keepass",
    "keepassxc",
    "lastpass",
    "ledger live",
    "nordpass",
    "proton pass",
    "trezor suite",
    "yubico authenticator",
];

fn normalize(s: &str) -> String {
    s.to_lowercase().chars().filter(|c| !matches!(c, ' ' | '.' | '-' | '_')).collect()
}

/// Returns the blocklist entry that matches the given executable name, if any.
pub fn blocked_match<'a>(app: &str, blocked: &'a [String]) -> Option<&'a str> {
    let a = normalize(app);
    if a.is_empty() {
        return None;
    }
    blocked
        .iter()
        .map(|b| (b, normalize(b)))
        .find(|(_, nb)| !nb.is_empty() && (a == *nb || a.contains(nb.as_str())))
        .map(|(b, _)| b.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matches_blocked_apps() {
        let list: Vec<String> = DEFAULT_BLOCKED_APPS.iter().map(|s| s.to_string()).collect();
        assert_eq!(blocked_match("KeePassXC", &list), Some("keepass"));
        assert_eq!(blocked_match("Proton Pass", &list), Some("proton pass"));
        assert_eq!(blocked_match("1Password", &list), Some("1password"));
        assert_eq!(blocked_match("chrome", &list), None);
        assert_eq!(blocked_match("", &list), None);
    }
}
