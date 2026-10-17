//! Card editing, deleting, summary feedback and the failed-card groups of the timeline.
//!
//! Ported from Dayflow (MIT, (c) 2025 Jerry Liu):
//! - Views/UI/MainView/Actions.swift (handleTitleChange, handleCategoryChange, handleTimelineDelete)
//!   and Core/Recording/StorageManager+TimelineCards.swift (updateTimelineCard,
//!   deleteTimelineCard: soft delete plus the observations under the card).
//! - Views/UI/MainView/TimelineActivityLoader.resolveDisplaySegments: consecutive "Processing
//!   failed" cards no more than 60 s apart are shown as one ("failed · N intervals").
//! - Views/UI/TimelineRateSummaryView.swift: thumbs up / down on a summary. Dayflow sends the
//!   rating and note to its analytics; here they are only stored in the local database.
//!
//! Dayflow does not protect an edited card from the next analysis window, which can rewrite it.
//! Daytrace marks edited cards (`db::USER_EDITED_KEY`); the window rewrite keeps them and leaves
//! them out of the model's input, like Idle cards.

use crate::db::{Db, StoredCard, ERROR_TITLE, SYSTEM_CATEGORY, USER_EDITED_KEY};
use crate::timeutil::now_ts;
use crate::types::Category;
use anyhow::{anyhow, bail, Result};
use rusqlite::{params, OptionalExtension};
use serde::Serialize;
use serde_json::{json, Map, Value};

/// Dayflow's failureGroupingGapTolerance.
pub const FAILURE_GAP: i64 = 60;

pub fn is_failed(c: &StoredCard) -> bool {
    c.category.trim() == SYSTEM_CATEGORY && c.title == ERROR_TITLE
}

struct Row {
    is_deleted: bool,
    category: String,
    meta: Map<String, Value>,
    start_ts: i64,
    end_ts: i64,
}

fn row(db: &Db, id: i64) -> Result<Option<Row>> {
    let c = db.conn();
    let r = c
        .query_row("SELECT is_deleted, category, metadata, start_ts, end_ts FROM timeline_cards WHERE id = ?", [id], |r| {
            let meta: Option<String> = r.get(2)?;
            Ok(Row {
                is_deleted: r.get::<_, i64>(0)? != 0,
                category: r.get(1)?,
                meta: meta.and_then(|m| serde_json::from_str::<Map<String, Value>>(&m).ok()).unwrap_or_default(),
                start_ts: r.get(3)?,
                end_ts: r.get(4)?,
            })
        })
        .optional()?;
    Ok(r)
}

fn live_row(db: &Db, id: i64) -> Result<Row> {
    match row(db, id)? {
        None => bail!("This card no longer exists."),
        Some(r) if r.is_deleted => bail!("This card was replaced by a newer version. Select it again on the timeline."),
        Some(r) => Ok(r),
    }
}

/// Whether the card carries the user-edited mark.
pub fn is_user_edited(db: &Db, id: i64) -> Result<bool> {
    Ok(row(db, id)?.is_some_and(|r| r.meta.contains_key(USER_EDITED_KEY)))
}

#[derive(Debug, Clone, Default)]
pub struct CardEdit {
    pub title: Option<String>,
    pub category: Option<String>,
    pub subcategory: Option<String>,
}

/// Renames and/or recategorizes a card and marks it as edited. Category names are matched to the
/// settings case-insensitively. Returns the updated card.
pub fn update_card(db: &Db, id: i64, edit: &CardEdit, cats: &[Category]) -> Result<StoredCard> {
    let r = live_row(db, id)?;
    if r.category.trim() == SYSTEM_CATEGORY {
        bail!("Cards that failed to process can't be edited. Try again or delete the card.");
    }
    let card = db.card(id)?.ok_or_else(|| anyhow!("This card no longer exists."))?;
    let title = match &edit.title {
        Some(t) if t.trim().is_empty() => bail!("The title can't be empty."),
        Some(t) => t.trim().to_string(),
        None => card.title.clone(),
    };
    let category = match &edit.category {
        Some(c) if c.trim().is_empty() => bail!("Choose a category."),
        Some(c) if c.trim().eq_ignore_ascii_case(SYSTEM_CATEGORY) => bail!("System is reserved for cards that failed to process."),
        Some(c) => {
            let c = c.trim();
            cats.iter().find(|k| k.name.trim().eq_ignore_ascii_case(c)).map(|k| k.name.trim().to_string()).unwrap_or_else(|| c.to_string())
        }
        None => card.category.clone(),
    };
    let subcategory = edit.subcategory.as_deref().map(|s| s.trim().to_string()).unwrap_or_else(|| card.subcategory.clone());

    let mut changed: Vec<&str> = Vec::new();
    if title != card.title {
        changed.push("title");
    }
    if category != card.category {
        changed.push("category");
    }
    if subcategory != card.subcategory {
        changed.push("subcategory");
    }
    if changed.is_empty() {
        return Ok(card);
    }

    let mut meta = r.meta;
    let mut fields: Vec<String> = meta
        .get(USER_EDITED_KEY)
        .and_then(|v| v.get("fields"))
        .and_then(|f| serde_json::from_value(f.clone()).ok())
        .unwrap_or_default();
    for f in changed {
        if !fields.iter().any(|x| x == f) {
            fields.push(f.to_string());
        }
    }
    // An edited card is the user's statement about that time; it no longer counts as an
    // automatic Idle card (which a later Idle batch could stretch over).
    meta.remove("idle");
    meta.insert(USER_EDITED_KEY.into(), json!({ "at": now_ts(), "fields": fields }));
    let n = db.conn().execute(
        "UPDATE timeline_cards SET title = ?, category = ?, subcategory = ?, metadata = ? WHERE id = ? AND is_deleted = 0",
        params![title, category, subcategory, Value::Object(meta).to_string(), id],
    )?;
    if n == 0 {
        bail!("This card was replaced by a newer version. Select it again on the timeline.");
    }
    db.card(id)?.ok_or_else(|| anyhow!("This card no longer exists."))
}

/// Removes the user-edited mark, so the next analysis of its window may rewrite the card.
pub fn clear_user_edit(db: &Db, id: i64) -> Result<()> {
    let Some(mut r) = row(db, id)? else { return Ok(()) };
    if r.meta.remove(USER_EDITED_KEY).is_some() {
        db.conn().execute("UPDATE timeline_cards SET metadata = ? WHERE id = ?", params![Value::Object(r.meta).to_string(), id])?;
    }
    Ok(())
}

/// Dayflow deleteTimelineCard: soft-deletes the card and removes the screen descriptions under
/// it, so the next window rewrite does not bring the activity back. A failed card has nothing
/// under it; deleting it dismisses the failure (no automatic retry). Returns false when the card
/// was already gone.
pub fn delete_card(db: &Db, id: i64) -> Result<bool> {
    let Some(r) = row(db, id)? else { return Ok(false) };
    if r.is_deleted {
        return Ok(false);
    }
    let mut c = db.conn();
    let tx = c.transaction()?;
    tx.execute("UPDATE timeline_cards SET is_deleted = 1 WHERE id = ? AND is_deleted = 0", [id])?;
    if r.category.trim() != SYSTEM_CATEGORY && r.end_ts > r.start_ts {
        tx.execute(
            "DELETE FROM observations WHERE (start_ts < ?1 AND end_ts > ?2) OR (start_ts >= ?2 AND start_ts < ?1)",
            params![r.end_ts, r.start_ts],
        )?;
    }
    tx.commit()?;
    Ok(true)
}

// ---- summary feedback (thumbs) ----

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Feedback {
    pub card_id: i64,
    /// "up" or "down".
    pub direction: String,
    pub note: String,
    pub updated_at: i64,
}

/// Stores a thumbs up/down and an optional note for a card's summary; `direction` None removes it.
pub fn set_feedback(db: &Db, card_id: i64, direction: Option<&str>, note: &str) -> Result<Option<Feedback>> {
    let Some(dir) = direction else {
        db.conn().execute("DELETE FROM card_feedback WHERE card_id = ?", [card_id])?;
        return Ok(None);
    };
    if dir != "up" && dir != "down" {
        bail!("Unknown rating: {dir}");
    }
    let card = db.card(card_id)?.ok_or_else(|| anyhow!("This card no longer exists."))?;
    let now = now_ts();
    let note = note.trim();
    db.conn().execute(
        "INSERT INTO card_feedback (card_id, direction, note, card_title, card_start_ts, card_end_ts, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
         ON CONFLICT(card_id) DO UPDATE SET direction = excluded.direction, note = excluded.note,
           card_title = excluded.card_title, updated_at = excluded.updated_at",
        params![card_id, dir, note, card.title, card.start_ts, card.end_ts, now],
    )?;
    Ok(Some(Feedback { card_id, direction: dir.into(), note: note.into(), updated_at: now }))
}

pub fn feedback_for(db: &Db, card_ids: &[i64]) -> Result<Vec<Feedback>> {
    if card_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = card_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let c = db.conn();
    let mut st = c.prepare(&format!(
        "SELECT card_id, direction, note, updated_at FROM card_feedback WHERE card_id IN ({placeholders}) ORDER BY card_id"
    ))?;
    let rows = st
        .query_map(rusqlite::params_from_iter(card_ids.iter()), |r| {
            Ok(Feedback { card_id: r.get(0)?, direction: r.get(1)?, note: r.get(2)?, updated_at: r.get(3)? })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

// ---- failed groups ----

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FailedGroup {
    pub card_ids: Vec<i64>,
    /// Distinct batches in time order.
    pub batch_ids: Vec<i64>,
    pub start_ts: i64,
    pub end_ts: i64,
    /// False when the screenshots of every batch in the group were deleted.
    pub retryable: bool,
}

/// Groups runs of consecutive failed cards (Dayflow resolveDisplaySegments). Cards are taken in
/// start order; a failed card joins the previous group when the card before it was failed too and
/// it starts no more than 60 s after the group ends. Single failures form groups of one.
pub fn group_failed(cards: &[StoredCard]) -> Vec<FailedGroup> {
    let mut sorted: Vec<&StoredCard> = cards.iter().collect();
    sorted.sort_by(|a, b| a.start_ts.cmp(&b.start_ts).then(a.end_ts.cmp(&b.end_ts)));
    let mut groups: Vec<FailedGroup> = Vec::new();
    let mut open = false;
    for c in sorted {
        if !is_failed(c) {
            open = false;
            continue;
        }
        match groups.last_mut() {
            Some(g) if open && c.start_ts - g.end_ts <= FAILURE_GAP => {
                g.card_ids.push(c.id);
                if let Some(b) = c.batch_id.filter(|b| !g.batch_ids.contains(b)) {
                    g.batch_ids.push(b);
                }
                g.start_ts = g.start_ts.min(c.start_ts);
                g.end_ts = g.end_ts.max(c.end_ts);
            }
            _ => groups.push(FailedGroup {
                card_ids: vec![c.id],
                batch_ids: c.batch_id.into_iter().collect(),
                start_ts: c.start_ts,
                end_ts: c.end_ts,
                retryable: true,
            }),
        }
        open = true;
    }
    groups
}

/// Whether a batch still has its screenshot rows (deleted after the retention period).
pub fn batch_has_frames(db: &Db, batch_id: i64) -> Result<bool> {
    let n: i64 = db.conn().query_row(
        "SELECT COUNT(*) FROM batch_screenshots b JOIN screenshots s ON s.id = b.screenshot_id WHERE b.batch_id = ? AND s.is_deleted = 0",
        [batch_id],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

/// Ids of the live cards among `cards` that the user edited.
pub fn edited_ids(db: &Db, cards: &[StoredCard]) -> Result<Vec<i64>> {
    let (Some(from), Some(to)) = (cards.iter().map(|c| c.start_ts).min(), cards.iter().map(|c| c.end_ts).max()) else {
        return Ok(Vec::new());
    };
    let set = db.user_edited_card_ids(from, to.max(from + 1))?;
    Ok(cards.iter().map(|c| c.id).filter(|id| set.contains(id)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::ResolvedCard;
    use crate::features::insights::stats::tests::card;
    use crate::types::{default_categories, ActivityCard, Observation};

    fn activity(title: &str, category: &str, start: i64, end: i64) -> ActivityCard {
        ActivityCard {
            start_time: crate::timeutil::fmt_clock(start),
            end_time: crate::timeutil::fmt_clock(end),
            category: category.into(),
            subcategory: String::new(),
            title: title.into(),
            summary: format!("{title} summary"),
            detailed_summary: String::new(),
            distractions: None,
            app_sites: None,
        }
    }

    fn batch(db: &Db, at: i64) -> i64 {
        db.insert_screenshot(at, "", 0, 0, None, None, None, None).unwrap();
        let shots = db.unbatched_screenshots(0).unwrap();
        db.create_batch(&shots).unwrap()
    }

    fn put(db: &Db, b: i64, from: i64, to: i64, cards: &[(&str, &str, i64, i64)], keep_idle: bool) {
        let rc: Vec<ResolvedCard> = cards
            .iter()
            .map(|(t, c, s, e)| ResolvedCard { start_ts: *s, end_ts: *e, card: activity(t, c, *s, *e), idle_meta: None })
            .collect();
        db.replace_cards_in_range(from, to, &rc, b, keep_idle).unwrap();
    }

    #[test]
    fn edited_card_survives_window_rewrite() {
        let db = Db::memory();
        let t = 1_800_000_000;
        let b1 = batch(&db, t);
        put(&db, b1, t, t + 1800, &[("Coding", "Work", t, t + 900), ("Videos", "Distraction", t + 900, t + 1800)], true);
        let live = db.cards_in_range(0, i64::MAX, false).unwrap();
        let videos = live.iter().find(|c| c.title == "Videos").unwrap().id;

        let edit = CardEdit { title: Some("Conference talk".into()), category: Some("work".into()), subcategory: Some("Learning".into()) };
        let edited = update_card(&db, videos, &edit, &default_categories()).unwrap();
        assert_eq!((edited.title.as_str(), edited.category.as_str(), edited.subcategory.as_str()), ("Conference talk", "Work", "Learning"));
        assert!(is_user_edited(&db, videos).unwrap());

        // The next batch rewrites the whole window; the edited card must stay as the user left it.
        let b2 = batch(&db, t + 2000);
        put(&db, b2, t, t + 2700, &[("Rewritten", "Personal", t, t + 2700)], true);
        let live = db.cards_in_range(0, i64::MAX, false).unwrap();
        let kept = live.iter().find(|c| c.id == videos).expect("edited card kept");
        assert_eq!(kept.title, "Conference talk");
        assert_eq!(live.len(), 2, "the unedited card was replaced, the edited one kept");
        assert!(db.user_edited_card_ids(t, t + 2700).unwrap().contains(&videos));
        // An Idle batch rewrite (keep_idle = false) keeps it too.
        put(&db, b2, t, t + 2700, &[], false);
        assert!(db.cards_in_range(0, i64::MAX, false).unwrap().iter().any(|c| c.id == videos));

        // Clearing the mark makes it rewritable again.
        clear_user_edit(&db, videos).unwrap();
        put(&db, b2, t, t + 2700, &[], true);
        assert!(db.cards_in_range(0, i64::MAX, false).unwrap().is_empty());
    }

    #[test]
    fn edit_validation_and_no_op() {
        let db = Db::memory();
        let t = 1_800_000_000;
        let b = batch(&db, t);
        put(&db, b, t, t + 900, &[("Coding", "Work", t, t + 900)], true);
        let id = db.cards_in_range(0, i64::MAX, false).unwrap()[0].id;
        let cats = default_categories();
        assert!(update_card(&db, id, &CardEdit { title: Some("  ".into()), ..Default::default() }, &cats).is_err());
        assert!(update_card(&db, id, &CardEdit { category: Some("system".into()), ..Default::default() }, &cats).is_err());
        // Same values: nothing changes and no mark is set.
        update_card(&db, id, &CardEdit { title: Some("Coding ".into()), ..Default::default() }, &cats).unwrap();
        assert!(!is_user_edited(&db, id).unwrap());
        // Failed cards can't be edited.
        let b2 = batch(&db, t + 1000);
        put(&db, b2, t + 1000, t + 1900, &[(ERROR_TITLE, SYSTEM_CATEGORY, t + 1000, t + 1900)], true);
        let failed = db.cards_in_range(t + 1000, t + 1900, false).unwrap().into_iter().find(is_failed_ref).unwrap().id;
        assert!(update_card(&db, failed, &CardEdit { title: Some("x".into()), ..Default::default() }, &cats).is_err());
        // A replaced (soft-deleted) card reports that.
        assert!(delete_card(&db, id).unwrap());
        assert!(update_card(&db, id, &CardEdit { title: Some("x".into()), ..Default::default() }, &cats).is_err());
        assert!(!delete_card(&db, id).unwrap(), "second delete is a no-op");
    }

    fn is_failed_ref(c: &StoredCard) -> bool {
        is_failed(c)
    }

    #[test]
    fn delete_removes_card_and_observations_under_it() {
        let db = Db::memory();
        let t = 1_800_000_000;
        let b = batch(&db, t);
        put(&db, b, t, t + 1800, &[("A", "Work", t, t + 900), ("B", "Work", t + 900, t + 1800)], true);
        let obs = [
            Observation { start_ts: t, end_ts: t + 600, text: "a".into() },
            Observation { start_ts: t + 1000, end_ts: t + 1500, text: "b".into() },
        ];
        db.insert_observations(b, &obs, None).unwrap();
        let b_id = db.cards_in_range(0, i64::MAX, false).unwrap().into_iter().find(|c| c.title == "B").unwrap().id;
        assert!(delete_card(&db, b_id).unwrap());
        let live = db.cards_in_range(0, i64::MAX, false).unwrap();
        assert_eq!(live.iter().map(|c| c.title.as_str()).collect::<Vec<_>>(), vec!["A"]);
        let left = db.observations_in_range(0, i64::MAX).unwrap();
        assert_eq!(left.iter().map(|o| o.text.as_str()).collect::<Vec<_>>(), vec!["a"]);
    }

    #[test]
    fn feedback_is_stored_locally() {
        let db = Db::memory();
        let t = 1_800_000_000;
        let b = batch(&db, t);
        put(&db, b, t, t + 900, &[("Coding", "Work", t, t + 900)], true);
        let id = db.cards_in_range(0, i64::MAX, false).unwrap()[0].id;
        set_feedback(&db, id, Some("down"), "  Wrong project  ").unwrap();
        let f = feedback_for(&db, &[id, 999]).unwrap();
        assert_eq!(f.len(), 1);
        assert_eq!((f[0].direction.as_str(), f[0].note.as_str()), ("down", "Wrong project"));
        set_feedback(&db, id, Some("up"), "").unwrap();
        assert_eq!(feedback_for(&db, &[id]).unwrap()[0].direction, "up");
        assert!(set_feedback(&db, id, Some("sideways"), "").is_err());
        set_feedback(&db, id, None, "").unwrap();
        assert!(feedback_for(&db, &[id]).unwrap().is_empty());
    }

    #[test]
    fn failed_runs_are_grouped() {
        let f = |id: i64, s: i64, e: i64, batch: i64| {
            let mut c = card(id, s, e, SYSTEM_CATEGORY, ERROR_TITLE);
            c.batch_id = Some(batch);
            c
        };
        let cards = vec![
            f(1, 0, 900, 10),
            f(2, 930, 1800, 11), // 30 s gap: same group
            f(3, 1800, 2700, 11), // same batch twice: listed once
            card(4, 2700, 3600, "Work", "Coding"),
            f(5, 3600, 4500, 12), // after a normal card: new group
            f(6, 4600, 5400, 13), // 100 s gap: new group
        ];
        let g = group_failed(&cards);
        assert_eq!(g.len(), 3);
        assert_eq!((g[0].card_ids.clone(), g[0].batch_ids.clone(), g[0].start_ts, g[0].end_ts), (vec![1, 2, 3], vec![10, 11], 0, 2700));
        assert_eq!(g[1].card_ids, vec![5]);
        assert_eq!(g[2].card_ids, vec![6]);
        assert!(group_failed(&[card(1, 0, 10, "Work", "x")]).is_empty());
    }
}
