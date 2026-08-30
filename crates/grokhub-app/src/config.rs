use grokhub_core::{is_plain_text, BoardCard};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// SOUL/USER/MEMORY on the UI thread. Bigger files freeze kick_model and the editor.
pub const MEMORY_FILE_CAP: usize = 1024 * 1024;

/// JSON stores grow with ordinary use — `threads.json` holds the whole chat history — so
/// they get a far larger ceiling than the markdown memory files. Anything over the cap is
/// quarantined rather than parsed: a severed JSON token deserializes to nothing, and the
/// next persist would write that nothing back over the user's data.
pub const JSON_STORE_CAP: usize = 32 * 1024 * 1024;

/// Write, fsync, then rename so a kill mid-persist cannot leave a truncated JSON.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let dir = path.parent().ok_or_else(|| "atomic write needs a parent".to_string())?;
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| "atomic write needs a file name".to_string())?;
    let tmp = dir.join(format!(".{name}.tmp"));
    // The temp file holds the same bytes as the destination, so it has to be private from
    // the moment it exists. Creating it 0644 and chmodding after the rename leaves the
    // console key and OAuth refresh token world-readable for the whole write.
    let mut f = create_private(&tmp).map_err(|e| e.to_string())?;
    f.write_all(bytes).map_err(|e| e.to_string())?;
    f.sync_all().map_err(|e| e.to_string())?;
    drop(f);
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        e.to_string()
    })?;
    restrict_private(path);
    Ok(())
}

/// Create (or replace) a file that is 0600 before any bytes reach it.
#[cfg(unix)]
pub fn create_private(path: &Path) -> std::io::Result<fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    // `mode` only applies to a fresh inode, so drop any leftover temp from a killed write
    // instead of inheriting its permissions.
    let _ = fs::remove_file(path);
    fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
pub fn create_private(path: &Path) -> std::io::Result<fs::File> {
    fs::File::create(path)
}

#[cfg(unix)]
fn restrict_private(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_private(_path: &Path) {}

enum StoreRead {
    Missing,
    Text(String),
    Unusable,
}

fn read_store(path: &Path, cap: usize) -> StoreRead {
    let meta = match fs::metadata(path) {
        Ok(m) => m,
        Err(_) => return StoreRead::Missing,
    };
    if meta.len() > cap as u64 {
        return StoreRead::Unusable;
    }
    match fs::File::open(path) {
        Ok(mut f) => {
            let mut raw = String::new();
            match f.read_to_string(&mut raw) {
                Ok(_) => StoreRead::Text(raw),
                Err(_) => StoreRead::Unusable,
            }
        }
        Err(_) => StoreRead::Missing,
    }
}

/// Move a store the loader could not parse out of the way, returning the new path.
///
/// Without this the caller falls back to a default value that the next persist tick
/// writes straight back over the original file, turning one unreadable store into
/// permanent data loss.
pub fn quarantine(path: &Path) -> Option<PathBuf> {
    let name = path.file_name().and_then(|s| s.to_str())?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let aside = path.with_file_name(format!("{name}.corrupt-{stamp}"));
    fs::rename(path, &aside).ok()?;
    Some(aside)
}

/// Load a JSON store, quarantining it rather than silently returning `fallback` over data
/// that is merely unreadable. A missing or empty store is normal and yields `fallback`.
pub fn load_json_or<T, F>(path: &Path, cap: usize, fallback: F) -> T
where
    T: serde::de::DeserializeOwned,
    F: FnOnce() -> T,
{
    let raw = match read_store(path, cap) {
        StoreRead::Missing => return fallback(),
        StoreRead::Unusable => {
            quarantine(path);
            return fallback();
        }
        StoreRead::Text(raw) => raw,
    };
    if raw.trim().is_empty() {
        return fallback();
    }
    match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => {
            quarantine(path);
            fallback()
        }
    }
}

/// `load_json_or` with `T::default()` as the fallback.
pub fn load_json<T: Default + serde::de::DeserializeOwned>(path: &Path, cap: usize) -> T {
    load_json_or(path, cap, T::default)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub api_key: String,
    #[serde(default)]
    pub device_name: String,
    #[serde(default = "default_yolo")]
    pub yolo: bool,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub imagine_model: String,
    #[serde(default)]
    pub voice_model: String,
    #[serde(default)]
    pub cabin_eyes: bool,
    #[serde(default = "default_autonomy")]
    pub autonomy: u8,
    /// Git clone used by Settings → Update / `grokhub --update`.
    #[serde(default)]
    pub source_dir: String,
    #[serde(default)]
    pub project_dir: String,
    #[serde(default = "default_host_on")]
    pub host_on: bool,
    #[serde(default = "default_host_cap")]
    pub host_hour_cap: u32,
    #[serde(default)]
    pub approve_risky_only: bool,
    #[serde(default)]
    pub current_thread: String,
    #[serde(default)]
    pub connector_hosts: Vec<String>,
    #[serde(default = "default_close_to_tray")]
    pub close_to_tray: bool,
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub reasoning_effort: String,
    #[serde(default = "default_quiet_start")]
    pub quiet_start: String,
    #[serde(default = "default_quiet_end")]
    pub quiet_end: String,
    #[serde(default = "default_daily_auto")]
    pub daily_auto_cap: u32,
    #[serde(default)]
    pub goal_pin: String,
    /// Cabin paints a new Imagine cover every few hours.
    #[serde(default = "default_imagine_wall")]
    pub imagine_wall: bool,
    #[serde(default = "default_theme")]
    pub theme: String,
    #[serde(default)]
    pub window: crate::window::WindowGeom,
}

fn default_yolo() -> bool {
    false
}

fn default_autonomy() -> u8 {
    4
}

fn default_host_on() -> bool {
    true
}

fn default_host_cap() -> u32 {
    40
}

fn default_close_to_tray() -> bool {
    true
}

fn default_quiet_start() -> String {
    "22:00".into()
}

fn default_quiet_end() -> String {
    "07:00".into()
}

fn default_daily_auto() -> u32 {
    40
}

fn default_imagine_wall() -> bool {
    true
}

fn default_theme() -> String {
    "dark".into()
}

fn default_reasoning_effort() -> String {
    "high".into()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            device_name: String::new(),
            yolo: default_yolo(),
            model: String::new(),
            imagine_model: String::new(),
            voice_model: String::new(),
            cabin_eyes: false,
            autonomy: default_autonomy(),
            source_dir: String::new(),
            project_dir: String::new(),
            host_on: default_host_on(),
            host_hour_cap: default_host_cap(),
            approve_risky_only: false,
            current_thread: String::new(),
            connector_hosts: Vec::new(),
            close_to_tray: default_close_to_tray(),
            mode: String::new(),
            reasoning_effort: default_reasoning_effort(),
            quiet_start: default_quiet_start(),
            quiet_end: default_quiet_end(),
            daily_auto_cap: default_daily_auto(),
            goal_pin: String::new(),
            imagine_wall: default_imagine_wall(),
            theme: default_theme(),
            window: crate::window::WindowGeom::default(),
        }
    }
}

pub fn config_dir() -> PathBuf {
    if let Ok(p) = std::env::var("GROKHUB_CONFIG") {
        return PathBuf::from(p);
    }
    dirs_fallback()
}

fn dirs_fallback() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".config/GrokHub");
    }
    PathBuf::from(".grokhub")
}

pub fn memory_dir() -> PathBuf {
    config_dir().join("memory")
}

pub fn default_device_name() -> String {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .or_else(|| hostname_cmd())
        .unwrap_or_else(|| "This computer".into())
}

fn hostname_cmd() -> Option<String> {
    let mut child = std::process::Command::new("hostname")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(st)) if st.success() => {
                let mut out = String::new();
                if let Some(mut stdout) = child.stdout.take() {
                    let _ = stdout.take(256).read_to_string(&mut out);
                }
                let name = out.trim().to_string();
                if name.is_empty() {
                    return None;
                }
                return Some(name);
            }
            Ok(Some(_)) => return None,
            Ok(None) if start.elapsed() > Duration::from_millis(400) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(15)),
            Err(_) => return None,
        }
    }
}

pub fn load() -> AppConfig {
    let path = config_dir().join("app.json");
    let mut cfg: AppConfig = load_json(&path, JSON_STORE_CAP);
    cfg.host_on = true;
    // Leftover `"yolo": true` from older cabins must not disable the bound-tree jail.
    cfg.yolo = false;
    if cfg.device_name.trim().is_empty() {
        cfg.device_name = default_device_name();
    }
    if cfg.reasoning_effort.trim().is_empty() {
        cfg.reasoning_effort = grokhub_core::agent_reasoning_effort_for_mode(&cfg.mode)
            .unwrap_or("high")
            .to_string();
    } else if let Some(effort) = grokhub_core::parse_reasoning_effort(&cfg.reasoning_effort) {
        cfg.reasoning_effort = effort.to_string();
    }
    cfg
}

const SOUL_SEED: &str = "# Soul\n\nWho this cabin is. Edit this.\n";
const USER_SEED: &str = "# User\n\nWho you are. Edit this.\n";
const MEMORY_SEED: &str = "# Memory\n\nLong-term notes.\n";

/// First-run SOUL/USER/MEMORY so Settings → Memory is not three empty editors.
pub fn ensure_memory_seeds() {
    let dir = memory_dir();
    let _ = fs::create_dir_all(&dir);
    for (name, body) in [
        ("SOUL.md", SOUL_SEED),
        ("USER.md", USER_SEED),
        ("MEMORY.md", MEMORY_SEED),
    ] {
        if !dir.join(name).exists() {
            let _ = write_memory(name, body);
        }
    }
}

pub fn save(cfg: &AppConfig) -> Result<(), String> {
    let mut cfg = cfg.clone();
    // Console key lives in secrets.json. Never rewrite a leftover into app.json.
    cfg.api_key.clear();
    let s = serde_json::to_string_pretty(&cfg).map_err(|e| e.to_string())?;
    atomic_write(&config_dir().join("app.json"), s.as_bytes())
}

pub fn read_file_capped(path: &Path, cap: usize) -> String {
    let mut f = match fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return String::new(),
    };
    // `Read::read` may stop short of the buffer, so loop to EOF or the cap instead of
    // trusting one call and silently dropping the rest of the file.
    let mut buf = Vec::new();
    if f.take(cap as u64).read_to_end(&mut buf).is_err() {
        return String::new();
    }
    while !buf.is_empty() && std::str::from_utf8(&buf).is_err() {
        buf.pop();
    }
    String::from_utf8(buf).unwrap_or_default()
}

pub fn read_memory(name: &str) -> String {
    read_file_capped(&memory_dir().join(name), MEMORY_FILE_CAP)
}

pub fn memory_updated_at(name: &str) -> u64 {
    let path = memory_dir().join(name);
    fs::metadata(&path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn write_memory(name: &str, body: &str) -> Result<(), String> {
    let mut bytes = body.as_bytes();
    if bytes.len() > MEMORY_FILE_CAP {
        bytes = &bytes[..MEMORY_FILE_CAP];
        while !bytes.is_empty() && std::str::from_utf8(bytes).is_err() {
            bytes = &bytes[..bytes.len() - 1];
        }
    }
    let body = std::str::from_utf8(bytes).unwrap_or("");
    if !is_plain_text(body) {
        return Err("Secrets never in markdown".into());
    }
    let dir = memory_dir();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(name);
    if path.exists() {
        if fs::metadata(&path).map(|m| m.len()).unwrap_or(u64::MAX) <= MEMORY_FILE_CAP as u64 {
            let _ = fs::copy(&path, dir.join(format!("{name}.prev")));
        }
    }
    atomic_write(&path, bytes)
}

pub fn restore_memory(name: &str) -> Result<String, String> {
    let prev = read_memory(&format!("{name}.prev"));
    if prev.is_empty() {
        return Err(format!("no {name}.prev"));
    }
    let dir = memory_dir();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    atomic_write(&dir.join(name), prev.as_bytes())?;
    Ok(prev)
}

pub fn append_memory(name: &str, line: &str) -> Result<(), String> {
    let mut body = read_memory(name);
    if !body.is_empty() && !body.ends_with('\n') {
        body.push('\n');
    }
    body.push_str(line.trim());
    body.push('\n');
    write_memory(name, &body)
}

pub fn hub_state_path() -> PathBuf {
    config_dir().join("hub-state.json")
}

pub fn chat_path() -> PathBuf {
    config_dir().join("chat.json")
}

pub fn load_chat() -> Vec<(String, String)> {
    load_json(&chat_path(), JSON_STORE_CAP)
}

pub fn workboard_path() -> PathBuf {
    config_dir().join("workboard.json")
}

pub fn load_board() -> Vec<BoardCard> {
    load_json(&workboard_path(), JSON_STORE_CAP)
}

pub fn save_board(cards: &[BoardCard]) -> Result<(), String> {
    let s = serde_json::to_string_pretty(cards).map_err(|e| e.to_string())?;
    atomic_write(&workboard_path(), s.as_bytes())
}

pub fn wall_dir() -> PathBuf {
    config_dir().join("imagine-wall")
}

pub fn imagine_dir() -> PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join("GrokHub-Work/imagine");
    }
    config_dir().join("imagine")
}

pub fn save_chat(msgs: &[(String, String)]) -> Result<(), String> {
    let s = serde_json::to_string_pretty(msgs).map_err(|e| e.to_string())?;
    atomic_write(&chat_path(), s.as_bytes())
}

#[cfg(test)]
pub static TEST_CONFIG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_under_grokhub_config() {
        let _g = TEST_CONFIG_LOCK.lock().unwrap();
        let root = std::env::temp_dir().join(format!("grokhub-cfg-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        std::env::set_var("GROKHUB_CONFIG", &root);
        let mut cfg = AppConfig::default();
        cfg.api_key = "xai-test".into();
        cfg.device_name = "cabin".into();
        cfg.source_dir = "/tmp/Grok-Hub".into();
        save(&cfg).expect("save");
        let loaded = load();
        assert!(
            loaded.api_key.is_empty(),
            "console key must not land in app.json"
        );
        assert_eq!(loaded.device_name, "cabin");
        assert_eq!(loaded.source_dir, "/tmp/Grok-Hub");
        assert_eq!(loaded.reasoning_effort, "high");
        let body = fs::read_to_string(config_dir().join("app.json")).expect("app.json");
        assert!(
            !body.contains("xai-test") && !body.to_ascii_lowercase().contains("apikey"),
            "app.json must omit the leftover console-key field: {body}"
        );
        fs::write(
            config_dir().join("app.json"),
            r#"{"apiKey":"xai-legacy","deviceName":"cabin"}"#,
        )
        .expect("legacy");
        let leftover = load();
        assert_eq!(
            leftover.api_key, "xai-legacy",
            "boot must still read a leftover app.json key so Cabin can migrate it"
        );
        ensure_memory_seeds();
        assert!(read_memory("SOUL.md").contains("Who this cabin is"));
        assert!(read_memory("USER.md").contains("Who you are"));
        assert!(read_memory("MEMORY.md").contains("Long-term notes"));
        write_memory("SOUL.md", "be useful").expect("mem");
        assert_eq!(read_memory("SOUL.md"), "be useful");
        ensure_memory_seeds();
        assert_eq!(read_memory("SOUL.md"), "be useful");
        assert!(
            memory_updated_at("SOUL.md") > 0,
            "sync LWW needs a real file time, not now_ms"
        );
        append_memory("MEMORY.md", "prefer nvim").expect("append");
        assert!(read_memory("MEMORY.md").contains("prefer nvim"));
        save_chat(&[("user".into(), "hi".into())]).expect("chat");
        assert_eq!(load_chat(), vec![("user".into(), "hi".into())]);
        write_memory("MEMORY.md", "prefer nvim").expect("mem2");
        write_memory("MEMORY.md", "prefer helix").expect("mem3");
        assert!(read_memory("MEMORY.md.prev").contains("prefer nvim"));
        let restored = restore_memory("MEMORY.md").expect("restore");
        assert!(restored.contains("prefer nvim"));
        assert!(write_memory("MEMORY.md", "token sk-abcdefghijklmnopqrstuv").is_err());
        let dest = root.join("atomic.json");
        atomic_write(&dest, br#"{"ok":true}"#).expect("atomic");
        assert_eq!(fs::read_to_string(&dest).unwrap(), r#"{"ok":true}"#);
        assert!(!root.join(".atomic.json.tmp").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(config_dir().join("app.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode, 0o600, "app.json is private config");
        }
        let _ = fs::remove_dir_all(&root);
        std::env::remove_var("GROKHUB_CONFIG");
    }

    #[test]
    fn empty_device_name_fills_from_the_box() {
        let _g = TEST_CONFIG_LOCK.lock().unwrap();
        let root = std::env::temp_dir().join(format!("grokhub-devname-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        std::env::set_var("GROKHUB_CONFIG", &root);
        fs::create_dir_all(&root).expect("dir");
        fs::write(root.join("app.json"), "{}").expect("write");
        let loaded = load();
        assert!(!loaded.device_name.trim().is_empty());
        assert_eq!(loaded.device_name, default_device_name());
        let _ = fs::remove_dir_all(&root);
        std::env::remove_var("GROKHUB_CONFIG");
    }

    #[test]
    fn reasoning_effort_migrates_from_legacy_mode() {
        let _g = TEST_CONFIG_LOCK.lock().unwrap();
        let root = std::env::temp_dir().join(format!("grokhub-effort-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        std::env::set_var("GROKHUB_CONFIG", &root);
        fs::create_dir_all(&root).expect("dir");
        fs::write(root.join("app.json"), r#"{"mode":"max"}"#).expect("write");
        let loaded = load();
        assert_eq!(loaded.reasoning_effort, "xhigh");
        let _ = fs::remove_dir_all(&root);
        std::env::remove_var("GROKHUB_CONFIG");
    }

    #[test]
    fn default_device_name_must_not_block_boot() {
        let src = include_str!("config.rs");
        let host = src
            .split("fn hostname_cmd()")
            .nth(1)
            .and_then(|s| s.split("pub fn load(").next())
            .expect("hostname_cmd");
        assert!(
            host.contains("try_wait") && !host.contains(".output()"),
            "boot hostname must time out: {host}"
        );
    }

    #[test]
    fn read_memory_does_not_slurp_a_huge_file() {
        let src = include_str!("config.rs");
        let read = src
            .split("pub fn read_memory(")
            .nth(1)
            .and_then(|s| s.split("pub fn memory_updated_at(").next())
            .expect("read_memory");
        assert!(
            read.contains("MEMORY_FILE_CAP") && !read.contains("read_to_string"),
            "Memory editor and kick_model must not slurp a huge MEMORY.md: {read}"
        );
        let _g = TEST_CONFIG_LOCK.lock().unwrap();
        let root = std::env::temp_dir().join(format!("grokhub-mem-cap-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        std::env::set_var("GROKHUB_CONFIG", &root);
        fs::create_dir_all(memory_dir()).unwrap();
        fs::write(memory_dir().join("MEMORY.md"), "x".repeat(MEMORY_FILE_CAP + 4096)).unwrap();
        assert_eq!(read_memory("MEMORY.md").len(), MEMORY_FILE_CAP);
        write_memory("SOUL.md", &"y".repeat(MEMORY_FILE_CAP + 2048)).expect("clip write");
        assert_eq!(read_memory("SOUL.md").len(), MEMORY_FILE_CAP);
        let src = include_str!("config.rs");
        let write = src
            .split("pub fn write_memory(")
            .nth(1)
            .and_then(|s| s.split("pub fn restore_memory(").next())
            .expect("write_memory");
        assert!(
            write.contains("MEMORY_FILE_CAP"),
            "saving Memory must not write a huge file on the UI thread: {write}"
        );
        let _ = fs::remove_dir_all(&root);
        std::env::remove_var("GROKHUB_CONFIG");
    }

    #[test]
    fn default_close_to_tray_is_on() {
        assert!(
            AppConfig::default().close_to_tray,
            "first persist must write closeToTray true so X hides to the tray"
        );
        assert!(default_close_to_tray());
        let _g = TEST_CONFIG_LOCK.lock().unwrap();
        let root = std::env::temp_dir().join(format!("grokhub-cfg-empty-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        std::env::set_var("GROKHUB_CONFIG", &root);
        let loaded = load();
        assert!(loaded.close_to_tray);
        assert!(loaded.host_on);
        fs::create_dir_all(config_dir()).unwrap();
        fs::write(config_dir().join("app.json"), r#"{"hostOn":false,"yolo":true}"#).unwrap();
        assert!(
            load().host_on,
            "stale hostOn false must not brick /sh after the toggle was removed"
        );
        assert!(
            !load().yolo,
            "leftover yolo true must not disable the bound-tree jail"
        );
        assert_eq!(loaded.autonomy, 4);
        assert!(
            !loaded.yolo,
            "Ask is the default — leftover yolo true disables the bound-tree jail"
        );
        assert!(loaded.imagine_wall);
        assert_eq!(loaded.theme, "dark");
        let mut placed = AppConfig::default();
        placed.window.x = Some(80.0);
        placed.window.y = Some(40.0);
        placed.window.w = 1280.0;
        placed.window.h = 800.0;
        save(&placed).expect("window save");
        let loaded = load();
        assert_eq!(loaded.window.x, Some(80.0));
        assert_eq!(loaded.window.y, Some(40.0));
        assert_eq!(loaded.window.w, 1280.0);
        assert_eq!(loaded.window.h, 800.0);
        placed.window.maximized = true;
        save(&placed).expect("maximized save");
        assert!(load().window.maximized);
        let mut themed = AppConfig::default();
        themed.theme = "system".into();
        save(&themed).expect("theme save");
        assert_eq!(load().theme, "system");
        let _ = fs::remove_dir_all(&root);
        std::env::remove_var("GROKHUB_CONFIG");
    }

    #[test]
    fn cabin_config_loads_do_not_slurp_huge_files() {
        let src = include_str!("config.rs");
        for (name, next) in [
            ("pub fn load(", "pub fn save("),
            ("pub fn load_chat(", "pub fn workboard_path("),
            ("pub fn load_board(", "pub fn save_board("),
        ] {
            let slice = src
                .split(name)
                .nth(1)
                .and_then(|s| s.split(next).next())
                .unwrap_or(name);
            assert!(
                slice.contains("load_json") && !slice.contains("read_to_string"),
                "boot must not slurp an unbounded {name}: {slice}"
            );
            assert!(
                !slice.contains("MEMORY_FILE_CAP"),
                "{name} is a JSON store: capping the read severs it mid-token, so the \
                 loader returns the default and the next persist saves that over the \
                 user's data: {slice}"
            );
        }
    }

    #[test]
    fn oversized_json_store_is_quarantined_not_wiped() {
        let _g = TEST_CONFIG_LOCK.lock().unwrap();
        let root = std::env::temp_dir().join(format!("grokhub-oversize-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("root");
        let path = root.join("threads.json");

        // A real store that happens to be bigger than the cap we read it with.
        let rows: Vec<String> = (0..200).map(|i| format!("\"row-{i}\"")).collect();
        let body = format!("[{}]", rows.join(","));
        fs::write(&path, &body).expect("write");
        let tiny_cap = 64;
        assert!(body.len() > tiny_cap, "fixture must exceed the cap");

        let loaded: Vec<String> = load_json(&path, tiny_cap);
        assert!(loaded.is_empty(), "an over-cap store cannot be parsed");
        assert!(
            !path.exists(),
            "the loader fell back to a default, so the original must be moved aside — \
             otherwise the next persist writes the empty default over it"
        );
        let saved: Vec<_> = fs::read_dir(&root)
            .expect("dir")
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.starts_with("threads.json.corrupt-"))
            .collect();
        assert_eq!(saved.len(), 1, "exactly one quarantined copy: {saved:?}");
        let kept = fs::read_to_string(root.join(&saved[0])).expect("quarantined");
        assert_eq!(kept, body, "the quarantined copy must be byte-identical");

        // Within the cap the same store loads normally and is left in place.
        fs::write(&path, &body).expect("rewrite");
        let loaded: Vec<String> = load_json(&path, JSON_STORE_CAP);
        assert_eq!(loaded.len(), 200);
        assert!(path.exists(), "a readable store must not be quarantined");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn corrupt_json_store_is_quarantined_and_missing_one_is_not() {
        let _g = TEST_CONFIG_LOCK.lock().unwrap();
        let root = std::env::temp_dir().join(format!("grokhub-corrupt-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("root");

        let torn = root.join("projects.json");
        fs::write(&torn, "[{\"id\":\"a\",\"na").expect("write");
        let loaded: Vec<String> = load_json(&torn, JSON_STORE_CAP);
        assert!(loaded.is_empty());
        assert!(!torn.exists(), "torn JSON must be preserved under a new name");

        // Missing and empty are ordinary first-run states, not corruption.
        let absent = root.join("absent.json");
        let loaded: Vec<String> = load_json(&absent, JSON_STORE_CAP);
        assert!(loaded.is_empty());
        let empty = root.join("empty.json");
        fs::write(&empty, "   \n").expect("write");
        let loaded: Vec<String> = load_json(&empty, JSON_STORE_CAP);
        assert!(loaded.is_empty());
        assert!(empty.exists(), "an empty store must not be quarantined");
        let junk: Vec<_> = fs::read_dir(&root)
            .expect("dir")
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains(".corrupt-"))
            .collect();
        assert_eq!(junk.len(), 1, "only the torn store is quarantined: {junk:?}");

        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn atomic_write_temp_is_private_before_the_rename() {
        use std::os::unix::fs::PermissionsExt;
        let _g = TEST_CONFIG_LOCK.lock().unwrap();
        let root = std::env::temp_dir().join(format!("grokhub-priv-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("root");
        let path = root.join("secrets.json");

        // The temp file holds the same secret bytes as the destination, so it must never
        // exist as 0644 — a chmod after the rename is a window, not a fix.
        let tmp = root.join(".secrets.json.tmp");
        let f = create_private(&tmp).expect("create");
        drop(f);
        let mode = fs::metadata(&tmp).expect("meta").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "temp file must be created 0600, got {mode:o}");
        let _ = fs::remove_file(&tmp);

        atomic_write(&path, b"{\"apiKey\":\"xai-secret\"}").expect("write");
        let mode = fs::metadata(&path).expect("meta").permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "destination must be 0600, got {mode:o}");
        assert!(!tmp.exists(), "temp must not survive a successful write");

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn read_file_capped_loops_to_the_cap() {
        let _g = TEST_CONFIG_LOCK.lock().unwrap();
        let root = std::env::temp_dir().join(format!("grokhub-shortread-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("root");
        let path = root.join("big.md");
        let body = "x".repeat(300_000);
        fs::write(&path, &body).expect("write");
        assert_eq!(
            read_file_capped(&path, MEMORY_FILE_CAP).len(),
            body.len(),
            "one short read must not silently drop the rest of the file"
        );
        assert_eq!(read_file_capped(&path, 1000).len(), 1000, "the cap still holds");
        let _ = fs::remove_dir_all(&root);
    }
}
