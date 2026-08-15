//! Plus-button upload: classify files, parse picker output, compose attach lines.

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
