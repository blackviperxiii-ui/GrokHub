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

/// Uses the window's real terminal (there is only one).
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

/// Uses the window's real terminal, like the test above; each test has its
/// own process, or they run one at a time.
#[cfg(unix)]
#[test]
fn a_busy_note_goes_away_once_the_shell_takes_input_again() {
    let base = std::env::temp_dir().join(format!("gh-cabin-term-busy-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("proj");
    let config = base.join("config");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&config).unwrap();
    let root = root.canonicalize().unwrap();

    let mut panel = TermPanel::default();
    panel.open(&root, &config, GridSize { rows: 10, cols: 80 }, || {});
    assert!(wait_for(&mut panel, |p| p
        .live
        .as_ref()
        .is_some_and(|l| l.id.is_some())));

    // Two seconds in which nothing reads what is typed.
    panel.send(b"sleep 2\r");
    std::thread::sleep(Duration::from_millis(300));
    // The most one paste may hold (1,024 lines that do nothing), then keys
    // until one is refused.
    let paste = format!(":{}\r", " ".repeat(62)).repeat(1024);
    assert_eq!(paste.len(), 65_536);
    panel.send(paste.as_bytes());
    assert_eq!(panel.status, None);
    for _ in 0..200 {
        panel.send(b":\r");
        if panel.status.is_some() {
            break;
        }
    }
    assert_eq!(
        panel.status.as_deref(),
        Some("The terminal is busy. Try again when the program is reading input.")
    );

    // The shell reads again once the sleep is over: the next key it takes
    // clears the note.
    let end = Instant::now() + Duration::from_secs(30);
    let mut taken = false;
    while !taken && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(100));
        panel.pump();
        let id = panel.live.as_ref().and_then(|l| l.id).unwrap();
        taken = backend::write(backend::global(), id, ":\r").is_ok();
    }
    assert!(taken, "the shell never took input again");
    let end = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end {
        panel.send(b":\r");
        if panel.status.is_none() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(panel.status, None);

    let id = panel.live.as_ref().and_then(|l| l.id).unwrap();
    panel.close(&config);
    // Closing runs off the UI thread; the next test's terminal must not be
    // the one it closes.
    let end = Instant::now() + Duration::from_secs(10);
    let mut after = backend::write(backend::global(), id, ":\r");
    while after.as_ref().err().map(String::as_str) != Some("The terminal has ended.")
        && Instant::now() < end
    {
        std::thread::sleep(Duration::from_millis(20));
        after = backend::write(backend::global(), id, ":\r");
    }
    assert_eq!(after.unwrap_err(), "The terminal has ended.");
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn the_answers_to_one_chunk_go_to_the_shell_as_one_write() {
    let mut live = Live {
        id: Some(7),
        screen: screen(5, 20),
        early: Vec::new(),
        exited: false,
    };
    live.screen.feed("\x1b[3;7H");
    let mut writes: Vec<(u32, String)> = Vec::new();
    // A chunk with 100 cursor-position queries in it.
    let chunk = Chunk {
        id: 7,
        text: "\x1b[6n".repeat(100),
        exited: false,
    };
    apply(&mut live, chunk, &mut |id, reply| {
        writes.push((id, reply.to_string()))
    });
    assert_eq!(writes, vec![(7, "\x1b[3;7R".repeat(100))]);

    // A chunk with no query in it writes nothing, and another session's
    // chunk is not answered.
    writes.clear();
    let plain = Chunk {
        id: 7,
        text: "hello".into(),
        exited: false,
    };
    apply(&mut live, plain, &mut |id, reply| {
        writes.push((id, reply.to_string()))
    });
    let other = Chunk {
        id: 8,
        text: "\x1b[6n".into(),
        exited: true,
    };
    apply(&mut live, other, &mut |id, reply| {
        writes.push((id, reply.to_string()))
    });
    assert_eq!(writes, Vec::new());
    assert!(!live.exited);
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

/// Every glyph the panel painted in one frame, with its color.
fn painted_glyphs(panel: &mut TermPanel) -> Vec<(String, Color32)> {
    let ctx = egui::Context::default();
    crate::theme::install_fonts_on(&ctx);
    let root = std::env::temp_dir();
    let config = std::env::temp_dir().join(format!("gh-cabin-term-paint-{}", std::process::id()));
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
        panel_ui(panel, ui, &root, &config);
    });
    out.textures_delta.clear();
    let mut glyphs = Vec::new();
    for clipped in out.shapes {
        if let egui::Shape::Text(t) = clipped.shape {
            glyphs.push((t.galley.text().to_string(), t.fallback_color));
        }
    }
    glyphs
}

fn live_panel(text: &str) -> TermPanel {
    let mut screen = screen(4, 30);
    screen.feed(text);
    let mut panel = TermPanel::default();
    panel.live = Some(Live {
        id: None,
        screen,
        early: Vec::new(),
        exited: false,
    });
    panel.root = Some(std::env::temp_dir());
    panel
}

#[test]
fn a_frame_paints_the_notice_and_each_cell_in_its_color() {
    let mut panel = live_panel("ok \x1b[31mred\x1b[0m");
    let glyphs = painted_glyphs(&mut panel);
    let texts: Vec<&str> = glyphs.iter().map(|(t, _)| t.as_str()).collect();
    assert!(texts.contains(&TITLE), "{texts:?}");
    assert!(texts.iter().any(|t| t.contains("git push")), "{texts:?}");
    assert!(texts.contains(&"Close terminal"), "{texts:?}");
    let red = Color32::from_rgb(0xcc, 0x66, 0x66);
    let reds: String = glyphs
        .iter()
        .filter(|(_, c)| *c == red)
        .map(|(t, _)| t.as_str())
        .collect();
    assert_eq!(reds, "red");
    assert!(glyphs
        .iter()
        .any(|(t, c)| t == "o" && *c == crate::theme::fg()));
    assert!(panel.painted);
}

#[test]
fn a_panel_not_drawn_has_no_screen_and_an_ended_shell_says_so() {
    let mut idle = TermPanel::default();
    let texts: Vec<String> = painted_glyphs(&mut idle).into_iter().map(|g| g.0).collect();
    assert!(texts.contains(&"Open terminal".to_string()), "{texts:?}");
    assert!(!texts.contains(&"o".to_string()));

    let mut ended = live_panel("bye");
    if let Some(l) = ended.live.as_mut() {
        l.exited = true;
    }
    let texts: Vec<String> = painted_glyphs(&mut ended)
        .into_iter()
        .map(|g| g.0)
        .collect();
    assert!(texts.contains(&ENDED.to_string()), "{texts:?}");
    assert!(texts.contains(&"Open terminal".to_string()), "{texts:?}");
}

// ---- the keyboard, the hidden window, and the ways a shell ends ----

/// A temp project folder and config folder for one real-shell test.
#[cfg(unix)]
fn shell_dirs(tag: &str) -> (PathBuf, PathBuf, PathBuf) {
    let base = std::env::temp_dir().join(format!("gh-cabin-term-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let root = base.join("proj");
    let config = base.join("config");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&config).unwrap();
    (base, root.canonicalize().unwrap(), config)
}

/// Open the window's real terminal in `root` and wait for the backend's id.
#[cfg(unix)]
fn open_shell(panel: &mut TermPanel, root: &Path, config: &Path) -> u32 {
    panel.open(root, config, GridSize { rows: 10, cols: 80 }, || {});
    assert!(wait_for(panel, |p| p
        .live
        .as_ref()
        .is_some_and(|l| l.id.is_some())));
    panel.live.as_ref().and_then(|l| l.id).unwrap()
}

/// Wait until a write to the shell `id` is refused, and return the refusal.
#[cfg(unix)]
fn refusal_after_end(id: u32) -> String {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        match backend::write(backend::global(), id, "") {
            Err(e) if e == "The terminal has ended." => return e,
            other if Instant::now() >= end => return format!("still open: {other:?}"),
            _ => std::thread::sleep(Duration::from_millis(20)),
        }
    }
}

fn key(key: Key, modifiers: Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

/// Ctrl as Linux and Windows report it: egui's `command` is set with it.
fn ctrl() -> Modifiers {
    Modifiers {
        ctrl: true,
        command: true,
        ..Modifiers::NONE
    }
}

/// One cabin pass with Ctrl held and `k` pressed, through the same shortcut
/// code the window runs.
fn press_ctrl(cabin: &mut crate::app::Cabin, k: Key) {
    let ctx = egui::Context::default();
    let raw = egui::RawInput {
        events: vec![egui::Event::ModifiersChanged(ctrl()), key(k, ctrl())],
        ..Default::default()
    };
    let _ = crate::theme::test_pass(&ctx, raw, |ui| {
        let ctx = ui.ctx().clone();
        cabin.logic_input_actions(&ctx);
    });
}

#[test]
fn keys_typed_in_the_terminal_do_not_fire_the_cabin_shortcuts() {
    use crate::app::{Cabin, Nav};
    // The terminal has the keyboard: Ctrl+G, K, N, / and , are the shell's.
    let mut cabin = Cabin::quiet_for_test();
    cabin.nav = Nav::Code;
    cabin.code_term.keys_held = true;
    let status = cabin.status.clone();
    let chats = cabin.threads.len();
    press_ctrl(&mut cabin, Key::G);
    assert_eq!(cabin.status, status, "Ctrl+G reached the voice shortcut");
    press_ctrl(&mut cabin, Key::K);
    assert!(!cabin.palette_open, "Ctrl+K opened the palette");
    press_ctrl(&mut cabin, Key::N);
    assert_eq!(cabin.threads.len(), chats, "Ctrl+N started a new chat");
    press_ctrl(&mut cabin, Key::Slash);
    assert!(!cabin.shortcuts_open, "Ctrl+/ opened Shortcuts");
    press_ctrl(&mut cabin, Key::Comma);
    assert_eq!(cabin.nav, Nav::Code, "Ctrl+, left the Code tab");

    // The same keys still work once the terminal lets go of the keyboard.
    cabin.code_term.keys_held = false;
    press_ctrl(&mut cabin, Key::K);
    assert!(cabin.palette_open);
    press_ctrl(&mut cabin, Key::Slash);
    assert!(cabin.shortcuts_open);
    press_ctrl(&mut cabin, Key::Comma);
    assert_eq!(cabin.nav, Nav::Settings);
}

#[test]
fn a_closed_or_undrawn_terminal_gives_the_keyboard_back() {
    let config = std::env::temp_dir().join(format!("gh-cabin-term-keys-{}", std::process::id()));
    // Closing it.
    let mut panel = live_panel("x");
    panel.keys_held = true;
    panel.close(&config);
    assert!(!panel.keys_held);
    // A frame that did not draw it (the tab was left).
    let mut panel = TermPanel::default();
    panel.keys_held = true;
    end_frame(&mut panel, &config);
    assert!(!panel.keys_held);
    // A frame that drew it without focus.
    let mut panel = live_panel("x");
    panel.keys_held = true;
    painted_glyphs(&mut panel);
    assert!(!panel.keys_held);
}

/// One chunk of exactly 4,096 bytes that ends its line with `n`.
fn numbered_chunk(id: u32, n: usize) -> Msg {
    let text = format!("{}{n:07}\r\n", "x".repeat(4087));
    assert_eq!(text.len(), 4096);
    Msg::Out(Chunk {
        id,
        text,
        exited: false,
    })
}

fn last_line(panel: &TermPanel) -> String {
    let live = panel.live.as_ref().expect("a live screen");
    live.screen
        .lines()
        .into_iter()
        .rev()
        .find(|l| !l.is_empty())
        .unwrap_or_default()
}

#[test]
fn a_backlog_is_taken_a_budget_at_a_time_without_the_page_drawing() {
    // 1,000 chunks of 4 KB wait while nothing draws the panel.
    let mut panel = live_panel("");
    if let Some(live) = panel.live.as_mut() {
        live.id = Some(1);
    }
    for n in 0..1000 {
        panel.tx.send(numbered_chunk(1, n)).unwrap();
    }
    panel
        .tx
        .send(Msg::Out(Chunk {
            id: 1,
            text: "the-end".into(),
            exited: false,
        }))
        .unwrap();

    // One pump takes 1 MB (256 chunks) and says more is waiting.
    assert!(panel.pump());
    assert_eq!(last_line(&panel), "xxxxxxx0000255");
    // 1,000 chunks are four pumps; the last one finds the queue empty.
    let mut pumps = 1;
    while panel.pump() {
        pumps += 1;
        assert!(pumps < 100, "the queue never emptied");
    }
    assert_eq!(pumps + 1, 4);
    assert_eq!(last_line(&panel), "the-end");
    assert!(!panel.pump());
    assert!(!panel.painted, "nothing drew the panel");
}

#[test]
fn the_cabin_takes_terminal_output_on_every_logic_pass() {
    let mut cabin = crate::app::Cabin::quiet_for_test();
    cabin.code_term = live_panel("");
    if let Some(live) = cabin.code_term.live.as_mut() {
        live.id = Some(1);
    }
    cabin
        .code_term
        .tx
        .send(Msg::Out(Chunk {
            id: 1,
            text: "printed while hidden".into(),
            exited: false,
        }))
        .unwrap();
    // No frame is painted: this is what `logic` runs from the tray.
    cabin.code_terminal_tick(&egui::Context::default());
    assert_eq!(last_line(&cabin.code_term), "printed while hidden");

    // `logic` is the pass a hidden window still gets, and it makes the call.
    let src = include_str!("mod.rs").replace("\r\n", "\n");
    let logic = src
        .split("\n    fn logic(&mut self, ctx: &egui::Context")
        .nth(1)
        .and_then(|rest| rest.split("\n    fn ui(&mut self").next())
        .expect("the cabin's logic pass");
    assert!(logic.contains("self.code_terminal_tick(ctx);"));
}

#[cfg(unix)]
#[test]
fn closing_before_the_open_finishes_leaves_no_shell() {
    let (base, root, config) = shell_dirs("early-close");
    let mut panel = TermPanel::default();
    panel.open(&root, &config, GridSize { rows: 10, cols: 80 }, || {});
    // The second press lands while the shell is still starting.
    panel.close(&config);
    assert!(!panel.is_open());
    // The open thread reports when it is done, whatever it did.
    let end = Instant::now() + Duration::from_secs(20);
    let mut reported = false;
    while !reported && Instant::now() < end {
        match panel.rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Msg::Opened(..)) => reported = true,
            Ok(Msg::Out(_)) | Err(_) => {}
        }
    }
    assert!(reported, "the open never reported");
    // Nothing is left for a close to end.
    assert!(
        !backend::close(backend::global()),
        "a shell was left running with no panel"
    );
    let _ = std::fs::remove_dir_all(&base);
}

#[cfg(unix)]
#[test]
fn a_shell_closed_while_it_started_is_ended_by_its_own_open() {
    let (base, root, _config) = shell_dirs("mid-close");
    let wanted = AtomicU64::new(1);
    // The close comes after the open's first look and before its second.
    let got = open_if_wanted(&wanted, 1, || {
        let id = backend::open(backend::global(), &root, Vec::new(), 10, 80, |_| {});
        wanted.store(2, Ordering::SeqCst);
        id
    });
    assert_eq!(got, None);
    assert!(!backend::close(backend::global()));
    // An open nobody wants any more is not made at all.
    let mut made = false;
    let got = open_if_wanted(&wanted, 1, || {
        made = true;
        Ok(7)
    });
    assert_eq!(got, None);
    assert!(!made);
    // The newest one goes through.
    assert_eq!(open_if_wanted(&wanted, 2, || Ok(7)), Some(Ok(7)));
    let _ = std::fs::remove_dir_all(&base);
}

#[test]
fn a_report_from_an_open_that_was_replaced_is_dropped() {
    let mut panel = live_panel("");
    panel.wanted.store(2, Ordering::SeqCst);
    // The first open's shell is gone; its id must not become the screen's.
    panel.tx.send(Msg::Opened(1, Ok(41))).unwrap();
    panel.pump();
    assert_eq!(panel.live.as_ref().and_then(|l| l.id), None);
    panel.tx.send(Msg::Opened(1, Err("late".into()))).unwrap();
    panel.pump();
    assert!(panel.live.is_some());
    assert_eq!(panel.status, None);
    panel.tx.send(Msg::Opened(2, Ok(42))).unwrap();
    panel.pump();
    assert_eq!(panel.live.as_ref().and_then(|l| l.id), Some(42));
}

#[cfg(unix)]
#[test]
fn leaving_the_tab_ends_the_shell_and_a_drawn_frame_keeps_it() {
    let (base, root, config) = shell_dirs("leave");
    let mut panel = TermPanel::default();
    let id = open_shell(&mut panel, &root, &config);

    // A frame that drew the terminal: the shell stays.
    panel.painted = true;
    end_frame(&mut panel, &config);
    assert!(panel.is_open());
    assert_eq!(backend::write(backend::global(), id, ""), Ok(()));

    // A frame that did not (another tab is up): the shell ends.
    end_frame(&mut panel, &config);
    assert!(!panel.is_open());
    assert_eq!(refusal_after_end(id), "The terminal has ended.");
    let _ = std::fs::remove_dir_all(&base);
}

#[cfg(unix)]
#[test]
fn dropping_the_panel_ends_its_shell() {
    let (base, root, config) = shell_dirs("drop");
    let mut panel = TermPanel::default();
    let id = open_shell(&mut panel, &root, &config);
    // The cabin quits: its terminal panel goes.
    drop(panel);
    assert_eq!(refusal_after_end(id), "The terminal has ended.");
    let _ = std::fs::remove_dir_all(&base);
}

/// One painted frame of the panel in a fixed 800 by 1200 window.
#[cfg(unix)]
fn frame(
    ctx: &egui::Context,
    panel: &mut TermPanel,
    root: &Path,
    config: &Path,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let raw = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 1200.0))),
        events,
        ..Default::default()
    };
    crate::theme::test_pass(ctx, raw, |ui| panel_ui(panel, ui, root, config))
}

#[cfg(unix)]
#[test]
fn keys_text_paste_copy_and_cut_reach_the_shell_as_a_terminal_sends_them() {
    let (base, root, config) = shell_dirs("events");
    let mut panel = TermPanel::default();
    let id = open_shell(&mut panel, &root, &config);
    // `cat -v` prints control bytes as ^X; with echo off only cat answers.
    // The last echo runs only if cat ends by itself (an end of input), not
    // when it is interrupted.
    panel.send(b"stty -echo; echo ready-$((40+2)); cat -v; echo eof-$((70+7))\r");
    assert!(
        wait_for(&mut panel, |p| shown(p).contains("ready-42")),
        "{}",
        shown(&panel)
    );

    // Click the screen: it takes the keyboard.
    let ctx = egui::Context::default();
    crate::theme::install_fonts_on(&ctx);
    let first = frame(&ctx, &mut panel, &root, &config, Vec::new());
    assert!(!panel.keys_held, "nothing has focus yet");
    let well = crate::theme::code_well();
    let at = first
        .shapes
        .iter()
        .find_map(|c| match &c.shape {
            egui::Shape::Rect(r) if r.fill == well => Some(r.rect.center()),
            _ => None,
        })
        .expect("the terminal screen was painted");
    let button = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    frame(
        &ctx,
        &mut panel,
        &root,
        &config,
        vec![egui::Event::PointerMoved(at), button(true)],
    );
    frame(&ctx, &mut panel, &root, &config, vec![button(false)]);
    frame(&ctx, &mut panel, &root, &config, Vec::new());
    assert!(panel.keys_held, "the clicked screen holds the keyboard");

    // Text, cut (Ctrl+X), a two-line paste and Ctrl+G in one frame.
    frame(
        &ctx,
        &mut panel,
        &root,
        &config,
        vec![
            egui::Event::Text("a".into()),
            egui::Event::Cut,
            egui::Event::Paste("p\nq".into()),
            egui::Event::ModifiersChanged(ctrl()),
            key(Key::G, ctrl()),
            egui::Event::ModifiersChanged(Modifiers::NONE),
        ],
    );
    let has_line = |p: &TermPanel, want: &str| shown(p).lines().any(|l| l == want);
    assert!(
        wait_for(&mut panel, |p| has_line(p, "a^Xp")),
        "{}",
        shown(&panel)
    );
    // Enter ends the second line: the q of the paste, then the Ctrl+G byte.
    frame(
        &ctx,
        &mut panel,
        &root,
        &config,
        vec![key(Key::Enter, Modifiers::NONE)],
    );
    assert!(
        wait_for(&mut panel, |p| has_line(p, "q^G")),
        "{}",
        shown(&panel)
    );

    // Copy is Ctrl+C here: it interrupts cat, and the shell takes a command.
    frame(&ctx, &mut panel, &root, &config, vec![egui::Event::Copy]);
    frame(
        &ctx,
        &mut panel,
        &root,
        &config,
        vec![
            egui::Event::Text("echo back-$((50+5))".into()),
            key(Key::Enter, Modifiers::NONE),
        ],
    );
    assert!(
        wait_for(&mut panel, |p| shown(p).contains("back-55")),
        "{}",
        shown(&panel)
    );
    assert!(
        !shown(&panel).contains("eof-77"),
        "cat ended without an interrupt: {}",
        shown(&panel)
    );

    panel.close(&config);
    assert_eq!(refusal_after_end(id), "The terminal has ended.");
    let _ = std::fs::remove_dir_all(&base);
}

#[cfg(unix)]
#[test]
fn open_pressed_again_at_once_keeps_the_new_shell_and_logs_the_old_one_closed() {
    let (base, root, config) = shell_dirs("reopen");
    let mut panel = TermPanel::default();
    let first = open_shell(&mut panel, &root, &config);
    // What the button does once the shell has ended: close, then open.
    panel.close(&config);
    let second = open_shell(&mut panel, &root, &config);
    assert_ne!(first, second);
    // The close that was already on its way did not end the new shell.
    std::thread::sleep(Duration::from_millis(300));
    panel.send(b"echo still-$((60+6))\r");
    assert!(
        wait_for(&mut panel, |p| shown(p).contains("still-66")),
        "{}",
        shown(&panel)
    );
    assert_eq!(
        backend::write(backend::global(), first, ""),
        Err("The terminal has ended.".to_string())
    );
    // Both openings and the close in between are in the activity list.
    let end = Instant::now() + Duration::from_secs(10);
    let mut said: Vec<String> = Vec::new();
    while said.len() < 3 && Instant::now() < end {
        said = grokhub_code::activity::list(&config, 10)
            .into_iter()
            .map(|r| r.summary)
            .collect();
        std::thread::sleep(Duration::from_millis(50));
    }
    said.sort();
    assert_eq!(
        said,
        vec![
            "Closed the terminal.",
            "Opened a terminal in the project folder.",
            "Opened a terminal in the project folder.",
        ]
    );
    panel.close(&config);
    assert_eq!(refusal_after_end(second), "The terminal has ended.");
    let _ = std::fs::remove_dir_all(&base);
}
