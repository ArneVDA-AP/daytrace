//! Replays a folder of JPEG frames (10 s apart, ending now) through the real analysis pipeline.
//! Ignored by default because it calls a live model.
//!
//!   DAYTRACE_API_KEY=... DAYTRACE_REPLAY_DIR=frames DAYTRACE_PIPELINE=cloud|local \
//!   [DAYTRACE_MODEL=claude-haiku-5-5] cargo test --test replay -- --ignored --nocapture

use daytrace_lib::config::{Paths, Pipeline};
use daytrace_lib::state::AppState;
use daytrace_lib::timeutil::now_ts;

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn replay_frames() {
    let _ = env_logger::builder().is_test(true).filter_level(log::LevelFilter::Info).try_init();
    let dir = std::env::var("DAYTRACE_REPLAY_DIR").expect("DAYTRACE_REPLAY_DIR");
    let mut frames: Vec<_> = std::fs::read_dir(&dir).unwrap().flatten().map(|e| e.path()).collect();
    frames.sort();
    // Repeat the last frame once so the trailing 15-minute batch closes.
    frames.push(frames.last().unwrap().clone());
    frames.push(frames.last().unwrap().clone());

    let tmp = tempfile::tempdir().unwrap();
    let state = AppState::new(Paths::new(tmp.path().to_path_buf()).unwrap()).unwrap();
    {
        let mut s = state.settings.write().unwrap();
        s.pipeline = if std::env::var("DAYTRACE_PIPELINE").as_deref() == Ok("local") { Pipeline::Local } else { Pipeline::Cloud };
        if let Ok(m) = std::env::var("DAYTRACE_MODEL") {
            s.model = m;
        }
        println!("pipeline {:?}, model {}", s.pipeline, s.model);
    }
    let base = now_ts() - 10 * frames.len() as i64;
    for (i, f) in frames.iter().enumerate() {
        let size = std::fs::metadata(f).unwrap().len() as i64;
        state.db.insert_screenshot(base + i as i64 * 10, &f.to_string_lossy(), size, 1, Some("Code"), None, None).unwrap();
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
        println!("{} - {} [{}] {}\n    {}\n    sites={:?} distractions={}", c.start, c.end, c.category, c.title, c.summary, c.app_sites, c.distractions.len());
    }
    let (i, o) = state.db.token_usage_since(0).unwrap();
    println!("\ntokens in={i} out={o}");
}
