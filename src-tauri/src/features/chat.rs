//! Chat with your work journal: questions about the timeline, answered by the configured model
//! with read-only tools over the stored cards, observations and screenshot metadata. Answers
//! link the cards they rely on. Ports Dayflow's dashboard chat (ChatService.swift and the Gemini
//! function-calling path, MIT, © Jerry Liu); see the submodules for what comes from where.

mod engine;
mod metadata;
mod prompt;
mod store;
mod tools;

use crate::config::{ProviderKind, Settings};
use crate::llm::chat_api::{ChatApi, Msg};
use crate::llm::client::{HttpError, LlmClient};
use crate::state::AppState;
use crate::timeutil::{fmt_clock, logical_day, now_ts};
use engine::{Cancelled, Progress, ToolRun};
use serde::Serialize;
use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use store::{MessageRow, ROLE_ASSISTANT, ROLE_ERROR, ROLE_USER};
use tauri::{Emitter, State};

/// Tables owned by this feature.
pub const SCHEMA: &str = store::SCHEMA;

/// Event emitted while a question is being answered.
pub const PROGRESS_EVENT: &str = "chat-progress";
/// The main window reloads the settings on this event (App.tsx), so a Settings form opened later
/// starts from the saved chat model instead of writing an old one back.
const SETTINGS_EVENT: &str = "settings-changed";

type S<'a> = State<'a, Arc<AppState>>;
type CmdResult<T> = Result<T, String>;

/// One question at a time (Dayflow's isProcessing guard).
static BUSY: AtomicBool = AtomicBool::new(false);
static CANCEL: AtomicBool = AtomicBool::new(false);
/// The conversation whose question is being answered. Set and checked under this lock, together
/// with deletes, so a conversation cannot disappear between the check and the stored answer.
static ACTIVE: Mutex<Option<i64>> = Mutex::new(None);

fn active() -> std::sync::MutexGuard<'static, Option<i64>> {
    ACTIVE.lock().unwrap_or_else(|e| e.into_inner())
}

struct BusyGuard;
impl Drop for BusyGuard {
    fn drop(&mut self) {
        *active() = None;
        BUSY.store(false, Ordering::SeqCst);
    }
}

pub const DELETE_WHILE_BUSY: &str = "This chat is still being answered. Stop the answer or wait for it, then delete the chat.";

/// Deletes a conversation unless its question is being answered (the answer would be lost).
fn delete_unless_active(db: &crate::db::Db, id: i64) -> Result<(), String> {
    let active = active();
    if *active == Some(id) {
        return Err(DELETE_WHILE_BUSY.into());
    }
    store::delete_conversation(db, id).map_err(err)
}

fn host_of(url: &str) -> String {
    reqwest::Url::parse(url.trim()).ok().and_then(|u| u.host_str().map(str::to_lowercase)).unwrap_or_else(|| url.trim().to_lowercase())
}

/// The chat-only model names a model on the provider and server it was chosen for. After a
/// switch to another provider or server it would not exist there (404), so it is cleared and
/// chat falls back to the timeline model.
pub fn clear_model_on_provider_change(old: &Settings, new: &mut Settings) {
    if old.provider != new.provider || host_of(&old.base_url) != host_of(&new.base_url) {
        new.chat_model.clear();
    }
}

fn err<E: std::fmt::Display>(e: E) -> String {
    format!("{e:#}")
}

// ---- which model answers ----

struct Target {
    kind: ProviderKind,
    base_url: String,
    model: String,
    key: Option<String>,
}

impl Target {
    fn from_settings(s: &Settings) -> Target {
        let model = if s.chat_model.trim().is_empty() { s.model.trim() } else { s.chat_model.trim() };
        Target { kind: s.provider, base_url: s.base_url.clone(), model: model.to_string(), key: crate::config::stored_key(s.provider, &s.base_url) }
    }

    fn provider_label(&self) -> String {
        match self.kind {
            ProviderKind::Anthropic => "Anthropic".into(),
            ProviderKind::Ollama => "Ollama".into(),
            ProviderKind::LmStudio => "LM Studio".into(),
            ProviderKind::OpenaiCompatible => reqwest::Url::parse(&self.base_url)
                .ok()
                .and_then(|u| u.host_str().map(String::from))
                .unwrap_or_else(|| "OpenAI-compatible server".into()),
            ProviderKind::ClaudeCli => "Claude Code CLI".into(),
            ProviderKind::CodexCli => "Codex CLI".into(),
        }
    }

    fn provider_id(&self) -> &'static str {
        match self.kind {
            ProviderKind::Anthropic => "anthropic",
            ProviderKind::Ollama => "ollama",
            ProviderKind::LmStudio => "lm_studio",
            ProviderKind::OpenaiCompatible => "openai_compatible",
            ProviderKind::ClaudeCli => "claude_cli",
            ProviderKind::CodexCli => "codex_cli",
        }
    }

    /// Why chat cannot run right now, in plain words.
    fn problem(&self) -> Option<String> {
        if self.model.is_empty() {
            return Some("No model is set. Choose one in Settings.".into());
        }
        if self.kind.is_cli() {
            return Some(format!("Chat does not work with the {} yet. Choose an API or local model provider in Settings to chat.", self.kind.label()));
        }
        if crate::llm::chat_api::key_required(self.kind, &self.base_url) && self.key.is_none() {
            return Some(format!("Chat needs an API key for {}. Add one in Settings.", self.provider_label()));
        }
        None
    }
}

/// Plain-words message and technical detail for a failed answer.
fn friendly_error(e: &anyhow::Error, t: &Target) -> (String, Option<String>) {
    let detail = Some(format!("{e:#}"));
    if e.downcast_ref::<Cancelled>().is_some() {
        return ("Stopped before the answer was finished.".into(), None);
    }
    if let Some(h) = e.downcast_ref::<HttpError>() {
        let body = h.body.to_lowercase();
        let msg = match h.status {
            401 | 403 => format!("{} rejected the API key. Check it in Settings.", t.provider_label()),
            404 => format!("The model \"{}\" or the server address was not found. Check the chat model and the server address.", t.model),
            429 => "The model provider is rate limiting requests. Wait a minute and try again.".into(),
            400 if body.contains("tool") => format!("The model \"{}\" does not support tool use, which chat needs. Choose a model that does, for example qwen3 or llama3.1 in Ollama.", t.model),
            400 if body.contains("context") || body.contains("too long") || body.contains("too large") => {
                "The conversation is too long for this model. Start a new chat or ask about a shorter period.".into()
            }
            529 | 503 => "The model provider is overloaded. Try again in a moment.".into(),
            s if s >= 500 => "The model provider had a problem answering. Try again in a moment.".into(),
            _ => "The model provider refused the request.".into(),
        };
        return (msg, detail);
    }
    if let Some(r) = e.downcast_ref::<reqwest::Error>() {
        if r.is_timeout() {
            return ("The model took too long to answer. Try a narrower question.".into(), detail);
        }
        let msg = match t.kind {
            ProviderKind::Ollama | ProviderKind::LmStudio => format!("Could not reach {} at {}. Is it running?", t.provider_label(), t.base_url),
            _ => format!("Could not reach {}. Check your internet connection.", t.provider_label()),
        };
        return (msg, detail);
    }
    // Errors raised by the chat loop itself are already written in plain words.
    (e.to_string(), None)
}

// ---- DTOs ----

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatInfoDto {
    provider: String,
    provider_label: String,
    /// The model chat uses.
    model: String,
    /// Chat-only model override; empty means the timeline model.
    chat_model: String,
    timeline_model: String,
    problem: Option<String>,
    busy: bool,
    /// The conversation being answered, once it has an id.
    busy_conversation_id: Option<i64>,
    welcome_prompts: Vec<&'static str>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationDto {
    id: i64,
    title: String,
    provider: Option<String>,
    model: Option<String>,
    created_at: i64,
    updated_at: i64,
    message_count: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CitationDto {
    /// The id the answer links to (`card:<id>`).
    cited_id: i64,
    /// The card to open now (a rewritten card resolves to its replacement); None when nothing
    /// on the timeline covers that time any more.
    card_id: Option<i64>,
    day: String,
    title: String,
    category: String,
    start: String,
    end: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageDto {
    id: i64,
    role: String,
    content: String,
    detail: Option<String>,
    tools: Vec<ToolRun>,
    /// Resolved citations, in order of first mention.
    citations: Vec<CitationDto>,
    suggestions: Vec<String>,
    provider: Option<String>,
    model: Option<String>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    created_at: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationDetailDto {
    conversation: ConversationDto,
    messages: Vec<MessageDto>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryDto {
    content: String,
    updated_at: Option<i64>,
    max_chars: usize,
}

fn conversation_dto(c: store::ConversationRow) -> ConversationDto {
    ConversationDto {
        id: c.id,
        title: c.title,
        provider: c.provider,
        model: c.model,
        created_at: c.created_at,
        updated_at: c.updated_at,
        message_count: c.message_count,
    }
}

fn message_dto(db: &crate::db::Db, m: MessageRow) -> MessageDto {
    let mut citations = Vec::new();
    for id in &m.citations {
        if let Ok(Some(c)) = store::resolve_citation(db, *id) {
            citations.push(CitationDto {
                cited_id: c.cited_id,
                card_id: c.card_id,
                day: c.day,
                title: c.title,
                category: c.category,
                start: fmt_clock(c.start_ts),
                end: fmt_clock(c.end_ts),
            });
        }
    }
    MessageDto {
        id: m.id,
        role: m.role,
        content: m.content,
        detail: m.detail,
        tools: m.tools,
        citations,
        suggestions: m.suggestions,
        provider: m.provider,
        model: m.model,
        input_tokens: m.input_tokens,
        output_tokens: m.output_tokens,
        created_at: m.created_at,
    }
}

fn detail(db: &crate::db::Db, id: i64) -> anyhow::Result<ConversationDetailDto> {
    let conv = store::conversation(db, id)?.ok_or_else(|| anyhow::anyhow!("This conversation no longer exists."))?;
    let messages = store::messages(db, id)?.into_iter().map(|m| message_dto(db, m)).collect();
    Ok(ConversationDetailDto { conversation: conversation_dto(conv), messages })
}

fn info(settings: &Settings) -> ChatInfoDto {
    let t = Target::from_settings(settings);
    ChatInfoDto {
        provider: t.provider_id().into(),
        provider_label: t.provider_label(),
        model: t.model.clone(),
        chat_model: settings.chat_model.trim().to_string(),
        timeline_model: settings.model.clone(),
        problem: t.problem(),
        busy: BUSY.load(Ordering::SeqCst),
        busy_conversation_id: *active(),
        welcome_prompts: prompt::WELCOME_PROMPTS.to_vec(),
    }
}

// ---- commands ----

#[tauri::command]
pub fn chat_info(state: S) -> ChatInfoDto {
    info(&state.settings())
}

/// Sets the chat-only model (empty uses the timeline model).
#[tauri::command]
pub fn chat_set_model(app: tauri::AppHandle, state: S, model: String) -> CmdResult<ChatInfoDto> {
    let s = crate::features::platform_extras::update_settings(&state, |g| g.chat_model = model.trim().to_string()).map_err(err)?;
    let _ = app.emit(SETTINGS_EVENT, ());
    Ok(info(&s))
}

#[tauri::command]
pub fn chat_list_conversations(state: S) -> CmdResult<Vec<ConversationDto>> {
    Ok(store::list_conversations(&state.db).map_err(err)?.into_iter().map(conversation_dto).collect())
}

#[tauri::command]
pub fn chat_get_conversation(state: S, id: i64) -> CmdResult<ConversationDetailDto> {
    detail(&state.db, id).map_err(err)
}

#[tauri::command]
pub fn chat_delete_conversation(state: S, id: i64) -> CmdResult<()> {
    delete_unless_active(&state.db, id)
}

/// Stops the question being answered; the loop checks between and during requests.
#[tauri::command]
pub fn chat_cancel() {
    if BUSY.load(Ordering::SeqCst) {
        CANCEL.store(true, Ordering::SeqCst);
    }
}

#[tauri::command]
pub fn chat_get_memory(state: S) -> CmdResult<MemoryDto> {
    let (content, updated_at) = store::load_memory(&state.db).map_err(err)?;
    Ok(MemoryDto { content, updated_at, max_chars: metadata::MEMORY_MAX_CHARS })
}

#[tauri::command]
pub fn chat_save_memory(state: S, content: String) -> CmdResult<MemoryDto> {
    store::save_memory(&state.db, &content, now_ts()).map_err(err)?;
    chat_get_memory(state)
}

/// Opens an http(s) link from an answer in the default browser.
#[tauri::command]
pub fn chat_open_link(app: tauri::AppHandle, url: String) -> CmdResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let parsed = reqwest::Url::parse(url.trim()).map_err(|_| "That link is not a valid web address.".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("Only web links can be opened.".into());
    }
    app.opener().open_url(parsed.as_str(), None::<&str>).map_err(err)
}

/// Earlier turns for the model: user and assistant text only (Dayflow drops tool calls from the
/// history; the model fetches again when it needs data).
fn history_from(prior: &[MessageRow]) -> Vec<Msg> {
    prior
        .iter()
        .filter(|m| !m.content.trim().is_empty())
        .filter_map(|m| match m.role.as_str() {
            ROLE_USER => Some(Msg::User(m.content.clone())),
            ROLE_ASSISTANT => Some(Msg::Assistant { text: m.content.clone(), calls: vec![], raw: None }),
            _ => None,
        })
        .collect()
}

/// Asks a question in a conversation (a new one when `conversation_id` is None) and returns
/// the whole conversation with the answer, or with a plain-words error message in its place.
#[tauri::command]
pub async fn chat_send(app: tauri::AppHandle, state: S<'_>, conversation_id: Option<i64>, text: String) -> CmdResult<ConversationDetailDto> {
    let question = text.trim().to_string();
    if question.is_empty() {
        return Err("Type a question first.".into());
    }
    let settings = state.settings();
    let target = Target::from_settings(&settings);
    if let Some(p) = target.problem() {
        return Err(p);
    }
    if BUSY.swap(true, Ordering::SeqCst) {
        return Err("Chat is still answering the previous question.".into());
    }
    let _busy = BusyGuard;
    CANCEL.store(false, Ordering::SeqCst);

    let db = state.db.clone();
    let provider = target.provider_id();
    let conv_id = {
        let mut active = active();
        let id = match conversation_id {
            Some(id) if store::conversation_exists(&db, id).map_err(err)? => id,
            Some(_) => return Err("This conversation no longer exists.".into()),
            None => store::create_conversation(&db, &store::title_from(&question), provider, &target.model, now_ts()).map_err(err)?,
        };
        *active = Some(id);
        id
    };
    let prior = store::messages(&db, conv_id).map_err(err)?;
    store::insert_message(&db, conv_id, &MessageRow { role: ROLE_USER.into(), content: question.clone(), created_at: now_ts(), ..Default::default() })
        .map_err(err)?;

    let mut recent = Vec::new();
    for m in prior.iter().filter(|m| m.role == ROLE_ASSISTANT) {
        metadata::push_recent(&mut recent, &m.suggestions);
    }
    let (memory, _) = store::load_memory(&db).map_err(err)?;
    let system = prompt::system_prompt(chrono::Local::now(), &logical_day(now_ts()), &settings.output_language, &memory, &recent);

    let mut client = LlmClient::new(target.kind, &target.base_url, &target.model, target.key.clone(), Some(db.clone())).map_err(err)?;
    client.reasoning = settings.reasoning;
    client.use_batch = false; // interactive: never the Batch API
    let api = ChatApi::new(client);
    let ctx = tools::ToolContext {
        db: db.clone(),
        capture_interval_secs: settings.capture_interval_secs,
        idle_category: settings.categories.iter().find(|c| c.is_idle).map(|c| c.name.clone()).unwrap_or_else(|| "Idle".into()),
    };
    let mut emit = |p: Progress| {
        // {"conversationId", "stage": "thinking" | "toolStarted" | "toolFinished", ...}
        let mut v = serde_json::to_value(&p).unwrap_or_default();
        v["conversationId"] = serde_json::json!(conv_id);
        let _ = app.emit(PROGRESS_EVENT, v);
    };
    let result = engine::run(&api, &ctx, &system, history_from(&prior), &question, &CANCEL, &mut emit).await;

    let row = match result {
        Ok(out) => {
            let meta = metadata::parse(&out.text);
            if let Some(mem) = &meta.memory
                && let Err(e) = store::save_memory(&db, mem, now_ts())
            {
                log::warn!("chat: could not save memory: {e:#}");
            }
            let mut allowed: HashSet<i64> = out.seen_card_ids.clone();
            allowed.extend(prior.iter().flat_map(|m| m.citations.iter().copied()));
            let (answer, cited) = engine::sanitize_citations(&meta.cleaned_text, &allowed, |id| db.card(id).ok().flatten().is_some());
            let (input_tokens, output_tokens) = (Some(out.input_tokens), Some(out.output_tokens));
            if answer.trim().is_empty() {
                MessageRow {
                    role: ROLE_ERROR.into(),
                    content: "The model returned an empty answer.".into(),
                    tools: out.tool_runs,
                    provider: Some(provider.into()),
                    model: Some(target.model.clone()),
                    input_tokens,
                    output_tokens,
                    created_at: now_ts(),
                    ..Default::default()
                }
            } else {
                MessageRow {
                    role: ROLE_ASSISTANT.into(),
                    content: answer,
                    tools: out.tool_runs,
                    citations: cited,
                    suggestions: meta.suggestions,
                    provider: Some(provider.into()),
                    model: Some(target.model.clone()),
                    input_tokens,
                    output_tokens,
                    created_at: now_ts(),
                    ..Default::default()
                }
            }
        }
        Err(e) => {
            log::warn!("chat question failed: {e:#}");
            let (content, detail) = friendly_error(&e, &target);
            MessageRow {
                role: ROLE_ERROR.into(),
                content,
                detail,
                provider: Some(provider.into()),
                model: Some(target.model.clone()),
                created_at: now_ts(),
                ..Default::default()
            }
        }
    };
    store::insert_message(&db, conv_id, &row).map_err(err)?;
    store::touch_conversation(&db, conv_id, provider, &target.model, now_ts()).map_err(err)?;
    detail(&db, conv_id).map_err(err)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(kind: ProviderKind, url: &str) -> Settings {
        Settings { provider: kind, base_url: url.into(), model: "m".into(), ..Default::default() }
    }

    #[test]
    fn target_uses_chat_model_and_reports_missing_key() {
        unsafe { std::env::remove_var("DAYTRACE_API_KEY") }; // test-only
        let mut s = settings(ProviderKind::Ollama, "http://localhost:11434");
        assert!(Target::from_settings(&s).problem().is_none());
        s.chat_model = " qwen3:8b ".into();
        assert_eq!(Target::from_settings(&s).model, "qwen3:8b");
        let a = Target { kind: ProviderKind::Anthropic, base_url: "https://api.anthropic.com".into(), model: "m".into(), key: None };
        assert_eq!(a.problem().unwrap(), "Chat needs an API key for Anthropic. Add one in Settings.");
        let custom = Target { kind: ProviderKind::OpenaiCompatible, base_url: "http://10.0.0.2:8000/v1".into(), model: "m".into(), key: None };
        assert!(custom.problem().is_none(), "self-hosted servers may not need a key");
    }

    #[test]
    fn errors_in_plain_words() {
        let t = Target { kind: ProviderKind::Ollama, base_url: "http://localhost:11434".into(), model: "gemma3:4b".into(), key: None };
        let e: anyhow::Error = HttpError { status: 400, body: "{\"error\":\"gemma3:4b does not support tools\"}".into() }.into();
        assert!(friendly_error(&e, &t).0.contains("does not support tool use"));
        let e: anyhow::Error = HttpError { status: 401, body: String::new() }.into();
        assert_eq!(friendly_error(&e, &t).0, "Ollama rejected the API key. Check it in Settings.");
        let e: anyhow::Error = Cancelled.into();
        assert_eq!(friendly_error(&e, &t), ("Stopped before the answer was finished.".into(), None));
        let e = anyhow::anyhow!(engine::ROUNDS_EXCEEDED);
        assert_eq!(friendly_error(&e, &t).0, engine::ROUNDS_EXCEEDED);
    }

    #[test]
    fn provider_change_clears_the_chat_model() {
        let old = Settings { chat_model: "qwen3:8b".into(), ..settings(ProviderKind::Ollama, "http://localhost:11434") };
        let mut same = old.clone();
        same.model = "other".into();
        clear_model_on_provider_change(&old, &mut same);
        assert_eq!(same.chat_model, "qwen3:8b", "kept when only the timeline model changes");
        let mut slash = Settings { base_url: "http://localhost:11434/".into(), ..old.clone() };
        clear_model_on_provider_change(&old, &mut slash);
        assert_eq!(slash.chat_model, "qwen3:8b");
        let mut kind = Settings { provider: ProviderKind::Anthropic, ..old.clone() };
        clear_model_on_provider_change(&old, &mut kind);
        assert_eq!(kind.chat_model, "");
        let a = Settings { chat_model: "gpt-5".into(), ..settings(ProviderKind::OpenaiCompatible, "https://api.openai.com/v1") };
        let mut b = Settings { base_url: "https://openrouter.ai/api/v1".into(), ..a.clone() };
        clear_model_on_provider_change(&a, &mut b);
        assert_eq!(b.chat_model, "", "another server has other model names");
    }

    #[test]
    fn busy_conversation_cannot_be_deleted() {
        let db = crate::db::Db::memory();
        let busy = store::create_conversation(&db, "q", "ollama", "m", 1).unwrap();
        let other = store::create_conversation(&db, "q2", "ollama", "m", 1).unwrap();
        *active() = Some(busy);
        assert_eq!(delete_unless_active(&db, busy).unwrap_err(), DELETE_WHILE_BUSY);
        delete_unless_active(&db, other).unwrap();
        drop(BusyGuard); // what chat_send does when the answer is stored
        assert_eq!(*active(), None);
        // The answer can still be stored after a refused delete.
        store::insert_message(&db, busy, &MessageRow { role: ROLE_ASSISTANT.into(), content: "a".into(), ..Default::default() }).unwrap();
        delete_unless_active(&db, busy).unwrap();
        assert!(store::list_conversations(&db).unwrap().is_empty());
    }

    #[test]
    fn history_keeps_text_turns_only() {
        let rows = vec![
            MessageRow { role: ROLE_USER.into(), content: "q1".into(), ..Default::default() },
            MessageRow { role: ROLE_ERROR.into(), content: "Could not reach Ollama".into(), ..Default::default() },
            MessageRow { role: ROLE_USER.into(), content: "q2".into(), ..Default::default() },
            MessageRow { role: ROLE_ASSISTANT.into(), content: "a2".into(), ..Default::default() },
        ];
        let h = history_from(&rows);
        assert_eq!(h.len(), 3);
        assert!(matches!(&h[2], Msg::Assistant { text, .. } if text == "a2"));
    }
}
