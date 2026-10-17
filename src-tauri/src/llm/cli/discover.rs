//! Finding the `claude` / `codex` executable. Dayflow runs the CLIs through the macOS login shell
//! (LoginShellRunner) so they are found the way Terminal finds them. On Windows there is no such
//! shell: Daytrace checks a path set in Settings, then PATH, then the places the installers use.
//!
//! Windows order for `claude` (and the same for `codex`):
//! 1. the path from Settings (a file, or a folder that holds the executable);
//! 2. every PATH folder: `claude.exe`, then `claude.cmd` (npm shim);
//! 3. `%USERPROFILE%\.local\bin\claude.exe` (native installer);
//! 4. `%LOCALAPPDATA%\Microsoft\WinGet\Links\claude.exe` (WinGet);
//! 5. `%APPDATA%\npm\claude.cmd` (npm global install; PATH can miss it when Daytrace was started
//!    before Node was installed).
//!
//! The file system and environment are behind `SysEnv` so tests can use a fake.

use super::CliTool;
use std::path::PathBuf;

pub trait SysEnv: Send + Sync {
    fn var(&self, key: &str) -> Option<String>;
    fn is_file(&self, path: &str) -> bool;
    fn is_dir(&self, path: &str) -> bool;
}

/// The real environment and file system.
pub struct RealEnv;

impl SysEnv for RealEnv {
    fn var(&self, key: &str) -> Option<String> {
        std::env::var(key).ok().filter(|v| !v.trim().is_empty())
    }
    fn is_file(&self, path: &str) -> bool {
        std::path::Path::new(path).is_file()
    }
    fn is_dir(&self, path: &str) -> bool {
        std::path::Path::new(path).is_dir()
    }
}

fn join(dir: &str, name: &str, windows: bool) -> String {
    let sep = if windows { '\\' } else { '/' };
    let dir = dir.trim().trim_matches('"').trim_end_matches(['\\', '/']);
    format!("{dir}{sep}{name}")
}

/// Executable file names to try in one folder, in order.
fn names(tool: CliTool, windows: bool) -> Vec<String> {
    let bin = tool.bin_name();
    if windows { vec![format!("{bin}.exe"), format!("{bin}.cmd")] } else { vec![bin.to_string()] }
}

/// Every place to look, in order (without the Settings override).
pub fn candidates(tool: CliTool, env: &dyn SysEnv, windows: bool) -> Vec<String> {
    let bin = tool.bin_name();
    let mut out = Vec::new();
    if let Some(path) = env.var("PATH").or_else(|| env.var("Path")) {
        let sep = if windows { ';' } else { ':' };
        for dir in path.split(sep).map(str::trim).filter(|d| !d.is_empty()) {
            for name in names(tool, windows) {
                out.push(join(dir, &name, windows));
            }
        }
    }
    if windows {
        if let Some(home) = env.var("USERPROFILE") {
            out.push(join(&home, &format!(".local\\bin\\{bin}.exe"), true));
        }
        if let Some(local) = env.var("LOCALAPPDATA") {
            out.push(join(&local, &format!("Microsoft\\WinGet\\Links\\{bin}.exe"), true));
        }
        if let Some(roaming) = env.var("APPDATA") {
            out.push(join(&roaming, &format!("npm\\{bin}.cmd"), true));
        }
    } else {
        if let Some(home) = env.var("HOME") {
            out.push(join(&home, &format!(".local/bin/{bin}"), false));
            out.push(join(&home, &format!(".claude/local/{bin}"), false));
            out.push(join(&home, &format!(".npm-global/bin/{bin}"), false));
        }
        out.push(format!("/usr/local/bin/{bin}"));
        out.push(format!("/opt/homebrew/bin/{bin}"));
    }
    out
}

/// Why no executable was found; the text contains "CLI not found" for the failure classifier.
pub fn not_found_message(tool: CliTool, override_path: Option<&str>) -> String {
    match override_path {
        Some(p) => format!(
            "{} not found at {p}. Fix the path in Settings, or leave it empty so Daytrace looks for it.",
            tool.display_name()
        ),
        None => format!(
            "{} not found. Install it ({}), sign in once in a terminal, or set its path in Settings.",
            tool.display_name(),
            tool.install_hint()
        ),
    }
}

/// Resolves the executable to run. `override_path` is the path from Settings (empty = search).
pub fn find_executable(tool: CliTool, override_path: &str, env: &dyn SysEnv, windows: bool) -> Result<PathBuf, String> {
    let custom = override_path.trim().trim_matches('"').trim();
    let custom = match custom.trim_end_matches(['\\', '/']) {
        "" => custom,
        t => t,
    };
    if !custom.is_empty() {
        if env.is_dir(custom) {
            return names(tool, windows)
                .iter()
                .map(|n| join(custom, n, windows))
                .find(|p| env.is_file(p))
                .map(PathBuf::from)
                .ok_or_else(|| not_found_message(tool, Some(custom)));
        }
        if env.is_file(custom) {
            if !windows || is_runnable_on_windows(custom) {
                return Ok(PathBuf::from(custom));
            }
            // A `claude.ps1` or extensionless npm shim was picked: use the .cmd/.exe next to it.
            let stem = custom.rsplit_once('.').filter(|(_, ext)| !ext.contains(['\\', '/'])).map(|(s, _)| s).unwrap_or(custom);
            return ["exe", "cmd"]
                .iter()
                .map(|ext| format!("{stem}.{ext}"))
                .find(|p| env.is_file(p))
                .map(PathBuf::from)
                .ok_or_else(|| not_found_message(tool, Some(custom)));
        }
        return Err(not_found_message(tool, Some(custom)));
    }
    candidates(tool, env, windows)
        .into_iter()
        .find(|p| env.is_file(p))
        .map(PathBuf::from)
        .ok_or_else(|| not_found_message(tool, None))
}

fn is_runnable_on_windows(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    [".exe", ".cmd", ".bat", ".com"].iter().any(|e| lower.ends_with(e))
}

#[cfg(test)]
pub mod fake {
    use super::SysEnv;
    use std::collections::{HashMap, HashSet};

    #[derive(Default)]
    pub struct FakeEnv {
        pub vars: HashMap<String, String>,
        pub files: HashSet<String>,
        pub dirs: HashSet<String>,
    }

    impl FakeEnv {
        pub fn new(vars: &[(&str, &str)], files: &[&str]) -> FakeEnv {
            FakeEnv {
                vars: vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
                files: files.iter().map(|f| f.to_string()).collect(),
                dirs: HashSet::new(),
            }
        }
    }

    impl SysEnv for FakeEnv {
        fn var(&self, key: &str) -> Option<String> {
            self.vars.get(key).cloned()
        }
        fn is_file(&self, path: &str) -> bool {
            self.files.contains(path)
        }
        fn is_dir(&self, path: &str) -> bool {
            self.dirs.contains(path)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::FakeEnv;
    use super::*;

    const VARS: &[(&str, &str)] = &[
        ("PATH", r#"C:\Windows\system32;"C:\Program Files\nodejs\";C:\Tools\"#),
        ("USERPROFILE", r"C:\Users\arne"),
        ("LOCALAPPDATA", r"C:\Users\arne\AppData\Local"),
        ("APPDATA", r"C:\Users\arne\AppData\Roaming"),
    ];

    fn find(files: &[&str], custom: &str) -> Result<String, String> {
        let env = FakeEnv::new(VARS, files);
        find_executable(CliTool::Claude, custom, &env, true).map(|p| p.to_string_lossy().into_owned())
    }

    #[test]
    fn windows_search_order() {
        let env = FakeEnv::new(VARS, &[]);
        let c = candidates(CliTool::Claude, &env, true);
        assert_eq!(
            c,
            vec![
                r"C:\Windows\system32\claude.exe",
                r"C:\Windows\system32\claude.cmd",
                r"C:\Program Files\nodejs\claude.exe",
                r"C:\Program Files\nodejs\claude.cmd",
                r"C:\Tools\claude.exe",
                r"C:\Tools\claude.cmd",
                r"C:\Users\arne\.local\bin\claude.exe",
                r"C:\Users\arne\AppData\Local\Microsoft\WinGet\Links\claude.exe",
                r"C:\Users\arne\AppData\Roaming\npm\claude.cmd",
            ]
        );
    }

    #[test]
    fn path_wins_over_known_locations_and_exe_over_cmd() {
        let npm = r"C:\Users\arne\AppData\Roaming\npm\claude.cmd";
        let native = r"C:\Users\arne\.local\bin\claude.exe";
        let on_path_cmd = r"C:\Tools\claude.cmd";
        let on_path_exe = r"C:\Tools\claude.exe";
        assert_eq!(find(&[npm], "").unwrap(), npm);
        assert_eq!(find(&[npm, native], "").unwrap(), native);
        assert_eq!(find(&[npm, native, on_path_cmd], "").unwrap(), on_path_cmd);
        assert_eq!(find(&[on_path_cmd, on_path_exe], "").unwrap(), on_path_exe);
        let err = find(&[], "").unwrap_err();
        assert!(err.contains("Claude Code CLI not found"), "{err}");
    }

    #[test]
    fn settings_path_is_used_first() {
        let npm = r"C:\Users\arne\AppData\Roaming\npm\claude.cmd";
        let custom = r"D:\bin\claude.exe";
        assert_eq!(find(&[npm, custom], &format!("\"{custom}\"")).unwrap(), custom);
        // A wrong custom path is an error, not a silent fallback to another install.
        let err = find(&[npm], r"D:\missing\claude.exe").unwrap_err();
        assert!(err.contains(r"not found at D:\missing\claude.exe"), "{err}");
        // A .ps1 shim maps to the .cmd next to it.
        assert_eq!(find(&[r"C:\npm\claude.ps1", r"C:\npm\claude.cmd"], r"C:\npm\claude.ps1").unwrap(), r"C:\npm\claude.cmd");
        // A folder is searched for the executable.
        let mut env = FakeEnv::new(VARS, &[r"D:\bin\claude.cmd"]);
        env.dirs.insert(r"D:\bin".into());
        assert_eq!(find_executable(CliTool::Claude, r"D:\bin\", &env, true).unwrap(), PathBuf::from(r"D:\bin\claude.cmd"));
    }

    #[test]
    fn codex_and_unix_names() {
        let env = FakeEnv::new(VARS, &[r"C:\Users\arne\AppData\Roaming\npm\codex.cmd"]);
        assert_eq!(find_executable(CliTool::Codex, "", &env, true).unwrap(), PathBuf::from(r"C:\Users\arne\AppData\Roaming\npm\codex.cmd"));
        let env = FakeEnv::new(&[("PATH", "/usr/bin:/opt/x/bin"), ("HOME", "/home/a")], &["/home/a/.local/bin/claude"]);
        assert_eq!(candidates(CliTool::Claude, &env, false)[..2], ["/usr/bin/claude".to_string(), "/opt/x/bin/claude".to_string()]);
        assert_eq!(find_executable(CliTool::Claude, "", &env, false).unwrap(), PathBuf::from("/home/a/.local/bin/claude"));
    }
}
