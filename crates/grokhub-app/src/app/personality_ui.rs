//! Settings → Personality (a card per preset, with Try it, Duplicate and an editor for
//! the user's own) and the quick switch in the chat header. A chat can pick its own
//! personality; "Make default" saves that pick as the default for every chat. Switching
//! is silent apart from the status line. Custom presets live in `{config}/personalities/`.

use super::*;
use grokhub_core::personality::{self, Personality};

const ADULTS_ONLY: &str = "Adults-only personalities";
const ADULTS_ONLY_HINT: &str =
    "Show explicit presets like Viper. Off hides them, and a chat set to one uses the default personality.";

/// What a "Try it" preview shows under its card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum TryState {
    Running,
    Reply(String),
    Failed(String),
}

/// Settings → Personality state: the presets on disk, the one "Try it" preview, and the
/// custom preset being edited.
#[derive(Default)]
pub(super) struct PickerState {
    /// The preset id and what its card shows.
    pub shown: Option<(String, TryState)>,
    pub rx: Option<mpsc::Receiver<(String, Result<String, String>)>>,
    /// Built-ins, then the user's own. Read when Settings or the switch first needs them
    /// and again after every save or delete.
    pub presets: Vec<Personality>,
    /// Custom files that could not be used, each naming the file and why.
    pub problems: Vec<String>,
    pub loaded: bool,
    pub draft: Option<Draft>,
}

/// A custom preset open in the editor.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct Draft {
    /// As it is on disk; Reset goes back to this.
    pub saved: Personality,
    pub edit: Personality,
    /// Why the last Save was refused.
    pub error: String,
    /// Delete was clicked once; the second click deletes.
    pub confirm_delete: bool,
}

/// The user's own presets.
pub(super) fn presets_dir() -> std::path::PathBuf {
    crate::config::config_dir().join("personalities")
}

/// How a preset id is stored: the default voice is an empty field.
pub(super) fn stored_id(id: &str) -> String {
    let id = id.trim();
    if id == personality::DEFAULT_ID || id == personality::LEGACY_DEFAULT_ID {
        String::new()
    } else {
        id.to_string()
    }
}

/// The id a stored field stands for. The retired "default" preset is Nyx.
fn shown_id(stored: &str) -> &str {
    match stored.trim() {
        "" | personality::LEGACY_DEFAULT_ID => personality::DEFAULT_ID,
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
        let own = self
            .threads
            .get(idx)
            .map(|t| t.personality.trim())
            .unwrap_or("");
        let id = if own.is_empty() {
            self.cfg.personality.trim()
        } else {
            own
        };
        self.allowed_id(shown_id(id))
    }

    /// `id`, or the default voice's when it names an explicit preset and adults-only
    /// personalities are off.
    fn allowed_id(&self, id: &str) -> String {
        match personality::lookup(&presets_dir(), id) {
            p if p.explicit && !self.cfg.explicit_personalities => personality::DEFAULT_ID.into(),
            _ => id.to_string(),
        }
    }

    /// The preset the chat being answered uses, custom ones included.
    pub(super) fn chat_personality(&self) -> Personality {
        personality::gated(
            personality::lookup(&presets_dir(), &self.personality_id()),
            self.cfg.explicit_personalities,
        )
    }

    /// The TTS voice for spoken replies: the chat's personality's voice hint when it names
    /// a voice xAI has, else the default voice.
    pub(super) fn speech_voice(&self) -> &'static str {
        grokhub_core::tts_voice(self.chat_personality().voice_hint.as_deref())
    }

    fn reload_presets(&mut self) {
        let (presets, problems) = personality::load_all(&presets_dir());
        self.picker.presets = presets;
        self.picker.problems = problems;
        self.picker.loaded = true;
    }

    /// What the pickers offer: explicit presets only with adults-only personalities on.
    fn presets_now(&mut self) -> Vec<Personality> {
        if !self.picker.loaded {
            self.reload_presets();
        }
        personality::visible(&self.picker.presets, self.cfg.explicit_personalities)
    }

    pub(super) fn titlebar_personality_label(&mut self) -> String {
        let presets = self.presets_now();
        let id = if self.nav == Nav::Chat {
            self.open_chat_personality()
        } else {
            self.allowed_id(shown_id(self.cfg.personality.trim()))
                .to_string()
        };
        personality::find(&presets, &id).name
    }

    /// The personality the open chat shows in its header.
    fn open_chat_personality(&self) -> String {
        let own = self
            .threads
            .get(self.thread_idx)
            .map(|t| t.personality.trim())
            .unwrap_or("");
        self.allowed_id(shown_id(if own.is_empty() {
            self.cfg.personality.trim()
        } else {
            own
        }))
    }

    /// Settings: the default for every chat that has not picked its own.
    pub(super) fn set_default_personality(&mut self, id: &str) {
        let p = personality::gated(
            personality::lookup(&presets_dir(), id),
            self.cfg.explicit_personalities,
        );
        self.cfg.personality = stored_id(&p.id);
        self.persist_cfg();
        self.status = default_note(&p);
    }

    /// Chat header: this chat only. Picking the default's own voice clears the override.
    pub(super) fn set_chat_personality(&mut self, id: &str) {
        let p = personality::gated(
            personality::lookup(&presets_dir(), id),
            self.cfg.explicit_personalities,
        );
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

    /// Settings → Adults-only personalities. Turning it off hides the explicit presets;
    /// the stored picks stay, and speak in the default voice until it is back on.
    pub(super) fn set_explicit_personalities(&mut self, on: bool) {
        self.cfg.explicit_personalities = on;
        self.persist_cfg();
        self.status = if on {
            "Adults-only personalities on".into()
        } else {
            "Adults-only personalities off".into()
        };
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

    /// Duplicate: a new custom preset copied from `id`, saved at once and opened in the editor.
    pub(super) fn duplicate_personality(&mut self, id: &str) {
        let presets = self.presets_now();
        let copy = personality::duplicate(&personality::find(&presets, id), &presets);
        match personality::save_custom(&presets_dir(), &copy) {
            Ok(_) => {
                self.reload_presets();
                self.status = format!("Made {}", copy.name);
                self.picker.draft = Some(Draft {
                    saved: copy.clone(),
                    edit: copy,
                    error: String::new(),
                    confirm_delete: false,
                });
            }
            Err(err) => self.status = err,
        }
    }

    pub(super) fn edit_personality(&mut self, id: &str) {
        let p = personality::find(&self.presets_now(), id);
        if !p.custom {
            return;
        }
        self.picker.draft = Some(Draft {
            saved: p.clone(),
            edit: p,
            error: String::new(),
            confirm_delete: false,
        });
    }

    /// Save checks the preset first; a refused one stays in the editor with the reason.
    pub(super) fn save_personality_draft(&mut self) {
        let Some(draft) = self.picker.draft.as_mut() else {
            return;
        };
        let e = &mut draft.edit;
        e.name = e.name.trim().to_string();
        e.description = e.description.trim().to_string();
        e.tone_rules = e.tone_rules.trim().to_string();
        e.address = e
            .address
            .as_deref()
            .map(str::trim)
            .filter(|a| !a.is_empty())
            .map(String::from);
        e.voice_hint = e
            .voice_hint
            .as_deref()
            .map(str::trim)
            .filter(|h| !h.is_empty())
            .map(String::from);
        match personality::save_custom(&presets_dir(), &draft.edit) {
            Ok(_) => {
                draft.saved = draft.edit.clone();
                draft.error.clear();
                self.status = format!("Saved {}", draft.edit.name);
                self.reload_presets();
            }
            Err(err) => draft.error = err,
        }
    }

    /// Reset: drop unsaved edits and go back to the saved preset.
    pub(super) fn reset_personality_draft(&mut self) {
        if let Some(draft) = self.picker.draft.as_mut() {
            draft.edit = draft.saved.clone();
            draft.error.clear();
            draft.confirm_delete = false;
        }
    }

    /// Delete (second click): removes the file. The default and any chat that used it go
    /// back to the default voice.
    pub(super) fn delete_personality_draft(&mut self) {
        let Some(draft) = self.picker.draft.as_mut() else {
            return;
        };
        if !draft.confirm_delete {
            draft.confirm_delete = true;
            return;
        }
        let (id, name) = (draft.saved.id.clone(), draft.saved.name.clone());
        if let Err(err) = personality::delete_custom(&presets_dir(), &id) {
            draft.error = err;
            return;
        }
        self.picker.draft = None;
        if self.cfg.personality == id {
            self.cfg.personality.clear();
            self.persist_cfg();
        }
        let mut chats = false;
        for thread in self.threads.iter_mut().filter(|t| t.personality == id) {
            thread.personality.clear();
            chats = true;
        }
        if chats {
            self.persist();
        }
        self.reload_presets();
        self.status = format!("Deleted {name}");
    }

    fn try_personality(&mut self, id: &str) {
        if !self.llm_ready() {
            self.picker.shown = Some((
                id.to_string(),
                TryState::Failed(grokhub_core::XAI_NEED_SIGNIN.into()),
            ));
            return;
        }
        let key = self.bearer();
        let model = model_for_mode("fast").to_string();
        let messages = personality::try_it_messages(&personality::lookup(&presets_dir(), id));
        let (tx, rx) = mpsc::channel();
        let id = id.to_string();
        self.picker.shown = Some((id.clone(), TryState::Running));
        self.picker.rx = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send((id, grok_chat(&key, &model, &messages, None, None)));
        });
    }

    fn poll_try_it(&mut self) {
        let Some(rx) = self.picker.rx.as_ref() else {
            return;
        };
        let Ok((id, out)) = rx.try_recv() else {
            return;
        };
        self.picker.rx = None;
        let state = match out {
            Ok(text) if !text.trim().is_empty() => TryState::Reply(text.trim().to_string()),
            Ok(_) => TryState::Failed("Grok sent an empty reply.".into()),
            Err(err) => TryState::Failed(err),
        };
        self.picker.shown = Some((id, state));
    }

    pub(super) fn ui_settings_personality(&mut self, ui: &mut egui::Ui) {
        self.poll_try_it();
        if self.picker.rx.is_some() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(200));
        }
        crate::cards::help_text(
            ui,
            "How replies sound in chat. Tone only: approvals, warnings and facts stay plain in every personality. A chat can pick its own from the chat header.",
        );
        ui.add_space(10.0);
        let mut adults = self.cfg.explicit_personalities;
        if crate::cards::settings_toggle(ui, ADULTS_ONLY, ADULTS_ONLY_HINT, &mut adults) {
            self.set_explicit_personalities(adults);
        }
        ui.add_space(10.0);
        if self.picker.draft.is_some() {
            self.ui_personality_editor(ui);
            ui.add_space(16.0);
        }
        let presets = self.presets_now();
        for problem in self.picker.problems.clone() {
            crate::cards::settings_note(ui, &format!("Skipped {problem}"));
        }
        let current = shown_id(&self.cfg.personality).to_string();
        let mut act_on: Option<(CardAct, String)> = None;
        // Two cards a row. A Frame's size is only known after it paints, so rows are
        // laid out by hand instead of with horizontal_wrapped.
        let card_w = ((ui.available_width() - 12.0) / 2.0).max(220.0);
        for pair in presets.chunks(2) {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                for p in pair {
                    let on = p.id == current;
                    let tried = self
                        .picker
                        .shown
                        .as_ref()
                        .filter(|(id, _)| *id == p.id)
                        .map(|(_, state)| state.clone());
                    if let Some(act) = paint_preset_card(ui, p, on, tried.as_ref(), card_w) {
                        act_on = Some((act, p.id.clone()));
                    }
                }
            });
            ui.add_space(12.0);
        }
        match act_on {
            Some((CardAct::Use, id)) => self.set_default_personality(&id),
            Some((CardAct::Try, id)) => self.try_personality(&id),
            Some((CardAct::Duplicate, id)) => self.duplicate_personality(&id),
            Some((CardAct::Edit, id)) => self.edit_personality(&id),
            None => {}
        }
    }

    fn ui_personality_editor(&mut self, ui: &mut egui::Ui) {
        let Some(mut draft) = self.picker.draft.clone() else {
            return;
        };
        let mut save = false;
        let mut reset = false;
        let mut delete = false;
        let mut close = false;
        egui::Frame::NONE
            .fill(crate::theme::surface())
            .corner_radius(12.0)
            .stroke(egui::Stroke::new(1.0_f32, crate::theme::fg()))
            .inner_margin(egui::Margin::same(14))
            .show(ui, |ui| {
                crate::cards::section_label(ui, &format!("Edit {}", draft.saved.name));
                crate::cards::help_text(
                    ui,
                    &format!(
                        "Saved as personalities/{}.toml in the cabin's settings folder. Tone only: approvals and safety rules still come after it.",
                        draft.edit.id
                    ),
                );
                let e = &mut draft.edit;
                crate::cards::settings_field(ui, "Name", &format!("Up to {} characters.", personality::NAME_MAX), &mut e.name, false);
                crate::cards::settings_field(
                    ui,
                    "Description",
                    &format!("One line, up to {} characters.", personality::DESCRIPTION_MAX),
                    &mut e.description,
                    false,
                );
                ui.label(RichText::new("Tone rules").size(15.0).color(crate::theme::fg()));
                ui.label(
                    RichText::new(format!(
                        "How it talks, {} of {} characters.",
                        e.tone_rules.chars().count(),
                        personality::TONE_RULES_MAX
                    ))
                    .size(12.0)
                    .color(crate::theme::muted()),
                );
                ui.add_space(6.0);
                ui.add(egui::TextEdit::multiline(&mut e.tone_rules).desired_rows(4).desired_width(f32::INFINITY));
                ui.add_space(10.0);
                let emoji = [
                    (personality::Emoji::None, "None"),
                    (personality::Emoji::Light, "Light"),
                    (personality::Emoji::Heavy, "Heavy"),
                ];
                let labels: Vec<String> = emoji.iter().map(|(_, l)| l.to_string()).collect();
                let now = emoji.iter().find(|(v, _)| *v == e.emoji).map(|(_, l)| *l).unwrap_or("None");
                if let Some(i) = crate::cards::settings_dropdown(ui, "Emoji", "", now, &labels) {
                    e.emoji = emoji[i].0;
                }
                let verbosity = [
                    (personality::Verbosity::Terse, "Terse"),
                    (personality::Verbosity::Normal, "Normal"),
                    (personality::Verbosity::Chatty, "Chatty"),
                ];
                let labels: Vec<String> = verbosity.iter().map(|(_, l)| l.to_string()).collect();
                let now = verbosity.iter().find(|(v, _)| *v == e.verbosity).map(|(_, l)| *l).unwrap_or("Normal");
                if let Some(i) = crate::cards::settings_dropdown(ui, "Length", "", now, &labels) {
                    e.verbosity = verbosity[i].0;
                }
                let mut address = e.address.clone().unwrap_or_default();
                crate::cards::settings_field(ui, "Calls you", "Optional, like sir or captain.", &mut address, false);
                // Trimmed on save, so a space typed mid-word survives the next frame.
                e.address = Some(address).filter(|a| !a.trim().is_empty());
                let mut hint = e.voice_hint.clone().unwrap_or_default();
                crate::cards::settings_field(
                    ui,
                    "Voice",
                    &format!(
                        "Spoken replies can use one of these voices: {}. Anything else speaks as {}.",
                        grokhub_core::TTS_VOICES.join(", "),
                        grokhub_core::DEFAULT_VOICE
                    ),
                    &mut hint,
                    false,
                );
                e.voice_hint = Some(hint).filter(|h| !h.trim().is_empty());
                if !draft.error.is_empty() {
                    ui.label(RichText::new(&draft.error).size(crate::theme::FONT_META).color(crate::theme::offline()));
                    ui.add_space(6.0);
                }
                ui.horizontal(|ui| {
                    if crate::cards::white_pill(ui, "Save") {
                        save = true;
                    }
                    if crate::cards::ghost_pill(ui, "Reset") {
                        reset = true;
                    }
                    let label = if draft.confirm_delete {
                        format!("Delete {}?", draft.saved.name)
                    } else {
                        "Delete".to_string()
                    };
                    if crate::cards::ghost_pill(ui, &label) {
                        delete = true;
                    }
                    if crate::cards::ghost_pill(ui, "Close") {
                        close = true;
                    }
                });
            });
        self.picker.draft = Some(draft);
        if save {
            self.save_personality_draft();
        }
        if reset {
            self.reset_personality_draft();
        }
        if delete {
            self.delete_personality_draft();
        }
        if close {
            self.picker.draft = None;
        }
    }

    /// Titlebar, left of the session menu: the open chat's personality and a menu to switch it.
    pub(super) fn paint_personality_switch(&mut self, ui: &mut egui::Ui) {
        if self.nav != Nav::Chat {
            return;
        }
        let open = self.open_chat_personality();
        let presets = self.presets_now();
        let now = personality::find(&presets, &open);
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
                egui::ScrollArea::vertical()
                    .max_height(360.0)
                    .show(ui, |ui| {
                        for p in &presets {
                            let mut label = p.name.clone();
                            if p.id == default && p.id != personality::DEFAULT_ID {
                                label.push_str(" (default)");
                            }
                            if p.explicit {
                                label.push_str(" · Explicit");
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
    Duplicate,
    Edit,
}

fn paint_preset_card(
    ui: &mut egui::Ui,
    p: &Personality,
    on: bool,
    tried: Option<&TryState>,
    width: f32,
) -> Option<CardAct> {
    let mut act = None;
    let stroke = if on {
        crate::theme::fg()
    } else {
        crate::theme::border()
    };
    egui::Frame::NONE
        .fill(crate::theme::surface())
        .corner_radius(12.0)
        .stroke(egui::Stroke::new(1.0_f32, stroke))
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(width - 24.0);
            // Each card stacks its own rows.
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
        if p.custom {
            crate::cards::status_chip(ui, "Yours", crate::cards::ChipTone::Mute);
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
        if p.custom {
            if crate::cards::ghost_pill(ui, "Edit") {
                *act = Some(CardAct::Edit);
            }
        } else if crate::cards::ghost_pill(ui, "Duplicate") {
            *act = Some(CardAct::Duplicate);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cabin_with_chat() -> (
        std::sync::MutexGuard<'static, ()>,
        crate::config::TestConfigDir,
        Cabin,
    ) {
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
    fn adults_only_off_hides_explicit_presets_and_mutes_a_stored_pick() {
        let (_lock, _root, mut cabin) = cabin_with_chat();
        assert!(!cabin.cfg.explicit_personalities, "off by default");
        let ids = |c: &mut Cabin| {
            c.presets_now()
                .into_iter()
                .map(|p| p.id)
                .collect::<Vec<_>>()
        };
        assert!(!ids(&mut cabin)
            .iter()
            .any(|id| id == "viper" || id == "whore"));
        // Picking an explicit preset while off picks the default voice instead.
        cabin.set_chat_personality("whore");
        assert_eq!(cabin.threads[0].personality, "");
        assert_eq!(cabin.chat_personality().id, "nyx");

        cabin.set_explicit_personalities(true);
        assert!(cabin.cfg.explicit_personalities);
        crate::config::save(&cabin.cfg).unwrap();
        assert!(
            crate::config::load().explicit_personalities,
            "round-trips in app.json"
        );
        assert!(ids(&mut cabin).ends_with(&["viper".to_string(), "whore".to_string()]));
        cabin.set_chat_personality("whore");
        assert_eq!(cabin.threads[0].personality, "whore");
        assert_eq!(cabin.chat_personality().id, "whore");
        assert!(cabin
            .chat_personality()
            .prompt_block()
            .starts_with("Personality: Whore. Whore mode:"));

        // Off again: the pick stays stored but the chat speaks as the default voice.
        cabin.set_explicit_personalities(false);
        assert_eq!(cabin.threads[0].personality, "whore");
        assert_eq!(cabin.personality_id(), "nyx");
        assert_eq!(cabin.chat_personality().id, "nyx");
        assert_eq!(cabin.open_chat_personality(), "nyx");
    }

    #[test]
    fn the_default_voice_is_stored_as_an_empty_field() {
        assert_eq!(stored_id("nyx"), "");
        // The retired "default" preset stores as the default voice too.
        assert_eq!(stored_id("default"), "");
        assert_eq!(shown_id("default"), "nyx");
        assert_eq!(stored_id(" pirate "), "pirate");
        assert_eq!(shown_id(""), "nyx");
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
        assert_eq!(cabin.personality_id(), "nyx");
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
    fn live_prompt_parts_put_safety_first_and_close_the_voice_tail() {
        let (_lock, _root, mut cabin) = cabin_with_chat();
        cabin.set_chat_personality("pirate");
        let (stable, volatile) = cabin.chat_prompt_parts();
        assert!(
            stable.contains("Ask only before"),
            "cabin safety stays in the stable prefix: {stable}"
        );
        assert!(
            !stable.contains("Personality: Pirate"),
            "voice must not bust the stable prefix: {stable}"
        );
        assert!(
            volatile.contains("Personality: Pirate"),
            "voice lives in the volatile tail: {volatile}"
        );
        assert!(
            volatile.contains("earlier in this prompt always win"),
            "closer reasserts cabin safety after the voice: {volatile}"
        );
        assert!(
            volatile.find("Personality: Pirate").unwrap()
                < volatile.find("earlier in this prompt always win").unwrap(),
            "closer is last: {volatile}"
        );
        let (plain_stable, plain_volatile) = {
            cabin.set_chat_personality("default");
            cabin.chat_prompt_parts()
        };
        assert_eq!(
            plain_stable, stable,
            "default voice keeps the same stable prefix"
        );
        assert!(
            plain_volatile.contains("Personality: Nyx."),
            "the default voice carries its values: {plain_volatile}"
        );
        assert!(
            plain_volatile.contains("not sycophantic"),
            "{plain_volatile}"
        );
        assert!(
            plain_volatile.contains("earlier in this prompt always win"),
            "the values voice still closes under the cabin rules: {plain_volatile}"
        );
    }

    #[test]
    fn chat_prompt_parts_put_soul_and_user_before_the_closer() {
        let (_lock, _pin, mut cabin) = cabin_with_chat();
        let root = crate::config::config_dir();
        std::fs::create_dir_all(root.join("memory")).unwrap();
        std::fs::write(
            root.join("memory").join("SOUL.md"),
            "HARBOR_SOUL_MARK speaks plainly\n",
        )
        .unwrap();
        std::fs::write(
            root.join("memory").join("USER.md"),
            "HARBOR_USER_MARK keeps a paper chart\n",
        )
        .unwrap();
        cabin.set_chat_personality("pirate");
        let (stable, volatile) = cabin.chat_prompt_parts();
        assert_eq!(
            stable,
            grokhub_core::cabin_rules::cabin_static_rules(cabin.cfg.desktop_control),
            "SOUL and USER stay out of the stable prefix"
        );
        assert!(!stable.contains("HARBOR_SOUL_MARK"), "{stable}");
        assert!(!stable.contains("HARBOR_USER_MARK"), "{stable}");
        let voice = volatile.find("Personality: Pirate").expect("voice");
        let soul = volatile.find("HARBOR_SOUL_MARK").expect("soul");
        let user = volatile.find("HARBOR_USER_MARK").expect("user");
        let closer = volatile
            .find("earlier in this prompt always win")
            .expect("closer");
        assert!(voice < soul && soul < user && user < closer, "{volatile}");
        cabin.set_chat_personality("butler");
        let (again, _) = cabin.chat_prompt_parts();
        assert_eq!(again, stable);
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
            cabin.picker.shown,
            Some((
                "pirate".to_string(),
                TryState::Failed(grokhub_core::XAI_NEED_SIGNIN.into())
            ))
        );
        assert!(cabin.picker.rx.is_none());
    }

    #[test]
    fn duplicate_edit_reset_save_and_delete_from_settings() {
        let (_lock, _root, mut cabin) = cabin_with_chat();
        cabin.duplicate_personality("butler");
        assert_eq!(cabin.status, "Made Butler copy");
        assert!(presets_dir().join("butler-copy.toml").exists());
        let draft = cabin
            .picker
            .draft
            .clone()
            .expect("the copy opens in the editor");
        assert_eq!(
            (draft.saved.id.as_str(), draft.edit.name.as_str()),
            ("butler-copy", "Butler copy")
        );

        // Reset drops unsaved edits.
        cabin.picker.draft.as_mut().unwrap().edit.name = "Scratch".into();
        cabin.reset_personality_draft();
        assert_eq!(
            cabin.picker.draft.as_ref().unwrap().edit.name,
            "Butler copy"
        );

        // A refused save keeps the editor open with the reason and writes nothing.
        cabin.picker.draft.as_mut().unwrap().edit.description = "two\nlines".into();
        cabin.save_personality_draft();
        assert_eq!(
            cabin.picker.draft.as_ref().unwrap().error,
            format!("Personality butler-copy: the description must be one line of at most {} characters.", personality::DESCRIPTION_MAX)
        );
        cabin.reset_personality_draft();

        cabin.picker.draft.as_mut().unwrap().edit.name = " Jeeves ".into();
        cabin.picker.draft.as_mut().unwrap().edit.address = Some("  ".into());
        cabin.picker.draft.as_mut().unwrap().edit.voice_hint = Some("sal ".into());
        cabin.save_personality_draft();
        let saved = &cabin.picker.draft.as_ref().unwrap().saved;
        assert_eq!(
            (
                saved.name.as_str(),
                saved.address.as_deref(),
                saved.voice_hint.as_deref()
            ),
            ("Jeeves", None, Some("sal"))
        );
        assert_eq!(cabin.status, "Saved Jeeves");
        assert!(cabin
            .picker
            .presets
            .iter()
            .any(|p| p.id == "butler-copy" && p.name == "Jeeves" && p.custom));

        // In use as the default and in one chat, it speaks in its own voice.
        cabin.set_default_personality("butler-copy");
        assert_eq!(cabin.status, "Personality: Jeeves");
        cabin.set_chat_personality("butler-copy");
        assert!(
            cabin.chat_rules().starts_with("Personality: Jeeves. "),
            "{}",
            cabin.chat_rules()
        );
        assert_eq!(cabin.speech_voice(), "sal");
        cabin.threads[1].personality = "butler-copy".into();

        // Delete takes two clicks, then everything that used it goes back to the default.
        cabin.delete_personality_draft();
        assert!(
            presets_dir().join("butler-copy.toml").exists(),
            "the first click only asks"
        );
        cabin.delete_personality_draft();
        assert!(!presets_dir().join("butler-copy.toml").exists());
        assert_eq!(cabin.status, "Deleted Jeeves");
        assert!(cabin.picker.draft.is_none());
        assert_eq!(cabin.cfg.personality, "");
        assert_eq!(cabin.threads[1].personality, "");
        assert_eq!(cabin.personality_id(), "nyx");
        assert_eq!(cabin.speech_voice(), grokhub_core::DEFAULT_VOICE);
    }

    #[test]
    fn built_ins_open_no_editor_and_a_style_note_speaks_in_the_default_voice() {
        let (_lock, _root, mut cabin) = cabin_with_chat();
        cabin.edit_personality("pirate");
        assert!(
            cabin.picker.draft.is_none(),
            "built-ins are duplicated, not edited"
        );
        cabin.set_chat_personality("butler");
        assert_eq!(
            cabin.speech_voice(),
            "rex",
            "Butler's hint names a real voice"
        );
        cabin.set_chat_personality("genie");
        assert_eq!(cabin.speech_voice(), "eve");
        cabin.set_chat_personality("pirate");
        assert_eq!(cabin.speech_voice(), grokhub_core::DEFAULT_VOICE, "no hint");
    }
}
