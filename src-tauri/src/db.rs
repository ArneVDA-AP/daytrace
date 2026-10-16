//! SQLite storage. The schema follows Dayflow's (timeline_cards, observations, analysis_batches,
//! screenshots, batch_screenshots) so the semantics carry over; screenshots are one JPEG per row.

use crate::timeutil;
use crate::types::{ActivityCard, AppSites, Distraction};
use anyhow::Result;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Serialize;
use std::path::Path;
use std::sync::{Arc, Mutex};

const SCHEMA: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS screenshots (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    captured_at INTEGER NOT NULL,
    file_path TEXT NOT NULL,
    file_size INTEGER,
    idle_seconds_at_capture INTEGER,
    app_name TEXT,
    window_title TEXT,
    hidden_app TEXT,
    is_deleted INTEGER NOT NULL DEFAULT 0,
    created_at DATETIME DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_screenshots_captured ON screenshots(captured_at);

CREATE TABLE IF NOT EXISTS analysis_batches (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    batch_start_ts INTEGER NOT NULL,
    batch_end_ts INTEGER NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    reason TEXT,
    attempts INTEGER NOT NULL DEFAULT 0,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_batches_status ON analysis_batches(status, batch_start_ts);

CREATE TABLE IF NOT EXISTS batch_screenshots (
    batch_id INTEGER NOT NULL REFERENCES analysis_batches(id) ON DELETE CASCADE,
    screenshot_id INTEGER NOT NULL REFERENCES screenshots(id) ON DELETE RESTRICT,
    PRIMARY KEY (batch_id, screenshot_id)
);
CREATE INDEX IF NOT EXISTS idx_batch_screenshots_shot ON batch_screenshots(screenshot_id);

CREATE TABLE IF NOT EXISTS observations (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    batch_id INTEGER NOT NULL REFERENCES analysis_batches(id) ON DELETE CASCADE,
    start_ts INTEGER NOT NULL,
    end_ts INTEGER NOT NULL,
    observation TEXT NOT NULL,
    llm_model TEXT,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_observations_range ON observations(start_ts, end_ts);

CREATE TABLE IF NOT EXISTS timeline_cards (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    batch_id INTEGER REFERENCES analysis_batches(id) ON DELETE CASCADE,
    start TEXT NOT NULL,
    end TEXT NOT NULL,
    start_ts INTEGER NOT NULL,
    end_ts INTEGER NOT NULL,
    day DATE NOT NULL,
    title TEXT NOT NULL,
    summary TEXT,
    category TEXT NOT NULL,
    subcategory TEXT,
    detailed_summary TEXT,
    metadata TEXT,
    is_deleted INTEGER NOT NULL DEFAULT 0,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_cards_live_range ON timeline_cards(start_ts, end_ts) WHERE is_deleted = 0;
CREATE INDEX IF NOT EXISTS idx_cards_day ON timeline_cards(day) WHERE is_deleted = 0;

CREATE TABLE IF NOT EXISTS llm_calls (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    batch_id INTEGER,
    operation TEXT NOT NULL,
    model TEXT,
    latency_ms INTEGER,
    request TEXT,
    response TEXT,
    error TEXT,
    input_tokens INTEGER,
    output_tokens INTEGER,
    created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
);
"#;

pub const SYSTEM_CATEGORY: &str = "System";
pub const ERROR_TITLE: &str = "Processing failed";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Shot {
    pub id: i64,
    pub captured_at: i64,
    pub file_path: String,
    pub idle_seconds: i64,
    pub app_name: Option<String>,
    pub window_title: Option<String>,
    pub hidden_app: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Batch {
    pub id: i64,
    pub start_ts: i64,
    pub end_ts: i64,
    pub status: String,
    pub reason: Option<String>,
    pub attempts: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredCard {
    pub id: i64,
    pub batch_id: Option<i64>,
    pub start: String,
    pub end: String,
    pub start_ts: i64,
    pub end_ts: i64,
    pub day: String,
    pub title: String,
    pub summary: String,
    pub category: String,
    pub subcategory: String,
    pub detailed_summary: String,
    pub distractions: Vec<Distraction>,
    pub app_sites: Option<AppSites>,
    pub is_idle_shortcut: bool,
}

impl StoredCard {
    pub fn to_activity(&self) -> ActivityCard {
        ActivityCard {
            start_time: self.start.clone(),
            end_time: self.end.clone(),
            category: self.category.clone(),
            subcategory: self.subcategory.clone(),
            title: self.title.clone(),
            summary: self.summary.clone(),
            detailed_summary: self.detailed_summary.clone(),
            distractions: if self.distractions.is_empty() { None } else { Some(self.distractions.clone()) },
            app_sites: self.app_sites.clone(),
        }
    }
}

/// A card with resolved timestamps, ready to insert.
#[derive(Debug, Clone)]
pub struct ResolvedCard {
    pub start_ts: i64,
    pub end_ts: i64,
    pub card: ActivityCard,
    pub idle_meta: Option<serde_json::Value>,
}

impl ResolvedCard {
    /// Resolve the LLM's clock strings around `anchor_ts`; None if unparsable.
    pub fn from_clock(card: ActivityCard, anchor_ts: i64) -> Option<ResolvedCard> {
        let (s, e) = timeutil::resolve_card_range(&card.start_time, &card.end_time, anchor_ts)?;
        Some(ResolvedCard { start_ts: s, end_ts: e, card, idle_meta: None })
    }
}

#[derive(Clone)]
pub struct Db {
    conn: Arc<Mutex<Connection>>,
}

const CARD_COLS: &str = "id, batch_id, start, end, start_ts, end_ts, day, title, summary, category, subcategory, detailed_summary, metadata";

fn card_from_row(r: &rusqlite::Row) -> rusqlite::Result<StoredCard> {
    let meta: Option<String> = r.get(12)?;
    let meta: serde_json::Value = meta.and_then(|m| serde_json::from_str(&m).ok()).unwrap_or_default();
    let distractions = meta
        .get("distractions")
        .and_then(|d| serde_json::from_value::<Vec<Distraction>>(d.clone()).ok())
        .unwrap_or_default();
    let app_sites = meta.get("appSites").and_then(crate::types::app_sites_from_value);
    Ok(StoredCard {
        id: r.get(0)?,
        batch_id: r.get(1)?,
        start: r.get(2)?,
        end: r.get(3)?,
        start_ts: r.get(4)?,
        end_ts: r.get(5)?,
        day: r.get(6)?,
        title: r.get(7)?,
        summary: r.get::<_, Option<String>>(8)?.unwrap_or_default(),
        category: r.get(9)?,
        subcategory: r.get::<_, Option<String>>(10)?.unwrap_or_default(),
        detailed_summary: r.get::<_, Option<String>>(11)?.unwrap_or_default(),
        distractions,
        app_sites,
        is_idle_shortcut: meta.get("idle").is_some(),
    })
}

fn shot_from_row(r: &rusqlite::Row) -> rusqlite::Result<Shot> {
    Ok(Shot {
        id: r.get(0)?,
        captured_at: r.get(1)?,
        file_path: r.get(2)?,
        idle_seconds: r.get::<_, Option<i64>>(3)?.unwrap_or(0),
        app_name: r.get(4)?,
        window_title: r.get(5)?,
        hidden_app: r.get(6)?,
    })
}

const SHOT_COLS: &str = "s.id, s.captured_at, s.file_path, s.idle_seconds_at_capture, s.app_name, s.window_title, s.hidden_app";

fn batch_from_row(r: &rusqlite::Row) -> rusqlite::Result<Batch> {
    Ok(Batch {
        id: r.get(0)?,
        start_ts: r.get(1)?,
        end_ts: r.get(2)?,
        status: r.get(3)?,
        reason: r.get(4)?,
        attempts: r.get(5)?,
    })
}
const BATCH_COLS: &str = "id, batch_start_ts, batch_end_ts, status, reason, attempts";

impl Db {
    pub fn open(path: &Path) -> Result<Db> {
        let conn = Connection::open(path)?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(SCHEMA)?;
        Ok(Db { conn: Arc::new(Mutex::new(conn)) })
    }

    #[cfg(test)]
    pub fn memory() -> Db {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        Db { conn: Arc::new(Mutex::new(conn)) }
    }

    fn c(&self) -> std::sync::MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|e| e.into_inner())
    }

    // ---- screenshots ----

    pub fn insert_screenshot(
        &self,
        captured_at: i64,
        file_path: &str,
        file_size: i64,
        idle: i64,
        app: Option<&str>,
        title: Option<&str>,
        hidden_app: Option<&str>,
    ) -> Result<i64> {
        let c = self.c();
        c.execute(
            "INSERT INTO screenshots (captured_at, file_path, file_size, idle_seconds_at_capture, app_name, window_title, hidden_app) VALUES (?,?,?,?,?,?,?)",
            params![captured_at, file_path, file_size, idle, app, title, hidden_app],
        )?;
        Ok(c.last_insert_rowid())
    }

    pub fn unbatched_screenshots(&self, since: i64) -> Result<Vec<Shot>> {
        let c = self.c();
        let mut st = c.prepare(&format!(
            "SELECT {SHOT_COLS} FROM screenshots s WHERE s.captured_at >= ? AND s.is_deleted = 0
             AND NOT EXISTS (SELECT 1 FROM batch_screenshots b WHERE b.screenshot_id = s.id)
             ORDER BY s.captured_at ASC"
        ))?;
        let rows = st.query_map([since], shot_from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn screenshots_in_range(&self, from: i64, to: i64) -> Result<Vec<Shot>> {
        let c = self.c();
        let mut st = c.prepare(&format!(
            "SELECT {SHOT_COLS} FROM screenshots s WHERE s.captured_at >= ? AND s.captured_at < ? AND s.is_deleted = 0 ORDER BY s.captured_at"
        ))?;
        let rows = st.query_map([from, to], shot_from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn screenshot(&self, id: i64) -> Result<Option<Shot>> {
        let c = self.c();
        Ok(c
            .query_row(&format!("SELECT {SHOT_COLS} FROM screenshots s WHERE s.id = ? AND s.is_deleted = 0"), [id], shot_from_row)
            .optional()?)
    }

    pub fn last_capture_ts(&self) -> Result<Option<i64>> {
        Ok(self.c().query_row("SELECT MAX(captured_at) FROM screenshots", [], |r| r.get(0))?)
    }

    /// Soft-deletes screenshots older than `before` and returns their file paths for removal.
    pub fn expire_screenshots(&self, before: i64) -> Result<Vec<String>> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let paths = {
            let mut st = tx.prepare("SELECT file_path FROM screenshots WHERE captured_at < ? AND is_deleted = 0")?;
            st.query_map([before], |r| r.get::<_, String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?
        };
        tx.execute("UPDATE screenshots SET is_deleted = 1 WHERE captured_at < ? AND is_deleted = 0", [before])?;
        tx.commit()?;
        Ok(paths)
    }

    /// Removes screen transcriptions and model call logs older than `before`; cards stay.
    pub fn expire_text(&self, before: i64) -> Result<()> {
        let c = self.c();
        c.execute("DELETE FROM observations WHERE end_ts < ?", [before])?;
        c.execute("DELETE FROM llm_calls WHERE created_at < datetime(?, 'unixepoch')", [before])?;
        Ok(())
    }

    // ---- batches ----

    pub fn create_batch(&self, shots: &[Shot]) -> Result<i64> {
        let (first, last) = (shots.first().unwrap().captured_at, shots.last().unwrap().captured_at);
        let mut c = self.c();
        let tx = c.transaction()?;
        tx.execute("INSERT INTO analysis_batches (batch_start_ts, batch_end_ts) VALUES (?, ?)", [first, last])?;
        let id = tx.last_insert_rowid();
        {
            let mut st = tx.prepare("INSERT INTO batch_screenshots (batch_id, screenshot_id) VALUES (?, ?)")?;
            for s in shots {
                st.execute([id, s.id])?;
            }
        }
        tx.commit()?;
        Ok(id)
    }

    pub fn batch(&self, id: i64) -> Result<Option<Batch>> {
        Ok(self
            .c()
            .query_row(&format!("SELECT {BATCH_COLS} FROM analysis_batches WHERE id = ?"), [id], batch_from_row)
            .optional()?)
    }

    pub fn batches_with_status(&self, statuses: &[&str], since: i64) -> Result<Vec<Batch>> {
        let placeholders = statuses.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT {BATCH_COLS} FROM analysis_batches WHERE status IN ({placeholders}) AND batch_start_ts >= ? ORDER BY batch_start_ts ASC"
        );
        let c = self.c();
        let mut st = c.prepare(&sql)?;
        let mut args: Vec<rusqlite::types::Value> = statuses.iter().map(|s| (*s).to_string().into()).collect();
        args.push(since.into());
        let rows = st
            .query_map(rusqlite::params_from_iter(args), batch_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn batch_screenshots(&self, batch_id: i64) -> Result<Vec<Shot>> {
        let c = self.c();
        let mut st = c.prepare(&format!(
            "SELECT {SHOT_COLS} FROM screenshots s JOIN batch_screenshots b ON b.screenshot_id = s.id WHERE b.batch_id = ? ORDER BY s.captured_at"
        ))?;
        let rows = st.query_map([batch_id], shot_from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn set_batch_status(&self, id: i64, status: &str, reason: Option<&str>) -> Result<()> {
        self.c().execute("UPDATE analysis_batches SET status = ?, reason = ? WHERE id = ?", params![status, reason, id])?;
        Ok(())
    }

    pub fn increment_attempts(&self, id: i64) -> Result<()> {
        self.c().execute("UPDATE analysis_batches SET attempts = attempts + 1 WHERE id = ?", [id])?;
        Ok(())
    }

    pub fn reset_attempts(&self, id: i64) -> Result<()> {
        self.c().execute("UPDATE analysis_batches SET attempts = 0 WHERE id = ?", [id])?;
        Ok(())
    }

    /// Whether the batch's "Processing failed" card is still on the timeline.
    pub fn has_error_card(&self, batch_id: i64) -> Result<bool> {
        let n: i64 = self.c().query_row(
            "SELECT COUNT(*) FROM timeline_cards WHERE batch_id = ? AND category = ? AND title = ? AND is_deleted = 0",
            params![batch_id, SYSTEM_CATEGORY, ERROR_TITLE],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    pub fn count_batches_with_status(&self, statuses: &[&str]) -> Result<i64> {
        let placeholders = statuses.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let c = self.c();
        let n = c.query_row(
            &format!("SELECT COUNT(*) FROM analysis_batches WHERE status IN ({placeholders})"),
            rusqlite::params_from_iter(statuses.iter()),
            |r| r.get(0),
        )?;
        Ok(n)
    }

    // ---- observations ----

    pub fn insert_observations(&self, batch_id: i64, obs: &[crate::types::Observation], model: Option<&str>) -> Result<()> {
        let mut c = self.c();
        let tx = c.transaction()?;
        {
            let mut st = tx.prepare("INSERT INTO observations (batch_id, start_ts, end_ts, observation, llm_model) VALUES (?,?,?,?,?)")?;
            for o in obs {
                st.execute(params![batch_id, o.start_ts, o.end_ts, o.text, model])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn delete_observations(&self, batch_id: i64) -> Result<()> {
        self.c().execute("DELETE FROM observations WHERE batch_id = ?", [batch_id])?;
        Ok(())
    }

    /// Observations overlapping [from, to).
    pub fn observations_in_range(&self, from: i64, to: i64) -> Result<Vec<crate::types::Observation>> {
        let c = self.c();
        let mut st = c.prepare(
            "SELECT start_ts, end_ts, observation FROM observations WHERE start_ts < ? AND end_ts > ? ORDER BY start_ts",
        )?;
        let rows = st
            .query_map([to, from], |r| {
                Ok(crate::types::Observation { start_ts: r.get(0)?, end_ts: r.get(1)?, text: r.get(2)? })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    // ---- cards ----

    /// Live cards overlapping [from, to), optionally excluding System (error) cards.
    pub fn cards_in_range(&self, from: i64, to: i64, exclude_system: bool) -> Result<Vec<StoredCard>> {
        let c = self.c();
        let sql = format!(
            "SELECT {CARD_COLS} FROM timeline_cards WHERE is_deleted = 0
             AND ((start_ts < ?1 AND end_ts > ?2) OR (start_ts >= ?2 AND start_ts < ?1))
             {} ORDER BY start_ts",
            if exclude_system { "AND category != 'System'" } else { "" }
        );
        let mut st = c.prepare(&sql)?;
        let rows = st.query_map([to, from], card_from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn cards_for_day(&self, day: &str) -> Result<Vec<StoredCard>> {
        let c = self.c();
        let mut st = c.prepare(&format!("SELECT {CARD_COLS} FROM timeline_cards WHERE is_deleted = 0 AND day = ? ORDER BY start_ts"))?;
        let rows = st.query_map([day], card_from_row)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn card(&self, id: i64) -> Result<Option<StoredCard>> {
        Ok(self
            .c()
            .query_row(&format!("SELECT {CARD_COLS} FROM timeline_cards WHERE id = ?"), [id], card_from_row)
            .optional()?)
    }

    /// The most recent live card ending at or before `ts`.
    pub fn last_card_ending_by(&self, ts: i64) -> Result<Option<StoredCard>> {
        Ok(self
            .c()
            .query_row(
                &format!("SELECT {CARD_COLS} FROM timeline_cards WHERE is_deleted = 0 AND end_ts <= ? ORDER BY end_ts DESC LIMIT 1"),
                [ts],
                card_from_row,
            )
            .optional()?)
    }

    /// Dayflow's replaceTimelineCardsInRange: soft-delete every live card overlapping [from, to)
    /// except System cards owned by other batches, then insert `cards` under `batch_id`.
    pub fn replace_cards_in_range(&self, from: i64, to: i64, cards: &[ResolvedCard], batch_id: i64) -> Result<usize> {
        let mut c = self.c();
        let tx = c.transaction()?;
        tx.execute(
            "UPDATE timeline_cards SET is_deleted = 1 WHERE is_deleted = 0
             AND ((start_ts < ?1 AND end_ts > ?2) OR (start_ts >= ?2 AND start_ts < ?1))
             AND (category != 'System' OR batch_id = ?3)",
            params![to, from, batch_id],
        )?;
        let mut n = 0;
        {
            let mut st = tx.prepare(
                "INSERT INTO timeline_cards (batch_id, start, end, start_ts, end_ts, day, title, summary, category, subcategory, detailed_summary, metadata)
                 VALUES (?,?,?,?,?,?,?,?,?,?,?,?)",
            )?;
            for rc in cards {
                let card = &rc.card;
                let mut meta = serde_json::Map::new();
                if let Some(d) = &card.distractions {
                    if !d.is_empty() {
                        meta.insert("distractions".into(), serde_json::to_value(d)?);
                    }
                }
                if let Some(a) = &card.app_sites {
                    meta.insert("appSites".into(), serde_json::to_value(a)?);
                }
                if let Some(i) = &rc.idle_meta {
                    meta.insert("idle".into(), i.clone());
                }
                st.execute(params![
                    batch_id,
                    card.start_time,
                    card.end_time,
                    rc.start_ts,
                    rc.end_ts,
                    timeutil::logical_day(rc.start_ts),
                    card.title,
                    card.summary,
                    card.category,
                    card.subcategory,
                    card.detailed_summary,
                    serde_json::Value::Object(meta).to_string(),
                ])?;
                n += 1;
            }
        }
        tx.commit()?;
        Ok(n)
    }

    // ---- llm call log ----

    #[allow(clippy::too_many_arguments)]
    pub fn log_llm_call(
        &self,
        batch_id: Option<i64>,
        operation: &str,
        model: &str,
        latency_ms: i64,
        request: &str,
        response: Option<&str>,
        error: Option<&str>,
        usage: (Option<i64>, Option<i64>),
    ) {
        let _ = self.c().execute(
            "INSERT INTO llm_calls (batch_id, operation, model, latency_ms, request, response, error, input_tokens, output_tokens) VALUES (?,?,?,?,?,?,?,?,?)",
            params![batch_id, operation, model, latency_ms, request, response, error, usage.0, usage.1],
        );
    }

    pub fn token_usage_since(&self, since: i64) -> Result<(i64, i64)> {
        Ok(self.c().query_row(
            "SELECT COALESCE(SUM(input_tokens),0), COALESCE(SUM(output_tokens),0) FROM llm_calls WHERE created_at >= datetime(?, 'unixepoch')",
            [since],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?)
    }
}
