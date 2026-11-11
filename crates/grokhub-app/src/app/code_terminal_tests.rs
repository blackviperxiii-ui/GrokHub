use super::*;
use std::time::{Duration, Instant};

fn screen(rows: u16, cols: u16) -> Screen {
    Screen::new(GridSize { rows, cols })
}

#[test]
fn the_grid_follows_the_panel_and_stays_in_the_backend_limits() {
    let cell = Vec2::new(8.0, 16.0);
    assert_eq!(
        grid_for(Vec2::new(640.0, 384.0), cell),
        GridSize { rows: 24, cols: 80 }
    );
    assert_eq!(
        grid_for(Vec2::new(645.0, 399.0), cell),
        GridSize { rows: 24, cols: 80 }
    );
    assert_eq!(
        grid_for(Vec2::new(4.0, 4.0), cell),
        GridSize { rows: 2, cols: 2 }
    );
    assert_eq!(
        grid_for(Vec2::new(100_000.0, 100_000.0), cell),
        GridSize {
            rows: 500,
            cols: 1000
        }
    );
    assert_eq!(
        grid_for(Vec2::new(640.0, 384.0), Vec2::ZERO),
        GridSize { rows: 2, cols: 2 }
    );
    assert_eq!(
        grid_for(Vec2::new(f32::NAN, 384.0), cell),
        GridSize { rows: 24, cols: 2 }
    );
}

#[test]
fn printed_text_lands_on_the_screen() {
    let mut s = screen(4, 20);
    assert!(s.feed("hello\r\nworld").is_empty());
    assert_eq!(s.lines(), vec!["hello", "world", "", ""]);
    assert_eq!(s.cursor(), Some((1, 5)));
}

#[test]
fn cursor_moves_and_erases_work_like_a_tui_needs() {
    let mut s = screen(3, 10);
    s.feed("abcdef\x1b[1;3HX\x1b[3;1Hbottom\x1b[2;1Hmid");
    assert_eq!(s.lines(), vec!["abXdef", "mid", "bottom"]);
    s.feed("\x1b[1;1H\x1b[2K");
    assert_eq!(s.lines(), vec!["", "mid", "bottom"]);
    s.feed("\x1b[2J");
    assert_eq!(s.lines(), vec!["", "", ""]);
}

#[test]
fn a_full_screen_app_gets_its_own_screen_and_leaves_the_shell_as_it_was() {
    let mut s = screen(3, 12);
    s.feed("$ less x");
    s.feed("\x1b[?1049h\x1b[H\x1b[2Jpage one");
    assert!(s.mode().contains(TermMode::ALT_SCREEN));
    assert_eq!(s.lines(), vec!["page one", "", ""]);
    s.feed("\x1b[?1049l");
    assert!(!s.mode().contains(TermMode::ALT_SCREEN));
    assert_eq!(s.lines(), vec!["$ less x", "", ""]);
}

#[test]
fn a_cursor_position_query_is_answered() {
    let mut s = screen(5, 20);
    s.feed("\x1b[3;7H");
    assert_eq!(s.feed("\x1b[6n"), vec!["\x1b[3;7R".to_string()]);
}

#[test]
fn modes_the_keys_depend_on_are_tracked() {
    let mut s = screen(3, 10);
    assert!(!s.mode().contains(TermMode::APP_CURSOR));
    s.feed("\x1b[?1h\x1b[?2004h");
    assert!(s.mode().contains(TermMode::APP_CURSOR));
    assert!(s.mode().contains(TermMode::BRACKETED_PASTE));
    s.feed("\x1b[?25l");
    assert_eq!(s.cursor(), None);
}

#[test]
fn a_resize_rewraps_and_reports_only_real_changes() {
    let mut s = screen(3, 10);
    assert!(!s.resize(GridSize { rows: 3, cols: 10 }));
    assert!(s.resize(GridSize { rows: 5, cols: 20 }));
    assert_eq!(s.size(), GridSize { rows: 5, cols: 20 });
    s.feed("0123456789abcdefghij");
    assert_eq!(s.lines()[0], "0123456789abcdefghij");
    assert_eq!(s.lines().len(), 5);
}

#[test]
fn scrolled_off_lines_can_be_scrolled_back_to() {
    let mut s = screen(2, 10);
    s.feed("one\r\ntwo\r\nthree\r\nfour");
    assert_eq!(s.lines(), vec!["three", "four"]);
    s.scroll(2);
    assert_eq!(s.display_offset(), 2);
    assert_eq!(s.lines(), vec!["one", "two"]);
    s.scroll_to_bottom();
    assert_eq!(s.lines(), vec!["three", "four"]);
}

#[test]
fn keys_become_the_bytes_a_shell_expects() {
    let none = Modifiers::NONE;
    let ctrl = Modifiers::CTRL;
    assert_eq!(key_bytes(Key::Enter, none, false), Some(b"\r".to_vec()));
    assert_eq!(key_bytes(Key::Backspace, none, false), Some(vec![0x7f]));
    assert_eq!(key_bytes(Key::Tab, none, false), Some(b"\t".to_vec()));
    assert_eq!(
        key_bytes(Key::Tab, Modifiers::SHIFT, false),
        Some(b"\x1b[Z".to_vec())
    );
    assert_eq!(key_bytes(Key::Escape, none, false), Some(vec![0x1b]));
    assert_eq!(key_bytes(Key::C, ctrl, false), Some(vec![0x03]));
    assert_eq!(key_bytes(Key::D, ctrl, false), Some(vec![0x04]));
    assert_eq!(key_bytes(Key::Z, ctrl, false), Some(vec![0x1a]));
    assert_eq!(key_bytes(Key::Space, ctrl, false), Some(vec![0]));
    assert_eq!(
        key_bytes(Key::ArrowUp, none, false),
        Some(b"\x1b[A".to_vec())
    );
    assert_eq!(
        key_bytes(Key::ArrowUp, none, true),
        Some(b"\x1bOA".to_vec())
    );
    assert_eq!(
        key_bytes(Key::ArrowLeft, ctrl, false),
        Some(b"\x1b[1;5D".to_vec())
    );
    assert_eq!(
        key_bytes(Key::Delete, none, false),
        Some(b"\x1b[3~".to_vec())
    );
    assert_eq!(
        key_bytes(Key::PageDown, none, false),
        Some(b"\x1b[6~".to_vec())
    );
    assert_eq!(key_bytes(Key::F1, none, false), Some(b"\x1bOP".to_vec()));
    assert_eq!(key_bytes(Key::F12, none, false), Some(b"\x1b[24~".to_vec()));
    // Plain letters come in as text events, not keys.
    assert_eq!(key_bytes(Key::A, none, false), None);
}

#[test]
fn a_paste_is_bracketed_when_asked_and_cannot_close_the_bracket() {
    assert_eq!(paste_bytes("ls\nwc", false), b"ls\rwc".to_vec());
    assert_eq!(
        paste_bytes("a\r\nb", true),
        b"\x1b[200~a\rb\x1b[201~".to_vec()
    );
    assert_eq!(
        paste_bytes("x\x1b[201~rm", true),
        b"\x1b[200~xrm\x1b[201~".to_vec()
    );
}

#[test]
fn colors_come_from_the_xterm_palette_or_the_theme() {
    let colors = Colors::default();
    assert_eq!(xterm_rgb(16), Rgb { r: 0, g: 0, b: 0 });
    assert_eq!(xterm_rgb(196), Rgb { r: 255, g: 0, b: 0 });
    assert_eq!(xterm_rgb(232), Rgb { r: 8, g: 8, b: 8 });
    assert_eq!(
        xterm_rgb(255),
        Rgb {
            r: 238,
            g: 238,
            b: 238
        }
    );
    assert_eq!(resolve(Color::Named(NamedColor::Foreground), &colors), None);
    assert_eq!(resolve(Color::Named(NamedColor::Background), &colors), None);
    assert_eq!(
        resolve(Color::Named(NamedColor::Red), &colors),
        Some(Color32::from_rgb(0xcc, 0x66, 0x66))
    );
    assert_eq!(
        resolve(Color::Indexed(21), &colors),
        Some(Color32::from_rgb(0, 0, 255))
    );
    assert_eq!(
        resolve(Color::Spec(Rgb { r: 1, g: 2, b: 3 }), &colors),
        Some(Color32::from_rgb(1, 2, 3))
    );
    let mut s = screen(1, 10);
    s.feed("\x1b]4;1;rgb:00/11/22\x07");
    assert_eq!(
        resolve(Color::Named(NamedColor::Red), s.term.colors()),
        Some(Color32::from_rgb(0, 0x11, 0x22))
    );
}

#[test]
fn the_notice_says_a_typed_push_skips_the_guard() {
    assert!(NOTICE.contains("git push"));
    assert!(NOTICE.contains("skip the push guard"));
    assert!(NOTICE.contains("Secret values are hidden"));
}

/// Pump the panel until `done` holds for its screen or 20 seconds pass.
#[cfg(unix)]
fn wait_for(panel: &mut TermPanel, done: impl Fn(&TermPanel) -> bool) -> bool {
    let end = Instant::now() + Duration::from_secs(20);
    while Instant::now() < end {
        panel.pump();
        if done(panel) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(30));
    }
    false
}

#[cfg(unix)]
fn shown(panel: &TermPanel) -> String {
    panel
        .live
        .as_ref()
        .map(|l| l.screen.lines().join("\n"))
        .unwrap_or_default()
}

/// The one test that uses the window's real terminal (there is only one).
#[cfg(unix)]
#[test]
fn a_shell_opens_in_the_project_hides_a_token_resizes_and_ends_on_close() {
    let base = std::env::temp_dir().join(format!("gh-cabin-term-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("proj");
    let config = base.join("config");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&config).unwrap();
    let root = root.canonicalize().unwrap();

    let mut panel = TermPanel::default();
    panel.open(
        &root,
        &config,
        GridSize {
            rows: 10,
            cols: 120,
        },
        || {},
    );
    assert!(panel.is_open());
    assert!(wait_for(&mut panel, |p| p
        .live
        .as_ref()
        .is_some_and(|l| l.id.is_some())));
    let id = panel.live.as_ref().and_then(|l| l.id).unwrap();

    panel.send(b"pwd\r");
    let want = root.to_string_lossy().into_owned();
    assert!(
        wait_for(&mut panel, |p| shown(p).contains(want.as_str())),
        "pwd output missing: {}",
        shown(&panel)
    );

    panel.send(b"echo ghp_abcdefghijklmnop1234 end\r");
    assert!(
        wait_for(&mut panel, |p| shown(p).contains("[redacted] end")),
        "no redaction: {}",
        shown(&panel)
    );
    assert!(!shown(&panel).contains("ghp_abcdefghijklmnop1234 end"));

    backend::resize(backend::global(), id, 12, 100).unwrap();
    panel.send(b"clear; stty size\r");
    assert!(
        wait_for(&mut panel, |p| shown(p).contains("12 100")),
        "stty did not see the resize: {}",
        shown(&panel)
    );

    panel.close(&config);
    assert!(!panel.is_open());
    // Closing runs off the UI thread; the shell is gone soon after.
    let end = Instant::now() + Duration::from_secs(10);
    let mut after = backend::write(backend::global(), id, "true\n");
    while after.is_ok() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(20));
        after = backend::write(backend::global(), id, "true\n");
    }
    assert_eq!(after.unwrap_err(), "The terminal has ended.");
    // Open and close are logged, never what was typed.
    let end = Instant::now() + Duration::from_secs(10);
    let mut rows = Vec::new();
    while Instant::now() < end {
        rows = grokhub_code::activity::list(&config, 10);
        if rows.len() == 2 {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let said: Vec<&str> = rows.iter().map(|r| r.summary.as_str()).collect();
    assert!(said.contains(&"Opened a terminal in the project folder."));
    assert!(said.contains(&"Closed the terminal."));
    assert!(!said
        .iter()
        .any(|s| s.contains("ghp_") || s.contains("echo")));
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn a_missing_folder_shows_the_reason_and_no_screen() {
    let mut panel = TermPanel::default();
    let gone = std::env::temp_dir().join("gh-cabin-term-no-such-dir");
    let config = std::env::temp_dir().join(format!("gh-cabin-term-cfg-{}", std::process::id()));
    std::fs::create_dir_all(&config).unwrap();
    panel.open(&gone, &config, GridSize { rows: 5, cols: 20 }, || {});
    let end = Instant::now() + Duration::from_secs(10);
    while panel.live.is_some() && Instant::now() < end {
        panel.pump();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(panel.live.is_none());
    assert_eq!(
        panel.status.as_deref(),
        Some("The project folder isn't there.")
    );
}
