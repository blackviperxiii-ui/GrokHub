//! Settings → Personality (deck slide 19): a card grid of every preset and a
//! Selected detail card (big letter icon, name, tags, quote, amber Talk as).
//! A chat can pick its own personality; "Make default" / Talk as saves that
//! pick as the default for every chat. Switching is silent apart from the
//! status line. Custom presets live in `{config}/personalities/`.
//! Optional built-in photos may land later at
//! `crates/grokhub-app/assets/personalities/<id>/1..5.png` — Loadout matches
//! slide 19 with the letter icon, not a placeholder photo strip.

use super::*;
use grokhub_core::personality::{self, Personality};

include!(concat!(env!("OUT_DIR"), "/personality_photos.rs"));

const ADULTS_ONLY: &str = "Adults-only personalities";
const ADULTS_ONLY_HINT: &str =
    "Show explicit presets like Viper. Off hides them, and a chat set to one uses the default personality.";
/// Width of the selected detail panel beside the grid (slide 19).
const DETAIL_W: f32 = 320.0;
/// Below this the detail panel stacks under the grid.
const TWO_COL_MIN_W: f32 = 640.0;
const GRID_GAP: f32 = 10.0;
const CARD_MIN_W: f32 = 140.0;
/// Big letter icon on the Selected card (slide 19).
const DETAIL_ICON: f32 = 96.0;
/// Kept for optional photo embed rebuilds when real art lands.
#[allow(dead_code)]
const PHOTO_SLOTS: u8 = 5;

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
    /// Which preset the detail panel shows. Empty means the current default.
    pub focus: Option<String>,
    /// Card under the pointer; the Selected panel follows this when set.
    pub hover: Option<String>,
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
pub(super) fn shown_id(stored: &str) -> &str {
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
            // An explicit focus drops when adults-only turns off.
            if !adults {
                if let Some(id) = self.picker.focus.clone() {
                    if personality::lookup(&presets_dir(), &id).explicit {
                        self.picker.focus = None;
                    }
                }
            }
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
        let current = self
            .allowed_id(shown_id(self.cfg.personality.trim()))
            .to_string();
        let focus = focus_id(&self.picker.focus, &current, &presets);
        let tried = self
            .picker
            .shown
            .as_ref()
            .filter(|(id, _)| *id == focus)
            .map(|(_, state)| state.clone());
        let showcase_id = self
            .picker
            .hover
            .clone()
            .filter(|id| presets.iter().any(|p| p.id == *id))
            .unwrap_or(focus.clone());
        let showcased = personality::find(&presets, &showcase_id);
        let mut act_on: Option<CardAct> = None;
        let mut hover_id: Option<String> = None;
        let avail_raw = ui.available_width().max(1.0);
        ui.set_max_width(avail_raw);
        // Finite row height from the visible body clip — never rely on
        // ScrollArea's unbounded available_height for the two-column row.
        let clip = ui.clip_rect();
        let arect = ui.available_rect_before_wrap();
        // Prefer the real free rect width when it disagrees with available_width
        // (indent / ScrollArea), so absolute column rects match the cards' max_rect.
        let avail = avail_raw.min(arect.width()).max(1.0);
        let row_h = arect.intersect(clip).height().max(200.0);
        let cols_widths = two_col_widths(avail);
        let (grid_w, detail_w) = cols_widths.unwrap_or((avail, avail));
        let cols = grid_cols(grid_w);
        let card_w = ((grid_w - GRID_GAP * (cols.saturating_sub(1) as f32)) / cols as f32)
            .clamp(96.0, grid_w);
        let paint_grid =
            |ui: &mut egui::Ui, act_on: &mut Option<CardAct>, hover_id: &mut Option<String>| {
                ui.set_width(grid_w);
                ui.set_max_width(grid_w);
                for row in presets.chunks(cols) {
                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = GRID_GAP;
                        ui.set_max_width(grid_w);
                        for p in row {
                            let selected = p.id == focus;
                            let (act, hovered) = paint_grid_card(ui, p, selected, card_w);
                            if hovered {
                                *hover_id = Some(p.id.clone());
                            }
                            if let Some(act) = act {
                                *act_on = Some(act);
                            }
                        }
                    });
                    ui.add_space(GRID_GAP);
                }
            };
        let paint_detail = |ui: &mut egui::Ui, act_on: &mut Option<CardAct>| {
            if let Some(act) = paint_detail_panel(
                ui,
                &showcased,
                showcased.id == current,
                tried.as_ref(),
                detail_w,
            ) {
                *act_on = Some(act);
            }
        };
        if let Some((grid_w, detail_w)) = cols_widths {
            // Reserve the Selected panel on the right first via absolute rects so
            // an expanding grid cannot push TALK AS past the clip rect.
            let row = egui::Rect::from_min_size(arect.min, egui::vec2(avail, row_h));
            let detail_rect = egui::Rect::from_min_size(
                egui::pos2(row.max.x - detail_w, row.min.y),
                egui::vec2(detail_w, row_h),
            );
            let grid_rect = egui::Rect::from_min_size(
                row.min,
                egui::vec2((detail_rect.min.x - GRID_GAP - row.min.x).max(0.0), row_h),
            );
            ui.scope_builder(egui::UiBuilder::new().max_rect(grid_rect), |ui| {
                ui.set_clip_rect(grid_rect.intersect(clip));
                ui.set_max_size(grid_rect.size());
                egui::ScrollArea::vertical()
                    .id_salt("personality-grid")
                    .auto_shrink([false, false])
                    .max_height(row_h)
                    .show(ui, |ui| {
                        ui.set_width(grid_w);
                        ui.set_max_width(grid_w);
                        paint_grid(ui, &mut act_on, &mut hover_id);
                    });
            });
            ui.scope_builder(egui::UiBuilder::new().max_rect(detail_rect), |ui| {
                ui.set_clip_rect(detail_rect.intersect(clip));
                ui.set_max_size(detail_rect.size());
                egui::ScrollArea::vertical()
                    .id_salt("personality-detail")
                    .auto_shrink([false, false])
                    .max_height(row_h)
                    .show(ui, |ui| {
                        ui.set_width(detail_w);
                        ui.set_max_width(detail_w);
                        paint_detail(ui, &mut act_on);
                    });
            });
            ui.advance_cursor_after_rect(row);
        } else {
            paint_detail(ui, &mut act_on);
            ui.add_space(12.0);
            let grid_h = row_h.max(160.0);
            egui::ScrollArea::vertical()
                .id_salt("personality-grid-stack")
                .auto_shrink([false, false])
                .max_height(grid_h)
                .show(ui, |ui| {
                    ui.set_width(avail);
                    ui.set_max_width(avail);
                    paint_grid(ui, &mut act_on, &mut hover_id);
                });
        }
        self.picker.hover = hover_id;
        match act_on {
            Some(CardAct::Focus(id)) => self.picker.focus = Some(id),
            Some(CardAct::Talk(id)) => {
                self.picker.focus = Some(id.clone());
                self.set_default_personality(&id);
            }
            Some(CardAct::Try(id)) => self.try_personality(&id),
            Some(CardAct::Duplicate(id)) => self.duplicate_personality(&id),
            Some(CardAct::Edit(id)) => self.edit_personality(&id),
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
    Focus(String),
    Talk(String),
    Try(String),
    Duplicate(String),
    Edit(String),
}

/// How many grid columns fit in `width` (slide 19 is four-up when wide).
fn grid_cols(width: f32) -> usize {
    let n = ((width + GRID_GAP) / (CARD_MIN_W + GRID_GAP)).floor() as usize;
    n.clamp(1, 4)
}

/// Two-column widths for the picker row. `None` means stack (detail above grid).
/// Always keeps `grid_w + GRID_GAP + detail_w <= avail` so the Selected panel
/// cannot be pushed past the Settings clip rect.
fn two_col_widths(avail: f32) -> Option<(f32, f32)> {
    if avail < TWO_COL_MIN_W {
        return None;
    }
    let detail_w = DETAIL_W.min(avail * 0.38).clamp(200.0, DETAIL_W);
    let grid_w = (avail - detail_w - GRID_GAP).max(0.0);
    if grid_w < CARD_MIN_W {
        return None;
    }
    Some((grid_w, detail_w))
}

/// The detail panel's preset: a still-visible focus, else the current default.
fn focus_id(focus: &Option<String>, current: &str, presets: &[Personality]) -> String {
    if let Some(id) = focus.as_deref() {
        if presets.iter().any(|p| p.id == id) {
            return id.to_string();
        }
    }
    if presets.iter().any(|p| p.id == current) {
        current.to_string()
    } else {
        presets
            .first()
            .map(|p| p.id.clone())
            .unwrap_or_else(|| personality::DEFAULT_ID.to_string())
    }
}

/// Amber Talk as label on the detail panel (slide 19 / acceptance).
fn talk_label(name: &str) -> String {
    format!("TALK AS {}", name.trim().to_uppercase())
}

/// First letter of the display name for the card avatar.
fn initial_letter(name: &str) -> char {
    name.chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_ascii_uppercase())
        .unwrap_or('?')
}

fn paint_letter_tile(ui: &mut egui::Ui, letter: char, size: f32, hot: bool) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    let radius = (size * 0.27).clamp(6.0, 18.0);
    let fill = if hot {
        crate::theme::selected()
    } else {
        crate::theme::hover()
    };
    let stroke = if crate::theme::is_loadout_paint() {
        egui::Stroke::new(if hot { 2.0 } else { 1.0 }, crate::theme::signal_red())
    } else {
        egui::Stroke::new(1.0, crate::theme::border())
    };
    ui.painter().rect_filled(rect, radius, fill);
    ui.painter()
        .rect_stroke(rect, radius, stroke, egui::StrokeKind::Inside);
    let font = if size >= 48.0 {
        crate::theme::loadout_font(size * 0.42)
    } else {
        crate::theme::loadout_font(15.0)
    };
    let galley = ui.fonts_mut(|f| f.layout_no_wrap(letter.to_string(), font, crate::theme::fg()));
    let pos = egui::pos2(
        rect.center().x - galley.size().x * 0.5,
        rect.center().y - galley.size().y * 0.5,
    );
    ui.painter().galley(pos, galley, crate::theme::fg());
}

fn paint_tag_chip(ui: &mut egui::Ui, label: &str, kind: TagKind) {
    let (fill, stroke, ink) = match kind {
        TagKind::Default => (
            crate::theme::hover(),
            egui::Stroke::new(1.0, crate::theme::border()),
            crate::theme::muted(),
        ),
        TagKind::Roast => (
            egui::Color32::TRANSPARENT,
            egui::Stroke::new(1.0, crate::theme::signal_red()),
            crate::theme::signal_red(),
        ),
        TagKind::Explicit => (
            crate::theme::amber(),
            egui::Stroke::NONE,
            crate::theme::void_bg(),
        ),
        TagKind::Yours => (
            egui::Color32::TRANSPARENT,
            egui::Stroke::new(1.0, crate::theme::border()),
            crate::theme::muted(),
        ),
    };
    egui::Frame::NONE
        .fill(fill)
        .stroke(stroke)
        .corner_radius(6.0)
        .inner_margin(egui::Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(label).size(11.0).strong().color(ink));
        });
}

#[derive(Clone, Copy)]
enum TagKind {
    Default,
    Roast,
    Explicit,
    Yours,
}

fn paint_preset_tags(ui: &mut egui::Ui, p: &Personality, detail: bool) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        if p.id == personality::DEFAULT_ID {
            paint_tag_chip(ui, "Default", TagKind::Default);
        }
        if p.explicit {
            let label = if detail {
                "Explicit · 18+"
            } else {
                "Explicit"
            };
            paint_tag_chip(ui, label, TagKind::Explicit);
        }
        if p.roast {
            paint_tag_chip(ui, "Roast", TagKind::Roast);
        }
        if p.custom {
            paint_tag_chip(ui, "Yours", TagKind::Yours);
        }
    });
}

fn paint_grid_card(
    ui: &mut egui::Ui,
    p: &Personality,
    selected: bool,
    width: f32,
) -> (Option<CardAct>, bool) {
    let mut act = None;
    let loadout = crate::theme::is_loadout_paint();
    // Slide 19: only the selected card gets the red rim. Recipe Card::Default
    // also uses signal_red, which lit every grid cell at 1600×900.
    let (fill, stroke) = if loadout {
        if selected {
            let look = crate::ui_kit::recipe::look(
                crate::ui_kit::recipe::Part::Card,
                crate::ui_kit::recipe::State::Selected,
            );
            (look.fill, look.stroke)
        } else {
            (
                crate::theme::raised(),
                egui::Stroke::new(1.0, crate::theme::border()),
            )
        }
    } else if selected {
        (
            crate::theme::surface(),
            egui::Stroke::new(1.0, crate::theme::fg()),
        )
    } else {
        (
            crate::theme::surface(),
            egui::Stroke::new(1.0, crate::theme::border()),
        )
    };
    let inner_w = (width - 24.0).max(64.0);
    // Allocate a fixed-width top-down child: the grid row is horizontal_top, and
    // a plain scope/Frame inherits that so a wrapped description sits in a
    // 1-char leftover beside the title.
    let resp = ui
        .allocate_ui_with_layout(
            egui::vec2(width, ui.available_height().max(120.0)),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.set_width(width);
                ui.set_max_width(width);
                egui::Frame::NONE
                    .fill(fill)
                    .corner_radius(if loadout {
                        crate::ui_kit::recipe::CARD_RADIUS
                    } else {
                        12.0
                    })
                    .stroke(stroke)
                    .inner_margin(egui::Margin::symmetric(12, 10))
                    .show(ui, |ui| {
                        ui.set_width(inner_w);
                        ui.set_max_width(inner_w);
                        ui.set_min_height(88.0);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            paint_letter_tile(ui, initial_letter(&p.name), 28.0, selected);
                            ui.vertical(|ui| {
                                ui.set_max_width((inner_w - 36.0).max(40.0));
                                ui.label(
                                    RichText::new(&p.name)
                                        .size(crate::theme::FONT_BODY)
                                        .strong()
                                        .color(crate::theme::fg()),
                                );
                                paint_preset_tags(ui, p, false);
                            });
                        });
                        ui.add_space(8.0);
                        ui.add(
                            egui::Label::new(
                                RichText::new(&p.description)
                                    .size(crate::theme::FONT_META)
                                    .color(crate::theme::muted()),
                            )
                            .wrap(),
                        );
                    });
            },
        )
        .response
        .interact(egui::Sense::click());
    let resp = crate::theme::pointing(resp);
    let hovered = resp.hovered();
    if resp.clicked() {
        act = Some(CardAct::Focus(p.id.clone()));
    }
    (act, hovered)
}

fn paint_detail_panel(
    ui: &mut egui::Ui,
    p: &Personality,
    in_use: bool,
    tried: Option<&TryState>,
    width: f32,
) -> Option<CardAct> {
    let mut act = None;
    let loadout = crate::theme::is_loadout_paint();
    // Slide 19: Selected panel rim is signal red, not a per-preset accent.
    let (fill, stroke) = if loadout {
        (
            crate::theme::selected(),
            egui::Stroke::new(2.0, crate::theme::signal_red()),
        )
    } else {
        (
            crate::theme::surface(),
            egui::Stroke::new(1.5, crate::theme::fg()),
        )
    };
    let inner_w = (width - 32.0).max(140.0);
    egui::Frame::NONE
        .fill(fill)
        .corner_radius(if loadout {
            crate::ui_kit::recipe::CARD_RADIUS
        } else {
            12.0
        })
        .stroke(stroke)
        .inner_margin(egui::Margin::same(16))
        .show(ui, |ui| {
            ui.set_width(inner_w);
            ui.set_max_width(inner_w);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                super::loadout_home::diamond(ui, true);
                ui.label(
                    RichText::new("SELECTED")
                        .size(crate::theme::TYPE_CAPTION)
                        .extra_letter_spacing(1.2)
                        .color(crate::theme::amber()),
                );
            });
            ui.add_space(12.0);
            // Slide 19: big icon beside name + tags (no placeholder photo strip).
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                paint_letter_tile(ui, initial_letter(&p.name), DETAIL_ICON, true);
                ui.vertical(|ui| {
                    ui.set_max_width((inner_w - DETAIL_ICON - 12.0).max(80.0));
                    ui.label(
                        RichText::new(&p.name)
                            .font(crate::theme::loadout_font(22.0))
                            .strong()
                            .color(crate::theme::fg()),
                    );
                    ui.add_space(4.0);
                    paint_preset_tags(ui, p, true);
                });
            });
            ui.add_space(10.0);
            ui.label(
                RichText::new(&p.description)
                    .size(crate::theme::FONT_BODY)
                    .color(crate::theme::fg()),
            );
            if !p.sample.trim().is_empty() {
                ui.add_space(8.0);
                let sample_stroke = if loadout {
                    egui::Stroke::new(1.0, crate::theme::signal_red())
                } else {
                    egui::Stroke::new(1.0, crate::theme::border())
                };
                egui::Frame::NONE
                    .fill(crate::theme::void_bg())
                    .stroke(sample_stroke)
                    .corner_radius(10.0)
                    .inner_margin(egui::Margin::same(10))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.set_max_width(ui.available_width());
                        ui.label(
                            RichText::new(format!("\u{201c}{}\u{201d}", p.sample.trim()))
                                .size(crate::theme::FONT_META)
                                .italics()
                                .color(crate::theme::fg()),
                        );
                    });
            }
            if !loadout {
                match tried {
                    Some(TryState::Running) => {
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new("Trying…")
                                .size(crate::theme::FONT_META)
                                .color(crate::theme::muted()),
                        );
                    }
                    Some(TryState::Reply(text)) => {
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new(text)
                                .size(crate::theme::FONT_META)
                                .color(crate::theme::fg()),
                        );
                    }
                    Some(TryState::Failed(why)) => {
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new(format!("Try it failed: {why}"))
                                .size(crate::theme::FONT_META)
                                .color(crate::theme::offline()),
                        );
                    }
                    None => {}
                }
            }
            ui.add_space(12.0);
            if p.explicit {
                ui.label(
                    RichText::new("Adults only · opt-in")
                        .size(crate::theme::FONT_META)
                        .color(crate::theme::amber()),
                );
                ui.add_space(8.0);
            }
            let talk = talk_label(&p.name);
            let talk_w = ui.available_width();
            let talk_clicked = if loadout {
                crate::ui_kit::recipe::button(
                    ui,
                    Some(egui::Id::new(("personality-talk", p.id.as_str()))),
                    &talk,
                    true,
                    egui::vec2(talk_w, 36.0),
                    true,
                )
                .clicked()
            } else {
                crate::cards::white_pill(ui, &talk)
            };
            if talk_clicked && !in_use {
                act = Some(CardAct::Talk(p.id.clone()));
            }
            // Slide 19 has no Try it / Duplicate on the Selected card.
            if !loadout {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let busy = matches!(tried, Some(TryState::Running));
                    if !busy && crate::cards::ghost_pill(ui, "Try it") {
                        act = Some(CardAct::Try(p.id.clone()));
                    }
                    if p.custom {
                        if crate::cards::ghost_pill(ui, "Edit") {
                            act = Some(CardAct::Edit(p.id.clone()));
                        }
                    } else if crate::cards::ghost_pill(ui, "Duplicate") {
                        act = Some(CardAct::Duplicate(p.id.clone()));
                    }
                });
            }
        });
    act
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

    #[test]
    fn talk_label_is_uppercase_talk_as_name() {
        assert_eq!(talk_label("Viper"), "TALK AS VIPER");
        assert_eq!(
            talk_label(" Sarcastic Sidekick "),
            "TALK AS SARCASTIC SIDEKICK"
        );
    }

    #[test]
    fn grid_cols_cap_at_four_and_need_room_for_each() {
        assert_eq!(grid_cols(100.0), 1);
        assert_eq!(grid_cols(300.0), 2);
        assert_eq!(grid_cols(460.0), 3);
        assert_eq!(grid_cols(900.0), 4);
        assert_eq!(grid_cols(2000.0), 4);
    }

    #[test]
    fn two_col_widths_reserve_detail_inside_avail() {
        assert!(
            two_col_widths(639.0).is_none(),
            "below TWO_COL_MIN_W stacks"
        );
        for avail in [640.0_f32, 782.0, 1100.0, 1282.0, 1602.0] {
            let (grid_w, detail_w) = two_col_widths(avail).expect("side-by-side");
            assert!(
                grid_w + GRID_GAP + detail_w <= avail + 0.01,
                "detail must stay inside avail={avail}: grid={grid_w} detail={detail_w}"
            );
            assert!((200.0..=DETAIL_W).contains(&detail_w));
            assert!(grid_w >= CARD_MIN_W);
            assert_eq!(
                grid_w,
                avail - detail_w - GRID_GAP,
                "grid takes only what remains after detail"
            );
        }
    }

    #[test]
    fn focus_falls_back_when_the_id_is_hidden_or_missing() {
        let presets = personality::builtins();
        let visible = personality::visible(&presets, false);
        assert!(!visible.iter().any(|p| p.id == "viper"));
        assert_eq!(
            focus_id(&Some("viper".into()), "nyx", &visible),
            "nyx",
            "explicit focus drops when adults-only is off"
        );
        assert_eq!(focus_id(&None, "pirate", &visible), "pirate");
        assert_eq!(
            focus_id(&Some("no-such".into()), "butler", &visible),
            "butler"
        );
        assert_eq!(initial_letter("Viper"), 'V');
        assert_eq!(initial_letter("84s Hacker"), '8');
    }

    #[test]
    fn the_picker_source_keeps_slide_19_chrome() {
        let src = include_str!("personality_ui.rs");
        for want in [
            "SELECTED",
            "TALK AS",
            "Explicit · 18+",
            "Roast",
            "Adults only · opt-in",
            "paint_detail_panel",
            "paint_grid_card",
            "paint_letter_tile",
            "DETAIL_W",
            "DETAIL_ICON",
            "two_col_widths",
            "advance_cursor_after_rect",
        ] {
            assert!(src.contains(want), "missing {want}");
        }
        assert!(src.contains("CardAct::Talk"), "Talk as stays reachable");
        assert!(
            src.contains("CardAct::Try")
                && src.contains("CardAct::Duplicate")
                && src.contains("CardAct::Edit"),
            "Try it / Duplicate / Edit remain for non-Loadout"
        );
        assert!(
            src.contains("DETAIL_ICON") && src.contains("paint_letter_tile"),
            "Loadout slide 19 uses the letter icon on Selected, not a photo strip"
        );
        assert!(
            src.contains("max_rect(detail_rect)") && src.contains("max_rect(grid_rect)"),
            "side-by-side must reserve absolute column rects"
        );
        assert!(
            src.contains("Layout::top_down") && src.contains("allocate_ui_with_layout"),
            "grid cards must force top-down so descriptions do not crush"
        );
        assert!(
            src.contains("only the selected card gets the red rim") || src.contains("signal_red"),
            "Selected panel / selected grid card use signal red"
        );
        assert!(
            src.contains("if !loadout") && src.contains("Try it"),
            "Try it / Duplicate stay off the Loadout Selected card"
        );
    }

    /// Deck sizes must keep the Selected panel (and amber TALK AS) on-screen.
    /// Also guards the 1100×720 crush where horizontal_top leaked into cards and
    /// wrapped descriptions into a 1-character column.
    #[test]
    fn personality_detail_panel_stays_on_screen_at_deck_sizes() {
        let _paint = crate::theme::hold_paint_test();
        crate::theme::set_paint_palette(crate::theme::PaintPalette::OpsDark);
        let (_lock, _root, mut cabin) = cabin_with_chat();
        cabin.cfg.explicit_personalities = true;
        cabin.cfg.personality = "viper".into();
        cabin.nav = Nav::Settings;
        cabin.settings_sec = SettingsSec::Personality;
        for (w, h) in [
            (900.0_f32, 720.0_f32),
            (1100.0, 720.0),
            (1600.0, 900.0),
            (1920.0, 1080.0),
        ] {
            let ctx = egui::Context::default();
            crate::theme::install_fonts_on(&ctx);
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(w, h),
                )),
                ..Default::default()
            };
            let mut texts: Vec<(String, egui::Pos2, egui::Rect, egui::Vec2)> = Vec::new();
            for _ in 0..2 {
                texts.clear();
                let shapes = crate::theme::test_pass(&ctx, raw.clone(), |ui| {
                    cabin.ui_settings(ui.ctx());
                })
                .shapes;
                for clipped in &shapes {
                    if let egui::Shape::Text(t) = &clipped.shape {
                        texts.push((
                            t.galley.job.text.clone(),
                            t.pos,
                            clipped.clip_rect,
                            t.galley.size(),
                        ));
                    }
                }
            }
            let selected = texts.iter().find(|(t, _, _, _)| t == "SELECTED");
            let talk = texts.iter().find(|(t, _, _, _)| t.starts_with("TALK AS "));
            let (sel_text, sel_pos, sel_clip, _) =
                selected.unwrap_or_else(|| panic!("SELECTED missing at {w}x{h}; texts={texts:?}"));
            assert_eq!(sel_text, "SELECTED");
            assert!(
                sel_clip.contains(*sel_pos) && sel_pos.x >= 0.0 && sel_pos.x < w,
                "SELECTED on-screen at {w}x{h}: pos={sel_pos:?} clip={sel_clip:?}"
            );
            let (talk_text, talk_pos, talk_clip, _) =
                talk.unwrap_or_else(|| panic!("TALK AS missing at {w}x{h}"));
            assert!(
                talk_text.starts_with("TALK AS "),
                "unexpected talk label {talk_text}"
            );
            assert!(
                talk_clip.contains(*talk_pos)
                    && talk_pos.x >= 0.0
                    && talk_pos.x < w
                    && talk_pos.y + 20.0 <= h,
                "TALK AS fully on-screen at {w}x{h}: pos={talk_pos:?} clip={talk_clip:?}"
            );
            // Side-by-side decks keep Selected on the right; narrow 900 stacks it on top.
            if w >= 1100.0 {
                assert!(
                    sel_pos.x > w * 0.45 && talk_pos.x > w * 0.45,
                    "Selected/TALK AS stay in the right half when side-by-side at {w}x{h}"
                );
            }
            // Side-by-side decks paint the grid in-frame; Nyx must not crush to
            // a ~1-character wrap column (900 stacks detail above and may clip the grid).
            if w >= 1100.0 {
                let nyx = texts.iter().find(|(t, _, _, _)| {
                    t.starts_with("Nerdy gamer") || t.contains("Nerdy gamer and techie")
                });
                let (_, pos, _, size) =
                    nyx.unwrap_or_else(|| panic!("Nyx description missing at {w}x{h}"));
                assert!(
                    size.x >= 80.0,
                    "Nyx description crushed at {w}x{h}: size={size:?} pos={pos:?}"
                );
            }
        }
    }

    #[test]
    fn personality_photos_are_optional_until_finals_land() {
        // Flat placeholder tiles were removed; build embeds only real files.
        assert_eq!(
            PERSONALITY_PHOTO_COUNT, 0,
            "do not ship 64×64 placeholder tiles; drop finals under assets/personalities/<id>/"
        );
        assert!(personality_photo_png("nyx", 1).is_none());
        assert!(personality_photo_png("viper", 1).is_none());
        assert!(personality_photo_png("no-such-preset", 1).is_none());
    }

    #[test]
    fn settings_pins_personality_for_deck_shots() {
        let src = include_str!("settings.rs");
        assert!(src.contains("GROKHUB_DECK_SETTINGS_SEC"));
        assert!(src.contains("pinned_settings_sec"));
        assert!(src.contains("settings_modal_size"));
        assert!(src.contains("settings_page_rect"));
        let shots = include_str!("../../../../scripts/deck-shots.ps1");
        assert!(shots.contains("-pinPersonality") || shots.contains("-personality"));
        assert!(shots.contains("personality-1600x900"));
        assert!(shots.contains("personality-1100x720"));
        assert!(shots.contains("personality-1920x1080"));
        assert!(shots.contains("personality-900x720"));
        assert!(shots.contains("slide-19") || shots.contains("'19'"));
        // Slide 19 must not rely on a rail click (missed → Hangout in #958).
        assert!(
            shots.contains("GROKHUB_DECK_SETTINGS_SEC opens Settings at")
                || !shots
                    .contains("personality-1600x900' 'ops-dark' '19' -pinPersonality -clickAt"),
            "personality slide 19 shots open Settings via env, not clickAt"
        );
    }
}
