//! The Code tab terminal in the cabin: the user's own shell on a
//! pseudo-terminal, started in the project folder. The shell runs through
//! `grokhub_code::term` (the same backend as the desktop shell app), so its
//! output is redacted (the project's secret values, then the usual token
//! patterns) before it reaches this widget. `alacritty_terminal` keeps the
//! screen grid; this module draws it with the painter and forwards keys.
//!
//! This is a normal shell: a `git push` typed here is not checked by the Code
//! tab's push guard, and the header says so.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

use alacritty_terminal::event::{Event as TermEvent, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::Point;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::term::{Config, TermMode};
use alacritty_terminal::vte::ansi::{Color, NamedColor, Processor, Rgb};
use alacritty_terminal::Term;
use eframe::egui::{self, Color32, FontId, Key, Modifiers, Pos2, Rect, Sense, Vec2};
use grokhub_code::term::{self as backend, Chunk};

/// The header line under the terminal's title.
pub(super) const NOTICE: &str = "Your own shell, started in the project folder. Secret values are hidden in what it shows. Commands typed here, git push included, skip the push guard.";
/// Lines kept above the screen for scrolling back.
const SCROLLBACK: usize = 5_000;
/// The smallest and largest grid the backend accepts.
const MIN_ROWS: u16 = 2;
const MAX_ROWS: u16 = 500;
const MIN_COLS: u16 = 2;
const MAX_COLS: u16 = 1000;
const FONT_SIZE: f32 = 13.0;
/// The screen's height in points.
const SCREEN_HEIGHT: f32 = 360.0;
/// Space between the screen's edge and the text, like the tab's other wells.
const INSET: f32 = 10.0;
/// The section title and the line shown once the shell has ended.
pub(super) const TITLE: &str = "Terminal";
pub(super) const ENDED: &str = "The shell ended. Open the terminal again to start a new one.";

/// Collects what the parser wants written back to the shell (answers to
/// cursor-position and device queries).
#[derive(Clone, Default)]
pub(super) struct Replies(Arc<Mutex<Vec<String>>>);

impl Replies {
    fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.0.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

impl EventListener for Replies {
    fn send_event(&self, event: TermEvent) {
        if let TermEvent::PtyWrite(text) = event {
            self.0.lock().unwrap_or_else(|e| e.into_inner()).push(text);
        }
    }
}

/// A grid size the parser and the backend agree on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct GridSize {
    pub rows: u16,
    pub cols: u16,
}

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        usize::from(self.rows)
    }
    fn screen_lines(&self) -> usize {
        usize::from(self.rows)
    }
    fn columns(&self) -> usize {
        usize::from(self.cols)
    }
}

/// How many rows and columns of `cell` fit in `area`, clamped to what the
/// backend accepts.
pub(super) fn grid_for(area: Vec2, cell: Vec2) -> GridSize {
    let fit = |len: f32, step: f32, lo: u16, hi: u16| -> u16 {
        if !step.is_finite() || step <= 0.0 || !len.is_finite() {
            return lo;
        }
        let n = (len / step).floor();
        if n < f32::from(lo) {
            lo
        } else if n > f32::from(hi) {
            hi
        } else {
            n as u16
        }
    };
    GridSize {
        rows: fit(area.y, cell.y, MIN_ROWS, MAX_ROWS),
        cols: fit(area.x, cell.x, MIN_COLS, MAX_COLS),
    }
}

/// The screen: parser plus grid, fed with redacted output.
pub(super) struct Screen {
    term: Term<Replies>,
    parser: Processor,
    replies: Replies,
    size: GridSize,
}

impl Screen {
    pub(super) fn new(size: GridSize) -> Self {
        let replies = Replies::default();
        let config = Config {
            scrolling_history: SCROLLBACK,
            ..Config::default()
        };
        Self {
            term: Term::new(config, &size, replies.clone()),
            parser: Processor::new(),
            replies,
            size,
        }
    }

    /// Feed redacted shell output. Returns what has to be written back.
    pub(super) fn feed(&mut self, text: &str) -> Vec<String> {
        self.parser.advance(&mut self.term, text.as_bytes());
        self.replies.take()
    }

    /// True when the size changed.
    pub(super) fn resize(&mut self, size: GridSize) -> bool {
        if size == self.size {
            return false;
        }
        self.size = size;
        self.term.resize(size);
        true
    }

    #[cfg(test)]
    pub(super) fn size(&self) -> GridSize {
        self.size
    }

    pub(super) fn mode(&self) -> TermMode {
        *self.term.mode()
    }

    /// Scroll back (positive) or forward (negative) by lines.
    pub(super) fn scroll(&mut self, lines: i32) {
        self.term.scroll_display(Scroll::Delta(lines));
    }

    pub(super) fn scroll_to_bottom(&mut self) {
        self.term.scroll_display(Scroll::Bottom);
    }

    #[cfg(test)]
    pub(super) fn display_offset(&self) -> usize {
        self.term.grid().display_offset()
    }

    /// The visible screen as text, one string per row, trailing spaces cut.
    #[cfg(test)]
    pub(super) fn lines(&self) -> Vec<String> {
        let rows = usize::from(self.size.rows);
        let mut out = vec![String::new(); rows];
        let content = self.term.renderable_content();
        let offset = content.display_offset as i32;
        for cell in content.display_iter {
            if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                continue;
            }
            let row = cell.point.line.0 + offset;
            if let Some(line) = usize::try_from(row).ok().and_then(|r| out.get_mut(r)) {
                line.push(cell.c);
            }
        }
        for line in &mut out {
            line.truncate(line.trim_end().len());
        }
        out
    }

    /// Where the cursor is on screen (row, column), if it is shown.
    pub(super) fn cursor(&self) -> Option<(usize, usize)> {
        let content = self.term.renderable_content();
        if content.cursor.shape == alacritty_terminal::vte::ansi::CursorShape::Hidden {
            return None;
        }
        let Point { line, column } = content.cursor.point;
        let row = line.0 + content.display_offset as i32;
        let row = usize::try_from(row).ok()?;
        (row < usize::from(self.size.rows)).then_some((row, column.0))
    }
}

/// The xterm palette entry `i` (0..=255).
pub(super) fn xterm_rgb(i: u8) -> Rgb {
    const BASE: [(u8, u8, u8); 16] = [
        (0x1d, 0x1f, 0x21),
        (0xcc, 0x66, 0x66),
        (0xb5, 0xbd, 0x68),
        (0xf0, 0xc6, 0x74),
        (0x81, 0xa2, 0xbe),
        (0xb2, 0x94, 0xbb),
        (0x8a, 0xbe, 0xb7),
        (0xc5, 0xc8, 0xc6),
        (0x66, 0x66, 0x66),
        (0xd5, 0x4e, 0x53),
        (0xb9, 0xca, 0x4a),
        (0xe7, 0xc5, 0x47),
        (0x7a, 0xa6, 0xda),
        (0xc3, 0x97, 0xd8),
        (0x70, 0xc0, 0xb1),
        (0xea, 0xea, 0xea),
    ];
    match i {
        0..=15 => {
            let (r, g, b) = BASE[usize::from(i)];
            Rgb { r, g, b }
        }
        16..=231 => {
            let n = i - 16;
            let step = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            Rgb {
                r: step(n / 36),
                g: step((n / 6) % 6),
                b: step(n % 6),
            }
        }
        _ => {
            let v = 8 + (i - 232) * 10;
            Rgb { r: v, g: v, b: v }
        }
    }
}

fn to32(c: Rgb) -> Color32 {
    Color32::from_rgb(c.r, c.g, c.b)
}

/// The color a cell's `color` draws in. `None` means the panel's own
/// foreground or background (the theme's), which this widget doesn't paint.
pub(super) fn resolve(color: Color, colors: &Colors) -> Option<Color32> {
    match color {
        Color::Spec(rgb) => Some(to32(rgb)),
        Color::Indexed(i) => Some(to32(colors[usize::from(i)].unwrap_or_else(|| xterm_rgb(i)))),
        Color::Named(named) => {
            if let Some(rgb) = colors[named] {
                return Some(to32(rgb));
            }
            let n = named as usize;
            match named {
                NamedColor::Foreground
                | NamedColor::Background
                | NamedColor::Cursor
                | NamedColor::BrightForeground
                | NamedColor::DimForeground => None,
                _ if n < 16 => Some(to32(xterm_rgb(n as u8))),
                // Dim black .. dim white follow the normal colors, faded.
                _ => {
                    let base = n.saturating_sub(NamedColor::DimBlack as usize).min(7);
                    Some(to32(xterm_rgb(base as u8)).gamma_multiply(0.66))
                }
            }
        }
    }
}

/// What one key press sends to the shell, or `None` for keys it ignores.
/// `app_cursor` is the shell's application cursor mode (vim, less, top).
pub(super) fn key_bytes(key: Key, mods: Modifiers, app_cursor: bool) -> Option<Vec<u8>> {
    let modifier = 1 + u8::from(mods.shift) + 2 * u8::from(mods.alt) + 4 * u8::from(mods.ctrl);
    let csi = |end: char| -> Vec<u8> {
        if modifier > 1 {
            format!("\x1b[1;{modifier}{end}").into_bytes()
        } else if app_cursor {
            format!("\x1bO{end}").into_bytes()
        } else {
            format!("\x1b[{end}").into_bytes()
        }
    };
    let tilde = |n: u8| -> Vec<u8> {
        if modifier > 1 {
            format!("\x1b[{n};{modifier}~").into_bytes()
        } else {
            format!("\x1b[{n}~").into_bytes()
        }
    };
    let ss3 = |end: char| -> Vec<u8> {
        if modifier > 1 {
            format!("\x1b[1;{modifier}{end}").into_bytes()
        } else {
            format!("\x1bO{end}").into_bytes()
        }
    };
    if mods.ctrl && !mods.alt {
        let name = key.name();
        let mut chars = name.chars();
        if let (Some(c), None) = (chars.next(), chars.next())
            && c.is_ascii_alphabetic()
        {
            return Some(vec![(c.to_ascii_uppercase() as u8) - b'A' + 1]);
        }
        match key {
            Key::Space | Key::Num2 => return Some(vec![0]),
            Key::OpenBracket => return Some(vec![0x1b]),
            Key::Backslash => return Some(vec![0x1c]),
            Key::CloseBracket => return Some(vec![0x1d]),
            _ => {}
        }
    }
    let out = match key {
        Key::Enter => b"\r".to_vec(),
        Key::Backspace => {
            if mods.ctrl {
                vec![0x08]
            } else {
                vec![0x7f]
            }
        }
        Key::Tab if mods.shift => b"\x1b[Z".to_vec(),
        Key::Tab => b"\t".to_vec(),
        Key::Escape => b"\x1b".to_vec(),
        Key::ArrowUp => csi('A'),
        Key::ArrowDown => csi('B'),
        Key::ArrowRight => csi('C'),
        Key::ArrowLeft => csi('D'),
        Key::Home => csi('H'),
        Key::End => csi('F'),
        Key::Insert => tilde(2),
        Key::Delete => tilde(3),
        Key::PageUp => tilde(5),
        Key::PageDown => tilde(6),
        Key::F1 => ss3('P'),
        Key::F2 => ss3('Q'),
        Key::F3 => ss3('R'),
        Key::F4 => ss3('S'),
        Key::F5 => tilde(15),
        Key::F6 => tilde(17),
        Key::F7 => tilde(18),
        Key::F8 => tilde(19),
        Key::F9 => tilde(20),
        Key::F10 => tilde(21),
        Key::F11 => tilde(23),
        Key::F12 => tilde(24),
        _ => return None,
    };
    Some(out)
}

/// Pasted text, wrapped for bracketed paste when the shell asked for it.
/// Line ends become carriage returns like a typed Enter.
pub(super) fn paste_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    let body = text.replace("\r\n", "\r").replace('\n', "\r");
    if bracketed {
        // A paste must not be able to end the bracket early.
        let body = body.replace("\x1b[201~", "");
        format!("\x1b[200~{body}\x1b[201~").into_bytes()
    } else {
        body.into_bytes()
    }
}

/// How many activity lines to keep, from the desktop app's Code settings.
fn keep_of(config_dir: &Path) -> usize {
    grokhub_code::ship::load_from_shell_json(config_dir).activity_keep
}

/// What the shell thread or the open thread reports back.
enum Msg {
    Opened(Result<u32, String>),
    Out(Chunk),
}

/// One terminal session as the widget sees it.
struct Live {
    /// The backend's id; `None` until the open finishes.
    id: Option<u32>,
    screen: Screen,
    /// Chunks that came in before the id was known.
    early: Vec<Chunk>,
    exited: bool,
}

/// The terminal widget's state between frames.
pub(super) struct TermPanel {
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    live: Option<Live>,
    /// The folder the shell was opened in, for the activity log.
    root: Option<PathBuf>,
    /// The last error or note to show under the header.
    status: Option<String>,
    /// Drawn this frame; a terminal not drawn for a frame is closed.
    painted: bool,
}

impl Default for TermPanel {
    fn default() -> Self {
        let (tx, rx) = channel();
        Self {
            tx,
            rx,
            live: None,
            root: None,
            status: None,
            painted: false,
        }
    }
}

impl Drop for TermPanel {
    fn drop(&mut self) {
        if self.live.is_some() {
            backend::close(backend::global());
        }
    }
}

impl TermPanel {
    /// True while a shell is open (or opening) and hasn't ended.
    pub(super) fn is_open(&self) -> bool {
        self.live.as_ref().is_some_and(|l| !l.exited)
    }

    /// Open the user's shell in `root`. Keychain reads and the spawn run off
    /// the UI thread; `repaint` wakes the window when output arrives.
    pub(super) fn open(
        &mut self,
        root: &Path,
        config_dir: &Path,
        size: GridSize,
        repaint: impl Fn() + Send + Sync + 'static,
    ) {
        self.status = None;
        self.live = Some(Live {
            id: None,
            screen: Screen::new(size),
            early: Vec::new(),
            exited: false,
        });
        self.root = Some(root.to_path_buf());
        let tx = self.tx.clone();
        let root = root.to_path_buf();
        let config = config_dir.to_path_buf();
        let repaint = Arc::new(repaint);
        std::thread::spawn(move || {
            let held = grokhub_code::secrets::held_for(&config, &root);
            let out = tx.clone();
            let wake = Arc::clone(&repaint);
            let opened = backend::open(
                backend::global(),
                &root,
                held,
                size.rows,
                size.cols,
                move |chunk| {
                    let _ = out.send(Msg::Out(chunk));
                    wake();
                },
            );
            if opened.is_ok() {
                backend::log(
                    &config,
                    &root,
                    "Opened a terminal in the project folder.",
                    keep_of(&config),
                );
            }
            let _ = tx.send(Msg::Opened(opened));
            repaint();
        });
    }

    /// End the shell. Logged when one was open.
    pub(super) fn close(&mut self, config_dir: &Path) {
        if self.live.take().is_none() {
            return;
        }
        let root = self.root.take();
        let config = config_dir.to_path_buf();
        std::thread::spawn(move || {
            if backend::close(backend::global())
                && let Some(root) = root
            {
                backend::log(&config, &root, "Closed the terminal.", keep_of(&config));
            }
        });
    }

    /// Apply what the backend sent since the last frame.
    fn pump(&mut self) {
        while let Ok(msg) = self.rx.try_recv() {
            let Some(live) = self.live.as_mut() else {
                continue;
            };
            match msg {
                Msg::Opened(Ok(id)) => {
                    live.id = Some(id);
                    for chunk in std::mem::take(&mut live.early) {
                        apply(live, chunk);
                    }
                }
                Msg::Opened(Err(e)) => {
                    self.live = None;
                    self.status = Some(e);
                }
                Msg::Out(chunk) => match live.id {
                    None => live.early.push(chunk),
                    Some(_) => apply(live, chunk),
                },
            }
        }
    }

    fn send(&mut self, bytes: &[u8]) {
        let Some(live) = self.live.as_mut() else {
            return;
        };
        let (Some(id), false) = (live.id, live.exited) else {
            return;
        };
        let text = String::from_utf8_lossy(bytes);
        if let Err(e) = backend::write(backend::global(), id, &text) {
            self.status = Some(e);
        }
    }
}

/// Feed one chunk into the screen if it belongs to this session.
fn apply(live: &mut Live, chunk: Chunk) {
    if Some(chunk.id) != live.id {
        return;
    }
    if !chunk.text.is_empty() {
        for reply in live.screen.feed(&chunk.text) {
            if let Some(id) = live.id {
                let _ = backend::write(backend::global(), id, &reply);
            }
        }
    }
    if chunk.exited {
        live.exited = true;
    }
}

fn cell_size(ui: &egui::Ui, font: &FontId) -> Vec2 {
    ui.fonts_mut(|f| Vec2::new(f.glyph_width(font, 'M'), f.row_height(font)))
}

/// Draw the terminal panel: header with the push-guard notice, Open / Close,
/// and the screen. `root` is the project folder.
pub(super) fn panel_ui(panel: &mut TermPanel, ui: &mut egui::Ui, root: &Path, config_dir: &Path) {
    panel.painted = true;
    panel.pump();
    // A shell left in another project's folder is closed.
    if panel.live.is_some() && panel.root.as_deref().is_some_and(|r| r != root) {
        panel.close(config_dir);
    }
    crate::cards::section_label(ui, TITLE);
    ui.label(
        egui::RichText::new(NOTICE)
            .size(crate::theme::TYPE_CAPTION)
            .color(crate::theme::muted()),
    );
    ui.add_space(crate::theme::SPACING_8);
    let open = panel.is_open();
    let label = if open {
        "Close terminal"
    } else {
        "Open terminal"
    };
    let pressed = if open {
        crate::cards::ghost_pill(ui, label)
    } else {
        crate::cards::white_pill(ui, label)
    };
    if pressed {
        panel.close(config_dir);
        if !open {
            let font = FontId::monospace(FONT_SIZE);
            let cell = cell_size(ui, &font);
            let area = Vec2::new(ui.available_width(), SCREEN_HEIGHT) - Vec2::splat(2.0 * INSET);
            let ctx = ui.ctx().clone();
            panel.open(root, config_dir, grid_for(area, cell), move || {
                ctx.request_repaint()
            });
        }
    }
    if let Some(s) = &panel.status {
        ui.colored_label(crate::theme::muted(), s);
    }
    if panel.live.as_ref().is_some_and(|l| l.exited) {
        ui.colored_label(crate::theme::muted(), ENDED);
    }
    if panel.live.is_some() {
        ui.add_space(crate::theme::SPACING_8);
        screen_ui(panel, ui);
    }
}

impl super::Cabin {
    /// The Code tab's terminal section.
    pub(super) fn ui_code_terminal(&mut self, ui: &mut egui::Ui) {
        let root = self.grok_cwd();
        panel_ui(&mut self.code_term, ui, &root, &crate::config::config_dir());
    }

    /// Call once per frame after the page is drawn: a terminal the Code tab
    /// didn't draw this frame (the tab was left) is closed.
    pub(super) fn code_terminal_end_frame(&mut self) {
        let panel = &mut self.code_term;
        if !std::mem::take(&mut panel.painted) && panel.live.is_some() {
            panel.close(&crate::config::config_dir());
        }
    }
}

fn screen_ui(panel: &mut TermPanel, ui: &mut egui::Ui) {
    let font = FontId::monospace(FONT_SIZE);
    let cell = cell_size(ui, &font);
    let height = SCREEN_HEIGHT.max(cell.y * 8.0);
    let (outer, resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::click());
    let rect = outer.shrink(INSET);
    if resp.clicked() {
        resp.request_focus();
    }
    let focused = resp.has_focus();
    if focused {
        ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                resp.id,
                egui::EventFilter {
                    tab: true,
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    escape: true,
                },
            )
        });
    }

    // Follow the panel's size.
    let size = grid_for(rect.size(), cell);
    let resize_id = panel.live.as_mut().and_then(|live| {
        live.screen
            .resize(size)
            .then_some(live.id)
            .flatten()
            .filter(|_| !live.exited)
    });
    if let Some(id) = resize_id
        && let Err(e) = backend::resize(backend::global(), id, size.rows, size.cols)
    {
        panel.status = Some(e);
    }

    // Keys, text and paste go to the shell while it has focus.
    if focused {
        let mode = panel
            .live
            .as_ref()
            .map(|l| l.screen.mode())
            .unwrap_or(TermMode::NONE);
        let events = ui.input(|i| i.events.clone());
        let mut out: Vec<u8> = Vec::new();
        for event in events {
            match event {
                egui::Event::Text(t) => out.extend_from_slice(t.as_bytes()),
                egui::Event::Paste(t) => {
                    out.extend(paste_bytes(&t, mode.contains(TermMode::BRACKETED_PASTE)))
                }
                // Ctrl+C and Ctrl+X reach egui as copy and cut; in a shell
                // they are interrupt and ^X.
                egui::Event::Copy => out.push(0x03),
                egui::Event::Cut => out.push(0x18),
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    if let Some(b) = key_bytes(key, modifiers, mode.contains(TermMode::APP_CURSOR))
                    {
                        out.extend(b);
                    }
                }
                _ => {}
            }
        }
        if !out.is_empty() {
            if let Some(live) = panel.live.as_mut() {
                live.screen.scroll_to_bottom();
            }
            panel.send(&out);
        }
    }

    // Wheel: scroll back through history, or arrows in full-screen apps.
    if resp.hovered() {
        let dy = ui.input(|i| i.smooth_scroll_delta.y);
        let lines = (dy / cell.y).round() as i32;
        if lines != 0
            && let Some(live) = panel.live.as_mut()
        {
            let mode = live.screen.mode();
            if mode.contains(TermMode::ALT_SCREEN) {
                let key = if lines > 0 {
                    Key::ArrowUp
                } else {
                    Key::ArrowDown
                };
                let one = key_bytes(key, Modifiers::NONE, mode.contains(TermMode::APP_CURSOR))
                    .unwrap_or_default();
                let all = one.repeat(lines.unsigned_abs() as usize);
                panel.send(&all);
            } else {
                live.screen.scroll(lines);
            }
        }
    }

    if let Some(live) = panel.live.as_ref() {
        let painter = ui.painter_at(outer);
        painter.rect_filled(outer, crate::theme::CARD_RADIUS, crate::theme::code_well());
        let edge = if focused {
            crate::theme::fg().gamma_multiply(0.5)
        } else {
            crate::theme::border()
        };
        painter.rect_stroke(
            outer,
            crate::theme::CARD_RADIUS,
            egui::Stroke::new(1.0, edge),
            egui::StrokeKind::Inside,
        );
        paint(ui, rect, cell, &font, &live.screen, focused);
    }
}

fn paint(ui: &egui::Ui, rect: Rect, cell: Vec2, font: &FontId, screen: &Screen, focused: bool) {
    let painter = ui.painter_at(rect);
    let bg = crate::theme::code_well();
    let fg = crate::theme::fg();
    let content = screen.term.renderable_content();
    let offset = content.display_offset as i32;
    let colors = content.colors;
    for item in content.display_iter {
        let row = item.point.line.0 + offset;
        let col = item.point.column.0;
        if row < 0 || item.flags.contains(Flags::WIDE_CHAR_SPACER) {
            continue;
        }
        let pos = rect.min + Vec2::new(col as f32 * cell.x, row as f32 * cell.y);
        let mut cfg = resolve(item.fg, colors).unwrap_or(fg);
        let mut cbg = resolve(item.bg, colors);
        if item.flags.contains(Flags::INVERSE) {
            let was = cfg;
            cfg = cbg.unwrap_or(bg);
            cbg = Some(was);
        }
        if item.flags.contains(Flags::DIM) {
            cfg = cfg.gamma_multiply(0.66);
        }
        let width = if item.flags.contains(Flags::WIDE_CHAR) {
            cell.x * 2.0
        } else {
            cell.x
        };
        if let Some(b) = cbg {
            painter.rect_filled(Rect::from_min_size(pos, Vec2::new(width, cell.y)), 0.0, b);
        }
        if item.c != ' ' && !item.flags.contains(Flags::HIDDEN) {
            painter.text(pos, egui::Align2::LEFT_TOP, item.c, font.clone(), cfg);
        }
        if item.flags.intersects(Flags::ALL_UNDERLINES) {
            let y = pos.y + cell.y - 1.0;
            painter.line_segment(
                [Pos2::new(pos.x, y), Pos2::new(pos.x + width, y)],
                egui::Stroke::new(1.0, cfg),
            );
        }
    }
    if let Some((row, col)) = screen.cursor() {
        let at = rect.min + Vec2::new(col as f32 * cell.x, row as f32 * cell.y);
        let r = Rect::from_min_size(at, cell);
        if focused {
            painter.rect_filled(r, 0.0, fg.gamma_multiply(0.6));
        } else {
            painter.rect_stroke(r, 0.0, egui::Stroke::new(1.0, fg), egui::StrokeKind::Inside);
        }
    }
}

#[cfg(test)]
#[path = "code_terminal_tests.rs"]
mod tests;
