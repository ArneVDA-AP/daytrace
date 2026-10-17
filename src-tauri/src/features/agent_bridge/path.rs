//! Putting `daytrace-mcp` on the user's PATH, so `daytrace-mcp cli today` works in any terminal
//! and MCP clients can start it by name (Dayflow's `dayflow link` / `unlink`, which symlink into
//! /usr/local/bin; MIT, (c) 2025 Jerry Liu).
//!
//! Windows has no such folder: the install folder itself goes into the user's `Path` value under
//! HKCU\Environment, and a WM_SETTINGCHANGE broadcast tells Explorer to pass the new value to
//! programs started from then on. Only the current user's value is touched (no administrator
//! rights). The installer runs `daytrace-mcp link` after installing and `unlink` before
//! uninstalling (src-tauri/windows/installer-hooks.nsh); Settings > AI tools has the same two
//! actions.
//!
//! The list editing is plain string work (tested on every platform); reading and writing the
//! registry is Windows-only.

use std::path::{Path, PathBuf};

/// The folder that holds the running executable (daytrace.exe and daytrace-mcp.exe sit together).
pub fn install_dir() -> Option<PathBuf> {
    std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf))
}

/// `%NAME%` references expanded from `env` (unknown names stay as written), for comparing.
fn expand(entry: &str, env: &dyn Fn(&str) -> Option<String>) -> String {
    let mut out = String::new();
    let mut rest = entry;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) if end > 0 => {
                let name = &after[..end];
                match env(name) {
                    Some(v) => out.push_str(&v),
                    None => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = &after[end + 1..];
            }
            _ => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// One PATH entry in comparable form: quotes and trailing separators gone, variables expanded,
/// forward slashes as backslashes, lower case (Windows paths ignore case).
fn normalize(entry: &str, env: &dyn Fn(&str) -> Option<String>) -> String {
    let e = entry.trim().trim_matches('"').trim();
    let e = expand(e, env).replace('/', "\\");
    e.trim_end_matches('\\').to_lowercase()
}

fn real_env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

/// Whether `dir` is one of the entries of `path` (a `;`-separated Windows PATH value).
pub fn contains(path: &str, dir: &str, env: &dyn Fn(&str) -> Option<String>) -> bool {
    let want = normalize(dir, env);
    !want.is_empty() && path.split(';').any(|e| normalize(e, env) == want)
}

/// `path` with `dir` appended, or None when it is already there. Other entries stay as written.
pub fn with_dir(path: &str, dir: &str, env: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    if contains(path, dir, env) {
        return None;
    }
    let trimmed = path.trim_end_matches(';');
    Some(if trimmed.trim().is_empty() { dir.to_string() } else { format!("{trimmed};{dir}") })
}

/// `path` without any entry equal to `dir`, or None when there is none.
pub fn without_dir(path: &str, dir: &str, env: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    if !contains(path, dir, env) {
        return None;
    }
    let want = normalize(dir, env);
    let kept: Vec<&str> = path.split(';').filter(|e| !e.trim().is_empty() && normalize(e, env) != want).collect();
    Some(kept.join(";"))
}

/// What `link` / `unlink` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    Added,
    Removed,
    Unchanged,
}

/// Whether `dir` is on the user's PATH (HKCU\Environment). False where there is no such value.
pub fn is_linked(dir: &Path) -> bool {
    match imp::read_user_path() {
        Ok(Some((value, _))) => contains(&value, &dir.to_string_lossy(), &real_env),
        _ => false,
    }
}

/// Adds `dir` to the user's PATH.
pub fn link(dir: &Path) -> anyhow::Result<Change> {
    let dir = dir.to_string_lossy().to_string();
    let (current, expandable) = imp::read_user_path()?.unwrap_or_default();
    match with_dir(&current, &dir, &real_env) {
        None => Ok(Change::Unchanged),
        Some(next) => {
            // A new value with %VARIABLES% in it must stay REG_EXPAND_SZ; Windows creates the
            // value that way, so keep the type it had (a new value: expandable).
            imp::write_user_path(&next, expandable || current.is_empty())?;
            imp::broadcast_environment_change();
            Ok(Change::Added)
        }
    }
}

/// Removes `dir` from the user's PATH.
pub fn unlink(dir: &Path) -> anyhow::Result<Change> {
    let dir = dir.to_string_lossy().to_string();
    let Some((current, expandable)) = imp::read_user_path()? else { return Ok(Change::Unchanged) };
    match without_dir(&current, &dir, &real_env) {
        None => Ok(Change::Unchanged),
        Some(next) => {
            imp::write_user_path(&next, expandable)?;
            imp::broadcast_environment_change();
            Ok(Change::Removed)
        }
    }
}

#[cfg(windows)]
mod imp {
    use anyhow::{anyhow, Result};
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS, LPARAM, WPARAM};
    use windows::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_EXPAND_SZ, REG_SZ,
        REG_VALUE_TYPE,
    };
    use windows::Win32::UI::WindowsAndMessaging::{SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE};

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    struct Key(HKEY);
    impl Drop for Key {
        fn drop(&mut self) {
            // SAFETY: the key was opened by RegOpenKeyExW and is closed once.
            unsafe {
                let _ = RegCloseKey(self.0);
            }
        }
    }

    fn open(access: windows::Win32::System::Registry::REG_SAM_FLAGS) -> Result<Key> {
        let mut key = HKEY(std::ptr::null_mut());
        let subkey = wide("Environment");
        // SAFETY: valid constant root key, NUL-terminated subkey, out pointer to a local.
        let r = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, PCWSTR(subkey.as_ptr()), None, access, &mut key) };
        if r != ERROR_SUCCESS {
            return Err(anyhow!("Could not open HKEY_CURRENT_USER\\Environment (error {})", r.0));
        }
        Ok(Key(key))
    }

    /// The user's Path value and whether it is REG_EXPAND_SZ; None when there is none.
    pub fn read_user_path() -> Result<Option<(String, bool)>> {
        let key = open(KEY_QUERY_VALUE)?;
        let name = wide("Path");
        let mut kind = REG_VALUE_TYPE::default();
        let mut size: u32 = 0;
        // SAFETY: size query with no buffer.
        let r = unsafe { RegQueryValueExW(key.0, PCWSTR(name.as_ptr()), None, Some(&mut kind as *mut REG_VALUE_TYPE), None, Some(&mut size as *mut u32)) };
        if r == ERROR_FILE_NOT_FOUND {
            return Ok(None);
        }
        if r != ERROR_SUCCESS {
            return Err(anyhow!("Could not read the user PATH (error {})", r.0));
        }
        if kind != REG_SZ && kind != REG_EXPAND_SZ {
            return Err(anyhow!("The user PATH is not a text value; leaving it alone"));
        }
        // The value can change between the two calls; one retry with the new size.
        for _ in 0..2 {
            let mut buf: Vec<u16> = vec![0; (size as usize).div_ceil(2) + 1];
            let mut bytes = (buf.len() * 2) as u32;
            // SAFETY: buf has `bytes` writable bytes.
            let r = unsafe { RegQueryValueExW(key.0, PCWSTR(name.as_ptr()), None, Some(&mut kind as *mut REG_VALUE_TYPE), Some(buf.as_mut_ptr() as *mut u8), Some(&mut bytes as *mut u32)) };
            if r == windows::Win32::Foundation::ERROR_MORE_DATA {
                size = bytes;
                continue;
            }
            if r != ERROR_SUCCESS {
                return Err(anyhow!("Could not read the user PATH (error {})", r.0));
            }
            let len = (bytes as usize / 2).min(buf.len());
            let mut text = &buf[..len];
            while let Some((&0, head)) = text.split_last() {
                text = head;
            }
            return Ok(Some((String::from_utf16_lossy(text), kind == REG_EXPAND_SZ)));
        }
        Err(anyhow!("The user PATH kept changing while it was read"))
    }

    pub fn write_user_path(value: &str, expandable: bool) -> Result<()> {
        let key = open(KEY_SET_VALUE)?;
        let name = wide("Path");
        let data = wide(value);
        // SAFETY: the byte view covers the UTF-16 buffer including its terminating zero.
        let bytes = unsafe { std::slice::from_raw_parts(data.as_ptr() as *const u8, data.len() * 2) };
        let kind = if expandable { REG_EXPAND_SZ } else { REG_SZ };
        // SAFETY: open key, NUL-terminated value name, valid data slice.
        let r = unsafe { RegSetValueExW(key.0, PCWSTR(name.as_ptr()), None, kind, Some(bytes)) };
        if r != ERROR_SUCCESS {
            return Err(anyhow!("Could not write the user PATH (error {})", r.0));
        }
        Ok(())
    }

    /// Tells Explorer and other listening programs that the environment changed, so programs
    /// started afterwards get the new PATH. Waits at most 5 s for a hung window.
    pub fn broadcast_environment_change() {
        let env = wide("Environment");
        // SAFETY: lParam points at a NUL-terminated UTF-16 string that outlives the call.
        unsafe {
            let _ = SendMessageTimeoutW(
                HWND_BROADCAST,
                WM_SETTINGCHANGE,
                WPARAM(0),
                LPARAM(PCWSTR(env.as_ptr()).0 as isize),
                SMTO_ABORTIFHUNG,
                5000,
                None,
            );
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use anyhow::{bail, Result};

    pub fn read_user_path() -> Result<Option<(String, bool)>> {
        Ok(None)
    }
    pub fn write_user_path(_value: &str, _expandable: bool) -> Result<()> {
        bail!("Adding daytrace-mcp to PATH is only available on Windows.")
    }
    pub fn broadcast_environment_change() {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(name: &str) -> Option<String> {
        match name {
            "LOCALAPPDATA" => Some(r"C:\Users\me\AppData\Local".into()),
            "USERPROFILE" => Some(r"C:\Users\me".into()),
            _ => None,
        }
    }

    const DIR: &str = r"C:\Users\me\AppData\Local\Daytrace";

    #[test]
    fn adds_once_and_keeps_other_entries_verbatim() {
        let before = r"%USERPROFILE%\.local\bin;C:\Tools\;%LOCALAPPDATA%\Microsoft\WindowsApps";
        let after = with_dir(before, DIR, &env).unwrap();
        assert_eq!(after, format!("{before};{DIR}"));
        assert_eq!(with_dir(&after, DIR, &env), None, "already there");
        // Spelled differently: other case, trailing slash, quotes, a variable, forward slashes.
        for same in [r"c:\users\ME\appdata\local\daytrace\", r#""C:\Users\me\AppData\Local\Daytrace""#, r"%LOCALAPPDATA%\Daytrace", "C:/Users/me/AppData/Local/Daytrace"] {
            assert!(contains(&format!(r"C:\x;{same}"), DIR, &env), "{same}");
        }
        assert_eq!(with_dir("", DIR, &env).unwrap(), DIR);
        assert_eq!(with_dir(r"C:\a;", DIR, &env).unwrap(), format!(r"C:\a;{DIR}"));
        // A longer folder that merely starts with ours is a different entry.
        assert!(!contains(r"C:\Users\me\AppData\Local\Daytrace2", DIR, &env));
    }

    #[test]
    fn removes_every_spelling_and_nothing_else() {
        let before = format!(r"C:\a;%LOCALAPPDATA%\Daytrace;C:\b;;{}\", DIR.to_lowercase());
        assert_eq!(without_dir(&before, DIR, &env).unwrap(), r"C:\a;C:\b");
        assert_eq!(without_dir(r"C:\a;C:\b", DIR, &env), None);
        assert_eq!(without_dir(DIR, DIR, &env).unwrap(), "");
    }

    #[test]
    fn expansion_leaves_unknown_and_stray_percent_signs() {
        assert_eq!(expand(r"%NOPE%\x;100%", &env), r"%NOPE%\x;100%");
        assert_eq!(expand(r"%USERPROFILE%\bin", &env), r"C:\Users\me\bin");
        assert_eq!(expand("%%", &env), "%%");
    }
}
