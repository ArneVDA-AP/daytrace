//! Private-app picker: programs seen in recent screenshots plus programs with a visible window
//! right now, so one can be added to the private list with one click.

use crate::db::Db;
use anyhow::Result;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppCandidate {
    /// Executable name as the private list matches it, e.g. `chrome`.
    pub name: String,
    /// Has a visible window right now.
    pub running: bool,
    /// Last screenshot with this app in front.
    pub last_seen: Option<i64>,
    /// Screenshots with this app in front in the period.
    pub captures: i64,
}

/// How far back the screenshot history is read.
pub const HISTORY_DAYS: i64 = 30;
const MAX: usize = 60;

/// Program names from screenshots since `since`: (name, last seen, count), most recent first.
pub fn history(db: &Db, since: i64) -> Result<Vec<(String, i64, i64)>> {
    let c = db.conn();
    let mut st = c.prepare(
        "SELECT app_name, MAX(captured_at), COUNT(*) FROM screenshots
         WHERE app_name IS NOT NULL AND app_name != '' AND captured_at >= ?
         GROUP BY app_name ORDER BY MAX(captured_at) DESC LIMIT 300",
    )?;
    let rows = st.query_map([since], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, i64>(2)?)))?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// Merges history and running programs, drops names the private list already covers, shell
/// hosts and Daytrace itself. Running programs come first, then by last use.
pub fn merge(history: &[(String, i64, i64)], running: &[String], blocked: &[String]) -> Vec<AppCandidate> {
    let mut by_key: HashMap<String, AppCandidate> = HashMap::new();
    let keep = |name: &str| {
        let n = name.trim();
        !n.is_empty() && !crate::platform::is_shell_host(n) && !n.eq_ignore_ascii_case("daytrace") && crate::platform::blocked_match(n, blocked).is_none()
    };
    for (name, last, count) in history {
        if !keep(name) {
            continue;
        }
        let e = by_key.entry(name.trim().to_lowercase()).or_insert_with(|| AppCandidate { name: name.trim().to_string(), running: false, last_seen: None, captures: 0 });
        e.last_seen = Some(e.last_seen.map_or(*last, |l: i64| l.max(*last)));
        e.captures += count;
    }
    for name in running {
        if !keep(name) {
            continue;
        }
        by_key
            .entry(name.trim().to_lowercase())
            .or_insert_with(|| AppCandidate { name: name.trim().to_string(), running: false, last_seen: None, captures: 0 })
            .running = true;
    }
    let mut v: Vec<AppCandidate> = by_key.into_values().collect();
    v.sort_by(|a, b| b.running.cmp(&a.running).then(b.last_seen.cmp(&a.last_seen)).then(a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    v.truncate(MAX);
    v
}

/// Executable names of programs with a visible top-level window.
#[cfg(windows)]
pub fn running_apps() -> Vec<String> {
    use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM};
    use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DwmGetWindowAttribute};
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW};
    use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowTextLengthW, GetWindowThreadProcessId, IsWindowVisible};
    use windows::core::PWSTR;

    unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> windows::core::BOOL {
        let out = unsafe { &mut *(lparam.0 as *mut Vec<HWND>) };
        out.push(hwnd);
        true.into()
    }
    let mut hwnds: Vec<HWND> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(collect), LPARAM(&mut hwnds as *mut Vec<HWND> as isize));
    }
    let mut pids: Vec<u32> = Vec::new();
    for hwnd in hwnds {
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() || GetWindowTextLengthW(hwnd) == 0 {
                continue;
            }
            let mut cloaked: u32 = 0;
            if DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut core::ffi::c_void, 4).is_ok() && cloaked != 0 {
                continue;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid != 0 && !pids.contains(&pid) {
                pids.push(pid);
            }
        }
    }
    let mut names: Vec<String> = Vec::new();
    for pid in pids {
        unsafe {
            let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { continue };
            let mut buf = [0u16; 1024];
            let mut size = buf.len() as u32;
            if QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut size).is_ok() {
                let full = String::from_utf16_lossy(&buf[..size as usize]);
                if let Some(stem) = std::path::Path::new(&full).file_stem() {
                    let name = stem.to_string_lossy().to_string();
                    if !names.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
                        names.push(name);
                    }
                }
            }
            let _ = CloseHandle(h);
        }
    }
    names
}

#[cfg(not(windows))]
pub fn running_apps() -> Vec<String> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_and_filters() {
        let db = Db::memory();
        for (t, app) in [(100, "chrome"), (200, "Code"), (300, "chrome"), (150, "KeePassXC"), (160, "SearchHost")] {
            db.insert_screenshot(t, "", 0, 0, Some(app), None, None, None).unwrap();
        }
        let h = history(&db, 0).unwrap();
        assert_eq!(h[0], ("chrome".to_string(), 300, 2));
        let blocked = vec!["keepass".to_string()];
        let v = merge(&h, &["Slack".to_string(), "code".to_string(), "Daytrace".to_string()], &blocked);
        let names: Vec<&str> = v.iter().map(|c| c.name.as_str()).collect();
        // Running first (Code was also seen, so it sorts by last use among running), then history.
        assert_eq!(names, ["Code", "Slack", "chrome"]);
        assert!(v[0].running && v[0].captures == 1 && v[0].last_seen == Some(200));
        assert!(v[1].running && v[1].last_seen.is_none());
        assert!(!v[2].running && v[2].captures == 2);
        assert!(history(&db, 250).unwrap().len() == 1);
    }
}
