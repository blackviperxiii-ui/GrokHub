//! Settings → Personality (a card per preset, with Try it) and the quick switch in the
//! chat header. A chat can pick its own personality; "Make default" saves that pick as
//! the default for every chat. Switching is silent apart from the status line.

use super::*;
use grokhub_core::personality::{self, Personality};

/// What a "Try it" preview shows under its card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum TryState {
    Running,
    Reply(String),
    Failed(String),
}

/// The one "Try it" preview in flight or on screen.
#[derive(Default)]
pub(super) struct TryVoice {
    /// The preset id and what its card shows.
    pub shown: Option<(String, TryState)>,
    pub rx: Option<mpsc::Receiver<(String, Result<String, String>)>>,
}

/// How a preset id is stored: the default voice is an empty field.
pub(super) fn stored_id(id: &str) -> String {
    let id = id.trim();
    if id == personality::DEFAULT_ID {
        String::new()
    } else {
        id.to_string()
    }
}

/// The id a stored field stands for.
fn shown_id(stored: &str) -> &str {
    match stored.trim() {
        "" => personality::DEFAULT_ID,
        id => id,
    }
}

/// The status line after the default changes.
pub(super) fn default_note(p: &Personality) -> String {
    format!("Personality: {}", p.name)
}

/// The status line after one chat switches.
pub(super) fn chat_note(p: &Personality) -> String {
    format!("This chat: {}", p.name)
}

impl Cabin {
    /// The personality the chat being answered uses: its own pick, else the default.
    pub(super) fn personality_id(&self) -> String {
        let idx = self
            .chat_job_thread
            .as_deref()
            .and_then(|id| self.threads.iter().position(|t| t.id == id))
            .unwrap_or(self.thread_idx);
        let own = self.threads.get(idx).map(|t| t.personality.trim()).unwrap_or("");
        let id = if own.is_empty() {
            self.cfg.personality.trim()
        } else {
            own
        };
        shown_id(id).to_string()
    }

    /// The personality the open chat shows in its header.
    fn open_chat_personality(&self) -> String {
        let own = self
            .threads
            .get(self.thread_idx)
            .map(|t| t.personality.trim())
            .unwrap_or("");
        shown_id(if own.is_empty() {
            self.cfg.personality.trim()
        } else {
            own
        })
        .to_string()
    }

    /// Settings: the default for every chat that has not picked its own.
    pub(super) fn set_default_personality(&mut self, id: &str) {
        let p = personality::builtin(id);
        self.cfg.personality = stored_id(&p.id);
        self.persist_cfg();
        self.status = default_note(&p);
    }

    /// Chat header: this chat only. Picking the default's own voice clears the override.
    pub(super) fn set_chat_personality(&mut self, id: &str) {
        let p = personality::builtin(id);
        let own = if stored_id(&p.id) == self.cfg.personality.trim() {
            String::new()
        } else {
            p.id.clone()
        };
        let Some(thread) = self.threads.get_mut(self.thread_idx) else {
            return;
        };
        thread.personality = own;
        self.persist();
        self.status = chat_note(&p);
    }

    /// Chat header → Make default: the open chat's pick becomes everyone's default.
    pub(super) fn make_chat_personality_default(&mut self) {
        let id = self.open_chat_personality();
        if let Some(thread) = self.threads.get_mut(self.thread_idx) {
            thread.personality.clear();
        }
        self.persist();
        self.set_default_personality(&id);
    }

    fn try_personality(&mut self, id: &str) {
        if !self.llm_ready() {
            self.try_voice.shown = Some((id.to_string(), TryState::Failed(grokhub_core::XAI_NEED_SIGNIN.into())));
            return;
        }
        let key = self.bearer();
        let model = model_for_mode("fast").to_string();
        let messages = personality::try_it_messages(&personality::builtin(id));
        let (tx, rx) = mpsc::channel();
        let id = id.to_string();
        self.try_voice.shown = Some((id.clone(), TryState::Running));
        self.try_voice.rx = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send((id, grok_chat(&key, &model, &messages, None, None)));
        });
    }

    fn poll_try_voice(&mut self) {
        let Some(rx) = self.try_voice.rx.as_ref() else {
            return;
        };
        let Ok((id, out)) = rx.try_recv() else {
            return;
        };
        self.try_voice.rx = None;
        let state = match out {
            Ok(text) if !text.trim().is_empty() => TryState::Reply(text.trim().to_string()),
            Ok(_) => TryState::Failed("Grok sent an empty reply.".into()),
            Err(err) => TryState::Failed(err),
        };
        self.try_voice.shown = Some((id, state));
    }

    pub(super) fn ui_settings_personality(&mut self, ui: &mut egui::Ui) {
        self.poll_try_voice();
        if self.try_voice.rx.is_some() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(200));
        }
        crate::cards::help_text(
            ui,
            "How replies sound in chat. Tone only: approvals, warnings and facts stay plain in every personality. A chat can pick its own from the chat header.",
        );
        ui.add_space(10.0);
        let current = shown_id(&self.cfg.personality).to_string();
        let mut use_id: Option<String> = None;
        let mut try_id: Option<String> = None;
        // Two cards a row. A Frame's size is only known after it paints, so rows are
        // laid out by hand instead of with horizontal_wrapped.
        let card_w = ((ui.available_width() - 12.0) / 2.0).max(220.0);
        let presets = personality::builtins();
        for pair in presets.chunks(2) {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                for p in pair {
                    let on = p.id == current;
                    let tried = self
                        .try_voice
                        .shown
                        .as_ref()
                        .filter(|(id, _)| *id == p.id)
                        .map(|(_, state)| state.clone());
                    match paint_preset_card(ui, p, on, tried.as_ref(), card_w) {
                        Some(CardAct::Use) => use_id = Some(p.id.clone()),
                        Some(CardAct::Try) => try_id = Some(p.id.clone()),
                        None => {}
                    }
                }
            });
            ui.add_space(12.0);
        }
        if let Some(id) = use_id {
            self.set_default_personality(&id);
        }
        if let Some(id) = try_id {
            self.try_personality(&id);
        }
    }

    /// Titlebar, left of the session menu: the open chat's personality and a menu to switch it.
    pub(super) fn paint_personality_switch(&mut self, ui: &mut egui::Ui) {
        if self.nav != Nav::Chat {
            return;
        }
        let open = self.open_chat_personality();
        let now = personality::builtin(&open);
        let resp = ui.add(
            egui::Button::new(
                RichText::new(now.name.as_str())
                    .size(crate::theme::FONT_META)
                    .color(crate::theme::muted()),
            )
            .frame(false),
        );
        let resp = crate::theme::pointing(resp).on_hover_text("Personality for this chat");
        let id = ui.make_persistent_id("personality-switch");
        if resp.clicked() {
            egui::Popup::toggle_id(ui.ctx(), id);
        }
        let default = shown_id(&self.cfg.personality).to_string();
        let mut pick: Option<String> = None;
        let mut make_default = false;
        egui::Popup::new(id, ui.ctx().clone(), &resp, ui.layer_id())
            .open_memory(None)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
            .align(egui::RectAlign::BOTTOM_END)
            .layout(egui::Layout::top_down_justified(egui::Align::LEFT))
            .show(|ui| {
                ui.set_min_width(200.0);
                egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                    for p in personality::builtins() {
                        let mut label = p.name.clone();
                        if p.id == default && p.id != personality::DEFAULT_ID {
                            label.push_str(" (default)");
                        }
                        if ui.selectable_label(p.id == open, label).clicked() {
                            pick = Some(p.id.clone());
                        }
                    }
                });
                ui.separator();
                let differs = open != default;
                if ui
                    .add_enabled(differs, egui::Button::new("Make default").frame(false))
                    .clicked()
                {
                    make_default = true;
                }
            });
        if let Some(id) = pick {
            self.set_chat_personality(&id);
        }
        if make_default {
            self.make_chat_personality_default();
        }
    }
}

enum CardAct {
    Use,
    Try,
}

fn paint_preset_card(
    ui: &mut egui::Ui,
    p: &Personality,
    on: bool,
    tried: Option<&TryState>,
    width: f32,
) -> Option<CardAct> {
    let mut act = None;
    let stroke = if on { crate::theme::fg() } else { crate::theme::border() };
    egui::Frame::NONE
        .fill(crate::theme::surface())
        .corner_radius(12.0)
        .stroke(egui::Stroke::new(1.0_f32, stroke))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(width - 24.0);
            // horizontal_wrapped places the cards; each card stacks its own rows.
            ui.vertical(|ui| paint_preset_rows(ui, p, on, tried, &mut act));
        });
    act
}

fn paint_preset_rows(
    ui: &mut egui::Ui,
    p: &Personality,
    on: bool,
    tried: Option<&TryState>,
    act: &mut Option<CardAct>,
) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(&p.name)
                .size(crate::theme::FONT_BODY)
                .strong()
                .color(crate::theme::fg()),
        );
        if p.explicit {
            crate::cards::status_chip(ui, "Explicit", crate::cards::ChipTone::Setup);
        }
        if on {
            crate::cards::status_chip(ui, "In use", crate::cards::ChipTone::Live);
        }
    });
    ui.label(
        RichText::new(&p.description)
            .size(crate::theme::FONT_META)
            .color(crate::theme::muted()),
    );
    if !p.sample.trim().is_empty() {
        ui.add_space(4.0);
        ui.label(
            RichText::new(format!("\u{201c}{}\u{201d}", p.sample.trim()))
                .size(crate::theme::FONT_META)
                .italics()
                .color(crate::theme::subtle()),
        );
    }
    match tried {
        Some(TryState::Running) => {
            ui.add_space(4.0);
            ui.label(
                RichText::new("Trying…")
                    .size(crate::theme::FONT_META)
                    .color(crate::theme::muted()),
            );
        }
        Some(TryState::Reply(text)) => {
            ui.add_space(4.0);
            ui.label(
                RichText::new(text)
                    .size(crate::theme::FONT_META)
                    .color(crate::theme::fg()),
            );
        }
        Some(TryState::Failed(why)) => {
            ui.add_space(4.0);
            ui.label(
                RichText::new(format!("Try it failed: {why}"))
                    .size(crate::theme::FONT_META)
                    .color(crate::theme::offline()),
            );
        }
        None => {}
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if !on && crate::cards::ghost_pill(ui, "Use") {
            *act = Some(CardAct::Use);
        }
        let busy = matches!(tried, Some(TryState::Running));
        if !busy && crate::cards::ghost_pill(ui, "Try it") {
            *act = Some(CardAct::Try);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cabin_with_chat() -> (std::sync::MutexGuard<'static, ()>, crate::config::TestConfigDir, Cabin) {
        let lock = crate::config::hold_test_config();
        let root = crate::config::test_config_root("personality-ui");
        std::fs::create_dir_all(&root).unwrap();
        let pin = crate::config::TestConfigDir::set(root);
        let mut cabin = Cabin::quiet_for_test();
        cabin.threads = vec![
            threads::ChatThread::new("a", false),
            threads::ChatThread::new("b", false),
        ];
        cabin.thread_idx = 0;
        cabin.chat_job_thread = None;
        (lock, pin, cabin)
    }

    #[test]
    fn the_default_voice_is_stored_as_an_empty_field() {
        assert_eq!(stored_id("default"), "");
        assert_eq!(stored_id(" pirate "), "pirate");
        assert_eq!(shown_id(""), "default");
        assert_eq!(shown_id("butler"), "butler");
    }

    #[test]
    fn a_chat_pick_overrides_the_default_for_that_chat_only() {
        let (_lock, _root, mut cabin) = cabin_with_chat();
        cabin.set_default_personality("butler");
        assert_eq!(cabin.cfg.personality, "butler");
        assert_eq!(cabin.status, "Personality: Butler");
        cabin.set_chat_personality("pirate");
        assert_eq!(cabin.threads[0].personality, "pirate");
        assert_eq!(cabin.status, "This chat: Pirate");
        assert_eq!(cabin.personality_id(), "pirate");
        cabin.thread_idx = 1;
        assert_eq!(cabin.personality_id(), "butler");
        cabin.thread_idx = 0;
        // Picking the default's own voice clears the override instead of pinning it.
        cabin.set_chat_personality("butler");
        assert_eq!(cabin.threads[0].personality, "");
        assert_eq!(cabin.personality_id(), "butler");
    }

    #[test]
    fn make_default_moves_the_chat_pick_to_every_chat() {
        let (_lock, _root, mut cabin) = cabin_with_chat();
        cabin.set_chat_personality("noir-detective");
        cabin.make_chat_personality_default();
        assert_eq!(cabin.cfg.personality, "noir-detective");
        assert_eq!(cabin.threads[0].personality, "");
        assert_eq!(cabin.status, "Personality: Noir Detective");
        cabin.thread_idx = 1;
        assert_eq!(cabin.personality_id(), "noir-detective");
        cabin.set_default_personality("default");
        assert_eq!(cabin.cfg.personality, "");
        assert_eq!(cabin.personality_id(), "default");
    }

    #[test]
    fn the_reply_being_answered_follows_its_own_chat() {
        let (_lock, _root, mut cabin) = cabin_with_chat();
        cabin.threads[1].personality = "surfer".into();
        let busy = cabin.threads[1].id.clone();
        cabin.chat_job_thread = Some(busy);
        assert_eq!(cabin.personality_id(), "surfer");
        assert!(
            cabin.chat_rules().starts_with("Personality: Surfer. "),
            "{}",
            cabin.chat_rules()
        );
    }

    #[test]
    fn a_chat_pick_survives_a_save_and_reload() {
        let mut thread = threads::ChatThread::new("a", false);
        thread.personality = "genie".into();
        let text = serde_json::to_string(&thread).unwrap();
        assert!(text.contains("\"personality\":\"genie\""), "{text}");
        let back: threads::ChatThread = serde_json::from_str(&text).unwrap();
        assert_eq!(back.personality, "genie");
        let plain = serde_json::to_string(&threads::ChatThread::new("b", false)).unwrap();
        assert!(
            !plain.contains("personality"),
            "a chat that follows the default stores nothing: {plain}"
        );
    }

    #[test]
    fn try_it_without_sign_in_says_so_on_the_card() {
        let (_lock, _root, mut cabin) = cabin_with_chat();
        cabin.cfg.api_key.clear();
        cabin.secrets = Default::default();
        cabin.try_personality("pirate");
        assert_eq!(
            cabin.try_voice.shown,
            Some((
                "pirate".to_string(),
                TryState::Failed(grokhub_core::XAI_NEED_SIGNIN.into())
            ))
        );
        assert!(cabin.try_voice.rx.is_none());
    }
}
