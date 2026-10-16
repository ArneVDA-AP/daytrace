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
        CredDeleteW, CredFree, CredReadW, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
    };
    use windows::Win32::System::StationsAndDesktops::{
        CloseDesktop, OpenInputDesktop, SwitchDesktop, DESKTOP_CONTROL_FLAGS, DESKTOP_SWITCHDESKTOP,
    };
    use windows::Win32::System::SystemInformation::GetTickCount;
    use windows::Win32::System::Threading::{
        OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetCursorPos, GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId, SystemParametersInfoW,
        SPI_GETSCREENSAVERRUNNING, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    };
    use windows::core::{HSTRING, PWSTR};

    pub fn idle_seconds() -> u64 {
        unsafe {
            let mut lii = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
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

    pub fn foreground_app() -> Option<ForegroundApp> {
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.is_invalid() {
                return None;
            }
            let mut buf = [0u16; 512];
            let n = GetWindowTextW(hwnd, &mut buf);
            let title = String::from_utf16_lossy(&buf[..n.max(0) as usize]);

            let mut pid: u32 = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            let mut app = String::new();
            if pid != 0 {
                if let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                    let mut path = [0u16; 1024];
                    let mut size = path.len() as u32;
                    if QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(path.as_mut_ptr()), &mut size).is_ok() {
                        let full = String::from_utf16_lossy(&path[..size as usize]);
                        app = std::path::Path::new(&full)
                            .file_stem()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_default();
                    }
                    let _ = CloseHandle(h);
                }
            }
            Some(ForegroundApp { app, title })
        }
    }

    const CRED_TARGET: &str = "Daytrace/llm-api-key";

    pub fn secret_get() -> Option<String> {
        unsafe {
            let mut ptr: *mut CREDENTIALW = std::ptr::null_mut();
            CredReadW(&HSTRING::from(CRED_TARGET), CRED_TYPE_GENERIC, None, &mut ptr).ok()?;
            let cred = &*ptr;
            let bytes = std::slice::from_raw_parts(cred.CredentialBlob, cred.CredentialBlobSize as usize);
            let value = String::from_utf8(bytes.to_vec()).ok();
            CredFree(ptr as *const core::ffi::c_void);
            value
        }
    }

    pub fn secret_set(value: &str) -> anyhow::Result<()> {
        unsafe {
            if value.is_empty() {
                let _ = CredDeleteW(&HSTRING::from(CRED_TARGET), CRED_TYPE_GENERIC, None);
                return Ok(());
            }
            let mut target: Vec<u16> = CRED_TARGET.encode_utf16().chain(std::iter::once(0)).collect();
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

    // Development fallback: the key lives in memory, seeded from DAYTRACE_API_KEY.
    static SECRET: Mutex<Option<String>> = Mutex::new(None);

    pub fn secret_get() -> Option<String> {
        let guard = SECRET.lock().unwrap();
        guard.clone().or_else(|| std::env::var("DAYTRACE_API_KEY").ok()).filter(|s| !s.is_empty())
    }
    pub fn secret_set(value: &str) -> anyhow::Result<()> {
        *SECRET.lock().unwrap() = Some(value.to_string());
        Ok(())
    }
}

pub use imp::*;

/// Password managers and authenticators hidden by default (Dayflow's seed list, adapted to
/// Windows executable names). Matching is on the normalized executable name.
pub const DEFAULT_BLOCKED_APPS: &[&str] = &[
    "1password", "authy", "bitwarden", "dashlane", "enpass", "keeper", "keepass", "keepassxc",
    "lastpass", "ledger live", "nordpass", "proton pass", "trezor suite", "yubico authenticator",
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
