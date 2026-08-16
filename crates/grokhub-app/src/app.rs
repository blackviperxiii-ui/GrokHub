use crate::helpers::{
    cabin_menu_should_dismiss, click_project_opens_board, collect_other_chip_threads, expand_home,
    next_maximized, next_starter_skill_name, wants_live_repaint,
};
use crate::titlebar::{
    apply_tray_window, titlebar_chrome_btn, titlebar_chrome_hit,
};
use crate::config::{self, AppConfig};
use crate::desktop::{
    capture_data_url, capture_webcam, clipboard_image, collect_rows, first_bin, load_image_data_url,
    pick_file, play_audio, read_text_capped, record_once, run_computer_op, run_computer_op_cancel,
    transcribe_local,
};
use crate::host::{run_host, run_host_stream};
use crate::secrets::{self, Secrets};
use crate::skills;
use crate::threads::{self, ChatThread};
use crate::update::{remember_source, resolve_source};
use crate::xai::{grok_chat, grok_chat_stream, grok_imagine, grok_stt, grok_tts, http_status_of};
use eframe::egui::{self, Color32, ColorImage, RichText, TextureHandle, TextureOptions};
use grokhub_core::{
    append_composer, anticipated_need, apply_work_update, attach_kind, attach_name, attach_prompt_line,
    cabin_system_prompt,
    appearance_choices, appearance_hint, approved_cmds, auth_bearer, blend_thread_goal,
    build_hub_snapshot,
    build_quick_chips, build_windshield, bump_skill_run, bump_usage, can_inhabit,
    catalog_line, chip_suggest_prompt, compact_keep_pin, compose_imagine_prompt,
    context_fingerprint,
    context_percent,
    dedicated_imagine_model, dedicated_voice_model, default_openclaw_paths, diagnostics_bundle,
    pick_fresh_seed, wall_can_paint, wall_evict, ImagineKind, ImagineSpec, ImagineWall,
    WallGif, WALL_GIF_EVERY_MS, WALL_GIF_MAX,
    imagine_shows_result_above, imagine_toolbox_dock, imagine_toolbox_shows_title,
    imagine_toolbox_top, imagine_wall_bounds,
    doctor_hands_line, due_automations, ensure_automation_schedule, estimate_messages, extract_connector_cmds,
    night_check_command, night_check_exit_code, skip_night_check_receipt,
    extract_imagine_prompt, extract_work_pins, filter_palette, format_consult_reply,
    imagine_aspect_label, imagine_aspect_name, imagine_style_label, imagine_video_dur_label,
    imagine_video_res_label, last_imagine_receipt,
    extract_insights, extract_work_updates, fact_candidates, failover_model, filter_slash_commands,
    frame_bytes, PresenceFrame,
    forget_topic, greet_from_last_job, has_auth, has_verify_ok, hey_grok_on_press,
    hey_grok_route, hey_grok_starts_ptt, import_memory_file, insight_pin, is_openclaw_workspace,
    add_to_folder, create_folder, create_project, drop_node, drop_selected, folder_choices,
    host_status_line, is_hard_run,
    project_menu_acts, project_menu_label, rename_node, restore_bound_path, seed_from_bound,
    settle_project_path, should_seed_sidebar, stage_project, toggle_folder, upsert_bound,
    visible_tree, ProjectKind, ProjectMenuAct,
    ProjectNode,
    is_plain_text, is_voice_error, keep_last_rewinds, last_user_text, load_hub_state, mark_automation_ran,
    match_skill, mode_from_chip_value, model_for_mode, nav_from_chip_value,
    cabin_eyes_request_text, cabin_frame_only, chat_attach_status, imagine_ref_status,
    needs_auth_banner, next_chat_image, next_goal_prompt,
    is_workload_user, merge_thinking, strip_thinking, visible_chat, ChatKind, ChatView,
    apply_stream_snapshot, chat_send_kind, chat_shows_thinking, chat_stream_is_visible, ChatSendKind,
    bubble_outer_width, bubble_wrap_width, clamp_row_width, BUBBLE_PAD_X, BUBBLE_PAD_Y,
    BUBBLE_RADIUS,
    plus_empty_status, plus_menu_rows, computer_cmd_line, hands_protocol, lock_blocks_hands,
    parse_computer_cmd_loose, should_attach_hands_frame, user_asks_cabin_eyes,
    user_asks_desktop_hands,
    resolve_chat_model, resolve_dark, effective_chat_mode, settings_pin_blocks_auto, parse_fast_topics,
    now_ms, parse_consult, parse_goal_outcome, parse_local_clock, patch_skill, prefer_patch,
    reply_needs_followup,
    recipe_from_cmds, replay_automation_target,
    parse_nl_automation, parse_recipe, parse_slash, parse_theme, pick_theme, plan_from_text, plan_room,
    presence_should_stream, propose_skill_from_turn, quiet_hours_active,
    parse_llm_chips, record_turn, reduce_voice_state, remember_chip_click, remember_chip_dismiss,
    remember_chip_outcome, remember_typed_prompt, roll_usage_day,
    greeting_fingerprint, greeting_prompt, local_greeting, pick_greeting,
    should_paint_greeting, should_refresh_greeting, GreetingInput, GREETING_LLM_MODE,
    recall_hits, redirect_prompt, redact_secrets, refused_lock, replay_ops, rewind_allowed,
    rewind_dest, save_hub_state, screen_from_extents, search_corpus,
    should_anticipate, should_auto_compact, should_keep_frame, should_refresh_llm, shortcut_help,
    user_asks_takeover, windshield_prompt,
    composer_enter, composer_go, composer_go_tip, ComposerEnter, ComposerGo,
    heartbeat_acts, heartbeat_due, heartbeat_repaint_ms, next_heartbeat_wait_ms, HeartbeatAct,
    HEARTBEAT_MS,
    build_review_digest, dedupe_suggestions, parse_suggest_lines, partition_suggestions, review_due,
    review_status_line, review_system_prompt, DigestLine, ReviewDigest, SuggestionStore, REVIEW_NIGHT_HOUR,
    should_capture_before_chat, should_failover_status, should_idle_reflect, should_send_screenshot,
    apply_auto_title, apply_manual_rename, delete_thread, display_tab_title, history_order,
    should_name_thread,
    skill_follow_block, slash_help, summarize_write, surgical_memory_edit,
    thread_goal_prompt, theme_id, theme_label, toggle_pin, DeleteOutcome, ThreadTab,
    top_habit_labels,
    unified_diff_cite, usage_line,
    transcribe_route, uid, update_cmds, overlay_update_begin, overlay_update_finish,
    realtime_bearer, realtime_can_connect, voice_log_role, voice_stream_token, voice_transcript_sends_chat,
    fold_stream_token, StreamTokenKind,
    update_wipes_config, voice_session_url, Automation, BoardCard,
    BoardStatus, ChipInput, ChipKind, ChipMemory, ChipThread, ComputerOp, DeviceCodeStart, HeyGrokAction,
    HeyGrokRoute, HubMemoryFile, QuickChip,
    HubSnapshot, HubState, InhabitBundle, LearningState, LocalClock, MintRealtimeFn, Policy, Recipe, ReplayOp, RewindRecord,
    AttachKind, PlusAct, PlusTarget, SkillMd, Slash, ThemeChoice, TranscribeRoute, UsageDay, VoiceEvent,
    VoiceState, CONTEXT_BUDGET_TOKENS, CHIP_LLM_MODE, CHIP_VISIBLE_MAX,
    user_pref_facts,
    DEFAULT_MODEL, FOLLOWUP_MAX_STEPS, FOLLOWUP_PROMPT, GOAL_DROP_AFTER, GOAL_MAX_STEPS, HUB_KIND,
    IDLE_REFLECT_MS, IMAGINE_ASPECTS,
    IMAGINE_STYLES,
    PRESENCE_RING_MS, TRANSCRIBERS,
};
use grokhub_hub::serve_lan;
use global_hotkey::{
    hotkey::{Code, HotKey, Modifiers},
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Nav {
    Chat,
    Devices,
    Memory,
    Workboard,
    Imagine,
    Skills,
    Eyes,
    Night,
    History,
    Command,
    Connectors,
    Agents,
    Settings,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SettingsSec {
    Account,
    Appearance,
    Behavior,
    Host,
    Imagine,
    Voice,
    Night,
    Github,
    Update,
    About,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SettingsGroup {
    General,
    Cabin,
    Data,
    About,
}

fn settings_group_home(group: SettingsGroup) -> SettingsSec {
    match group {
        SettingsGroup::General => SettingsSec::Account,
        SettingsGroup::Cabin => SettingsSec::Host,
        SettingsGroup::Data => SettingsSec::Github,
        SettingsGroup::About => SettingsSec::Update,
    }
}

fn slash_pick_step(pick: usize, len: usize, dir: i8) -> usize {
    if len == 0 {
        return 0;
    }
    let clamped = pick.min(len - 1);
    match dir {
        1 => (clamped + 1).min(len - 1),
        -1 => clamped.saturating_sub(1),
        _ => clamped,
    }
}

/// Tab / click accept. `Some` means run the command this frame.
fn slash_pick_take(composer: &mut String, insert: &str, run_on_pick: bool) -> Option<String> {
    *composer = insert.to_string();
    if run_on_pick {
        Some(std::mem::take(composer))
    } else {
        None
    }
}

fn slash_pick_retain(pick: usize, list_changed: bool, len: usize) -> usize {
    if list_changed || len == 0 {
        0
    } else {
        slash_pick_step(pick, len, 0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ComposerStackSlot {
    AuthBanner,
    SlashPalette,
    Chips,
    Attach,
    Pill,
}

fn composer_stack_order() -> &'static [ComposerStackSlot] {
    &[
        ComposerStackSlot::AuthBanner,
        ComposerStackSlot::SlashPalette,
        ComposerStackSlot::Chips,
        ComposerStackSlot::Attach,
        ComposerStackSlot::Pill,
    ]
}

fn consume_enter_keys(ui: &mut egui::Ui) {
    ui.input_mut(|i| {
        i.events.retain(|ev| match ev {
            egui::Event::Key {
                key: egui::Key::Enter,
                ..
            } => false,
            egui::Event::Text(t) if t == "\n" || t == "\r" || t == "\r\n" => false,
            _ => true,
        });
    });
}

/// Enter sends. Control+Enter is left for TextEdit (`return_key`) to insert a newline.
fn take_focused_composer(
    ui: &mut egui::Ui,
    composer: &mut String,
    focused: bool,
) -> Option<String> {
    if !focused {
        return None;
    }
    let (enter, control) = ui.input(|i| {
        (
            i.key_pressed(egui::Key::Enter),
            i.modifiers.ctrl || i.modifiers.command,
        )
    });
    match composer_enter(enter, control) {
        Some(ComposerEnter::Send) => {
            consume_enter_keys(ui);
            if composer.ends_with('\n') {
                composer.pop();
            }
            Some(std::mem::take(composer))
        }
        Some(ComposerEnter::Newline) => None,
        None => None,
    }
}

const HIDDEN_HEARTBEAT_MS: u64 = 400;

fn night_host_check_blocks_ui() -> bool {
    false
}

fn mode_status_line(mode: &str, pinned_model: &str) -> String {
    if matches!(mode, "auto" | "adaptive" | "smart") && !settings_pin_blocks_auto(pinned_model) {
        return "Mode auto — routes Fast / Balance / Think / Max".into();
    }
    let model = resolve_chat_model(mode, pinned_model);
    match grokhub_core::reasoning_effort_for_mode(mode) {
        Some(effort) => format!("Mode {mode} → {model} · {effort}"),
        None => format!("Mode {mode} → {model}"),
    }
}

const RAIL_FOOTER_H: f32 = 52.0;
const PALETTE_LIST_H: f32 = 280.0;

struct OauthPhotoOut {
    tokens: Option<grokhub_core::XaiOAuthTokens>,
    url: String,
    bytes: Option<Vec<u8>>,
}

fn oauth_photo_image(bytes: &[u8]) -> Option<ColorImage> {
    let rgba = crate::oauth::avatar_rgba(bytes)?;
    let size = [rgba.width() as usize, rgba.height() as usize];
    Some(ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()))
}

fn settings_sec_title(sec: SettingsSec) -> &'static str {
    match sec {
        SettingsSec::Account => "Account",
        SettingsSec::Appearance => "Appearance",
        SettingsSec::Behavior => "Behavior",
        SettingsSec::Host => "Host",
        SettingsSec::Imagine => "Imagine",
        SettingsSec::Voice => "Voice",
        SettingsSec::Night => "Night",
        SettingsSec::Github => "GitHub",
        SettingsSec::Update => "Update",
        SettingsSec::About => "About",
    }
}

struct Msg {
    role: String,
    content: String,
}

#[derive(Default)]
struct ImagineBarOut {
    generate: bool,
    stop: bool,
    go_settings: bool,
}

fn imagine_popup(
    ctx: &egui::Context,
    id: &'static str,
    anchor: egui::Rect,
    rows: &[(String, bool)],
) -> (Option<usize>, egui::Rect) {
    let mut picked = None;
    let mut menu_rect = egui::Rect::NOTHING;
    egui::Area::new(egui::Id::new(id))
        .fixed_pos(anchor.left_bottom() + egui::vec2(0.0, 6.0))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(anchor.width().max(168.0));
                ui.spacing_mut().item_spacing.y = 2.0;
                for (i, (label, on)) in rows.iter().enumerate() {
                    if ui.selectable_label(*on, label).clicked() {
                        picked = Some(i);
                    }
                }
                menu_rect = ui.min_rect();
            });
        });
    (picked, menu_rect)
}

enum TabAct {
    Switch(usize),
    Pin(usize),
    StartRename(usize),
    CommitRename(usize),
    CancelRename,
    Delete(usize),
}

enum JobOut {
    Chat { text: String, truncated: bool },
    ChatDelta(String),
    ThoughtDelta(String),
    Imagine(String),
    Voice(String),
    HostLine(String),
    HostDone(String),
    UpdateProgress { pct: u8, msg: String },
    UpdateDone { ok: bool },
    Connector(String),
    Consult(String),
    Err(String),
}

struct AgentJob {
    title: String,
    status: String,
    prompt: String,
}

fn listen_turn(api_key: &str) -> String {
    let wav = match record_once() {
        Ok(p) => p,
        Err(e) => return format!("VOICE_RECEIPT: {e}"),
    };
    let has_local = first_bin(TRANSCRIBERS).is_some();
    match transcribe_route(!api_key.trim().is_empty(), has_local) {
        TranscribeRoute::Xai => match std::fs::read(&wav) {
            Ok(bytes) => match grok_stt(api_key, &bytes) {
                Ok(t) => t,
                Err(e) => transcribe_local(&wav).unwrap_or_else(|local| {
                    format!("VOICE_RECEIPT: {e}; {local}")
                }),
            },
            Err(e) => format!("VOICE_RECEIPT: {e}"),
        },
        TranscribeRoute::Local => match transcribe_local(&wav) {
            Ok(t) if !t.trim().is_empty() => t.trim().to_string(),
            Ok(_) => "VOICE_RECEIPT: empty transcript".into(),
            Err(e) => format!("VOICE_RECEIPT: {e}"),
        },
        TranscribeRoute::None => {
            "VOICE_RECEIPT: Connect Grok OAuth for STT, or install whisper".into()
        }
    }
}

fn fit_rail_label(ui: &egui::Ui, label: &str, max_w: f32) -> String {
    let font = egui::FontId::proportional(crate::theme::FONT_CHROME);
    let fits = |s: &str| {
        ui.fonts(|f| f.layout_no_wrap(s.to_owned(), font.clone(), egui::Color32::WHITE))
            .size()
            .x
            <= max_w
    };
    if fits(label) {
        return label.to_string();
    }
    let mut t = label.to_string();
    while t.pop().is_some() {
        let candidate = format!("{}…", t.trim_end());
        if fits(&candidate) {
            return candidate;
        }
    }
    "…".into()
}

fn paint_speech_bubble(ui: &mut egui::Ui, body: &str, user: bool, markdown: bool) -> egui::Response {
    let avail = clamp_row_width(ui.available_width().min(ui.max_rect().width()));
    let wrap = bubble_wrap_width(avail, BUBBLE_PAD_X);
    let content = crate::markdown::measure_text(ui, body, wrap);
    let inner_w = content.x.max(1.0).min(wrap);
    let outer_w = bubble_outer_width(avail, inner_w, BUBBLE_PAD_X);
    let fill = if user {
        crate::theme::bubble_user()
    } else {
        crate::theme::bubble_assistant()
    };
    let frame = egui::Frame::none()
        .fill(fill)
        .rounding(BUBBLE_RADIUS)
        .inner_margin(egui::Margin::symmetric(BUBBLE_PAD_X, BUBBLE_PAD_Y));
    let mut resp = None;
    let paint = |ui: &mut egui::Ui| {
        ui.set_max_width(outer_w);
        ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
            ui.set_max_width(outer_w);
            resp = Some(
                frame
                    .show(ui, |ui| {
                        ui.set_width(inner_w);
                        ui.set_max_width(inner_w);
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                        if markdown {
                            crate::markdown::show(ui, body);
                        } else {
                            ui.add(
                                egui::Label::new(RichText::new(body).color(crate::theme::fg()))
                                    .wrap(),
                            );
                        }
                    })
                    .response,
            );
        });
    };
    if user {
        ui.with_layout(egui::Layout::right_to_left(egui::Align::TOP), paint);
    } else {
        ui.with_layout(egui::Layout::left_to_right(egui::Align::TOP), paint);
    }
    resp.expect("speech bubble")
}

fn paint_chat_block(ui: &mut egui::Ui, block: &ChatView, _idx: usize, thought_open: bool) {
    let bubble_w = crate::markdown::bubble_width(ui.available_width());
    match block.kind {
        ChatKind::User => {
            let _ = paint_speech_bubble(ui, &block.body, true, false);
        }
        ChatKind::Assistant => {
            let _ = paint_speech_bubble(ui, &block.body, false, true);
        }
        ChatKind::Thought => {
            let open = thought_open;
            egui::Frame::none()
                .fill(crate::theme::elevated())
                .rounding(10.0)
                .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                .show(ui, |ui| {
                    ui.set_max_width(bubble_w);
                    ui.label(
                        RichText::new("Thought")
                            .size(crate::theme::FONT_META)
                            .color(crate::theme::muted()),
                    );
                    if open || !block.body.is_empty() {
                        ui.add_space(4.0);
                        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                        ui.add(
                            egui::Label::new(
                                RichText::new(&block.body)
                                    .size(crate::theme::FONT_META)
                                    .color(crate::theme::subtle()),
                            )
                            .wrap(),
                        );
                    }
                });
        }
        ChatKind::Tool => {
            egui::Frame::none()
                .fill(crate::theme::panel())
                .rounding(10.0)
                .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                .show(ui, |ui| {
                    ui.set_max_width(bubble_w);
                    ui.label(
                        RichText::new(&block.title)
                            .size(crate::theme::FONT_META)
                            .color(crate::theme::muted()),
                    );
                    ui.add_space(4.0);
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
                    ui.add(
                        egui::Label::new(
                            RichText::new(&block.body)
                                .size(crate::theme::FONT_META)
                                .monospace()
                                .color(crate::theme::fg()),
                        )
                        .wrap(),
                    );
                });
        }
    }
}

fn screen_from_rows(rows: &[grokhub_core::AtspiRow]) -> Option<grokhub_core::ScreenSize> {
    let mut mx = 0;
    let mut my = 0;
    for r in rows {
        mx = mx.max(r.x + r.w);
        my = my.max(r.y + r.h);
    }
    screen_from_extents(mx, my)
}

pub struct Cabin {
    nav: Nav,
    cfg: AppConfig,
    composer: String,
    messages: Vec<Msg>,
    status: String,
    running: bool,
    host_halt: Arc<AtomicBool>,
    rx: Option<mpsc::Receiver<JobOut>>,
    chat_job_thread: Option<String>,
    hub: Arc<Mutex<HubState>>,
    hub_on: bool,
    hub_port: u16,
    task_prompt: String,
    mem_name: String,
    mem_body: String,
    last_persist: Instant,
    board: Vec<BoardCard>,
    board_title: String,
    imagine_prompt: String,
    imagine_last: String,
    skill_name: String,
    skill_body: String,
    skill_list: Vec<SkillMd>,
    eyes_text: String,
    last_host: Vec<String>,
    last_frame_url: Option<String>,
    hands_attach: bool,
    eyes_attach: bool,
    speak_next: bool,
    verify_ok_turn: bool,
    verify_chip: String,
    reflect_diff: String,
    last_activity: Instant,
    reflected_idle: bool,
    last_recipe: Option<Recipe>,
    pending_update: bool,
    update_pct: Option<u8>,
    update_can_restart: bool,
    secrets: Secrets,
    threads: Vec<ChatThread>,
    thread_idx: usize,
    oauth_pending: Option<DeviceCodeStart>,
    tray: Option<crate::tray::TrayHost>,
    tray_rx: Option<mpsc::Receiver<Option<crate::tray::TrayHost>>>,
    window_visible: bool,
    want_quit: bool,
    told_tray: bool,
    automations: Vec<Automation>,
    night_nl: String,
    history_q: String,
    history_hits: Vec<String>,
    last_receipt_ok: Option<bool>,
    last_receipts: Vec<(String, bool)>,
    last_rewind_id: Option<String>,
    rewind_rows: Vec<RewindRecord>,
    host_live: String,
    daily_auto_used: u32,
    daily_auto_day: String,
    slash_pick: usize,
    slash_filter_n: usize,
    slash_filter_first: &'static str,
    last_window_title: String,
    voice_orb: String,
    last_night_tick: Instant,
    last_heartbeat: Instant,
    night_check_rx: Option<(String, mpsc::Receiver<(String, i32)>)>,
    learning: LearningState,
    suggestions: SuggestionStore,
    review_rx: Option<mpsc::Receiver<Result<String, String>>>,
    review_busy: bool,
    usage: UsageDay,
    palette_open: bool,
    palette_q: String,
    palette_pick: usize,
    palette_focus: bool,
    shortcuts_open: bool,
    active_skill_follow: Option<String>,
    last_anticipate_ms: u64,
    goal_step: u32,
    followup_step: u32,
    stream_buf: String,
    thought_buf: String,
    presence_ring: Vec<(u64, String)>,
    webcam_url: Option<String>,
    voice_sock: Option<crate::voice_ws::VoiceSock>,
    voice_state: VoiceState,
    cmd_line: String,
    cmd_hist: Vec<String>,
    agents: Vec<AgentJob>,
    last_live: Instant,
    #[allow(dead_code)]
    hotkeys: Option<GlobalHotKeyManager>,
    hotkey_hey: u32,
    hotkey_halt: u32,
    tools_collapsed: bool,
    sidebar_q: String,
    rename_idx: Option<usize>,
    rename_buf: String,
    rename_focus: bool,
    rename_lock: Option<String>,
    chip_memory: ChipMemory,
    chip_dismissed: Vec<String>,
    llm_chips: Vec<QuickChip>,
    visible_chips: Vec<QuickChip>,
    chip_rx: Option<mpsc::Receiver<Vec<QuickChip>>>,
    chip_busy: bool,
    chip_fp: String,
    chip_llm_at: u64,
    greeting: String,
    greeting_fp: String,
    greeting_llm_fp: String,
    greeting_rx: Option<mpsc::Receiver<String>>,
    greeting_busy: bool,
    greeting_llm_at: u64,
    continue_hint: String,
    skills_tab_connectors: bool,
    skill_q: String,
    github_args: String,
    pending_connectors: Vec<(String, String, String)>,
    auto_compose: bool,
    board_compose: bool,
    settings_menu_open: bool,
    settings_menu_ignore: bool,
    win_max: bool,
    geom_dirty: bool,
    imagine_want_focus: bool,
    settings_sec: SettingsSec,
    settings_back: Nav,
    imagine_aspect: u8,
    imagine_quality: bool,
    imagine_kind: ImagineKind,
    imagine_style: u8,
    imagine_video_res: u8,
    imagine_video_dur: u8,
    imagine_video_audio: bool,
    imagine_aspect_open: bool,
    imagine_style_open: bool,
    imagine_menu_ignore: bool,
    imagine_style_anchor: egui::Rect,
    imagine_aspect_anchor: egui::Rect,
    goal_rx: Option<mpsc::Receiver<(String, String)>>,
    goal_busy: bool,
    goal_stale: bool,
    wall: ImagineWall,
    wall_rx: Option<mpsc::Receiver<Result<WallGif, String>>>,
    wall_busy: bool,
    attach_url: Option<String>,
    attach_name: Option<String>,
    imagine_ref: Option<String>,
    plus_menu: Option<PlusTarget>,
    plus_anchor: egui::Pos2,
    plus_ignore_close: bool,
    file_pick: Option<PlusTarget>,
    pick_dir: String,
    projects: Vec<ProjectNode>,
    project_sel: Option<String>,
    proj_menu_pos: egui::Pos2,
    proj_plus_open: bool,
    proj_plus_pos: egui::Pos2,
    proj_add_for: Option<String>,
    proj_rename: Option<String>,
    proj_rename_buf: String,
    proj_rename_focus: bool,
    proj_rename_lock: Option<String>,
    proj_staged: Option<String>,
    proj_ignore_close: bool,
    projects_dirty: bool,
    oauth_photo: Option<TextureHandle>,
    oauth_photo_key: String,
    oauth_photo_rx: Option<mpsc::Receiver<OauthPhotoOut>>,
    oauth_photo_busy: bool,
    oauth_profile_tried: bool,
}

fn paint_wall_cover(
    key: &str,
    model: &str,
    id: &str,
    dir: &std::path::Path,
    title: &str,
    prompt: &str,
    prompt_b: &str,
    tall: bool,
    created_ms: u64,
) -> Result<WallGif, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let src_a = grok_imagine(key, model, prompt)?;
    let path_a = dir.join(format!("{id}_a.png"));
    std::fs::copy(&src_a, &path_a).map_err(|e| e.to_string())?;
    let path_b = dir.join(format!("{id}_b.png"));
    match grok_imagine(key, model, prompt_b) {
        Ok(src_b) => {
            if std::fs::copy(&src_b, &path_b).is_err() {
                crate::desktop::sibling_still(&path_a, &path_b)?;
            }
        }
        Err(_) => crate::desktop::sibling_still(&path_a, &path_b)?,
    }
    if !path_b.exists() {
        crate::desktop::sibling_still(&path_a, &path_b)?;
    }
    Ok(WallGif {
        id: id.into(),
        title: title.into(),
        prompt: prompt.into(),
        created_ms,
        path_a: path_a.display().to_string(),
        path_b: path_b.display().to_string(),
        tall,
    })
}

impl Cabin {
    pub fn new(hidden: bool) -> Self {
        let cfg = config::load();
        let mut hub = load_hub_state(&config::hub_state_path()).unwrap_or_else(HubState::empty);
        if !cfg.device_name.trim().is_empty() {
            hub.device_name = cfg.device_name.clone();
        }
        let mem_name = "SOUL.md".to_string();
        let mem_body = config::read_memory(&mem_name);
        let mut threads = threads::load();
        if threads.is_empty() {
            let mut t = ChatThread::new("Chat", false);
            t.messages = config::load_chat();
            threads.push(t);
        }
        let thread_idx = threads
            .iter()
            .position(|t| t.id == cfg.current_thread)
            .unwrap_or(0);
        let messages: Vec<Msg> = threads
            .get(thread_idx)
            .map(|t| {
                t.messages
                    .iter()
                    .map(|(role, content)| Msg {
                        role: role.clone(),
                        content: content.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let imagine_last = last_imagine_receipt(messages.iter().map(|m| m.content.as_str()))
            .unwrap_or_default();
        let mut cfg = cfg;
        if cfg.source_dir.trim().is_empty() {
            if let Some(src) = resolve_source("") {
                remember_source(&src);
                cfg.source_dir = src.display().to_string();
            }
        }
        let mut projects = crate::store::load_projects();
        let sidebar_file = crate::store::projects_path().exists();
        let work = std::env::var("HOME")
            .ok()
            .map(|home| format!("{home}/GrokHub-Work"))
            .unwrap_or_default();
        cfg.project_dir = restore_bound_path(&cfg.project_dir, &work, sidebar_file);
        if should_seed_sidebar(sidebar_file, &projects) {
            projects = seed_from_bound(&cfg.project_dir);
        }
        let project_sel = projects
            .iter()
            .find(|n| n.kind == ProjectKind::Project && n.path == cfg.project_dir)
            .or_else(|| projects.iter().find(|n| n.kind == ProjectKind::Project))
            .map(|n| n.id.clone());
        let secrets = secrets::load();
        let win_max = cfg.window.maximized;
        let mut c = Self {
            nav: Nav::Chat,
            cfg,
            composer: String::new(),
            messages,
            status: String::new(),
            running: false,
            host_halt: Arc::new(AtomicBool::new(false)),
            rx: None,
            chat_job_thread: None,
            hub: Arc::new(Mutex::new(hub)),
            hub_on: false,
            hub_port: grokhub_core::DEFAULT_PORT,
            task_prompt: String::new(),
            mem_name,
            mem_body,
            last_persist: Instant::now(),
            board: config::load_board(),
            board_title: String::new(),
            imagine_prompt: String::new(),
            imagine_last,
            skill_name: String::new(),
            skill_body: String::new(),
            skill_list: skills::list_skills(),
            eyes_text: String::new(),
            last_host: vec![],
            last_frame_url: None,
            hands_attach: false,
            eyes_attach: false,
            speak_next: false,
            verify_ok_turn: false,
            verify_chip: String::new(),
            reflect_diff: String::new(),
            last_activity: Instant::now(),
            reflected_idle: false,
            last_recipe: None,
            pending_update: false,
            update_pct: None,
            update_can_restart: false,
            secrets,
            threads,
            thread_idx,
            oauth_pending: None,
            tray: None,
            tray_rx: if crate::tray::tray_needed_at_launch(hidden) {
                Some(crate::tray::begin_tray_spawn())
            } else {
                None
            },
            window_visible: !hidden,
            want_quit: false,
            told_tray: false,
            automations: crate::night::load(),
            night_nl: String::new(),
            history_q: String::new(),
            history_hits: vec![],
            last_receipt_ok: None,
            last_receipts: vec![],
            last_rewind_id: None,
            rewind_rows: crate::night::load_rewinds(),
            host_live: String::new(),
            daily_auto_used: 0,
            daily_auto_day: String::new(),
            slash_pick: 0,
            slash_filter_n: 0,
            slash_filter_first: "",
            last_window_title: String::new(),
            voice_orb: "idle".into(),
            last_night_tick: Instant::now(),
            last_heartbeat: Instant::now(),
            night_check_rx: None,
            learning: crate::store::load_learning(),
            suggestions: crate::store::load_suggestions(),
            review_rx: None,
            review_busy: false,
            usage: crate::store::load_usage(),
            palette_open: false,
            palette_q: String::new(),
            palette_pick: 0,
            palette_focus: false,
            shortcuts_open: false,
            active_skill_follow: None,
            last_anticipate_ms: 0,
            goal_step: 0,
            followup_step: 0,
            stream_buf: String::new(),
            thought_buf: String::new(),
            presence_ring: vec![],
            webcam_url: None,
            voice_sock: None,
            voice_state: VoiceState::Idle,
            cmd_line: String::new(),
            cmd_hist: vec![],
            agents: vec![],
            last_live: Instant::now(),
            hotkeys: None,
            hotkey_hey: 0,
            hotkey_halt: 0,
            tools_collapsed: false,
            sidebar_q: String::new(),
            rename_idx: None,
            rename_buf: String::new(),
            rename_focus: false,
            rename_lock: None,
            chip_memory: crate::store::load_chips(),
            chip_dismissed: vec![],
            llm_chips: vec![],
            visible_chips: vec![],
            chip_rx: None,
            chip_busy: false,
            chip_fp: String::new(),
            chip_llm_at: 0,
            greeting: String::new(),
            greeting_fp: String::new(),
            greeting_llm_fp: String::new(),
            greeting_rx: None,
            greeting_busy: false,
            greeting_llm_at: 0,
            continue_hint: String::new(),
            skills_tab_connectors: false,
            skill_q: String::new(),
            github_args: String::new(),
            pending_connectors: vec![],
            auto_compose: false,
            board_compose: false,
            settings_menu_open: false,
            settings_menu_ignore: false,
            win_max,
            geom_dirty: false,
            imagine_want_focus: false,
            settings_sec: SettingsSec::Account,
            settings_back: Nav::Chat,
            imagine_aspect: 0,
            imagine_quality: true,
            imagine_kind: ImagineKind::Image,
            imagine_style: 0,
            imagine_video_res: 0,
            imagine_video_dur: 0,
            imagine_video_audio: true,
            imagine_aspect_open: false,
            imagine_style_open: false,
            imagine_menu_ignore: false,
            imagine_style_anchor: egui::Rect::NOTHING,
            imagine_aspect_anchor: egui::Rect::NOTHING,
            goal_rx: None,
            goal_busy: false,
            goal_stale: false,
            wall: crate::store::load_wall(),
            wall_rx: None,
            wall_busy: false,
            attach_url: None,
            attach_name: None,
            imagine_ref: None,
            plus_menu: None,
            plus_anchor: egui::Pos2::ZERO,
            plus_ignore_close: false,
            file_pick: None,
            pick_dir: std::env::var("HOME").unwrap_or_else(|_| "/tmp".into()),
            projects,
            project_sel,
            proj_menu_pos: egui::Pos2::ZERO,
            proj_plus_open: false,
            proj_plus_pos: egui::Pos2::ZERO,
            proj_add_for: None,
            proj_rename: None,
            proj_rename_buf: String::new(),
            proj_rename_focus: false,
            proj_rename_lock: None,
            proj_staged: None,
            proj_ignore_close: false,
            projects_dirty: false,
            oauth_photo: None,
            oauth_photo_key: String::new(),
            oauth_photo_rx: None,
            oauth_photo_busy: false,
            oauth_profile_tried: false,
        };
        if let Ok(mgr) = GlobalHotKeyManager::new() {
            let hey = HotKey::new(Some(Modifiers::SUPER), Code::KeyG);
            let halt = HotKey::new(Some(Modifiers::SUPER | Modifiers::SHIFT), Code::Escape);
            let hey_id = hey.id();
            let halt_id = halt.id();
            if mgr.register(hey).is_ok() && mgr.register(halt).is_ok() {
                c.hotkey_hey = hey_id;
                c.hotkey_halt = halt_id;
                c.hotkeys = Some(mgr);
            }
        }
        c
    }

    fn capture_window(&mut self, ctx: &egui::Context) {
        let (outer, inner, maximized) = ctx.input(|i| {
            (
                i.viewport().outer_rect,
                i.viewport().inner_rect,
                i.viewport().maximized,
            )
        });
        let Some(outer) = outer else {
            return;
        };
        let size = inner.map(|r| r.size()).unwrap_or(outer.size());
        let maximized = maximized.unwrap_or(self.win_max);
        if let Some(g) = crate::window::remember_geom(
            self.window_visible,
            maximized,
            outer.min.x,
            outer.min.y,
            size.x,
            size.y,
            self.cfg.window,
        ) {
            if crate::window::geom_moved(g, self.cfg.window) {
                self.cfg.window = g;
                self.geom_dirty = true;
            }
            self.win_max = g.maximized;
        }
    }

    fn flush_window(&mut self, ctx: &egui::Context) {
        match crate::window::geom_flush(
            self.geom_dirty,
            self.last_persist.elapsed().as_millis() as u64,
        ) {
            crate::window::GeomFlush::Skip => {}
            crate::window::GeomFlush::Now => {
                if config::save(&self.cfg).is_ok() {
                    self.geom_dirty = false;
                }
            }
            crate::window::GeomFlush::AfterMs(ms) => {
                ctx.request_repaint_after(Duration::from_millis(ms));
            }
        }
    }

    fn persist(&mut self) {
        let msgs: Vec<(String, String)> = self
            .messages
            .iter()
            .map(|m| (m.role.clone(), m.content.clone()))
            .collect();
        if let Some(t) = self.threads.get_mut(self.thread_idx) {
            t.messages = msgs.clone();
            self.cfg.current_thread = t.id.clone();
        }
        let _ = threads::save(&self.threads);
        let _ = config::save_chat(&msgs);
        let _ = config::save_board(&self.board);
        let _ = crate::night::save(&self.automations);
        let _ = crate::night::save_rewinds(&self.rewind_rows);
        let _ = crate::store::save_learning(&self.learning);
        let _ = crate::store::save_suggestions(&self.suggestions);
        let _ = crate::store::save_usage(&self.usage);
        let _ = crate::store::save_chips(&self.chip_memory);
        let _ = crate::store::save_wall(&self.wall);
        self.flush_projects();
        let _ = config::save(&self.cfg);
        self.sync_hub_voice();
        if let Ok(st) = self.hub.lock() {
            let _ = save_hub_state(&config::hub_state_path(), &st);
        }
        self.last_persist = Instant::now();
        self.geom_dirty = false;
    }

    fn sync_hub_voice(&self) {
        if let Ok(mut st) = self.hub.lock() {
            st.console_api_key = self.cfg.api_key.clone();
            if st.mint_realtime.is_none() {
                st.mint_realtime = Some(MintRealtimeFn(Arc::new(|key| {
                    crate::xai::grok_realtime_secret(key)
                })));
            }
        }
    }

    fn scratch(&self) -> bool {
        self.threads
            .get(self.thread_idx)
            .map(|t| t.scratch)
            .unwrap_or(false)
    }

    fn visible_thread_id(&self) -> String {
        self.threads
            .get(self.thread_idx)
            .map(|t| t.id.clone())
            .unwrap_or_default()
    }

    fn thinking_here(&self) -> bool {
        chat_shows_thinking(
            self.chat_job_thread.as_deref(),
            &self.visible_thread_id(),
            self.running,
        )
    }

    fn halt_in_flight(&mut self) {
        self.host_halt.store(true, Ordering::SeqCst);
        self.rx = None;
        self.running = false;
        self.chat_job_thread = None;
        self.followup_step = 0;
        self.stream_buf.clear();
        self.thought_buf.clear();
        if let Some(mut s) = self.voice_sock.take() {
            s.halt();
        }
        self.voice_state = VoiceState::Idle;
        self.voice_orb = "idle".into();
    }

    fn apply_assistant_snapshot(&mut self, content: String) {
        if content.is_empty() {
            return;
        }
        let vis = self.visible_thread_id();
        let mut visible: Vec<(String, String)> = self
            .messages
            .iter()
            .map(|m| (m.role.clone(), m.content.clone()))
            .collect();
        let mut stored: Vec<(String, Vec<(String, String)>)> = self
            .threads
            .iter()
            .map(|t| (t.id.clone(), t.messages.clone()))
            .collect();
        apply_stream_snapshot(
            self.chat_job_thread.as_deref(),
            &vis,
            &mut visible,
            &mut stored,
            &content,
        );
        self.messages = visible
            .into_iter()
            .map(|(role, content)| Msg { role, content })
            .collect();
        let Some(job) = self.chat_job_thread.as_deref() else {
            return;
        };
        if job == vis {
            return;
        }
        if let Some((_, msgs)) = stored.iter().find(|(id, _)| id == job) {
            if let Some(t) = self.threads.iter_mut().find(|t| t.id == job) {
                t.messages = msgs.clone();
            }
        }
    }

    fn apply_live_assistant(&mut self) {
        self.apply_assistant_snapshot(merge_thinking(&self.thought_buf, &self.stream_buf));
    }

    fn has_key(&self) -> bool {
        has_auth(&self.cfg.api_key, &secrets::access_token(&self.secrets))
    }

    fn open_plus(&mut self, target: PlusTarget, anchor: egui::Pos2) {
        self.plus_menu = Some(target);
        self.plus_anchor = anchor;
        self.plus_ignore_close = true;
        self.file_pick = None;
    }

    fn run_plus_act(&mut self, target: PlusTarget, act: PlusAct) {
        match act {
            PlusAct::Upload => {
                if let Some(p) = pick_file() {
                    self.apply_path(target, &p);
                } else {
                    self.file_pick = Some(target);
                }
            }
            PlusAct::Paste => self.apply_clipboard(target),
        }
    }

    fn apply_clipboard(&mut self, target: PlusTarget) {
        if let Some(p) = clipboard_image() {
            self.apply_path(target, &p);
            return;
        }
        if let Some(clip) = crate::desktop::clipboard_once() {
            match target {
                PlusTarget::Chat => {
                    self.composer = append_composer(&self.composer, &clip);
                    self.status = "Pasted clipboard".into();
                }
                PlusTarget::Imagine => {
                    self.imagine_prompt = append_composer(&self.imagine_prompt, &clip);
                    self.status = "Pasted clipboard".into();
                }
            }
            return;
        }
        self.status = plus_empty_status().into();
    }

    fn apply_path(&mut self, target: PlusTarget, path: &Path) {
        let raw = path.display().to_string();
        let kind = attach_kind(&raw);
        let name = attach_name(&raw);
        match target {
            PlusTarget::Chat => match kind {
                AttachKind::Image => match load_image_data_url(path) {
                    Ok(url) => {
                        self.attach_url = Some(url);
                        self.attach_name = Some(name.clone());
                        self.status = chat_attach_status(kind, &name);
                    }
                    Err(e) => self.status = e,
                },
                AttachKind::Text => match read_text_capped(path) {
                    Ok(t) => {
                        self.composer = append_composer(&self.composer, &t);
                        self.status = chat_attach_status(kind, &name);
                    }
                    Err(e) => self.status = e,
                },
                AttachKind::Other => {
                    self.composer = append_composer(&self.composer, &raw);
                    self.status = chat_attach_status(kind, &name);
                }
            },
            PlusTarget::Imagine => match kind {
                AttachKind::Image => {
                    self.imagine_ref = Some(name.clone());
                    let hint = attach_prompt_line(kind, &name);
                    self.imagine_prompt = append_composer(&self.imagine_prompt, &hint);
                    self.status = imagine_ref_status(&name);
                }
                AttachKind::Text => match read_text_capped(path) {
                    Ok(t) => {
                        self.imagine_prompt = append_composer(&self.imagine_prompt, &t);
                        self.status = chat_attach_status(kind, &name);
                    }
                    Err(e) => self.status = e,
                },
                AttachKind::Other => {
                    self.imagine_prompt = append_composer(&self.imagine_prompt, &raw);
                    self.status = chat_attach_status(kind, &name);
                }
            },
        }
        self.file_pick = None;
    }

    fn clear_chat_attach(&mut self) {
        self.attach_url = None;
        self.attach_name = None;
        self.status.clear();
    }

    fn pick_entries(dir: &Path) -> Vec<(String, bool)> {
        let mut dirs = Vec::new();
        let mut files = Vec::new();
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with('.') || name.is_empty() {
                    continue;
                }
                if e.path().is_dir() {
                    dirs.push(name);
                } else {
                    files.push(name);
                }
            }
        }
        dirs.sort();
        files.sort();
        let mut out = Vec::new();
        for d in dirs {
            out.push((d, true));
        }
        for f in files {
            out.push((f, false));
        }
        out
    }

    fn ui_plus_overlays(&mut self, ctx: &egui::Context) {
        if let Some(target) = self.plus_menu {
            let mut picked = None;
            let mut menu_rect = egui::Rect::NOTHING;
            egui::Area::new(egui::Id::new("plus-menu"))
                .fixed_pos(self.plus_anchor + egui::vec2(0.0, 6.0))
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_min_width(168.0);
                        ui.spacing_mut().item_spacing.y = 2.0;
                        for (label, act) in plus_menu_rows() {
                            if ui.selectable_label(false, *label).clicked() {
                                picked = Some(*act);
                            }
                        }
                        menu_rect = ui.min_rect();
                    });
                });
            if let Some(act) = picked {
                self.plus_menu = None;
                self.run_plus_act(target, act);
            } else if self.plus_ignore_close {
                self.plus_ignore_close = false;
            } else if ctx.input(|i| i.pointer.any_click()) {
                if let Some(pos) = ctx.pointer_interact_pos() {
                    if !menu_rect.expand(8.0).contains(pos) {
                        self.plus_menu = None;
                    }
                }
            }
        }
        if let Some(target) = self.file_pick {
            let mut picked: Option<PathBuf> = None;
            let mut up = false;
            let mut cancel = false;
            let mut paste = false;
            let dir = PathBuf::from(&self.pick_dir);
            let entries = Self::pick_entries(&dir);
            egui::Window::new("Upload")
                .collapsible(false)
                .resizable(true)
                .default_width(420.0)
                .default_height(360.0)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(
                        RichText::new(dir.display().to_string())
                            .size(12.0)
                            .color(crate::theme::muted()),
                    );
                    ui.horizontal(|ui| {
                        if crate::cards::ghost_pill(ui, "Up") {
                            up = true;
                        }
                        if crate::cards::ghost_pill(ui, "Home") {
                            if let Ok(home) = std::env::var("HOME") {
                                self.pick_dir = home;
                            }
                        }
                        if crate::cards::ghost_pill(ui, "Paste clipboard") {
                            paste = true;
                        }
                        if crate::cards::ghost_pill(ui, "Cancel") {
                            cancel = true;
                        }
                    });
                    ui.add_space(6.0);
                    egui::ScrollArea::vertical()
                        .max_height(260.0)
                        .show(ui, |ui| {
                            for (name, is_dir) in &entries {
                                let icon = if *is_dir {
                                    crate::icons::RailIcon::Folder
                                } else {
                                    crate::icons::RailIcon::File
                                };
                                let row = ui
                                    .horizontal(|ui| {
                                        crate::icons::paint_rail_icon(
                                            ui,
                                            icon,
                                            16.0,
                                            crate::theme::muted(),
                                        );
                                        ui.selectable_label(false, name)
                                    })
                                    .inner;
                                if row.clicked() {
                                    let next = dir.join(name);
                                    if *is_dir {
                                        self.pick_dir = next.display().to_string();
                                    } else {
                                        picked = Some(next);
                                    }
                                }
                            }
                        });
                });
            if up {
                if let Some(parent) = dir.parent() {
                    self.pick_dir = parent.display().to_string();
                }
            }
            if let Some(p) = picked {
                self.apply_path(target, &p);
            } else if paste {
                self.file_pick = None;
                self.apply_clipboard(target);
            } else if cancel {
                self.file_pick = None;
            }
        }
    }

    fn ui_imagine_overlays(&mut self, ctx: &egui::Context) {
        if self.page_nav() != Nav::Imagine {
            self.imagine_style_open = false;
            self.imagine_aspect_open = false;
            return;
        }
        let mut menu_rect = egui::Rect::NOTHING;
        let mut trigger = egui::Rect::NOTHING;
        if self.imagine_style_open {
            let rows: Vec<(String, bool)> = IMAGINE_STYLES
                .iter()
                .enumerate()
                .map(|(i, label)| ((*label).to_string(), self.imagine_style == i as u8))
                .collect();
            let (picked, rect) = imagine_popup(
                ctx,
                "imagine_style_menu",
                self.imagine_style_anchor,
                &rows,
            );
            menu_rect = rect;
            trigger = self.imagine_style_anchor;
            if let Some(i) = picked {
                self.imagine_style = i as u8;
                self.imagine_style_open = false;
            }
        } else if self.imagine_aspect_open {
            let rows: Vec<(String, bool)> = IMAGINE_ASPECTS
                .iter()
                .enumerate()
                .map(|(i, (ratio, name))| {
                    (
                        format!("{ratio}  {name}"),
                        self.imagine_aspect == i as u8,
                    )
                })
                .collect();
            let (picked, rect) = imagine_popup(
                ctx,
                "imagine_aspect_menu",
                self.imagine_aspect_anchor,
                &rows,
            );
            menu_rect = rect;
            trigger = self.imagine_aspect_anchor;
            if let Some(i) = picked {
                self.imagine_aspect = i as u8;
                self.imagine_aspect_open = false;
            }
        }
        let outside = ctx.input(|i| i.pointer.any_click())
            && ctx.pointer_interact_pos().is_some_and(|pos| {
                !menu_rect.expand(8.0).contains(pos) && !trigger.expand(4.0).contains(pos)
            });
        if cabin_menu_should_dismiss(self.imagine_menu_ignore, outside) {
            self.imagine_style_open = false;
            self.imagine_aspect_open = false;
        }
        self.imagine_menu_ignore = false;
    }

    fn ui_attach_chip(&mut self, ui: &mut egui::Ui, target: PlusTarget) {
        match target {
            PlusTarget::Chat => {
                if let Some(name) = self.attach_name.clone() {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("Attached {name}"))
                                .size(12.0)
                                .color(crate::theme::fg()),
                        );
                        if ui.small_button("×").clicked() {
                            self.clear_chat_attach();
                        }
                    });
                }
            }
            PlusTarget::Imagine => {
                if let Some(name) = self.imagine_ref.clone() {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("Reference {name}"))
                                .size(12.0)
                                .color(crate::theme::fg()),
                        );
                        if ui.small_button("×").clicked() {
                            self.imagine_ref = None;
                        }
                    });
                }
            }
        }
        if !self.status.is_empty() && self.status != "Thinking…" {
            ui.label(
                RichText::new(crate::cards::clip_status(&self.status, 72))
                    .size(12.0)
                    .color(crate::theme::muted()),
            );
        }
    }

    fn work_root(&self) -> String {
        if let Ok(home) = std::env::var("HOME") {
            format!("{home}/GrokHub-Work")
        } else {
            "GrokHub-Work".into()
        }
    }

    fn touch_projects(&mut self) {
        self.projects_dirty = true;
    }

    fn flush_projects(&mut self) {
        if !self.projects_dirty {
            return;
        }
        match crate::store::save_projects(&self.projects) {
            Ok(()) => self.projects_dirty = false,
            Err(e) => self.status = format!("Projects not saved: {e}"),
        }
    }

    fn bind_project_id(&mut self, id: &str) {
        let Some(n) = self.projects.iter().find(|n| n.id == id && n.kind == ProjectKind::Project) else {
            return;
        };
        let path = n.path.clone();
        let name = n.name.clone();
        if !path.trim().is_empty() {
            if !std::path::Path::new(&path).is_dir() {
                let _ = std::fs::create_dir_all(&path);
            }
            self.cfg.project_dir = path.clone();
        }
        let already = self.project_sel.as_deref() == Some(id);
        self.project_sel = Some(id.to_string());
        if click_project_opens_board(already) {
            self.nav = Nav::Workboard;
        }
        self.status = format!("Bound {name}");
        self.persist();
    }

    fn make_project(&mut self, name: &str, parent: Option<&str>) {
        let id = uid("proj");
        let root = self.work_root();
        match create_project(&mut self.projects, &id, name, parent, &root) {
            Ok(i) => {
                let path = self.projects[i].path.clone();
                if !std::path::Path::new(&path).is_dir() {
                    let _ = std::fs::create_dir_all(&path);
                }
                self.touch_projects();
                self.bind_project_id(&id);
                self.status = format!("Project {}", self.projects[i].name);
            }
            Err(e) => self.status = e.into(),
        }
    }

    fn remove_project_id(&mut self, id: &str) {
        let bound = self.cfg.project_dir.clone();
        let selected = self.project_sel.as_deref() == Some(id);
        let out = drop_selected(&mut self.projects, id, &bound);
        if !out.dropped {
            self.status = "Project not found".into();
            return;
        }
        if out.unbound {
            self.cfg.project_dir.clear();
        }
        if selected {
            self.project_sel = None;
        }
        self.touch_projects();
        self.persist();
        self.status = if out.unbound {
            format!("Removed {} · unbound", out.name)
        } else {
            format!("Removed {}", out.name)
        };
    }

    fn apply_project_menu(&mut self, id: String, act: ProjectMenuAct) {
        match act {
            ProjectMenuAct::Rename => {
                if let Some(n) = self.projects.iter().find(|n| n.id == id) {
                    self.begin_proj_rename(id, n.name.clone());
                }
            }
            ProjectMenuAct::AddToFolder => {
                self.proj_add_for = Some(id.clone());
                self.project_sel = Some(id);
                self.proj_ignore_close = true;
            }
            ProjectMenuAct::RemoveFromFolder => {
                if add_to_folder(&mut self.projects, &id, None).is_ok() {
                    self.status = "Moved to Projects".into();
                    self.touch_projects();
                    self.persist();
                }
            }
            ProjectMenuAct::NewHere => self.stage_new_project(Some(&id)),
            ProjectMenuAct::Delete => self.remove_project_id(&id),
        }
    }

    fn stage_new_project(&mut self, parent: Option<&str>) {
        let id = uid("proj");
        match stage_project(&mut self.projects, &id, "Project", parent) {
            Ok(_) => {
                if let Some(pid) = parent {
                    if let Some(f) = self.projects.iter_mut().find(|n| n.id == pid) {
                        f.open = true;
                    }
                }
                self.begin_proj_rename(id.clone(), String::new());
                self.proj_staged = Some(id);
                self.status = "Name this project".into();
                self.touch_projects();
                self.persist();
            }
            Err(e) => self.status = e.into(),
        }
    }

    fn make_folder(&mut self, name: &str) {
        let id = uid("fold");
        match create_folder(&mut self.projects, &id, name, None) {
            Ok(i) => {
                self.status = format!("Folder {}", self.projects[i].name);
                self.touch_projects();
                self.persist();
            }
            Err(e) => self.status = e.into(),
        }
    }

    fn stage_new_folder(&mut self) {
        let id = uid("fold");
        match create_folder(&mut self.projects, &id, "Folder", None) {
            Ok(_) => {
                self.begin_proj_rename(id.clone(), String::new());
                self.proj_staged = Some(id);
                self.status = "Name this folder".into();
                self.touch_projects();
                self.persist();
            }
            Err(e) => self.status = e.into(),
        }
    }

    fn begin_proj_rename(&mut self, id: String, buf: String) {
        self.proj_rename_lock = if buf.is_empty() { None } else { Some(buf.clone()) };
        self.proj_rename_buf = buf;
        self.proj_rename = Some(id);
        self.proj_rename_focus = true;
    }

    fn cancel_proj_rename(&mut self) {
        let id = self.proj_rename.take();
        self.proj_rename_buf.clear();
        self.proj_rename_focus = false;
        self.proj_rename_lock = None;
        if let Some(id) = id {
            if self.proj_staged.as_deref() == Some(id.as_str()) {
                drop_node(&mut self.projects, &id);
                self.touch_projects();
                self.persist();
            }
        }
        self.proj_staged = None;
    }

    fn finish_proj_rename(&mut self) {
        let Some(id) = self.proj_rename.take() else {
            return;
        };
        let staged = self.proj_staged.as_deref() == Some(id.as_str());
        match rename_node(&mut self.projects, &id, &self.proj_rename_buf) {
            Ok(()) => {
                self.status = format!("Renamed {}", self.proj_rename_buf.trim());
                self.touch_projects();
                let mut bound = false;
                if staged {
                    let root = self.work_root();
                    if let Ok(path) = settle_project_path(&mut self.projects, &id, &root) {
                        if !path.is_empty() {
                            if !std::path::Path::new(&path).is_dir() {
                                let _ = std::fs::create_dir_all(&path);
                            }
                            self.bind_project_id(&id);
                            bound = true;
                        }
                    }
                }
                if !bound {
                    self.persist();
                }
            }
            Err(e) => {
                if staged {
                    drop_node(&mut self.projects, &id);
                    self.touch_projects();
                }
                self.status = e.into();
            }
        }
        self.proj_rename_buf.clear();
        self.proj_rename_focus = false;
        self.proj_rename_lock = None;
        self.proj_staged = None;
    }

    fn move_sel_to_folder_name(&mut self, folder: &str) {
        let Some(pid) = self.project_sel.clone() else {
            self.status = "Select a project first".into();
            return;
        };
        if folder.eq_ignore_ascii_case("root") {
            match add_to_folder(&mut self.projects, &pid, None) {
                Ok(()) => {
                    self.status = "Moved to Projects".into();
                    self.touch_projects();
                    self.persist();
                }
                Err(e) => self.status = e.into(),
            }
            return;
        }
        let fid = self
            .projects
            .iter()
            .find(|n| n.kind == ProjectKind::Folder && n.name.eq_ignore_ascii_case(folder))
            .map(|n| n.id.clone());
        let Some(fid) = fid else {
            self.status = format!("No folder {folder}");
            return;
        };
        match add_to_folder(&mut self.projects, &pid, Some(&fid)) {
            Ok(()) => {
                if let Some(f) = self.projects.iter_mut().find(|n| n.id == fid) {
                    f.open = true;
                }
                self.status = format!("Added to {folder}");
                self.touch_projects();
                self.persist();
            }
            Err(e) => self.status = e.into(),
        }
    }

    fn chat_pairs(&self) -> Vec<(String, String)> {
        self.messages
            .iter()
            .map(|m| (m.role.clone(), m.content.clone()))
            .collect()
    }

    fn chip_hour() -> u8 {
        Self::local_clock().hour as u8
    }

    fn other_chip_threads(&self) -> Vec<ChipThread> {
        let current = self
            .threads
            .get(self.thread_idx)
            .map(|t| t.id.as_str())
            .unwrap_or("");
        collect_other_chip_threads(&self.threads, current)
    }

    fn poll_chips(&mut self) {
        let Some(rx) = self.chip_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(chips) => {
                self.chip_busy = false;
                self.llm_chips = chips
                    .into_iter()
                    .filter(|c| is_plain_text(&c.label) && is_plain_text(&c.value))
                    .collect();
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.chip_rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.chip_busy = false;
            }
        }
    }

    fn poll_greeting(&mut self) {
        let Some(rx) = self.greeting_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(raw) => {
                self.greeting_busy = false;
                self.greeting = pick_greeting(&self.greeting, Some(&raw));
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.greeting_rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.greeting_busy = false;
            }
        }
    }

    fn refresh_greeting(&mut self) {
        let empty = self.messages.is_empty();
        let scratch = self.scratch();
        if !should_paint_greeting(empty, scratch) {
            if !self.greeting.is_empty() {
                self.greeting.clear();
            }
            return;
        }
        let user_md = config::read_memory("USER.md");
        let memory_md = config::read_memory("MEMORY.md");
        let insights: Vec<String> = self
            .learning
            .insights
            .iter()
            .take(6)
            .map(|i| i.text.clone())
            .collect();
        let display_name = self
            .secrets
            .oauth
            .as_ref()
            .and_then(|t| t.name.clone())
            .unwrap_or_default();
        let hour = Self::chip_hour();
        let last_night = self.last_night_hint();
        let input = GreetingInput {
            user_md: &user_md,
            memory_md: &memory_md,
            insights: &insights,
            display_name: &display_name,
            hour,
            last_night: &last_night,
        };
        let local = local_greeting(&input);
        let fp = greeting_fingerprint(&input);
        if self.greeting_fp != fp {
            self.greeting = local;
            self.greeting_fp = fp.clone();
        }
        if should_refresh_greeting(
            &self.greeting_llm_fp,
            &fp,
            self.greeting_llm_at,
            now_ms(),
            self.has_key(),
            self.greeting_busy,
        ) {
            self.greeting_llm_fp = fp;
            self.greeting_llm_at = now_ms();
            self.spawn_greeting_llm(
                &user_md,
                &memory_md,
                &insights,
                &display_name,
                hour,
                &last_night,
            );
        }
    }

    fn spawn_greeting_llm(
        &mut self,
        user_md: &str,
        memory_md: &str,
        insights: &[String],
        display_name: &str,
        hour: u8,
        last_night: &str,
    ) {
        if self.greeting_busy {
            return;
        }
        let key = self.bearer();
        if key.trim().is_empty() {
            return;
        }
        let input = GreetingInput {
            user_md,
            memory_md,
            insights,
            display_name,
            hour,
            last_night,
        };
        let prompt = greeting_prompt(&input);
        let model = model_for_mode(GREETING_LLM_MODE).to_string();
        let (tx, rx) = mpsc::channel();
        self.greeting_rx = Some(rx);
        self.greeting_busy = true;
        std::thread::spawn(move || {
            let raw = grok_chat(&key, &model, &[("user".into(), prompt)], None, None)
                .unwrap_or_default();
            let _ = tx.send(raw);
        });
    }

    fn poll_goals(&mut self) {
        let Some(rx) = self.goal_rx.take() else {
            if self.goal_stale {
                self.spawn_thread_goal();
            }
            return;
        };
        match rx.try_recv() {
            Ok((tid, reply)) => {
                self.goal_busy = false;
                self.apply_thread_goal(&tid, &reply);
                if self.goal_stale {
                    self.spawn_thread_goal();
                }
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.goal_rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.goal_busy = false;
                if self.goal_stale {
                    self.spawn_thread_goal();
                }
            }
        }
    }

    fn apply_thread_goal(&mut self, tid: &str, reply: &str) {
        let topics = parse_fast_topics(reply);
        if topics.is_empty() {
            return;
        }
        let current = self
            .threads
            .get(self.thread_idx)
            .map(|t| t.id.clone())
            .unwrap_or_default();
        let Some(t) = self.threads.iter_mut().find(|t| t.id == tid) else {
            return;
        };
        if t.scratch {
            return;
        }
        t.goal = blend_thread_goal(&t.goal, &topics, GOAL_DROP_AFTER);
        if !t.goal.label.is_empty() {
            let mut tab = ThreadTab {
                title: t.title.clone(),
                pinned: t.pinned,
                title_locked: t.title_locked,
            };
            if apply_auto_title(&mut tab, &t.goal.label) {
                t.title = tab.title;
            }
            if tid == current {
                self.cfg.goal_pin = t.goal.label.clone();
            }
        }
        let _ = threads::save(&self.threads);
        let _ = config::save(&self.cfg);
    }

    fn spawn_thread_goal(&mut self) {
        if self.goal_busy {
            self.goal_stale = true;
            return;
        }
        let scratch = self.scratch();
        let user_turns = self.messages.iter().filter(|m| m.role == "user").count();
        let locked = self
            .threads
            .get(self.thread_idx)
            .map(|t| t.title_locked)
            .unwrap_or(false);
        if locked || !should_name_thread(scratch, user_turns) {
            self.goal_stale = false;
            return;
        }
        if !self.has_key() {
            self.goal_stale = false;
            return;
        }
        let tid = self
            .threads
            .get(self.thread_idx)
            .map(|t| t.id.clone())
            .unwrap_or_default();
        if tid.is_empty() {
            self.goal_stale = false;
            return;
        }
        let prompt = thread_goal_prompt(&self.chat_pairs());
        let key = self.bearer();
        if key.trim().is_empty() {
            self.goal_stale = false;
            return;
        }
        let model = model_for_mode("fast").to_string();
        let (tx, rx) = mpsc::channel();
        self.goal_rx = Some(rx);
        self.goal_busy = true;
        self.goal_stale = false;
        std::thread::spawn(move || {
            let reply = grok_chat(&key, &model, &[("user".into(), prompt)], None, None)
                .unwrap_or_default();
            let _ = tx.send((tid, reply));
        });
    }

    fn refresh_chips(&mut self) {
        let chat = self.chat_pairs();
        let hour = Self::chip_hour();
        let title = self
            .threads
            .get(self.thread_idx)
            .map(|t| t.title.clone())
            .unwrap_or_default();
        let last_failed = self.last_receipt_ok == Some(false);
        let others = self.other_chip_threads();
        let input = ChipInput {
            chat: &chat,
            draft: &self.composer,
            grok_connected: self.has_key(),
            host_on: true,
            mode: if self.cfg.mode.trim().is_empty() {
                "auto"
            } else {
                self.cfg.mode.as_str()
            },
            thread_title: &title,
            usage_messages: self.usage.messages,
            usage_cap: self.cfg.daily_auto_cap,
            memory: &self.chip_memory,
            dismissed: &self.chip_dismissed,
            llm_chips: &self.llm_chips,
            last_failed,
            hour,
            now_ms: now_ms(),
            max: CHIP_VISIBLE_MAX,
            other_threads: &others,
        };
        let mode = input.mode;
        self.visible_chips = build_quick_chips(input);
        let mut fp = context_fingerprint(&chat, &self.composer, last_failed, hour, mode);
        if !others.is_empty() {
            let extra: String = others
                .iter()
                .take(4)
                .map(|t| t.title.chars().take(16).collect::<String>())
                .collect::<Vec<_>>()
                .join(",");
            fp = format!("{fp}+o:{extra}");
        }
        if should_refresh_llm(
            &self.chip_fp,
            &fp,
            self.chip_llm_at,
            now_ms(),
            self.has_key(),
            self.chip_busy,
        ) {
            self.chip_fp = fp;
            self.chip_llm_at = now_ms();
            self.spawn_chip_llm();
        }
    }

    fn spawn_chip_llm(&mut self) {
        if self.chip_busy {
            return;
        }
        let key = self.bearer();
        if key.trim().is_empty() {
            return;
        }
        let chat = self.chat_pairs();
        let title = self
            .threads
            .get(self.thread_idx)
            .map(|t| t.title.clone())
            .unwrap_or_default();
        let habits = top_habit_labels(&self.chip_memory, 6);
        let others = self.other_chip_threads();
        let prompt = chip_suggest_prompt(
            &chat,
            &title,
            &self.composer,
            &habits,
            &self.chip_dismissed,
            &others,
        );
        let model = model_for_mode(CHIP_LLM_MODE).to_string();
        let (tx, rx) = mpsc::channel();
        self.chip_rx = Some(rx);
        self.chip_busy = true;
        std::thread::spawn(move || {
            let chips = grok_chat(&key, &model, &[("user".into(), prompt)], None, None)
                .map(|t| parse_llm_chips(&t))
                .unwrap_or_default();
            let _ = tx.send(chips);
        });
    }

    fn apply_chip(&mut self, chip: QuickChip) {
        let hour = Self::chip_hour();
        let mode = if self.cfg.mode.trim().is_empty() {
            "auto"
        } else {
            self.cfg.mode.as_str()
        };
        let tag = context_fingerprint(
            &self.chat_pairs(),
            &self.composer,
            self.last_receipt_ok == Some(false),
            hour,
            mode,
        );
        remember_chip_click(&mut self.chip_memory, &chip, Some(&tag), now_ms(), hour);
        let _ = crate::store::save_chips(&self.chip_memory);
        match chip.kind {
            ChipKind::Nav => {
                if let Some(id) = nav_from_chip_value(&chip.value) {
                    self.nav = Self::nav_from_id(id);
                }
            }
            ChipKind::Mode => {
                if let Some(mode) = mode_from_chip_value(&chip.value) {
                    self.run_slash(Slash::Mode(mode.to_string()));
                }
            }
            ChipKind::Shell => {
                let cmd = chip.value.trim().trim_start_matches('$').trim();
                let cmd = cmd.strip_prefix("/sh ").unwrap_or(cmd);
                self.run_slash(Slash::Sh(cmd.to_string()));
            }
            ChipKind::Chat => {
                if chip.value.starts_with('/') {
                    if let Some(slash) = parse_slash(&chip.value) {
                        self.run_slash(slash);
                        return;
                    }
                }
                self.composer.clear();
                self.send_chat(chip.value);
            }
        }
    }

    fn dismiss_chip(&mut self, chip: QuickChip) {
        remember_chip_dismiss(&mut self.chip_memory, &chip, now_ms(), Self::chip_hour());
        self.chip_dismissed.push(chip.id);
        self.chip_dismissed.push(chip.value);
        let _ = crate::store::save_chips(&self.chip_memory);
    }

    fn nav_from_id(id: &str) -> Nav {
        match id {
            "settings" => Nav::Settings,
            "imagine" => Nav::Imagine,
            "history" => Nav::History,
            "workboard" => Nav::Workboard,
            "skills" => Nav::Skills,
            "night" => Nav::Night,
            "command" => Nav::Command,
            "agents" => Nav::Agents,
            "chat" => Nav::Chat,
            _ => Nav::Chat,
        }
    }

    fn bearer(&mut self) -> String {
        if let Some(tok) = self.secrets.oauth.clone() {
            if let Ok((access, next, refreshed)) = crate::oauth::ensure_access(&tok) {
                if refreshed {
                    self.secrets.oauth = Some(next);
                    let _ = secrets::save(&self.secrets);
                }
                if self.cfg.api_key.trim().is_empty() {
                    return access;
                }
            }
        }
        auth_bearer(&self.cfg.api_key, &secrets::access_token(&self.secrets)).unwrap_or_default()
    }

    fn switch_thread(&mut self, idx: usize) {
        if let Some(t) = self.threads.get_mut(self.thread_idx) {
            t.messages = self
                .messages
                .iter()
                .map(|m| (m.role.clone(), m.content.clone()))
                .collect();
        }
        self.thread_idx = idx.min(self.threads.len().saturating_sub(1));
        self.rename_idx = None;
        self.messages = self
            .threads
            .get(self.thread_idx)
            .map(|t| {
                t.messages
                    .iter()
                    .map(|(role, content)| Msg {
                        role: role.clone(),
                        content: content.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.imagine_last =
            last_imagine_receipt(self.messages.iter().map(|m| m.content.as_str())).unwrap_or_default();
        self.cfg.goal_pin = self
            .threads
            .get(self.thread_idx)
            .map(|t| t.goal.label.clone())
            .unwrap_or_default();
        self.stamp_current_access();
        self.persist();
    }

    fn stamp_current_access(&mut self) {
        if let Some(t) = self.threads.get_mut(self.thread_idx) {
            t.accessed_ms = now_ms();
        }
    }

    fn open_recent_chat(&mut self) {
        if let Some(idx) = threads::most_recently_accessed_index(&self.threads) {
            if idx != self.thread_idx {
                self.switch_thread(idx);
                return;
            }
        }
        self.stamp_current_access();
    }

    fn new_thread(&mut self, scratch: bool) {
        if let Some(t) = self.threads.get_mut(self.thread_idx) {
            t.messages = self
                .messages
                .iter()
                .map(|m| (m.role.clone(), m.content.clone()))
                .collect();
        }
        let title = if scratch { "Scratch" } else { "Chat" };
        self.threads.push(ChatThread::new(title, scratch));
        self.thread_idx = self.threads.len() - 1;
        self.messages.clear();
        self.imagine_last.clear();
        self.cfg.goal_pin.clear();
        self.goal_step = 0;
        self.followup_step = 0;
        self.status = if scratch {
            "Scratch — no memory writes".into()
        } else {
            "New chat".into()
        };
        self.stamp_current_access();
        self.persist();
    }

    fn begin_chat_rename(&mut self, idx: usize) {
        self.rename_buf = self
            .threads
            .get(idx)
            .map(|t| t.title.clone())
            .unwrap_or_default();
        self.rename_lock = if self.rename_buf.is_empty() {
            None
        } else {
            Some(self.rename_buf.clone())
        };
        self.rename_idx = Some(idx);
        self.rename_focus = true;
    }

    fn rename_thread(&mut self, idx: usize, title: &str) {
        let Some(t) = self.threads.get_mut(idx) else {
            return;
        };
        let mut tab = ThreadTab {
            title: t.title.clone(),
            pinned: t.pinned,
            title_locked: t.title_locked,
        };
        if apply_manual_rename(&mut tab, title) {
            t.title = tab.title;
            t.title_locked = true;
            self.status = format!("Renamed {}", t.title);
            self.rename_idx = None;
            self.rename_focus = false;
            self.rename_lock = None;
            self.persist();
        }
    }

    fn pin_thread(&mut self, idx: usize) {
        let Some(t) = self.threads.get_mut(idx) else {
            return;
        };
        t.pinned = toggle_pin(t.pinned);
        self.status = if t.pinned {
            format!("Pinned {}", t.title)
        } else {
            format!("Unpinned {}", t.title)
        };
        self.persist();
    }

    fn delete_thread_at(&mut self, idx: usize) {
        let was_current = idx == self.thread_idx;
        match delete_thread(self.threads.len(), idx, self.thread_idx) {
            DeleteOutcome::ResetLast => {
                self.halt_in_flight();
                self.threads.clear();
                self.threads.push(ChatThread::new("Chat", false));
                self.thread_idx = 0;
                self.messages.clear();
                self.imagine_last.clear();
                self.cfg.goal_pin.clear();
                self.goal_step = 0;
                self.status = "Chat deleted".into();
                self.stamp_current_access();
            }
            DeleteOutcome::Removed { next } => {
                let gone = self.threads.remove(idx);
                if self.chat_job_thread.as_deref() == Some(gone.id.as_str()) {
                    self.halt_in_flight();
                }
                self.thread_idx = next;
                if was_current {
                    self.messages = self
                        .threads
                        .get(next)
                        .map(|t| {
                            t.messages
                                .iter()
                                .map(|(role, content)| Msg {
                                    role: role.clone(),
                                    content: content.clone(),
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    self.imagine_last = last_imagine_receipt(
                        self.messages.iter().map(|m| m.content.as_str()),
                    )
                    .unwrap_or_default();
                    self.cfg.goal_pin = self
                        .threads
                        .get(next)
                        .map(|t| t.goal.label.clone())
                        .unwrap_or_default();
                }
                self.status = format!("Deleted {}", gone.title);
            }
        }
        self.rename_idx = None;
        self.persist();
    }

    fn send_chat(&mut self, text: String) {
        let mut text = text.trim().to_string();
        if text.is_empty() {
            return;
        }
        match chat_send_kind(
            self.chat_job_thread.as_deref(),
            &self.visible_thread_id(),
            self.running,
        ) {
            ChatSendKind::Redirect => {
                let prev = last_user_text(
                    &self
                        .messages
                        .iter()
                        .map(|m| (m.role.clone(), m.content.clone()))
                        .collect::<Vec<_>>(),
                )
                .unwrap_or_default();
                self.halt_work("Redirected");
                text = redirect_prompt(&prev, &text);
            }
            ChatSendKind::Fresh => {
                if self.running && self.chat_job_thread.is_some() {
                    self.halt_in_flight();
                }
            }
        }
        self.touch();
        remember_typed_prompt(
            &mut self.chip_memory,
            &text,
            now_ms(),
            Self::local_clock().hour as u8,
        );
        if let Some(slash) = parse_slash(&text) {
            self.run_slash(slash);
            return;
        }
        self.active_skill_follow = None;
        if let Some(sk) = match_skill(&text, &self.skill_list) {
            self.skill_name = sk.name.clone();
            self.status = format!("Skill {}", sk.name);
            if self.policy().injects_skill() {
                self.active_skill_follow = Some(skill_follow_block(sk));
            }
        }
        self.messages.push(Msg {
            role: "user".into(),
            content: text.clone(),
        });
        if user_asks_desktop_hands(&text) || user_asks_takeover(&text) {
            self.hands_attach = true;
            self.eyes_attach = true;
        }
        if self.cfg.cabin_eyes && user_asks_cabin_eyes(&text) {
            self.eyes_attach = true;
        }
        self.followup_step = 0;
        self.persist();
        self.kick_model();
    }

    fn send_followup_turn(&mut self) {
        if self.followup_step >= FOLLOWUP_MAX_STEPS {
            return;
        }
        self.followup_step += 1;
        self.messages.push(Msg {
            role: "user".into(),
            content: FOLLOWUP_PROMPT.into(),
        });
        self.persist();
        self.kick_model();
    }

    fn run_slash(&mut self, slash: Slash) {
        match slash {
            Slash::Forget(topic) => match topic {
                None => match config::write_memory("MEMORY.md", "") {
                    Ok(()) => {
                        if self.mem_name == "MEMORY.md" {
                            self.mem_body.clear();
                        }
                        self.status = "Forgot MEMORY.md".into();
                    }
                    Err(e) => self.status = e,
                },
                Some(q) => {
                    let next = forget_topic(&config::read_memory("MEMORY.md"), &q);
                    match config::write_memory("MEMORY.md", &next) {
                        Ok(()) => {
                            if self.mem_name == "MEMORY.md" {
                                self.mem_body = next;
                            }
                            self.status = format!("Forgot {q}");
                        }
                        Err(e) => self.status = e,
                    }
                }
            },
            Slash::MemoryShow => {
                self.nav = Nav::Memory;
                self.status = "Memory".into();
            }
            Slash::MemoryNote(note) => {
                if self.scratch() {
                    self.status = "Scratch — no memory writes".into();
                    return;
                }
                if !is_plain_text(&note) {
                    self.status = "Secrets never in markdown".into();
                    return;
                }
                match config::append_memory("MEMORY.md", &note) {
                    Ok(()) => {
                        if self.mem_name == "MEMORY.md" {
                            self.mem_body = config::read_memory("MEMORY.md");
                        }
                        self.status = "Wrote MEMORY.md".into();
                    }
                    Err(e) => self.status = e,
                }
            }
            Slash::Board => {
                self.nav = Nav::Workboard;
                self.status = format!("{} cards", self.board.len());
            }
            Slash::Imagine(p) => {
                self.nav = Nav::Imagine;
                self.imagine_want_focus = true;
                self.imagine_prompt = p;
                self.kick_imagine();
            }
            Slash::Compact => {
                let pin = if self.cfg.goal_pin.trim().is_empty() {
                    None
                } else {
                    Some(self.cfg.goal_pin.as_str())
                };
                let msgs: Vec<(String, String)> = self
                    .messages
                    .iter()
                    .map(|m| (m.role.clone(), m.content.clone()))
                    .collect();
                self.messages = compact_keep_pin(&msgs, 8, pin)
                    .into_iter()
                    .map(|(role, content)| Msg { role, content })
                    .collect();
                self.persist();
                self.status = "Compacted".into();
            }
            Slash::Skill(name) => {
                self.nav = Nav::Skills;
                if let Some(s) = self.skill_list.iter().find(|s| s.name == name || s.slash == name) {
                    self.skill_name = s.name.clone();
                    self.skill_body = grokhub_core::render_skill_md(s);
                    self.status = format!("Skill {}", s.name);
                } else {
                    self.status = format!("No skill {name}");
                }
            }
            Slash::LearnReflect => self.run_reflect(),
            Slash::Update => self.queue_update(),
            Slash::Help => {
                self.messages.push(Msg {
                    role: "assistant".into(),
                    content: slash_help(),
                });
                self.persist();
            }
            Slash::New => self.new_thread(false),
            Slash::Scratch => self.new_thread(true),
            Slash::Clear => {
                self.messages.clear();
                self.persist();
                self.status = "Cleared".into();
            }
            Slash::Undo => {
                if let Some(i) = self.messages.iter().rposition(|m| m.role == "assistant") {
                    self.messages.remove(i);
                    self.persist();
                    self.status = "Undid last assistant turn".into();
                } else {
                    self.status = "Nothing to undo".into();
                }
            }
            Slash::Retry => {
                if let Some(m) = self
                    .messages
                    .iter()
                    .rev()
                    .find(|m| m.role == "user" && !is_workload_user(&m.content))
                {
                    let t = m.content.clone();
                    self.kick_model_retry(t);
                } else {
                    self.status = "Nothing to retry".into();
                }
            }
            Slash::Stop => self.halt_work("Stopped"),
            Slash::Sh(cmd) => self.queue_sh(cmd),
            Slash::HostStatus => {
                self.status = format!(
                    "Host on · {}",
                    if self.host_live.is_empty() {
                        "idle"
                    } else {
                        &self.host_live
                    }
                );
            }
            Slash::Rename(title) => self.rename_thread(self.thread_idx, &title),
            Slash::Pin => self.pin_thread(self.thread_idx),
            Slash::Delete => self.delete_thread_at(self.thread_idx),
            Slash::Context => {
                let n = self.messages.len();
                let tokens = estimate_messages(
                    &self
                        .messages
                        .iter()
                        .map(|m| (m.role.clone(), m.content.clone()))
                        .collect::<Vec<_>>(),
                );
                self.status = format!(
                    "{n} turns · {} tokens · {}% · pin {}",
                    tokens,
                    context_percent(tokens, CONTEXT_BUDGET_TOKENS),
                    if self.cfg.goal_pin.is_empty() {
                        "none"
                    } else {
                        &self.cfg.goal_pin
                    }
                );
            }
            Slash::Health => {
                self.nav = Nav::Settings;
                self.settings_sec = health_settings_sec();
                self.status = self.doctor_text();
            }
            Slash::Fix => {
                self.halt_work("Stopped");
                self.nav = Nav::Settings;
                self.settings_sec = health_settings_sec();
                self.status = self.doctor_text();
            }
            Slash::Remember(note) => self.run_slash(Slash::MemoryNote(note)),
            Slash::Mode(mode) => {
                self.cfg.mode = mode.clone();
                let _ = config::save(&self.cfg);
                self.status = mode_status_line(&mode, &self.cfg.model);
            }
            Slash::Dream => self.run_dream(),
            Slash::Import => self.import_openclaw(),
            Slash::Consult(q) => self.run_consult(q),
            Slash::Usage => {
                self.status = usage_line(&self.usage);
            }
            Slash::Models => {
                self.messages.push(Msg {
                    role: "assistant".into(),
                    content: catalog_line(),
                });
                self.persist();
            }
            Slash::Palette => self.open_palette(),
            Slash::ProjectBind(path) => {
                let p = path.unwrap_or_else(|| self.cfg.project_dir.clone());
                let p = expand_home(&p);
                self.cfg.project_dir = p.clone();
                let _ = std::fs::create_dir_all(&p);
                self.project_sel = upsert_bound(&mut self.projects, &p);
                self.touch_projects();
                self.persist();
                self.status = format!("Bound {p}");
            }
            Slash::ProjectClear => {
                self.cfg.project_dir.clear();
                let _ = config::save(&self.cfg);
                self.status = "Unbound — full desktop".into();
            }
            Slash::ProjectShow => {
                self.status = if self.cfg.project_dir.trim().is_empty() {
                    "No bound project".into()
                } else {
                    format!("Project {}", self.cfg.project_dir)
                };
            }
            Slash::ProjectNew(name) => self.make_project(&name, None),
            Slash::ProjectFolder(name) => self.make_folder(&name),
            Slash::ProjectRename(name) => {
                let Some(id) = self.project_sel.clone() else {
                    self.status = "Select a project first".into();
                    return;
                };
                match rename_node(&mut self.projects, &id, &name) {
                    Ok(()) => {
                        self.status = format!("Renamed {name}");
                        self.touch_projects();
                        self.persist();
                    }
                    Err(e) => self.status = e.into(),
                }
            }
            Slash::ProjectMove(folder) => self.move_sel_to_folder_name(&folder),
            Slash::ProjectDelete => {
                let Some(id) = self.project_sel.clone() else {
                    self.status = "Select a project first".into();
                    return;
                };
                self.remove_project_id(&id);
            }
            Slash::Send(task) => self.dispatch_send(task),
            Slash::Sync => self.sync_hub(),
            Slash::Hub => {
                self.nav = Nav::Devices;
                self.status = if self.hub_on { "Hub sharing".into() } else { "Start share on Devices".into() };
            }
            Slash::Inhabit(peer) => self.queue_inhabit(peer),
            Slash::Rewind => self.rewind_project(),
            Slash::Room(name) => {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
                let plan = plan_room(&name, &home);
                let p = format!("{home}/{}", plan.project_rel);
                let _ = std::fs::create_dir_all(&p);
                self.cfg.project_dir = p.clone();
                self.project_sel = upsert_bound(&mut self.projects, &p);
                self.touch_projects();
                self.persist();
                self.status = format!("Room {} → {p}", plan.slug);
                self.queue_sh(plan.host_script);
            }
            Slash::Export => {
                if let Some(t) = self.threads.get(self.thread_idx) {
                    let md = threads::export_markdown(t);
                    let dest = if self.cfg.project_dir.trim().is_empty() {
                        config::config_dir().join("export.md")
                    } else {
                        std::path::PathBuf::from(&self.cfg.project_dir).join("export.md")
                    };
                    match std::fs::write(&dest, md) {
                        Ok(()) => self.status = format!("Wrote {}", dest.display()),
                        Err(e) => self.status = e.to_string(),
                    }
                }
            }
            Slash::Recall(q) => {
                let corpus = [
                    ("SOUL.md", config::read_memory("SOUL.md")),
                    ("USER.md", config::read_memory("USER.md")),
                    ("MEMORY.md", config::read_memory("MEMORY.md")),
                ];
                let refs: Vec<(&str, &str)> = corpus.iter().map(|(n, b)| (*n, b.as_str())).collect();
                let mut hits = recall_hits(&q, &refs);
                let mut rows: Vec<(String, String)> = corpus
                    .iter()
                    .map(|(n, b)| ((*n).to_string(), b.clone()))
                    .collect();
                for t in &self.threads {
                    let body = t
                        .messages
                        .iter()
                        .map(|(_, c)| c.as_str())
                        .collect::<Vec<_>>()
                        .join("\n");
                    rows.push((t.title.clone(), body));
                }
                hits.extend(search_corpus(&q, &rows));
                hits.sort();
                hits.dedup();
                let body = if hits.is_empty() {
                    format!("No recall for {q}")
                } else {
                    hits.join("\n")
                };
                self.messages.push(Msg {
                    role: "assistant".into(),
                    content: body,
                });
                self.persist();
            }
        }
    }

    fn kick_model_retry(&mut self, _t: String) {
        self.kick_model();
    }

    fn policy(&self) -> Policy {
        Policy::max()
    }

    fn commit_proposed_skill(&mut self, proposed: SkillMd) {
        let to_save = if let Some(name) = prefer_patch(&self.skill_list, &proposed) {
            if let Some(existing) = self.skill_list.iter().find(|s| s.name == name) {
                patch_skill(existing, &proposed)
            } else {
                proposed
            }
        } else {
            proposed
        };
        match skills::save_skill(&to_save) {
            Ok(_) => {
                self.skill_list = skills::list_skills();
                self.skill_name = to_save.name.clone();
                self.skill_body = grokhub_core::render_skill_md(&to_save);
                self.status = format!("Wrote skill {}", to_save.name);
            }
            Err(e) => self.status = e,
        }
    }

    fn queue_sh(&mut self, cmd: String) {
        self.run_cmds(vec![cmd]);
    }

    fn queue_inhabit(&mut self, peer: String) {
        let p = peer.to_ascii_lowercase();
        if p.contains("phone") || p.contains("android") {
            self.status = "will not inhabit onto the phone".into();
            return;
        }
        let paired = self
            .hub
            .lock()
            .ok()
            .map(|s| s.pair.is_some() || s.sharing)
            .unwrap_or(false);
        if !can_inhabit(paired, true, true) {
            self.status = "Inhabit needs a paired idle box".into();
            return;
        }
        let bundle = InhabitBundle {
            soul: config::read_memory("SOUL.md"),
            skill_ids: self.skill_list.iter().map(|s| s.name.clone()).collect(),
            goal: self.board.first().map(|c| c.title.clone()),
            project_snapshot_id: None,
            from_id: None,
            from_name: Some(self.cfg.device_name.clone()),
            at: Some(grokhub_core::now_ms()),
        };
        if let Ok(mut st) = self.hub.lock() {
            st.inhabit = Some(bundle);
        }
        self.status = format!("Inhabit staged for {peer}");
        self.nav = Nav::Devices;
    }

    fn rewind_project(&mut self) {
        let home = std::env::var("HOME").unwrap_or_default();
        let src = self.cfg.project_dir.trim();
        if src.is_empty() {
            self.status = "Bind a project first — /project bind".into();
            return;
        }
        if !rewind_allowed(src, &home) {
            self.status = "will not rewind $HOME unbound".into();
            return;
        }
        if let Some(last) = self.rewind_rows.first().cloned() {
            if std::path::Path::new(&last.path).exists() {
                self.queue_sh(format!(
                    "cp -a '{}' '{}'",
                    last.path.replace('\'', r#"'"'"'"#),
                    src.replace('\'', r#"'"'"'"#)
                ));
                self.status = format!("Restored {}", last.job_id);
                return;
            }
        }
        self.snapshot_project();
        self.status = "No snapshot yet — took one. /rewind again to restore.".into();
    }

    fn snapshot_project(&mut self) {
        let home = std::env::var("HOME").unwrap_or_default();
        let src = self.cfg.project_dir.trim().to_string();
        if !rewind_allowed(&src, &home) {
            return;
        }
        let id = uid("rw");
        let dest = rewind_dest(&config::config_dir().display().to_string(), &id);
        let _ = std::fs::create_dir_all(&dest);
        let quoted_src = src.replace('\'', r#"'"'"'"#);
        let quoted_dest = dest.replace('\'', r#"'"'"'"#);
        self.queue_sh(format!("cp -a '{quoted_src}/.' '{quoted_dest}'"));
        self.rewind_rows.insert(
            0,
            RewindRecord {
                job_id: id.clone(),
                path: dest,
                root: src,
                created_at: now_ms(),
                method: "copy".into(),
            },
        );
        self.rewind_rows = keep_last_rewinds(&self.rewind_rows, 5);
        self.last_rewind_id = Some(id);
        let _ = crate::night::save_rewinds(&self.rewind_rows);
    }

    fn doctor_text(&self) -> String {
        let mut lines = grokhub_core::doctor_lines(self.has_key(), true, HUB_KIND);
        lines.extend(grokhub_core::doctor_extras(
            self.last_receipt_ok,
            self.skill_list.len(),
        ));
        lines.push(doctor_hands_line(&crate::desktop::hands_chip_text()));
        lines
            .into_iter()
            .map(|l| format!("{} {}", if l.ok { "ok" } else { "ERR" }, l.text))
            .collect::<Vec<_>>()
            .join(" · ")
    }

    fn run_dream(&mut self) {
        let g = greet_from_last_job(
            if self.cfg.goal_pin.is_empty() {
                None
            } else {
                Some(self.cfg.goal_pin.as_str())
            },
            &self.last_receipts,
            self.last_rewind_id.as_deref(),
        );
        self.imagine_prompt = g.dream_prompt.clone();
        self.nav = Nav::Imagine;
        self.imagine_want_focus = true;
        self.status = g
            .goal
            .clone()
            .unwrap_or_else(|| "Dream of last night".into());
        self.messages.push(Msg {
            role: "assistant".into(),
            content: format!(
                "{}\n\n{}",
                g.goal.unwrap_or_else(|| "Last night".into()),
                g.dream_prompt
            ),
        });
        self.persist();
        self.kick_imagine();
    }

    fn dispatch_send(&mut self, task: String) {
        if self.hub_on {
            if let Ok(mut st) = self.hub.lock() {
                let id = st.device_id.clone();
                let name = st.device_name.clone();
                st.inbox.push(grokhub_core::task::HubTask::enqueue(
                    &id,
                    &name,
                    &id,
                    &task,
                    &task,
                    now_ms(),
                ));
            }
            self.status = "Task queued on hub".into();
            self.nav = Nav::Devices;
            return;
        }
        self.nav = Nav::Chat;
        self.send_chat(task);
    }

    fn sync_hub(&mut self) {
        let mem = ["SOUL.md", "USER.md", "MEMORY.md"]
            .into_iter()
            .map(|n| HubMemoryFile {
                name: n.into(),
                content: config::read_memory(n),
                updated_at: now_ms(),
            })
            .collect();
        let threads = self
            .threads
            .iter()
            .map(|t| {
                serde_json::json!({
                    "id": t.id,
                    "title": t.title,
                    "updatedAt": now_ms(),
                    "messages": t.messages.iter().map(|(r,c)| serde_json::json!({"role": r, "content": c})).collect::<Vec<_>>(),
                })
            })
            .collect();
        let skills = self
            .skill_list
            .iter()
            .map(|s| serde_json::json!({"id": s.name, "name": s.name, "updatedAt": now_ms()}))
            .collect();
        let autos = self
            .automations
            .iter()
            .filter_map(|a| serde_json::to_value(a).ok())
            .collect();
        let snap = build_hub_snapshot(
            &self
                .hub
                .lock()
                .ok()
                .map(|s| s.device_id.clone())
                .unwrap_or_default(),
            &self.cfg.device_name,
            now_ms(),
            threads,
            serde_json::json!({"items": self.board}),
            skills,
            autos,
            mem,
        );
        if let Ok(mut st) = self.hub.lock() {
            st.snapshot = serde_json::to_value(&snap).ok();
        }
        self.status = "Hub snapshot written — peers pull /v1/snapshot".into();
        self.apply_inbound_snapshot();
        self.nav = Nav::Devices;
    }

    fn local_clock() -> LocalClock {
        let out = std::process::Command::new("date")
            .arg("+%w %H %M")
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();
        parse_local_clock(&out, now_ms()).unwrap_or(LocalClock {
            now_ms: now_ms(),
            weekday: 1,
            hour: 12,
            minute: 0,
        })
    }

    fn local_day() -> String {
        std::process::Command::new("date")
            .arg("+%F")
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "1970-01-01".into())
    }

    fn tick_heartbeat(&mut self) {
        let elapsed = self.last_heartbeat.elapsed().as_millis() as u64;
        if !heartbeat_due(elapsed, HEARTBEAT_MS) {
            return;
        }
        self.last_heartbeat = Instant::now();
        let mut night_fired = false;
        for act in heartbeat_acts() {
            match act {
                HeartbeatAct::Housekeep => {
                    self.roll_today();
                    if self.nav == Nav::Chat && !self.scratch() {
                        self.stamp_current_access();
                    }
                    if self.last_persist.elapsed() > Duration::from_secs(2) {
                        self.persist();
                    }
                }
                HeartbeatAct::Inbox => self.drain_inbox(),
                HeartbeatAct::Night => night_fired = self.tick_night(),
                HeartbeatAct::Review => {
                    if !night_fired && !self.running {
                        self.tick_review();
                    }
                }
                HeartbeatAct::Wall => self.tick_wall(),
                HeartbeatAct::MidThought => self.tick_mid_thought(),
                HeartbeatAct::Reflect => {
                    if should_idle_reflect(
                        self.last_activity.elapsed().as_millis() as u64,
                        self.running,
                        IDLE_REFLECT_MS,
                    ) && !self.reflected_idle
                    {
                        self.reflected_idle = true;
                        self.run_reflect();
                    }
                }
                HeartbeatAct::Anticipate => self.tick_anticipate(),
            }
        }
    }

    fn tick_anticipate(&mut self) {
        let clock = Self::local_clock();
        let quiet = quiet_hours_active(&clock.hm(), &self.cfg.quiet_start, &self.cfg.quiet_end);
        if !should_anticipate(
            self.running,
            self.review_busy,
            self.composer.trim().is_empty(),
            quiet,
        ) {
            return;
        }
        let Some(prompt) = anticipated_need(
            &self.learning.insights,
            &self.skill_list,
            self.last_anticipate_ms,
            now_ms(),
            IDLE_REFLECT_MS,
        ) else {
            return;
        };
        self.last_anticipate_ms = now_ms();
        self.daily_auto_used = self.daily_auto_used.saturating_add(1);
        self.send_chat(prompt);
    }

    fn tick_night(&mut self) -> bool {
        if self.running || self.last_night_tick.elapsed() < Duration::from_secs(5) {
            return self.running || self.night_check_rx.is_some();
        }
        self.last_night_tick = Instant::now();
        let clock = Self::local_clock();
        let day = format!("{}-{}", clock.weekday, clock.hour);
        if self.daily_auto_day != day && clock.hour == 0 && clock.minute < 6 {
            self.daily_auto_used = 0;
            self.daily_auto_day = day;
        }
        let clock_copy = clock;
        self.automations = std::mem::take(&mut self.automations)
            .into_iter()
            .map(|a| ensure_automation_schedule(a, clock_copy))
            .collect();
        if self.poll_night_check(clock.now_ms) {
            return true;
        }
        let due = due_automations(&self.automations, clock.now_ms);
        let Some(a) = due.into_iter().next() else {
            return false;
        };
        if let Some(cmd) = night_check_command(&a.check_command) {
            self.spawn_night_check(a.id.clone(), a.name.clone(), cmd.to_string());
            return true;
        }
        self.fire_night(a, clock.now_ms);
        true
    }

    fn poll_night_check(&mut self, now_ms: u64) -> bool {
        let Some((id, rx)) = self.night_check_rx.take() else {
            return false;
        };
        match rx.try_recv() {
            Ok((out, _code)) => {
                if skip_night_check_receipt(&out) {
                    let name = self
                        .automations
                        .iter()
                        .find(|a| a.id == id)
                        .map(|a| a.name.clone())
                        .unwrap_or_else(|| id.clone());
                    self.mark_auto_ran(&id, now_ms);
                    self.status = format!("Night skipped {name} (check)");
                } else if let Some(a) = self.automations.iter().find(|x| x.id == id).cloned() {
                    self.fire_night(a, now_ms);
                }
                true
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.night_check_rx = Some((id, rx));
                true
            }
            Err(mpsc::TryRecvError::Disconnected) => false,
        }
    }

    fn spawn_night_check(&mut self, id: String, name: String, cmd: String) {
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let out = run_host(&cmd, Duration::from_secs(20));
            let code = night_check_exit_code(&out);
            let _ = tx.send((out, code));
        });
        self.night_check_rx = Some((id, rx));
        self.status = format!("Night check: {name}");
    }

    fn fire_night(&mut self, a: Automation, now_ms: u64) {
        self.mark_auto_ran(&a.id, now_ms);
        self.daily_auto_used = self.daily_auto_used.saturating_add(1);
        self.status = format!("Night: {}", a.name);
        if let Some(id) = replay_automation_target(&a.instructions) {
            self.replay_saved_recipe(id);
            return;
        }
        self.nav = Nav::Chat;
        self.send_chat(a.instructions);
    }

    fn tick_review(&mut self) {
        if self.review_busy {
            return;
        }
        let today = Self::local_day();
        if !review_due(
            self.suggestions.last_review_day.as_deref(),
            &today,
            &Self::local_clock(),
            REVIEW_NIGHT_HOUR,
        ) {
            return;
        }
        if !self.has_key() {
            return;
        }
        self.spawn_review();
    }

    fn review_digest(&self) -> String {
        let current = self
            .threads
            .get(self.thread_idx)
            .map(|t| t.id.as_str())
            .unwrap_or("");
        let mut thread_lines = Vec::new();
        for m in self.messages.iter().rev() {
            if m.role != "user" && m.role != "assistant" {
                continue;
            }
            thread_lines.push(DigestLine {
                role: m.role.clone(),
                text: m.content.clone(),
            });
            if thread_lines.len() >= 24 {
                break;
            }
        }
        for t in self.threads.iter().rev() {
            if t.id == current {
                continue;
            }
            for (role, text) in t.messages.iter().rev() {
                if role != "user" && role != "assistant" {
                    continue;
                }
                thread_lines.push(DigestLine {
                    role: role.clone(),
                    text: text.clone(),
                });
                if thread_lines.len() >= 40 {
                    break;
                }
            }
            if thread_lines.len() >= 40 {
                break;
            }
        }
        thread_lines.reverse();
        let mut host_receipts = self.last_host.clone();
        for (line, _) in self.last_receipts.iter().rev().take(6) {
            host_receipts.push(line.clone());
        }
        let input = ReviewDigest {
            insight_pin: insight_pin(&self.learning),
            user_md: config::read_memory("USER.md"),
            memory_md: config::read_memory("MEMORY.md"),
            skill_names: self.skill_list.iter().map(|s| s.name.clone()).collect(),
            automation_names: self.automations.iter().map(|a| a.name.clone()).collect(),
            github_pat: !self.secrets.github_token.trim().is_empty(),
            host_receipts,
            chip_habits: top_habit_labels(&self.chip_memory, 6),
            thread_lines,
        };
        build_review_digest(&input)
    }

    fn spawn_review(&mut self) {
        if self.review_busy {
            return;
        }
        let key = self.bearer();
        if key.trim().is_empty() {
            return;
        }
        let digest = self.review_digest();
        let model = model_for_mode("balanced").to_string();
        let prompt = review_system_prompt().to_string();
        let (tx, rx) = mpsc::channel();
        self.review_rx = Some(rx);
        self.review_busy = true;
        std::thread::spawn(move || {
            let messages = [("system".into(), prompt), ("user".into(), digest)];
            let out = grok_chat(&key, &model, &messages, None, None);
            let _ = tx.send(out);
        });
    }

    fn poll_review(&mut self) {
        let Some(rx) = self.review_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(raw) => {
                self.review_busy = false;
                self.apply_review_reply(raw);
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.review_rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.review_busy = false;
            }
        }
    }

    fn apply_review_reply(&mut self, raw: Result<String, String>) {
        match raw {
            Ok(text) => {
                let skill_names: Vec<String> =
                    self.skill_list.iter().map(|s| s.name.clone()).collect();
                let auto_names: Vec<String> =
                    self.automations.iter().map(|a| a.name.clone()).collect();
                let items = dedupe_suggestions(
                    parse_suggest_lines(&text),
                    &skill_names,
                    &auto_names,
                    &[],
                );
                let day = Some(Self::local_day());
                let ms = now_ms();
                if items.is_empty() {
                    self.suggestions.last_review_day = day;
                    self.suggestions.last_review_ms = ms;
                } else {
                    let mut store = partition_suggestions(items);
                    store.last_review_day = day;
                    store.last_review_ms = ms;
                    self.suggestions = store;
                }
                let _ = crate::store::save_suggestions(&self.suggestions);
            }
            Err(e) => {
                self.status = format!("Nightly review held — {e}");
            }
        }
    }

    fn poll_wall(&mut self) {
        let Some(rx) = self.wall_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(gif)) => {
                self.wall_busy = false;
                self.wall.last_ms = now_ms();
                self.wall.gifs.push(gif.clone());
                let (kept, evicted) = wall_evict(std::mem::take(&mut self.wall.gifs), WALL_GIF_MAX);
                self.wall.gifs = kept;
                for old in evicted {
                    let _ = std::fs::remove_file(&old.path_a);
                    let _ = std::fs::remove_file(&old.path_b);
                }
                self.status = format!("New cover on the wall — {}", gif.title);
                self.persist();
            }
            Ok(Err(e)) => {
                self.wall_busy = false;
                self.wall.last_ms = now_ms()
                    .saturating_sub(WALL_GIF_EVERY_MS)
                    .saturating_add(15 * 60 * 1000);
                self.status = format!("Wall cover held — {e}");
                let _ = crate::store::save_wall(&self.wall);
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.wall_rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.wall_busy = false;
            }
        }
    }

    fn tick_wall(&mut self) {
        let clock = Self::local_clock();
        let quiet = quiet_hours_active(&clock.hm(), &self.cfg.quiet_start, &self.cfg.quiet_end);
        if !wall_can_paint(
            self.has_key(),
            self.cfg.imagine_wall,
            self.wall_busy,
            self.running,
            quiet,
            self.wall.last_ms,
            now_ms(),
        ) {
            return;
        }
        self.kick_wall();
    }

    fn kick_wall(&mut self) {
        if self.wall_busy {
            return;
        }
        let taken: Vec<String> = self.wall.gifs.iter().map(|g| g.title.clone()).collect();
        let taken_ref: Vec<&str> = taken.iter().map(|s| s.as_str()).collect();
        let seed = pick_fresh_seed(now_ms(), &taken_ref);
        let id = format!("{:x}", now_ms());
        let dir = config::wall_dir();
        let key = self.bearer();
        let model = dedicated_imagine_model(&self.cfg.imagine_model);
        let title = seed.title.to_string();
        let prompt = seed.prompt.to_string();
        let prompt_b = seed.prompt_b.to_string();
        let tall = seed.tall;
        let created_ms = now_ms();
        let (tx, rx) = mpsc::channel();
        self.wall_rx = Some(rx);
        self.wall_busy = true;
        self.status = format!("Painting a wall cover — {title}");
        std::thread::spawn(move || {
            let _ = tx.send(paint_wall_cover(
                &key, &model, &id, &dir, &title, &prompt, &prompt_b, tall, created_ms,
            ));
        });
    }

    fn import_openclaw(&mut self) {
        let home = std::env::var("HOME").unwrap_or_default();
        let mut root = None;
        for p in default_openclaw_paths(&home) {
            let names: Vec<String> = std::fs::read_dir(&p)
                .ok()
                .map(|rd| {
                    rd.flatten()
                        .map(|e| e.file_name().to_string_lossy().into_owned())
                        .collect()
                })
                .unwrap_or_default();
            let refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
            if is_openclaw_workspace(&refs) {
                root = Some(p);
                break;
            }
        }
        let Some(root) = root else {
            self.status = "No OpenClaw workspace (~/.openclaw/workspace)".into();
            return;
        };
        let mut imported = 0u32;
        if let Ok(rd) = std::fs::read_dir(&root) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if let Ok(body) = std::fs::read_to_string(e.path()) {
                    if let Some((dest, content)) = import_memory_file(&name, &body) {
                        if config::write_memory(&dest, &content).is_ok() {
                            imported += 1;
                        }
                    }
                }
            }
        }
        let skills_dir = std::path::PathBuf::from(&root).join("skills");
        if let Ok(rd) = std::fs::read_dir(skills_dir) {
            for e in rd.flatten() {
                let md = e.path().join("SKILL.md");
                if let Ok(raw) = std::fs::read_to_string(md) {
                    let parsed = grokhub_core::parse_skill_md(&raw);
                    if !parsed.name.is_empty() && skills::save_skill(&parsed).is_ok() {
                        imported += 1;
                    }
                }
            }
        }
        self.skill_list = skills::list_skills();
        self.status = format!("Imported {imported} files from {root}");
        self.nav = Nav::Memory;
    }

    fn run_consult(&mut self, q: String) {
        if !self.has_key() {
            self.status = "Connect Grok OAuth in Settings".into();
            return;
        }
        self.running = true;
        self.chat_job_thread = None;
        self.status = "Consult…".into();
        let key = self.bearer();
        let model = model_for_mode("fast").to_string();
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        std::thread::spawn(move || {
            let r = grok_chat(&key, &model, &[("user".into(), q.clone())], None, None);
            let _ = tx.send(match r {
                Ok(t) => JobOut::Consult(format_consult_reply(&q, &t)),
                Err(e) => JobOut::Err(e),
            });
        });
    }

    fn open_palette(&mut self) {
        self.palette_open = true;
        self.palette_focus = true;
        self.palette_pick = 0;
        self.palette_q.clear();
        self.settings_menu_open = false;
    }

    fn run_palette(&mut self, action: &str) {
        self.palette_open = false;
        self.settings_menu_open = false;
        match action {
            "nav:chat" => self.nav = Nav::Chat,
            "nav:night" => self.nav = Nav::Night,
            "nav:history" => self.nav = Nav::History,
            "nav:devices" => self.nav = Nav::Devices,
            "nav:connectors" => self.nav = Nav::Connectors,
            "nav:command" => self.nav = Nav::Command,
            "nav:agents" => self.nav = Nav::Agents,
            "nav:eyes" => self.nav = Nav::Eyes,
            "nav:skills" => self.nav = Nav::Skills,
            "nav:board" => self.nav = Nav::Workboard,
            "nav:imagine" => {
                self.imagine_want_focus = true;
                self.nav = Nav::Imagine;
            }
            "nav:memory" => self.nav = Nav::Memory,
            "nav:settings" => self.nav = Nav::Settings,
            "oauth" => self.start_oauth(),
            "diag" => {
                self.status = diagnostics_bundle(
                    env!("CARGO_PKG_VERSION"),
                    self.has_key(),
                    HUB_KIND,
                    self.skill_list.len(),
                    self.last_receipt_ok,
                    self.board.len(),
                    &self.status,
                );
            }
            "voice" => self.listen_voice(),
            slash if slash.starts_with('/') => self.run_slash_line(slash),
            _ => {}
        }
    }

    fn run_slash_line(&mut self, line: &str) {
        if let Some(s) = parse_slash(line) {
            self.run_slash(s);
        }
    }

    fn apply_inbound_snapshot(&mut self) {
        let Some(remote_v) = self.hub.lock().ok().and_then(|s| s.snapshot.clone()) else {
            return;
        };
        let Ok(remote) = serde_json::from_value::<HubSnapshot>(remote_v) else {
            return;
        };
        for f in remote.memory_files {
            if import_memory_file(&f.name, &f.content).is_some() {
                let _ = config::write_memory(&f.name, &f.content);
            }
        }
        self.status = format!("Merged hub snapshot from {}", remote.from_device_name);
    }

    fn push_presence(&mut self, url: String) {
        let now = now_ms();
        self.presence_ring.push((now, url));
        self.presence_ring
            .retain(|(ts, _)| should_keep_frame(*ts, now, PRESENCE_RING_MS));
    }

    fn live_room(&mut self) {
        if self.last_live.elapsed() < Duration::from_millis(900) {
            return;
        }
        self.last_live = Instant::now();
        if !presence_should_stream(self.running, self.nav == Nav::Eyes || self.nav == Nav::Command)
        {
            return;
        }
        if let Ok(url) = capture_data_url() {
            if should_send_screenshot(&self.last_window_title, "") {
                if let Ok(mut st) = self.hub.lock() {
                    st.store_frame(&url);
                }
                self.last_frame_url = Some(url.clone());
                self.push_presence(url);
            }
        }
        if let Ok(cam) = capture_webcam() {
            self.webcam_url = Some(cam);
        }
    }

    fn tick_mid_thought(&mut self) {
        if !self.last_receipts.is_empty() || !self.rewind_rows.is_empty() {
            return;
        }
        self.continue_hint = threads::continue_thread_hint(&self.threads);
    }

    fn last_night_hint(&self) -> String {
        if self.last_receipts.is_empty() && self.rewind_rows.is_empty() {
            return self.continue_hint.chars().take(80).collect();
        }
        let g = greet_from_last_job(
            if self.cfg.goal_pin.is_empty() {
                None
            } else {
                Some(self.cfg.goal_pin.as_str())
            },
            &self.last_receipts,
            self.last_rewind_id.as_deref(),
        );
        let mut bits = Vec::new();
        if let Some(goal) = g.goal {
            bits.push(goal);
        }
        if let Some(fail) = g.last_fail {
            bits.push(format!("failed: {fail}"));
        }
        bits.join(" · ").chars().take(80).collect()
    }

    fn mark_auto_ran(&mut self, id: &str, now: u64) {
        if let Some(a) = self.automations.iter_mut().find(|x| x.id == id) {
            *a = mark_automation_ran(a.clone(), now);
        }
        let _ = crate::night::save(&self.automations);
    }

    fn start_oauth(&mut self) {
        match crate::oauth::start_device() {
            Ok(start) => {
                let uri = start
                    .verification_uri_complete
                    .clone()
                    .unwrap_or_else(|| start.verification_uri.clone());
                let _ = crate::oauth::open_browser(&uri);
                self.status = format!("Grok OAuth code {} — approve in the browser", start.user_code);
                self.oauth_pending = Some(start);
            }
            Err(e) => self.status = e,
        }
    }

    fn poll_oauth(&mut self) {
        let Some(p) = self.oauth_pending.clone() else {
            return;
        };
        match crate::oauth::poll_device(&p.device_code) {
            Ok(r) => match r.status {
                grokhub_core::PollStatus::Ready => {
                    if let Some(t) = r.tokens {
                        self.secrets.oauth = Some(t);
                        let _ = secrets::save(&self.secrets);
                        self.oauth_pending = None;
                        self.oauth_profile_tried = false;
                        self.oauth_photo = None;
                        self.oauth_photo_key.clear();
                        self.status = "Grok OAuth connected".into();
                    }
                }
                grokhub_core::PollStatus::Expired | grokhub_core::PollStatus::Denied => {
                    self.oauth_pending = None;
                    self.status = r.error.unwrap_or_else(|| "OAuth failed".into());
                }
                grokhub_core::PollStatus::SlowDown | grokhub_core::PollStatus::Pending => {}
            },
            Err(e) => self.status = e,
        }
    }

    fn clear_oauth_photo(&mut self) {
        self.oauth_photo = None;
        self.oauth_photo_key.clear();
        self.oauth_photo_rx = None;
        self.oauth_photo_busy = false;
        self.oauth_profile_tried = false;
    }

    fn sign_out_oauth(&mut self) {
        self.secrets.oauth = None;
        self.clear_oauth_photo();
        let _ = secrets::save(&self.secrets);
        self.status = "Signed out".into();
    }

    fn poll_oauth_photo(&mut self, ctx: &egui::Context) {
        if let Some(rx) = self.oauth_photo_rx.take() {
            match rx.try_recv() {
                Ok(out) => {
                    self.oauth_photo_busy = false;
                    self.oauth_profile_tried = true;
                    if let Some(tokens) = out.tokens {
                        let changed = self.secrets.oauth.as_ref() != Some(&tokens);
                        self.secrets.oauth = Some(tokens);
                        if changed {
                            let _ = secrets::save(&self.secrets);
                        }
                    }
                    self.oauth_photo_key = out.url;
                    self.oauth_photo = out.bytes.as_ref().and_then(|b| {
                        oauth_photo_image(b).map(|img| {
                            ctx.load_texture("oauth-avatar", img, TextureOptions::LINEAR)
                        })
                    });
                }
                Err(mpsc::TryRecvError::Empty) => {
                    self.oauth_photo_rx = Some(rx);
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.oauth_photo_busy = false;
                }
            }
        }
        self.kick_oauth_photo();
    }

    fn kick_oauth_photo(&mut self) {
        if self.oauth_photo_busy {
            return;
        }
        let Some(tok) = self.secrets.oauth.clone() else {
            if self.oauth_photo.is_some() || !self.oauth_photo_key.is_empty() {
                self.clear_oauth_photo();
            }
            return;
        };
        let url = tok
            .picture
            .as_ref()
            .and_then(|u| grokhub_core::trusted_profile_photo_url(u).ok())
            .unwrap_or_default();
        if !url.is_empty() && url == self.oauth_photo_key {
            return;
        }
        if url.is_empty() && self.oauth_profile_tried {
            return;
        }
        self.oauth_photo_busy = true;
        let (tx, rx) = mpsc::channel();
        self.oauth_photo_rx = Some(rx);
        std::thread::spawn(move || {
            let tokens = match crate::oauth::ensure_access(&tok) {
                Ok((_, t, _)) => crate::oauth::enrich_tokens(t),
                Err(_) => crate::oauth::enrich_tokens(tok),
            };
            let url = tokens
                .picture
                .as_ref()
                .and_then(|u| grokhub_core::trusted_profile_photo_url(u).ok())
                .unwrap_or_default();
            let bytes = if url.is_empty() {
                None
            } else {
                crate::oauth::fetch_profile_photo(&url, &tokens.access_token).ok()
            };
            let _ = tx.send(OauthPhotoOut {
                tokens: Some(tokens),
                url,
                bytes,
            });
        });
    }

    fn kick_model(&mut self) {
        if !self.has_key() {
            self.status = "Connect Grok OAuth in Settings".into();
            return;
        }
        self.running = true;
        self.status = "Thinking…".into();
        self.chat_job_thread = Some(self.visible_thread_id());
        let key = self.bearer();
        let last_user = self
            .messages
            .iter()
            .rev()
            .find(|m| m.role == "user" && !is_workload_user(&m.content))
            .map(|m| m.content.as_str())
            .unwrap_or("");
        let mode = effective_chat_mode(&self.cfg.mode, last_user, &self.cfg.model);
        let model = resolve_chat_model(&mode, &self.cfg.model);
        let effort = if model == "grok-4.6" {
            grokhub_core::reasoning_effort_for_mode(&mode)
        } else {
            None
        };
        let mut msgs: Vec<(String, String)> = self
            .messages
            .iter()
            .map(|m| {
                let content = if m.role == "assistant" {
                    strip_thinking(&m.content)
                } else {
                    m.content.clone()
                };
                (m.role.clone(), content)
            })
            .collect();
        let soul = config::read_memory("SOUL.md");
        let user_md = config::read_memory("USER.md");
        let memory_md = config::read_memory("MEMORY.md");
        let pins = skills::pin_text(&self.skill_list);
        let mut board = String::new();
        for c in self.board.iter().take(8) {
            board.push_str(&format!("- [{}] {}\n", c.status.as_str(), c.title));
        }
        let last_host = self
            .last_host
            .last()
            .map(|h| h.chars().take(400).collect::<String>())
            .unwrap_or_default();
        let pin = insight_pin(&self.learning);
        if user_asks_desktop_hands(last_user) || user_asks_takeover(last_user) {
            self.hands_attach = true;
            self.eyes_attach = true;
        }
        if self.cfg.cabin_eyes && user_asks_cabin_eyes(last_user) {
            self.eyes_attach = true;
        }
        let mut hands = hands_protocol().to_string();
        if self.eyes_attach || self.hands_attach {
            let rows = collect_rows();
            let titles: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
            let frame = build_windshield(
                &rows,
                None,
                refused_lock(&titles),
                self.board.first().map(|c| c.title.as_str()),
                self.skill_list.first().map(|s| s.name.as_str()),
                4,
            );
            let glass = windshield_prompt(&frame);
            if !glass.is_empty() {
                hands.push_str("\n\nWindshield:\n");
                hands.push_str(&glass);
            }
        }
        let sys = cabin_system_prompt(
            &soul,
            &user_md,
            &memory_md,
            &pins,
            self.active_skill_follow.as_deref(),
            &self.cfg.goal_pin,
            &board,
            &last_host,
            &hands,
            &pin,
        );
        if !sys.is_empty() {
            msgs.insert(0, ("system".into(), sys));
        }
        self.ensure_cabin_frame();
        let user_img = self.attach_url.take();
        self.attach_name = None;
        let fused = self.last_frame_url.clone().or_else(|| self.webcam_url.clone());
        let eyes_turn = self.eyes_attach;
        let hands_turn = self.hands_attach;
        self.eyes_attach = false;
        self.hands_attach = false;
        let cabin = if should_attach_hands_frame(eyes_turn, hands_turn, fused.is_some()) {
            fused
        } else {
            None
        };
        let image = next_chat_image(user_img.as_deref(), cabin.as_deref()).map(|s| s.to_string());
        if cabin_frame_only(user_img.as_deref(), cabin.as_deref()) {
            if let Some((_, content)) = msgs
                .iter_mut()
                .rev()
                .find(|(role, c)| role == "user" && !is_workload_user(c))
            {
                *content = cabin_eyes_request_text(content);
            }
        }
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        self.stream_buf.clear();
        self.thought_buf.clear();
        std::thread::spawn(move || {
            let tx_d = tx.clone();
            let r = grok_chat_stream(
                &key,
                &model,
                &msgs,
                image.as_deref(),
                effort,
                |d| {
                    let _ = tx_d.send(JobOut::ChatDelta(d.to_string()));
                },
                |t| {
                    let _ = tx_d.send(JobOut::ThoughtDelta(t.to_string()));
                },
            );
            let r = match r {
                Err(e) => {
                    if http_status_of(&e).map(should_failover_status).unwrap_or(false) {
                        if let Some(next) = failover_model(&model) {
                            grok_chat(&key, next, &msgs, image.as_deref(), None)
                                .map(|t| (t, false))
                                .map_err(|e2| format!("{e}; failover {next}: {e2}"))
                        } else {
                            Err(e)
                        }
                    } else {
                        Err(e)
                    }
                }
                ok => ok,
            };
            let _ = tx.send(match r {
                Ok((t, truncated)) => JobOut::Chat { text: t, truncated },
                Err(e) => JobOut::Err(e),
            });
        });
    }

    fn upsert_stream_assistant(&mut self) {
        self.apply_live_assistant();
    }

    fn poll_job(&mut self) {
        let Some(rx) = self.rx.take() else { return };
        match rx.try_recv() {
            Ok(JobOut::ChatDelta(d)) => {
                self.rx = Some(rx);
                self.stream_buf.push_str(&d);
                if chat_stream_is_visible(
                    self.chat_job_thread.as_deref(),
                    &self.visible_thread_id(),
                ) {
                    self.status = "Thinking…".into();
                }
                self.upsert_stream_assistant();
            }
            Ok(JobOut::ThoughtDelta(d)) => {
                self.rx = Some(rx);
                self.thought_buf.push_str(&d);
                if chat_stream_is_visible(
                    self.chat_job_thread.as_deref(),
                    &self.visible_thread_id(),
                ) {
                    self.status = "Thinking…".into();
                }
                self.upsert_stream_assistant();
            }
            Ok(JobOut::Chat { text, truncated }) => {
                let here = chat_stream_is_visible(
                    self.chat_job_thread.as_deref(),
                    &self.visible_thread_id(),
                );
                self.running = false;
                self.voice_orb = "idle".into();
                if here {
                    self.status.clear();
                }
                remember_chip_outcome(&mut self.chip_memory, true, now_ms());
                record_turn(&mut self.learning);
                if self.policy().learns() && !self.scratch() {
                    let facts = fact_candidates(
                        &self
                            .messages
                            .iter()
                            .map(|m| (m.role.clone(), m.content.clone()))
                            .collect::<Vec<_>>(),
                    );
                    extract_insights(&mut self.learning, &facts);
                }
                bump_usage(&mut self.usage, "message");
                let text = if self.thought_buf.is_empty() {
                    text
                } else {
                    merge_thinking(&self.thought_buf, &strip_thinking(&text))
                };
                self.apply_assistant_snapshot(text.clone());
                self.thought_buf.clear();
                self.stream_buf.clear();
                self.chat_job_thread = None;
                if here && self.speak_next {
                    self.speak_next = false;
                    self.speak_reply(&text);
                }
                self.persist();
                for card in extract_work_pins(&text) {
                    self.board.push(card);
                }
                if let Some(r) = parse_recipe(&text) {
                    self.last_recipe = Some(r);
                }
                if has_verify_ok(&text) {
                    self.verify_ok_turn = true;
                    self.verify_chip = "VERIFY_OK".into();
                }
                for (key, st) in extract_work_updates(&text) {
                    let _ = apply_work_update(&mut self.board, &key, st);
                }
                if here {
                    if let Some(plan) = plan_from_text(&text) {
                        self.pending_update = false;
                        self.run_cmds(approved_cmds(&plan));
                    }
                    for c in extract_connector_cmds(&text) {
                        self.run_connector(&c.connector_id, &c.tool, &c.args);
                    }
                    if let Some(p) = extract_imagine_prompt(&text) {
                        self.imagine_prompt = p;
                        self.nav = Nav::Imagine;
                        self.imagine_want_focus = true;
                        self.kick_imagine();
                    }
                }
                if let Some(mut a) = parse_nl_automation(&text) {
                    if a.id.is_empty() {
                        a.id = uid("auto");
                    }
                    self.automations.push(a.clone());
                    let _ = crate::night::save(&self.automations);
                    if here {
                        self.status = format!("Night saved: {} {}", a.schedule, a.time);
                    }
                }
                if here {
                    if let Some(q) = parse_consult(&text) {
                        self.run_consult(q);
                    }
                }
                let outcome = parse_goal_outcome(&text);
                if here && outcome == "continue" && !self.cfg.goal_pin.is_empty() {
                    if let Some(next) = next_goal_prompt(
                        &self.cfg.goal_pin,
                        &text,
                        self.goal_step,
                        GOAL_MAX_STEPS,
                    ) {
                        self.goal_step = self.goal_step.saturating_add(1);
                        self.agents.push(AgentJob {
                            title: format!("{} · step {}", self.cfg.goal_pin, self.goal_step + 1),
                            status: "queued".into(),
                            prompt: next.clone(),
                        });
                        self.send_chat(next);
                    }
                } else if outcome != "continue" {
                    self.goal_step = 0;
                }
                if here {
                    let tokens = estimate_messages(
                        &self
                            .messages
                            .iter()
                            .map(|m| (m.role.clone(), m.content.clone()))
                            .collect::<Vec<_>>(),
                    );
                    if should_auto_compact(tokens, CONTEXT_BUDGET_TOKENS) {
                        self.run_slash(Slash::Compact);
                        self.status = format!(
                            "Auto-compact · {}% context",
                            context_percent(tokens, CONTEXT_BUDGET_TOKENS)
                        );
                    }
                    self.spawn_thread_goal();
                }
                if here
                    && !self.running
                    && self.followup_step < FOLLOWUP_MAX_STEPS
                    && reply_needs_followup(
                        &last_user_text(
                            &self
                                .messages
                                .iter()
                                .map(|m| (m.role.clone(), m.content.clone()))
                                .collect::<Vec<_>>(),
                        )
                        .unwrap_or_default(),
                        &text,
                        truncated,
                    )
                {
                    self.send_followup_turn();
                }
            }
            Ok(JobOut::Consult(detail)) => {
                self.running = false;
                self.messages.push(Msg {
                    role: "assistant".into(),
                    content: detail,
                });
                self.persist();
            }
            Ok(JobOut::HostLine(line)) => {
                self.rx = Some(rx);
                self.host_live = line.clone();
                self.status = "Host…".into();
                self.voice_orb = "hands".into();
            }
            Ok(JobOut::HostDone(block)) => {
                self.running = false;
                self.voice_orb = "idle".into();
                self.host_live.clear();
                let ok = !crate::update::host_receipt_failed(&block);
                self.last_receipt_ok = Some(ok);
                self.last_receipts.push((block.chars().take(160).collect(), ok));
                if self.last_receipts.len() > 12 {
                    self.last_receipts.remove(0);
                }
                if let Some(cite) = summarize_write(
                    self.last_host.last().map(|s| s.as_str()).unwrap_or(""),
                    &block,
                ) {
                    self.status = cite;
                }
                let all_hands = !self.last_host.is_empty()
                    && self
                        .last_host
                        .iter()
                        .all(|c| parse_computer_cmd_loose(c).is_some());
                let any_hands = self
                    .last_host
                    .iter()
                    .any(|c| parse_computer_cmd_loose(c).is_some());
                let prefix = if all_hands {
                    "COMPUTER_RESULT (facts only):"
                } else {
                    "HOST_RESULT (facts only):"
                };
                self.messages.push(Msg {
                    role: "user".into(),
                    content: format!("{prefix}\n{block}"),
                });
                if any_hands {
                    self.hands_attach = true;
                    let rows = collect_rows();
                    let titles: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
                    if !lock_blocks_hands(&titles) {
                        match capture_data_url() {
                            Ok(url) => {
                                if let Ok(mut st) = self.hub.lock() {
                                    st.store_frame(&url);
                                }
                                self.last_frame_url = Some(url);
                            }
                            Err(_) => {}
                        }
                    }
                    if let Some(recipe) = recipe_from_cmds(&self.last_host, screen_from_rows(&rows)) {
                        if crate::recipes::save_recipe("last", &recipe).is_ok() {
                            self.last_recipe = Some(recipe);
                        }
                    }
                    if !self.scratch() {
                        let user = last_user_text(
                            &self
                                .messages
                                .iter()
                                .map(|m| (m.role.clone(), m.content.clone()))
                                .collect::<Vec<_>>(),
                        )
                        .unwrap_or_default();
                        let proposed = propose_skill_from_turn(&user, &block, &self.last_host);
                        self.commit_proposed_skill(proposed);
                    }
                }
                self.persist();
                self.run_skill_verify();
                bump_usage(&mut self.usage, "host");
                if let Some(cite) = summarize_write(
                    self.last_host.last().map(|s| s.as_str()).unwrap_or(""),
                    &block,
                ) {
                    if let Some(path) = cite.split_whitespace().last() {
                        if let Ok(after) = std::fs::read_to_string(path) {
                            let diff = unified_diff_cite(path, "", &after);
                            self.messages.push(Msg {
                                role: "user".into(),
                                content: format!("HOST_DIFF:\n{diff}"),
                            });
                        }
                    }
                }
                if !any_hands
                    && is_hard_run(self.last_host.len() as u32, !ok, false, self.scratch())
                {
                    let user = last_user_text(
                        &self
                            .messages
                            .iter()
                            .map(|m| (m.role.clone(), m.content.clone()))
                            .collect::<Vec<_>>(),
                    )
                    .unwrap_or_default();
                    let proposed = propose_skill_from_turn(&user, &block, &self.last_host);
                    self.commit_proposed_skill(proposed);
                }
                self.kick_model();
            }
            Ok(JobOut::Connector(detail)) => {
                self.running = false;
                self.messages.push(Msg {
                    role: "user".into(),
                    content: format!("CONNECTOR_RESULT (facts only):\n{detail}"),
                });
                self.persist();
                if !self.pending_connectors.is_empty() {
                    let (id, tool, args) = self.pending_connectors.remove(0);
                    self.run_connector(&id, &tool, &args);
                } else {
                    self.kick_model();
                }
            }
            Ok(JobOut::Imagine(url)) => {
                self.running = false;
                self.imagine_last = url.clone();
                self.status = "Imagine ready".into();
                self.messages.push(Msg {
                    role: "assistant".into(),
                    content: format!("IMAGINE: {url}"),
                });
                self.persist();
            }
            Ok(JobOut::Voice(t)) => {
                self.running = false;
                self.voice_orb = "idle".into();
                if is_voice_error(&t) {
                    self.status = t;
                } else {
                    self.status = "Hey Grok".into();
                    self.speak_next = true;
                    self.send_chat(t);
                }
            }
            Ok(JobOut::UpdateProgress { pct, msg }) => {
                self.rx = Some(rx);
                self.update_pct = Some(pct);
                self.update_can_restart = false;
                self.status = msg;
            }
            Ok(JobOut::UpdateDone { ok }) => {
                self.running = false;
                self.last_receipt_ok = Some(ok);
                let view = overlay_update_finish(ok, self.update_pct.unwrap_or(0));
                self.update_pct = Some(view.pct);
                self.update_can_restart = view.can_restart;
                self.status = view.status;
            }
            Ok(JobOut::Err(e)) => {
                self.running = false;
                self.voice_orb = "idle".into();
                self.chat_job_thread = None;
                self.stream_buf.clear();
                self.thought_buf.clear();
                remember_chip_outcome(&mut self.chip_memory, false, now_ms());
                self.status = e;
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.running = false;
                self.chat_job_thread = None;
                self.stream_buf.clear();
                self.thought_buf.clear();
            }
        }
    }

    fn run_cmds(&mut self, cmds: Vec<String>) {
        if self.running {
            self.status = "Busy — wait, then host".into();
            return;
        }
        if cmds.is_empty() {
            return;
        }
        self.snapshot_project();
        self.last_host = cmds.clone();
        self.host_halt.store(false, Ordering::SeqCst);
        self.running = true;
        self.chat_job_thread = None;
        self.voice_orb = "hands".into();
        self.host_live = cmds[0].clone();
        self.status = "Host…".into();
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let clock = Self::local_clock();
        let quiet = quiet_hours_active(&clock.hm(), &self.cfg.quiet_start, &self.cfg.quiet_end);
        let halt = self.host_halt.clone();
        std::thread::spawn(move || {
            let started = Instant::now();
            let mut inhibit = crate::notify::inhibit_sleep();
            let mut block = String::new();
            for c in &cmds {
                if halt.load(Ordering::SeqCst) {
                    block.push_str("HOST_RECEIPT: halted\n");
                    break;
                }
                let tx_line = tx.clone();
                let cmd = c.clone();
                let receipt = if let Some(op) = parse_computer_cmd_loose(c) {
                    let _ = tx_line.send(JobOut::HostLine(format!(
                        "Hands: {}",
                        computer_cmd_line(&op)
                    )));
                    run_computer_op_cancel(&op, Some(&halt))
                } else {
                    run_host_stream(c, Duration::from_secs(90), Some(&halt), move |line| {
                        let _ = tx_line.send(JobOut::HostLine(host_status_line(&cmd, line, 0)));
                    })
                };
                if let Some(cite) = summarize_write(c, &receipt) {
                    block.push_str(&cite);
                    block.push('\n');
                }
                block.push_str(&redact_secrets(&receipt));
                block.push_str("\n\n");
            }
            crate::notify::release_inhibit(&mut inhibit);
            crate::notify::ping_if_long_quiet(
                started.elapsed(),
                quiet,
                "GrokHub",
                "Host job finished",
            );
            let _ = tx.send(JobOut::HostDone(block));
        });
    }

    fn run_connector(&mut self, id: &str, tool: &str, args: &str) {
        if id != "github" {
            self.messages.push(Msg {
                role: "user".into(),
                content: format!(
                    "CONNECTOR_RESULT (facts only):\n{id} {tool} — not wired. GitHub is the only live connector."
                ),
            });
            return;
        }
        if self.running {
            self.pending_connectors
                .push((id.to_string(), tool.to_string(), args.to_string()));
            return;
        }
        self.running = true;
        self.chat_job_thread = None;
        self.status = format!("GitHub {tool}…");
        let token = self.secrets.github_token.clone();
        let tool = tool.to_string();
        let args = args.to_string();
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        std::thread::spawn(move || {
            let detail = crate::github::run_github_tool(&tool, &args, &token);
            let _ = tx.send(JobOut::Connector(detail));
        });
    }

    fn kick_imagine(&mut self) {
        if !self.has_key() {
            self.status = "Connect Grok OAuth in Settings".into();
            return;
        }
        let prompt = self.imagine_prompt.trim().to_string();
        if prompt.is_empty() || self.running {
            return;
        }
        let prompt = compose_imagine_prompt(&ImagineSpec {
            prompt: &prompt,
            kind: self.imagine_kind,
            quality: self.imagine_quality,
            style: imagine_style_label(self.imagine_style),
            aspect: imagine_aspect_label(self.imagine_aspect),
            video_res: imagine_video_res_label(self.imagine_video_res),
            video_dur: imagine_video_dur_label(self.imagine_video_dur),
            video_audio: self.imagine_video_audio,
        });
        self.running = true;
        self.chat_job_thread = None;
        self.status = match self.imagine_kind {
            ImagineKind::Image => "Imagining…".into(),
            ImagineKind::Video => "Imagining storyboard still…".into(),
            ImagineKind::Agent => "Imagining agent still…".into(),
        };
        let key = self.bearer();
        let model = dedicated_imagine_model(&self.cfg.imagine_model);
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        std::thread::spawn(move || {
            let r = grok_imagine(&key, &model, &prompt);
            let _ = tx.send(match r {
                Ok(u) => JobOut::Imagine(u),
                Err(e) => JobOut::Err(e),
            });
        });
    }

    fn listen_voice(&mut self) {
        let action = hey_grok_on_press(self.voice_state, self.running);
        if action == HeyGrokAction::Halt {
            self.halt_work("Hands on — halted");
            return;
        }
        let oauth = secrets::access_token(&self.secrets);
        let speech = auth_bearer(&self.cfg.api_key, &oauth);
        let has_local = first_bin(TRANSCRIBERS).is_some();
        let route = hey_grok_route(
            realtime_can_connect(&self.cfg.api_key),
            speech.as_ref().is_some_and(|s| !s.is_empty()),
            has_local,
        );
        if self.voice_sock.is_none() {
            match route {
                HeyGrokRoute::Realtime => {
                    if let Some(key) = realtime_bearer(&self.cfg.api_key, &oauth) {
                        match crate::voice_ws::start(&key, &self.cfg.voice_model) {
                            Ok(sock) => {
                                self.voice_sock = Some(sock);
                                self.voice_state = VoiceState::Listening;
                                self.voice_orb = "listening".into();
                                self.status = format!(
                                    "Voice live {}",
                                    voice_session_url(&dedicated_voice_model(&self.cfg.voice_model))
                                );
                                return;
                            }
                            Err(e) => {
                                self.status = format!("{e} — push-to-talk");
                            }
                        }
                    }
                }
                HeyGrokRoute::PushToTalk => {}
                HeyGrokRoute::None => {
                    self.status =
                        "Connect Grok OAuth for STT/TTS, or paste a console key for duplex Voice."
                            .into();
                    return;
                }
            }
        }
        if !hey_grok_starts_ptt(self.voice_sock.is_some(), self.running) {
            return;
        }
        self.voice_orb = "listening".into();
        self.voice_state = VoiceState::Listening;
        self.running = true;
        self.chat_job_thread = None;
        self.status = "Listening… STT".into();
        let key = self.bearer();
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(JobOut::Voice(listen_turn(&key)));
        });
    }

    fn ensure_cabin_frame(&mut self) {
        if !should_capture_before_chat(self.eyes_attach || self.hands_attach) {
            return;
        }
        let rows = collect_rows();
        self.last_window_title = rows
            .iter()
            .map(|r| r.name.as_str())
            .find(|n| !n.is_empty() && *n != "cursor")
            .unwrap_or("")
            .to_string();
        if !should_send_screenshot(&self.last_window_title, "") {
            self.status = "eyes: skipped lock/password frame".into();
            return;
        }
        match capture_data_url() {
            Ok(url) => {
                if let Ok(mut st) = self.hub.lock() {
                    st.store_frame(&url);
                }
                self.last_frame_url = Some(url);
            }
            Err(e) => {
                if self.status.is_empty() || self.status == "Thinking…" {
                    self.status = format!("eyes: {e}");
                }
            }
        }
    }

    fn queue_update(&mut self) {
        self.nav = Nav::Settings;
        self.settings_sec = SettingsSec::Update;
        let Some(src) = resolve_source(&self.cfg.source_dir) else {
            self.status = "Set Settings → source (clone path) or GROKHUB_SRC".into();
            return;
        };
        self.cfg.source_dir = src.display().to_string();
        remember_source(&src);
        let _ = config::save(&self.cfg);
        match update_cmds(&src) {
            Ok(cmds) if !update_wipes_config(&cmds) => {
                self.start_overlay_update(cmds);
            }
            Ok(_) => self.status = "refusing an update that would wipe config".into(),
            Err(e) => self.status = e,
        }
    }

    fn restart_after_update(&mut self, ctx: &egui::Context) {
        if self.running {
            self.status = "Busy — wait, then restart".into();
            return;
        }
        self.persist();
        self.status = "Restarting GrokHub…".into();
        if let Some(tray) = self.tray.take() {
            crate::tray::drop_off_thread(tray);
        }
        match crate::update::restart_system(!self.window_visible) {
            Ok(()) => {
                self.want_quit = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Err(e) => self.status = e,
        }
    }

    fn start_overlay_update(&mut self, cmds: Vec<String>) {
        if self.running {
            self.status = "Busy — wait, then update".into();
            return;
        }
        if cmds.is_empty() {
            self.status = "Update plan empty".into();
            return;
        }
        self.nav = Nav::Settings;
        self.settings_sec = SettingsSec::Update;
        let begin = overlay_update_begin(cmds.len());
        self.running = begin.running;
        self.chat_job_thread = None;
        self.update_pct = Some(begin.pct);
        self.update_can_restart = begin.can_restart;
        self.status = begin.status;
        self.last_host = cmds.clone();
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        std::thread::spawn(move || {
            let progress = tx.clone();
            let r = crate::update::run_update_cmds_with_progress(&cmds, |pct, msg| {
                let _ = progress.send(JobOut::UpdateProgress {
                    pct,
                    msg: msg.to_string(),
                });
            });
            let _ = tx.send(match r {
                Ok(_) => JobOut::UpdateDone { ok: true },
                Err(e) if crate::update::host_receipt_failed(&e) => JobOut::UpdateDone { ok: false },
                Err(e) => JobOut::Err(e),
            });
        });
    }

    fn touch(&mut self) {
        self.last_activity = Instant::now();
        self.reflected_idle = false;
    }

    fn run_reflect(&mut self) {
        if self.scratch() {
            self.status = "Scratch — no reflect".into();
            return;
        }
        let msgs: Vec<(String, String)> = self
            .messages
            .iter()
            .map(|m| (m.role.clone(), m.content.clone()))
            .collect();
        let facts = fact_candidates(&msgs);
        if self.policy().learns() {
            extract_insights(&mut self.learning, &facts);
        }
        let current = config::read_memory("MEMORY.md");
        let edit = surgical_memory_edit(&current, &facts);
        let mut wrote = false;
        if !edit.diff.is_empty() {
            match config::write_memory("MEMORY.md", &edit.next) {
                Ok(()) => {
                    self.reflect_diff = edit.diff;
                    if self.mem_name == "MEMORY.md" {
                        self.mem_body = edit.next;
                    }
                    wrote = true;
                }
                Err(e) => {
                    self.status = e;
                    return;
                }
            }
        }
        if self.policy().writes_user_md() {
            let prefs = user_pref_facts(&facts);
            if !prefs.is_empty() {
                let user = config::read_memory("USER.md");
                let user_edit = surgical_memory_edit(&user, &prefs);
                if !user_edit.diff.is_empty() {
                    match config::write_memory("USER.md", &user_edit.next) {
                        Ok(()) => {
                            if self.mem_name == "USER.md" {
                                self.mem_body = user_edit.next;
                            }
                            if self.reflect_diff.is_empty() {
                                self.reflect_diff = user_edit.diff;
                            }
                            wrote = true;
                        }
                        Err(e) => {
                            self.status = e;
                            return;
                        }
                    }
                }
            }
        }
        self.status = if wrote {
            "Reflected MEMORY.md".into()
        } else {
            "Reflect: nothing new".into()
        };
    }

    fn run_skill_verify(&mut self) {
        if self.skill_name.is_empty() {
            return;
        }
        let Some(v) = skills::run_verify(&self.skill_name) else {
            return;
        };
        self.verify_ok_turn = v.ok;
        self.verify_chip = if v.ok {
            "verify pass".into()
        } else {
            "verify fail".into()
        };
        self.messages.push(Msg {
            role: "user".into(),
            content: format!("VERIFY_RESULT:\n{}", v.detail),
        });
        if v.ok {
            if let Some(s) = self.skill_list.iter_mut().find(|s| s.name == self.skill_name) {
                s.runs = bump_skill_run(s.runs);
                let bumped = s.clone();
                let _ = skills::save_skill(&bumped);
            }
        }
    }

    fn take_over_desktop(&mut self) {
        self.nav = Nav::Chat;
        self.hands_attach = true;
        self.eyes_attach = true;
        self.send_chat("Take over this desktop. Look at the screen and fix what is broken.".into());
    }

    fn replay_saved_recipe(&mut self, id: &str) {
        let recipe = if id.eq_ignore_ascii_case("last") {
            crate::recipes::load_last().or_else(|| self.last_recipe.clone())
        } else {
            crate::recipes::load_recipe(id).or_else(|| self.last_recipe.clone())
        };
        match recipe {
            Some(r) => {
                self.last_recipe = Some(r);
                self.replay_recipe();
            }
            None => self.status = format!("No recipe {id}"),
        }
    }

    fn replay_recipe(&mut self) {
        if self.last_recipe.is_none() {
            self.last_recipe = crate::recipes::load_last();
        }
        let Some(recipe) = self.last_recipe.clone() else {
            self.status = "No recipe".into();
            return;
        };
        let rows = collect_rows();
        let current = screen_from_rows(&rows);
        let ops = replay_ops(&recipe, current);
        let mut t = String::new();
        if let Some(c) = current {
            t.push_str(&format!("screen {}x{}\n", c.w, c.h));
        }
        for op in ops {
            match op {
                ReplayOp::Reshoot => {
                    t.push_str("reshoot: screen changed, skip coordinate clicks\n");
                    if let Ok(url) = capture_data_url() {
                        if let Ok(mut st) = self.hub.lock() {
                            st.store_frame(&url);
                        }
                        self.last_frame_url = Some(url);
                        t.push_str("frame: captured\n");
                    }
                }
                ReplayOp::Op(op) => {
                    t.push_str(&computer_cmd_line(&op));
                    t.push('\n');
                    t.push_str(&run_computer_op(&op));
                    t.push('\n');
                }
            }
        }
        self.eyes_text = t;
        self.status = "Recipe replay".into();
    }

    fn speak_reply(&self, text: &str) {
        let key = auth_bearer(&self.cfg.api_key, &secrets::access_token(&self.secrets))
            .unwrap_or_default();
        let text = text.to_string();
        std::thread::spawn(move || {
            let Ok(bytes) = grok_tts(&key, &text) else {
                return;
            };
            let path = std::env::temp_dir().join("grokhub-speak.mp3");
            if std::fs::write(&path, bytes).is_ok() {
                let _ = play_audio(&path);
            }
        });
    }

    fn refresh_eyes(&mut self) {
        let rows = collect_rows();
        let labels: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        let refused = refused_lock(&labels);
        let frame = build_windshield(
            &rows,
            None,
            refused,
            self.board.first().map(|c| c.title.as_str()),
            self.skill_list.first().map(|s| s.name.as_str()),
            4,
        );
        let mut t = format!(
            "AT-SPI/wmctrl · autonomy {} · {} objects\n",
            frame.autonomy,
            frame.objects.len()
        );
        for o in &frame.objects {
            t.push_str(&format!("- [{}] {} @{},{} {}x{}\n", o.kind, o.label, o.x, o.y, o.w, o.h));
        }
        if let Some(g) = &frame.goal {
            t.push_str(&format!("goal: {g}\n"));
        }
        if self.cfg.cabin_eyes {
            match capture_data_url() {
                Ok(url) => {
                    if let Ok(mut st) = self.hub.lock() {
                        st.store_frame(&url);
                    }
                    self.last_frame_url = Some(url);
                    t.push_str("frame: captured (on hub, not disk)\n");
                }
                Err(e) => t.push_str(&format!("frame: {e}\n")),
            }
        }
        self.eyes_text = t;
        self.status = format!("{} objects", frame.objects.len());
    }

    fn halt_work(&mut self, status: impl Into<String>) {
        self.halt_in_flight();
        self.status = status.into();
    }

    fn drain_inbox(&mut self) {
        if !self.hub_on || self.running {
            return;
        }
        let id = self
            .hub
            .lock()
            .ok()
            .map(|s| s.device_id.clone())
            .unwrap_or_default();
        if id.is_empty() {
            return;
        }
        let task = self.hub.lock().ok().and_then(|mut s| s.take_next_queued(&id));
        if let Some(t) = task {
            self.nav = Nav::Chat;
            self.send_chat(format!("[from {}] {}", t.from_name, t.prompt));
        }
    }

    fn hide_to_tray(&mut self, ctx: &egui::Context) {
        match crate::tray::hide_action(self.window_visible, self.told_tray) {
            crate::tray::HideAction::Skip => return,
            crate::tray::HideAction::Hide => {
                self.unmap_to_tray(ctx);
            }
            crate::tray::HideAction::HideAndPing => {
                self.unmap_to_tray(ctx);
                let clock = Self::local_clock();
                let quiet =
                    quiet_hours_active(&clock.hm(), &self.cfg.quiet_start, &self.cfg.quiet_end);
                if crate::notify::allow_ping(quiet) {
                    crate::notify::ping("GrokHub", "Still running in the tray");
                }
                self.status = "In the tray — Show cabin to sit down".into();
            }
        }
        self.told_tray = true;
    }

    fn unmap_to_tray(&mut self, ctx: &egui::Context) {
        self.capture_window(ctx);
        self.persist();
        self.geom_dirty = false;
        self.window_visible = false;
        apply_tray_window(ctx, crate::tray::hide_to_tray_window());
        self.ensure_tray_spawn();
    }

    fn ensure_tray_spawn(&mut self) {
        if self.tray.is_some() || self.tray_rx.is_some() || !crate::tray::tray_wanted() {
            return;
        }
        self.tray_rx = Some(crate::tray::begin_tray_spawn());
    }

    fn show_from_tray(&mut self, ctx: &egui::Context) {
        self.window_visible = true;
        apply_tray_window(ctx, crate::tray::show_from_tray_window());
        self.ensure_tray_spawn();
        ctx.request_repaint();
    }

    fn poll_voice(&mut self) {
        let Some(sock) = &self.voice_sock else {
            return;
        };
        let mut evs = Vec::new();
        while let Ok(ev) = sock.rx.try_recv() {
            evs.push(ev);
        }
        for ev in evs {
            self.voice_state = reduce_voice_state(self.voice_state, &ev);
            self.voice_orb = match self.voice_state {
                VoiceState::Listening => "listening",
                VoiceState::Speaking => "speaking",
                VoiceState::Hands => "hands",
                VoiceState::Idle => "idle",
            }
            .into();
            match ev {
                VoiceEvent::Transcript { .. } => {
                    if let Some((role, text, kind)) = voice_stream_token(&ev) {
                        if voice_transcript_sends_chat(self.voice_sock.is_some()) {
                            if voice_log_role(&ev).is_some() && role == "user" {
                                self.send_chat(text.to_string());
                            }
                        } else {
                            let mut pairs: Vec<(String, String)> = self
                                .messages
                                .iter()
                                .map(|m| (m.role.clone(), m.content.clone()))
                                .collect();
                            fold_stream_token(&mut pairs, role, text, kind);
                            self.messages = pairs
                                .into_iter()
                                .map(|(role, content)| Msg { role, content })
                                .collect();
                            if matches!(kind, StreamTokenKind::Replace) && voice_log_role(&ev).is_some()
                            {
                                self.persist();
                            }
                        }
                    }
                }
                VoiceEvent::Fallback | VoiceEvent::Error(_) => {
                    if let Some(mut s) = self.voice_sock.take() {
                        s.halt();
                    }
                    self.status = "Voice socket failed — push-to-talk".into();
                }
                VoiceEvent::Close => {
                    self.voice_sock = None;
                    self.voice_state = VoiceState::Idle;
                }
                _ => {}
            }
        }
    }

    fn poll_tray(&mut self, ctx: &egui::Context) {
        let ready = self
            .tray_rx
            .as_ref()
            .and_then(crate::tray::take_spawn_result);
        if let Some(maybe) = ready {
            self.tray_rx = None;
            if let Some(host) = maybe {
                self.tray = crate::tray::keep_if_hidden(!self.window_visible, host);
            }
        }
        let Some(tray) = &self.tray else {
            return;
        };
        match tray.try_recv() {
            Some(crate::tray::TrayCmd::Show) => self.show_from_tray(ctx),
            Some(crate::tray::TrayCmd::Halt) => self.halt_work("Stopped"),
            Some(crate::tray::TrayCmd::Quit) => {
                self.want_quit = true;
                if let Some(tray) = self.tray.take() {
                    crate::tray::drop_off_thread(tray);
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            None => {}
        }
    }

    fn start_hub(&mut self) {
        if self.hub_on {
            return;
        }
        if let Ok(mut st) = self.hub.lock() {
            st.sharing = true;
            st.port = self.hub_port;
            if st.pair.is_none() {
                st.rotate_pair();
            }
        }
        self.sync_hub_voice();
        match serve_lan(self.hub.clone(), self.hub_port) {
            Ok(p) => {
                self.hub_port = p;
                self.hub_on = true;
                self.status = format!("Hub live on :{p} ({HUB_KIND})");
                self.persist();
            }
            Err(e) => self.status = e,
        }
    }

    fn ui_project_overlays(&mut self, ctx: &egui::Context) {
        if self.proj_plus_open {
            let mut pick: Option<&'static str> = None;
            let mut menu_rect = egui::Rect::NOTHING;
            egui::Area::new(egui::Id::new("proj-plus"))
                .fixed_pos(self.proj_plus_pos + egui::vec2(0.0, 4.0))
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_min_width(160.0);
                        if ui.selectable_label(false, "New project").clicked() {
                            pick = Some("project");
                        }
                        if ui.selectable_label(false, "New folder").clicked() {
                            pick = Some("folder");
                        }
                        menu_rect = ui.min_rect();
                    });
                });
            if let Some(kind) = pick {
                self.proj_plus_open = false;
                match kind {
                    "project" => self.stage_new_project(None),
                    "folder" => self.stage_new_folder(),
                    _ => {}
                }
            } else if self.proj_ignore_close {
                self.proj_ignore_close = false;
            } else if ctx.input(|i| i.pointer.any_click()) {
                if let Some(pos) = ctx.pointer_interact_pos() {
                    if !menu_rect.expand(8.0).contains(pos) {
                        self.proj_plus_open = false;
                    }
                }
            }
        }
        if let Some(pid) = self.proj_add_for.clone() {
            let folders = folder_choices(&self.projects);
            let mut picked: Option<Option<String>> = None;
            let mut menu_rect = egui::Rect::NOTHING;
            egui::Area::new(egui::Id::new("proj-add"))
                .fixed_pos(self.proj_menu_pos + egui::vec2(8.0, 8.0))
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_min_width(168.0);
                        ui.label(RichText::new("Add to folder").size(12.0).color(crate::theme::muted()));
                        if folders.is_empty() {
                            ui.label("Create a folder first");
                        }
                        for (fid, name) in &folders {
                            if ui.selectable_label(false, name).clicked() {
                                picked = Some(Some(fid.clone()));
                            }
                        }
                        if ui.selectable_label(false, "Projects (root)").clicked() {
                            picked = Some(None);
                        }
                        menu_rect = ui.min_rect();
                    });
                });
            if let Some(folder) = picked {
                self.proj_add_for = None;
                match add_to_folder(&mut self.projects, &pid, folder.as_deref()) {
                    Ok(()) => {
                        if let Some(fid) = folder {
                            if let Some(f) = self.projects.iter_mut().find(|n| n.id == fid) {
                                f.open = true;
                            }
                            self.status = "Added to folder".into();
                        } else {
                            self.status = "Moved to Projects".into();
                        }
                        self.touch_projects();
                        self.persist();
                    }
                    Err(e) => self.status = e.into(),
                }
            } else if self.proj_ignore_close {
                self.proj_ignore_close = false;
            } else if ctx.input(|i| i.pointer.any_click()) {
                if let Some(pos) = ctx.pointer_interact_pos() {
                    if !menu_rect.expand(8.0).contains(pos) {
                        self.proj_add_for = None;
                    }
                }
            }
        }
    }
}


impl eframe::App for Cabin {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some(tray) = self.tray.take() {
            crate::tray::drop_off_thread(tray);
        }
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_job();
        self.poll_chips();
        self.poll_review();
        self.poll_greeting();
        self.poll_goals();
        self.refresh_chips();
        self.refresh_greeting();
        self.drain_inbox();
        self.poll_tray(ctx);
        self.poll_voice();
        self.poll_global_hotkeys();
        self.poll_night_check(now_ms());
        self.poll_wall();
        self.live_room();
        self.tick_heartbeat();
        if ctx.input(|i| i.viewport().close_requested()) {
            let hide = crate::tray::should_hide_on_close(
                self.cfg.close_to_tray,
                self.tray.is_some(),
            ) && !self.want_quit;
            if hide {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.hide_to_tray(ctx);
            } else {
                self.capture_window(ctx);
                self.persist();
                self.geom_dirty = false;
            }
        }
        if self.oauth_pending.is_some() {
            self.poll_oauth();
            ctx.request_repaint_after(Duration::from_secs(2));
        }
        self.poll_oauth_photo(ctx);
        if !self.composer.trim().is_empty()
            || ctx.input(|i| {
                i.pointer.any_pressed()
                    || i.events.iter().any(|e| {
                        matches!(
                            e,
                            egui::Event::Text(_)
                                | egui::Event::Key {
                                    pressed: true,
                                    ..
                                }
                        )
                    })
            })
        {
            self.touch();
        }
        if ctx.input(|i| i.modifiers.command && i.modifiers.shift && i.key_pressed(egui::Key::Escape))
        {
            self.halt_work("Stopped");
        }
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::G) && !i.modifiers.shift) {
            self.listen_voice();
        }
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::N) && !i.modifiers.shift) {
            self.new_thread(false);
        }
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::K) && !i.modifiers.shift) {
            if self.palette_open {
                self.palette_open = false;
            } else {
                self.open_palette();
            }
        }
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Slash)) {
            self.shortcuts_open = !self.shortcuts_open;
        }
        self.capture_window(ctx);
        self.flush_window(ctx);
        if self.last_persist.elapsed() > Duration::from_secs(2) {
            self.persist();
        }
        let wait = next_heartbeat_wait_ms(
            self.last_heartbeat.elapsed().as_millis() as u64,
            HEARTBEAT_MS,
        );
        let live = wants_live_repaint(
            self.running,
            self.chip_busy || self.goal_busy || self.oauth_photo_busy || self.review_busy,
            self.hub_on,
            self.window_visible,
            self.page_nav() == Nav::Imagine,
            self.wall_busy,
        );
        if !self.window_visible {
            apply_tray_window(ctx, crate::tray::hide_to_tray_window());
        }
        ctx.request_repaint_after(Duration::from_millis(heartbeat_repaint_ms(
            live,
            !self.window_visible,
            wait,
            HIDDEN_HEARTBEAT_MS,
        )));

        crate::theme::apply(
            ctx,
            resolve_dark(
                parse_theme(&self.cfg.theme),
                crate::theme::desktop_prefers_dark(),
            ),
        );
        self.ui_titlebar(ctx);
        self.ui_sidebar(ctx);
        self.ui_settings_menu(ctx);

        match self.page_nav() {
            Nav::Chat => self.ui_chat(ctx),
            Nav::Devices => self.ui_devices(ctx),
            Nav::Memory => self.ui_memory(ctx),
            Nav::Workboard => self.ui_board(ctx),
            Nav::Imagine => self.ui_imagine(ctx),
            Nav::Skills => self.ui_skills(ctx),
            Nav::Eyes => self.ui_eyes(ctx),
            Nav::Night => self.ui_night(ctx),
            Nav::History => self.ui_history(ctx),
            Nav::Command => self.ui_command(ctx),
            Nav::Connectors => self.ui_connectors(ctx),
            Nav::Agents => self.ui_agents(ctx),
            Nav::Settings => self.ui_chat(ctx),
        }
        if self.nav == Nav::Settings {
            self.ui_settings(ctx);
        }
        if self.palette_open {
            self.ui_palette(ctx);
        }
        if self.shortcuts_open {
            egui::Window::new("Shortcuts")
                .collapsible(false)
                .default_width(420.0)
                .show(ctx, |ui| {
                    ui.set_max_width(400.0);
                    for line in shortcut_help().lines() {
                        ui.label(line);
                    }
                    if crate::cards::ghost_pill(ui, "Close") {
                        self.shortcuts_open = false;
                    }
                });
        }
        self.ui_plus_overlays(ctx);
        self.ui_imagine_overlays(ctx);
        self.ui_project_overlays(ctx);
    }
}

impl Cabin {
    fn poll_global_hotkeys(&mut self) {
        if self.hotkeys.is_none() {
            return;
        }
        while let Ok(ev) = GlobalHotKeyEvent::receiver().try_recv() {
            if ev.state != HotKeyState::Pressed {
                continue;
            }
            if ev.id == self.hotkey_hey {
                self.listen_voice();
            } else if ev.id == self.hotkey_halt {
                self.halt_work("Stopped");
            }
        }
    }

    fn roll_today(&mut self) {
        let today = std::process::Command::new("date")
            .arg("+%F")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_default();
        if !today.is_empty() {
            roll_usage_day(&mut self.usage, &today);
        }
    }

    fn ui_settings_menu(&mut self, ctx: &egui::Context) {
        if !self.settings_menu_open {
            return;
        }
        let mut pick: Option<&'static str> = None;
        let mut connect = false;
        let mut disconnect = false;
        let mut help = false;
        let authed = self.has_key();
        let shown = egui::Window::new("settings-menu")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::LEFT_BOTTOM, [12.0, -56.0])
            .frame(
                egui::Frame::none()
                    .fill(crate::theme::panel())
                    .rounding(12.0)
                    .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                    .inner_margin(egui::Margin::same(8.0)),
            )
            .show(ctx, |ui| {
                ui.set_min_width(220.0);
                ui.spacing_mut().item_spacing.y = 2.0;
                for (id, label) in crate::theme::CABIN_MENU {
                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new(*label)
                                    .size(crate::theme::FONT_CHROME)
                                    .color(crate::theme::fg()),
                            )
                            .fill(egui::Color32::TRANSPARENT)
                            .rounding(8.0)
                            .min_size(egui::vec2(204.0, 36.0)),
                        )
                        .clicked()
                    {
                        pick = Some(*id);
                    }
                }
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("Help")
                                .size(crate::theme::FONT_CHROME)
                                .color(crate::theme::fg()),
                        )
                        .fill(egui::Color32::TRANSPARENT)
                        .rounding(8.0)
                        .min_size(egui::vec2(204.0, 36.0)),
                    )
                    .clicked()
                {
                    help = true;
                }
                let auth_label = if authed { "Sign out" } else { "Connect Grok" };
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new(auth_label)
                                .size(crate::theme::FONT_CHROME)
                                .color(crate::theme::fg()),
                        )
                        .fill(egui::Color32::TRANSPARENT)
                        .rounding(8.0)
                        .min_size(egui::vec2(204.0, 36.0)),
                    )
                    .clicked()
                {
                    if authed {
                        disconnect = true;
                    } else {
                        connect = true;
                    }
                }
            });
        let menu_rect = shown.map(|r| r.response.rect);
        if let Some(id) = pick {
            self.set_nav_id(id);
            self.settings_menu_open = false;
        }
        if help {
            self.shortcuts_open = true;
            self.settings_menu_open = false;
        }
        if connect {
            self.start_oauth();
            self.settings_menu_open = false;
        }
        if disconnect {
            self.sign_out_oauth();
            self.settings_menu_open = false;
        }
        let outside = ctx.input(|i| i.pointer.any_click())
            && ctx.pointer_interact_pos().is_some_and(|pos| {
                menu_rect.map(|r| !r.expand(8.0).contains(pos)).unwrap_or(true)
            });
        if cabin_menu_should_dismiss(self.settings_menu_ignore, outside) {
            self.settings_menu_open = false;
        }
        self.settings_menu_ignore = false;
    }

    fn ui_palette(&mut self, ctx: &egui::Context) {
        let mut close = false;
        let mut picked: Option<String> = None;
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            close = true;
        }
        egui::Window::new("Palette")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_TOP, [0.0, 48.0])
            .show(ctx, |ui| {
                ui.set_min_width(360.0);
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut self.palette_q)
                        .hint_text("Go to…")
                        .desired_width(360.0),
                );
                if self.palette_focus {
                    edit.request_focus();
                    self.palette_focus = false;
                }
                let hits = filter_palette(&self.palette_q);
                self.palette_pick = slash_pick_step(self.palette_pick, hits.len(), 0);
                if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)) {
                    self.palette_pick = slash_pick_step(self.palette_pick, hits.len(), 1);
                } else if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp))
                {
                    self.palette_pick = slash_pick_step(self.palette_pick, hits.len(), -1);
                } else if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
                    if let Some((_, action)) = hits.get(self.palette_pick) {
                        picked = Some((*action).to_string());
                    }
                }
                egui::ScrollArea::vertical()
                    .max_height(PALETTE_LIST_H)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.set_min_width(360.0);
                        for (i, (label, action)) in hits.iter().enumerate() {
                            if ui
                                .add_sized(
                                    [ui.available_width(), 28.0],
                                    egui::SelectableLabel::new(i == self.palette_pick, *label),
                                )
                                .clicked()
                            {
                                picked = Some((*action).to_string());
                            }
                        }
                    });
                if crate::cards::ghost_pill(ui, "Close") {
                    close = true;
                }
            });
        if let Some(a) = picked {
            self.run_palette(&a);
        }
        if close {
            self.palette_open = false;
        }
    }

    fn ui_command(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(crate::theme::bg()).inner_margin(egui::Margin::same(24.0)))
            .show(ctx, |ui| {
            if crate::cards::page_header(ui, "Command", "Run") {
                let line = self.cmd_line.trim().to_string();
                if !line.is_empty() {
                    self.cmd_hist.push(line.clone());
                    self.cmd_line.clear();
                    self.queue_sh(line);
                }
            }
            crate::cards::section_label(ui, "This box");
            if !self.host_live.is_empty() {
                crate::cards::status_chip(ui, &self.host_live, crate::cards::ChipTone::Setup);
                ui.add_space(8.0);
            }
            let mut run = false;
            egui::Frame::none()
                .fill(crate::theme::elevated())
                .rounding(12.0)
                .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                .show(ui, |ui| {
                    let enter = ui
                        .add(
                            egui::TextEdit::singleline(&mut self.cmd_line)
                                .hint_text("$ ls — bound project is the working tree")
                                .desired_width(f32::INFINITY)
                                .frame(false),
                        )
                        .lost_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if enter {
                        run = true;
                    }
                });
            if run {
                let line = self.cmd_line.trim().to_string();
                if !line.is_empty() {
                    self.cmd_hist.push(line.clone());
                    self.cmd_line.clear();
                    self.queue_sh(line);
                }
            }
            ui.add_space(16.0);
            crate::cards::section_label(ui, "History");
            if self.cmd_hist.is_empty() && self.last_host.is_empty() {
                let _ = crate::cards::empty_prompt_tile(
                    ui,
                    crate::icons::TileIcon::Host,
                    "Nothing run yet",
                    "Type a command above. The bound project is the working tree.",
                );
            } else {
                let hist: Vec<String> = self.cmd_hist.iter().rev().take(6).cloned().collect();
                crate::cards::tile_row(ui, hist.len(), |ui, i| {
                    let cmd = &hist[i];
                    crate::cards::grok_tile(
                        ui,
                        crate::icons::TileIcon::Host,
                        cmd,
                        "Ran on this box",
                        None,
                        false,
                    );
                });
                if !self.last_host.is_empty() {
                    ui.add_space(12.0);
                    crate::cards::section_label(ui, "Last host");
                    let receipt: String = self.last_host.join(" ").chars().take(80).collect();
                    crate::cards::grok_tile(
                        ui,
                        crate::icons::TileIcon::Check,
                        "Last receipt",
                        &receipt,
                        None,
                        false,
                    );
                }
            }
        });
    }

    fn ui_connectors(&mut self, ctx: &egui::Context) {
        self.skills_tab_connectors = true;
        self.ui_skills(ctx);
    }

    fn ui_agents(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(crate::theme::bg()).inner_margin(egui::Margin::same(24.0)))
            .show(ctx, |ui| {
            let _ = crate::cards::page_header(ui, "Queue", "");
            ui.label(RichText::new("Background jobs for this thread.").color(crate::theme::muted()));
            ui.add_space(12.0);
            if !self.cfg.goal_pin.is_empty() {
                crate::cards::status_chip(
                    ui,
                    &format!("Pinned · step {}", self.goal_step),
                    crate::cards::ChipTone::Mute,
                );
                ui.add_space(8.0);
            }
            let mut run_at: Option<usize> = None;
            if self.agents.is_empty() {
                let _ = crate::cards::empty_prompt_tile(
                    ui,
                    crate::icons::TileIcon::List,
                    "No jobs yet",
                    "Queued work from chat shows up here.",
                );
            }
            for (i, a) in self.agents.iter().enumerate() {
                crate::cards::grok_tile(
                    ui,
                    crate::icons::TileIcon::Bolt,
                    &a.title,
                    &a.status,
                    None,
                    false,
                );
                ui.add_space(6.0);
                if crate::cards::ghost_pill(ui, "Run") {
                    run_at = Some(i);
                }
            }
            if let Some(i) = run_at {
                if i < self.agents.len() {
                    self.agents[i].status = "running".into();
                    let p = self.agents[i].prompt.clone();
                    self.nav = Nav::Chat;
                    self.send_chat(p);
                }
            }
        });
    }

    fn page_nav(&self) -> Nav {
        if self.nav != Nav::Settings {
            return self.nav;
        }
        if self.settings_back == Nav::Settings {
            Nav::Chat
        } else {
            self.settings_back
        }
    }

    fn nav_id(&self) -> &'static str {
        match self.page_nav() {
            Nav::Chat => "chat",
            Nav::History => "history",
            Nav::Imagine => "imagine",
            Nav::Workboard => "workboard",
            Nav::Settings => "chat",
            Nav::Skills => "skills",
            Nav::Night => "automations",
            Nav::Command => "command",
            Nav::Agents => "queue",
            Nav::Devices => "devices",
            Nav::Memory => "memory",
            Nav::Eyes => "eyes",
            Nav::Connectors => "connectors",
        }
    }

    fn set_nav_id(&mut self, id: &str) {
        self.nav = match id {
            "history" => Nav::History,
            "imagine" => {
                self.imagine_want_focus = true;
                Nav::Imagine
            }
            "workboard" => Nav::Workboard,
            "settings" => {
                if self.nav != Nav::Settings {
                    self.settings_back = self.nav;
                }
                self.settings_sec = SettingsSec::Account;
                Nav::Settings
            }
            "skills" => {
                self.skills_tab_connectors = false;
                Nav::Skills
            }
            "automations" => Nav::Night,
            "command" => Nav::Command,
            "queue" => Nav::Agents,
            "devices" => Nav::Devices,
            "memory" => Nav::Memory,
            "eyes" => Nav::Eyes,
            "connectors" => {
                self.skills_tab_connectors = true;
                Nav::Connectors
            }
            "chat" => {
                self.open_recent_chat();
                Nav::Chat
            }
            _ => Nav::Chat,
        };
    }

    #[allow(dead_code)]
    fn conn_kind(&self) -> &'static str {
        if self.has_key() {
            "live"
        } else if self.oauth_pending.is_some() {
            "setup"
        } else {
            "setup"
        }
    }

    fn ui_titlebar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("titlebar")
            .exact_height(crate::theme::TITLEBAR_H)
            .frame(egui::Frame::none().fill(crate::theme::bg()))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if titlebar_chrome_hit(&titlebar_chrome_btn(ui, "×")) {
                            let hide = crate::tray::should_hide_on_close(
                                self.cfg.close_to_tray,
                                self.tray.is_some(),
                            ) && !self.want_quit;
                            if hide {
                                self.hide_to_tray(ctx);
                            } else {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            }
                        }
                        if titlebar_chrome_hit(&titlebar_chrome_btn(ui, "□")) {
                            let currently = ctx
                                .input(|i| i.viewport().maximized)
                                .unwrap_or(self.win_max);
                            self.win_max = next_maximized(currently);
                            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(self.win_max));
                            self.cfg.window.maximized = self.win_max;
                            self.geom_dirty = true;
                        }
                        if titlebar_chrome_hit(&titlebar_chrome_btn(ui, "–")) {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                        }
                    });
                });
            });
    }

    fn nav_row(
        ui: &mut egui::Ui,
        active: bool,
        icon: crate::icons::RailIcon,
        label: &str,
        outline: bool,
    ) -> egui::Response {
        let fill = if active {
            crate::theme::nav_active()
        } else {
            egui::Color32::TRANSPARENT
        };
        let color = if active {
            crate::theme::fg()
        } else {
            crate::theme::muted()
        };
        let w = ui.available_width();
        let (_rect, resp) = ui.allocate_exact_size(egui::vec2(w, crate::theme::NAV_ROW_H), egui::Sense::click());
        let (resp, rect, fill) = crate::theme::feel_response(ui, resp, fill);
        ui.painter().rect_filled(rect, 10.0, fill);
        if outline {
            ui.painter().rect_stroke(
                rect,
                10.0,
                egui::Stroke::new(1.0_f32, crate::theme::border_strong()),
            );
        }
        let icon_c = egui::pos2(rect.left() + 20.0, rect.center().y);
        let icon_rect = egui::Rect::from_center_size(icon_c, egui::vec2(20.0, 20.0));
        crate::icons::paint_rail_icon_at(ui.painter(), icon_rect, icon, color);
        let text_left = rect.left() + 38.0;
        let text_right = rect.right() - 12.0;
        let painted = fit_rail_label(ui, label, (text_right - text_left).max(8.0));
        ui.painter().text(
            egui::pos2(text_left, rect.center().y),
            egui::Align2::LEFT_CENTER,
            painted,
            egui::FontId::proportional(crate::theme::FONT_CHROME),
            color,
        );
        resp
    }

    fn cabin_avatar(
        ui: &mut egui::Ui,
        account: &str,
        email: &str,
        photo: Option<&TextureHandle>,
    ) -> egui::Response {
        let (rect, resp) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), RAIL_FOOTER_H), egui::Sense::click());
        let (resp, rect, wash) = crate::theme::feel_response(ui, resp, egui::Color32::TRANSPARENT);
        if wash.a() > 0 {
            ui.painter().rect_filled(rect, 10.0, wash);
        }
        let c = egui::pos2(rect.left() + 20.0, rect.center().y);
        if let Some(tex) = photo {
            let size = egui::vec2(28.0, 28.0);
            egui::Image::from_texture(tex)
                .fit_to_exact_size(size)
                .rounding(14.0)
                .paint_at(ui, egui::Rect::from_center_size(c, size));
        } else {
            ui.painter().circle_filled(c, 14.0, crate::theme::panel());
        }
        ui.painter().circle_stroke(
            c,
            14.0,
            egui::Stroke::new(1.0_f32, crate::theme::border_strong()),
        );
        ui.painter().text(
            egui::pos2(rect.left() + 42.0, rect.center().y - 8.0),
            egui::Align2::LEFT_CENTER,
            account,
            egui::FontId::proportional(crate::theme::FONT_META),
            crate::theme::fg(),
        );
        ui.painter().text(
            egui::pos2(rect.left() + 42.0, rect.center().y + 8.0),
            egui::Align2::LEFT_CENTER,
            email,
            egui::FontId::proportional(11.0),
            crate::theme::subtle(),
        );
        resp
    }

    fn ui_sidebar(&mut self, ctx: &egui::Context) {
        let account = self
            .secrets
            .oauth
            .as_ref()
            .and_then(|t| t.name.clone().or(t.email.clone()))
            .unwrap_or_else(|| "Cabin".into());
        let email = self
            .secrets
            .oauth
            .as_ref()
            .and_then(|t| t.email.clone())
            .unwrap_or_else(|| "Connect Grok".into());
        egui::SidePanel::left("rail")
            .exact_width(crate::theme::SIDEBAR_W)
            .resizable(false)
            .frame(egui::Frame::none().fill(crate::theme::bg()).inner_margin(egui::Margin::same(8.0)))
            .show(ctx, |ui| {
                ui.add_space(4.0);
                if Self::nav_row(ui, false, crate::icons::RailIcon::Search, "Search", false).clicked()
                {
                    self.open_palette();
                }
                if Self::nav_row(ui, false, crate::icons::RailIcon::Compose, "New chat", true)
                    .clicked()
                {
                    self.new_thread(false);
                    self.nav = Nav::Chat;
                }
                ui.add_space(6.0);
                let cur = self.nav_id();
                for (id, label) in crate::theme::GROK_NAV {
                    if Self::nav_row(ui, cur == *id, crate::icons::rail_icon_for(id), label, false)
                        .clicked()
                    {
                        self.set_nav_id(id);
                    }
                }
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Projects").size(12.0).color(crate::theme::subtle()));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let plus = ui.add(
                            egui::Button::new(RichText::new("+").size(16.0).color(crate::theme::muted()))
                                .fill(egui::Color32::TRANSPARENT)
                                .stroke(egui::Stroke::NONE)
                                .min_size(egui::vec2(22.0, 22.0)),
                        );
                        let plus_pos = plus.rect.left_bottom();
                        if plus.on_hover_text("New project or folder").clicked() {
                            self.proj_plus_open = true;
                            self.proj_plus_pos = plus_pos;
                            self.proj_ignore_close = true;
                        }
                    });
                });
                let tree = visible_tree(&self.projects);
                let mut proj_act: Option<(String, ProjectMenuAct, egui::Pos2)> = None;
                for (depth, idx) in tree {
                    let kind = self.projects[idx].kind;
                    let open = self.projects[idx].open;
                    let indent = 20.0 * depth as f32;
                    if self.proj_rename.as_deref() == Some(self.projects[idx].id.as_str()) {
                        ui.horizontal(|ui| {
                            ui.add_space(indent);
                            let edit = ui.add(
                                egui::TextEdit::singleline(&mut self.proj_rename_buf)
                                    .desired_width(ui.available_width() - 8.0)
                                    .hint_text("Name")
                                    .font(egui::FontId::proportional(13.0)),
                            );
                            if self.proj_rename_focus {
                                edit.request_focus();
                                if edit.has_focus() {
                                    self.proj_rename_focus = false;
                                }
                            }
                            if let Some(lock) = self.proj_rename_lock.clone() {
                                if self.proj_rename_buf == lock {
                                    select_all_edit(ui, edit.id, &self.proj_rename_buf);
                                } else {
                                    self.proj_rename_lock = None;
                                }
                            }
                            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                self.cancel_proj_rename();
                            } else if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                self.finish_proj_rename();
                            } else if edit.lost_focus() && !self.proj_rename_focus {
                                self.finish_proj_rename();
                            }
                        });
                        continue;
                    }
                    let icon = match kind {
                        ProjectKind::Folder => crate::icons::RailIcon::Folder,
                        ProjectKind::Project => crate::icons::RailIcon::Chat,
                    };
                    let active = project_row_active(
                        self.project_sel.as_deref() == Some(self.projects[idx].id.as_str()),
                        kind == ProjectKind::Project,
                        self.nav,
                    );
                    let row = ui
                        .horizontal(|ui| {
                            ui.add_space(indent);
                            if kind == ProjectKind::Folder {
                                crate::icons::paint_folder_caret(
                                    ui,
                                    open,
                                    crate::theme::subtle(),
                                );
                            }
                            Self::nav_row(ui, active, icon, &self.projects[idx].name, false)
                        })
                        .inner;
                    if row.double_clicked() {
                        self.begin_proj_rename(
                            self.projects[idx].id.clone(),
                            self.projects[idx].name.clone(),
                        );
                    } else if row.clicked() {
                        let id = self.projects[idx].id.clone();
                        match kind {
                            ProjectKind::Folder => {
                                toggle_folder(&mut self.projects, &id);
                                self.touch_projects();
                                self.flush_projects();
                            }
                            ProjectKind::Project => self.bind_project_id(&id),
                        }
                    }
                    let nid = self.projects[idx].id.clone();
                    let row_pos = row.rect.left_bottom();
                    row.context_menu(|ui| {
                        for a in project_menu_acts(kind) {
                            if ui.button(project_menu_label(*a)).clicked() {
                                proj_act = Some((nid.clone(), *a, row_pos));
                                ui.close_menu();
                            }
                        }
                    });
                }
                if let Some((id, act, pos)) = proj_act {
                    self.proj_menu_pos = pos;
                    self.apply_project_menu(id, act);
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("History").size(12.0).color(crate::theme::subtle()));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(RichText::new("See all").size(11.0).color(crate::theme::subtle()))
                                    .fill(egui::Color32::TRANSPARENT)
                                    .stroke(egui::Stroke::NONE),
                            )
                            .clicked()
                        {
                            self.nav = Nav::History;
                        }
                    });
                });
                crate::cards::search_bar(
                    ui,
                    &mut self.sidebar_q,
                    "Filter chats…",
                    (ui.available_width() - 8.0).max(80.0),
                );
                let hist_h = (ui.available_height() - RAIL_FOOTER_H).max(36.0);
                egui::ScrollArea::vertical()
                    .id_salt("rail-history")
                    .auto_shrink([false, true])
                    .max_height(hist_h)
                    .show(ui, |ui| {
                        let q = self.sidebar_q.to_ascii_lowercase();
                        let pinned: Vec<bool> = self.threads.iter().map(|t| t.pinned).collect();
                        let order = history_order(&pinned);
                        let mut act: Option<TabAct> = None;
                        for i in order {
                            let title = self.threads[i].title.clone();
                            if !q.is_empty() && !title.to_ascii_lowercase().contains(&q) {
                                continue;
                            }
                            if self.rename_idx == Some(i) {
                                let edit = ui.add(
                                    egui::TextEdit::singleline(&mut self.rename_buf)
                                        .desired_width(ui.available_width())
                                        .hint_text("Name this chat")
                                        .font(egui::FontId::proportional(13.0)),
                                );
                                if self.rename_focus {
                                    edit.request_focus();
                                    if edit.has_focus() {
                                        self.rename_focus = false;
                                    }
                                }
                                if let Some(lock) = self.rename_lock.clone() {
                                    if self.rename_buf == lock {
                                        select_all_edit(ui, edit.id, &self.rename_buf);
                                    } else {
                                        self.rename_lock = None;
                                    }
                                }
                                if ui.input(|inp| inp.key_pressed(egui::Key::Escape)) {
                                    act = Some(TabAct::CancelRename);
                                } else if ui.input(|inp| inp.key_pressed(egui::Key::Enter)) {
                                    act = Some(TabAct::CommitRename(i));
                                } else if edit.lost_focus() && !self.rename_focus {
                                    act = Some(TabAct::CommitRename(i));
                                }
                                continue;
                            }
                            let icon = if self.threads[i].pinned {
                                crate::icons::RailIcon::Pin
                            } else {
                                crate::icons::RailIcon::Chat
                            };
                            let resp = Self::nav_row(
                                ui,
                                i == self.thread_idx && self.nav == Nav::Chat,
                                icon,
                                &display_tab_title(&title),
                                false,
                            );
                            if resp.double_clicked() {
                                act = Some(TabAct::StartRename(i));
                            } else if resp.clicked() {
                                act = Some(TabAct::Switch(i));
                            }
                            let pinned = self.threads[i].pinned;
                            resp.context_menu(|ui| {
                                if ui.button(if pinned { "Unpin" } else { "Pin" }).clicked() {
                                    act = Some(TabAct::Pin(i));
                                    ui.close_menu();
                                }
                                if ui.button("Rename").clicked() {
                                    act = Some(TabAct::StartRename(i));
                                    ui.close_menu();
                                }
                                if ui.button("Delete").clicked() {
                                    act = Some(TabAct::Delete(i));
                                    ui.close_menu();
                                }
                            });
                        }
                        match act {
                            Some(TabAct::Switch(i)) => {
                                self.switch_thread(i);
                                self.nav = Nav::Chat;
                            }
                            Some(TabAct::Pin(i)) => self.pin_thread(i),
                            Some(TabAct::StartRename(i)) => self.begin_chat_rename(i),
                            Some(TabAct::CommitRename(i)) => {
                                let name = self.rename_buf.clone();
                                self.rename_thread(i, &name);
                            }
                            Some(TabAct::CancelRename) => {
                                self.rename_idx = None;
                                self.rename_focus = false;
                                self.rename_lock = None;
                            }
                            Some(TabAct::Delete(i)) => self.delete_thread_at(i),
                            None => {}
                        }
                    });
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    if Self::cabin_avatar(ui, &account, &email, self.oauth_photo.as_ref()).clicked() {
                        self.settings_menu_open = !self.settings_menu_open;
                        self.settings_menu_ignore = true;
                    }
                });
            });
    }

    fn ui_chat(&mut self, ctx: &egui::Context) {
        let empty = self.messages.is_empty();
        if !empty {
            egui::TopBottomPanel::bottom("composer")
                .frame(
                    egui::Frame::none()
                        .fill(crate::theme::bg())
                        .inner_margin(egui::Margin {
                            left: 32.0,
                            right: 32.0,
                            top: 10.0,
                            bottom: 22.0,
                        }),
                )
                .show(ctx, |ui| {
                    self.ui_composer_stack(ui);
                });
        }
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(crate::theme::bg()).inner_margin(egui::Margin::same(20.0)))
            .show(ctx, |ui| {
                if empty {
                    self.ui_empty_home(ui);
                    return;
                }
                egui::ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        let pairs: Vec<(String, String)> = self
                            .messages
                            .iter()
                            .map(|m| (m.role.clone(), m.content.clone()))
                            .collect();
                        let views = visible_chat(&pairs);
                        let last_thought = views.iter().rposition(|v| v.kind == ChatKind::Thought);
                        for (i, block) in views.iter().enumerate() {
                            paint_chat_block(
                                ui,
                                block,
                                i,
                                self.thinking_here() && last_thought == Some(i),
                            );
                            ui.add_space(10.0);
                        }
                        if self.thinking_here() {
                            match views.last().map(|v| v.kind) {
                                None | Some(ChatKind::User) => {
                                    ui.label(
                                        RichText::new("Thinking…")
                                            .size(crate::theme::GREETING_SIZE)
                                            .color(crate::theme::whisper()),
                                    );
                                }
                                Some(ChatKind::Assistant)
                                | Some(ChatKind::Thought)
                                | Some(ChatKind::Tool) => {}
                            }
                        }
                    });
            });
    }

    fn ui_empty_home(&mut self, ui: &mut egui::Ui) {
        let greet_on = should_paint_greeting(self.messages.is_empty(), self.scratch())
            && !self.greeting.is_empty();
        let greet_h = if greet_on {
            crate::theme::GREETING_SIZE + 28.0
        } else {
            28.0
        };
        let block = crate::theme::WORDMARK + greet_h + crate::theme::QUERY_MIN_H;
        let lift = ((ui.available_height() - block) * 0.5).clamp(24.0, 320.0);
        ui.add_space(lift);
        let pane_w = crate::cards::composer_pill_w(ui.ctx().screen_rect().width());
        ui.vertical_centered_justified(|ui| {
            ui.set_max_width(pane_w);
            ui.label(
                RichText::new("GrokHub")
                    .font(crate::theme::title_font(crate::theme::WORDMARK))
                    .color(crate::theme::fg()),
            );
            if greet_on {
                ui.add_space(10.0);
                ui.label(
                    RichText::new(&self.greeting)
                        .size(crate::theme::GREETING_SIZE)
                        .italics()
                        .color(crate::theme::whisper()),
                );
                ui.add_space(18.0);
            } else {
                ui.add_space(28.0);
            }
            self.ui_composer_stack(ui);
        });
    }

    fn ui_composer_stack(&mut self, ui: &mut egui::Ui) {
            ui.add_space(6.0);
            ui.vertical_centered_justified(|ui| {
            let col_w = crate::cards::composer_pill_w(ui.ctx().screen_rect().width());
            ui.set_max_width(col_w);
            for slot in composer_stack_order() {
                match slot {
                    ComposerStackSlot::AuthBanner => {
                        if needs_auth_banner(self.has_key()) {
                            ui.horizontal(|ui| {
                                crate::cards::settings_note(ui, "Connect Grok to chat.");
                                if crate::cards::ghost_pill(ui, "Settings") {
                                    self.nav = Nav::Settings;
                                }
                            });
                        }
                    }
                    ComposerStackSlot::SlashPalette => {
                        let hits = filter_slash_commands(&self.composer);
                        if !hits.is_empty() {
                            let first = hits.first().map(|s| s.cmd).unwrap_or("");
                            let n = hits.len();
                            let changed = self.slash_filter_n != n || self.slash_filter_first != first;
                            self.slash_pick = slash_pick_retain(self.slash_pick, changed, n);
                            self.slash_filter_n = n;
                            self.slash_filter_first = first;
                            ui.label(
                                RichText::new("↑↓  Tab accepts")
                                    .size(crate::theme::FONT_META)
                                    .color(crate::theme::subtle()),
                            );
                            ui.add_space(4.0);
                            egui::Frame::none()
                                .fill(crate::theme::elevated())
                                .rounding(12.0)
                                .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                                .inner_margin(egui::Margin::same(8.0))
                                .show(ui, |ui| {
                                    egui::ScrollArea::vertical()
                                        .max_height(148.0)
                                        .auto_shrink([false, true])
                                        .show(ui, |ui| {
                                            for (i, s) in hits.iter().enumerate() {
                                                let on = i == self.slash_pick;
                                                let row = format!("{}  {}", s.cmd, s.hint);
                                                let fill = if on {
                                                    crate::theme::nav_active()
                                                } else {
                                                    egui::Color32::TRANSPARENT
                                                };
                                                if crate::theme::pointing(
                                                    ui.add(
                                                        egui::Button::new(
                                                            RichText::new(row)
                                                                .size(13.0)
                                                                .color(if on {
                                                                    crate::theme::fg()
                                                                } else {
                                                                    crate::theme::muted()
                                                                }),
                                                        )
                                                        .fill(fill)
                                                        .rounding(8.0)
                                                        .min_size(egui::vec2(ui.available_width(), 28.0)),
                                                    ),
                                                )
                                                .clicked()
                                                {
                                                    if let Some(t) = slash_pick_take(
                                                        &mut self.composer,
                                                        s.insert,
                                                        s.run_on_pick,
                                                    ) {
                                                        self.send_chat(t);
                                                    }
                                                }
                                            }
                                        });
                                });
                            if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)) {
                                self.slash_pick = slash_pick_step(self.slash_pick, hits.len(), 1);
                            } else if ui.input_mut(|i| {
                                i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)
                            }) {
                                self.slash_pick = slash_pick_step(self.slash_pick, hits.len(), -1);
                            } else if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Tab))
                            {
                                let s = hits[self.slash_pick.min(hits.len() - 1)];
                                if let Some(t) =
                                    slash_pick_take(&mut self.composer, s.insert, s.run_on_pick)
                                {
                                    self.send_chat(t);
                                }
                            }
                            ui.add_space(6.0);
                        } else {
                            self.slash_pick = 0;
                            self.slash_filter_n = 0;
                            self.slash_filter_first = "";
                        }
                    }
                    ComposerStackSlot::Chips => {
            if let Some(act) = crate::cards::quick_chip_row(ui, &self.visible_chips) {
                match act {
                    crate::cards::ChipRowAct::Apply(i) => {
                        if let Some(c) = self.visible_chips.get(i).cloned() {
                            self.apply_chip(c);
                        }
                    }
                    crate::cards::ChipRowAct::Dismiss(i) => {
                        if let Some(c) = self.visible_chips.get(i).cloned() {
                            self.dismiss_chip(c);
                            self.refresh_chips();
                        }
                    }
                }
            }
                    }
                    ComposerStackSlot::Attach => {
            self.ui_attach_chip(ui, PlusTarget::Chat);
                    }
                    ComposerStackSlot::Pill => {
            let pill_w = crate::cards::composer_pill_w(ui.ctx().screen_rect().width());
            let cap = pill_w.min(ui.available_width()).max(360.0);
            ui.allocate_ui_with_layout(
                egui::vec2(cap, crate::theme::QUERY_MIN_H),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_width(cap);
                    ui.set_max_width(cap);
            egui::Frame::none()
                .fill(crate::theme::elevated())
                .rounding(crate::theme::QUERY_RADIUS)
                .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                .inner_margin(egui::Margin::same(8.0))
                .show(ui, |ui| {
                    let inner = (cap - 16.0).max(200.0);
                    ui.set_width(inner);
                    ui.set_max_width(inner);
                    ui.set_min_height(crate::theme::QUERY_MIN_H - 16.0);
                    ui.spacing_mut().item_spacing.x = 8.0;
                    ui.horizontal(|ui| {
                        let plus = crate::icons::paint_bar_icon(
                            ui,
                            crate::icons::BarIcon::Plus,
                            22.0,
                            crate::theme::muted(),
                        )
                        .on_hover_text("Upload a file or paste clipboard");
                        if plus.clicked() {
                            self.open_plus(PlusTarget::Chat, plus.rect.left_bottom());
                        }
                        let composer_id = egui::Id::new("chat-composer");
                        let focused = ui.memory(|m| m.has_focus(composer_id));
                        if let Some(t) =
                            take_focused_composer(ui, &mut self.composer, focused)
                        {
                            self.send_chat(t);
                        }
                        let cluster = crate::cards::composer_go_cluster_w();
                        let go_sz = crate::cards::composer_go_hit_w();
                        let mid = crate::cards::composer_mid_w(inner);
                        let rows = (self.composer.matches('\n').count() + 1).min(8);
                        let bar_h = crate::theme::QUERY_MIN_H - 16.0;
                        ui.allocate_ui_with_layout(
                            egui::vec2(mid, bar_h),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                ui.spacing_mut().item_spacing.x = 8.0;
                                let text_w = (ui.available_width() - cluster + go_sz).max(80.0);
                                let edit = ui.add(
                                    egui::TextEdit::multiline(&mut self.composer)
                                        .id(composer_id)
                                        .desired_width(text_w)
                                        .desired_rows(rows)
                                        .frame(false)
                                        .hint_text("What do you want to know?")
                                        .return_key(Some(egui::KeyboardShortcut::new(
                                            egui::Modifiers::COMMAND,
                                            egui::Key::Enter,
                                        ))),
                                );
                                if let Some(t) = take_focused_composer(
                                    ui,
                                    &mut self.composer,
                                    edit.has_focus(),
                                ) {
                                    self.send_chat(t);
                                }
                                let mode_now = if self.cfg.mode.trim().is_empty() {
                                    "auto".to_string()
                                } else {
                                    self.cfg.mode.clone()
                                };
                                if let Some(mode) = crate::cards::mode_pill(ui, &mode_now) {
                                    self.run_slash(Slash::Mode(mode));
                                }
                                if crate::icons::paint_bar_icon(
                                    ui,
                                    crate::icons::BarIcon::Mic,
                                    22.0,
                                    crate::theme::muted(),
                                )
                                .on_hover_text("Hey Grok")
                                .clicked()
                                {
                                    self.listen_voice();
                                }
                            },
                        );
                        let ready = !self.composer.trim().is_empty();
                        let go = composer_go(self.running, ready);
                        ui.allocate_ui_with_layout(
                            egui::vec2(go_sz, bar_h),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                let send = crate::icons::paint_bar_icon(
                                    ui,
                                    match go {
                                        ComposerGo::Stop => crate::icons::BarIcon::Stop,
                                        ComposerGo::Send => crate::icons::BarIcon::Send,
                                        ComposerGo::Idle => crate::icons::BarIcon::ArrowUp,
                                    },
                                    match go {
                                        ComposerGo::Idle => 22.0,
                                        ComposerGo::Send | ComposerGo::Stop => 28.0,
                                    },
                                    match go {
                                        ComposerGo::Idle => crate::theme::muted(),
                                        ComposerGo::Send | ComposerGo::Stop => {
                                            crate::theme::fg()
                                        }
                                    },
                                )
                                .on_hover_text(composer_go_tip(self.running));
                                let go_hit = send.clicked()
                                    || (send.is_pointer_button_down_on()
                                        && ui.input(|i| i.pointer.primary_pressed()));
                                match go {
                                    ComposerGo::Stop => {
                                        if go_hit {
                                            self.run_slash(Slash::Stop);
                                        }
                                    }
                                    ComposerGo::Send | ComposerGo::Idle => {
                                        if go_hit {
                                            let t = std::mem::take(&mut self.composer);
                                            self.send_chat(t);
                                        }
                                    }
                                }
                            },
                        );
                    });
                });
                },
            );
                    }
                }
            }
            });
    }

    fn ui_devices(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(crate::theme::bg()).inner_margin(egui::Margin::same(24.0)))
            .show(ctx, |ui| {
            if crate::cards::page_header(ui, "Devices", if self.hub_on { "Sharing" } else { "Start share" }) {
                self.start_hub();
            }
            crate::cards::section_label(ui, "This computer");
            let (name, sharing, pair_code) = if let Ok(st) = self.hub.lock() {
                (
                    st.device_name.clone(),
                    self.hub_on,
                    st.pair.as_ref().map(|p| p.code.clone()),
                )
            } else {
                (String::new(), false, None)
            };
            let body = if sharing {
                format!("Sharing on port {}", self.hub_port)
            } else {
                "Not sharing. Start share to pair a phone or another computer.".into()
            };
            crate::cards::grok_tile(
                ui,
                crate::icons::TileIcon::Host,
                if name.is_empty() { "This cabin" } else { &name },
                &body,
                None,
                sharing,
            );
            ui.add_space(16.0);
            crate::cards::section_label(ui, "Pair");
            if let Some(code) = pair_code {
                crate::cards::grok_tile(
                    ui,
                    crate::icons::TileIcon::Connect,
                    &code,
                    &format!("Open http://<lan>:{} on the other device.", self.hub_port),
                    None,
                    false,
                );
            } else if sharing {
                ui.label(
                    RichText::new("Paired. Make a new code after another device joins.")
                        .size(13.0)
                        .color(crate::theme::muted()),
                );
                ui.add_space(8.0);
                if crate::cards::ghost_pill(ui, "New code") {
                    if let Ok(mut s) = self.hub.lock() {
                        s.rotate_pair();
                    }
                }
            } else if crate::cards::empty_prompt_tile(
                ui,
                crate::icons::TileIcon::Connect,
                "No pair code",
                "Start share to mint a code for another device.",
            ) {
                self.start_hub();
            }
            ui.add_space(16.0);
            crate::cards::section_label(ui, "Send a task");
            egui::Frame::none()
                .fill(crate::theme::elevated())
                .rounding(12.0)
                .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                .inner_margin(egui::Margin::same(12.0))
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut self.task_prompt)
                            .desired_rows(3)
                            .desired_width(f32::INFINITY)
                            .frame(false)
                            .hint_text("What should this computer do?"),
                    );
                });
            ui.add_space(8.0);
            if crate::cards::white_pill(ui, "Send a task home") {
                let t = std::mem::take(&mut self.task_prompt);
                self.nav = Nav::Chat;
                self.send_chat(t);
            }
        });
    }

    fn ui_memory(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(crate::theme::bg()).inner_margin(egui::Margin::same(24.0)))
            .show(ctx, |ui| {
            let _ = crate::cards::page_header(ui, "Memory", "");
            ui.horizontal(|ui| {
                for name in ["SOUL.md", "USER.md", "MEMORY.md"] {
                    if crate::cards::tab_pill(ui, name, self.mem_name == name) {
                        self.mem_name = name.into();
                        self.mem_body = config::read_memory(name);
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if crate::cards::ghost_pill(ui, "Restore") {
                        match config::restore_memory(&self.mem_name) {
                            Ok(body) => {
                                self.mem_body = body;
                                self.status = format!("Restored {}.prev", self.mem_name);
                            }
                            Err(e) => self.status = e,
                        }
                    }
                    if crate::cards::ghost_pill(ui, "Reflect") {
                        self.run_reflect();
                    }
                    if crate::cards::white_pill(ui, "Save") {
                        match config::write_memory(&self.mem_name, &self.mem_body) {
                            Ok(()) => self.status = format!("Wrote {}", self.mem_name),
                            Err(e) => self.status = e,
                        }
                    }
                });
            });
            if !self.reflect_diff.is_empty() {
                ui.add_space(8.0);
                ui.label(RichText::new("Last reflect").size(12.0).color(crate::theme::subtle()));
                ui.label(RichText::new(&self.reflect_diff).monospace().size(12.0).color(crate::theme::muted()));
            }
            ui.add_space(12.0);
            egui::Frame::none()
                .fill(crate::theme::elevated())
                .rounding(12.0)
                .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                .inner_margin(egui::Margin::same(12.0))
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut self.mem_body)
                            .desired_rows(24)
                            .desired_width(f32::INFINITY)
                            .frame(false)
                            .font(egui::TextStyle::Monospace),
                    );
                });
        });
    }

    fn save_settings(&mut self) {
        if let Ok(mut st) = self.hub.lock() {
            if !self.cfg.device_name.trim().is_empty() {
                st.device_name = self.cfg.device_name.clone();
            }
        }
        let _ = secrets::save(&self.secrets);
        match config::save(&self.cfg) {
            Ok(()) => self.status = "Saved".into(),
            Err(e) => self.status = e,
        }
    }

    fn ui_settings(&mut self, ctx: &egui::Context) {
        let mut save = false;
        let mut connect = false;
        let mut disconnect = false;
        let mut update = false;
        let mut restart = false;
        let mut copy_diag = false;
        let oauth_line = self.secrets.oauth.as_ref().map(|t| {
            t.email
                .clone()
                .or(t.name.clone())
                .unwrap_or_else(|| "connected".into())
        });
        let pending = self.oauth_pending.as_ref().map(|p| {
            format!("Approve {} at {}", p.user_code, p.verification_uri)
        });
        let imagine_live = dedicated_imagine_model(&self.cfg.imagine_model);
        let voice_live = dedicated_voice_model(&self.cfg.voice_model);
        let doctor = self.doctor_text();
        let usage = usage_line(&self.usage);
        let catalog = catalog_line();
        let mut close = false;
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
            close = true;
        }
        let mut next_sec: Option<SettingsSec> = None;
        let sec = self.settings_sec;
        let screen = ctx.screen_rect();
        egui::Area::new(egui::Id::new("settings-overlay"))
            .fixed_pos(screen.min)
            .order(egui::Order::Foreground)
            .interactable(true)
            .show(ctx, |ui| {
                ui.set_min_size(screen.size());
                ui.painter()
                    .rect_filled(screen, 0.0, Color32::from_black_alpha(180));
                let modal = egui::Rect::from_center_size(
                    screen.center(),
                    egui::vec2(920.0, 620.0).min(screen.size() - egui::vec2(48.0, 48.0)),
                );
                ui.allocate_ui_at_rect(modal, |ui| {
                    egui::Frame::none()
                        .fill(crate::theme::bg())
                        .rounding(16.0)
                        .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                        .inner_margin(egui::Margin::ZERO)
                        .show(ui, |ui| {
                            ui.set_min_size(modal.size());
                            ui.horizontal(|ui| {
                                ui.allocate_ui_with_layout(
                                    egui::vec2(220.0, modal.height()),
                                    egui::Layout::top_down(egui::Align::Min),
                                    |ui| {
                                        egui::Frame::none()
                                            .fill(crate::theme::surface())
                                            .inner_margin(egui::Margin::same(12.0))
                                            .show(ui, |ui| {
                                                ui.set_width(196.0);
                                                ui.set_min_height(modal.height() - 24.0);
                                                if crate::cards::section_label(ui, "General") {
                                                    next_sec = Some(settings_group_home(SettingsGroup::General));
                                                }
                                                for (s, label) in [
                                                    (SettingsSec::Account, "Account"),
                                                    (SettingsSec::Appearance, "Appearance"),
                                                    (SettingsSec::Behavior, "Behavior"),
                                                ] {
                                                    if crate::cards::settings_nav(ui, label, sec == s) {
                                                        next_sec = Some(s);
                                                    }
                                                }
                                                ui.add_space(10.0);
                                                if crate::cards::section_label(ui, "Cabin") {
                                                    next_sec = Some(settings_group_home(SettingsGroup::Cabin));
                                                }
                                                for (s, label) in [
                                                    (SettingsSec::Host, "Host"),
                                                    (SettingsSec::Imagine, "Imagine"),
                                                    (SettingsSec::Voice, "Voice"),
                                                    (SettingsSec::Night, "Night"),
                                                ] {
                                                    if crate::cards::settings_nav(ui, label, sec == s) {
                                                        next_sec = Some(s);
                                                    }
                                                }
                                                ui.add_space(10.0);
                                                if crate::cards::section_label(ui, "Data") {
                                                    next_sec = Some(settings_group_home(SettingsGroup::Data));
                                                }
                                                if crate::cards::settings_nav(ui, "GitHub", sec == SettingsSec::Github) {
                                                    next_sec = Some(SettingsSec::Github);
                                                }
                                                ui.add_space(10.0);
                                                if crate::cards::section_label(ui, "About") {
                                                    next_sec = Some(settings_group_home(SettingsGroup::About));
                                                }
                                                for (s, label) in [
                                                    (SettingsSec::Update, "Update"),
                                                    (SettingsSec::About, "About"),
                                                ] {
                                                    if crate::cards::settings_nav(ui, label, sec == s) {
                                                        next_sec = Some(s);
                                                    }
                                                }
                                            });
                                    },
                                );
                                ui.allocate_ui_with_layout(
                                    egui::vec2((modal.width() - 220.0).max(320.0), modal.height()),
                                    egui::Layout::top_down(egui::Align::Min),
                                    |ui| {
                                        ui.add_space(16.0);
                                        ui.horizontal(|ui| {
                                            ui.add_space(20.0);
                                            ui.label(
                                                RichText::new(settings_sec_title(sec))
                                                    .font(crate::theme::title_font(22.0))
                                                    .color(crate::theme::fg()),
                                            );
                                            ui.with_layout(
                                                egui::Layout::right_to_left(egui::Align::Center),
                                                |ui| {
                                                    ui.add_space(16.0);
                                                    if ui
                                                        .add(
                                                            egui::Button::new(
                                                                RichText::new("×")
                                                                    .size(18.0)
                                                                    .color(crate::theme::muted()),
                                                            )
                                                            .fill(Color32::TRANSPARENT)
                                                            .stroke(egui::Stroke::NONE),
                                                        )
                                                        .clicked()
                                                    {
                                                        close = true;
                                                    }
                                                    if crate::cards::ghost_pill(ui, "Save") {
                                                        save = true;
                                                    }
                                                },
                                            );
                                        });
                                        ui.add_space(12.0);
                                        egui::ScrollArea::vertical()
                                            .auto_shrink([false, false])
                                            .show(ui, |ui| {
                                                ui.set_width((modal.width() - 260.0).max(280.0));
                                                ui.add_space(8.0);
                                                ui.indent("settings-body", |ui| {
                                                    match sec {
                                                        SettingsSec::Account => {
                                                            let auth_title = if oauth_line.is_some() {
                                                                "Connected"
                                                            } else {
                                                                "Connect Grok"
                                                            };
                                                            let auth_hint = oauth_line.as_deref().unwrap_or(
                                                                "Device-code OAuth. Same public client as Grok CLI.",
                                                            );
                                                            if crate::cards::settings_action(
                                                                ui,
                                                                auth_title,
                                                                auth_hint,
                                                                if oauth_line.is_some() { "Sign out" } else { "Connect" },
                                                            ) {
                                                                if oauth_line.is_some() {
                                                                    disconnect = true;
                                                                } else {
                                                                    connect = true;
                                                                }
                                                            }
                                                            if let Some(p) = &pending {
                                                                crate::cards::settings_note(ui, p);
                                                            }
                                                            crate::cards::settings_field(ui, "Console key", "Fallback for chat. Required for duplex Voice (grok-voice-think-fast-2.0). Never in markdown.", &mut self.cfg.api_key, true);
                                                            crate::cards::settings_field(ui, "Device name", "How this box shows up on the hub.", &mut self.cfg.device_name, false);
                                                            crate::cards::settings_field(ui, "Chat model", "Empty or a ladder default (mini / 4.3 / 4.6) lets Auto route. A catalog pin such as grok-3 skips Auto. Imagine never shares this.", &mut self.cfg.model, false);
                                                            crate::cards::settings_field(ui, "Composer mode", "Auto routes Fast / Balance / Think / Max. Fast is Grok 3 mini. Balance is Grok 4.3. Think is Grok 4.6 high. Max is Grok 4.6 xhigh. The combo does not overwrite Chat model.", &mut self.cfg.mode, false);
                                                        }
                                                        SettingsSec::Appearance => {
                                                            crate::cards::settings_note(
                                                                ui,
                                                                appearance_hint(),
                                                            );
                                                            ui.horizontal(|ui| {
                                                                let current = parse_theme(&self.cfg.theme);
                                                                let os_dark = crate::theme::desktop_prefers_dark();
                                                                for choice in appearance_choices() {
                                                                    let on = current == *choice;
                                                                    let preview = if resolve_dark(*choice, os_dark)
                                                                    {
                                                                        crate::theme::BG
                                                                    } else {
                                                                        crate::theme::LIGHT_BG
                                                                    };
                                                                    if crate::cards::appearance_card(
                                                                        ui,
                                                                        theme_label(*choice),
                                                                        on,
                                                                        preview,
                                                                    ) {
                                                                        if let Some(next) =
                                                                            pick_theme(current, *choice)
                                                                        {
                                                                            self.cfg.theme = theme_id(next).into();
                                                                            save = true;
                                                                        }
                                                                    }
                                                                    ui.add_space(10.0);
                                                                }
                                                            });
                                                        }
                                                        SettingsSec::Behavior => {
                                                            if crate::cards::settings_toggle(ui, "Close to tray", "The cabin keeps working in the background.", &mut self.cfg.close_to_tray) {
                                                                save = true;
                                                            }
                                                            if crate::cards::settings_toggle(ui, "Cabin eyes", "Armed. They look only when you ask, or when hands need a frame.", &mut self.cfg.cabin_eyes) {
                                                                save = true;
                                                            }
                                                        }
                                                        SettingsSec::Host => {
                                                            crate::cards::settings_note(ui, "Host is always on. The cabin drives. Halt stops a running job.");
                                                        }
                                                        SettingsSec::Imagine => {
                                                            crate::cards::settings_note(ui, &format!("Live still model: {imagine_live}. Chat models never run here."));
                                                            crate::cards::settings_field(ui, "Imagine override", "Must contain “image” or the cabin keeps grok-2-image.", &mut self.cfg.imagine_model, false);
                                                            if crate::cards::settings_toggle(
                                                                ui,
                                                                "Living wall",
                                                                "Every few hours the cabin paints a new cover. Twenty live. Oldest leaves first. Random seat.",
                                                                &mut self.cfg.imagine_wall,
                                                            ) {
                                                                save = true;
                                                            }
                                                            crate::cards::settings_note(
                                                                ui,
                                                                &format!(
                                                                    "{} of {WALL_GIF_MAX} covers on the wall.",
                                                                    self.wall.gifs.len()
                                                                ),
                                                            );
                                                        }
                                                        SettingsSec::Voice => {
                                                            crate::cards::settings_note(ui, &format!("Live voice model: {voice_live}."));
                                                            crate::cards::settings_note(
                                                                ui,
                                                                "OAuth runs Hey Grok STT and TTS. Duplex (wss://api.x.ai/v1/realtime) needs a console API key.",
                                                            );
                                                            crate::cards::settings_field(ui, "Voice override", "Must contain “voice” or “realtime”. Empty keeps grok-voice-think-fast-2.0.", &mut self.cfg.voice_model, false);
                                                        }
                                                        SettingsSec::Night => {
                                                            crate::cards::settings_note(ui, "Night always runs. Quiet hours and daily caps do not hold work.");
                                                        }
                                                        SettingsSec::Github => {
                                                            crate::cards::settings_field(ui, "Personal access token", "CONNECTOR_CMD only. GitHub is the only live connector.", &mut self.secrets.github_token, true);
                                                            crate::cards::settings_field(ui, "Bound project", "The world. Host, Imagine, and memory stay here.", &mut self.cfg.project_dir, false);
                                                        }
                                                        SettingsSec::Update => {
                                                            crate::cards::settings_note(ui, "Overlay only — git pull --ff-only origin main, then install.sh --user. The clone must be on main. Does not wipe ~/.config/GrokHub.");
                                                            crate::cards::settings_field(ui, "Source clone", "Empty uses GROKHUB_SRC or the install receipt.", &mut self.cfg.source_dir, false);
                                                            if crate::cards::settings_action(ui, "Install overlay", "Pulls this clone and runs the user install.", "Update") {
                                                                update = true;
                                                            }
                                                            if let Some(pct) = self.update_pct {
                                                                let fill = if self.last_receipt_ok == Some(false) && !self.running {
                                                                    crate::theme::OFFLINE
                                                                } else {
                                                                    crate::theme::LIVE
                                                                };
                                                                crate::cards::settings_progress(ui, pct, fill);
                                                            }
                                                            if self.update_can_restart
                                                                && crate::cards::settings_action(
                                                                    ui,
                                                                    "Restart GrokHub",
                                                                    "Relaunch the cabin, tray, and hub from the new overlay.",
                                                                    "Restart",
                                                                )
                                                            {
                                                                restart = true;
                                                            }
                                                            if !self.status.is_empty() {
                                                                crate::cards::settings_note(ui, &self.status);
                                                            }
                                                        }
                                                        SettingsSec::About => {
                                                            crate::cards::settings_note(ui, &usage);
                                                            crate::cards::settings_note(ui, &catalog);
                                                            crate::cards::settings_note(ui, &doctor);
                                                            if crate::cards::settings_action(ui, "Diagnostics", "Copy a redacted bundle. No secrets.", "Copy") {
                                                                copy_diag = true;
                                                            }
                                                            crate::cards::settings_note(ui, "GrokHub cabin.");
                                                        }
                                                    }
                                                });
                                            });
                                    },
                                );
                            });
                        });
                });
            });
        if let Some(s) = next_sec {
            self.settings_sec = s;
        }
        if close {
            self.nav = self.settings_back;
        }
        if connect {
            self.start_oauth();
        }
        if disconnect {
            self.sign_out_oauth();
        }
        if update {
            self.queue_update();
        }
        if restart {
            self.restart_after_update(ctx);
        }
        if copy_diag {
            let bundle = diagnostics_bundle(
                env!("CARGO_PKG_VERSION"),
                self.has_key(),
                HUB_KIND,
                self.skill_list.len(),
                self.last_receipt_ok,
                self.board.len(),
                &self.status,
            );
            ctx.output_mut(|o| o.copied_text = bundle);
            self.status = "Diagnostics copied".into();
        }
        if save {
            self.save_settings();
        }
    }

    fn add_automation_seed(&mut self, seed: &str) {
        if let Some(mut a) = parse_nl_automation(seed) {
            a.id = uid("auto");
            self.automations.push(a);
            let _ = crate::night::save(&self.automations);
            self.status = "Automation added".into();
        } else {
            self.status = "Need “every weekday at 9…” or “heartbeat every 15 min…”".into();
        }
    }

    fn ui_night(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(crate::theme::bg()).inner_margin(egui::Margin::same(24.0)))
            .show(ctx, |ui| {
            if crate::cards::page_header(ui, "Automations", "New Automation") {
                self.auto_compose = true;
            }
            egui::ScrollArea::vertical().show(ui, |ui| {
            if self.auto_compose {
                ui.add_space(12.0);
                egui::Frame::none()
                    .fill(crate::theme::elevated())
                    .rounding(12.0)
                    .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                    .inner_margin(egui::Margin::same(14.0))
                    .show(ui, |ui| {
                        ui.label(RichText::new("New automation").strong());
                        ui.add(
                            egui::TextEdit::singleline(&mut self.night_nl)
                                .hint_text("every weekday at 9, summarize the workboard")
                                .desired_width(f32::INFINITY),
                        );
                        ui.horizontal(|ui| {
                            if crate::cards::white_pill(ui, "Add") {
                                let seed = std::mem::take(&mut self.night_nl);
                                self.add_automation_seed(&seed);
                                if self.status == "Automation added" {
                                    self.auto_compose = false;
                                }
                            }
                            if crate::cards::ghost_pill(ui, "Cancel") {
                                self.auto_compose = false;
                            }
                        });
                    });
            }
            ui.add_space(8.0);
            crate::cards::section_label(ui, "Active");
            if self.status.starts_with("Night:") {
                crate::cards::status_chip(ui, &self.status, crate::cards::ChipTone::Live);
                ui.add_space(8.0);
            }
            let mut drop: Option<usize> = None;
            if self.automations.is_empty() {
                if crate::cards::empty_prompt_tile(
                    ui,
                    crate::icons::TileIcon::Moon,
                    "None yet",
                    "Pick a suggestion or add a new automation.",
                ) {
                    self.auto_compose = true;
                }
                ui.add_space(16.0);
            } else {
                for i in 0..self.automations.len() {
                    let title = self.automations[i].name.chars().take(40).collect::<String>();
                    let body = format!(
                        "{} {} · {} runs",
                        self.automations[i].schedule,
                        self.automations[i].time,
                        self.automations[i].run_count
                    );
                    egui::Frame::none()
                        .fill(crate::theme::elevated())
                        .rounding(14.0)
                        .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                        .inner_margin(egui::Margin::same(12.0))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.checkbox(&mut self.automations[i].enabled, "");
                                ui.vertical(|ui| {
                                    ui.label(
                                        RichText::new(&title).size(15.0).color(crate::theme::fg()),
                                    );
                                    ui.label(
                                        RichText::new(&body).size(12.0).color(crate::theme::muted()),
                                    );
                                });
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if crate::cards::ghost_pill(ui, "Remove") {
                                            drop = Some(i);
                                        }
                                        if crate::cards::white_pill(ui, "Run") {
                                            drop = Some(usize::MAX - i);
                                        }
                                    },
                                );
                            });
                        });
                    ui.add_space(8.0);
                }
                ui.add_space(12.0);
            }
            crate::cards::section_label(ui, "Suggested");
            ui.label(
                RichText::new(review_status_line(
                    self.suggestions.last_review_day.as_deref(),
                    &Self::local_day(),
                ))
                .size(12.0)
                .color(crate::theme::muted()),
            );
            ui.add_space(8.0);
            let active_names: Vec<String> = self.automations.iter().map(|a| a.name.clone()).collect();
            let auto_tiles = crate::cards::merge_suggested_autos(&self.suggestions.autos, &active_names);
            crate::cards::tile_row(ui, auto_tiles.len(), |ui, i| {
                let (icon, title, body, seed) = &auto_tiles[i];
                if matches!(
                    crate::cards::grok_tile(ui, *icon, title, body, Some("Add"), false),
                    crate::cards::TileHit::Add | crate::cards::TileHit::Body
                ) {
                    self.add_automation_seed(seed);
                }
            });
            if let Some(i) = drop {
                if i < self.automations.len() {
                    self.automations.remove(i);
                    let _ = crate::night::save(&self.automations);
                } else {
                    let idx = usize::MAX - i;
                    if let Some(a) = self.automations.get(idx) {
                        let inst = a.instructions.clone();
                        if let Some(id) = replay_automation_target(&inst) {
                            self.replay_saved_recipe(id);
                        } else {
                            self.nav = Nav::Chat;
                            self.send_chat(inst);
                        }
                    }
                }
            }
            });
        });
    }

    fn ui_history(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(crate::theme::bg()).inner_margin(egui::Margin::same(24.0)))
            .show(ctx, |ui| {
            let _ = crate::cards::page_header(ui, "History", "");
            ui.horizontal(|ui| {
                crate::cards::search_bar(ui, &mut self.history_q, "Search chats and memory", 320.0);
                if crate::cards::white_pill(ui, "Search") {
                    let mut rows: Vec<(String, String)> = vec![
                        ("SOUL.md".into(), config::read_memory("SOUL.md")),
                        ("USER.md".into(), config::read_memory("USER.md")),
                        ("MEMORY.md".into(), config::read_memory("MEMORY.md")),
                    ];
                    for t in &self.threads {
                        let body = t
                            .messages
                            .iter()
                            .map(|(_, c)| c.as_str())
                            .collect::<Vec<_>>()
                            .join("\n");
                        rows.push((t.title.clone(), body));
                    }
                    self.history_hits = search_corpus(&self.history_q, &rows);
                    self.status = format!("{} hits", self.history_hits.len());
                }
            });
            if self.history_hits.is_empty() && !self.history_q.is_empty() {
                ui.label(RichText::new("No matches.").size(13.0).color(crate::theme::muted()));
            }
            for h in &self.history_hits {
                egui::Frame::none()
                    .fill(crate::theme::elevated())
                    .rounding(10.0)
                    .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                    .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                    .show(ui, |ui| {
                        ui.label(RichText::new(h).size(13.0).color(crate::theme::fg()));
                    });
                ui.add_space(6.0);
            }
            ui.add_space(16.0);
            crate::cards::section_label(ui, "Chats");
            ui.label(
                RichText::new("Double-click to rename. Right-click to pin, rename, or delete.")
                    .size(12.0)
                    .color(crate::theme::subtle()),
            );
            egui::ScrollArea::vertical()
                .id_salt("history-chats")
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    let pinned: Vec<bool> = self.threads.iter().map(|t| t.pinned).collect();
                    let order = history_order(&pinned);
                    let mut act: Option<TabAct> = None;
                    for i in order {
                        let title = self.threads[i].title.clone();
                        if self.rename_idx == Some(i) {
                            let edit = ui.add(
                                egui::TextEdit::singleline(&mut self.rename_buf)
                                    .id_salt(("page", i))
                                    .desired_width(ui.available_width())
                                    .hint_text("Name this chat"),
                            );
                            if self.rename_focus {
                                edit.request_focus();
                                if edit.has_focus() {
                                    self.rename_focus = false;
                                }
                            }
                            if ui.input(|inp| inp.key_pressed(egui::Key::Escape)) {
                                act = Some(TabAct::CancelRename);
                            } else if ui.input(|inp| inp.key_pressed(egui::Key::Enter)) {
                                act = Some(TabAct::CommitRename(i));
                            } else if edit.lost_focus() && !self.rename_focus {
                                act = Some(TabAct::CommitRename(i));
                            }
                            continue;
                        }
                        let icon = if self.threads[i].pinned {
                            crate::icons::RailIcon::Pin
                        } else {
                            crate::icons::RailIcon::Chat
                        };
                        let resp = Self::nav_row(
                            ui,
                            i == self.thread_idx && self.nav == Nav::Chat,
                            icon,
                            &display_tab_title(&title),
                            false,
                        );
                        if resp.double_clicked() {
                            act = Some(TabAct::StartRename(i));
                        } else if resp.clicked() {
                            act = Some(TabAct::Switch(i));
                        }
                        let pinned_on = self.threads[i].pinned;
                        resp.context_menu(|ui| {
                            if ui
                                .button(if pinned_on { "Unpin" } else { "Pin" })
                                .clicked()
                            {
                                act = Some(TabAct::Pin(i));
                                ui.close_menu();
                            }
                            if ui.button("Rename").clicked() {
                                act = Some(TabAct::StartRename(i));
                                ui.close_menu();
                            }
                            if ui.button("Delete").clicked() {
                                act = Some(TabAct::Delete(i));
                                ui.close_menu();
                            }
                        });
                    }
                    match act {
                        Some(TabAct::Switch(i)) => {
                            self.switch_thread(i);
                            self.nav = Nav::Chat;
                        }
                        Some(TabAct::Pin(i)) => self.pin_thread(i),
                        Some(TabAct::StartRename(i)) => self.begin_chat_rename(i),
                        Some(TabAct::CommitRename(i)) => {
                            let name = self.rename_buf.clone();
                            self.rename_thread(i, &name);
                        }
                        Some(TabAct::CancelRename) => {
                            self.rename_idx = None;
                            self.rename_focus = false;
                            self.rename_lock = None;
                        }
                        Some(TabAct::Delete(i)) => self.delete_thread_at(i),
                        None => {}
                    }
                });
        });
    }

    fn ui_board(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(crate::theme::bg()).inner_margin(egui::Margin::same(24.0)))
            .show(ctx, |ui| {
            if crate::cards::page_header(ui, "Workboard", "New card") {
                self.board_compose = true;
            }
            if self.board_compose {
                egui::Frame::none()
                    .fill(crate::theme::elevated())
                    .rounding(16.0)
                    .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                    .inner_margin(egui::Margin::same(14.0))
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.board_title)
                                .hint_text("Card title")
                                .desired_width(f32::INFINITY)
                                .frame(false),
                        );
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if crate::cards::white_pill(ui, "Add") && !self.board_title.trim().is_empty() {
                                self.board.push(BoardCard::new(
                                    &std::mem::take(&mut self.board_title),
                                    "",
                                    "",
                                ));
                                self.board_compose = false;
                                self.persist();
                            }
                            if crate::cards::ghost_pill(ui, "Cancel") {
                                self.board_compose = false;
                                self.board_title.clear();
                            }
                        });
                    });
                ui.add_space(16.0);
            }
            crate::cards::section_label(ui, "Open");
            let mut bump: Option<(usize, BoardStatus)> = None;
            if self.board.is_empty() {
                let _ = crate::cards::empty_prompt_tile(
                    ui,
                    crate::icons::TileIcon::Board,
                    "No cards yet",
                    "Pin a task from chat, or add one here.",
                );
            } else {
                let n = self.board.len();
                crate::cards::tile_row(ui, n, |ui, i| {
                    let c = &self.board[i];
                    let body = if c.detail.is_empty() {
                        c.status.as_str().to_string()
                    } else {
                        format!("{} · {}", c.status.as_str(), c.detail.chars().take(72).collect::<String>())
                    };
                    crate::cards::grok_tile(
                        ui,
                        crate::icons::TileIcon::Board,
                        &c.title,
                        &body,
                        None,
                        false,
                    );
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if crate::cards::ghost_pill(ui, "Open") {
                            bump = Some((i, BoardStatus::Approved));
                        }
                        if crate::cards::ghost_pill(ui, "Start") {
                            bump = Some((i, BoardStatus::InProgress));
                        }
                        if crate::cards::ghost_pill(ui, "Done") {
                            bump = Some((i, BoardStatus::Done));
                        }
                        if crate::cards::ghost_pill(ui, "Dismiss") {
                            bump = Some((i, BoardStatus::Dismissed));
                        }
                    });
                });
            }
            if let Some((i, st)) = bump {
                if let Some(c) = self.board.get_mut(i) {
                    c.status = st;
                }
                self.persist();
            }
        });
    }

    fn ui_imagine(&mut self, ctx: &egui::Context) {
        let mut generate = false;
        let mut stop = false;
        let mut new_project = false;
        let mut go_settings = false;
        let mut seed: Option<String> = None;
        let word = crate::cards::imagine_word(now_ms());
        let selected = self.imagine_prompt.clone();
        let last = self.imagine_last.clone();
        let dock = imagine_toolbox_dock(
            !self.imagine_prompt.trim().is_empty(),
            !last.is_empty(),
            self.running,
        );
        let show_result = imagine_shows_result_above(!last.is_empty(), dock);
        let composer_id = egui::Id::new("imagine-composer");
        let cap = if imagine_toolbox_shows_title(dock) {
            260.0
        } else {
            180.0
        };
        let measured = ctx
            .memory(|m| m.area_rect(composer_id).map(|r| r.height()))
            .unwrap_or(0.0);
        let box_h = if measured > 80.0 {
            measured.min(cap)
        } else {
            cap - 40.0
        };
        let panel = egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(crate::theme::bg()).inner_margin(egui::Margin::ZERO))
            .show(ctx, |ui| {
                let content = ui.max_rect();
                let toolbox_top =
                    imagine_toolbox_top(content.top(), content.height(), box_h, dock);
                let (wall_top, wall_h) = imagine_wall_bounds(
                    content.top(),
                    content.height(),
                    toolbox_top,
                    box_h,
                    dock,
                );
                if wall_h <= 8.0 {
                    return;
                }
                let wall = egui::Rect::from_min_size(
                    egui::pos2(content.left(), wall_top),
                    egui::vec2(content.width(), wall_h),
                );
                ui.allocate_ui_at_rect(wall, |ui| {
                    ui.set_clip_rect(wall);
                    if show_result {
                        crate::cards::imagine_result_hero(ui, &last);
                    } else {
                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                                crate::cards::imagine_masonry(
                                    ui,
                                    &selected,
                                    now_ms(),
                                    &self.wall.gifs,
                                    |p| {
                                        seed = Some(p);
                                    },
                                );
                            });
                    }
                });
            });
        let content = panel.response.rect;
        let bar_w = (content.width() - 48.0)
            .min(crate::theme::IMAGINE_BAR_W)
            .max(280.0);
        let y = imagine_toolbox_top(content.top(), content.height(), box_h, dock);
        let x = content.center().x - bar_w * 0.5;
        egui::Area::new(egui::Id::new("imagine-new"))
            .fixed_pos(egui::pos2(content.right() - 148.0, content.top() + 12.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                if crate::cards::white_pill(ui, "+ New project") {
                    new_project = true;
                }
            });
        egui::Area::new(composer_id)
            .default_size(egui::vec2(bar_w, 8.0))
            .fixed_pos(egui::pos2(x, y))
            .constrain_to(content)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.set_width(bar_w);
                ui.vertical(|ui| {
                    ui.set_width(bar_w);
                    if imagine_toolbox_shows_title(dock) {
                        ui.vertical_centered(|ui| {
                            ui.label(
                                RichText::new(format!("Imagine {word}"))
                                    .font(crate::theme::title_font(crate::theme::IMAGINE_TITLE))
                                    .color(crate::theme::fg()),
                            );
                        });
                        ui.add_space(crate::theme::IMAGINE_GAP);
                    }
                    self.ui_attach_chip(ui, PlusTarget::Imagine);
                    let bar = self.ui_imagine_bar(ui);
                    generate = bar.generate;
                    go_settings = bar.go_settings;
                    if bar.stop {
                        generate = false;
                    }
                    stop = bar.stop;
                });
            });
        if new_project {
            self.imagine_prompt.clear();
            self.imagine_last.clear();
            self.imagine_want_focus = true;
        }
        if let Some(p) = seed {
            self.imagine_prompt = p;
            self.imagine_want_focus = true;
        }
        if go_settings {
            if self.nav != Nav::Settings {
                self.settings_back = self.nav;
            }
            self.settings_sec = SettingsSec::Account;
            self.nav = Nav::Settings;
        }
        if stop {
            self.run_slash(Slash::Stop);
        } else if generate {
            self.kick_imagine();
        }
    }

    fn ui_imagine_bar(&mut self, ui: &mut egui::Ui) -> ImagineBarOut {
        let mut out = ImagineBarOut::default();
        let bar_w = ui.available_width().min(crate::theme::IMAGINE_BAR_W);
        let focused = ui.memory(|m| m.has_focus(egui::Id::new("imagine-prompt")));
        let stroke = if focused {
            crate::theme::border_strong()
        } else {
            crate::theme::border()
        };
        let model = dedicated_imagine_model(&self.cfg.imagine_model);
        let ready = !self.imagine_prompt.trim().is_empty();
        let authed = self.has_key();
        egui::Frame::none()
            .fill(crate::theme::surface())
            .rounding(crate::theme::IMAGINE_BAR_RADIUS)
            .stroke(egui::Stroke::new(1.0_f32, stroke))
            .inner_margin(egui::Margin::same(12.0))
            .show(ui, |ui| {
                ui.set_width(bar_w);
                let prompt_w = (ui.available_width() - 8.0).max(80.0);
                let prompt_h = crate::cards::imagine_prompt_h();
                let (prompt_rect, _) = ui.allocate_exact_size(
                    egui::vec2(prompt_w, prompt_h),
                    egui::Sense::hover(),
                );
                let edit = ui.put(
                    prompt_rect,
                    egui::TextEdit::singleline(&mut self.imagine_prompt)
                        .id(egui::Id::new("imagine-prompt"))
                        .desired_width(prompt_w)
                        .clip_text(true)
                        .frame(false)
                        .hint_text("Type to imagine"),
                );
                if self.imagine_want_focus {
                    edit.request_focus();
                    self.imagine_want_focus = false;
                }
                if edit.has_focus()
                    && ui.input(|i| {
                        i.key_pressed(egui::Key::Enter) && !i.modifiers.shift && !i.modifiers.command
                    })
                {
                    if self.imagine_prompt.ends_with('\n') {
                        self.imagine_prompt.pop();
                    }
                    if ready {
                        out.generate = true;
                    }
                }
                if edit.has_focus()
                    && ready
                    && ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.command)
                {
                    out.generate = true;
                }
                ui.add_space(crate::cards::imagine_prompt_chip_gap());
                let send_w = crate::cards::imagine_send_cluster_w();
                let chips_w = (ui.available_width() - send_w).max(crate::theme::IMAGINE_HIT * 4.0);
                let chip_h = crate::cards::imagine_chip_stack_h();
                ui.horizontal(|ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(chips_w, chip_h),
                        egui::Layout::left_to_right(egui::Align::Min).with_main_wrap(true),
                        |ui| {
                            ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                            let (plus_r, plus) = ui.allocate_exact_size(
                                egui::vec2(crate::theme::IMAGINE_HIT, crate::theme::IMAGINE_HIT),
                                egui::Sense::click(),
                            );
                            ui.painter()
                                .circle_filled(plus_r.center(), 18.0, crate::theme::panel());
                            crate::icons::paint_plus_at(ui.painter(), plus_r, crate::theme::muted());
                            if plus
                                .on_hover_text("Upload a file or paste clipboard")
                                .clicked()
                            {
                                self.open_plus(PlusTarget::Imagine, plus_r.left_bottom());
                            }
                            crate::cards::imagine_seg_track(ui, |ui| {
                                for kind in [
                                    ImagineKind::Image,
                                    ImagineKind::Video,
                                    ImagineKind::Agent,
                                ] {
                                    let on = self.imagine_kind == kind;
                                    let label = crate::cards::imagine_kind_label(kind);
                                    let ink = if on {
                                        crate::theme::fg()
                                    } else {
                                        crate::theme::muted()
                                    };
                                    if crate::cards::imagine_seg_chip(ui, on, |ui| {
                                        match kind {
                                            ImagineKind::Image => {
                                                crate::icons::paint_image_mode(ui, 16.0, ink);
                                            }
                                            ImagineKind::Video => {
                                                crate::icons::paint_video_mode(ui, 16.0, ink);
                                            }
                                            ImagineKind::Agent => {
                                                crate::icons::paint_agent_mode(ui, 16.0, ink);
                                            }
                                        }
                                        ui.add_space(4.0);
                                        ui.label(
                                            RichText::new(label)
                                                .size(crate::theme::FONT_CHROME)
                                                .color(ink),
                                        );
                                    }) {
                                        self.imagine_kind = kind;
                                        self.status = match kind {
                                            ImagineKind::Image => "Image still".into(),
                                            ImagineKind::Video => {
                                                "Video chips hint a storyboard still — cabin has no video file."
                                                    .into()
                                            }
                                            ImagineKind::Agent => {
                                                "Agent paints a character sprite still.".into()
                                            }
                                        };
                                    }
                                }
                            });
                            match self.imagine_kind {
                                ImagineKind::Video => {
                                    crate::cards::imagine_seg_track(ui, |ui| {
                                        for (i, label) in ["480p", "720p"].into_iter().enumerate() {
                                            let on = self.imagine_video_res == i as u8;
                                            if crate::cards::imagine_seg_chip(ui, on, |ui| {
                                                ui.label(
                                                    RichText::new(label)
                                                        .size(crate::theme::FONT_CHROME)
                                                        .color(if on {
                                                            crate::theme::fg()
                                                        } else {
                                                            crate::theme::muted()
                                                        }),
                                                );
                                            }) {
                                                self.imagine_video_res = i as u8;
                                            }
                                        }
                                    });
                                    crate::cards::imagine_seg_track(ui, |ui| {
                                        for (i, label) in ["6s", "10s", "15s"].into_iter().enumerate()
                                        {
                                            let on = self.imagine_video_dur == i as u8;
                                            if crate::cards::imagine_seg_chip(ui, on, |ui| {
                                                ui.label(
                                                    RichText::new(label)
                                                        .size(crate::theme::FONT_CHROME)
                                                        .color(if on {
                                                            crate::theme::fg()
                                                        } else {
                                                            crate::theme::muted()
                                                        }),
                                                );
                                            }) {
                                                self.imagine_video_dur = i as u8;
                                            }
                                        }
                                    });
                                    let audio_on = self.imagine_video_audio;
                                    if crate::cards::imagine_seg_chip(ui, audio_on, |ui| {
                                        ui.label(
                                            RichText::new("Video audio")
                                                .size(crate::theme::FONT_CHROME)
                                                .color(if audio_on {
                                                    crate::theme::fg()
                                                } else {
                                                    crate::theme::muted()
                                                }),
                                        );
                                    }) {
                                        self.imagine_video_audio = !self.imagine_video_audio;
                                    }
                                }
                                ImagineKind::Image | ImagineKind::Agent => {
                                    crate::cards::imagine_seg_track(ui, |ui| {
                                        for quality in [false, true] {
                                            let on = self.imagine_quality == quality;
                                            let label = crate::cards::imagine_quality_label(quality);
                                            if crate::cards::imagine_seg_chip(ui, on, |ui| {
                                                ui.label(
                                                    RichText::new(label)
                                                        .size(crate::theme::FONT_CHROME)
                                                        .color(if on {
                                                            crate::theme::fg()
                                                        } else {
                                                            crate::theme::muted()
                                                        }),
                                                );
                                            }) {
                                                self.imagine_quality = quality;
                                            }
                                        }
                                    });
                                }
                            }
                            let style_label = imagine_style_label(self.imagine_style);
                            let style_inner = egui::Frame::none()
                                .fill(crate::theme::panel())
                                .rounding(crate::theme::IMAGINE_HIT)
                                .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                                .show(ui, |ui| {
                                    ui.set_height(crate::theme::IMAGINE_HIT - 12.0);
                                    ui.set_min_width(56.0);
                                    ui.horizontal_centered(|ui| {
                                        crate::icons::paint_style_auto(ui, 16.0, crate::theme::fg());
                                        ui.add_space(4.0);
                                        ui.label(
                                            RichText::new(style_label)
                                                .size(crate::theme::FONT_CHROME)
                                                .color(crate::theme::fg()),
                                        );
                                        ui.add_space(4.0);
                                        crate::icons::paint_menu_caret(ui, crate::theme::muted());
                                    });
                                });
                            let style = ui
                                .interact(
                                    style_inner.response.rect,
                                    egui::Id::new("imagine-style-hit"),
                                    egui::Sense::click(),
                                )
                                .on_hover_text("Style — suffix on the still");
                            if style.clicked() {
                                self.imagine_style_open = !self.imagine_style_open;
                                self.imagine_aspect_open = false;
                                self.imagine_style_anchor = style.rect;
                                self.imagine_menu_ignore = true;
                            }
                            let aspect = imagine_aspect_label(self.imagine_aspect);
                            let aspect_name = imagine_aspect_name(self.imagine_aspect);
                            let aspect_inner = egui::Frame::none()
                                .fill(crate::theme::panel())
                                .rounding(crate::theme::IMAGINE_HIT)
                                .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                                .show(ui, |ui| {
                                    ui.set_height(crate::theme::IMAGINE_HIT - 12.0);
                                    ui.set_min_width(56.0);
                                    ui.horizontal_centered(|ui| {
                                        crate::icons::paint_aspect_rect(
                                            ui,
                                            self.imagine_aspect,
                                            16.0,
                                            crate::theme::fg(),
                                        );
                                        ui.add_space(4.0);
                                        ui.label(
                                            RichText::new(aspect)
                                                .size(crate::theme::FONT_CHROME)
                                                .color(crate::theme::fg()),
                                        );
                                        ui.add_space(4.0);
                                        crate::icons::paint_menu_caret(ui, crate::theme::muted());
                                    });
                                });
                            let aspect_hit = ui
                                .interact(
                                    aspect_inner.response.rect,
                                    egui::Id::new("imagine-aspect-hit"),
                                    egui::Sense::click(),
                                )
                                .on_hover_text(format!("{aspect} {aspect_name} · {model}"));
                            if aspect_hit.clicked() {
                                self.imagine_aspect_open = !self.imagine_aspect_open;
                                self.imagine_style_open = false;
                                self.imagine_aspect_anchor = aspect_hit.rect;
                                self.imagine_menu_ignore = true;
                            }
                            if !authed && crate::cards::ghost_pill(ui, "Connect Grok") {
                                out.go_settings = true;
                            } else if self.running && self.page_nav() == Nav::Imagine {
                                ui.label(
                                    RichText::new("Imagining…")
                                        .size(crate::theme::FONT_META)
                                        .color(crate::theme::muted()),
                                );
                            }
                        },
                    );
                    ui.allocate_ui_with_layout(
                        egui::vec2(send_w, chip_h),
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            let go = composer_go(self.running, ready);
                            let send = crate::icons::paint_bar_icon(
                                ui,
                                match go {
                                    ComposerGo::Stop => crate::icons::BarIcon::Stop,
                                    ComposerGo::Send => crate::icons::BarIcon::Send,
                                    ComposerGo::Idle => crate::icons::BarIcon::ArrowUp,
                                },
                                crate::theme::IMAGINE_HIT,
                                match go {
                                    ComposerGo::Idle => crate::theme::muted(),
                                    ComposerGo::Send | ComposerGo::Stop => crate::theme::fg(),
                                },
                            )
                            .on_hover_text(match go {
                                ComposerGo::Stop => composer_go_tip(true),
                                ComposerGo::Send | ComposerGo::Idle => "Generate still · Enter",
                            });
                            let go_hit = send.clicked()
                                || (send.is_pointer_button_down_on()
                                    && ui.input(|i| i.pointer.primary_pressed()));
                            match go {
                                ComposerGo::Stop => {
                                    if go_hit {
                                        out.stop = true;
                                    }
                                }
                                ComposerGo::Send => {
                                    if go_hit {
                                        out.generate = true;
                                    }
                                }
                                ComposerGo::Idle => {}
                            }
                            if crate::icons::paint_bar_icon(
                                ui,
                                crate::icons::BarIcon::Mic,
                                crate::theme::IMAGINE_HIT,
                                crate::theme::muted(),
                            )
                            .on_hover_text("Hey Grok")
                            .clicked()
                            {
                                self.listen_voice();
                            }
                        },
                    );
                });
            });
        out
    }

    fn ui_skills(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(crate::theme::bg()).inner_margin(egui::Margin::same(24.0)))
            .show(ctx, |ui| {
            if crate::cards::page_header(ui, "Skills and Connectors", "New Skill") {
                self.skills_tab_connectors = false;
                let existing: Vec<String> = self.skill_list.iter().map(|s| s.name.clone()).collect();
                let stub = crate::cards::starter_skill(&next_starter_skill_name(&existing));
                if skills::save_skill(&stub).is_ok() {
                    self.skill_list = skills::list_skills();
                    self.skill_name = stub.name.clone();
                    self.skill_body = grokhub_core::render_skill_md(&stub);
                    self.status = format!("Wrote skill {}", stub.name);
                }
            }
            if !self.verify_chip.is_empty() {
                ui.label(RichText::new(&self.verify_chip).strong());
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if crate::cards::tab_pill(ui, "Skills", !self.skills_tab_connectors) {
                    self.skills_tab_connectors = false;
                    self.nav = Nav::Skills;
                }
                if crate::cards::tab_pill(ui, "Connectors", self.skills_tab_connectors) {
                    self.skills_tab_connectors = true;
                    self.nav = Nav::Connectors;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    crate::cards::search_field(ui, &mut self.skill_q);
                });
            });
            ui.add_space(16.0);
            egui::ScrollArea::vertical().show(ui, |ui| {
            if self.skills_tab_connectors {
                crate::cards::section_label(ui, "Live");
                ui.label(
                    RichText::new("GitHub is the only live connector. No Outlook, Gmail, or Drive — those are not wired.")
                        .color(crate::theme::muted()),
                );
                ui.add_space(12.0);
                let has_pat = !self.secrets.github_token.trim().is_empty();
                let mut gh_tool: Option<&'static str> = None;
                for c in crate::cards::LIVE_CONNECTORS {
                    if !crate::cards::is_cabin_catalog(c.id) {
                        continue;
                    }
                    let body = if has_pat {
                        "PAT present (never shown). These buttons hit api.github.com."
                    } else {
                        "No PAT — set one in Settings, then these buttons work."
                    };
                    crate::cards::grok_tile(ui, c.icon, c.title, body, None, false);
                    ui.add_space(10.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut self.github_args)
                            .hint_text("repo:owner/name  or  query:…  (issues / search)")
                            .desired_width(f32::INFINITY),
                    );
                    ui.add_space(8.0);
                    ui.horizontal_wrapped(|ui| {
                        for (label, tool) in c.tools {
                            if ui
                                .add(
                                    egui::Button::new(RichText::new(*label).color(crate::theme::fg()))
                                        .fill(crate::theme::elevated())
                                        .rounding(14.0),
                                )
                                .clicked()
                            {
                                gh_tool = Some(*tool);
                            }
                        }
                        if ui
                            .add(
                                egui::Button::new(RichText::new("Settings").color(crate::theme::bg()))
                                    .fill(crate::theme::fg())
                                    .rounding(14.0),
                            )
                            .clicked()
                        {
                            self.nav = Nav::Settings;
                        }
                    });
                }
                if let Some(tool) = gh_tool {
                    let args = self.github_args.clone();
                    self.nav = Nav::Chat;
                    self.run_connector("github", tool, &args);
                }
                ui.add_space(20.0);
                crate::cards::section_label(ui, "Suggested");
                let conn_tiles = crate::cards::merge_suggested_connectors(&self.suggestions.connectors);
                let mut conn_tool: Option<String> = None;
                crate::cards::tile_row(ui, conn_tiles.len(), |ui, i| {
                    let (icon, title, body, tool) = &conn_tiles[i];
                    if matches!(
                        crate::cards::grok_tile(ui, *icon, title, body, Some("Add"), false),
                        crate::cards::TileHit::Add | crate::cards::TileHit::Body
                    ) {
                        conn_tool = Some(tool.clone());
                    }
                });
                if let Some(tool) = conn_tool {
                    if has_pat {
                        let args = self.github_args.clone();
                        self.nav = Nav::Chat;
                        self.run_connector("github", &tool, &args);
                    } else {
                        self.nav = Nav::Settings;
                    }
                }
            } else {
            crate::cards::section_label(ui, "Suggested");
            ui.label(
                RichText::new(review_status_line(
                    self.suggestions.last_review_day.as_deref(),
                    &Self::local_day(),
                ))
                .size(12.0)
                .color(crate::theme::muted()),
            );
            ui.add_space(8.0);
            let saved_names: Vec<String> = self.skill_list.iter().map(|s| s.name.clone()).collect();
            let pending = crate::cards::merge_suggested_skills(&self.suggestions.skills, &saved_names);
            crate::cards::tile_row(ui, pending.len(), |ui, i| {
                let s = &pending[i];
                let icon = crate::icons::icon_for_label(&s.title);
                if matches!(
                    crate::cards::grok_tile(ui, icon, &s.title, &s.body, Some("Add"), false),
                    crate::cards::TileHit::Add | crate::cards::TileHit::Body
                ) {
                    let sk = crate::cards::skill_from_learned(s);
                    if skills::save_skill(&sk).is_ok() {
                        self.skill_list = skills::list_skills();
                        self.skill_name = sk.name.clone();
                        self.skill_body = grokhub_core::render_skill_md(&sk);
                        self.status = format!("Wrote skill {}", sk.name);
                    }
                }
            });
            ui.add_space(20.0);
            crate::cards::section_label(ui, "Personal");
            let q = self.skill_q.clone();
            let list: Vec<_> = self
                .skill_list
                .iter()
                .filter(|s| crate::cards::skill_matches(&s.name, &s.description, &q))
                .cloned()
                .collect();
            if list.is_empty() {
                ui.label(
                    RichText::new("No skills yet — New Skill writes a SKILL.md you can edit.")
                        .color(crate::theme::muted()),
                );
            } else {
                let names: Vec<(String, String, bool)> = list
                    .iter()
                    .map(|s| {
                        let body = if s.description.trim().is_empty() {
                            s.instructions.chars().take(90).collect()
                        } else {
                            s.description.clone()
                        };
                        (s.name.clone(), body, self.skill_name == s.name)
                    })
                    .collect();
                let mut pick: Option<String> = None;
                crate::cards::tile_row(ui, names.len(), |ui, i| {
                    let (name, body, selected) = &names[i];
                    if crate::cards::catalog_card(ui, name, body, *selected) {
                        pick = Some(name.clone());
                    }
                });
                if let Some(name) = pick {
                    if let Some(s) = self.skill_list.iter().find(|s| s.name == name).cloned() {
                        self.skill_name = s.name.clone();
                        self.skill_body = grokhub_core::render_skill_md(&s);
                    }
                }
            }
            if !self.skill_body.is_empty() {
                ui.add_space(12.0);
                egui::Frame::none()
                    .fill(crate::theme::elevated())
                    .rounding(12.0)
                    .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                    .inner_margin(egui::Margin::same(10.0))
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut self.skill_body)
                                .desired_rows(12)
                                .desired_width(f32::INFINITY)
                                .frame(false)
                                .font(egui::TextStyle::Monospace),
                        );
                    });
                ui.horizontal(|ui| {
                    if crate::cards::white_pill(ui, "Save SKILL.md") {
                        let parsed = grokhub_core::parse_skill_md(&self.skill_body);
                        match skills::save_skill(&parsed) {
                            Ok(p) => {
                                self.skill_list = skills::list_skills();
                                self.status = format!("Wrote {}", p.display());
                            }
                            Err(e) => self.status = e,
                        }
                    }
                    if crate::cards::ghost_pill(ui, "Use in chat") && !self.skill_name.is_empty() {
                        let name = self.skill_name.clone();
                        if let Some(s) = self.skill_list.iter().find(|s| s.name == name) {
                            if let Some(r) = parse_recipe(&s.instructions) {
                                self.last_recipe = Some(r);
                            }
                        }
                        self.nav = Nav::Chat;
                        self.send_chat(format!("Follow skill {name}"));
                    }
                    if crate::cards::ghost_pill(ui, "Run verify") && !self.skill_name.is_empty() {
                        self.run_skill_verify();
                        if self.verify_ok_turn {
                            self.status = format!(
                                "verify pass · {} runs",
                                self.skill_list
                                    .iter()
                                    .find(|s| s.name == self.skill_name)
                                    .map(|s| s.runs)
                                    .unwrap_or(0)
                            );
                        }
                    }
                });
            }
            }
            });
        });
    }

    fn ui_eyes(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(crate::theme::bg()).inner_margin(egui::Margin::same(24.0)))
            .show(ctx, |ui| {
            let _ = crate::cards::page_header(ui, "Eyes", "");
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Look at the screen, then drive it. Halt stops a running job.")
                        .color(crate::theme::muted()),
                );
                let chip = crate::desktop::hands_chip_text();
                crate::cards::status_chip(
                    ui,
                    &chip,
                    if crate::desktop::hands_ready() {
                        crate::cards::ChipTone::Live
                    } else {
                        crate::cards::ChipTone::Offline
                    },
                );
            });
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if crate::cards::white_pill(ui, "Take over") {
                    self.take_over_desktop();
                }
                if crate::cards::ghost_pill(ui, "Install hands") {
                    self.status = crate::desktop::install_hands_status();
                }
                if crate::cards::ghost_pill(ui, "Scan") {
                    self.refresh_eyes();
                }
                if crate::cards::ghost_pill(ui, "Replay") {
                    self.replay_recipe();
                }
            });
            ui.add_space(12.0);
            if let Some(url) = self.last_frame_url.clone() {
                if let Some((tex, size)) = eyes_frame_tex(ui.ctx(), &url) {
                    let max_w = ui.available_width().min(480.0);
                    crate::cards::framed_preview(ui, &tex, size, max_w);
                }
            } else {
                let _ = crate::cards::empty_prompt_tile(
                    ui,
                    crate::icons::TileIcon::Image,
                    "No frame yet",
                    "Scan to capture this desktop.",
                );
            }
            ui.add_space(10.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                for line in self.eyes_text.lines() {
                    let Some(rest) = line.strip_prefix("- [") else {
                        continue;
                    };
                    let Some((kind, rest)) = rest.split_once(']') else {
                        continue;
                    };
                    let label = rest.trim().split(" @").next().unwrap_or(rest).trim();
                    crate::cards::object_chip(ui, kind, label);
                }
            });
        });
    }
}


fn eyes_frame_tex(ctx: &egui::Context, url: &str) -> Option<(TextureHandle, [usize; 2])> {
    let key: String = url.chars().take(48).collect();
    let id = egui::Id::new(("eyes-frame", url.len(), key));
    if let Some(hit) = ctx.data(|d| d.get_temp::<(TextureHandle, [usize; 2])>(id)) {
        return Some(hit);
    }
    let frame = PresenceFrame {
        data_url: url.to_string(),
        at: 0,
    };
    let (_, buf) = frame_bytes(&frame)?;
    let img = image::load_from_memory(&buf).ok()?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let tex = ctx.load_texture(
        "eyes-last-frame",
        ColorImage::from_rgba_unmultiplied(size, img.as_raw()),
        TextureOptions::LINEAR,
    );
    let hit = (tex, size);
    ctx.data_mut(|d| d.insert_temp(id, hit.clone()));
    Some(hit)
}

fn project_row_active(selected: bool, is_project: bool, nav: Nav) -> bool {
    if !selected || !is_project {
        return false;
    }
    match nav {
        Nav::Workboard => true,
        Nav::Chat
        | Nav::Devices
        | Nav::Memory
        | Nav::Imagine
        | Nav::Skills
        | Nav::Eyes
        | Nav::Night
        | Nav::History
        | Nav::Command
        | Nav::Connectors
        | Nav::Agents
        | Nav::Settings => false,
    }
}

fn health_settings_sec() -> SettingsSec {
    SettingsSec::About
}

fn select_all_edit(ui: &egui::Ui, id: egui::Id, text: &str) {
    let mut state = egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
    let end = text.chars().count();
    state.cursor.set_char_range(Some(egui::text::CCursorRange::two(
        egui::text::CCursor::new(0),
        egui::text::CCursor::new(end),
    )));
    state.store(ui.ctx(), id);
}

#[cfg(test)]
mod tests {
    use super::select_all_edit;
    use eframe::egui;

    #[test]
    fn rename_focus_selects_the_placeholder() {
        egui::__run_test_ui(|ui| {
            let mut buf = String::from("Project");
            let edit = ui.add(egui::TextEdit::singleline(&mut buf));
            select_all_edit(ui, edit.id, &buf);
            let state = egui::TextEdit::load_state(ui.ctx(), edit.id).expect("edit state");
            let range = state.cursor.char_range().expect("selection");
            let [a, b] = range.sorted();
            assert_eq!(a.index, 0);
            assert_eq!(b.index, 7);
        });
    }

    #[test]
    fn short_user_bubble_hugs_the_text() {
        with_fonts_ui(|ui| {
            ui.allocate_ui(egui::vec2(800.0, 200.0), |ui| {
                ui.set_max_width(800.0);
                let resp = super::paint_speech_bubble(ui, "Hi", true, false);
                assert!(
                    resp.rect.width() < 200.0,
                    "short bubble stretched to {}",
                    resp.rect.width()
                );
                assert!(resp.rect.width() > 24.0);
                assert!(resp.rect.height() > 20.0);
            });
        });
    }

    #[test]
    fn long_assistant_bubble_wraps_instead_of_one_line() {
        with_fonts_ui(|ui| {
            ui.allocate_ui(egui::vec2(800.0, 400.0), |ui| {
                ui.set_max_width(800.0);
                let body = "word ".repeat(80);
                let resp = super::paint_speech_bubble(ui, &body, false, true);
                assert!(
                    resp.rect.width() <= grokhub_core::bubble_max_width(800.0) + 8.0,
                    "bubble {}",
                    resp.rect.width()
                );
                assert!(
                    resp.rect.width() <= grokhub_core::BUBBLE_MAX_PX + 8.0,
                    "tight column {}",
                    resp.rect.width()
                );
                assert!(
                    resp.rect.height() > 48.0,
                    "wrapped bubble height {}",
                    resp.rect.height()
                );
            });
        });
    }

    #[test]
    fn long_sentence_stays_in_a_tight_column_on_a_wide_row() {
        with_fonts_ui(|ui| {
            ui.allocate_ui(egui::vec2(1600.0, 500.0), |ui| {
                ui.set_max_width(1600.0);
                let body = "the clam gods? oh you know... ancient, briny, and extremely picky about their cream-to-broth ratio. they live in the black void between chowder pots, only emerging when someone dares to say manhattan style in their presence. knock twice and offer a saltine or they won't even open up.";
                let resp = super::paint_speech_bubble(ui, body, false, true);
                assert!(
                    resp.rect.width() <= grokhub_core::BUBBLE_MAX_PX + 8.0,
                    "wide pane stretched the bubble to {}",
                    resp.rect.width()
                );
                assert!(
                    resp.rect.height() > 56.0,
                    "long sentence must wrap into several lines, height {}",
                    resp.rect.height()
                );
            });
        });
    }

    fn with_fonts_ui(mut add: impl FnMut(&mut egui::Ui)) {
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| add(ui));
        });
    }

    #[test]
    fn click_other_project_stays_on_this_pane() {
        assert!(!super::click_project_opens_board(false));
    }

    #[test]
    fn click_bound_project_opens_the_board() {
        assert!(super::click_project_opens_board(true));
    }

    #[test]
    fn selected_project_highlights_only_on_the_board() {
        assert!(super::project_row_active(true, true, super::Nav::Workboard));
        assert!(
            !super::project_row_active(true, true, super::Nav::Chat),
            "a History chat must not leave the project lit"
        );
        assert!(
            !super::project_row_active(true, true, super::Nav::Imagine),
            "Imagine must not leave the project lit"
        );
        assert!(!super::project_row_active(true, true, super::Nav::Night));
        assert!(!super::project_row_active(true, true, super::Nav::Skills));
        assert!(!super::project_row_active(true, true, super::Nav::History));
        assert!(!super::project_row_active(true, false, super::Nav::Workboard));
        assert!(!super::project_row_active(false, true, super::Nav::Workboard));
    }

    #[test]
    fn health_opens_the_about_page() {
        assert_eq!(super::health_settings_sec(), super::SettingsSec::About);
    }

    fn about_section_opens_update() {
        assert_eq!(
            super::settings_group_home(super::SettingsGroup::About),
            super::SettingsSec::Update
        );
    }

    #[test]
    fn overlay_update_skips_chat() {
        let v = grokhub_core::overlay_update_begin(2);
        assert!(v.stay_on_update);
        assert!(!v.posts_chat);
        let done = grokhub_core::overlay_update_finish(true, 50);
        assert!(!done.posts_chat);
        assert!(done.stay_on_update);
        assert!(done.can_restart);
        assert!(grokhub_core::overlay_update_can_restart(true, false));
        assert!(!grokhub_core::overlay_update_can_restart(true, true));
    }

    #[test]
    fn general_section_opens_account() {
        assert_eq!(
            super::settings_group_home(super::SettingsGroup::General),
            super::SettingsSec::Account
        );
    }

    #[test]
    fn slash_arrows_move_and_clamp() {
        assert_eq!(super::slash_pick_step(0, 5, 1), 1);
        assert_eq!(super::slash_pick_step(0, 5, -1), 0);
        assert_eq!(super::slash_pick_step(4, 5, 1), 4);
        assert_eq!(super::slash_pick_step(9, 3, 0), 2);
    }

    #[test]
    fn tab_accept_runs_on_pick() {
        let mut composer = "/fi".into();
        let run = super::slash_pick_take(&mut composer, "/fix", true);
        assert_eq!(run.as_deref(), Some("/fix"));
        assert!(composer.is_empty());
    }

    #[test]
    fn tab_accept_stays_for_args() {
        let mut composer = "/proj".into();
        let run = super::slash_pick_take(&mut composer, "/project bind ", false);
        assert!(run.is_none());
        assert_eq!(composer, "/project bind ");
    }

    #[test]
    fn slash_pick_resets_when_the_list_changes() {
        assert_eq!(super::slash_pick_retain(2, true, 4), 0);
        assert_eq!(super::slash_pick_retain(2, false, 4), 2);
        assert_eq!(super::slash_pick_retain(9, false, 3), 2);
        assert_eq!(super::slash_pick_retain(1, true, 0), 0);
    }

    #[test]
    fn idle_visible_cabin_does_not_spin() {
        assert!(!super::wants_live_repaint(false, false, false, true, false, false));
        assert!(!super::wants_live_repaint(false, false, false, false, false, false));
        assert!(super::wants_live_repaint(true, false, false, true, false, false));
        assert!(super::wants_live_repaint(false, false, false, false, false, true));
        assert!(super::HIDDEN_HEARTBEAT_MS > 80);
        assert!(!super::night_host_check_blocks_ui());
        assert_eq!(
            grokhub_core::heartbeat_repaint_ms(false, false, grokhub_core::HEARTBEAT_MS, super::HIDDEN_HEARTBEAT_MS),
            grokhub_core::HEARTBEAT_MS
        );
        assert_eq!(
            grokhub_core::heartbeat_repaint_ms(false, true, grokhub_core::HEARTBEAT_MS, super::HIDDEN_HEARTBEAT_MS),
            grokhub_core::HEARTBEAT_MS
        );
        assert_eq!(
            grokhub_core::heartbeat_repaint_ms(true, true, grokhub_core::HEARTBEAT_MS, super::HIDDEN_HEARTBEAT_MS),
            80
        );
    }

    #[test]
    fn show_cabin_keeps_the_tray_icon() {
        let src = include_str!("app.rs");
        let show = src
            .split("fn show_from_tray")
            .nth(1)
            .and_then(|s| s.split("fn poll_voice").next())
            .expect("show_from_tray");
        assert!(
            !show.contains("drop_off_thread"),
            "Show cabin must not tear down the tray icon: {show}"
        );
        assert!(
            show.contains("ensure_tray_spawn"),
            "Show cabin should keep a live tray: {show}"
        );
        assert!(
            src.contains("force_x11_for_close_to_tray")
                || include_str!("main.rs").contains("force_x11_for_close_to_tray"),
            "winit 0.30 must drop WAYLAND_DISPLAY so × can unmap"
        );
    }

    #[test]
    fn mode_status_does_not_treat_ladder_default_as_auto_pin() {
        assert_eq!(
            super::mode_status_line("auto", "grok-3-mini-fast"),
            "Mode auto — routes Fast / Balance / Think / Max"
        );
        assert_eq!(
            super::mode_status_line("auto", "grok-4.6"),
            "Mode auto — routes Fast / Balance / Think / Max"
        );
        assert_eq!(super::mode_status_line("auto", "grok-3"), "Mode auto → grok-3");
        assert_eq!(
            super::mode_status_line("think", "grok-3"),
            "Mode think → grok-4.6 · high"
        );
        assert_eq!(
            super::mode_status_line("max", ""),
            "Mode max → grok-4.6 · xhigh"
        );
    }

    #[test]
    fn empty_home_paints_faint_greeting() {
        let src = include_str!("app.rs");
        let home = src.find("fn ui_empty_home").expect("empty home");
        let slice = &src[home..home + 1600];
        assert!(
            slice.contains("self.greeting"),
            "new chats paint a greeting blurb: {slice}"
        );
        let mark = slice.find("GrokHub").expect("wordmark");
        let greet = slice[mark..]
            .find("self.greeting")
            .map(|i| mark + i)
            .expect("greeting under wordmark");
        let composer = slice.find("ui_composer_stack").expect("composer");
        assert!(
            mark < greet && greet < composer,
            "greeting sits under the wordmark, above the chat box"
        );
        assert!(
            slice.contains("whisper"),
            "greeting uses the faint paint color"
        );
    }

    #[test]
    fn rail_footer_is_reserved() {
        assert_eq!(super::RAIL_FOOTER_H, 52.0);
        assert!(super::PALETTE_LIST_H < 400.0);
    }

    #[test]
    fn rail_chat_title_stays_short() {
        assert_eq!(
            grokhub_core::display_tab_title("chowder and food interest and cho"),
            "chowder"
        );
        with_fonts_ui(|ui| {
            let painted = super::fit_rail_label(ui, "chowder and food interest and cho", 72.0);
            assert!(
                painted.chars().count() < 20,
                "rail label must not run off the pill: {painted}"
            );
            assert!(painted.ends_with('…') || painted == "chowder", "{painted}");
        });
    }

    #[test]
    fn appearance_tab_offers_light() {
        let ids: Vec<&str> = grokhub_core::appearance_choices()
            .iter()
            .copied()
            .map(grokhub_core::theme_id)
            .collect();
        assert_eq!(ids, vec!["dark", "light", "system"]);
        assert_eq!(
            grokhub_core::parse_theme("light"),
            grokhub_core::ThemeChoice::Light
        );
        assert!(!grokhub_core::resolve_dark(
            grokhub_core::ThemeChoice::Light,
            true
        ));
        assert!(grokhub_core::resolve_dark(
            grokhub_core::ThemeChoice::Dark,
            false
        ));
        assert!(!grokhub_core::resolve_dark(
            grokhub_core::ThemeChoice::System,
            false
        ));
    }

    #[test]
    fn eyes_page_is_product_copy() {
        let src = include_str!("app.rs");
        let start = src.find("fn ui_eyes").expect("eyes");
        let slice = &src[start..start + 2800];
        assert!(
            !slice.contains("Presence ring"),
            "intern presence notes stay off the page: {slice}"
        );
        assert!(
            !slice.contains("ydotoold") && !slice.contains("xdotool on X11"),
            "Eyes subtitle is not a man page: {slice}"
        );
        assert!(slice.contains("Take over"));
        assert!(slice.contains("Install hands"));
        assert!(slice.contains("hands_chip_text"));
        assert!(slice.contains("framed_preview") || slice.contains("object_chip"));
        assert!(slice.contains("Look at the screen"));
    }

    #[test]
    fn thought_uses_live_theme_tokens() {
        let src = include_str!("app.rs");
        let start = src.find("ChatKind::Thought =>").expect("thought");
        let slice = &src[start..start + 1600];
        assert!(slice.contains("theme::muted()"), "{slice}");
        assert!(slice.contains("theme::subtle()"), "{slice}");
        assert!(!slice.contains("theme::MUTED"));
        assert!(!slice.contains("theme::SUBTLE"));
    }

    #[test]
    fn composer_stack_drops_approve_slots() {
        let src = include_str!("app.rs");
        let impl_end = src.find("#[cfg(test)]").unwrap_or(src.len());
        let impl_src = &src[..impl_end];
        assert!(!impl_src.contains("SkillApprove"));
        assert!(!impl_src.contains("SaveAsSkill"));
        assert!(!impl_src.contains("HostPlan"));
        let order = super::composer_stack_order();
        assert_eq!(
            order,
            &[
                super::ComposerStackSlot::AuthBanner,
                super::ComposerStackSlot::SlashPalette,
                super::ComposerStackSlot::Chips,
                super::ComposerStackSlot::Attach,
                super::ComposerStackSlot::Pill,
            ]
        );
    }

    #[test]
    fn chips_sit_above_the_composer_pill() {
        let order = super::composer_stack_order();
        let chips = order
            .iter()
            .position(|s| *s == super::ComposerStackSlot::Chips);
        let pill = order
            .iter()
            .position(|s| *s == super::ComposerStackSlot::Pill)
            .expect("pill");
        assert!(chips.is_some(), "chips belong above the composer pill");
        assert!(chips.unwrap() < pill);
    }

    #[test]
    fn other_chip_threads_skip_current_and_scratch() {
        let mut current = crate::threads::ChatThread::new("Now", false);
        current.id = "cur".into();
        current.messages.push(("user".into(), "this chat".into()));
        let mut prev = crate::threads::ChatThread::new("Night cabin", false);
        prev.id = "prev".into();
        prev.messages.push(("user".into(), "paint the wall".into()));
        prev.messages
            .push(("assistant".into(), "I can sketch the first coat.".into()));
        let mut scratch = crate::threads::ChatThread::new("Scratch", true);
        scratch.id = "scr".into();
        scratch.messages.push(("user".into(), "ignore me".into()));
        let others = super::collect_other_chip_threads(&[current, prev, scratch], "cur");
        assert_eq!(others.len(), 1);
        assert_eq!(others[0].title, "Night cabin");
        assert_eq!(others[0].last_user, "paint the wall");
    }

    #[test]
    fn chat_composer_pins_stop_on_the_right() {
        let src = include_str!("app.rs");
        let start = src.find("ComposerStackSlot::Pill =>").expect("pill arm");
        let pill = &src[start..start + 8000];
        assert!(
            pill.contains("composer_go_cluster_w()"),
            "Fast + mic + Stop need a reserved strip: {pill}"
        );
        assert!(
            pill.contains("composer_mid_w(") && pill.contains("composer_go_hit_w("),
            "Plus/mid/Stop widths come from the window pill, not inflated available: {pill}"
        );
        let stop = pill.find("ComposerGo::Stop").expect("stop glyph");
        let edit = pill.find("TextEdit::multiline").expect("composer field");
        assert!(
            edit < stop,
            "Send/Stop is the last sibling after an exact-width mid strip"
        );
        assert!(
            pill.contains("is_pointer_button_down_on"),
            "Stop must halt on press; click-release is eaten by the shrink feel: {pill}"
        );
        assert!(
            pill.contains("primary_pressed"),
            "go press is edge-triggered so holding Send does not immediately Stop: {pill}"
        );
        assert!(
            !pill.contains("- 180.0"),
            "180px left Fast as the pill's right edge on a 900-wide cabin"
        );
        let home = src.find("fn ui_empty_home").expect("empty home");
        let home = &src[home..home + 1400];
        let cap = home.find("composer_pill_w").expect("pane cap");
        let mark = home.find("GrokHub").expect("wordmark");
        assert!(
            cap < mark,
            "cap pane width before the wordmark shrink-wraps the column"
        );
        assert!(
            home.contains("vertical_centered_justified"),
            "justify the empty-home column so the pill fills the pane: {home}"
        );
        let stack = src.find("for slot in composer_stack_order()").expect("stack");
        let cap = &src[stack.saturating_sub(280)..stack];
        assert!(
            cap.contains("composer_pill_w("),
            "chip row must not stretch the centered column past the pane: {cap}"
        );
    }

    #[test]
    fn nightly_review_stays_quiet() {
        let src = include_str!("app.rs");
        let tick = src
            .split("fn tick_review(")
            .nth(1)
            .and_then(|s| s.split("fn review_digest(").next())
            .expect("tick_review");
        assert!(
            !tick.contains("send_chat") && !tick.contains("Nav::Chat") && !tick.contains("self.running"),
            "tick_review must not open Chat or take the composer: {tick}"
        );
        let spawn = src
            .split("fn spawn_review(")
            .nth(1)
            .and_then(|s| s.split("fn poll_review(").next())
            .expect("spawn_review");
        assert!(
            !spawn.contains("send_chat") && !spawn.contains("Nav::Chat"),
            "spawn_review must not dump the review into chat: {spawn}"
        );
        assert!(
            !spawn.contains("self.running"),
            "spawn_review leaves the user chat free: {spawn}"
        );
        assert!(
            spawn.contains("model_for_mode(\"balanced\")"),
            "nightly review forces Balance: {spawn}"
        );
        let apply = src
            .split("fn apply_review_reply(")
            .nth(1)
            .and_then(|s| s.split("fn poll_wall(").next())
            .expect("apply_review_reply");
        assert!(
            !apply.contains("send_chat") && !apply.contains("Nav::Chat"),
            "applying suggestions stays off the chat: {apply}"
        );
        assert!(src.contains("self.tick_review()"));
        assert!(
            src.contains("if !night_fired && !self.running"),
            "Review waits if Night just fired or chat is running"
        );
        let night = src
            .split("fn ui_night(")
            .nth(1)
            .and_then(|s| s.split("fn ui_history(").next())
            .expect("ui_night");
        assert!(
            night.contains("merge_suggested_autos"),
            "Automations Suggested uses learned tiles first: {night}"
        );
        assert!(
            night.contains("review_status_line"),
            "Suggested header shows Reviewed today / due tonight: {night}"
        );
        let skills = src
            .split("fn ui_skills(")
            .nth(1)
            .and_then(|s| s.split("fn ui_eyes(").next())
            .expect("ui_skills");
        assert!(
            skills.contains("merge_suggested_skills") && skills.contains("merge_suggested_connectors"),
            "Skills and Connectors Suggested use learned tiles: {skills}"
        );
        assert!(
            skills.contains("review_status_line"),
            "Skills Suggested header shows review status: {skills}"
        );
    }

    #[test]
    fn chat_arm_checks_stream_end_followup() {
        let src = include_str!("app.rs");
        let chat = src
            .split("Ok(JobOut::Chat { text, truncated })")
            .nth(1)
            .and_then(|s| s.split("Ok(JobOut::Consult").next())
            .expect("Chat arm");
        assert!(
            chat.contains("reply_needs_followup"),
            "stream-end follow-up belongs in the Chat arm: {chat}"
        );
        assert!(
            chat.contains("send_followup_turn"),
            "Chat arm kicks a quiet continue, not send_chat: {chat}"
        );
        assert!(
            chat.contains("FOLLOWUP_MAX_STEPS") && chat.contains("followup_step"),
            "auto-follow is capped per user turn: {chat}"
        );
        assert!(
            chat.contains("!self.running"),
            "skip follow-up when host/goal already continues: {chat}"
        );
        let mid = src
            .split("fn tick_mid_thought(")
            .nth(1)
            .and_then(|s| s.split("fn last_night_hint(").next())
            .expect("tick_mid_thought");
        assert!(
            !mid.contains("send_chat") && !mid.contains("send_followup_turn"),
            "MidThought must not auto-continue chat: {mid}"
        );
    }

    #[test]
    fn mid_thought_stays_out_of_chat() {
        let src = include_str!("app.rs");
        let impl_src = src.split("#[cfg(test)]").next().unwrap_or(src);
        assert!(
            !impl_src.contains("You sit down. Last night"),
            "MidThought must not inject a fake assistant turn"
        );
        let mid = src
            .split("fn tick_mid_thought(")
            .nth(1)
            .and_then(|s| s.split("fn last_night_hint(").next())
            .expect("tick_mid_thought");
        assert!(
            !mid.contains("send_chat") && !mid.contains("Nav::Chat") && !mid.contains("self.running"),
            "MidThought stays quiet: {mid}"
        );
        assert!(
            mid.contains("continue_thread_hint"),
            "MidThought folds Continue {{title}} into the greeting path: {mid}"
        );
        let hint = src
            .split("fn last_night_hint(")
            .nth(1)
            .and_then(|s| s.split("fn mark_auto_ran(").next())
            .expect("last_night_hint");
        assert!(
            !hint.contains("messages.push") && !hint.contains("send_chat"),
            "last-night context stays in the greeting: {hint}"
        );
        assert!(hint.contains("continue_hint"), "empty last-night falls back to continue hint: {hint}");
        assert!(src.contains("last_night: &last_night") || src.contains("last_night: &self.last_night_hint()"));
        assert!(src.contains("self.tick_mid_thought()"));
    }

    #[test]
    fn chat_rail_opens_most_recent_thread() {
        let src = include_str!("app.rs");
        let theme = include_str!("theme.rs");
        let chat = theme.find("(\"chat\", \"Chat\")").expect("chat rail");
        let imagine = theme.find("(\"imagine\", \"Imagine\")").expect("imagine rail");
        assert!(chat < imagine, "Chat sits above Imagine on the rail");
        let set_nav = src
            .split("fn set_nav_id(")
            .nth(1)
            .and_then(|s| s.split("fn conn_kind(").next())
            .expect("set_nav_id");
        assert!(
            set_nav.contains("\"chat\" =>") && set_nav.contains("self.open_recent_chat()"),
            "Chat rail click opens the last accessed thread: {set_nav}"
        );
        let open = src
            .split("fn open_recent_chat(")
            .nth(1)
            .and_then(|s| s.split("fn new_thread(").next())
            .expect("open_recent_chat");
        assert!(
            open.contains("most_recently_accessed_index") && open.contains("switch_thread"),
            "Chat rail uses last-access, not leftover thread_idx: {open}"
        );
        let house = src
            .split("HeartbeatAct::Housekeep =>")
            .nth(1)
            .and_then(|s| s.split("HeartbeatAct::Inbox =>").next())
            .expect("housekeep");
        assert!(
            house.contains("stamp_current_access") && house.contains("Nav::Chat"),
            "Housekeep stamps access while sitting on Chat: {house}"
        );
        let mut older = crate::threads::ChatThread::new("Older", false);
        older.accessed_ms = 1_000;
        let mut newer = crate::threads::ChatThread::new("Night cabin", false);
        newer.accessed_ms = 8_000;
        let mut scratch = crate::threads::ChatThread::new("Scratch", true);
        scratch.accessed_ms = 9_000;
        assert_eq!(
            crate::threads::most_recently_accessed_index(&[older, newer, scratch]),
            Some(1)
        );
    }
}
