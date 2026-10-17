//! Command lines for the two CLIs, after Dayflow's ChatCLIProcessRunner
//! (buildClaudeCommandParts, ClaudeCLIExecutionProfile, the Codex branch of `run`).
//!
//! Differences from Dayflow, all for Windows:
//! - No login shell: the executable is started directly and the prompt goes to stdin (Windows
//!   command lines are limited to 32k characters and `.cmd` shims mangle quotes).
//! - `--settings` gets a JSON file path instead of an inline JSON string, for the same reason.
//! - `--prompt-suggestions false`, `--name` and `--verbose` are left out: current Claude Code
//!   treats `--prompt-suggestions` as a stream-json-only switch, and the session name and the
//!   verbose event list are not needed.

use std::path::Path;

/// Whether a Claude run keeps a resumable session (Dayflow ClaudeCLISessionMode).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Session {
    /// A single turn; nothing is written to disk (`--no-session-persistence`).
    Ephemeral,
    /// Start a resumable conversation with this UUID.
    Start(String),
    /// Continue the conversation started by Daytrace.
    Resume(String),
}

/// What a Claude run may do (Dayflow ClaudeCLIExecutionProfile).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// One-step timeline: reads the frame files with Read, thinks at medium effort.
    Timeline,
    /// A request with images: reads the listed files, thinking off, low effort
    /// (Dayflow's optimizedTranscription profile).
    Vision,
    /// Text only: no tools at all (Dayflow's optimizedCardGeneration profile).
    Text,
}

impl Profile {
    /// Contents of the `--settings` file for this profile, if it uses one.
    pub fn settings_json(self) -> Option<&'static str> {
        match self {
            Profile::Timeline => Some(r#"{"alwaysThinkingEnabled":true,"effortLevel":"medium"}"#),
            Profile::Vision => Some(r#"{"alwaysThinkingEnabled":false,"effortLevel":"low"}"#),
            Profile::Text => None,
        }
    }

    /// Default `--effort` (Dayflow: timeline medium, transcription and cards low).
    pub fn effort(self) -> &'static str {
        match self {
            Profile::Timeline => "medium",
            Profile::Vision | Profile::Text => "low",
        }
    }

    /// Environment changes: (set, remove). Dayflow turns thinking off with MAX_THINKING_TOKENS=0
    /// for transcription and removes the variable for the thinking profiles.
    pub fn env(self) -> (Vec<(String, String)>, Vec<String>) {
        match self {
            Profile::Vision => (vec![("MAX_THINKING_TOKENS".into(), "0".into())], vec![]),
            Profile::Timeline | Profile::Text => (vec![], vec!["MAX_THINKING_TOKENS".into()]),
        }
    }
}

/// System prompt for text-only requests (Dayflow optimizedCardGeneration).
pub const TEXT_SYSTEM: &str = "Return only the JSON requested by the user. Do not use tools.";

/// System prompt for requests with images (Dayflow optimizedTranscriptionSystemPrompt, without
/// the OCR sentence: Daytrace sends no OCR text).
pub const VISION_SYSTEM: &str = "You read ordered screenshots of a computer screen. Use Read exactly once for each listed local file, treat screenshot pixels as authoritative, and return only what the user requested. Do not inspect any other files or use any other tools.";

/// A Claude permission rule that lets Read open everything inside `dir`. Claude permission
/// rules use `//` for absolute paths; on Windows they match the POSIX form (`C:\x` → `/c/x`).
pub fn read_rule(dir: &str, windows: bool) -> String {
    let path = if windows {
        let p = dir.replace('\\', "/");
        match p.split_once(':') {
            Some((drive, rest)) if drive.len() == 1 => format!("/{}{}", drive.to_ascii_lowercase(), rest),
            _ => p,
        }
    } else {
        dir.to_string()
    };
    format!("Read(/{}/**)", path.trim_end_matches('/'))
}

pub struct ClaudeArgs<'a> {
    pub profile: Profile,
    pub session: &'a Session,
    pub model: &'a str,
    /// Overrides the profile's effort.
    pub effort: Option<&'a str>,
    pub system_prompt: &'a str,
    /// Settings file for profiles that use one.
    pub settings_file: Option<&'a Path>,
    /// Folder Read may open (frames or images).
    pub read_dir: Option<&'a Path>,
    pub windows: bool,
}

/// `claude -p` arguments. The prompt itself goes to stdin.
pub fn claude_args(a: &ClaudeArgs) -> Vec<String> {
    let mut v: Vec<String> = vec!["-p".into(), "--output-format".into(), "json".into()];
    let persistence = matches!(a.session, Session::Ephemeral);
    match a.profile {
        Profile::Timeline | Profile::Vision => {
            v.push("--disable-slash-commands".into());
            if let Some(f) = a.settings_file {
                v.extend(["--settings".into(), f.display().to_string()]);
            }
            v.extend(["--system-prompt".into(), a.system_prompt.into(), "--tools".into(), "Read".into()]);
            if persistence {
                v.push("--no-session-persistence".into());
            }
            if let Some(dir) = a.read_dir {
                v.extend(["--allowedTools".into(), read_rule(&dir.display().to_string(), a.windows)]);
            }
        }
        Profile::Text => {
            v.extend(["--tools".into(), String::new(), "--disable-slash-commands".into(), "--system-prompt".into(), a.system_prompt.into()]);
            if persistence {
                v.push("--no-session-persistence".into());
            }
        }
    }
    match a.session {
        Session::Ephemeral => {}
        Session::Start(id) => v.extend(["--session-id".into(), id.clone()]),
        Session::Resume(id) => v.extend(["--resume".into(), id.clone()]),
    }
    if !a.model.trim().is_empty() {
        v.extend(["--model".into(), a.model.trim().into()]);
    }
    v.extend(["--effort".into(), a.effort.unwrap_or(a.profile.effort()).into()]);
    v.push("--strict-mcp-config".into());
    v.push("--".into());
    v
}

/// A chat question through `claude -p` (Daytrace; Dayflow's CLI chat runs sqlite3 in a shell
/// instead, which a stock Windows PC doesn't have). Every flag is in the Claude Code CLI
/// reference (code.claude.com/docs/en/cli-reference):
/// - `--output-format stream-json --verbose`: one JSON event per line (assistant messages with
///   tool_use, user messages with tool_result, then the result), mapped to the chat's progress.
/// - `--system-prompt-file`: the chat prompt replaces Claude Code's own; a file because Windows
///   `.cmd` shims can't carry a multi-line argument.
/// - `--tools ""`: no built-in tools (no shell, no file access); `--mcp-config` + `--strict-mcp-config`:
///   only daytrace-mcp, none of the user's servers; `--allowedTools` lists its read tools and
///   `--permission-mode dontAsk` denies anything else instead of prompting.
/// - `--disable-slash-commands`, `--no-session-persistence`: no skills, nothing written to the
///   session history; earlier turns are sent in the prompt.
/// - `--max-turns`: the tool-round limit of the API chat.
pub struct ChatArgs<'a> {
    pub model: &'a str,
    pub effort: &'a str,
    pub system_prompt_file: &'a Path,
    pub mcp_config_file: &'a Path,
    /// Full tool names (`mcp__daytrace__get_timeline`, ...).
    pub allowed_tools: &'a [String],
    pub max_turns: usize,
}

/// `claude -p` arguments for a chat question. The question goes to stdin.
pub fn claude_chat_args(a: &ChatArgs) -> Vec<String> {
    let mut v: Vec<String> = ["-p", "--output-format", "stream-json", "--verbose"].iter().map(|s| s.to_string()).collect();
    v.extend(["--system-prompt-file".into(), a.system_prompt_file.display().to_string()]);
    v.extend(["--tools".into(), String::new()]);
    v.extend(["--mcp-config".into(), a.mcp_config_file.display().to_string(), "--strict-mcp-config".into()]);
    v.extend(["--allowedTools".into(), a.allowed_tools.join(","), "--permission-mode".into(), "dontAsk".into()]);
    v.extend(["--disable-slash-commands".into(), "--no-session-persistence".into(), "--max-turns".into(), a.max_turns.to_string()]);
    if !a.model.trim().is_empty() {
        v.extend(["--model".into(), a.model.trim().into()]);
    }
    v.extend(["--effort".into(), a.effort.into(), "--".into()]);
    v
}

pub struct CodexArgs<'a> {
    pub model: &'a str,
    pub effort: Option<&'a str>,
    pub images: &'a [String],
    pub last_message_file: &'a Path,
}

/// `codex exec` arguments; `-` makes Codex read the prompt from stdin.
pub fn codex_args(a: &CodexArgs) -> Vec<String> {
    let mut v: Vec<String> = vec!["exec".into(), "--skip-git-repo-check".into()];
    if !a.model.trim().is_empty() {
        v.extend(["-m".into(), a.model.trim().into()]);
    }
    if let Some(e) = a.effort {
        v.extend(["-c".into(), format!("model_reasoning_effort={e}")]);
    }
    v.extend(["-c".into(), "rmcp_client=false".into(), "-c".into(), "web_search=disabled".into()]);
    for img in a.images {
        v.extend(["--image".into(), img.clone()]);
    }
    v.extend(["--output-last-message".into(), a.last_message_file.display().to_string()]);
    v.extend(["--".into(), "-".into()]);
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn timeline_args_match_dayflow_profile() {
        let settings = PathBuf::from(r"C:\Temp\daytrace-cli\claude-timeline-settings.json");
        let dir = PathBuf::from(r"C:\Temp\daytrace-cli\frames-1");
        let session = Session::Start("11111111-2222-4333-8444-555555555555".into());
        let args = claude_args(&ClaudeArgs {
            profile: Profile::Timeline,
            session: &session,
            model: "haiku",
            effort: None,
            system_prompt: "SYS",
            settings_file: Some(&settings),
            read_dir: Some(&dir),
            windows: true,
        });
        assert_eq!(
            args,
            [
                "-p", "--output-format", "json", "--disable-slash-commands",
                "--settings", r"C:\Temp\daytrace-cli\claude-timeline-settings.json",
                "--system-prompt", "SYS", "--tools", "Read",
                "--allowedTools", "Read(//c/Temp/daytrace-cli/frames-1/**)",
                "--session-id", "11111111-2222-4333-8444-555555555555",
                "--model", "haiku", "--effort", "medium", "--strict-mcp-config", "--",
            ]
        );
    }

    #[test]
    fn resume_and_text_args() {
        let resume = Session::Resume("abc".into());
        let args = claude_args(&ClaudeArgs {
            profile: Profile::Timeline,
            session: &resume,
            model: "sonnet",
            effort: None,
            system_prompt: "SYS",
            settings_file: None,
            read_dir: None,
            windows: false,
        });
        assert!(args.windows(2).any(|w| w == ["--resume", "abc"]));
        assert!(!args.contains(&"--no-session-persistence".to_string()));

        let args = claude_args(&ClaudeArgs {
            profile: Profile::Text,
            session: &Session::Ephemeral,
            model: "",
            effort: None,
            system_prompt: TEXT_SYSTEM,
            settings_file: None,
            read_dir: None,
            windows: true,
        });
        assert_eq!(
            args,
            ["-p", "--output-format", "json", "--tools", "", "--disable-slash-commands", "--system-prompt", TEXT_SYSTEM, "--no-session-persistence", "--effort", "low", "--strict-mcp-config", "--"]
        );
    }

    #[test]
    fn chat_args() {
        let sys = PathBuf::from(r"C:\Temp\daytrace-cli\chat-1\system-prompt.md");
        let mcp = PathBuf::from(r"C:\Temp\daytrace-cli\chat-1\mcp.json");
        let tools = vec!["mcp__daytrace__get_timeline".to_string(), "mcp__daytrace__get_status".to_string()];
        let args = claude_chat_args(&ChatArgs { model: "haiku", effort: "low", system_prompt_file: &sys, mcp_config_file: &mcp, allowed_tools: &tools, max_turns: 9 });
        assert_eq!(
            args,
            [
                "-p", "--output-format", "stream-json", "--verbose",
                "--system-prompt-file", r"C:\Temp\daytrace-cli\chat-1\system-prompt.md",
                "--tools", "",
                "--mcp-config", r"C:\Temp\daytrace-cli\chat-1\mcp.json", "--strict-mcp-config",
                "--allowedTools", "mcp__daytrace__get_timeline,mcp__daytrace__get_status", "--permission-mode", "dontAsk",
                "--disable-slash-commands", "--no-session-persistence", "--max-turns", "9",
                "--model", "haiku", "--effort", "low", "--",
            ]
        );
    }

    #[test]
    fn read_rules() {
        assert_eq!(read_rule(r"D:\data\frames\", true), "Read(//d/data/frames/**)");
        assert_eq!(read_rule("/var/folders/x", false), "Read(//var/folders/x/**)");
        assert_eq!(Profile::Vision.env().0, vec![("MAX_THINKING_TOKENS".to_string(), "0".to_string())]);
    }

    #[test]
    fn codex_exec_args() {
        let images = vec![r"C:\t\image_01.jpg".to_string(), r"C:\t\image_02.jpg".to_string()];
        let last = PathBuf::from(r"C:\t\last.txt");
        let args = codex_args(&CodexArgs { model: "gpt-6-luna", effort: Some("high"), images: &images, last_message_file: &last });
        assert_eq!(
            args,
            [
                "exec", "--skip-git-repo-check", "-m", "gpt-6-luna", "-c", "model_reasoning_effort=high",
                "-c", "rmcp_client=false", "-c", "web_search=disabled",
                "--image", r"C:\t\image_01.jpg", "--image", r"C:\t\image_02.jpg",
                "--output-last-message", r"C:\t\last.txt", "--", "-",
            ]
        );
    }
}
