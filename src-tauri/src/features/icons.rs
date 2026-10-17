//! App and site icons next to app and site names (Dayflow: `Core/Net/FaviconService.swift` and
//! "Show app/website icons in timeline" in `Views/UI/Settings/SettingsOtherTabView.swift`).
//!
//! Apps. The recorder stores the full path of the executable in front with each screenshot
//! (`screenshots.app_path`, never for a private app). The first time a program is in front, and
//! again when its path changes, its icon is drawn into a 32×32 PNG in
//! `<data>/icons/apps/<exe>.png` (Windows: SHDefExtractIconW, GetIconInfo and GetDIBits), and the
//! file description and product name of its version resource are kept, so a model's label such as
//! "Visual Studio Code" finds `Code.exe`. Other platforms extract nothing and show letters.
//!
//! Sites. Dayflow races Google's favicon service against the site; that service would learn every
//! domain on the timeline, so Daytrace uses none. With "Look up site icons" on (off by default)
//! Daytrace asks the site itself for `https://<host>/favicon.ico`: 4 s timeout, 200 KB cap,
//! decoded as ICO, PNG, BMP or JPEG of at most 512 px and drawn again as a 32×32 PNG, so only
//! pixels reach the window. Otherwise the window shows a letter badge.
//!
//! The window asks by name (`icons_lookup`) and gets PNG data URLs, never a path. Cleanup runs
//! with the hourly retention pass: an app icon goes a year after the program was last in front
//! (screenshot rows with program names are kept that long) and at once when the app is private;
//! a site icon 90 days after its last use, and all of them when the lookup is off. At most 400
//! apps and 500 sites are kept.

use crate::config::Settings;
use crate::platform::blocked_match;
use crate::state::AppState;
use crate::timeutil::now_ts;
use base64::Engine;
use image::RgbaImage;
use rusqlite::OptionalExtension;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

type S<'a> = tauri::State<'a, Arc<AppState>>;

/// Edge of the stored icons in pixels (16 px marks at 200 % scale).
pub const SIZE: u32 = 32;
const MAX_APPS: usize = 400;
const MAX_SITES: usize = 500;
/// App icons are kept as long as the screenshot rows that name the program.
const APP_KEEP_SECS: i64 = crate::db::SCREENSHOT_ROW_DAYS * 86400;
const SITE_KEEP_SECS: i64 = 90 * 86400;
/// A program without an icon is tried again after a day (same path), a site after a week.
const APP_RETRY_SECS: i64 = 86400;
const SITE_RETRY_SECS: i64 = 7 * 86400;
/// Last-seen and last-used times are written at most this often.
const TOUCH_SAVE_SECS: i64 = 15 * 60;
/// A name nothing was found for is not looked up in the screenshot rows again for this long.
const MISS_SECS: i64 = 3600;
/// Screenshot rows searched for the path of a program that has no icon yet.
const BACKFILL_SECS: i64 = 30 * 86400;
const MAX_FAVICON_BYTES: usize = 200 * 1024;
const MAX_FAVICON_PX: u32 = 512;
const MAX_PNG_BYTES: usize = 64 * 1024;
const MAX_NAMES: usize = 100;
const MAX_NAME_LEN: usize = 200;

// ---- names ----

/// Lowercase letters and digits of a name: "Visual Studio Code" → "visualstudiocode",
/// "ms-teams" → "msteams".
pub fn norm(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// Cache key of an executable stem.
fn app_key(exe: &str) -> Option<String> {
    let k: String = norm(exe).chars().take(64).collect();
    (!k.is_empty()).then_some(k)
}

/// File stem of a Windows or Unix path: `C:\Program Files\App\Code.exe` → `Code`.
pub fn exe_stem(path: &str) -> &str {
    let file = path.rsplit(['\\', '/']).next().unwrap_or(path);
    match file.rfind('.') {
        Some(i) if i > 0 => &file[..i],
        _ => file,
    }
}

/// Suffixes that never name a public website.
const LOCAL_SUFFIXES: &[&str] =
    &["local", "localhost", "localdomain", "lan", "internal", "intranet", "home", "corp", "private", "test", "example", "invalid", "arpa", "onion"];

/// The host when a name is a web address ("github.com", "https://docs.rs/serde",
/// "www.youtube.com"), else None. Only names that look public: no IP address, port, user name,
/// single label, or local or reserved suffix.
pub fn site_host(name: &str) -> Option<String> {
    let lower = name.trim().to_lowercase();
    let rest = lower.strip_prefix("https://").or_else(|| lower.strip_prefix("http://")).unwrap_or(&lower);
    let host = rest.split(['/', '?', '#']).next()?.trim_end_matches('.');
    if host.is_empty() || host.len() > 253 {
        return None;
    }
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 {
        return None;
    }
    let label_ok = |l: &&str| {
        !l.is_empty() && l.len() <= 63 && !l.starts_with('-') && !l.ends_with('-') && l.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    };
    if !labels.iter().all(label_ok) {
        return None;
    }
    let tld = labels[labels.len() - 1];
    // A letters-only suffix also turns away IPv4 addresses.
    if tld.len() < 2 || !tld.chars().all(|c| c.is_ascii_lowercase()) || LOCAL_SUFFIXES.contains(&tld) {
        return None;
    }
    Some(host.to_string())
}

/// Windows refuses these as file names, with any extension.
const RESERVED: &[&str] = &[
    "con", "prn", "aux", "nul", "com0", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8", "com9", "lpt0", "lpt1", "lpt2", "lpt3",
    "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// A file name for a key (an app key or a host) that is safe on Windows.
fn file_stem(key: &str) -> String {
    if !key.is_ascii() {
        return format!("u_{}", key.bytes().map(|b| format!("{b:02x}")).collect::<String>());
    }
    if RESERVED.contains(&key.split('.').next().unwrap_or("")) { format!("_{key}") } else { key.to_string() }
}

/// Common names of programs whose executable is called something else (Windows).
const ALIASES: &[(&str, &[&str])] = &[
    ("vscode", &["code"]),
    ("visualstudiocode", &["code"]),
    ("edge", &["msedge"]),
    ("microsoftedge", &["msedge"]),
    ("googlechrome", &["chrome"]),
    ("mozillafirefox", &["firefox"]),
    ("terminal", &["windowsterminal"]),
    ("fileexplorer", &["explorer"]),
    ("windowsexplorer", &["explorer"]),
    ("word", &["winword"]),
    ("microsoftword", &["winword"]),
    ("microsoftexcel", &["excel"]),
    ("powerpoint", &["powerpnt"]),
    ("microsoftpowerpoint", &["powerpnt"]),
    ("outlook", &["outlook", "olk"]),
    ("microsoftoutlook", &["outlook", "olk"]),
    ("teams", &["msteams", "teams"]),
    ("microsoftteams", &["msteams", "teams"]),
    ("microsoftonenote", &["onenote"]),
    ("commandprompt", &["cmd"]),
    ("powershell", &["pwsh", "powershell"]),
    ("windowspowershell", &["powershell"]),
    ("settings", &["systemsettings"]),
    ("windowssettings", &["systemsettings"]),
    ("taskmanager", &["taskmgr"]),
    ("visualstudio", &["devenv"]),
    ("intellij", &["idea64"]),
    ("intellijidea", &["idea64"]),
    ("pycharm", &["pycharm64"]),
    ("androidstudio", &["studio64"]),
    ("obs", &["obs64"]),
    ("obsstudio", &["obs64"]),
    ("paint", &["mspaint"]),
    ("remotedesktop", &["mstsc"]),
    ("remotedesktopconnection", &["mstsc"]),
    ("ciscopackettracer", &["packettracer"]),
    ("vmwareworkstation", &["vmware"]),
    ("gitbash", &["mintty"]),
    ("acrobat", &["acrobat", "acrord32"]),
    ("adobeacrobat", &["acrobat", "acrord32"]),
    ("acrobatreader", &["acrord32", "acrobat"]),
    ("adobeacrobatreader", &["acrord32", "acrobat"]),
    ("calculator", &["calculatorapp", "calc"]),
];

/// The cached program a name stands for: its executable stem, a common name for it, the file
/// description or product name in its version resource, then the longest stem of at least 4
/// letters the name contains (or that contains the name). Ties go to the program in front last.
fn resolve_app<'a>(apps: &'a BTreeMap<String, AppEntry>, name: &str) -> Option<&'a str> {
    let n = norm(name);
    if n.is_empty() {
        return None;
    }
    if let Some((k, _)) = apps.get_key_value(&n) {
        return Some(k);
    }
    if let Some((_, keys)) = ALIASES.iter().find(|(a, _)| *a == n) {
        if let Some((k, _)) = keys.iter().find_map(|k| apps.get_key_value(*k)) {
            return Some(k);
        }
    }
    let latest = |it: &mut dyn Iterator<Item = (&'a String, &'a AppEntry)>| it.max_by_key(|(k, e)| (k.len(), e.seen)).map(|(k, _)| k.as_str());
    if let Some(k) = latest(&mut apps.iter().filter(|(_, e)| e.names.iter().any(|x| norm(x) == n))) {
        return Some(k);
    }
    latest(&mut apps.iter().filter(|(k, _)| (k.chars().count() >= 4 && n.contains(k.as_str())) || (n.chars().count() >= 4 && k.contains(&n))))
}

// ---- cache ----

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
struct AppEntry {
    /// Full path of the executable last seen in front (stays on this PC).
    path: String,
    /// File description and product name from the version resource.
    names: Vec<String>,
    /// Last time the program was in front.
    seen: i64,
    /// Last time its icon was extracted.
    tried: i64,
    /// The PNG exists.
    icon: bool,
    /// A black mark on a transparent background: drawn light in dark mode.
    tint: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
struct SiteEntry {
    /// The PNG exists.
    ok: bool,
    /// Last request to the site.
    fetched: i64,
    /// Last time the window showed it.
    used: i64,
    tint: bool,
}

#[derive(Serialize, Deserialize)]
#[serde(default)]
struct IndexFile<T> {
    version: u32,
    entries: BTreeMap<String, T>,
}

impl<T> Default for IndexFile<T> {
    fn default() -> Self {
        IndexFile { version: 1, entries: BTreeMap::new() }
    }
}

struct Store<T> {
    entries: BTreeMap<String, T>,
    saved_at: i64,
}

struct Cache {
    dir: PathBuf,
    apps: Mutex<Store<AppEntry>>,
    sites: Mutex<Store<SiteEntry>>,
    /// Extractions and lookups running now ("app:<key>", "site:<host>").
    busy: Mutex<HashSet<String>>,
    /// Names no program matched, with the time (not saved).
    misses: Mutex<HashMap<String, i64>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The cache of a data folder (one per folder; tests use several).
fn cache(root: &Path) -> Arc<Cache> {
    static CACHES: OnceLock<Mutex<Vec<(PathBuf, Arc<Cache>)>>> = OnceLock::new();
    let mut all = lock(CACHES.get_or_init(Default::default));
    if let Some((_, c)) = all.iter().find(|(r, _)| r == root) {
        return c.clone();
    }
    let c = Arc::new(Cache::load(root.join("icons")));
    all.push((root.to_path_buf(), c.clone()));
    c
}

fn read_index<T: DeserializeOwned>(path: &Path) -> BTreeMap<String, T> {
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice::<IndexFile<T>>(&b).ok()).map(|f| f.entries).unwrap_or_default()
}

/// Writes through a temporary file and a rename, so a crash leaves the old file or the new one.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes).and_then(|_| std::fs::rename(&tmp, path)).inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// Drops the entries past `max`, oldest `time` first; returns their keys.
fn evict<T>(entries: &mut BTreeMap<String, T>, max: usize, time: impl Fn(&T) -> i64) -> Vec<String> {
    if entries.len() <= max {
        return Vec::new();
    }
    let mut by_age: Vec<(i64, String)> = entries.iter().map(|(k, e)| (time(e), k.clone())).collect();
    by_age.sort();
    let gone: Vec<String> = by_age.into_iter().take(entries.len() - max).map(|(_, k)| k).collect();
    for k in &gone {
        entries.remove(k);
    }
    gone
}

impl Cache {
    fn load(dir: PathBuf) -> Cache {
        Cache {
            apps: Mutex::new(Store { entries: read_index(&dir.join("apps.json")), saved_at: 0 }),
            sites: Mutex::new(Store { entries: read_index(&dir.join("sites.json")), saved_at: 0 }),
            busy: Mutex::new(HashSet::new()),
            misses: Mutex::new(HashMap::new()),
            dir,
        }
    }

    fn app_png(&self, key: &str) -> PathBuf {
        self.dir.join("apps").join(format!("{}.png", file_stem(key)))
    }

    fn site_png(&self, host: &str) -> PathBuf {
        self.dir.join("sites").join(format!("{}.png", file_stem(host)))
    }

    fn save<T: Serialize>(&self, file: &str, store: &mut Store<T>, now: i64) {
        #[derive(Serialize)]
        struct Out<'a, T> {
            version: u32,
            entries: &'a BTreeMap<String, T>,
        }
        store.saved_at = now;
        match serde_json::to_vec(&Out { version: 1, entries: &store.entries }) {
            Ok(b) => {
                if let Err(e) = write_atomic(&self.dir.join(file), &b) {
                    log::warn!("icons: could not save {file}: {e}");
                }
            }
            Err(e) => log::warn!("icons: could not save {file}: {e}"),
        }
    }

    /// Marks work as running; false when it already is.
    fn claim(&self, tag: &str) -> bool {
        lock(&self.busy).insert(tag.to_string())
    }

    fn release(&self, tag: &str) {
        lock(&self.busy).remove(tag);
    }

    fn is_busy(&self, tag: &str) -> bool {
        lock(&self.busy).contains(tag)
    }

    /// Stores an extracted icon (None: the program has none) with the names from its version
    /// resource.
    fn finish_app(&self, key: &str, path: &str, img: Option<RgbaImage>, names: Vec<String>, now: i64) {
        let png = img.and_then(to_png);
        let file = self.app_png(key);
        let icon = match &png {
            Some((bytes, _)) => match write_atomic(&file, bytes) {
                Ok(()) => true,
                Err(e) => {
                    log::warn!("icons: could not write {}: {e}", file.display());
                    false
                }
            },
            None => {
                let _ = std::fs::remove_file(&file);
                false
            }
        };
        let mut apps = lock(&self.apps);
        let e = apps.entries.entry(key.to_string()).or_default();
        *e = AppEntry { path: path.to_string(), names, seen: now, tried: now, icon, tint: icon && png.is_some_and(|p| p.1) };
        for k in evict(&mut apps.entries, MAX_APPS, |e| e.seen) {
            let _ = std::fs::remove_file(self.app_png(&k));
        }
        self.save("apps.json", &mut apps, now);
        drop(apps);
        lock(&self.misses).clear();
    }

    /// Stores a fetched favicon (None: the site gave nothing usable).
    fn finish_site(&self, host: &str, img: Option<RgbaImage>, now: i64) {
        let png = img.and_then(to_png);
        let file = self.site_png(host);
        let ok = match &png {
            Some((bytes, _)) => write_atomic(&file, bytes).is_ok(),
            None => false,
        };
        let mut sites = lock(&self.sites);
        sites.entries.insert(host.to_string(), SiteEntry { ok, fetched: now, used: now, tint: ok && png.is_some_and(|p| p.1) });
        for k in evict(&mut sites.entries, MAX_SITES, |e| e.used) {
            let _ = std::fs::remove_file(self.site_png(&k));
        }
        self.save("sites.json", &mut sites, now);
    }

    /// Forgets every site icon (the lookup was turned off).
    fn clear_sites(&self) {
        let mut sites = lock(&self.sites);
        let had = !sites.entries.is_empty();
        sites.entries.clear();
        let dir = self.dir.join("sites");
        if had || dir.exists() || self.dir.join("sites.json").exists() {
            let _ = std::fs::remove_dir_all(&dir);
            let _ = std::fs::remove_file(self.dir.join("sites.json"));
            log::info!("icons: site icons removed (lookup is off)");
        }
    }

    fn read_png(path: &Path) -> Option<Vec<u8>> {
        let b = std::fs::read(path).ok()?;
        (b.len() <= MAX_PNG_BYTES && b.starts_with(b"\x89PNG\r\n\x1a\n")).then_some(b)
    }

    fn cleanup(&self, settings: &Settings, now: i64) {
        let blocked = &settings.blocked_apps;
        {
            let mut apps = lock(&self.apps);
            let mut gone: Vec<String> = apps
                .entries
                .iter()
                .filter(|(k, e)| {
                    now - e.seen > APP_KEEP_SECS || blocked_match(k, blocked).is_some() || (!e.path.is_empty() && blocked_match(exe_stem(&e.path), blocked).is_some())
                })
                .map(|(k, _)| k.clone())
                .collect();
            for k in &gone {
                apps.entries.remove(k);
            }
            gone.extend(evict(&mut apps.entries, MAX_APPS, |e| e.seen));
            for k in &gone {
                let _ = std::fs::remove_file(self.app_png(k));
            }
            if !gone.is_empty() {
                self.save("apps.json", &mut apps, now);
            }
            let keep: HashSet<String> = apps.entries.iter().filter(|(_, e)| e.icon).map(|(k, _)| format!("{}.png", file_stem(k))).collect();
            remove_strays(&self.dir.join("apps"), &keep);
        }
        if !settings.lookup_site_icons {
            self.clear_sites();
            return;
        }
        let mut sites = lock(&self.sites);
        let mut gone: Vec<String> = sites
            .entries
            .iter()
            .filter(|(_, e)| if e.ok { now - e.used > SITE_KEEP_SECS } else { now - e.fetched > SITE_RETRY_SECS })
            .map(|(k, _)| k.clone())
            .collect();
        for k in &gone {
            sites.entries.remove(k);
        }
        gone.extend(evict(&mut sites.entries, MAX_SITES, |e| e.used));
        for k in &gone {
            let _ = std::fs::remove_file(self.site_png(k));
        }
        if !gone.is_empty() {
            self.save("sites.json", &mut sites, now);
        }
        let keep: HashSet<String> = sites.entries.iter().filter(|(_, e)| e.ok).map(|(k, _)| format!("{}.png", file_stem(k))).collect();
        remove_strays(&self.dir.join("sites"), &keep);
    }
}

/// Removes files in `dir` that are not in `keep`, except ones written in the last 10 minutes
/// (an extraction or lookup may be finishing).
fn remove_strays(dir: &Path, keep: &HashSet<String>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let recent = |e: &std::fs::DirEntry| e.metadata().and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_none_or(|age| age.as_secs() < 600);
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !keep.contains(&name) && e.file_type().is_ok_and(|t| t.is_file()) && !recent(&e) {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

// ---- pixels ----

/// Fits an image into SIZE×SIZE (centered, aspect kept) and encodes it as PNG; also says whether
/// it is a dark silhouette. None for an empty or fully transparent image.
fn to_png(img: RgbaImage) -> Option<(Vec<u8>, bool)> {
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 || img.pixels().all(|p| p.0[3] == 0) {
        return None;
    }
    let fitted = if (w, h) == (SIZE, SIZE) {
        img
    } else {
        let scale = SIZE as f32 / w.max(h) as f32;
        let (nw, nh) = (((w as f32 * scale).round() as u32).clamp(1, SIZE), ((h as f32 * scale).round() as u32).clamp(1, SIZE));
        let small = image::imageops::resize(&img, nw, nh, image::imageops::FilterType::Lanczos3);
        let mut canvas = RgbaImage::new(SIZE, SIZE);
        image::imageops::overlay(&mut canvas, &small, ((SIZE - nw) / 2) as i64, ((SIZE - nh) / 2) as i64);
        canvas
    };
    let tint = is_dark_silhouette(&fitted);
    let mut out = Vec::new();
    image::DynamicImage::ImageRgba8(fitted).write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).ok()?;
    Some((out, tint))
}

/// Dayflow's FaviconContrast rule (FaviconService.swift): a mark whose visible pixels are all
/// near black, with at least a fifth of the image transparent around it. Such marks disappear on
/// a dark surface, so the window draws them light there; colored icons and finished app tiles
/// keep their own colors.
fn is_dark_silhouette(img: &RgbaImage) -> bool {
    let total = (img.width() * img.height()).max(1) as f32;
    let (mut transparent, mut visible) = (0u32, 0u32);
    for p in img.pixels() {
        let [r, g, b, a] = p.0;
        if a as f32 / 255.0 <= 0.05 {
            transparent += 1;
        }
        if (a as f32 / 255.0) < 0.5 {
            continue;
        }
        let hi = r.max(g).max(b) as f32 / 255.0;
        let lo = r.min(g).min(b) as f32 / 255.0;
        if hi > 0.25 || hi - lo > 0.04 {
            return false;
        }
        visible += 1;
    }
    visible > 0 && transparent as f32 / total >= 0.2
}

/// Decodes a downloaded favicon: ICO, PNG, BMP or JPEG, at most 512 px a side.
pub fn decode_favicon(bytes: &[u8]) -> Option<RgbaImage> {
    if bytes.is_empty() || bytes.len() > MAX_FAVICON_BYTES {
        return None;
    }
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?;
    if !matches!(reader.format(), Some(image::ImageFormat::Ico | image::ImageFormat::Png | image::ImageFormat::Bmp | image::ImageFormat::Jpeg)) {
        return None;
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_FAVICON_PX);
    limits.max_image_height = Some(MAX_FAVICON_PX);
    limits.max_alloc = Some(16 << 20);
    reader.limits(limits);
    let img = reader.decode().ok()?;
    (img.width() > 0 && img.height() > 0).then(|| img.to_rgba8())
}

// ---- apps ----

/// Called by the recorder after each screenshot with a program in front (not for private apps):
/// extracts the program's icon the first time, or when its path changed.
pub fn note_foreground(state: &AppState, settings: &Settings, app: &str, path: &str) {
    if !settings.show_icons || path.is_empty() || blocked_match(app, &settings.blocked_apps).is_some() {
        return;
    }
    let Some(key) = app_key(app) else { return };
    let cache = cache(&state.paths.root);
    let now = now_ts();
    let mut apps = lock(&cache.apps);
    let current = apps.entries.get_mut(&key).filter(|e| e.path == path && (e.icon || now - e.tried < APP_RETRY_SECS));
    match current {
        Some(e) => {
            e.seen = now;
            if now - apps.saved_at > TOUCH_SAVE_SECS {
                cache.save("apps.json", &mut apps, now);
            }
        }
        None => {
            drop(apps);
            start_extract(&cache, key, path.to_string());
        }
    }
}

/// Extracts a program's icon on a thread of its own (a slow drive must not hold up the recorder).
fn start_extract(cache: &Arc<Cache>, key: String, path: String) {
    let tag = format!("app:{key}");
    if !cache.claim(&tag) {
        return;
    }
    let c = cache.clone();
    let t = tag.clone();
    let spawned = std::thread::Builder::new().name("app-icon".into()).spawn(move || {
        let img = sys::exe_icon(&path);
        let names = sys::exe_names(&path);
        c.finish_app(&key, &path, img, names, now_ts());
        c.release(&t);
    });
    if spawned.is_err() {
        cache.release(&tag);
    }
}

/// The newest stored path of a program seen in front recently (never a private app's row).
fn recent_path(state: &AppState, app: &str, now: i64) -> Option<String> {
    let c = state.db.conn();
    c.query_row(
        "SELECT app_path FROM screenshots WHERE captured_at >= ?1 AND app_path IS NOT NULL AND app_path != '' AND hidden_app IS NULL
         AND app_name = ?2 COLLATE NOCASE ORDER BY captured_at DESC LIMIT 1",
        rusqlite::params![now - BACKFILL_SECS, app],
        |r| r.get::<_, String>(0),
    )
    .optional()
    .ok()
    .flatten()
}

// ---- lookup ----

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct IconDto {
    /// `data:image/png;base64,…`, 32×32.
    pub src: String,
    /// A dark mark on a transparent background; the window draws it light in dark mode.
    pub tint: bool,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IconLookup {
    /// Every name asked for: its icon, or null for a letter badge.
    pub icons: HashMap<String, Option<IconDto>>,
    /// Names whose icon is being made or fetched now; ask again in a few seconds.
    pub pending: Vec<String>,
}

enum Found {
    Icon(IconDto),
    Pending,
    Nothing,
}

fn dto(png: Vec<u8>, tint: bool) -> IconDto {
    IconDto { src: format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(png)), tint }
}

/// Answers names (program names, model labels such as "Visual Studio Code", site names) with
/// icons. Returns the hosts to fetch, which the command starts.
fn lookup(state: &AppState, names: &[String], now: i64) -> (IconLookup, Vec<String>) {
    let settings = state.settings();
    let mut out = IconLookup::default();
    let mut fetch = Vec::new();
    let mut seen = HashSet::new();
    let names: Vec<&str> =
        names.iter().map(|n| n.trim()).filter(|n| !n.is_empty() && n.len() <= MAX_NAME_LEN).filter(|n| seen.insert(n.to_string())).take(MAX_NAMES).collect();
    if !settings.show_icons {
        out.icons = names.iter().map(|n| (n.to_string(), None)).collect();
        return (out, fetch);
    }
    let cache = cache(&state.paths.root);
    if !settings.lookup_site_icons {
        cache.clear_sites();
    }
    for name in names {
        let found = if blocked_match(name, &settings.blocked_apps).is_some() {
            Found::Nothing
        } else if let Some(host) = site_host(name) {
            if settings.lookup_site_icons { site(&cache, &host, now, &mut fetch) } else { Found::Nothing }
        } else {
            app(state, &cache, &settings, name, now)
        };
        let icon = match found {
            Found::Icon(i) => Some(i),
            Found::Pending => {
                out.pending.push(name.to_string());
                None
            }
            Found::Nothing => None,
        };
        out.icons.insert(name.to_string(), icon);
    }
    (out, fetch)
}

fn site(cache: &Arc<Cache>, host: &str, now: i64, fetch: &mut Vec<String>) -> Found {
    let mut sites = lock(&cache.sites);
    match sites.entries.get(host).cloned() {
        Some(e) if e.ok => {
            if let Some(png) = Cache::read_png(&cache.site_png(host)) {
                // Last use, kept to the day (cleanup drops icons unused for 90 days).
                if now - e.used > 86400 {
                    if let Some(x) = sites.entries.get_mut(host) {
                        x.used = now;
                    }
                    cache.save("sites.json", &mut sites, now);
                }
                return Found::Icon(dto(png, e.tint));
            }
        }
        Some(e) if now - e.fetched < SITE_RETRY_SECS => return Found::Nothing,
        _ => {}
    }
    drop(sites);
    if !cache.is_busy(&format!("site:{host}")) {
        fetch.push(host.to_string());
    }
    Found::Pending
}

fn app(state: &AppState, cache: &Arc<Cache>, settings: &Settings, name: &str, now: i64) -> Found {
    let apps = lock(&cache.apps);
    if let Some(key) = resolve_app(&apps.entries, name) {
        let e = &apps.entries[key];
        if !e.path.is_empty() && blocked_match(exe_stem(&e.path), &settings.blocked_apps).is_some() {
            return Found::Nothing;
        }
        if e.icon {
            if let Some(png) = Cache::read_png(&cache.app_png(key)) {
                return Found::Icon(dto(png, e.tint));
            }
        }
        let key = key.to_string();
        if cache.is_busy(&format!("app:{key}")) {
            return Found::Pending;
        }
        // The PNG went missing, or the last try is a day old: try again.
        if !e.path.is_empty() && (e.icon || now - e.tried >= APP_RETRY_SECS) {
            let path = e.path.clone();
            drop(apps);
            start_extract(cache, key, path);
            return Found::Pending;
        }
        return Found::Nothing;
    }
    drop(apps);
    // A program that was in front before icons were on (or before this column existed): find
    // its path in the screenshot rows.
    if lock(&cache.misses).get(name).is_some_and(|t| now - t < MISS_SECS) {
        return Found::Nothing;
    }
    if let (Some(key), Some(path)) = (app_key(name), recent_path(state, name, now)) {
        if blocked_match(exe_stem(&path), &settings.blocked_apps).is_none() {
            start_extract(cache, key, path);
            return Found::Pending;
        }
    }
    lock(&cache.misses).insert(name.to_string(), now);
    Found::Nothing
}

// ---- site fetch ----

fn http() -> Option<&'static reqwest::Client> {
    static CLIENT: OnceLock<Option<reqwest::Client>> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(4))
                .connect_timeout(std::time::Duration::from_secs(3))
                // Up to 3 redirects, to https addresses that pass the same check as the site.
                .redirect(reqwest::redirect::Policy::custom(|a| {
                    let ok = a.previous().len() < 3 && a.url().scheme() == "https" && a.url().host_str().and_then(site_host).is_some();
                    if ok { a.follow() } else { a.stop() }
                }))
                .user_agent(concat!("Daytrace/", env!("CARGO_PKG_VERSION")))
                .build()
                .map_err(|e| log::warn!("icons: no HTTP client: {e}"))
                .ok()
        })
        .as_ref()
}

/// GET https://<host>/favicon.ico: no cookies, image types accepted, 200 KB at most.
async fn download(host: &str) -> Option<Vec<u8>> {
    let client = http()?;
    let mut resp = client.get(format!("https://{host}/favicon.ico")).header(reqwest::header::ACCEPT, "image/*").send().await.ok()?;
    if !resp.status().is_success() || resp.content_length().is_some_and(|n| n as usize > MAX_FAVICON_BYTES) {
        return None;
    }
    let mut body = Vec::new();
    while let Some(chunk) = resp.chunk().await.ok()? {
        body.extend_from_slice(&chunk);
        if body.len() > MAX_FAVICON_BYTES {
            return None;
        }
    }
    Some(body)
}

async fn fetch_site(cache: Arc<Cache>, host: String) {
    static PERMITS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(4);
    let tag = format!("site:{host}");
    if !cache.claim(&tag) {
        return;
    }
    let img = match PERMITS.acquire().await {
        Ok(_permit) => match download(&host).await {
            Some(bytes) => tauri::async_runtime::spawn_blocking(move || decode_favicon(&bytes)).await.ok().flatten(),
            None => None,
        },
        Err(_) => None,
    };
    let c = cache.clone();
    let h = host.clone();
    let _ = tauri::async_runtime::spawn_blocking(move || c.finish_site(&h, img, now_ts())).await;
    cache.release(&tag);
}

// ---- commands ----

/// Icons for names: program names (Daily and Weekly "Programs in front"), the app and site
/// labels of cards, and site names. Data URLs only; paths never leave the backend.
#[tauri::command]
pub async fn icons_lookup(state: S<'_>, names: Vec<String>) -> Result<IconLookup, String> {
    let st = state.inner().clone();
    let (out, fetch) = tauri::async_runtime::spawn_blocking(move || lookup(&st, &names, now_ts())).await.map_err(|e| e.to_string())?;
    if !fetch.is_empty() {
        let c = cache(&state.paths.root);
        for host in fetch {
            tauri::async_runtime::spawn(fetch_site(c.clone(), host));
        }
    }
    Ok(out)
}

/// Retention pass (analysis.rs): see the module comment.
pub fn cleanup(root: &Path, settings: &Settings, now: i64) {
    if root.join("icons").exists() {
        cache(root).cleanup(settings, now);
    }
}

// ---- Windows ----

#[cfg(windows)]
mod sys {
    use super::SIZE;
    use image::RgbaImage;
    use windows::Win32::Foundation::S_OK;
    use windows::Win32::Graphics::Gdi::{
        BI_RGB, BITMAP, BITMAPINFO, BITMAPINFOHEADER, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDIBits, GetObjectW, HBITMAP, HDC, HGDIOBJ,
    };
    use windows::Win32::Storage::FileSystem::{GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW};
    use windows::Win32::UI::Shell::SHDefExtractIconW;
    use windows::Win32::UI::WindowsAndMessaging::{DestroyIcon, GetIconInfo, HICON, ICONINFO};
    use windows::core::HSTRING;

    /// The executable's first icon at SIZE×SIZE, as straight RGBA.
    pub fn exe_icon(path: &str) -> Option<RgbaImage> {
        unsafe {
            let mut large = HICON::default();
            // nIconSize: low word the large size, high word the small size (not asked for).
            let hr = SHDefExtractIconW(&HSTRING::from(path), 0, 0, Some(&mut large), None, SIZE);
            if hr != S_OK || large.is_invalid() {
                return None;
            }
            let img = icon_rgba(large);
            let _ = DestroyIcon(large);
            img
        }
    }

    unsafe fn icon_rgba(icon: HICON) -> Option<RgbaImage> {
        unsafe {
            let mut info = ICONINFO::default();
            GetIconInfo(icon, &mut info).ok()?;
            let dc = CreateCompatibleDC(None);
            let out = if dc.is_invalid() || info.hbmColor.is_invalid() { None } else { color_and_mask(dc, info.hbmColor, info.hbmMask) };
            if !dc.is_invalid() {
                let _ = DeleteDC(dc);
            }
            for b in [info.hbmColor, info.hbmMask] {
                if !b.is_invalid() {
                    let _ = DeleteObject(HGDIOBJ::from(b));
                }
            }
            out
        }
    }

    unsafe fn color_and_mask(dc: HDC, color: HBITMAP, mask: HBITMAP) -> Option<RgbaImage> {
        unsafe {
            let mut bm = BITMAP::default();
            if GetObjectW(HGDIOBJ::from(color), std::mem::size_of::<BITMAP>() as i32, Some(&mut bm as *mut BITMAP as *mut core::ffi::c_void)) == 0 {
                return None;
            }
            let (w, h) = (bm.bmWidth, bm.bmHeight.abs());
            if w <= 0 || h <= 0 || w > 256 || h > 256 {
                return None;
            }
            let mut px = dib(dc, color, w, h)?;
            // Icons without an alpha channel: the AND mask says what is transparent (white).
            if px.chunks_exact(4).all(|p| p[3] == 0) {
                let m = if mask.is_invalid() { None } else { dib(dc, mask, w, h) };
                for (i, p) in px.chunks_exact_mut(4).enumerate() {
                    p[3] = match &m {
                        Some(m) if m[i * 4] != 0 => 0,
                        _ => 255,
                    };
                }
            }
            // BGRA → RGBA.
            for p in px.chunks_exact_mut(4) {
                p.swap(0, 2);
            }
            RgbaImage::from_raw(w as u32, h as u32, px)
        }
    }

    /// The bitmap as 32-bit top-down BGRA.
    unsafe fn dib(dc: HDC, bmp: HBITMAP, w: i32, h: i32) -> Option<Vec<u8>> {
        unsafe {
            let mut bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut buf = vec![0u8; (w * h * 4) as usize];
            let lines = GetDIBits(dc, bmp, 0, h as u32, Some(buf.as_mut_ptr() as *mut core::ffi::c_void), &mut bmi, DIB_RGB_COLORS);
            (lines == h).then_some(buf)
        }
    }

    /// File description and product name from the version resource ("Visual Studio Code").
    pub fn exe_names(path: &str) -> Vec<String> {
        unsafe {
            let file = HSTRING::from(path);
            let size = GetFileVersionInfoSizeW(&file, None);
            if size == 0 || size > (1 << 20) {
                return Vec::new();
            }
            let mut data = vec![0u8; size as usize];
            if GetFileVersionInfoW(&file, None, size, data.as_mut_ptr() as *mut core::ffi::c_void).is_err() {
                return Vec::new();
            }
            let block = data.as_ptr() as *const core::ffi::c_void;
            let mut ptr: *mut core::ffi::c_void = std::ptr::null_mut();
            let mut len: u32 = 0;
            let mut langs: Vec<(u16, u16)> = Vec::new();
            if VerQueryValueW(block, &HSTRING::from("\\VarFileInfo\\Translation"), &mut ptr, &mut len).as_bool() && !ptr.is_null() {
                let pairs = std::slice::from_raw_parts(ptr as *const u16, (len as usize / 4) * 2);
                langs.extend(pairs.chunks_exact(2).map(|p| (p[0], p[1])));
            }
            // US English, Unicode and Western code pages, for files without a translation table.
            langs.extend([(0x0409, 0x04B0), (0x0409, 0x04E4)]);
            let mut out = Vec::new();
            for field in ["FileDescription", "ProductName"] {
                for (lang, cp) in &langs {
                    let sub = HSTRING::from(format!("\\StringFileInfo\\{lang:04x}{cp:04x}\\{field}"));
                    let mut p: *mut core::ffi::c_void = std::ptr::null_mut();
                    let mut n: u32 = 0;
                    if VerQueryValueW(block, &sub, &mut p, &mut n).as_bool() && !p.is_null() && n > 0 {
                        let text = String::from_utf16_lossy(std::slice::from_raw_parts(p as *const u16, n as usize));
                        let text = text.trim_end_matches('\0').trim().chars().take(120).collect::<String>();
                        if !text.is_empty() && !out.contains(&text) {
                            out.push(text);
                        }
                        break;
                    }
                }
            }
            out
        }
    }
}

#[cfg(not(windows))]
mod sys {
    use image::RgbaImage;

    /// No icons outside Windows: the window shows letters.
    pub fn exe_icon(_path: &str) -> Option<RgbaImage> {
        None
    }

    pub fn exe_names(_path: &str) -> Vec<String> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Paths;

    fn state() -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().unwrap();
        let st = AppState::new(Paths::new(dir.path().to_path_buf()).unwrap()).unwrap();
        (dir, st)
    }

    /// A 48×48 colored square on transparent, like an app icon.
    fn square(rgb: [u8; 3]) -> RgbaImage {
        RgbaImage::from_fn(48, 48, |x, y| if (8..40).contains(&x) && (8..40).contains(&y) { image::Rgba([rgb[0], rgb[1], rgb[2], 255]) } else { image::Rgba([0, 0, 0, 0]) })
    }

    fn entry(path: &str, names: &[&str], seen: i64) -> AppEntry {
        AppEntry { path: path.into(), names: names.iter().map(|s| s.to_string()).collect(), seen, tried: seen, icon: true, tint: false }
    }

    #[test]
    fn names_that_are_sites() {
        assert_eq!(site_host("github.com").as_deref(), Some("github.com"));
        assert_eq!(site_host(" https://Docs.rs/serde/latest ").as_deref(), Some("docs.rs"));
        assert_eq!(site_host("www.youtube.com/watch?v=1").as_deref(), Some("www.youtube.com"));
        assert_eq!(site_host("learn.microsoft.com.").as_deref(), Some("learn.microsoft.com"));
        for not in [
            "Visual Studio Code",
            "Code",
            "localhost:3000",
            "192.168.1.1",
            "router.local",
            "bank.example",
            "user@mail.com",
            "-bad.com",
            "nas.lan",
            "",
            "a.b-",
        ] {
            assert_eq!(site_host(not), None, "{not}");
        }
    }

    #[test]
    fn stems_and_file_names() {
        assert_eq!(exe_stem(r"C:\Program Files\Microsoft VS Code\Code.exe"), "Code");
        assert_eq!(exe_stem("/usr/bin/foo.bar.exe"), "foo.bar");
        assert_eq!(exe_stem("plain"), "plain");
        assert_eq!(app_key("ms-teams").as_deref(), Some("msteams"));
        assert_eq!(app_key(" -_ "), None);
        assert_eq!(file_stem("code"), "code");
        assert_eq!(file_stem("con"), "_con");
        assert_eq!(file_stem("aux.example.com"), "_aux.example.com");
        assert_eq!(file_stem("ünï"), "u_c3bc6ec3af");
    }

    #[test]
    fn labels_find_programs() {
        let mut apps = BTreeMap::new();
        apps.insert("code".to_string(), entry(r"C:\VS Code\Code.exe", &["Visual Studio Code"], 10));
        apps.insert("msedge".to_string(), entry(r"C:\Edge\msedge.exe", &["Microsoft Edge"], 10));
        apps.insert("windowsterminal".to_string(), entry(r"C:\WT\WindowsTerminal.exe", &[], 10));
        apps.insert("claude".to_string(), entry(r"C:\Claude\claude.exe", &["Claude"], 10));
        apps.insert("packettracer".to_string(), entry(r"C:\Cisco\PacketTracer.exe", &["Cisco Packet Tracer"], 10));
        apps.insert("msteams".to_string(), entry(r"C:\Teams\ms-teams.exe", &["Microsoft Teams"], 10));
        let r = |n: &str| resolve_app(&apps, n).map(str::to_string);
        assert_eq!(r("Code").as_deref(), Some("code"), "the executable stem");
        assert_eq!(r("VS Code").as_deref(), Some("code"), "a common name");
        assert_eq!(r("Visual Studio Code").as_deref(), Some("code"), "the file description");
        assert_eq!(r("Microsoft Edge").as_deref(), Some("msedge"));
        assert_eq!(r("Edge").as_deref(), Some("msedge"));
        assert_eq!(r("Windows Terminal").as_deref(), Some("windowsterminal"));
        assert_eq!(r("Terminal").as_deref(), Some("windowsterminal"));
        assert_eq!(r("Cisco Packet Tracer").as_deref(), Some("packettracer"));
        assert_eq!(r("Teams").as_deref(), Some("msteams"));
        assert_eq!(r("Claude Code").as_deref(), Some("claude"), "the longest stem inside the name");
        assert_eq!(r("Notion"), None);
        assert_eq!(r("Event Viewer"), None);
        assert_eq!(r(""), None);
    }

    #[test]
    fn png_is_fitted_and_silhouettes_are_marked() {
        let (png, tint) = to_png(square([30, 120, 220])).unwrap();
        let back = image::load_from_memory(&png).unwrap();
        assert_eq!((back.width(), back.height()), (SIZE, SIZE));
        assert!(!tint, "a colored icon keeps its colors");
        let (_, tint) = to_png(square([10, 10, 10])).unwrap();
        assert!(tint, "a black mark on transparent is drawn light in dark mode");
        let full_black = RgbaImage::from_pixel(32, 32, image::Rgba([0, 0, 0, 255]));
        assert!(!to_png(full_black).unwrap().1, "a finished tile without clear space keeps its look");
        assert!(to_png(RgbaImage::new(16, 16)).is_none(), "a fully transparent image is no icon");
        // A wide image keeps its aspect: centered, transparent above and below.
        let wide = RgbaImage::from_pixel(64, 16, image::Rgba([200, 30, 30, 255]));
        let back = image::load_from_memory(&to_png(wide).unwrap().0).unwrap().to_rgba8();
        assert_eq!(back.get_pixel(16, 0).0[3], 0);
        assert_eq!(back.get_pixel(16, 16).0[3], 255);
    }

    #[test]
    fn favicons_are_validated() {
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(square([1, 2, 3])).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        assert!(decode_favicon(&png).is_some());
        let mut ico = Vec::new();
        image::DynamicImage::ImageRgba8(square([1, 2, 3])).write_to(&mut std::io::Cursor::new(&mut ico), image::ImageFormat::Ico).unwrap();
        assert_eq!(decode_favicon(&ico).map(|i| i.dimensions()), Some((48, 48)));
        assert!(decode_favicon(b"<!doctype html><title>Not found</title>").is_none(), "a soft 404 page");
        assert!(decode_favicon(b"").is_none());
        assert!(decode_favicon(&vec![0u8; MAX_FAVICON_BYTES + 1]).is_none(), "over the size cap");
        let mut big = Vec::new();
        image::DynamicImage::new_rgba8(600, 600).write_to(&mut std::io::Cursor::new(&mut big), image::ImageFormat::Png).unwrap();
        assert!(decode_favicon(&big).is_none(), "over 512 px");
        let gif = b"GIF89a\x01\x00\x01\x00\x00\x00\x00;";
        assert!(decode_favicon(gif).is_none(), "formats outside the list");
    }

    #[test]
    fn app_icons_are_served_by_label_and_never_for_private_apps() {
        let (_d, st) = state();
        let now = now_ts();
        let c = cache(&st.paths.root);
        c.finish_app("code", r"C:\VS Code\Code.exe", Some(square([30, 120, 220])), vec!["Visual Studio Code".into()], now);
        c.finish_app("keepassxc", r"C:\KeePassXC\KeePassXC.exe", Some(square([90, 160, 60])), vec![], now);
        c.finish_app("blank", r"C:\Blank\blank.exe", None, vec![], now);
        let names: Vec<String> = ["Code", "Visual Studio Code", "KeePassXC", "blank", "github.com", "Nothing here"].iter().map(|s| s.to_string()).collect();
        let (out, fetch) = lookup(&st, &names, now);
        let src = |n: &str| out.icons.get(n).cloned().flatten().map(|i| i.src);
        assert!(src("Code").is_some_and(|s| s.starts_with("data:image/png;base64,")));
        assert_eq!(src("Visual Studio Code"), src("Code"));
        assert_eq!(src("KeePassXC"), None, "private apps get no icon");
        assert_eq!(src("blank"), None, "no icon in the executable: a letter");
        assert_eq!(src("github.com"), None, "site lookups are off by default");
        assert!(fetch.is_empty() && out.pending.is_empty(), "nothing is fetched while the lookup is off");
        assert_eq!(out.icons.len(), names.len(), "every name gets an answer");
        let json = serde_json::to_string(&out).unwrap();
        assert!(!json.contains("Code.exe") && !json.contains(r"C:\"), "no paths reach the window");

        // The setting off: nothing at all.
        st.settings.write().unwrap().show_icons = false;
        let (out, _) = lookup(&st, &names, now);
        assert!(out.icons.values().all(|i| i.is_none()));
    }

    #[test]
    fn site_lookups_start_once_and_serve_the_cache() {
        let (_d, st) = state();
        let now = now_ts();
        st.settings.write().unwrap().lookup_site_icons = true;
        let names = vec!["github.com".to_string(), "docs.rs".to_string(), "bank.example".to_string()];
        let (out, fetch) = lookup(&st, &names, now);
        assert_eq!(fetch, vec!["github.com".to_string(), "docs.rs".to_string()], "only public names are fetched");
        assert_eq!(out.pending, fetch);
        let c = cache(&st.paths.root);
        c.finish_site("github.com", Some(square([20, 20, 20])), now);
        c.finish_site("docs.rs", None, now);
        let (out, fetch) = lookup(&st, &names, now + 10);
        assert!(fetch.is_empty(), "no new request for a cached icon or a recent failure");
        let gh = out.icons["github.com"].clone().unwrap();
        assert!(gh.tint, "a dark mark is drawn light in dark mode");
        assert_eq!(out.icons["docs.rs"], None);
        // A failure is tried again after a week.
        let (_, fetch) = lookup(&st, &names, now + SITE_RETRY_SECS + 1);
        assert_eq!(fetch, vec!["docs.rs".to_string()]);

        // Turning the lookup off removes the cached site icons.
        st.settings.write().unwrap().lookup_site_icons = false;
        let (out, fetch) = lookup(&st, &names, now + 20);
        assert!(fetch.is_empty() && out.icons["github.com"].is_none());
        assert!(!c.dir.join("sites").exists() && !c.dir.join("sites.json").exists());
    }

    #[test]
    fn recorder_paths_lead_to_icons_and_rows_backfill() {
        let (_d, st) = state();
        let now = now_ts();
        let settings = st.settings();
        // A row from the recorder with the path; a private app's row never has one.
        st.db.insert_screenshot_with_path(now - 60, "", 0, 0, Some("Obsidian"), None, None, None, Some(r"C:\Obsidian\Obsidian.exe")).unwrap();
        assert_eq!(recent_path(&st, "obsidian", now).as_deref(), Some(r"C:\Obsidian\Obsidian.exe"));
        assert_eq!(recent_path(&st, "Code", now), None);
        let (out, _) = lookup(&st, &["Obsidian".to_string()], now);
        assert_eq!(out.pending, vec!["Obsidian".to_string()], "extraction from the stored path started");
        // On this platform the extraction finds no icon; wait for the thread.
        let c = cache(&st.paths.root);
        for _ in 0..200 {
            if !c.is_busy("app:obsidian") {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(lock(&c.apps).entries.get("obsidian").is_some_and(|e| !e.icon && e.path.ends_with("Obsidian.exe")));
        let (out, _) = lookup(&st, &["Obsidian".to_string()], now);
        assert!(out.pending.is_empty() && out.icons["Obsidian"].is_none(), "not tried again within a day");

        // note_foreground: nothing for private apps or with icons off.
        note_foreground(&st, &settings, "KeePassXC", r"C:\KeePassXC\KeePassXC.exe");
        let mut off = settings.clone();
        off.show_icons = false;
        note_foreground(&st, &off, "Slack", r"C:\Slack\slack.exe");
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(!lock(&c.apps).entries.contains_key("keepassxc") && !lock(&c.apps).entries.contains_key("slack"));
    }

    #[test]
    fn older_databases_get_the_path_column() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.sqlite");
        {
            let c = rusqlite::Connection::open(&path).unwrap();
            c.execute_batch(
                "CREATE TABLE screenshots (id INTEGER PRIMARY KEY AUTOINCREMENT, captured_at INTEGER NOT NULL, file_path TEXT NOT NULL,
                 file_size INTEGER, idle_seconds_at_capture INTEGER, app_name TEXT, window_title TEXT, hidden_app TEXT,
                 is_deleted INTEGER NOT NULL DEFAULT 0, created_at DATETIME DEFAULT CURRENT_TIMESTAMP);
                 INSERT INTO screenshots (captured_at, file_path, app_name) VALUES (1, '', 'Code');",
            )
            .unwrap();
        }
        let db = crate::db::Db::open(&path).unwrap();
        db.insert_screenshot_with_path(2, "", 0, 0, Some("Code"), None, None, None, Some(r"C:\VS Code\Code.exe")).unwrap();
        let rows: Vec<Option<String>> = {
            let c = db.conn();
            let mut st = c.prepare("SELECT app_path FROM screenshots ORDER BY captured_at").unwrap();
            st.query_map([], |r| r.get(0)).unwrap().map(|r| r.unwrap()).collect()
        };
        assert_eq!(rows, vec![None, Some(r"C:\VS Code\Code.exe".to_string())]);
    }

    #[test]
    fn cleanup_keeps_the_cache_bounded() {
        let (_d, st) = state();
        let now = now_ts();
        let c = cache(&st.paths.root);
        c.finish_app("old", r"C:\Old\old.exe", Some(square([1, 100, 1])), vec![], now - APP_KEEP_SECS - 10);
        c.finish_app("fresh", r"C:\Fresh\fresh.exe", Some(square([1, 100, 1])), vec![], now);
        c.finish_app("bitwarden", r"C:\Bitwarden\Bitwarden.exe", Some(square([1, 100, 200])), vec![], now);
        let mut settings = st.settings();
        settings.lookup_site_icons = true;
        c.finish_site("used.com", Some(square([9, 9, 200])), now);
        c.finish_site("stale.com", Some(square([9, 9, 200])), now - SITE_KEEP_SECS - 10);
        // A stray file that is old enough to go.
        let stray = c.dir.join("apps").join("stray.png");
        std::fs::write(&stray, b"x").unwrap();
        let old = std::fs::File::options().write(true).open(&stray).unwrap();
        old.set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3600)).unwrap();
        drop(old);
        cleanup(&st.paths.root, &settings, now);
        let apps = lock(&c.apps).entries.keys().cloned().collect::<Vec<_>>();
        assert_eq!(apps, vec!["fresh".to_string()], "a year unseen and private apps go");
        assert!(!c.app_png("old").exists() && !c.app_png("bitwarden").exists() && c.app_png("fresh").exists());
        assert!(!stray.exists());
        assert_eq!(lock(&c.sites).entries.keys().cloned().collect::<Vec<_>>(), vec!["used.com".to_string()]);
        assert!(!c.site_png("stale.com").exists());
        // The index survives a restart.
        let reloaded = Cache::load(c.dir.clone());
        assert!(lock(&reloaded.apps).entries.contains_key("fresh"));

        // Bounded: the oldest go first.
        let mut many = BTreeMap::new();
        for i in 0..(MAX_APPS + 5) {
            many.insert(format!("app{i:03}"), entry("", &[], i as i64));
        }
        let gone = evict(&mut many, MAX_APPS, |e| e.seen);
        assert_eq!(gone, (0..5).map(|i| format!("app{i:03}")).collect::<Vec<_>>());
        assert_eq!(many.len(), MAX_APPS);
    }
}
