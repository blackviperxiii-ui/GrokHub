//! Composer attachments (LO-17i): files and pictures added with the + button,
//! a drop or a paste wait on the composer until the message is sent.
//!
//! Each one is checked with the cabin's rules (`grokhub_core::attach`) and
//! copied into `{config}/sessions/<id>/attach/`, readable only by this user.
//! On send, the first picture goes to the model as image input; every other
//! file goes in as a line that names its path, for the agent to read.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use base64::Engine as _;
use grokhub_core::attach::{
    append_composer, attach_kind, attach_name, attach_send_line, image_pixels_ok,
    pending_item_user_message, pending_item_with_model, png_ihdr_size, validate_pending_path,
    AttachKind, PendingFileRole, PendingItemStatus, PendingValidateIssue, IMAGE_FILE_CAP,
    PENDING_ATTACH_MAX, TEXT_FILE_CAP,
};
use serde::{Deserialize, Serialize};

/// Phones have no system picker here; paste or share into the chat instead.
pub const NO_PICKER: &str = "The file picker isn't available on this device. Paste a file instead.";

/// PDFs share the picture cap: big enough for a manual, small enough to copy.
pub const DOC_FILE_CAP: u64 = IMAGE_FILE_CAP;

/// What a staged file is to the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    /// A PNG or JPEG: image input.
    Image,
    /// A text or code file the agent reads.
    Text,
    /// A PDF the agent reads.
    Document,
}

/// One file waiting on the composer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Staged {
    /// The copy in the chat's attach folder.
    pub path: String,
    /// The file's own name, as the chip and the send line show it.
    pub name: String,
    pub kind: Kind,
    /// "Picture shot.png", "File notes.md".
    pub label: String,
    /// The cabin's note when the chat's model can't read it; it won't send.
    pub status: Option<String>,
}

/// One picked or dropped file: staged, or why not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    pub staged: Option<Staged>,
    pub error: Option<String>,
}

impl From<Result<Staged, String>> for Outcome {
    fn from(result: Result<Staged, String>) -> Self {
        match result {
            Ok(staged) => Self {
                staged: Some(staged),
                error: None,
            },
            Err(error) => Self {
                staged: None,
                error: Some(error),
            },
        }
    }
}

/// The message the engine gets: the text with its send lines, and the
/// first picture as a data URL.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Composed {
    pub text: String,
    pub image: Option<String>,
}

/// `name: file is too large`, in the cabin's words.
fn refusal(name: &str, status: PendingItemStatus) -> String {
    let note = pending_item_user_message(status).unwrap_or("could not attach file");
    format!("{name}: {note}")
}

fn too_large(name: &str) -> String {
    refusal(
        name,
        PendingItemStatus::Invalid(PendingValidateIssue::TooLarge),
    )
}

fn unsupported(name: &str) -> String {
    refusal(
        name,
        PendingItemStatus::Invalid(PendingValidateIssue::UnsupportedType),
    )
}

/// The input kinds the model lists, or text and image when the registry
/// doesn't know it (the cabin's default).
pub fn modalities(config_dir: &Path, model: &str) -> Vec<String> {
    let reg = grokhub_core::model_registry::store::load_registry(config_dir);
    reg.models
        .get(model)
        .and_then(|m| m.meta.input_modalities.clone())
        .filter(|mods| !mods.is_empty())
        .unwrap_or_else(|| vec!["text".into(), "image".into()])
}

fn refs(mods: &[String]) -> Vec<&str> {
    mods.iter().map(String::as_str).collect()
}

/// A name safe to put in a file name: letters, digits, `.`, `-`, `_` and
/// spaces, at most 80 bytes, never hidden.
fn safe_name(raw: &str) -> String {
    let base = attach_name(raw.trim());
    let mut out: String = base
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ' ') {
                c
            } else {
                '_'
            }
        })
        .collect();
    while out.starts_with('.') {
        out.remove(0);
    }
    if out.len() > 80 {
        let ext = Path::new(&out)
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| format!(".{e}"))
            .unwrap_or_default();
        out.truncate(80 - ext.len().min(16));
        out.push_str(&ext);
    }
    if out.trim().is_empty() {
        "file".into()
    } else {
        out
    }
}

/// A conversation id is a folder name here, so it may hold only letters,
/// digits, `-` and `_`.
fn chat_folder(config_dir: &Path, conversation_id: &str) -> Result<PathBuf, String> {
    let ok = !conversation_id.is_empty()
        && conversation_id.len() <= 64
        && conversation_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !ok {
        return Err("That chat can't take files.".into());
    }
    Ok(config_dir
        .join("sessions")
        .join(conversation_id)
        .join("attach"))
}

fn image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]) {
        Some("image/png")
    } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else {
        None
    }
}

/// Width and height from the header only, never a decode: a tiny file can
/// claim 50k by 50k.
fn image_size(bytes: &[u8]) -> Option<(u32, u32)> {
    match image_mime(bytes)? {
        "image/png" => png_ihdr_size(bytes),
        _ => image::ImageReader::with_format(std::io::Cursor::new(bytes), image::ImageFormat::Jpeg)
            .into_dimensions()
            .ok(),
    }
}

/// The type and size rules, before anything is read: what the file is, or
/// the cabin's refusal.
fn check_meta(name: &str, size: u64) -> Result<Kind, String> {
    let status = validate_pending_path(name, size);
    let kind = match status {
        PendingItemStatus::Valid(PendingFileRole::VisionImage) => Kind::Image,
        PendingItemStatus::Valid(PendingFileRole::Document) => match attach_kind(name) {
            AttachKind::Text => Kind::Text,
            _ => Kind::Document,
        },
        // GIF, WebP, BMP and SVG need a re-encode the shell doesn't do; the
        // composer turns a pasted or dropped one into a PNG first.
        PendingItemStatus::NeedsTranscode => return Err(unsupported(name)),
        other => return Err(refusal(name, other)),
    };
    let cap = match kind {
        Kind::Image => IMAGE_FILE_CAP,
        Kind::Text => TEXT_FILE_CAP as u64,
        Kind::Document => DOC_FILE_CAP,
    };
    if size > cap {
        return Err(too_large(name));
    }
    Ok(kind)
}

/// The content rules: a picture must be a real PNG or JPEG within the pixel
/// cap.
fn check_bytes(name: &str, kind: Kind, bytes: &[u8]) -> Result<(), String> {
    if kind != Kind::Image {
        return Ok(());
    }
    let (width, height) = image_size(bytes).ok_or_else(|| unsupported(name))?;
    if !image_pixels_ok(width, height) {
        return Err(too_large(name));
    }
    Ok(())
}

fn label(kind: Kind, name: &str) -> String {
    let core = match kind {
        Kind::Image => AttachKind::Image,
        Kind::Text => AttachKind::Text,
        Kind::Document => AttachKind::Doc,
    };
    grokhub_core::attach::attach_chip_label(core, name)
}

fn role(kind: Kind) -> PendingFileRole {
    match kind {
        Kind::Image => PendingFileRole::VisionImage,
        Kind::Text | Kind::Document => PendingFileRole::Document,
    }
}

/// The core's note when `model_mods` can't take this kind, else `None`.
fn model_note(kind: Kind, model_mods: &[&str]) -> Option<String> {
    let status = pending_item_with_model(PendingItemStatus::Valid(role(kind)), model_mods);
    pending_item_user_message(status).map(str::to_string)
}

#[cfg(unix)]
fn private_file(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

#[cfg(not(unix))]
fn private_file(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
}

#[cfg(unix)]
fn private_dir(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)?;
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn private_dir(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)
}

static SEQ: AtomicU64 = AtomicU64::new(0);

fn write_copy(dir: &Path, name: &str, bytes: &[u8]) -> Result<PathBuf, String> {
    private_dir(dir).map_err(|e| format!("Couldn't save {name}: {e}"))?;
    let stamp = grokhub_core::now_ms();
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let path = dir.join(format!("{stamp}-{n}-{}", safe_name(name)));
    let mut file = private_file(&path).map_err(|e| format!("Couldn't save {name}: {e}"))?;
    file.write_all(bytes)
        .map_err(|e| format!("Couldn't save {name}: {e}"))?;
    Ok(path)
}

/// Every chat's waiting files, by conversation id.
#[derive(Default)]
pub struct Trays(Mutex<HashMap<String, Vec<Staged>>>);

impl Trays {
    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<String, Vec<Staged>>> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn room(&self, conversation_id: &str, name: &str) -> Result<(), String> {
        let held = self.lock().get(conversation_id).map_or(0, Vec::len);
        if held >= PENDING_ATTACH_MAX {
            return Err(refusal(
                name,
                PendingItemStatus::Invalid(PendingValidateIssue::TooMany),
            ));
        }
        Ok(())
    }

    fn keep(
        &self,
        conversation_id: &str,
        name: &str,
        kind: Kind,
        path: PathBuf,
        model_mods: &[&str],
    ) -> Result<Staged, String> {
        let staged = Staged {
            path: path.display().to_string(),
            name: name.to_string(),
            kind,
            label: label(kind, name),
            status: model_note(kind, model_mods),
        };
        let mut trays = self.lock();
        let tray = trays.entry(conversation_id.to_string()).or_default();
        if tray.len() >= PENDING_ATTACH_MAX {
            drop(trays);
            let _ = std::fs::remove_file(&path);
            return Err(refusal(
                name,
                PendingItemStatus::Invalid(PendingValidateIssue::TooMany),
            ));
        }
        tray.push(staged.clone());
        Ok(staged)
    }

    /// Stages pasted or dropped bytes (the webview and the web app).
    pub fn stage(
        &self,
        config_dir: &Path,
        conversation_id: &str,
        name: &str,
        bytes: &[u8],
        model_mods: &[&str],
    ) -> Result<Staged, String> {
        let name = safe_name(name);
        let dir = chat_folder(config_dir, conversation_id)?;
        self.room(conversation_id, &name)?;
        let kind = check_meta(&name, bytes.len() as u64)?;
        check_bytes(&name, kind, bytes)?;
        let path = write_copy(&dir, &name, bytes)?;
        self.keep(conversation_id, &name, kind, path, model_mods)
    }

    /// [`Trays::stage`] for base64 from the UI.
    pub fn stage_base64(
        &self,
        config_dir: &Path,
        conversation_id: &str,
        name: &str,
        data: &str,
        model_mods: &[&str],
    ) -> Result<Staged, String> {
        let name = safe_name(name);
        // Base64 is 4 bytes per 3: refuse a body past every cap unread.
        if data.len() as u64 / 4 * 3 > IMAGE_FILE_CAP + 3 {
            return Err(too_large(&name));
        }
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data.trim())
            .map_err(|_| unsupported(&name))?;
        self.stage(config_dir, conversation_id, &name, &bytes, model_mods)
    }

    /// Stages a file on this computer (the picker, a desktop drop). The size
    /// is checked before anything is read, and no more than the cap is read.
    pub fn stage_path(
        &self,
        config_dir: &Path,
        conversation_id: &str,
        path: &Path,
        model_mods: &[&str],
    ) -> Result<Staged, String> {
        let name = safe_name(&path.display().to_string());
        let dir = chat_folder(config_dir, conversation_id)?;
        self.room(conversation_id, &name)?;
        let meta = std::fs::metadata(path).map_err(|_| format!("{name}: can't open the file"))?;
        if !meta.is_file() {
            return Err(unsupported(&name));
        }
        let kind = check_meta(&name, meta.len())?;
        let mut bytes = Vec::new();
        std::fs::File::open(path)
            .and_then(|f| f.take(IMAGE_FILE_CAP + 1).read_to_end(&mut bytes))
            .map_err(|_| format!("{name}: can't open the file"))?;
        // It grew after the size check.
        let kind = check_meta(&name, bytes.len() as u64).and(Ok(kind))?;
        check_bytes(&name, kind, &bytes)?;
        let copy = write_copy(&dir, &name, &bytes)?;
        self.keep(conversation_id, &name, kind, copy, model_mods)
    }

    /// The chip's ×: forgets the file and deletes its copy.
    pub fn remove(&self, conversation_id: &str, path: &str) {
        let mut trays = self.lock();
        let Some(tray) = trays.get_mut(conversation_id) else {
            return;
        };
        if let Some(at) = tray.iter().position(|s| s.path == path) {
            let gone = tray.remove(at);
            let _ = std::fs::remove_file(&gone.path);
        }
    }

    /// Hands the files a send names to the turn, in the composer's order.
    /// A path this chat didn't stage is refused, so a caller can't point the
    /// agent at another file on this computer.
    pub fn take(&self, conversation_id: &str, paths: &[String]) -> Result<Vec<Staged>, String> {
        if paths.is_empty() {
            return Ok(Vec::new());
        }
        if paths.len() > PENDING_ATTACH_MAX {
            return Err(pending_item_user_message(PendingItemStatus::Invalid(
                PendingValidateIssue::TooMany,
            ))
            .unwrap_or("too many attachments")
            .to_string());
        }
        let mut trays = self.lock();
        let tray = trays.entry(conversation_id.to_string()).or_default();
        let mut out = Vec::with_capacity(paths.len());
        for path in paths {
            let found = tray
                .iter()
                .find(|s| &s.path == path)
                .cloned()
                .ok_or_else(|| "That file is no longer attached. Add it again.".to_string())?;
            out.push(found);
        }
        tray.retain(|s| !paths.contains(&s.path));
        Ok(out)
    }

    /// New chat: its waiting files are forgotten (their copies stay with the
    /// chat's folder).
    pub fn forget(&self, conversation_id: &str) {
        self.lock().remove(conversation_id);
    }
}

fn data_url(item: &Staged) -> Result<String, String> {
    let mut bytes = Vec::new();
    std::fs::File::open(&item.path)
        .and_then(|f| f.take(IMAGE_FILE_CAP + 1).read_to_end(&mut bytes))
        .map_err(|_| format!("{}: can't open the file", item.name))?;
    if bytes.len() as u64 > IMAGE_FILE_CAP {
        return Err(too_large(&item.name));
    }
    let mime = image_mime(&bytes).ok_or_else(|| unsupported(&item.name))?;
    Ok(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&bytes)
    ))
}

/// The message for the engine. A file the model can't read stops the send
/// with the cabin's note, so nothing goes out half attached.
pub fn compose(text: &str, items: &[Staged], model_mods: &[String]) -> Result<Composed, String> {
    let mods = refs(model_mods);
    let mut out = text.trim_end().to_string();
    let mut image = None;
    for item in items {
        if let Some(note) = model_note(item.kind, &mods) {
            return Err(format!("{}: {note}", item.name));
        }
        let line = match item.kind {
            Kind::Image if image.is_none() => {
                image = Some(data_url(item)?);
                attach_send_line(AttachKind::Image, &item.name, &item.path)
            }
            // One picture per message goes to the model, as in the cabin; the
            // rest are named with their paths.
            Kind::Image => attach_send_line(AttachKind::Other, &item.name, &item.path),
            Kind::Text => attach_send_line(AttachKind::Text, &item.name, &item.path),
            Kind::Document => attach_send_line(AttachKind::Doc, &item.name, &item.path),
        };
        out = append_composer(&out, &line);
    }
    Ok(Composed { text: out, image })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-shell-attach-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    const BOTH: &[&str] = &["text", "image"];

    /// A PNG header claiming `width` by `height`; nothing past IHDR.
    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut out = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        out.extend_from_slice(&13u32.to_be_bytes());
        out.extend_from_slice(b"IHDR");
        out.extend_from_slice(&width.to_be_bytes());
        out.extend_from_slice(&height.to_be_bytes());
        out.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
        out
    }

    #[test]
    fn a_text_file_is_copied_privately_into_the_chat_folder() {
        let config = temp_dir("text");
        let trays = Trays::default();
        let staged = trays
            .stage(&config, "shell-a1", "notes.md", b"# plan\n", BOTH)
            .expect("staged");
        assert_eq!(staged.name, "notes.md");
        assert_eq!(staged.kind, Kind::Text);
        assert_eq!(staged.label, "File notes.md");
        assert_eq!(staged.status, None);
        let path = PathBuf::from(&staged.path);
        assert_eq!(
            path.parent(),
            Some(config.join("sessions/shell-a1/attach").as_path())
        );
        assert_eq!(std::fs::read(&path).expect("copy"), b"# plan\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).expect("meta").permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn an_oversized_picture_is_refused_and_nothing_is_written() {
        let config = temp_dir("big");
        let trays = Trays::default();
        let mut bytes = png(100, 100);
        bytes.resize(IMAGE_FILE_CAP as usize + 1, 0);
        assert_eq!(
            trays.stage(&config, "shell-a2", "huge.png", &bytes, BOTH),
            Err("huge.png: file is too large".to_string())
        );
        assert!(!config.join("sessions/shell-a2/attach").exists());
    }

    #[test]
    fn a_png_that_claims_too_many_pixels_is_refused() {
        let config = temp_dir("pixels");
        let trays = Trays::default();
        assert_eq!(
            trays.stage(&config, "shell-a3", "bomb.png", &png(50_000, 50_000), BOTH),
            Err("bomb.png: file is too large".to_string())
        );
        // 6000 x 6000 is exactly the cap.
        let ok = trays
            .stage(&config, "shell-a3", "big.png", &png(6_000, 6_000), BOTH)
            .expect("at the cap");
        assert_eq!(ok.label, "Picture big.png");
    }

    #[test]
    fn a_text_file_past_its_cap_and_an_unknown_type_are_refused() {
        let config = temp_dir("caps");
        let trays = Trays::default();
        let long = vec![b'a'; TEXT_FILE_CAP + 1];
        assert_eq!(
            trays.stage(&config, "shell-a4", "log.txt", &long, BOTH),
            Err("log.txt: file is too large".to_string())
        );
        assert_eq!(
            trays.stage(&config, "shell-a4", "tool.exe", b"MZ", BOTH),
            Err("tool.exe: file type is not supported".to_string())
        );
        // A .png that isn't one is refused too.
        assert_eq!(
            trays.stage(&config, "shell-a4", "fake.png", b"hello there friend", BOTH),
            Err("fake.png: file type is not supported".to_string())
        );
    }

    #[test]
    fn the_eleventh_attachment_is_refused() {
        let config = temp_dir("eleven");
        let trays = Trays::default();
        for n in 0..PENDING_ATTACH_MAX {
            trays
                .stage(&config, "shell-a5", &format!("f{n}.txt"), b"x", BOTH)
                .expect("room");
        }
        assert_eq!(
            trays.stage(&config, "shell-a5", "f10.txt", b"x", BOTH),
            Err("f10.txt: too many attachments".to_string())
        );
        // Another chat has its own ten.
        assert!(trays
            .stage(&config, "shell-b5", "f0.txt", b"x", BOTH)
            .is_ok());
    }

    #[test]
    fn a_picture_on_a_text_only_model_carries_the_core_note_and_wont_send() {
        let config = temp_dir("textonly");
        let trays = Trays::default();
        let staged = trays
            .stage(&config, "shell-a6", "shot.png", &png(4, 4), &["text"])
            .expect("staged");
        assert_eq!(staged.status.as_deref(), Some("model can't read this file"));
        assert_eq!(
            compose("look", &[staged], &["text".to_string()]),
            Err("shot.png: model can't read this file".to_string())
        );
    }

    #[test]
    fn a_bad_chat_id_cant_reach_outside_the_sessions_folder() {
        let config = temp_dir("ids");
        let trays = Trays::default();
        assert_eq!(
            trays.stage(&config, "../../etc", "a.txt", b"x", BOTH),
            Err("That chat can't take files.".to_string())
        );
        let staged = trays
            .stage(&config, "shell-a7", "../../a b?.txt", b"x", BOTH)
            .expect("staged");
        assert_eq!(staged.name, "a b_.txt");
    }

    #[test]
    fn take_hands_over_only_this_chats_files_once() {
        let config = temp_dir("take");
        let trays = Trays::default();
        let a = trays
            .stage(&config, "shell-a8", "a.txt", b"x", BOTH)
            .expect("a");
        let elsewhere = config.join("secret.txt");
        std::fs::write(&elsewhere, "key").expect("write");
        assert_eq!(
            trays.take("shell-a8", &[elsewhere.display().to_string()]),
            Err("That file is no longer attached. Add it again.".to_string())
        );
        assert_eq!(
            trays.take("shell-a8", std::slice::from_ref(&a.path)),
            Ok(vec![a.clone()])
        );
        assert_eq!(
            trays.take("shell-a8", std::slice::from_ref(&a.path)),
            Err("That file is no longer attached. Add it again.".to_string())
        );
    }

    #[test]
    fn remove_deletes_the_copy() {
        let config = temp_dir("remove");
        let trays = Trays::default();
        let a = trays
            .stage(&config, "shell-a9", "a.txt", b"x", BOTH)
            .expect("a");
        trays.remove("shell-a9", &a.path);
        assert!(!Path::new(&a.path).exists());
        assert_eq!(
            trays.take("shell-a9", std::slice::from_ref(&a.path)),
            Err("That file is no longer attached. Add it again.".to_string())
        );
    }

    #[test]
    fn a_path_is_size_checked_before_it_is_read() {
        let config = temp_dir("path");
        let src = config.join("big.txt");
        std::fs::write(&src, vec![b'a'; TEXT_FILE_CAP + 1]).expect("write");
        let trays = Trays::default();
        assert_eq!(
            trays.stage_path(&config, "shell-a10", &src, BOTH),
            Err("big.txt: file is too large".to_string())
        );
        let small = config.join("small.rs");
        std::fs::write(&small, "fn main() {}").expect("write");
        let staged = trays
            .stage_path(&config, "shell-a10", &small, BOTH)
            .expect("staged");
        assert_eq!(staged.label, "File small.rs");
        assert_ne!(PathBuf::from(&staged.path), small);
        assert_eq!(
            std::fs::read_to_string(&staged.path).expect("copy"),
            "fn main() {}"
        );
    }

    #[test]
    fn compose_sends_the_first_picture_as_image_input_and_names_the_rest() {
        let config = temp_dir("compose");
        let trays = Trays::default();
        let shot = trays
            .stage(&config, "shell-a11", "shot.png", &png(2, 2), BOTH)
            .expect("shot");
        let second = trays
            .stage(&config, "shell-a11", "two.png", &png(2, 2), BOTH)
            .expect("two");
        let notes = trays
            .stage(&config, "shell-a11", "notes.md", b"hi", BOTH)
            .expect("notes");
        let mods = vec!["text".to_string(), "image".to_string()];
        let out = compose(
            "what is this",
            &[shot.clone(), second.clone(), notes.clone()],
            &mods,
        )
        .expect("composed");
        let expected_image = format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(png(2, 2))
        );
        assert_eq!(out.image.as_deref(), Some(expected_image.as_str()));
        assert_eq!(
            out.text,
            format!(
                "what is this\nfrom reference shot.png\nFile two.png is on this computer at {}. Read it yourself. Do not ask for the contents to be pasted.\nFile notes.md is on this computer at {}. Read it yourself. Do not ask for the contents to be pasted.",
                second.path, notes.path
            )
        );
    }

    #[test]
    fn base64_from_the_ui_stages_and_junk_is_refused() {
        let config = temp_dir("b64");
        let trays = Trays::default();
        let data = base64::engine::general_purpose::STANDARD.encode(b"hello");
        let staged = trays
            .stage_base64(&config, "shell-a12", "hi.txt", &data, BOTH)
            .expect("staged");
        assert_eq!(std::fs::read(&staged.path).expect("copy"), b"hello");
        assert_eq!(
            trays.stage_base64(&config, "shell-a12", "hi.txt", "%%%", BOTH),
            Err("hi.txt: file type is not supported".to_string())
        );
    }
}
