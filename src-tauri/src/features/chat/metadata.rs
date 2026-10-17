//! The trailing ```suggestions``` and ```memory``` blocks the chat prompt asks for, and the
//! memory blob rules. Ports Dayflow's ChatMetadataParser.swift, ChatService.parseAssistantMetadata
//! and DashboardChatMemoryStore.swift (MIT, © Jerry Liu). The Swift version uses regexes with
//! look-ahead, which Rust's regex crate does not support, so the matching is written out by hand
//! with the same behaviour.

/// DashboardChatMemoryStore.maxCharacters
pub const MEMORY_MAX_CHARS: usize = 10_000;

#[derive(Debug, Clone, PartialEq, Default)]
pub struct AssistantMetadata {
    pub cleaned_text: String,
    pub suggestions: Vec<String>,
    pub memory: Option<String>,
}

/// Byte offsets of the lines in `text`: (start, end without the newline).
fn lines(text: &str) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, ch) in text.char_indices() {
        if ch == '\n' {
            out.push((start, i));
            start = i + 1;
        }
    }
    out.push((start, text.len()));
    out
}

/// A heading line Dayflow strips together with its block: `## Suggestions` or `Suggestions:`.
fn is_heading(line: &str, word: &str) -> bool {
    let t = line.trim();
    let lower = t.to_lowercase();
    let hashes = lower.chars().take_while(|c| *c == '#').count();
    if (1..=6).contains(&hashes) && lower[hashes..].trim() == word {
        return true;
    }
    lower.strip_suffix(':').is_some_and(|w| w.trim() == word)
}

/// Removes the first fenced block tagged `tag` (```suggestions ... ```), plus a heading right
/// before it, and returns (text without it, block content).
pub fn extract_tagged_block(text: &str, tag: &str) -> Option<(String, String)> {
    let ls = lines(text);
    let opener = format!("```{tag}");
    let idx = ls.iter().position(|(s, e)| {
        let l = text[*s..*e].trim().to_lowercase();
        l.strip_prefix(&opener).is_some_and(|rest| rest.trim().is_empty())
    })?;
    let content_start = ls.get(idx + 1).map(|(s, _)| *s).unwrap_or(text.len());
    let close_rel = text[content_start..].find("```")?;
    let close = content_start + close_rel;
    let content = text[content_start..close].trim_end_matches(['\n', '\r']).to_string();
    let block_end = close + 3;
    // Include a heading line (and the blank lines between) right above the fence.
    let mut block_start = ls[idx].0;
    let mut j = idx;
    while j > 0 {
        let (s, e) = ls[j - 1];
        let l = &text[s..e];
        if l.trim().is_empty() {
            j -= 1;
            continue;
        }
        if is_heading(l, tag) {
            block_start = s;
        }
        break;
    }
    let mut out = String::with_capacity(text.len());
    out.push_str(&text[..block_start]);
    out.push_str(&text[block_end..]);
    Some((out, content))
}

/// Dayflow's extractLooseSuggestions: a "Suggestions" heading followed by a JSON array, up to a
/// Memory heading or the end of the text.
pub fn extract_loose_suggestions(text: &str) -> Option<(String, String)> {
    let ls = lines(text);
    let idx = ls.iter().position(|(s, e)| is_heading(&text[*s..*e], "suggestions"))?;
    let after = ls[idx].1;
    let rest = &text[after..];
    let open = rest.find('[')?;
    if !rest[..open].trim().is_empty() {
        return None;
    }
    let close = rest.find(']')?;
    let content = rest[open..=close].to_string();
    let tail = rest[close + 1..].trim_start();
    let tail_lower = tail.to_lowercase();
    let next_is_memory = tail.is_empty() || tail_lower.starts_with("memory:") || (tail.starts_with('#') && tail_lower.trim_start_matches('#').trim_start().starts_with("memory"));
    if !next_is_memory {
        return None;
    }
    let end = after + close + 1;
    let mut out = String::new();
    out.push_str(&text[..ls[idx].0]);
    out.push('\n');
    out.push_str(&text[end..]);
    Some((out, content))
}

/// Dayflow's extractLooseMemory: a "Memory" heading followed by Profile:/Style: lines at the end.
pub fn extract_loose_memory(text: &str) -> Option<(String, String)> {
    let ls = lines(text);
    let idx = ls.iter().rposition(|(s, e)| is_heading(&text[*s..*e], "memory"))?;
    let body = text[ls[idx].1..].trim_start_matches(['\n', '\r']);
    if !body.trim_start().to_lowercase().starts_with("profile:") || !body.to_lowercase().contains("style:") {
        return None;
    }
    if body.lines().any(|l| l.trim_start().starts_with('#')) {
        return None;
    }
    let mut out = text[..ls[idx].0].to_string();
    out.push('\n');
    Some((out, body.to_string()))
}

pub fn strip_residual_headings(text: &str) -> String {
    text.split('\n')
        .filter(|l| !(is_heading(l, "suggestions") || is_heading(l, "memory")))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Keeps only the Profile: and Style: lines (Dayflow's normalizeAutoMemoryBlob).
pub fn normalize_auto_memory(raw: &str) -> Option<String> {
    let mut profile = None;
    let mut style = None;
    for line in raw.replace("\r\n", "\n").replace('\r', "\n").split('\n').map(str::trim).filter(|l| !l.is_empty()) {
        let lower = line.to_lowercase();
        if lower.starts_with("profile:") {
            let v = line["profile:".len()..].trim();
            if !v.is_empty() {
                profile = Some(v.to_string());
            }
        } else if lower.starts_with("style:") {
            let v = line["style:".len()..].trim();
            if !v.is_empty() {
                style = Some(v.to_string());
            }
        }
    }
    let mut out = Vec::new();
    if let Some(p) = profile {
        out.push(format!("Profile: {p}"));
    }
    if let Some(s) = style {
        out.push(format!("Style: {s}"));
    }
    (!out.is_empty()).then(|| out.join("\n"))
}

/// DashboardChatMemoryStore.normalize: unify newlines, collapse blank runs, cap the length.
pub fn normalize_memory(input: &str) -> String {
    let mut text = input.replace("\r\n", "\n").replace('\r', "\n").trim().to_string();
    while text.contains("\n\n\n") {
        text = text.replace("\n\n\n", "\n\n");
    }
    if text.chars().count() > MEMORY_MAX_CHARS {
        text = text.chars().take(MEMORY_MAX_CHARS).collect::<String>().trim().to_string();
    }
    text
}

fn parse_string_array(raw: &str) -> Option<Vec<String>> {
    serde_json::from_str::<Vec<String>>(raw.trim()).ok()
}

/// ChatService.parseAssistantMetadata.
pub fn parse(text: &str) -> AssistantMetadata {
    let mut working = text.to_string();
    let mut suggestions = Vec::new();
    let mut memory = None;

    if let Some((stripped, raw)) = extract_tagged_block(&working, "suggestions") {
        working = stripped;
        match parse_string_array(&raw) {
            Some(s) => suggestions = s,
            None => log::info!("chat: failed to parse suggestions JSON"),
        }
    }
    if suggestions.is_empty() {
        if let Some((stripped, raw)) = extract_loose_suggestions(&working) {
            working = stripped;
            if let Some(s) = parse_string_array(&raw) {
                suggestions = s;
            }
        }
    }
    if let Some((stripped, raw)) = extract_tagged_block(&working, "memory") {
        working = stripped;
        memory = normalize_auto_memory(&raw);
    }
    if memory.is_none() {
        if let Some((stripped, raw)) = extract_loose_memory(&working) {
            working = stripped;
            memory = normalize_auto_memory(&raw);
        }
    }
    working = strip_residual_headings(&working);
    suggestions = suggestions.into_iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).take(4).collect();
    AssistantMetadata { cleaned_text: working.trim().to_string(), suggestions, memory }
}

/// ChatService.appendRecentSuggestions: newest last, case-insensitive dedupe, at most 12.
pub fn push_recent(history: &mut Vec<String>, new: &[String]) {
    for s in new {
        let t = s.trim();
        if t.is_empty() {
            continue;
        }
        let key = t.to_lowercase();
        history.retain(|h| h.trim().to_lowercase() != key);
        history.push(t.to_string());
    }
    if history.len() > 12 {
        let extra = history.len() - 12;
        history.drain(..extra);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_both_fenced_blocks() {
        let text = "Pulled yesterday's cards.\n- **Morning:** code\n\n```suggestions\n[\"When did I start?\", \"Most focused hour?\", \"Any long breaks?\"]\n```\n\n```memory\nProfile: Student.\nStyle: default=brief\nIgnored: x\n```";
        let m = parse(text);
        assert_eq!(m.cleaned_text, "Pulled yesterday's cards.\n- **Morning:** code");
        assert_eq!(m.suggestions.len(), 3);
        assert_eq!(m.memory.as_deref(), Some("Profile: Student.\nStyle: default=brief"));
    }

    #[test]
    fn strips_headings_and_handles_loose_blocks() {
        let text = "Answer.\n\n### Suggestions\n```suggestions\n[\"A?\"]\n```\n### Memory\n```memory\nProfile: p\nStyle: s\n```";
        let m = parse(text);
        assert_eq!(m.cleaned_text, "Answer.");
        assert_eq!(m.suggestions, vec!["A?"]);
        assert_eq!(m.memory.as_deref(), Some("Profile: p\nStyle: s"));

        let loose = "Answer.\n\nSuggestions:\n[\"B?\", \"C?\"]\nMemory:\nProfile: q\nStyle: t";
        let m = parse(loose);
        assert_eq!(m.cleaned_text, "Answer.");
        assert_eq!(m.suggestions, vec!["B?", "C?"]);
        assert_eq!(m.memory.as_deref(), Some("Profile: q\nStyle: t"));
    }

    #[test]
    fn leaves_plain_answers_alone() {
        let m = parse("Just an answer with a [link](card:12).\n\n```rust\nlet x = 1;\n```");
        assert_eq!(m.cleaned_text, "Just an answer with a [link](card:12).\n\n```rust\nlet x = 1;\n```");
        assert!(m.suggestions.is_empty() && m.memory.is_none());
        // Malformed suggestions JSON: block removed, no suggestions.
        let m = parse("Hi\n```suggestions\nnot json\n```");
        assert_eq!(m.cleaned_text, "Hi");
        assert!(m.suggestions.is_empty());
        // Memory without Profile/Style lines is ignored.
        assert!(parse("Hi\n```memory\nrandom\n```").memory.is_none());
    }

    #[test]
    fn memory_normalization() {
        assert_eq!(normalize_memory("a\r\n\n\n\nb  "), "a\n\nb");
        assert_eq!(normalize_memory(&"x".repeat(MEMORY_MAX_CHARS + 50)).len(), MEMORY_MAX_CHARS);
        assert_eq!(normalize_auto_memory("style: s\nPROFILE: p"), Some("Profile: p\nStyle: s".into()));
    }

    #[test]
    fn recent_suggestions_dedupe_and_cap() {
        let mut h = vec!["A?".to_string()];
        push_recent(&mut h, &["a?".into(), "B?".into()]);
        assert_eq!(h, vec!["a?", "B?"]);
        let many: Vec<String> = (0..20).map(|i| format!("Q{i}")).collect();
        push_recent(&mut h, &many);
        assert_eq!(h.len(), 12);
        assert_eq!(h.last().unwrap(), "Q19");
    }
}
