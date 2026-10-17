//! Timed pause (Dayflow's PauseManager): pause recording for one of three preset lengths, until
//! the next day starts (4 AM), or until resumed. A timed pause stores its end in
//! `Settings::pause_until` so it survives a restart; the platform loop resumes recording when the
//! time has passed (also right after the PC wakes from sleep, like Dayflow's handleWake).

use crate::config::Settings;
use crate::timeutil::DAY_START_HOUR;
use chrono::{DateTime, Duration, LocalResult, TimeZone, Timelike};

/// Dayflow's PausePresets.defaultMinutes.
pub const DEFAULT_PRESETS: [u32; 3] = [15, 30, 60];
/// Dayflow's PausePresets.minuteOptions: the lengths a preset can be set to.
pub const MINUTE_OPTIONS: [u32; 12] = [5, 10, 15, 20, 30, 45, 60, 90, 120, 180, 240, 480];

/// What the user picked in the tray menu or the status pill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PauseChoice {
    Minutes(u32),
    /// Until the next logical day starts (4 AM).
    Tomorrow,
    /// Until the user resumes.
    Indefinite,
}

impl PauseChoice {
    /// Parses `"15"`, `"tomorrow"` or `"indefinite"` (the values the UI and the tray menu send).
    pub fn parse(s: &str) -> Option<PauseChoice> {
        match s.trim() {
            "tomorrow" => Some(PauseChoice::Tomorrow),
            "indefinite" => Some(PauseChoice::Indefinite),
            n => n.parse::<u32>().ok().filter(|m| (1..=24 * 60).contains(m)).map(PauseChoice::Minutes),
        }
    }

    pub fn id(&self) -> String {
        match self {
            PauseChoice::Minutes(m) => m.to_string(),
            PauseChoice::Tomorrow => "tomorrow".into(),
            PauseChoice::Indefinite => "indefinite".into(),
        }
    }

    /// Menu label, e.g. "15 minutes", "1 hour", "1 hour 30 minutes".
    pub fn label(&self) -> String {
        match self {
            PauseChoice::Minutes(m) => minutes_label(*m),
            PauseChoice::Tomorrow => "Until tomorrow (4 AM)".into(),
            PauseChoice::Indefinite => "Until I resume".into(),
        }
    }
}

pub fn minutes_label(m: u32) -> String {
    let (h, rest) = (m / 60, m % 60);
    let hours = match h {
        0 => String::new(),
        1 => "1 hour".into(),
        n => format!("{n} hours"),
    };
    let mins = match rest {
        0 => String::new(),
        1 => "1 minute".into(),
        n => format!("{n} minutes"),
    };
    match (hours.is_empty(), mins.is_empty()) {
        (true, _) => mins,
        (false, true) => hours,
        (false, false) => format!("{hours} {mins}"),
    }
}

/// The three configured presets, or Dayflow's defaults when the stored list is not three valid
/// lengths (Dayflow keeps a stored list only when it has exactly three entries).
pub fn presets(settings: &Settings) -> [u32; 3] {
    let p = &settings.pause_presets;
    if p.len() == 3 && p.iter().all(|m| (1..=24 * 60).contains(m)) {
        [p[0], p[1], p[2]]
    } else {
        DEFAULT_PRESETS
    }
}

/// All choices in menu order: the timed presets, until tomorrow, until resumed.
pub fn choices(settings: &Settings) -> Vec<PauseChoice> {
    let mut v: Vec<PauseChoice> = presets(settings).iter().map(|m| PauseChoice::Minutes(*m)).collect();
    v.push(PauseChoice::Tomorrow);
    v.push(PauseChoice::Indefinite);
    v
}

/// Start of the next logical day after `now`: 4 AM today when it is before 4 AM, otherwise 4 AM
/// tomorrow, in wall-clock time of `now`'s zone. When DST makes 4 AM ambiguous the first one
/// counts; when 4 AM does not exist the first valid time after the gap does.
pub fn next_day_start<Tz: TimeZone>(now: &DateTime<Tz>) -> DateTime<Tz> {
    let tz = now.timezone();
    let local = now.naive_local();
    let mut date = local.date();
    if local.hour() >= DAY_START_HOUR {
        date = date.succ_opt().unwrap_or(date);
    }
    let target = date.and_hms_opt(DAY_START_HOUR, 0, 0).expect("valid time");
    let resolved = match tz.from_local_datetime(&target) {
        LocalResult::Single(d) => Some(d),
        LocalResult::Ambiguous(a, _) => Some(a),
        // In a spring-forward gap: walk forward in 15-minute steps to the first valid time.
        LocalResult::None => (1..=12).find_map(|i| tz.from_local_datetime(&(target + Duration::minutes(15 * i))).earliest()),
    };
    match resolved {
        Some(d) if d > *now => d,
        _ => now.clone() + Duration::hours(24),
    }
}

/// When a pause picked at `now` ends; None for "until I resume".
pub fn pause_end<Tz: TimeZone>(choice: PauseChoice, now: &DateTime<Tz>) -> Option<i64> {
    match choice {
        PauseChoice::Minutes(m) => Some(now.timestamp() + m as i64 * 60),
        PauseChoice::Tomorrow => Some(next_day_start(now).timestamp()),
        PauseChoice::Indefinite => None,
    }
}

/// Recording mode, mirroring Dayflow's RecordingControlMode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Active,
    PausedUntil(i64),
    PausedIndefinite,
    /// First-run setup not finished: the recorder waits.
    Setup,
}

pub fn mode(settings: &Settings) -> Mode {
    if !settings.onboarding_completed {
        Mode::Setup
    } else if settings.recording_enabled {
        Mode::Active
    } else if let Some(t) = settings.pause_until {
        Mode::PausedUntil(t)
    } else {
        Mode::PausedIndefinite
    }
}

/// True when the recorder should take screenshots (setting on and setup finished).
pub fn recording_allowed(settings: &Settings) -> bool {
    settings.recording_enabled && settings.onboarding_completed
}

/// Applies a pause to the settings.
pub fn apply_pause(settings: &mut Settings, choice: PauseChoice, now: &DateTime<impl TimeZone>) {
    settings.recording_enabled = false;
    settings.pause_until = pause_end(choice, now);
}

/// Applies a pause asked for by a `daytrace://pause` link, which any web page can open: it never
/// shortens the pause that is on. A pause until resumed stays as it is, and of two pause ends the
/// later one is kept. Returns whether the settings changed.
pub fn extend_pause(settings: &mut Settings, choice: PauseChoice, now: &DateTime<impl TimeZone>) -> bool {
    let end = pause_end(choice, now);
    if !settings.recording_enabled {
        match (settings.pause_until, end) {
            // Paused until resumed: nothing lasts longer.
            (None, _) => return false,
            (Some(current), Some(new)) if current >= new => return false,
            _ => {}
        }
    }
    settings.recording_enabled = false;
    settings.pause_until = end;
    true
}

/// Applies a resume (by the user or because the timer ran out).
pub fn apply_resume(settings: &mut Settings) {
    settings.recording_enabled = true;
    settings.pause_until = None;
}

/// What the platform loop has to do about the pause state at `now`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Due {
    Nothing,
    /// The timed pause ended: resume recording.
    Resume,
    /// Recording is on but an old pause end is still stored (e.g. resumed elsewhere): clear it.
    ClearStale,
}

pub fn due(settings: &Settings, now: i64) -> Due {
    match (settings.recording_enabled, settings.pause_until) {
        (false, Some(t)) if now >= t => Due::Resume,
        (true, Some(_)) => Due::ClearStale,
        _ => Due::Nothing,
    }
}

#[cfg(test)]
pub(crate) mod test_zone {
    //! A time zone with one DST period, defined by its UTC start and end, so DST cases can be
    //! tested without depending on the machine's time zone.
    use chrono::{FixedOffset, LocalResult, NaiveDate, NaiveDateTime, Offset, TimeZone};

    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct Zone {
        pub std: i32,
        pub dst: i32,
        pub start: i64,
        pub end: i64,
    }

    #[derive(Debug, Clone, Copy)]
    pub struct ZoneOffset {
        zone: Zone,
        secs: i32,
    }

    impl Offset for ZoneOffset {
        fn fix(&self) -> FixedOffset {
            FixedOffset::east_opt(self.secs).unwrap()
        }
    }

    impl Zone {
        fn at(&self, utc: i64) -> i32 {
            if utc >= self.start && utc < self.end { self.dst } else { self.std }
        }

        /// Europe/Brussels in 2026: CET +1, CEST +2 from 29 March 01:00 UTC to 25 October 01:00 UTC.
        pub fn brussels_2026() -> Zone {
            let utc = |y, m, d, h| NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(h, 0, 0).unwrap().and_utc().timestamp();
            Zone { std: 3600, dst: 7200, start: utc(2026, 3, 29, 1), end: utc(2026, 10, 25, 1) }
        }
    }

    impl TimeZone for Zone {
        type Offset = ZoneOffset;
        fn from_offset(o: &ZoneOffset) -> Zone {
            o.zone
        }
        fn offset_from_local_date(&self, local: &NaiveDate) -> LocalResult<ZoneOffset> {
            self.offset_from_local_datetime(&local.and_hms_opt(0, 0, 0).unwrap())
        }
        fn offset_from_local_datetime(&self, local: &NaiveDateTime) -> LocalResult<ZoneOffset> {
            let wall = local.and_utc().timestamp();
            let mut fits: Vec<i32> = [self.dst, self.std].into_iter().filter(|off| self.at(wall - *off as i64) == *off).collect();
            fits.dedup();
            // Earliest instant first: the larger offset gives the earlier UTC time.
            fits.sort_by(|a, b| b.cmp(a));
            match fits.as_slice() {
                [] => LocalResult::None,
                [o] => LocalResult::Single(ZoneOffset { zone: *self, secs: *o }),
                [a, b, ..] => LocalResult::Ambiguous(ZoneOffset { zone: *self, secs: *a }, ZoneOffset { zone: *self, secs: *b }),
            }
        }
        fn offset_from_utc_date(&self, utc: &NaiveDate) -> ZoneOffset {
            self.offset_from_utc_datetime(&utc.and_hms_opt(0, 0, 0).unwrap())
        }
        fn offset_from_utc_datetime(&self, utc: &NaiveDateTime) -> ZoneOffset {
            ZoneOffset { zone: *self, secs: self.at(utc.and_utc().timestamp()) }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_zone::Zone;
    use super::*;
    use chrono::NaiveDate;

    fn at(z: &Zone, y: i32, m: u32, d: u32, h: u32, min: u32) -> DateTime<Zone> {
        z.from_local_datetime(&NaiveDate::from_ymd_opt(y, m, d).unwrap().and_hms_opt(h, min, 0).unwrap()).earliest().unwrap()
    }

    fn wall(t: &DateTime<Zone>) -> String {
        t.naive_local().format("%Y-%m-%d %H:%M").to_string()
    }

    #[test]
    fn parses_choices() {
        assert_eq!(PauseChoice::parse("15"), Some(PauseChoice::Minutes(15)));
        assert_eq!(PauseChoice::parse("tomorrow"), Some(PauseChoice::Tomorrow));
        assert_eq!(PauseChoice::parse("indefinite"), Some(PauseChoice::Indefinite));
        assert_eq!(PauseChoice::parse("0"), None);
        assert_eq!(PauseChoice::parse("abc"), None);
        assert_eq!(PauseChoice::parse("100000"), None);
        for c in [PauseChoice::Minutes(90), PauseChoice::Tomorrow, PauseChoice::Indefinite] {
            assert_eq!(PauseChoice::parse(&c.id()), Some(c));
        }
    }

    #[test]
    fn labels() {
        assert_eq!(PauseChoice::Minutes(15).label(), "15 minutes");
        assert_eq!(PauseChoice::Minutes(60).label(), "1 hour");
        assert_eq!(PauseChoice::Minutes(90).label(), "1 hour 30 minutes");
        assert_eq!(PauseChoice::Minutes(480).label(), "8 hours");
        assert_eq!(PauseChoice::Tomorrow.label(), "Until tomorrow (4 AM)");
    }

    #[test]
    fn presets_fall_back_to_defaults() {
        let mut s = Settings::default();
        assert_eq!(presets(&s), [15, 30, 60]);
        s.pause_presets = vec![5, 45, 240];
        assert_eq!(presets(&s), [5, 45, 240]);
        s.pause_presets = vec![5, 45];
        assert_eq!(presets(&s), DEFAULT_PRESETS);
        s.pause_presets = vec![0, 45, 240];
        assert_eq!(presets(&s), DEFAULT_PRESETS);
        let ids: Vec<String> = choices(&Settings::default()).iter().map(|c| c.id()).collect();
        assert_eq!(ids, ["15", "30", "60", "tomorrow", "indefinite"]);
    }

    #[test]
    fn timed_pause_ends_after_its_length() {
        let z = Zone::brussels_2026();
        let now = at(&z, 2026, 6, 10, 15, 0);
        assert_eq!(pause_end(PauseChoice::Minutes(15), &now), Some(now.timestamp() + 900));
        assert_eq!(pause_end(PauseChoice::Indefinite, &now), None);
    }

    #[test]
    fn until_tomorrow_is_next_4am() {
        let z = Zone::brussels_2026();
        // Afternoon: 4 AM the next calendar day.
        let t = next_day_start(&at(&z, 2026, 6, 10, 15, 0));
        assert_eq!(wall(&t), "2026-06-11 04:00");
        // Exactly 4 AM: the next day, never "now".
        let t = next_day_start(&at(&z, 2026, 6, 10, 4, 0));
        assert_eq!(wall(&t), "2026-06-11 04:00");
        // After midnight but before 4 AM: still the same logical day, so 4 AM today.
        let t = next_day_start(&at(&z, 2026, 6, 11, 1, 30));
        assert_eq!(wall(&t), "2026-06-11 04:00");
    }

    #[test]
    fn until_tomorrow_across_fall_back() {
        let z = Zone::brussels_2026();
        // 24 October 22:00 CEST; clocks go back at 03:00 on the 25th, so the night is an hour longer.
        let now = at(&z, 2026, 10, 24, 22, 0);
        let end = next_day_start(&now);
        assert_eq!(wall(&end), "2026-10-25 04:00");
        assert_eq!(end.timestamp() - now.timestamp(), 7 * 3600);
        // During the repeated hour (second 2:30 AM, CET), 4 AM is 1.5 hours away.
        let second_0230 = z.from_local_datetime(&NaiveDate::from_ymd_opt(2026, 10, 25).unwrap().and_hms_opt(2, 30, 0).unwrap()).latest().unwrap();
        assert_eq!(next_day_start(&second_0230).timestamp() - second_0230.timestamp(), 90 * 60);
    }

    #[test]
    fn until_tomorrow_across_spring_forward() {
        let z = Zone::brussels_2026();
        // 28 March 22:00 CET; clocks jump from 02:00 to 03:00 on the 29th: 5 hours to 4 AM.
        let now = at(&z, 2026, 3, 28, 22, 0);
        let end = next_day_start(&now);
        assert_eq!(wall(&end), "2026-03-29 04:00");
        assert_eq!(end.timestamp() - now.timestamp(), 5 * 3600);
    }

    #[test]
    fn until_tomorrow_when_4am_is_skipped() {
        // A zone whose clocks jump from 04:00 to 05:00 (UTC+0 to UTC+1 at 04:00 UTC).
        let utc = |d, h| NaiveDate::from_ymd_opt(2026, 3, d).unwrap().and_hms_opt(h, 0, 0).unwrap().and_utc().timestamp();
        let z = Zone { std: 0, dst: 3600, start: utc(29, 4), end: utc(29, 4) + 200 * 86400 };
        let now = at(&z, 2026, 3, 28, 20, 0);
        let end = next_day_start(&now);
        assert_eq!(wall(&end), "2026-03-29 05:00");
        assert_eq!(end.timestamp(), utc(29, 4));
    }

    #[test]
    fn pause_and_resume_settings() {
        let z = Zone::brussels_2026();
        let now = at(&z, 2026, 6, 10, 15, 0);
        let mut s = Settings { onboarding_completed: true, ..Settings::default() };
        assert_eq!(mode(&s), Mode::Active);
        apply_pause(&mut s, PauseChoice::Minutes(60), &now);
        assert!(!s.recording_enabled);
        assert_eq!(mode(&s), Mode::PausedUntil(now.timestamp() + 3600));
        assert_eq!(due(&s, now.timestamp() + 3599), Due::Nothing);
        assert_eq!(due(&s, now.timestamp() + 3600), Due::Resume);
        apply_resume(&mut s);
        assert_eq!(mode(&s), Mode::Active);
        assert_eq!(due(&s, now.timestamp()), Due::Nothing);

        apply_pause(&mut s, PauseChoice::Indefinite, &now);
        assert_eq!(mode(&s), Mode::PausedIndefinite);
        assert_eq!(due(&s, now.timestamp() + 365 * 86400), Due::Nothing);

        // Recording switched back on elsewhere while a pause end was stored.
        s.recording_enabled = true;
        s.pause_until = Some(now.timestamp() + 60);
        assert_eq!(due(&s, now.timestamp()), Due::ClearStale);
    }

    #[test]
    fn a_link_never_shortens_a_pause() {
        let z = Zone::brussels_2026();
        let now = at(&z, 2026, 6, 10, 15, 0);
        let mut s = Settings { onboarding_completed: true, ..Settings::default() };
        // Recording: the link pauses.
        assert!(extend_pause(&mut s, PauseChoice::Minutes(30), &now));
        assert_eq!(mode(&s), Mode::PausedUntil(now.timestamp() + 1800));
        // A shorter pause from a link leaves the longer one.
        assert!(!extend_pause(&mut s, PauseChoice::Minutes(1), &now));
        assert_eq!(mode(&s), Mode::PausedUntil(now.timestamp() + 1800));
        assert!(!extend_pause(&mut s, PauseChoice::Minutes(30), &now), "the same end changes nothing");
        // A longer one extends it.
        assert!(extend_pause(&mut s, PauseChoice::Minutes(90), &now));
        assert_eq!(mode(&s), Mode::PausedUntil(now.timestamp() + 5400));
        // Until tomorrow (4 AM) is later than a minute from now.
        apply_pause(&mut s, PauseChoice::Tomorrow, &now);
        let tomorrow = s.pause_until;
        assert!(!extend_pause(&mut s, PauseChoice::Minutes(1), &now));
        assert_eq!(s.pause_until, tomorrow);
        // Until resumed beats any end.
        apply_pause(&mut s, PauseChoice::Indefinite, &now);
        assert!(!extend_pause(&mut s, PauseChoice::Minutes(1440), &now));
        assert_eq!(mode(&s), Mode::PausedIndefinite);
        // An end that already passed (the loop has not resumed yet) is replaced.
        s.pause_until = Some(now.timestamp() - 5);
        assert!(extend_pause(&mut s, PauseChoice::Minutes(1), &now));
        assert_eq!(s.pause_until, Some(now.timestamp() + 60));
        // Recording on with a stale end left over: the link pauses.
        s.recording_enabled = true;
        s.pause_until = Some(now.timestamp() + 999_999);
        assert!(extend_pause(&mut s, PauseChoice::Minutes(5), &now));
        assert_eq!(mode(&s), Mode::PausedUntil(now.timestamp() + 300));
    }

    #[test]
    fn setup_holds_recording() {
        let s = Settings::default();
        assert!(!s.onboarding_completed);
        assert_eq!(mode(&s), Mode::Setup);
        assert!(!recording_allowed(&s));
        let s = Settings { onboarding_completed: true, ..Settings::default() };
        assert!(recording_allowed(&s));
    }

    #[test]
    fn pause_survives_settings_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let mut s = Settings { onboarding_completed: true, ..Settings::default() };
        s.recording_enabled = false;
        s.pause_until = Some(1_800_000_000);
        s.save(&path).unwrap();
        let back = Settings::load(&path);
        assert_eq!(back.pause_until, Some(1_800_000_000));
        assert!(!back.recording_enabled);
        assert_eq!(due(&back, 1_800_000_001), Due::Resume);
    }
}
