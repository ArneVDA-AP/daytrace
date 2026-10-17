//! Replays a folder of JPEG frames (10 s apart, ending now) through the real analysis pipeline.
//! Ignored by default because it calls a live model.
//!
//!   DAYTRACE_API_KEY=... DAYTRACE_REPLAY_DIR=frames DAYTRACE_PIPELINE=cloud|local \
//!   [DAYTRACE_MODEL=claude-haiku-5-5] cargo test --test replay -- --ignored --nocapture
//!
//! DAYTRACE_REPLAY_JSON=<file> also writes batches, observations, cards and token usage as JSON,
//! for reference/compare.py (side-by-side with the Dayflow reference harness).

use daytrace_lib::config::{Paths, Pipeline};
use daytrace_lib::state::AppState;
use daytrace_lib::timeutil::now_ts;

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn replay_frames() {
    let _ = env_logger::builder().is_test(true).filter_level(log::LevelFilter::Info).try_init();
    let dir = std::env::var("DAYTRACE_REPLAY_DIR").expect("DAYTRACE_REPLAY_DIR");
    // Image files only: reference/frames also holds the harness manifest.json.
    let is_image = |p: &std::path::Path| {
        matches!(p.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref(), Some("jpg" | "jpeg" | "png"))
    };
    let mut frames: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).filter(|p| is_image(p)).collect();
    frames.sort();
    // Repeat the last frame once so the trailing 15-minute batch closes.
    frames.push(frames.last().unwrap().clone());
    frames.push(frames.last().unwrap().clone());

    let tmp = tempfile::tempdir().unwrap();
    // DAYTRACE_REPLAY_OUT keeps the resulting data directory (to open it in the app afterwards).
    let root = std::env::var("DAYTRACE_REPLAY_OUT").map(std::path::PathBuf::from).unwrap_or(tmp.path().to_path_buf());
    let state = AppState::new(Paths::new(root).unwrap()).unwrap();
    {
        let mut s = state.settings.write().unwrap();
        s.pipeline = if std::env::var("DAYTRACE_PIPELINE").as_deref() == Ok("local") { Pipeline::Local } else { Pipeline::Cloud };
        s.use_batch_api = std::env::var("DAYTRACE_BATCH").as_deref() == Ok("1");
        s.reasoning = match std::env::var("DAYTRACE_REASONING").as_deref() {
            Ok("low") => daytrace_lib::config::Reasoning::Low,
            Ok("default") => daytrace_lib::config::Reasoning::Default,
            _ => daytrace_lib::config::Reasoning::Off,
        };
        if let Ok(m) = std::env::var("DAYTRACE_MODEL") {
            s.model = m;
        }
        println!("pipeline {:?}, model {}, batch api {}, reasoning {:?}", s.pipeline, s.model, s.use_batch_api, s.reasoning);
    }
    let base = now_ts() - 10 * frames.len() as i64;
    for (i, f) in frames.iter().enumerate() {
        let size = std::fs::metadata(f).unwrap().len() as i64;
        state.db.insert_screenshot(base + i as i64 * 10, &f.to_string_lossy(), size, 1, Some("Code"), None, None, None).unwrap();
    }

    let started = std::time::Instant::now();
    daytrace_lib::analysis::tick(&state).await.unwrap();
    println!("analysis took {:.1}s", started.elapsed().as_secs_f64());

    for b in state.db.batches_with_status(&["completed", "analyzed", "failed", "skipped_short", "pending"], 0).unwrap() {
        println!("batch {} {} {}..{} reason={:?}", b.id, b.status, b.start_ts, b.end_ts, b.reason);
    }
    println!("\nOBSERVATIONS");
    for o in state.db.observations_in_range(0, i64::MAX).unwrap() {
        println!("[{} - {}] {}", daytrace_lib::timeutil::fmt_clock(o.start_ts), daytrace_lib::timeutil::fmt_clock(o.end_ts), o.text);
    }
    println!("\nCARDS");
    for c in state.db.cards_in_range(0, i64::MAX, false).unwrap() {
        println!("{} - {} [{}] {}\n    {}\n    sites={:?} distractions={}\n    detailed: {}", c.start, c.end, c.category, c.title, c.summary, c.app_sites, c.distractions.len(), c.detailed_summary.lines().count());
    }
    let (i, o) = state.db.token_usage_since(0).unwrap();
    println!("\ntokens in={i} out={o}");

    if let Ok(path) = std::env::var("DAYTRACE_REPLAY_JSON") {
        write_json(&state, &path, base, frames.len());
    }
}

/// Same shape as the Dayflow harness output where it can be (offsets are seconds after the first frame).
fn write_json(state: &AppState, path: &str, base: i64, frame_count: usize) {
    use daytrace_lib::timeutil::fmt_clock;
    use serde_json::json;
    let settings = state.settings.read().unwrap().clone();
    let batches: Vec<_> = state
        .db
        .batches_with_status(&["completed", "analyzed", "failed", "skipped_short", "pending", "processing", "failed_empty"], 0)
        .unwrap()
        .into_iter()
        .map(|b| json!({"id": b.id, "start_ts": b.start_ts, "end_ts": b.end_ts, "status": b.status, "reason": b.reason}))
        .collect();
    let observations: Vec<_> = state
        .db
        .observations_in_range(0, i64::MAX)
        .unwrap()
        .into_iter()
        .map(|o| {
            json!({"start_ts": o.start_ts, "end_ts": o.end_ts, "offset_start_s": o.start_ts - base, "offset_end_s": o.end_ts - base,
                   "start_clock": fmt_clock(o.start_ts), "end_clock": fmt_clock(o.end_ts), "observation": o.text})
        })
        .collect();
    let cards: Vec<_> = state
        .db
        .cards_in_range(0, i64::MAX, false)
        .unwrap()
        .into_iter()
        .map(|c| {
            json!({"id": c.id, "batch_id": c.batch_id, "start_ts": c.start_ts, "end_ts": c.end_ts,
                   "offset_start_s": c.start_ts - base, "offset_end_s": c.end_ts - base, "day": c.day,
                   "card": {"startTime": c.start, "endTime": c.end, "category": c.category, "subcategory": c.subcategory,
                            "title": c.title, "summary": c.summary, "detailedSummary": c.detailed_summary,
                            "distractions": c.distractions, "appSites": c.app_sites}})
        })
        .collect();
    let calls: Vec<serde_json::Value> = {
        let conn = state.db.conn();
        let mut st = conn
            .prepare("SELECT batch_id, operation, model, latency_ms, error, input_tokens, output_tokens FROM llm_calls ORDER BY id")
            .unwrap();
        st.query_map([], |r| {
            Ok(json!({"batch_id": r.get::<_, Option<i64>>(0)?, "operation": r.get::<_, String>(1)?, "model": r.get::<_, Option<String>>(2)?,
                      "latency_ms": r.get::<_, Option<i64>>(3)?, "error": r.get::<_, Option<String>>(4)?,
                      "input_tokens": r.get::<_, Option<i64>>(5)?, "output_tokens": r.get::<_, Option<i64>>(6)?}))
        })
        .unwrap()
        .flatten()
        .collect()
    };
    let (input_tokens, output_tokens) = state.db.token_usage_since(0).unwrap();
    let out = json!({
        "source": "daytrace replay test",
        "pipeline": format!("{:?}", settings.pipeline),
        "model": settings.model,
        "reasoning": format!("{:?}", settings.reasoning),
        "batch_api": settings.use_batch_api,
        "time_zone": std::env::var("TZ").ok(),
        "base_ts": base,
        "frames": frame_count,
        "batches": batches,
        "observations": observations,
        "cards": cards,
        "requests": calls,
        "usage": {"input_tokens": input_tokens, "output_tokens": output_tokens},
    });
    std::fs::write(path, serde_json::to_string_pretty(&out).unwrap()).unwrap();
    println!("wrote {path}");
}
