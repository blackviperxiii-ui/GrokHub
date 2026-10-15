//! Plus-button upload: classify files, parse picker output, compose attach lines.

use std::borrow::Cow;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachKind {
    Image,
    Video,
    Doc,
    Text,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlusTarget {
    Chat,
    Imagine,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlusAct {
    Upload,
    Paste,
}

pub const TEXT_FILE_CAP: usize = 64 * 1024;
/// Plus-button and Imagine wall stills. Bigger files decode on the UI thread and can OOM.
pub const IMAGE_FILE_CAP: u64 = 8 * 1024 * 1024;
/// Imagine video / TTS download. A huge body must not fill RAM.
pub const MEDIA_FILE_CAP: u64 = 64 * 1024 * 1024;
/// 8K still fits. A tiny PNG that claims 50k×50k must not decode on the UI thread.
pub const IMAGE_PIXEL_CAP: u64 = 36_000_000;

pub fn image_pixels_ok(width: u32, height: u32) -> bool {
    (width as u64).saturating_mul(height as u64) <= IMAGE_PIXEL_CAP
}

fn scan_end(s: &str, cap: usize) -> usize {
    let mut end = cap.min(s.len());
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    end
}

fn scan_start(s: &str, cap: usize) -> usize {
    if s.len() <= cap {
        return 0;
    }
    let mut start = s.len() - cap;
    while start < s.len() && !s.is_char_boundary(start) {
        start += 1;
    }
    start
}

/// Head plus tail so end markers like `GOAL_COMPLETE` survive a huge complete.
pub fn bound_scan(s: &str) -> Cow<'_, str> {
    if s.len() <= TEXT_FILE_CAP {
        return Cow::Borrowed(s);
    }
    let half = TEXT_FILE_CAP / 2;
    let head = scan_end(s, half);
    let tail = scan_start(s, half);
    if tail <= head {
        return Cow::Borrowed(&s[..scan_end(s, TEXT_FILE_CAP)]);
    }
    let mut out = String::with_capacity(head + s.len() - tail);
    out.push_str(&s[..head]);
    out.push_str(&s[tail..]);
    Cow::Owned(out)
}

/// PNG IHDR only — do not decode IDAT. A 50k×50k bomb is still a tiny file.
pub fn png_ihdr_size(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 24 {
        return None;
    }
    if bytes[0..8] != [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a] {
        return None;
    }
    if &bytes[12..16] != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(bytes[16..20].try_into().ok()?);
    let height = u32::from_be_bytes(bytes[20..24].try_into().ok()?);
    Some((width, height))
}

pub fn attach_kind(path: &str) -> AttachKind {
    let ext = std::path::Path::new(path)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "svg" => AttachKind::Image,
        "mp4" | "webm" | "mov" | "mkv" | "m4v" => AttachKind::Video,
        "pdf" | "doc" | "docx" | "odt" | "rtf" => AttachKind::Doc,
        "txt" | "md" | "rs" | "toml" | "json" | "log" | "csv" | "xml" | "yaml" | "yml" | "sh"
        | "py" | "js" | "ts" | "tsx" | "jsx" | "html" | "css" | "go" | "c" | "h" | "cpp"
        | "hpp" | "java" | "rb" | "php" | "sql" | "ini" | "conf" | "lock" | "rst" | "tex" => {
            AttachKind::Text
        }
        _ => AttachKind::Other,
    }
}

pub fn parse_picker_stdout(stdout: &str) -> Option<String> {
    stdout
        .lines()
        .map(str::trim)
        .find(|s| !s.is_empty())
        .map(|s| s.to_string())
}

pub fn picker_args(bin: &str) -> Option<Vec<String>> {
    match bin {
        "zenity" | "qarma" => Some(vec![
            "--file-selection".into(),
            "--title=Add to chat".into(),
        ]),
        "kdialog" => Some(vec![
            "--getopenfilename".into(),
            ".".into(),
            "All files (*)".into(),
        ]),
        "yad" => Some(vec!["--file".into(), "--title=Upload".into()]),
        _ => None,
    }
}

pub fn picker_save_args(bin: &str, filename: &str) -> Option<Vec<String>> {
    let filename = filename.trim();
    if filename.is_empty() {
        return None;
    }
    match bin {
        "zenity" | "qarma" => Some(vec![
            "--file-selection".into(),
            "--save".into(),
            "--confirm-overwrite".into(),
            format!("--filename={filename}"),
        ]),
        "kdialog" => Some(vec!["--getsavefilename".into(), filename.into()]),
        "yad" => Some(vec![
            "--file".into(),
            "--save".into(),
            format!("--filename={filename}"),
        ]),
        _ => None,
    }
}

pub fn clip_image_args(bin: &str) -> Option<Vec<String>> {
    match bin {
        "xclip" => Some(vec![
            "-selection".into(),
            "clipboard".into(),
            "-t".into(),
            "image/png".into(),
            "-o".into(),
        ]),
        "wl-paste" => Some(vec!["--type".into(), "image/png".into()]),
        _ => None,
    }
}

pub fn attach_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(path)
        .to_string()
}

pub fn attach_prompt_line(kind: AttachKind, name: &str) -> String {
    match kind {
        AttachKind::Image => format!("from reference {name}"),
        AttachKind::Video => format!("from video {name}"),
        AttachKind::Doc => format!("from document {name}"),
        AttachKind::Text => format!("from file {name}"),
        AttachKind::Other => format!("file {name}"),
    }
}

/// What the user message should say when a file is waiting on the composer.
/// Pictures stay a short reference. Video and documents name the path so the
/// cabin can open them on this computer.
pub fn attach_send_line(kind: AttachKind, name: &str, path: &str) -> String {
    let path = path.trim();
    match kind {
        AttachKind::Image => attach_prompt_line(kind, name),
        AttachKind::Video if !path.is_empty() => {
            format!("Video {name} is on this computer at {path}. Look at it.")
        }
        AttachKind::Doc if !path.is_empty() => {
            format!("Document {name} is on this computer at {path}. Read it.")
        }
        AttachKind::Text | AttachKind::Other if !path.is_empty() => {
            format!("File {name} is on this computer at {path}. Read it yourself. Do not ask for the contents to be pasted.")
        }
        other => attach_prompt_line(other, name),
    }
}

pub fn attach_chip_label(kind: AttachKind, name: &str) -> String {
    let word = match kind {
        AttachKind::Image => "Picture",
        AttachKind::Video => "Video",
        AttachKind::Doc => "Document",
        AttachKind::Text | AttachKind::Other => "File",
    };
    format!("{word} {name}")
}

pub fn append_composer(existing: &str, incoming: &str) -> String {
    let incoming = incoming.trim_end();
    if incoming.is_empty() {
        return existing.to_string();
    }
    if existing.is_empty() {
        return incoming.to_string();
    }
    if existing.ends_with('\n') {
        format!("{existing}{incoming}")
    } else {
        format!("{existing}\n{incoming}")
    }
}

pub fn take_text_body(s: &str) -> String {
    if s.len() <= TEXT_FILE_CAP {
        s.to_string()
    } else {
        s.chars().take(TEXT_FILE_CAP).collect()
    }
}

pub fn next_chat_image<'a>(user: Option<&'a str>, cabin: Option<&'a str>) -> Option<&'a str> {
    user.filter(|s| !s.is_empty())
        .or_else(|| cabin.filter(|s| !s.is_empty()))
}

/// Only a typed send / retry should consume the plus-button image.
pub fn kick_consumes_attach(user_originated: bool) -> bool {
    user_originated
}

/// Only a frame captured on this turn may go to the model.
/// A leftover desktop or webcam JPEG is not a this-turn capture.
pub fn this_turn_cabin_frame(
    eyes_turn: bool,
    hands_turn: bool,
    captured_this_turn: Option<&str>,
) -> Option<&str> {
    let url = captured_this_turn.filter(|s| !s.is_empty())?;
    if crate::recipe::should_attach_hands_frame(eyes_turn, hands_turn, true) {
        Some(url)
    } else {
        None
    }
}

/// True when the pixel is a cabin/hands frame, not a user drop.
pub fn cabin_frame_only(user: Option<&str>, cabin: Option<&str>) -> bool {
    user.filter(|s| !s.is_empty()).is_none() && cabin.filter(|s| !s.is_empty()).is_some()
}

/// Request-only note so the model does not narrate “an image is attached”.
pub fn cabin_eyes_request_text(user_text: &str) -> String {
    const NOTE: &str = "Cabin eyes sent a desktop frame. Do not say an image is attached.";
    let t = user_text.trim_end();
    if t.is_empty() {
        NOTE.to_string()
    } else {
        format!("{t}\n\n{NOTE}")
    }
}

pub fn plus_menu_rows() -> &'static [(&'static str, PlusAct)] {
    &[
        ("Upload file", PlusAct::Upload),
        ("Paste clipboard", PlusAct::Paste),
    ]
}

pub fn list_pick_names(names: &[&str]) -> Vec<String> {
    let mut out: Vec<String> = names
        .iter()
        .filter(|n| !n.is_empty() && !n.starts_with('.'))
        .map(|s| (*s).to_string())
        .collect();
    out.sort();
    out
}

pub fn plus_empty_status() -> &'static str {
    "pick a file or copy something first"
}

pub fn imagine_ref_status(name: &str) -> String {
    format!("cabin stills are prompt-only — {name} added as a hint")
}

pub fn chat_attach_status(kind: AttachKind, name: &str) -> String {
    match kind {
        AttachKind::Image => format!("Attached {name} — sends with the next message"),
        AttachKind::Video => format!("Video {name} — the agent will read it"),
        AttachKind::Doc => format!("Document {name} — the agent will read it"),
        AttachKind::Text => format!("File {name} — the agent will read it"),
        AttachKind::Other => format!("File {name} — the agent will read it"),
    }
}

/// Composer plus-button queue. Staged files are not sent until the user sends a message.
pub const PENDING_ATTACH_MAX: usize = 10;
/// xAI vision accepts JPEG/PNG up to this size.
pub const PENDING_VISION_IMAGE_MAX: u64 = 20 * 1024 * 1024;
pub const MODEL_CANT_READ_FILE: &str = "model can't read this file";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingFileRole {
    VisionImage,
    Document,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingValidateIssue {
    TooMany,
    TooLarge,
    UnsupportedType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PendingItemStatus {
    Valid(PendingFileRole),
    NeedsTranscode,
    Invalid(PendingValidateIssue),
    ModelCantRead,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingAttachEntry {
    pub path: String,
    pub size_bytes: u64,
    pub status: PendingItemStatus,
}

/// Staged attachments waiting on the composer (never auto-send).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PendingAttach {
    items: Vec<PendingAttachEntry>,
}

impl PendingAttach {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn items(&self) -> &[PendingAttachEntry] {
        &self.items
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Stage one file. Returns `TooMany` when the list is already at [`PENDING_ATTACH_MAX`].
    pub fn stage(&mut self, path: String, size_bytes: u64) -> Result<(), PendingValidateIssue> {
        if self.items.len() >= PENDING_ATTACH_MAX {
            return Err(PendingValidateIssue::TooMany);
        }
        let status = validate_pending_path(&path, size_bytes);
        self.items.push(PendingAttachEntry {
            path,
            size_bytes,
            status,
        });
        Ok(())
    }

    /// Mark items the active model cannot consume (for example vision images on a text-only model).
    pub fn apply_model_input_modalities(&mut self, input_modalities: &[&str]) {
        for item in &mut self.items {
            item.status = pending_item_with_model(item.status, input_modalities);
        }
    }

    pub fn valid_items(&self) -> usize {
        self.items
            .iter()
            .filter(|e| {
                matches!(
                    e.status,
                    PendingItemStatus::Valid(_) | PendingItemStatus::NeedsTranscode
                )
            })
            .count()
    }
}

fn path_ext_lower(path: &str) -> String {
    std::path::Path::new(path)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn vision_native_ext(ext: &str) -> bool {
    matches!(ext, "png" | "jpg" | "jpeg")
}

fn pending_doc_path(path: &str) -> bool {
    let kind = attach_kind(path);
    match kind {
        AttachKind::Text => true,
        AttachKind::Doc => path_ext_lower(path) == "pdf",
        AttachKind::Image | AttachKind::Video | AttachKind::Other => false,
    }
}

/// File-type and size rules for one staged path (no model check).
pub fn validate_pending_path(path: &str, size_bytes: u64) -> PendingItemStatus {
    let ext = path_ext_lower(path);
    let kind = attach_kind(path);
    match kind {
        AttachKind::Video | AttachKind::Other => {
            PendingItemStatus::Invalid(PendingValidateIssue::UnsupportedType)
        }
        AttachKind::Doc if ext != "pdf" => {
            PendingItemStatus::Invalid(PendingValidateIssue::UnsupportedType)
        }
        AttachKind::Image => {
            if size_bytes > PENDING_VISION_IMAGE_MAX {
                return PendingItemStatus::Invalid(PendingValidateIssue::TooLarge);
            }
            if vision_native_ext(&ext) {
                PendingItemStatus::Valid(PendingFileRole::VisionImage)
            } else {
                PendingItemStatus::NeedsTranscode
            }
        }
        AttachKind::Text | AttachKind::Doc => {
            if pending_doc_path(path) {
                PendingItemStatus::Valid(PendingFileRole::Document)
            } else {
                PendingItemStatus::Invalid(PendingValidateIssue::UnsupportedType)
            }
        }
    }
}

fn model_reads_role(role: PendingFileRole, input_modalities: &[&str]) -> bool {
    match role {
        PendingFileRole::Document => input_modalities.contains(&"text"),
        PendingFileRole::VisionImage => input_modalities.contains(&"image"),
    }
}

fn model_reads_transcoded_image(input_modalities: &[&str]) -> bool {
    input_modalities.contains(&"image")
}

/// Apply model listing modalities to a validated item status.
pub fn pending_item_with_model(
    status: PendingItemStatus,
    input_modalities: &[&str],
) -> PendingItemStatus {
    match status {
        PendingItemStatus::Valid(PendingFileRole::VisionImage) => {
            if model_reads_role(PendingFileRole::VisionImage, input_modalities) {
                status
            } else {
                PendingItemStatus::ModelCantRead
            }
        }
        PendingItemStatus::NeedsTranscode => {
            if model_reads_transcoded_image(input_modalities) {
                status
            } else {
                PendingItemStatus::ModelCantRead
            }
        }
        PendingItemStatus::Valid(PendingFileRole::Document) => {
            if model_reads_role(PendingFileRole::Document, input_modalities) {
                status
            } else {
                PendingItemStatus::ModelCantRead
            }
        }
        PendingItemStatus::Invalid(_) | PendingItemStatus::ModelCantRead => status,
    }
}

pub fn pending_item_user_message(status: PendingItemStatus) -> Option<&'static str> {
    match status {
        PendingItemStatus::ModelCantRead => Some(MODEL_CANT_READ_FILE),
        PendingItemStatus::Invalid(PendingValidateIssue::TooLarge) => Some("file is too large"),
        PendingItemStatus::Invalid(PendingValidateIssue::UnsupportedType) => {
            Some("file type is not supported")
        }
        PendingItemStatus::Invalid(PendingValidateIssue::TooMany) => Some("too many attachments"),
        PendingItemStatus::Valid(_) | PendingItemStatus::NeedsTranscode => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bound_scan_keeps_the_tail_of_a_huge_body() {
        let mut huge = "a".repeat(TEXT_FILE_CAP + 8);
        huge.push_str("GOAL_COMPLETE");
        let scan = bound_scan(&huge);
        assert!(scan.len() <= TEXT_FILE_CAP);
        assert!(scan.contains("GOAL_COMPLETE"));
        assert_eq!(bound_scan("short").as_ref(), "short");
    }

    #[test]
    fn image_pixels_ok_rejects_a_decompression_bomb() {
        assert!(image_pixels_ok(3840, 2160));
        assert!(image_pixels_ok(7680, 4320));
        assert!(!image_pixels_ok(50_000, 50_000));
        assert!(!image_pixels_ok(u32::MAX, 2));
        assert_eq!(IMAGE_PIXEL_CAP, 36_000_000);
        assert_eq!(MEDIA_FILE_CAP, 64 * 1024 * 1024);
        let mut hdr = vec![0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];
        hdr.extend_from_slice(&13u32.to_be_bytes());
        hdr.extend_from_slice(b"IHDR");
        hdr.extend_from_slice(&50_000u32.to_be_bytes());
        hdr.extend_from_slice(&50_000u32.to_be_bytes());
        assert_eq!(png_ihdr_size(&hdr), Some((50_000, 50_000)));
        assert!(png_ihdr_size(b"not a png").is_none());
    }

    #[test]
    fn classifies_image_text_other() {
        assert_eq!(attach_kind("shot.PNG"), AttachKind::Image);
        assert_eq!(attach_kind("/tmp/a.jpeg"), AttachKind::Image);
        assert_eq!(attach_kind("notes.md"), AttachKind::Text);
        assert_eq!(attach_kind("main.rs"), AttachKind::Text);
        assert_eq!(attach_kind("Cargo.toml"), AttachKind::Text);
        assert_eq!(attach_kind("bin.elf"), AttachKind::Other);
        assert_eq!(attach_kind("noext"), AttachKind::Other);
        assert_eq!(attach_kind("clip.MP4"), AttachKind::Video);
        assert_eq!(attach_kind("/tmp/notes.pdf"), AttachKind::Doc);
        assert_eq!(attach_kind("main.go"), AttachKind::Text);
        assert!(
            attach_send_line(AttachKind::Video, "clip.mp4", "/tmp/clip.mp4")
                .contains("/tmp/clip.mp4")
        );
        assert!(
            attach_send_line(AttachKind::Doc, "notes.pdf", "/tmp/notes.pdf").contains("Read it")
        );
        assert_eq!(
            attach_send_line(AttachKind::Image, "ref.png", ""),
            "from reference ref.png"
        );
    }

    #[test]
    fn parse_picker_stdout_takes_first_path() {
        assert_eq!(
            parse_picker_stdout("/home/user/shot.png\n"),
            Some("/home/user/shot.png".into())
        );
        assert_eq!(
            parse_picker_stdout("  \n  /tmp/a.txt  \n"),
            Some("/tmp/a.txt".into())
        );
        assert_eq!(parse_picker_stdout(""), None);
        assert_eq!(parse_picker_stdout("   \n"), None);
    }

    #[test]
    fn picker_and_clip_image_args() {
        let z = picker_args("zenity").expect("zenity");
        assert!(z.iter().any(|a| a.contains("file-selection")));
        assert!(picker_args("kdialog").is_some());
        assert!(picker_args("yad").is_some());
        assert!(picker_args("qarma").is_some());
        assert!(picker_args("not-a-picker").is_none());
        let save = picker_save_args("zenity", "night.png").expect("save");
        assert!(save.iter().any(|a| a == "--save"));
        assert!(save.iter().any(|a| a.contains("night.png")));
        assert!(picker_save_args("kdialog", "clip.mp4").is_some());
        assert!(picker_save_args("zenity", "  ").is_none());
        let x = clip_image_args("xclip").expect("xclip");
        assert!(x.iter().any(|a| a.contains("image/png")));
        assert!(clip_image_args("wl-paste").is_some());
        assert!(clip_image_args("xsel").is_none());
    }

    #[test]
    fn attach_lines_and_composer() {
        assert_eq!(attach_name("/tmp/ref.png"), "ref.png");
        assert_eq!(
            attach_prompt_line(AttachKind::Image, "ref.png"),
            "from reference ref.png"
        );
        assert_eq!(append_composer("", "hello"), "hello");
        assert_eq!(append_composer("hi", "there"), "hi\nthere");
        assert_eq!(append_composer("hi\n", "there"), "hi\nthere");
        assert_eq!(take_text_body("abc"), "abc");
        assert_eq!(
            take_text_body(&"x".repeat(TEXT_FILE_CAP + 8)).len(),
            TEXT_FILE_CAP
        );
    }

    #[test]
    fn user_image_wins_over_cabin_frame() {
        assert_eq!(
            next_chat_image(Some("data:user"), Some("data:cabin")),
            Some("data:user")
        );
        assert_eq!(
            next_chat_image(None, Some("data:cabin")),
            Some("data:cabin")
        );
        assert_eq!(
            next_chat_image(Some(""), Some("data:cabin")),
            Some("data:cabin")
        );
        assert_eq!(next_chat_image(None, None), None);
        assert!(kick_consumes_attach(true));
        assert!(
            !kick_consumes_attach(false),
            "followup/host/goal kicks must leave the attached image for the next send"
        );
        assert!(!cabin_frame_only(Some("data:user"), Some("data:cabin")));
        assert!(cabin_frame_only(None, Some("data:cabin")));
        assert!(cabin_frame_only(Some(""), Some("data:cabin")));
        assert!(!cabin_frame_only(None, None));
        assert_eq!(
            cabin_eyes_request_text("what's in the bowl"),
            "what's in the bowl\n\nCabin eyes sent a desktop frame. Do not say an image is attached."
        );
        assert_eq!(
            cabin_eyes_request_text("  "),
            "Cabin eyes sent a desktop frame. Do not say an image is attached."
        );
        assert_eq!(
            this_turn_cabin_frame(true, false, None),
            None,
            "a failed capture must not attach a stale or webcam frame"
        );
        assert_eq!(
            this_turn_cabin_frame(true, false, Some("data:cabin-now")),
            Some("data:cabin-now")
        );
        assert_eq!(
            this_turn_cabin_frame(false, false, Some("data:stale")),
            None
        );
    }

    #[test]
    fn plus_menu_and_empty_status() {
        let rows = plus_menu_rows();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0], ("Upload file", PlusAct::Upload));
        assert_eq!(rows[1], ("Paste clipboard", PlusAct::Paste));
        assert!(plus_empty_status().contains("file") || plus_empty_status().contains("copy"));
        assert!(imagine_ref_status("ref.png").contains("prompt-only"));
        assert!(chat_attach_status(AttachKind::Image, "ref.png").contains("ref.png"));
        assert_eq!(
            list_pick_names(&[".hidden", "b.txt", "a.png", ""]),
            vec!["a.png", "b.txt"]
        );
        let _ = PlusTarget::Chat;
        let _ = PlusTarget::Imagine;
    }

    #[test]
    fn pending_attach_validation_table() {
        struct Row {
            path: &'static str,
            size: u64,
            expect: PendingItemStatus,
        }
        let rows = [
            Row {
                path: "shot.png",
                size: 1,
                expect: PendingItemStatus::Valid(PendingFileRole::VisionImage),
            },
            Row {
                path: "shot.JPG",
                size: PENDING_VISION_IMAGE_MAX,
                expect: PendingItemStatus::Valid(PendingFileRole::VisionImage),
            },
            Row {
                path: "big.png",
                size: PENDING_VISION_IMAGE_MAX + 1,
                expect: PendingItemStatus::Invalid(PendingValidateIssue::TooLarge),
            },
            Row {
                path: "anim.webp",
                size: 1024,
                expect: PendingItemStatus::NeedsTranscode,
            },
            Row {
                path: "notes.md",
                size: 50,
                expect: PendingItemStatus::Valid(PendingFileRole::Document),
            },
            Row {
                path: "data.csv",
                size: 50,
                expect: PendingItemStatus::Valid(PendingFileRole::Document),
            },
            Row {
                path: "packet.json",
                size: 50,
                expect: PendingItemStatus::Valid(PendingFileRole::Document),
            },
            Row {
                path: "/tmp/readme.pdf",
                size: 50,
                expect: PendingItemStatus::Valid(PendingFileRole::Document),
            },
            Row {
                path: "main.rs",
                size: 50,
                expect: PendingItemStatus::Valid(PendingFileRole::Document),
            },
            Row {
                path: "clip.mp4",
                size: 50,
                expect: PendingItemStatus::Invalid(PendingValidateIssue::UnsupportedType),
            },
            Row {
                path: "memo.docx",
                size: 50,
                expect: PendingItemStatus::Invalid(PendingValidateIssue::UnsupportedType),
            },
            Row {
                path: "bin.elf",
                size: 50,
                expect: PendingItemStatus::Invalid(PendingValidateIssue::UnsupportedType),
            },
        ];
        for row in rows {
            assert_eq!(
                validate_pending_path(row.path, row.size),
                row.expect,
                "path={}",
                row.path
            );
        }
        assert_eq!(
            pending_item_with_model(
                PendingItemStatus::Valid(PendingFileRole::VisionImage),
                &["text"],
            ),
            PendingItemStatus::ModelCantRead
        );
        assert_eq!(
            pending_item_user_message(PendingItemStatus::ModelCantRead),
            Some(MODEL_CANT_READ_FILE)
        );
        assert_eq!(
            pending_item_with_model(
                PendingItemStatus::Valid(PendingFileRole::VisionImage),
                &["text", "image"],
            ),
            PendingItemStatus::Valid(PendingFileRole::VisionImage)
        );
        assert_eq!(
            pending_item_with_model(PendingItemStatus::NeedsTranscode, &["text"]),
            PendingItemStatus::ModelCantRead
        );
    }

    #[test]
    fn pending_attach_caps_at_ten_items() {
        let mut q = PendingAttach::new();
        for i in 0..PENDING_ATTACH_MAX {
            assert!(q.stage(format!("/tmp/file{i}.png"), 1).is_ok());
        }
        assert_eq!(q.len(), PENDING_ATTACH_MAX);
        assert_eq!(
            q.stage("/tmp/overflow.png".into(), 1),
            Err(PendingValidateIssue::TooMany)
        );
        assert_eq!(q.len(), PENDING_ATTACH_MAX);
    }

    #[test]
    fn pending_attach_one_bad_file_leaves_siblings_valid() {
        let mut q = PendingAttach::new();
        q.stage("good.png".into(), 100).unwrap();
        q.stage("bad.bin".into(), 100).unwrap();
        q.stage("notes.txt".into(), 100).unwrap();
        assert_eq!(q.len(), 3);
        assert_eq!(q.valid_items(), 2);
        assert_eq!(
            q.items()[0].status,
            PendingItemStatus::Valid(PendingFileRole::VisionImage)
        );
        assert_eq!(
            q.items()[1].status,
            PendingItemStatus::Invalid(PendingValidateIssue::UnsupportedType)
        );
        assert_eq!(
            q.items()[2].status,
            PendingItemStatus::Valid(PendingFileRole::Document)
        );
        q.apply_model_input_modalities(&["text"]);
        assert_eq!(q.items()[0].status, PendingItemStatus::ModelCantRead);
        assert_eq!(
            q.items()[1].status,
            PendingItemStatus::Invalid(PendingValidateIssue::UnsupportedType)
        );
        assert_eq!(
            q.items()[2].status,
            PendingItemStatus::Valid(PendingFileRole::Document)
        );
    }
}
