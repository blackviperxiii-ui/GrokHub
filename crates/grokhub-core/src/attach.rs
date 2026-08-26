//! Plus-button upload: classify files, parse picker output, compose attach lines.

use std::borrow::Cow;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachKind {
    Image,
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
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" => AttachKind::Image,
        "txt" | "md" | "rs" | "toml" | "json" | "log" | "csv" | "xml" | "yaml" | "yml" | "sh"
        | "py" | "js" | "ts" | "html" | "css" => AttachKind::Text,
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
        "zenity" | "qarma" => Some(vec!["--file-selection".into(), "--title=Upload".into()]),
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
        AttachKind::Text => format!("from file {name}"),
        AttachKind::Other => format!("file {name}"),
    }
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
pub fn this_turn_cabin_frame<'a>(
    eyes_turn: bool,
    hands_turn: bool,
    captured_this_turn: Option<&'a str>,
) -> Option<&'a str> {
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
        AttachKind::Text => format!("Pasted {name}"),
        AttachKind::Other => format!("Added path {name}"),
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
    }

    #[test]
    fn parse_picker_stdout_takes_first_path() {
        assert_eq!(
            parse_picker_stdout("/home/viper/shot.png\n"),
            Some("/home/viper/shot.png".into())
        );
        assert_eq!(parse_picker_stdout("  \n  /tmp/a.txt  \n"), Some("/tmp/a.txt".into()));
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
        assert_eq!(take_text_body(&"x".repeat(TEXT_FILE_CAP + 8)).len(), TEXT_FILE_CAP);
    }

    #[test]
    fn user_image_wins_over_cabin_frame() {
        assert_eq!(next_chat_image(Some("data:user"), Some("data:cabin")), Some("data:user"));
        assert_eq!(next_chat_image(None, Some("data:cabin")), Some("data:cabin"));
        assert_eq!(next_chat_image(Some(""), Some("data:cabin")), Some("data:cabin"));
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
        assert_eq!(list_pick_names(&[".hidden", "b.txt", "a.png", ""]), vec!["a.png", "b.txt"]);
        let _ = PlusTarget::Chat;
        let _ = PlusTarget::Imagine;
    }
}
