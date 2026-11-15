//! Settings → Account → Profile: your name and a profile picture, shown on
//! the rail. They are the cabin's own `displayName` and `profilePicture` keys
//! in app.json plus `profile.png` in the config folder, so a profile set in
//! the cabin shows here with no import.
//!
//! The picture is always read from `<config>/profile.png`, never from the
//! path stored in app.json, so a hand-edited path can't make the page read
//! some other file. It is cropped square and made 256 px on install.

use std::io::Read;
use std::path::Path;
use std::sync::Mutex;

use base64::Engine;
use serde::Serialize;
use serde_json::{json, Value};

/// Longest name kept, in characters.
pub const NAME_MAX: usize = 64;
/// The picture in the config folder.
pub const PICTURE_FILE: &str = "profile.png";
/// The picture's side after install, in pixels.
const SIDE: u32 = 256;
const NAME_KEY: &str = "displayName";
const PICTURE_KEY: &str = "profilePicture";

pub const TOO_BIG: &str = "Choose a smaller image";
pub const NOT_AN_IMAGE: &str = "Choose an image";

/// Install and Remove one at a time, so a late install can't bring back a
/// picture that was just removed.
static FILE_IO: Mutex<()> = Mutex::new(());

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    /// The saved name; empty when none is set.
    pub name: String,
    /// What the rail shows: the saved name, else the Grok name, else the
    /// name in memory, else "You".
    pub shown_name: String,
    /// The first letter of `shown_name`, for the rail without a picture.
    pub initial: String,
    /// `data:image/png;base64,…`, or `None`.
    pub picture: Option<String>,
}

/// Trimmed, then cut to [`NAME_MAX`] characters.
pub fn clip_name(name: &str) -> String {
    name.trim().chars().take(NAME_MAX).collect()
}

/// The name the rail shows, in the cabin's order.
pub fn shown_name(saved: &str, grok_name: Option<&str>, user_md: &str) -> String {
    let saved = clip_name(saved);
    if !saved.is_empty() {
        return saved;
    }
    if let Some(name) = grok_name.map(|n| grokhub_core::greeting_name("", n)) {
        if !name.is_empty() {
            return name;
        }
    }
    let from_memory = grokhub_core::greeting_name(user_md, "");
    if from_memory.is_empty() {
        "You".into()
    } else {
        from_memory
    }
}

fn user_md(config_dir: &Path) -> String {
    let Ok(file) = std::fs::File::open(config_dir.join("memory").join("USER.md")) else {
        return String::new();
    };
    let mut text = String::new();
    let _ = file
        .take(grokhub_core::TEXT_FILE_CAP as u64)
        .read_to_string(&mut text);
    text
}

fn picture(config_dir: &Path, set: bool) -> Option<String> {
    if !set {
        return None;
    }
    let file = std::fs::File::open(config_dir.join(PICTURE_FILE)).ok()?;
    let mut bytes = Vec::new();
    file.take(grokhub_core::IMAGE_FILE_CAP + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > grokhub_core::IMAGE_FILE_CAP
        || crate::attach::image_mime(&bytes) != Some("image/png")
    {
        return None;
    }
    Some(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}

/// The profile now. `grok_name` is the Sign in with Grok name, if any.
pub fn load(config_dir: &Path, grok_name: Option<&str>) -> Profile {
    let app = grokhub_core::app_json::read(config_dir).unwrap_or_default();
    let text = |key: &str| {
        app.get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let name = clip_name(&text(NAME_KEY));
    let shown = shown_name(&name, grok_name, &user_md(config_dir));
    let initial = shown
        .chars()
        .next()
        .map(|c| c.to_uppercase().collect())
        .unwrap_or_default();
    Profile {
        name,
        initial,
        shown_name: shown,
        picture: picture(config_dir, !text(PICTURE_KEY).trim().is_empty()),
    }
}

/// Save the name; a blank one removes it, so the Grok name shows again.
pub fn set_name(config_dir: &Path, name: &str) -> Result<(), String> {
    let name = clip_name(name);
    grokhub_core::app_json::edit(config_dir, |map| {
        if name.is_empty() {
            map.remove(NAME_KEY);
        } else {
            map.insert(NAME_KEY.into(), json!(name));
        }
    })
}

/// A square crop from the middle, then at most [`SIDE`] pixels across.
fn avatar(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let (w, h) = crate::attach::image_size(bytes).ok_or(NOT_AN_IMAGE)?;
    // The header is checked before any decode: a small file can claim 50k by 50k.
    if !grokhub_core::image_pixels_ok(w, h) {
        return Err(NOT_AN_IMAGE.into());
    }
    let img = image::load_from_memory(bytes).map_err(|_| NOT_AN_IMAGE)?;
    let side = img.width().min(img.height());
    let square = img.crop_imm(
        (img.width() - side) / 2,
        (img.height() - side) / 2,
        side,
        side,
    );
    let square = if side > SIDE {
        square.thumbnail(SIDE, SIDE)
    } else {
        square
    };
    let mut out = std::io::Cursor::new(Vec::new());
    square
        .write_to(&mut out, image::ImageFormat::Png)
        .map_err(|_| NOT_AN_IMAGE)?;
    Ok(out.into_inner())
}

/// Copy `src` in as the profile picture (PNG or JPEG, up to 8 MiB).
pub fn install_picture(config_dir: &Path, src: &Path) -> Result<(), String> {
    let file = std::fs::File::open(src).map_err(|_| TOO_BIG)?;
    let len = file.metadata().map(|m| m.len()).unwrap_or(u64::MAX);
    if len == 0 || len > grokhub_core::IMAGE_FILE_CAP {
        return Err(TOO_BIG.into());
    }
    let mut bytes = Vec::new();
    file.take(grokhub_core::IMAGE_FILE_CAP + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| TOO_BIG)?;
    if bytes.len() as u64 > grokhub_core::IMAGE_FILE_CAP {
        return Err(TOO_BIG.into());
    }
    let png = avatar(&bytes)?;
    let _io = FILE_IO.lock().unwrap_or_else(|e| e.into_inner());
    let dest = config_dir.join(PICTURE_FILE);
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let tmp = config_dir.join(format!("{PICTURE_FILE}.tmp"));
    std::fs::write(&tmp, &png)
        .and_then(|()| std::fs::rename(&tmp, &dest))
        .map_err(|e| format!("Couldn't save the picture: {e}"))?;
    grokhub_core::app_json::patch(config_dir, &[(PICTURE_KEY, json!(dest.to_string_lossy()))])
}

/// Remove the picture: the key first, then the file.
pub fn clear_picture(config_dir: &Path) -> Result<(), String> {
    let _io = FILE_IO.lock().unwrap_or_else(|e| e.into_inner());
    grokhub_core::app_json::edit(config_dir, |map| {
        map.remove(PICTURE_KEY);
    })?;
    match std::fs::remove_file(config_dir.join(PICTURE_FILE)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("Couldn't remove the picture: {e}")),
    }
}

#[cfg(test)]
#[path = "profile_tests.rs"]
mod tests;
