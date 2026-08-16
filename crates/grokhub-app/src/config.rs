use grokhub_core::{is_plain_text, BoardCard};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Write, fsync, then rename so a kill mid-persist cannot leave a truncated JSON.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let dir = path.parent().ok_or_else(|| "atomic write needs a parent".to_string())?;
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| "atomic write needs a file name".to_string())?;
    let tmp = dir.join(format!(".{name}.tmp"));
    let mut f = fs::File::create(&tmp).map_err(|e| e.to_string())?;
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

#[cfg(unix)]
fn restrict_private(path: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
}

#[cfg(not(unix))]
fn restrict_private(_path: &Path) {}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfig {
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub device_name: String,
    #[serde(default)]
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

fn default_autonomy() -> u8 {
    1
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

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            device_name: String::new(),
            yolo: false,
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

pub fn load() -> AppConfig {
    let path = config_dir().join("app.json");
    let raw = fs::read_to_string(path).unwrap_or_default();
    serde_json::from_str(&raw).unwrap_or_default()
}

pub fn save(cfg: &AppConfig) -> Result<(), String> {
    let s = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    atomic_write(&config_dir().join("app.json"), s.as_bytes())
}

pub fn read_memory(name: &str) -> String {
    let path = memory_dir().join(name);
    fs::read_to_string(path).unwrap_or_default()
}

pub fn write_memory(name: &str, body: &str) -> Result<(), String> {
    if !is_plain_text(body) {
        return Err("Secrets never in markdown".into());
    }
    let dir = memory_dir();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(name);
    if path.exists() {
        let _ = fs::copy(&path, dir.join(format!("{name}.prev")));
    }
    atomic_write(&path, body.as_bytes())
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
    let raw = fs::read_to_string(chat_path()).unwrap_or_default();
    serde_json::from_str(&raw).unwrap_or_default()
}

pub fn workboard_path() -> PathBuf {
    config_dir().join("workboard.json")
}

pub fn load_board() -> Vec<BoardCard> {
    let raw = fs::read_to_string(workboard_path()).unwrap_or_default();
    serde_json::from_str(&raw).unwrap_or_default()
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
        assert_eq!(loaded.api_key, "xai-test");
        assert_eq!(loaded.device_name, "cabin");
        assert_eq!(loaded.source_dir, "/tmp/Grok-Hub");
        write_memory("SOUL.md", "be useful").expect("mem");
        assert_eq!(read_memory("SOUL.md"), "be useful");
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
            assert_eq!(mode, 0o600, "app.json holds the console key");
        }
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
        assert_eq!(loaded.autonomy, 1);
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
}
