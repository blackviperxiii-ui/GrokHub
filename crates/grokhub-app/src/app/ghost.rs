//! Inline sentence autocomplete (ghost text) for the chat composer.

use super::*;
use egui::text::CCursor;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::mpsc;
use std::time::{Duration, Instant};

pub(super) const GHOST_DEBOUNCE: Duration = Duration::from_millis(400);
const MIN_DRAFT_CHARS: usize = 3;
const SYSTEM: &str =
    "Complete the user's partial chat message. Reply with only the continuation (no quotes, no preamble). One short line only. Do not repeat text they already typed.";

#[derive(Clone, Debug, PartialEq, Eq)]
struct DraftKey {
    text: String,
    cursor: usize,
    attach_fp: u64,
}

struct GhostReply {
    gen: u64,
    draft: String,
    suggestion: String,
}

#[derive(Default)]
pub(super) struct ComposerGhost {
    generation: u64,
    key: Option<DraftKey>,
    suggestion: String,
    pending_rx: Option<mpsc::Receiver<GhostReply>>,
    debounce_at: Option<Instant>,
    ime_preedit: bool,
    /// Draft we last scheduled or answered for (debounce tests).
    scheduled_draft: String,
}

impl ComposerGhost {
    pub(super) fn clear(&mut self) {
        self.generation += 1;
        self.key = None;
        self.suggestion.clear();
        self.pending_rx = None;
        self.debounce_at = None;
        self.ime_preedit = false;
        self.scheduled_draft.clear();
    }

    pub(super) fn note_ime(&mut self, events: &[egui::Event]) {
        for ev in events {
            if let egui::Event::Ime(ime) = ev {
                match ime {
                    egui::ImeEvent::Preedit { text, .. } => {
                        self.ime_preedit = !text.is_empty();
                    }
                    egui::ImeEvent::Commit(_) => {
                        self.ime_preedit = false;
                    }
                    _ => {}
                }
            }
        }
    }

    pub(super) fn dismiss(&mut self) -> bool {
        if self.suggestion.is_empty() {
            return false;
        }
        self.suggestion.clear();
        true
    }

    pub(super) fn has_suggestion(&self) -> bool {
        !self.suggestion.is_empty()
    }

    pub(super) fn can_accept(&self, slash_open: bool) -> bool {
        self.has_suggestion() && !slash_open && !self.ime_preedit
    }

    pub(super) fn accept(&mut self, composer: &mut String) -> bool {
        if self.suggestion.is_empty() || self.ime_preedit {
            return false;
        }
        let prior = composer.clone();
        composer.push_str(&self.suggestion);
        debug_assert_eq!(prior.as_bytes(), &composer.as_bytes()[..prior.len()]);
        self.clear();
        true
    }

    pub(super) fn paint(
        &self,
        ui: &mut egui::Ui,
        edit_rect: egui::Rect,
        composer: &str,
        text_w: f32,
    ) -> bool {
        if self.suggestion.is_empty() || composer.is_empty() {
            return false;
        }
        let ink = super::chat_ui::composer_hint_ink();
        let font = egui::FontId::new(15.0, egui::FontFamily::Proportional);
        let (prelude, ghost) = ui.fonts_mut(|fonts| {
            let prelude = fonts.layout(
                composer.to_string(),
                font.clone(),
                crate::theme::fg(),
                text_w,
            );
            let ghost = fonts.layout_no_wrap(self.suggestion.clone(), font, ink);
            (prelude, ghost)
        });
        let cursor = CCursor::new(composer.chars().count());
        let caret = prelude.pos_from_cursor(cursor);
        let pos = edit_rect.min + caret.min.to_vec2();
        let response = ui.allocate_rect(
            egui::Rect::from_min_size(pos, ghost.size()),
            egui::Sense::click(),
        );
        ui.painter().galley(pos, ghost, ink);
        response.on_hover_text("Tab to accept").clicked()
    }

    pub(super) fn frame(
        &mut self,
        enabled: bool,
        composer: &str,
        cursor_index: usize,
        attach_fp: u64,
        slash_open: bool,
        turn_running: bool,
        api_key: &str,
        model: &str,
    ) {
        if !enabled {
            if self.key.is_some() || self.has_suggestion() || self.pending_rx.is_some() {
                self.clear();
            }
            return;
        }
        let key = DraftKey {
            text: composer.to_string(),
            cursor: cursor_index,
            attach_fp,
        };
        if self.key.as_ref() != Some(&key) {
            self.key = Some(key);
            self.suggestion.clear();
            self.generation += 1;
            self.pending_rx = None;
            if should_request(composer, slash_open, turn_running) {
                self.debounce_at = Some(Instant::now() + GHOST_DEBOUNCE);
            } else {
                self.debounce_at = None;
            }
        }
        self.poll_replies();
        if !should_request(composer, slash_open, turn_running) {
            return;
        }
        if !cursor_at_end(composer, cursor_index) {
            return;
        }
        if api_key.trim().is_empty() {
            return;
        }
        if self.pending_rx.is_some() {
            return;
        }
        let Some(at) = self.debounce_at else {
            return;
        };
        if Instant::now() < at {
            return;
        }
        self.debounce_at = None;
        let draft = composer.to_string();
        self.scheduled_draft = draft.clone();
        let gen = self.generation;
        let (tx, rx) = mpsc::channel();
        self.pending_rx = Some(rx);
        let api_key = api_key.to_string();
        let model = model.to_string();
        std::thread::spawn(move || {
            let messages = [
                ("system".to_string(), SYSTEM.to_string()),
                (
                    "user".to_string(),
                    format!("Continue this draft:\n{draft}"),
                ),
            ];
            let suggestion = crate::xai::grok_chat(
                &api_key,
                &model,
                &messages,
                None,
                Some(grokhub_core::BACKGROUND_EFFORT),
            )
            .ok()
            .map(|s| normalize_suggestion(&s, &draft))
            .unwrap_or_default();
            let _ = tx.send(GhostReply {
                gen,
                draft,
                suggestion,
            });
        });
    }

    fn poll_replies(&mut self) {
        let Some(rx) = self.pending_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(reply) => {
                self.pending_rx = None;
                if reply.gen != self.generation {
                    return;
                }
                if self.key.as_ref().map(|k| k.text.as_str()) != Some(reply.draft.as_str()) {
                    return;
                }
                if reply.suggestion.is_empty() {
                    return;
                }
                self.suggestion = reply.suggestion;
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.pending_rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {}
        }
    }

    #[cfg(test)]
    pub(super) fn scheduled_draft(&self) -> &str {
        &self.scheduled_draft
    }

    #[cfg(test)]
    pub(super) fn set_debounce_ready(&mut self) {
        self.debounce_at = Some(Instant::now());
    }
}

impl Cabin {
    pub(super) fn attach_fingerprint(&self) -> u64 {
        let mut h = DefaultHasher::new();
        self.attach_url.hash(&mut h);
        self.attach_name.hash(&mut h);
        self.attach_path.hash(&mut h);
        if let Some(k) = self.attach_kind {
            format!("{k:?}").hash(&mut h);
        }
        self.attach_tray.len().hash(&mut h);
        for item in self.attach_tray.queue.items() {
            item.path.hash(&mut h);
        }
        h.finish()
    }

    pub(super) fn composer_cursor_index(&self, ctx: &egui::Context, composer_id: egui::Id) -> usize {
        egui::TextEdit::load_state(ctx, composer_id)
            .and_then(|s| s.cursor.char_range())
            .map(|r| usize::from(r.primary.index))
            .unwrap_or(0)
    }

    pub(super) fn poll_composer_ghost(&mut self, ctx: &egui::Context, composer_id: egui::Id) {
        let slash_open =
            !filter_slash_hits(&self.composer, &self.grok_commands).is_empty();
        let cursor = self.composer_cursor_index(ctx, composer_id);
        let attach_fp = self.attach_fingerprint();
        let model = resolve_chat_model(&self.cfg.mode, &self.cfg.model);
        let api_key = self.bearer();
        self.composer_ghost.frame(
            self.cfg.composer_autocomplete,
            &self.composer,
            cursor,
            attach_fp,
            slash_open,
            self.running,
            &api_key,
            &model,
        );
    }
}

pub(super) fn should_request(composer: &str, slash_open: bool, turn_running: bool) -> bool {
    if slash_open || turn_running {
        return false;
    }
    let t = composer.trim();
    if t.is_empty() || t.starts_with('/') || t.len() < MIN_DRAFT_CHARS {
        return false;
    }
    true
}

fn cursor_at_end(composer: &str, cursor_index: usize) -> bool {
    cursor_index >= composer.chars().count()
}

pub(super) fn normalize_suggestion(raw: &str, draft: &str) -> String {
    let line = raw
        .lines()
        .next()
        .unwrap_or("")
        .trim()
        .trim_matches('"')
        .trim_matches('\'');
    if line.is_empty() {
        return String::new();
    }
    let draft_trim = draft.trim_end();
    line.strip_prefix(draft_trim)
        .map(|tail| tail.trim_start().to_string())
        .unwrap_or_else(|| line.to_string())
}

#[cfg(test)]
#[allow(clippy::field_reassign_with_default)]
mod tests {
    use super::*;

    #[test]
    fn setting_off_should_not_request() {
        assert!(should_request("hello there", false, false));
        let mut g = ComposerGhost::default();
        g.frame(false, "hello there", 11, 0, false, false, "sk-test", "grok-4.7");
        assert!(g.scheduled_draft().is_empty());
        assert!(!g.has_suggestion());
        assert!(g.pending_rx.is_none());
    }

    #[test]
    fn absent_or_empty_draft_skips() {
        assert!(!should_request("", false, false));
        assert!(!should_request("  ", false, false));
        assert!(!should_request("/help", false, false));
        assert!(!should_request("ab", false, false));
    }

    #[test]
    fn slash_or_running_blocks() {
        assert!(!should_request("hello", true, false));
        assert!(!should_request("hello", false, true));
    }

    #[test]
    fn debounce_waits_before_schedule() {
        let mut g = ComposerGhost::default();
        g.frame(true, "hello there", 11, 0, false, false, "sk-test", "grok-4.7");
        assert!(g.debounce_at.is_some());
        assert!(g.pending_rx.is_none());
        assert!(g.scheduled_draft().is_empty());
        g.set_debounce_ready();
        g.frame(true, "hello there", 11, 0, false, false, "sk-test", "grok-4.7");
        assert_eq!(g.scheduled_draft(), "hello there");
        assert!(g.pending_rx.is_some());
    }

    #[test]
    fn stale_generation_dropped() {
        let mut g = ComposerGhost::default();
        g.generation = 2;
        g.key = Some(DraftKey {
            text: "hello".into(),
            cursor: 5,
            attach_fp: 0,
        });
        let (tx, rx) = mpsc::channel();
        let _ = tx.send(GhostReply {
            gen: 1,
            draft: "hello".into(),
            suggestion: " world".into(),
        });
        g.pending_rx = Some(rx);
        g.poll_replies();
        assert!(!g.has_suggestion());
    }

    #[test]
    fn accept_appends_without_mutating_prior_bytes() {
        let mut g = ComposerGhost::default();
        g.suggestion = " world".into();
        let mut composer = "hello".to_string();
        let before = composer.as_bytes().to_vec();
        assert!(g.accept(&mut composer));
        assert_eq!(composer, "hello world");
        assert_eq!(&composer.as_bytes()[..before.len()], before.as_slice());
    }

    #[test]
    fn dismiss_leaves_composer() {
        let mut g = ComposerGhost::default();
        g.suggestion = "hint".into();
        let composer = "hi".to_string();
        assert!(g.dismiss());
        assert_eq!(composer, "hi");
        assert!(!g.has_suggestion());
    }

    #[test]
    fn ime_blocks_accept() {
        let mut g = ComposerGhost::default();
        g.suggestion = "next".into();
        g.ime_preedit = true;
        assert!(!g.can_accept(false));
        let mut composer = "hi".to_string();
        assert!(!g.accept(&mut composer));
    }

    #[test]
    fn normalize_single_line() {
        assert_eq!(
            normalize_suggestion("one line\nand more", "hi"),
            "one line"
        );
        assert_eq!(normalize_suggestion("\"quoted\"", "hi"), "quoted");
    }

    #[test]
    fn typing_invalidates_suggestion() {
        let mut g = ComposerGhost::default();
        g.suggestion = "there".into();
        g.key = Some(DraftKey {
            text: "hello".into(),
            cursor: 5,
            attach_fp: 0,
        });
        g.frame(true, "hello!", 6, 0, false, false, "sk-test", "grok-4.7");
        assert!(!g.has_suggestion());
    }
}
