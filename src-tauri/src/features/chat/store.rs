//! Chat persistence (Dayflow's StorageManager+ChatHistory.swift and DashboardChatMemoryStore):
//! conversations, their messages, and the single memory blob. Messages are appended instead of
//! Dayflow's replace-all save, since Daytrace writes them one at a time.

use super::engine::ToolRun;
use super::metadata::normalize_memory;
use crate::db::Db;
use anyhow::Result;
use rusqlite::{params, OptionalExtension};

pub const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS chat_conversations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    title TEXT NOT NULL,
    provider TEXT,
    model TEXT,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_chat_conversations_updated ON chat_conversations(updated_at);

CREATE TABLE IF NOT EXISTS chat_messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    conversation_id INTEGER NOT NULL REFERENCES chat_conversations(id) ON DELETE CASCADE,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    detail TEXT,
    tools TEXT,
    citations TEXT,
    suggestions TEXT,
    provider TEXT,
    model TEXT,
    input_tokens INTEGER,
    output_tokens INTEGER,
    created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_chat_messages_conversation ON chat_messages(conversation_id, id);

CREATE TABLE IF NOT EXISTS chat_memory (
    id INTEGER PRIMARY KEY CHECK (id = 1),
    content TEXT NOT NULL,
    updated_at INTEGER NOT NULL
);
"#;

pub const ROLE_USER: &str = "user";
pub const ROLE_ASSISTANT: &str = "assistant";
/// A failed answer, shown in plain words and never sent back to the model.
pub const ROLE_ERROR: &str = "error";

#[derive(Debug, Clone)]
pub struct ConversationRow {
    pub id: i64,
    pub title: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub message_count: i64,
}

#[derive(Debug, Clone, Default)]
pub struct MessageRow {
    pub id: i64,
    pub role: String,
    pub content: String,
    pub detail: Option<String>,
    pub tools: Vec<ToolRun>,
    pub citations: Vec<i64>,
    pub suggestions: Vec<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub created_at: i64,
}

/// Dayflow's conversation title: the first question on one line, at most 80 characters.
pub fn title_from(question: &str) -> String {
    let one_line = question.split_whitespace().collect::<Vec<_>>().join(" ");
    let t: String = one_line.chars().take(80).collect();
    if t.is_empty() { "New chat".into() } else { t }
}

pub fn create_conversation(db: &Db, title: &str, provider: &str, model: &str, now: i64) -> Result<i64> {
    let c = db.conn();
    c.execute(
        "INSERT INTO chat_conversations (title, provider, model, created_at, updated_at) VALUES (?, ?, ?, ?, ?)",
        params![title, provider, model, now, now],
    )?;
    Ok(c.last_insert_rowid())
}

pub fn conversation_exists(db: &Db, id: i64) -> Result<bool> {
    Ok(db.conn().query_row("SELECT 1 FROM chat_conversations WHERE id = ?", [id], |_| Ok(())).optional()?.is_some())
}

pub fn touch_conversation(db: &Db, id: i64, provider: &str, model: &str, now: i64) -> Result<()> {
    db.conn().execute("UPDATE chat_conversations SET updated_at = ?, provider = ?, model = ? WHERE id = ?", params![now, provider, model, id])?;
    Ok(())
}

pub fn list_conversations(db: &Db) -> Result<Vec<ConversationRow>> {
    let c = db.conn();
    let mut st = c.prepare(
        "SELECT c.id, c.title, c.provider, c.model, c.created_at, c.updated_at,
                (SELECT COUNT(*) FROM chat_messages m WHERE m.conversation_id = c.id)
         FROM chat_conversations c ORDER BY c.updated_at DESC, c.id DESC",
    )?;
    let rows = st
        .query_map([], |r| {
            Ok(ConversationRow {
                id: r.get(0)?,
                title: r.get(1)?,
                provider: r.get(2)?,
                model: r.get(3)?,
                created_at: r.get(4)?,
                updated_at: r.get(5)?,
                message_count: r.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn conversation(db: &Db, id: i64) -> Result<Option<ConversationRow>> {
    Ok(list_conversations(db)?.into_iter().find(|c| c.id == id))
}

pub fn delete_conversation(db: &Db, id: i64) -> Result<()> {
    let mut c = db.conn();
    let tx = c.transaction()?;
    // Explicit, in case foreign keys are off (as Dayflow does).
    tx.execute("DELETE FROM chat_messages WHERE conversation_id = ?", [id])?;
    tx.execute("DELETE FROM chat_conversations WHERE id = ?", [id])?;
    tx.commit()?;
    Ok(())
}

pub fn insert_message(db: &Db, conversation_id: i64, m: &MessageRow) -> Result<i64> {
    let json_or_null = |empty: bool, v: String| if empty { None } else { Some(v) };
    let c = db.conn();
    c.execute(
        "INSERT INTO chat_messages (conversation_id, role, content, detail, tools, citations, suggestions, provider, model, input_tokens, output_tokens, created_at)
         VALUES (?,?,?,?,?,?,?,?,?,?,?,?)",
        params![
            conversation_id,
            m.role,
            m.content,
            m.detail,
            json_or_null(m.tools.is_empty(), serde_json::to_string(&m.tools)?),
            json_or_null(m.citations.is_empty(), serde_json::to_string(&m.citations)?),
            json_or_null(m.suggestions.is_empty(), serde_json::to_string(&m.suggestions)?),
            m.provider,
            m.model,
            m.input_tokens,
            m.output_tokens,
            m.created_at,
        ],
    )?;
    Ok(c.last_insert_rowid())
}

fn parse_json<T: serde::de::DeserializeOwned + Default>(s: Option<String>) -> T {
    s.and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

pub fn messages(db: &Db, conversation_id: i64) -> Result<Vec<MessageRow>> {
    let c = db.conn();
    let mut st = c.prepare(
        "SELECT id, role, content, detail, tools, citations, suggestions, provider, model, input_tokens, output_tokens, created_at
         FROM chat_messages WHERE conversation_id = ? ORDER BY id",
    )?;
    let rows = st
        .query_map([conversation_id], |r| {
            Ok(MessageRow {
                id: r.get(0)?,
                role: r.get(1)?,
                content: r.get(2)?,
                detail: r.get(3)?,
                tools: parse_json(r.get(4)?),
                citations: parse_json(r.get(5)?),
                suggestions: parse_json(r.get(6)?),
                provider: r.get(7)?,
                model: r.get(8)?,
                input_tokens: r.get(9)?,
                output_tokens: r.get(10)?,
                created_at: r.get(11)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// The memory blob and when it was last saved.
pub fn load_memory(db: &Db) -> Result<(String, Option<i64>)> {
    let row: Option<(String, i64)> =
        db.conn().query_row("SELECT content, updated_at FROM chat_memory WHERE id = 1", [], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
    Ok(match row {
        Some((c, at)) => (c.trim().to_string(), Some(at)),
        None => (String::new(), None),
    })
}

/// Saves the normalized blob; an empty blob clears memory. Returns what was stored.
pub fn save_memory(db: &Db, text: &str, now: i64) -> Result<String> {
    let normalized = normalize_memory(text);
    let c = db.conn();
    if normalized.is_empty() {
        c.execute("DELETE FROM chat_memory WHERE id = 1", [])?;
    } else {
        c.execute(
            "INSERT INTO chat_memory (id, content, updated_at) VALUES (1, ?, ?) ON CONFLICT(id) DO UPDATE SET content = excluded.content, updated_at = excluded.updated_at",
            params![normalized, now],
        )?;
    }
    Ok(normalized)
}

/// A cited card as shown under an answer. Cards are rewritten by later analysis (soft-deleted
/// and replaced); a replaced card resolves to the live card that overlaps it most.
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Citation {
    /// The card to open; None when nothing on the timeline covers that time any more.
    pub card_id: Option<i64>,
    /// The id the answer linked to.
    pub cited_id: i64,
    pub day: String,
    pub title: String,
    pub category: String,
    pub start_ts: i64,
    pub end_ts: i64,
}

pub fn resolve_citation(db: &Db, id: i64) -> Result<Option<Citation>> {
    let row: Option<(String, String, String, i64, i64, i64)> = db
        .conn()
        .query_row("SELECT day, title, category, start_ts, end_ts, is_deleted FROM timeline_cards WHERE id = ?", [id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?))
        })
        .optional()?;
    let Some((day, title, category, start_ts, end_ts, deleted)) = row else { return Ok(None) };
    if deleted == 0 {
        return Ok(Some(Citation { card_id: Some(id), cited_id: id, day, title, category, start_ts, end_ts }));
    }
    let best = db
        .cards_in_range(start_ts, end_ts.max(start_ts + 1), true)?
        .into_iter()
        .max_by_key(|c| c.end_ts.min(end_ts) - c.start_ts.max(start_ts));
    Ok(Some(match best {
        Some(c) => Citation { card_id: Some(c.id), cited_id: id, day: c.day, title: c.title, category: c.category, start_ts: c.start_ts, end_ts: c.end_ts },
        None => Citation { card_id: None, cited_id: id, day, title, category, start_ts, end_ts },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::chat::tools::tests::{ctx, ts};

    #[test]
    fn conversation_round_trip() {
        let db = Db::memory();
        let id = create_conversation(&db, &title_from("  What did I\n do yesterday?  "), "anthropic", "claude-haiku-5-5", 100).unwrap();
        assert!(conversation_exists(&db, id).unwrap());
        insert_message(&db, id, &MessageRow { role: ROLE_USER.into(), content: "q".into(), created_at: 100, ..Default::default() }).unwrap();
        let run = ToolRun { name: "fetchTimeline".into(), label: "Fetching timeline".into(), input: serde_json::json!({}), summary: "s".into(), ok: true };
        insert_message(
            &db,
            id,
            &MessageRow {
                role: ROLE_ASSISTANT.into(),
                content: "a".into(),
                tools: vec![run.clone()],
                citations: vec![3, 4],
                suggestions: vec!["Next?".into()],
                input_tokens: Some(10),
                created_at: 101,
                ..Default::default()
            },
        )
        .unwrap();
        touch_conversation(&db, id, "ollama", "qwen3", 102).unwrap();
        let list = list_conversations(&db).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].title, "What did I do yesterday?");
        assert_eq!((list[0].message_count, list[0].updated_at, list[0].model.as_deref()), (2, 102, Some("qwen3")));
        let msgs = messages(&db, id).unwrap();
        assert_eq!(msgs[1].tools, vec![run]);
        assert_eq!(msgs[1].citations, vec![3, 4]);
        assert_eq!(msgs[1].suggestions, vec!["Next?"]);
        assert!(msgs[0].tools.is_empty());
        delete_conversation(&db, id).unwrap();
        assert!(list_conversations(&db).unwrap().is_empty());
        assert!(messages(&db, id).unwrap().is_empty());
        assert_eq!(title_from(&"x".repeat(200)).len(), 80);
        assert_eq!(title_from(" "), "New chat");
    }

    #[test]
    fn memory_save_load_clear() {
        let db = Db::memory();
        assert_eq!(load_memory(&db).unwrap(), (String::new(), None));
        assert_eq!(save_memory(&db, " Profile: a\r\nStyle: b ", 5).unwrap(), "Profile: a\nStyle: b");
        assert_eq!(load_memory(&db).unwrap(), ("Profile: a\nStyle: b".into(), Some(5)));
        save_memory(&db, "  ", 6).unwrap();
        assert_eq!(load_memory(&db).unwrap().0, "");
    }

    #[test]
    fn citations_follow_rewritten_cards() {
        let c = ctx();
        let first = c.db.cards_for_day("2026-10-16").unwrap()[0].clone();
        let live = resolve_citation(&c.db, first.id).unwrap().unwrap();
        assert_eq!(live.card_id, Some(first.id));
        // A later analysis replaces the 9:00 card with a new one covering 9:00-10:00.
        let shots = c.db.screenshots_in_range(0, i64::MAX).unwrap();
        let b = c.db.create_batch(&shots[..1]).unwrap();
        let mut repl = first.to_activity();
        repl.title = "Rewritten".into();
        let rc = crate::db::ResolvedCard { start_ts: ts(16, 9, 0), end_ts: ts(16, 10, 0), card: repl, idle_meta: None };
        c.db.replace_cards_in_range(ts(16, 9, 0), ts(16, 10, 0), &[rc], b, true).unwrap();
        let moved = resolve_citation(&c.db, first.id).unwrap().unwrap();
        assert_ne!(moved.card_id, Some(first.id));
        assert_eq!(moved.title, "Rewritten");
        assert_eq!(moved.cited_id, first.id);
        assert!(resolve_citation(&c.db, 424242).unwrap().is_none());
    }
}
