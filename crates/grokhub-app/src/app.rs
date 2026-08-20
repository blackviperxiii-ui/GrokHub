use crate::build_agent;
use crate::helpers::{
    cabin_menu_should_dismiss, click_project_opens_board, collect_other_chip_threads, expand_home,
    next_maximized, next_starter_skill_name, wants_live_repaint,
};
use crate::titlebar::{
    apply_tray_window, titlebar_chrome_btn, titlebar_chrome_hit, titlebar_should_start_drag,
};
use crate::config::{self, AppConfig};
use crate::desktop::{
    capture_data_url, capture_webcam, clipboard_image, collect_rows, first_bin, load_image_data_url,
    lock_titles, pick_file, play_audio, prepare_windshield, read_text_capped, record_once,
    run_computer_op_cancel, run_limited, transcribe_local,
};
use crate::host::{host_working_dir, resolve_host_cite_path, run_host, run_host_stream};
use crate::secrets::{self, Secrets};
use crate::skills;
use crate::threads::{self, ChatThread};
use crate::update::{remember_source, resolve_source};
use crate::xai::{
    grok_chat, grok_chat_stream, grok_imagine_opts, grok_imagine_video, grok_stt,
    grok_tts, http_status_of,
};
use eframe::egui::{self, Color32, ColorImage, RichText, TextureHandle, TextureOptions};
use grokhub_acp::{merge_tool_card, AcpEvent, PermissionMode, SessionMode, ToolCard};
use grokhub_core::{
    append_composer, anticipate_consumes_slot, anticipated_need, apply_work_update, attach_kind, attach_name, attach_prompt_line,
    cabin_system_prompt,
    appearance_choices, appearance_hint, approved_cmds, auth_bearer, automation_blocked_by_policy,
    blend_thread_goal, flush_visible_goal,
    build_hub_snapshot, merge_hub_snapshots,
    build_quick_chips, build_windshield, bump_skill_run, bump_usage,
    inhabit_claim_allowed, inhabit_ready, hub_pair_url, devices_shows_pair_code, pair_code_is_live, parse_hostname_i, pick_lan_ipv4,
    start_hub_rotates_pair,
    catalog_line, chip_suggest_prompt, compact_keep_start_from, compose_imagine_prompt,
    context_fingerprint,
    context_percent,
    dedicated_imagine_model, dedicated_video_model, dedicated_voice_model, default_openclaw_paths, diagnostics_bundle,
    pick_fresh_seed, wall_can_paint, wall_evict, ImagineKind, ImagineSpec, ImagineWall,
    WallGif, WALL_GIF_EVERY_MS, WALL_GIF_MAX,
    imagine_shows_result_above, imagine_toolbox_dock, imagine_toolbox_shows_title,
    imagine_toolbox_top, imagine_wall_bounds,
    doctor_hands_line, due_automations, ensure_automation_schedule, estimate_messages,
    estimate_messages_from,
    extract_connector_cmds, mark_automation_skipped, retain_held_plan, yolo_plan_split, chat_bearer,
    oauth_access_live,
    drop_trailing_assistant, job_error_goes_to_chat, job_is_scratch,
    persist_user_turn, refund_host_reserved, daily_units_blocked,
    night_check_command, night_check_exit_code, skip_night_check_receipt,
    extract_imagine_prompt, extract_work_pins, filter_palette, format_consult_reply,
    imagine_aspect_label, imagine_aspect_name, imagine_image_resolution, imagine_style_label,
    imagine_video_dur_label, imagine_video_duration_secs, imagine_video_res_label,
    imagine_video_resolution, last_imagine_receipt,
    extract_insights, extract_work_updates, fact_candidates, fact_candidates_from, failover_model, filter_slash_commands,
    frame_bytes, PresenceFrame,
    forget_topic, greet_from_last_job, has_auth, has_verify_ok, hey_grok_on_press,
    thread_host_receipts, thread_host_receipts_from,
    hey_grok_route, hey_grok_starts_ptt, import_memory_file, merge_imported_memory, insight_pin, is_openclaw_workspace,
    add_to_folder, create_folder, create_project, drop_node, drop_selected, folder_choices,
    host_cmd_leaves_project, host_hour_blocked, host_risk, host_status_line, is_hard_run,
    verify_ok_after_user_turn, VerifyResult,
    project_menu_acts, project_menu_label, rename_node, resolve_acp_cwd, resolve_bind_path, restore_bound_path, seed_from_bound,
    settle_project_path, should_seed_sidebar, stage_project, toggle_folder, upsert_bound,
    visible_tree, ProjectKind, ProjectMenuAct,
    ProjectNode,
    is_plain_text, is_voice_error, keep_last_rewinds, last_user_scan, load_hub_state, mark_automation_ran,
    night_check_may_fire, night_counts_run, night_unauth_should_skip,
    match_skill, mode_from_chip_value, model_for_mode, nav_from_chip_value,
    cabin_eyes_request_text, cabin_frame_only, chat_attach_status, imagine_ref_status,
    kick_consumes_attach, next_chat_image, next_goal_prompt, paint_connect_banner,
    this_turn_cabin_frame,
    is_workload_user, merge_thinking_capped, prefer_complete_reply, quote_for_reply, strip_thinking,
    refresh_last_stretch, visible_chat_refs, visible_turn_count, visible_turn_count_from, ChatKind, ChatView,
    apply_job_error, chat_send_kind, chat_shows_thinking, chat_stream_is_visible,
    upsert_assistant_turn,
    worker_gone_status, ChatSendKind,
    bubble_outer_width, bubble_wrap_width, clamp_row_width, BUBBLE_PAD_X,
    BUBBLE_PAD_Y,
    BUBBLE_RADIUS,
    plus_empty_status, plus_menu_rows, computer_cmd_line, hands_protocol, lock_blocks_hands,
    parse_computer_op, see_drive_attach, user_asks_cabin_eyes,
    resolve_chat_model, resolve_dark, effective_chat_mode, settings_pin_blocks_auto, parse_fast_topics,
    goal_continue_pin, goal_pin_for_job, goal_step_after_outcome, hub_dispatch_ok, should_auto_continue_goal,
    visible_goal_step_on_continue,
    now_ms, parse_consult, parse_goal_outcome, parse_local_clock, patch_skill, prefer_patch,
    reply_needs_followup,
    recipe_from_cmds, replay_automation_target,
    parse_nl_automation, parse_recipe, parse_slash, parse_theme, pick_theme, plan_from_text, plan_room,
    chat_may_save_automation, user_asked_to_schedule,
    presence_should_stream, propose_skill_from_turn, quiet_hours_active,
    parse_llm_chips, record_turn, reduce_voice_state, remember_chip_click, remember_chip_dismiss,
    remember_chip_outcome, remember_typed_prompt, roll_usage_day,
    greeting_fingerprint, greeting_name, greeting_prompt, local_greeting, pick_greeting,
    should_paint_greeting, should_refresh_greeting, GreetingInput, GREETING_LLM_MODE,
    recall_hits, redirect_prompt, redact_secrets, refused_lock, replay_ops, rewind_allowed,
    is_rewind_copy_cmd, is_rewind_copy_cmd_in, rewind_blocked_reason, rewind_copy_cmd, rewind_snapshot_ready,
    rewind_dest, rewind_restore_matches, save_hub_state, screen_from_extents, search_corpus,
    search_thread_body,
    state_for_disk,
    clear_pending_after_complete, inbox_claim_ready,
    should_anticipate, should_auto_compact_now, should_keep_frame, should_refresh_llm,
    should_trim_result_bodies, shortcut_help,
    windshield_prompt,
    composer_enter, composer_go, composer_go_tip, ComposerEnter, ComposerGo,
    heartbeat_acts, heartbeat_due, heartbeat_repaint_ms, next_heartbeat_wait_ms, HeartbeatAct,
    HEARTBEAT_MS,
    chip_scan,
    build_review_digest, dedupe_suggestions, merge_suggestion_store, parse_suggest_lines,
    parse_suggest_skill_patches, parse_trajectory_jsonl, summarize_trajectory, trajectory_jsonl_line,
    trim_result_bodies_in_place, yesterday_ms, RESULT_TRIM_KEEP_HOPS,
    partition_suggestions, prune_live_suggestions, review_due,
    review_status_line, review_system_prompt, digest_line_from, DigestLine, ReviewDigest, SuggestionStore,
    CABIN_GITHUB_TOOLS, REVIEW_NIGHT_HOUR,
    should_capture_before_chat, should_failover_status, should_idle_reflect, should_send_screenshot,
    apply_auto_title_in, apply_manual_rename, delete_thread, display_tab_title, history_order,
    history_row_visible, leftover_empty_thread, mark_slash_result, reuse_empty_thread_idx,
    unknown_cabin_slash, ThreadReuseView,
    should_name_thread,
    skill_follow_block, skill_use_in_chat_prompt, slash_help, summarize_write, surgical_memory_edit, MemoryEdit,
    thread_goal_prompt, theme_id, theme_label, toggle_pin, DeleteOutcome, ThreadTab,
    top_habit_labels,
    unified_diff_cite, usage_line,
    transcribe_route, uid, update_cmds, overlay_update_begin, overlay_update_finish,
    realtime_bearer, realtime_can_connect, voice_log_role, voice_stream_token, voice_transcript_sends_chat,
    fold_stream_fields, StreamTokenKind,
    update_wipes_config, voice_session_url, Automation, BoardCard,
    BoardStatus, ChipInput, ChipKind, ChipMemory, ChipThread, ComputerOp, DeviceCodeStart, HeyGrokAction,
    HeyGrokRoute, HubMemoryFile, QuickChip,
    HubSnapshot, HubState, InhabitBundle, LearningState, LocalClock, MintRealtimeFn, Policy, Recipe, ReplayOp, RewindRecord,
    HostPlanStep, HostRisk, forbidden_reason, mint_host_halt,
    AttachKind, PlusAct, PlusTarget, SkillMd, Slash, ThemeChoice, TranscribeRoute, UsageDay, VoiceEvent,
    VoiceState, CONTEXT_BUDGET_TOKENS, CABIN_FAST_FALLBACK, CABIN_FAST_MODEL, CHIP_LLM_MODE, CHIP_VISIBLE_MAX, FRAME_CAP, IMAGE_FILE_CAP,
    TEXT_FILE_CAP, bound_scan,
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
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
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

enum PlusPick {
    NativeMiss,
    ClipEmpty,
    ClipText(String),
    Ready(PlusReady),
    Err(String),
}

struct PlusReady {
    kind: AttachKind,
    name: String,
    raw: String,
    image_url: Option<String>,
    text: Option<String>,
}

fn plus_from_path(target: PlusTarget, path: PathBuf) -> PlusPick {
    let raw = path.display().to_string();
    let kind = attach_kind(&raw);
    let name = attach_name(&raw);
    match kind {
        AttachKind::Image if target == PlusTarget::Chat => match load_image_data_url(&path) {
            Ok(url) => PlusPick::Ready(PlusReady {
                kind,
                name,
                raw,
                image_url: Some(url),
                text: None,
            }),
            Err(e) => PlusPick::Err(e),
        },
        AttachKind::Text => match read_text_capped(&path) {
            Ok(t) => PlusPick::Ready(PlusReady {
                kind,
                name,
                raw,
                image_url: None,
                text: Some(t),
            }),
            Err(e) => PlusPick::Err(e),
        },
        _ => PlusPick::Ready(PlusReady {
            kind,
            name,
            raw,
            image_url: None,
            text: None,
        }),
    }
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
        ComposerStackSlot::Attach,
        ComposerStackSlot::Pill,
        ComposerStackSlot::Chips,
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

fn cabin_fast_llm(key: String, prompt: String) -> String {
    let key = if key.trim().is_empty() {
        grokhub_acp::grok_cli_key().unwrap_or_default()
    } else {
        key
    };
    if !key.trim().is_empty() {
        let primary = grok_chat(
            &key,
            CABIN_FAST_MODEL,
            &[("user".into(), prompt.clone())],
            None,
            None,
        )
        .unwrap_or_default();
        if !primary.trim().is_empty() {
            return primary;
        }
        return grok_chat(
            &key,
            CABIN_FAST_FALLBACK,
            &[("user".into(), prompt)],
            None,
            None,
        )
        .unwrap_or_default();
    }
    let Some(bin) = grokhub_acp::find_grok() else {
        return String::new();
    };
    let home = std::env::var("HOME").ok();
    let work = match home.as_deref() {
        Some(h) => format!("{h}/GrokHub-Work"),
        None => "GrokHub-Work".into(),
    };
    let picked = resolve_acp_cwd("", home.as_deref(), &work);
    let path = std::path::PathBuf::from(picked);
    let cwd = grokhub_acp::ensure_session_cwd(&path).unwrap_or(path);
    let text = grokhub_acp::grok_stdout(
        &bin,
        &cwd,
        &[
            "--no-auto-update",
            "--model",
            CABIN_FAST_MODEL,
            "-p",
            &prompt,
        ],
    )
    .unwrap_or_default();
    if !text.trim().is_empty() {
        return text;
    }
    grokhub_acp::grok_stdout(
        &bin,
        &cwd,
        &[
            "--no-auto-update",
            "--model",
            CABIN_FAST_FALLBACK,
            "-p",
            &prompt,
        ],
    )
    .unwrap_or_default()
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
    image: Option<ColorImage>,
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
    OpenGrok(String),
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
    thread_id: String,
}

struct ImportOpenclawOut {
    status: String,
    mem_name: String,
    mem_body: String,
    skill_list: Vec<SkillMd>,
    open_memory: bool,
}

struct ReplayDeskOut {
    text: String,
    cmds: Vec<String>,
    frame: Option<Result<String, String>>,
}

fn listen_turn(api_key: &str) -> String {
    let wav = match record_once() {
        Ok(p) => p,
        Err(e) => return format!("VOICE_RECEIPT: {e}"),
    };
    let has_local = first_bin(TRANSCRIBERS).is_some();
    match transcribe_route(!api_key.trim().is_empty(), has_local) {
        TranscribeRoute::Xai => {
            let len = std::fs::metadata(&wav).map(|m| m.len()).unwrap_or(u64::MAX);
            if len > IMAGE_FILE_CAP {
                "VOICE_RECEIPT: recording too large".into()
            } else {
                match std::fs::read(&wav) {
                    Ok(bytes) => match grok_stt(api_key, &bytes) {
                        Ok(t) => t,
                        Err(e) => transcribe_local(&wav).unwrap_or_else(|local| {
                            format!("VOICE_RECEIPT: {e}; {local}")
                        }),
                    },
                    Err(e) => format!("VOICE_RECEIPT: {e}"),
                }
            }
        }
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

#[derive(Debug, Clone, PartialEq, Eq)]
enum ChatBlockAct {
    None,
    Copy(String),
    Reply(String),
}

fn take_ui_text(mut s: String, cap: u64) -> String {
    if (s.len() as u64) <= cap {
        return s;
    }
    s.truncate(cap as usize);
    while !s.is_empty() && !s.is_char_boundary(s.len()) {
        s.pop();
    }
    s
}

fn push_stream_capped(buf: &mut String, d: &str, cap: u64) -> bool {
    let before = buf.len();
    if (buf.len() as u64) >= cap {
        return false;
    }
    let room = (cap as usize).saturating_sub(buf.len());
    if d.len() <= room {
        buf.push_str(d);
        return buf.len() != before;
    }
    let mut end = room;
    while end > 0 && !d.is_char_boundary(end) {
        end -= 1;
    }
    buf.push_str(&d[..end]);
    buf.len() != before
}

fn paint_speech_bubble(ui: &mut egui::Ui, body: &str, user: bool, markdown: bool) -> egui::Response {
    let body = crate::markdown::display_text(body);
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
    ui.scope(|ui| {
        ui.set_max_width(avail);
        ui.horizontal(|ui| {
            ui.set_max_width(avail);
            if user {
                ui.add_space((avail - outer_w).max(0.0));
            }
            ui.with_layout(egui::Layout::top_down(egui::Align::LEFT), |ui| {
                ui.set_max_width(outer_w);
                resp = Some(
                    frame
                        .show(ui, |ui| {
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
        });
    });
    resp.expect("speech bubble")
}

fn paint_msg_acts(ui: &mut egui::Ui, user: bool, body: &str, avail: f32, align_w: f32) -> ChatBlockAct {
    let mut act = ChatBlockAct::None;
    let mut paint = |ui: &mut egui::Ui| {
        let copy = ui.add(
            egui::Button::new(
                RichText::new("Copy")
                    .size(crate::theme::FONT_META)
                    .color(crate::theme::muted()),
            )
            .frame(false),
        );
        if copy.clicked() {
            act = ChatBlockAct::Copy(body.to_string());
        }
        let reply = ui.add(
            egui::Button::new(
                RichText::new("Reply")
                    .size(crate::theme::FONT_META)
                    .color(crate::theme::muted()),
            )
            .frame(false),
        );
        if reply.clicked() {
            act = ChatBlockAct::Reply(body.to_string());
        }
    };
    ui.scope(|ui| {
        ui.set_max_width(avail);
        ui.horizontal(|ui| {
            ui.set_max_width(avail);
            if user {
                ui.add_space((avail - align_w.max(96.0)).max(0.0));
            }
            paint(ui);
        });
    });
    act
}

fn paint_chat_block(ui: &mut egui::Ui, block: &ChatView, _idx: usize, thought_open: bool) -> ChatBlockAct {
    let avail = clamp_row_width(ui.available_width().min(ui.max_rect().width()));
    let bubble_w = crate::markdown::bubble_width(avail);
    match block.kind {
        ChatKind::User => {
            let resp = paint_speech_bubble(ui, &block.body, true, false);
            if resp.hovered() {
                paint_msg_acts(ui, true, &block.body, avail, resp.rect.width())
            } else {
                ChatBlockAct::None
            }
        }
        ChatKind::Assistant => {
            let resp = paint_speech_bubble(ui, &block.body, false, true);
            if resp.hovered() {
                paint_msg_acts(ui, false, &block.body, avail, resp.rect.width())
            } else {
                ChatBlockAct::None
            }
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
                    if open {
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
            ChatBlockAct::None
        }
        ChatKind::Tool => {
            egui::Frame::none()
                .fill(crate::theme::elevated())
                .rounding(8.0)
                .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                .inner_margin(egui::Margin::symmetric(10.0, 4.0))
                .show(ui, |ui| {
                    ui.set_max_width(bubble_w);
                    let title = if block.title.is_empty() {
                        "Tool"
                    } else {
                        block.title.as_str()
                    };
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(title)
                                .size(crate::theme::FONT_META)
                                .color(crate::theme::muted()),
                        );
                        ui.label(
                            RichText::new(&block.body)
                                .size(crate::theme::FONT_META)
                                .color(crate::theme::subtle()),
                        );
                    });
                });
            ChatBlockAct::None
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

struct LiveCap {
    url: Option<String>,
}

enum CabinFrame {
    Skip,
    Pending,
    Ready(String),
}

struct PersistSnap {
    threads: Vec<ChatThread>,
    msgs: Vec<(String, String)>,
    board: Vec<BoardCard>,
    automations: Vec<Automation>,
    rewind_rows: Vec<RewindRecord>,
    learning: LearningState,
    suggestions: SuggestionStore,
    usage: UsageDay,
    chip_memory: ChipMemory,
    wall: ImagineWall,
    cfg: AppConfig,
    hub: Option<Arc<Mutex<HubState>>>,
    projects: Option<Vec<ProjectNode>>,
    secrets: Option<crate::secrets::Secrets>,
}

fn write_persist_disk(snap: &PersistSnap) {
    let _ = threads::save(&snap.threads);
    let msgs = snap
        .threads
        .iter()
        .find(|t| t.id == snap.cfg.current_thread)
        .or_else(|| snap.threads.first())
        .map(|t| t.messages.as_slice())
        .unwrap_or(snap.msgs.as_slice());
    let _ = config::save_chat(msgs);
    let _ = config::save_board(&snap.board);
    let _ = crate::night::save(&snap.automations);
    let _ = crate::night::save_rewinds(&snap.rewind_rows);
    let _ = crate::store::save_learning(&snap.learning);
    let _ = crate::store::save_suggestions(&snap.suggestions);
    let _ = crate::store::save_usage(&snap.usage);
    let _ = crate::store::save_chips(&snap.chip_memory);
    let _ = crate::store::save_wall(&snap.wall);
    if let Some(p) = &snap.projects {
        let _ = crate::store::save_projects(p);
    }
    let _ = config::save(&snap.cfg);
    if let Some(s) = &snap.secrets {
        let _ = secrets::save(s);
    }
    if let Some(hub) = &snap.hub {
        let disk = hub.lock().ok().map(|st| state_for_disk(&st));
        if let Some(disk) = disk {
            let _ = save_hub_state(&config::hub_state_path(), &disk);
        }
    }
}

pub struct Cabin {
    nav: Nav,
    cfg: AppConfig,
    composer: String,
    messages: Arc<Vec<(String, String)>>,
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
    mem_cache_at: [u64; 3],
    mem_cache_body: [String; 3],
    last_persist: Instant,
    persist_idle_key: String,
    persist_rx: Option<mpsc::Receiver<()>>,
    persist_io: Arc<Mutex<()>>,
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
    oauth_next_poll: Instant,
    oauth_start_rx: Option<mpsc::Receiver<Result<DeviceCodeStart, String>>>,
    oauth_poll_rx: Option<mpsc::Receiver<Result<grokhub_core::PollResult, String>>>,
    host_hour_count: u32,
    host_hour_at: Instant,
    host_reserved: u32,
    approve_risky_only: bool,
    plan_pending: Option<Vec<HostPlanStep>>,
    tray: Option<crate::tray::TrayHost>,
    tray_rx: Option<mpsc::Receiver<Option<crate::tray::TrayHost>>>,
    window_visible: bool,
    tray_saw_unfocused: bool,
    tray_hid_at: Instant,
    want_quit: bool,
    told_tray: bool,
    pending_hub_task: Option<String>,
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
    chat_views: Vec<ChatView>,
    chat_view_tid: String,
    chat_view_n: usize,
    chat_view_last: usize,
    presence_ring: Vec<(u64, String)>,
    voice_sock: Option<crate::voice_ws::VoiceSock>,
    voice_state: VoiceState,
    cmd_line: String,
    cmd_hist: Vec<String>,
    agents: Vec<AgentJob>,
    last_live: Instant,
    live_cap_rx: Option<mpsc::Receiver<LiveCap>>,
    eyes_cap_rx: Option<mpsc::Receiver<Result<String, String>>>,
    kick_cap_rx: Option<mpsc::Receiver<Result<String, String>>>,
    pending_kick: Option<bool>,
    kick_frame: Option<String>,
    kick_skip: bool,
    recipe_cap_rx: Option<mpsc::Receiver<Result<String, String>>>,
    recipe_desk_rx: Option<mpsc::Receiver<ReplayDeskOut>>,
    host_diff_rx: Option<mpsc::Receiver<Option<String>>>,
    host_diff_kick: bool,
    verify_rx: Option<mpsc::Receiver<Option<VerifyResult>>>,
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
    chip_paint_key: String,
    chip_llm_at: u64,
    greeting: String,
    greeting_fp: String,
    greeting_user_at: u64,
    greeting_memory_at: u64,
    greeting_user_md: String,
    greeting_memory_md: String,
    greeting_files_rx: Option<mpsc::Receiver<(u64, String, u64, String)>>,
    greeting_flush_name: String,
    greeting_flush_len: usize,
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
    composer_want_focus: bool,
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
    pick_rx: Option<mpsc::Receiver<(PlusTarget, PlusPick)>>,
    pick_list_rx: Option<mpsc::Receiver<(String, Vec<(String, bool)>)>>,
    pick_dir: String,
    pick_cache: Option<(String, Vec<(String, bool)>)>,
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
    acp: Option<grokhub_acp::AcpHandle>,
    acp_spawn_rx: Option<mpsc::Receiver<Result<grokhub_acp::AcpHandle, String>>>,
    tool_cards: Vec<ToolCard>,
    desk_frame: Option<String>,
    perm_ask: Option<grokhub_acp::PermissionAsk>,
    session_mode: SessionMode,
    permission_mode: PermissionMode,
    grok_sessions: Vec<grokhub_acp::GrokSession>,
    grok_sessions_loaded: bool,
    grok_sessions_rx: Option<mpsc::Receiver<Vec<grokhub_acp::GrokSession>>>,
    inspect_rx: Option<mpsc::Receiver<String>>,
    history_rx: Option<mpsc::Receiver<Vec<String>>>,
    mem_restore_rx: Option<mpsc::Receiver<(String, Result<String, String>)>>,
    mem_file_rx: Option<(String, mpsc::Receiver<(u64, String)>)>,
    recall_rx: Option<mpsc::Receiver<String>>,
    sync_rx: Option<mpsc::Receiver<(String, Vec<HubMemoryFile>)>>,
    inhabit_rx: Option<mpsc::Receiver<InhabitBundle>>,
    reflect_rx: Option<mpsc::Receiver<(MemoryEdit, Option<MemoryEdit>)>>,
    session_show_rx: Option<(String, mpsc::Receiver<String>)>,
    import_rx: Option<mpsc::Receiver<ImportOpenclawOut>>,
    inspect_text: String,
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
    let aspect = if tall { Some("2:3") } else { Some("16:9") };
    let src_a = grok_imagine_opts(key, model, prompt, aspect, Some("1k"))?;
    let path_a = dir.join(format!("{id}_a.png"));
    std::fs::copy(&src_a, &path_a).map_err(|e| e.to_string())?;
    let path_b = dir.join(format!("{id}_b.png"));
    match grok_imagine_opts(key, model, prompt_b, aspect, Some("1k")) {
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
        let mut cfg = config::load();
        if cfg.device_name.trim().is_empty() {
            cfg.device_name = config::default_device_name();
            let _ = config::save(&cfg);
        }
        config::ensure_memory_seeds();
        let mut hub = load_hub_state(&config::hub_state_path()).unwrap_or_else(HubState::empty);
        if !cfg.device_name.trim().is_empty() {
            hub.device_name = cfg.device_name.clone();
        }
        let peer = hub.device_id.clone();
        if hub.requeue_claimed_for(&peer) > 0 {
            let _ = save_hub_state(&config::hub_state_path(), &hub);
        }
        let mem_name = "SOUL.md".to_string();
        let mem_body = config::read_memory(&mem_name);
        let mem_cache_at = [config::memory_updated_at("SOUL.md"), 0, 0];
        let mem_cache_body = [mem_body.clone(), String::new(), String::new()];
        let mut threads = threads::load();
        if threads.is_empty() {
            let mut t = ChatThread::new("Chat", false);
            t.messages = Arc::new(config::load_chat());
            threads.push(t);
        }
        let keep_id = threads
            .iter()
            .find(|t| t.id == cfg.current_thread)
            .map(|t| t.id.clone())
            .or_else(|| threads.first().map(|t| t.id.clone()));
        let before = threads.len();
        threads.retain(|t| {
            keep_id.as_deref() == Some(t.id.as_str())
                || t.pinned
                || !leftover_empty_thread(&t.title, t.scratch, t.messages.is_empty())
        });
        if threads.is_empty() {
            threads.push(ChatThread::new("Chat", false));
        }
        let dropped_leftover = threads.len() != before;
        let thread_idx = threads
            .iter()
            .position(|t| keep_id.as_deref() == Some(t.id.as_str()))
            .or_else(|| threads.iter().position(|t| t.id == cfg.current_thread))
            .unwrap_or(0);
        let messages = threads
            .get(thread_idx)
            .map(|t| t.messages.clone())
            .unwrap_or_else(|| Arc::new(Vec::new()));
        let imagine_last =
            last_imagine_receipt(messages.iter().map(|(_, c)| c.as_str())).unwrap_or_default();
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
        cfg.project_dir = expand_home(&restore_bound_path(&cfg.project_dir, &work, sidebar_file));
        if should_seed_sidebar(sidebar_file, &projects) {
            projects = seed_from_bound(&cfg.project_dir);
        }
        let project_sel = projects
            .iter()
            .find(|n| n.kind == ProjectKind::Project && expand_home(&n.path) == cfg.project_dir)
            .or_else(|| projects.iter().find(|n| n.kind == ProjectKind::Project))
            .map(|n| n.id.clone());
        let mut secrets = secrets::load();
        secrets::migrate_console_key(&mut cfg, &mut secrets);
        let win_max = cfg.window.maximized;
        let approve_risky_only = cfg.approve_risky_only;
        let goal_step = threads.get(thread_idx).map(|t| t.goal.step).unwrap_or(0);
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
            mem_cache_at,
            mem_cache_body,
            last_persist: Instant::now(),
            persist_idle_key: String::new(),
            persist_rx: None,
            persist_io: Arc::new(Mutex::new(())),
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
            oauth_next_poll: Instant::now(),
            oauth_start_rx: None,
            oauth_poll_rx: None,
            host_hour_count: 0,
            host_hour_at: Instant::now(),
            host_reserved: 0,
            approve_risky_only,
            plan_pending: None,
            tray: None,
            tray_rx: if crate::tray::tray_needed_at_launch(hidden) {
                Some(crate::tray::begin_tray_spawn())
            } else {
                None
            },
            window_visible: !hidden,
            tray_saw_unfocused: false,
            tray_hid_at: Instant::now(),
            want_quit: false,
            told_tray: false,
            pending_hub_task: None,
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
            goal_step,
            followup_step: 0,
            stream_buf: String::new(),
            thought_buf: String::new(),
            chat_views: vec![],
            chat_view_tid: String::new(),
            chat_view_n: usize::MAX,
            chat_view_last: usize::MAX,
            presence_ring: vec![],
            voice_sock: None,
            voice_state: VoiceState::Idle,
            cmd_line: String::new(),
            cmd_hist: vec![],
            agents: vec![],
            last_live: Instant::now(),
            live_cap_rx: None,
            eyes_cap_rx: None,
            kick_cap_rx: None,
            pending_kick: None,
            kick_frame: None,
            kick_skip: false,
            recipe_cap_rx: None,
            recipe_desk_rx: None,
            host_diff_rx: None,
            host_diff_kick: false,
            verify_rx: None,
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
            chip_paint_key: String::new(),
            chip_llm_at: 0,
            greeting: String::new(),
            greeting_fp: String::new(),
            greeting_user_at: 0,
            greeting_memory_at: 0,
            greeting_user_md: String::new(),
            greeting_memory_md: String::new(),
            greeting_files_rx: None,
            greeting_flush_name: String::new(),
            greeting_flush_len: usize::MAX,
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
            composer_want_focus: false,
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
            pick_rx: None,
            pick_list_rx: None,
            pick_dir: std::env::var("HOME").unwrap_or_else(|_| "/tmp".into()),
            pick_cache: None,
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
            acp: None,
            acp_spawn_rx: None,
            tool_cards: Vec::new(),
            desk_frame: None,
            perm_ask: None,
            session_mode: SessionMode::Chat,
            permission_mode: PermissionMode::Ask,
            grok_sessions: Vec::new(),
            grok_sessions_loaded: false,
            grok_sessions_rx: None,
            inspect_rx: None,
            history_rx: None,
            mem_restore_rx: None,
            mem_file_rx: None,
            recall_rx: None,
            sync_rx: None,
            inhabit_rx: None,
            reflect_rx: None,
            session_show_rx: None,
            import_rx: None,
            inspect_text: String::new(),
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
        if dropped_leftover {
            c.persist_bg();
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
                self.geom_dirty = false;
                let io = self.persist_io.clone();
                let mut cfg = self.cfg.clone();
                cfg.api_key.clear();
                std::thread::spawn(move || {
                    if let Ok(_g) = io.lock() {
                        let _ = config::save(&cfg);
                    }
                });
            }
            crate::window::GeomFlush::AfterMs(ms) => {
                ctx.request_repaint_after(Duration::from_millis(ms));
            }
        }
    }

    fn persist(&mut self) {
        let mut snap = self.persist_snap();
        if snap.projects.is_some() {
            self.projects_dirty = false;
        }
        self.sync_hub_voice();
        snap.secrets = Some(self.secrets.clone());
        self.persist_idle_key = self.persist_idle_now();
        self.last_persist = Instant::now();
        self.geom_dirty = false;
        let io = self.persist_io.clone();
        std::thread::spawn(move || {
            if let Ok(_g) = io.lock() {
                write_persist_disk(&snap);
            }
        });
    }

    fn persist_snap(&mut self) -> PersistSnap {
        if let Some(t) = self.threads.get_mut(self.thread_idx) {
            t.messages = self.messages.clone();
            self.cfg.current_thread = t.id.clone();
        }
        let projects = if self.projects_dirty {
            Some(self.projects.clone())
        } else {
            None
        };
        PersistSnap {
            threads: self.threads.clone(),
            msgs: Vec::new(),
            board: self.board.clone(),
            automations: self.automations.clone(),
            rewind_rows: self.rewind_rows.clone(),
            learning: self.learning.clone(),
            suggestions: self.suggestions.clone(),
            usage: self.usage.clone(),
            chip_memory: self.chip_memory.clone(),
            wall: self.wall.clone(),
            cfg: {
                let mut cfg = self.cfg.clone();
                cfg.api_key.clear();
                cfg
            },
            hub: Some(self.hub.clone()),
            projects,
            // Idle persist must not write secrets.json from a stale snap.
            secrets: None,
        }
    }

    fn persist_idle_now(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
            self.threads.len(),
            self.thread_idx,
            self.messages.len(),
            self.messages.last().map(|(_, c)| c.len()).unwrap_or(0),
            self.board.len(),
            self.automations.len(),
            self.usage.day,
            self.usage.messages,
            self.cfg.current_thread,
            self.threads
                .get(self.thread_idx)
                .and_then(|t| t.grok_session.as_deref())
                .unwrap_or(""),
            self.threads
                .get(self.thread_idx)
                .and_then(|t| t.grok_cwd.as_deref())
                .unwrap_or(""),
        )
    }

    fn persist_bg(&mut self) {
        if self.running {
            return;
        }
        if self.persist_rx.is_some() {
            return;
        }
        let key = self.persist_idle_now();
        if !self.projects_dirty && self.persist_idle_key == key {
            self.last_persist = Instant::now();
            return;
        }
        self.persist_idle_key = key;
        let snap = self.persist_snap();
        if snap.projects.is_some() {
            self.projects_dirty = false;
        }
        self.sync_hub_voice();
        self.last_persist = Instant::now();
        self.geom_dirty = false;
        let io = self.persist_io.clone();
        let (tx, rx) = mpsc::channel();
        self.persist_rx = Some(rx);
        std::thread::spawn(move || {
            if let Ok(_g) = io.lock() {
                write_persist_disk(&snap);
            }
            let _ = tx.send(());
        });
    }

    fn poll_persist(&mut self) {
        let Some(rx) = self.persist_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(()) | Err(mpsc::TryRecvError::Disconnected) => {}
            Err(mpsc::TryRecvError::Empty) => {
                self.persist_rx = Some(rx);
            }
        }
    }

    fn sync_hub_voice(&self) {
        if let Ok(mut st) = self.hub.lock() {
            st.console_api_key = self.console_key().to_string();
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
        if let Some(h) = &self.acp {
            if let Some(p) = self.perm_ask.take() {
                let _ = h.answer_permission(p.rpc_id, false);
            }
            let _ = h.cancel();
            while let Ok(ev) = h.try_recv() {
                if let AcpEvent::Permission(p) = ev {
                    let _ = h.answer_permission(p.rpc_id, false);
                }
            }
        }
        self.rx = None;
        self.running = false;
        if self.host_reserved > 0 {
            self.host_hour_count = refund_host_reserved(self.host_hour_count, self.host_reserved);
            self.host_reserved = 0;
        }
        self.pending_connectors.clear();
        self.pending_kick = None;
        self.kick_cap_rx = None;
        self.kick_frame = None;
        self.kick_skip = false;
        self.acp_spawn_rx = None;
        self.recipe_cap_rx = None;
        self.recipe_desk_rx = None;
        self.host_diff_rx = None;
        self.host_diff_kick = false;
        self.verify_rx = None;
        self.plan_pending = None;
        self.agents.clear();
        self.active_skill_follow = None;
        self.followup_step = 0;
        self.speak_next = false;
        self.stream_buf.clear();
        self.thought_buf.clear();
        self.perm_ask = None;
        let vis = self.visible_thread_id();
        let job = self.chat_job_thread.clone();
        if job.as_deref().is_none_or(|id| id == vis) {
            if self.messages.last().is_some_and(|m| m.0 == "assistant") {
                self.live_mut().pop();
            }
            self.stamp_current_access();
        } else if let Some(id) = job.as_deref() {
            if let Some(t) = self.threads.iter_mut().find(|t| t.id == id) {
                drop_trailing_assistant(t.messages_mut());
                t.accessed_ms = now_ms();
            }
        }
        self.chat_job_thread = None;
        self.persist();
        if let Some(mut s) = self.voice_sock.take() {
            s.halt();
        }
        self.voice_state = VoiceState::Idle;
        self.voice_orb = "idle".into();
    }

    fn apply_assistant_snapshot(&mut self, content: String) {
        let content = take_ui_text(content, IMAGE_FILE_CAP);
        if content.is_empty() {
            return;
        }
        let vis = self.visible_thread_id();
        let job = self.chat_job_thread.as_deref();
        if job.is_none() || job == Some(vis.as_str()) {
            let msgs = self.live_mut();
            if let Some(m) = msgs.last_mut() {
                if m.0 == "assistant" {
                    m.1 = content;
                } else {
                    msgs.push(("assistant".into(), content));
                }
            } else {
                msgs.push(("assistant".into(), content));
            }
        } else if let Some(job_id) = job {
            if let Some(t) = self.threads.iter_mut().find(|t| t.id == job_id) {
                upsert_assistant_turn(t.messages_mut(), &content);
            }
        }
        let target = self
            .chat_job_thread
            .clone()
            .unwrap_or_else(|| vis);
        if let Some(t) = self.threads.iter_mut().find(|t| t.id == target) {
            t.accessed_ms = now_ms();
        }
    }

    fn push_bound_msg(&mut self, role: &str, content: String) {
        let content = take_ui_text(content, IMAGE_FILE_CAP);
        let vis = self.visible_thread_id();
        let job = self.chat_job_thread.as_deref();
        if job.is_none() || job == Some(vis.as_str()) {
            self.live_mut().push((role.to_string(), content));
            if let Some(t) = self.threads.iter_mut().find(|t| t.id == vis) {
                t.accessed_ms = now_ms();
            }
            return;
        }
        if let Some(job_id) = job {
            if let Some(t) = self.threads.iter_mut().find(|t| t.id == job_id) {
                t.messages_mut().push((role.to_string(), content));
                t.accessed_ms = now_ms();
            }
        }
    }

    fn apply_live_assistant(&mut self) {
        self.apply_assistant_snapshot(merge_thinking_capped(
            &self.thought_buf,
            &self.stream_buf,
            TEXT_FILE_CAP,
        ));
    }

    fn has_key(&self) -> bool {
        has_auth(self.console_key(), &secrets::access_token(&self.secrets))
    }

    fn console_key(&self) -> &str {
        secrets::console_key(&self.secrets, &self.cfg.api_key)
    }

    fn can_agent(&self) -> bool {
        build_agent::can_agent(self.has_key())
    }

    fn llm_ready(&self) -> bool {
        self.has_key() || grokhub_acp::find_grok().is_some() || grokhub_acp::grok_cli_key().is_some()
    }

    fn grok_cwd(&self) -> std::path::PathBuf {
        let home = std::env::var("HOME").ok();
        let work = self.work_root();
        let picked = resolve_acp_cwd(&self.cfg.project_dir, home.as_deref(), &work);
        std::path::PathBuf::from(picked)
    }

    fn reload_grok_sessions(&mut self) {
        if self.grok_sessions_rx.is_some() {
            return;
        }
        let bin = grokhub_acp::find_grok();
        let cwd = self.grok_cwd();
        let (tx, rx) = mpsc::channel();
        self.grok_sessions_rx = Some(rx);
        std::thread::spawn(move || {
            let listed = if let Some(bin) = bin {
                grokhub_acp::list_sessions(&bin, &cwd).unwrap_or_default()
            } else {
                Vec::new()
            };
            let files = grokhub_acp::discover_session_files();
            let listed_ids: Vec<String> = listed
                .iter()
                .map(|r| grokhub_acp::split_session_row(r).id)
                .collect();
            let mut rows = grokhub_acp::merge_grok_sessions(&listed, files);
            for s in &mut rows {
                if listed_ids.iter().any(|id| id == &s.id) {
                    s.cwd = Some(cwd.clone());
                }
            }
            let _ = tx.send(rows);
        });
    }

    fn poll_grok_sessions(&mut self) {
        let Some(rx) = self.grok_sessions_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(rows) => {
                self.grok_sessions = rows;
                self.grok_sessions_loaded = true;
                if self.nav == Nav::History {
                    self.status = format!("{} Grok sessions", self.grok_sessions.len());
                }
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.grok_sessions_rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.grok_sessions_loaded = true;
            }
        }
    }

    fn poll_inspect(&mut self) {
        let Some(rx) = self.inspect_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(text) => {
                self.inspect_text = text;
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.inspect_rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {}
        }
    }

    fn poll_history_search(&mut self) {
        let Some(rx) = self.history_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(hits) => {
                self.history_hits = hits;
                self.status = format!("{} hits", self.history_hits.len());
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.history_rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {}
        }
    }

    fn poll_mem_restore(&mut self) {
        let Some(rx) = self.mem_restore_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok((name, Ok(body))) => {
                if let Some(i) = Self::mem_file_idx(&name) {
                    self.mem_cache_at[i] = config::memory_updated_at(&name);
                    self.mem_cache_body[i] = body.clone();
                }
                if self.mem_name == name {
                    self.mem_body = body;
                }
                self.status = format!("Restored {name}.prev");
            }
            Ok((_, Err(e))) => self.status = e,
            Err(mpsc::TryRecvError::Empty) => {
                self.mem_restore_rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {}
        }
    }

    fn mem_file_idx(name: &str) -> Option<usize> {
        match name {
            "SOUL.md" => Some(0),
            "USER.md" => Some(1),
            "MEMORY.md" => Some(2),
            _ => None,
        }
    }

    fn poll_mem_file(&mut self) {
        let Some((name, rx)) = self.mem_file_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok((at, body)) => {
                if let Some(i) = Self::mem_file_idx(&name) {
                    self.mem_cache_at[i] = at;
                    self.mem_cache_body[i] = body.clone();
                }
                if self.mem_name == name {
                    self.mem_body = body;
                }
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.mem_file_rx = Some((name, rx));
            }
            Err(mpsc::TryRecvError::Disconnected) => {}
        }
    }

    fn poll_recall(&mut self) {
        let Some(rx) = self.recall_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(body) => {
                self.live_mut().push(("assistant".into(), mark_slash_result(&body)));
                self.stamp_current_access();
                self.persist();
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.recall_rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {}
        }
    }

    fn poll_session_show(&mut self) {
        let Some((id, rx)) = self.session_show_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(text) => {
                if text.trim().is_empty() {
                    return;
                }
                let msgs = grokhub_acp::parse_session_markdown(&text);
                if msgs.is_empty() {
                    return;
                }
                let Some(i) = self
                    .threads
                    .iter()
                    .position(|t| t.grok_session.as_deref() == Some(id.as_str()))
                else {
                    return;
                };
                let filled = Arc::new(msgs);
                if let Some(t) = self.threads.get_mut(i) {
                    if t.messages.is_empty() {
                        t.messages = filled.clone();
                    }
                }
                if i == self.thread_idx && self.messages.is_empty() {
                    self.messages = filled;
                }
                self.persist_bg();
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.session_show_rx = Some((id, rx));
            }
            Err(mpsc::TryRecvError::Disconnected) => {}
        }
    }

    fn poll_acp_spawn(&mut self) {
        let Some(rx) = self.acp_spawn_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(h)) => {
                let sid = h.session_id.clone();
                let cwd = h.cwd.display().to_string();
                if !sid.trim().is_empty() {
                    let job = self.chat_job_thread.clone();
                    let idx = job
                        .as_deref()
                        .and_then(|id| self.threads.iter().position(|t| t.id == id))
                        .unwrap_or(self.thread_idx);
                    if let Some(t) = self.threads.get_mut(idx) {
                        t.grok_session = Some(sid);
                        t.grok_cwd = Some(cwd);
                    }
                }
                self.acp = Some(h);
                self.persist();
            }
            Ok(Err(e)) => {
                self.running = false;
                self.pending_kick = None;
                self.status = self.apply_job_fail(&e);
                self.chat_job_thread = None;
                self.persist();
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.acp_spawn_rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.running = false;
                self.pending_kick = None;
                self.status = self.apply_job_fail("Grok Build session missing");
                self.chat_job_thread = None;
                self.persist();
            }
        }
    }

    fn open_grok_session(&mut self, id: &str) {
        if let Some(i) = self
            .threads
            .iter()
            .position(|t| t.grok_session.as_deref() == Some(id))
        {
            self.switch_thread(i);
            self.nav = Nav::Chat;
            return;
        }
        let sess = self
            .grok_sessions
            .iter()
            .find(|s| s.id == id)
            .cloned();
        let title = sess
            .as_ref()
            .map(|s| s.title.clone())
            .filter(|t| !t.is_empty())
            .unwrap_or_else(|| id.chars().take(24).collect());
        let mut t = ChatThread::new(&title, false);
        t.grok_session = Some(id.to_string());
        t.grok_cwd = sess
            .as_ref()
            .and_then(|s| s.cwd.as_ref())
            .map(|p| p.display().to_string())
            .filter(|s| !s.is_empty());
        t.accessed_ms = now_ms();
        if t.messages.is_empty() {
            let path = sess.as_ref().and_then(|s| s.path.clone());
            let cwd = sess
                .as_ref()
                .and_then(|s| s.cwd.clone())
                .unwrap_or_else(|| self.grok_cwd());
            let sid = id.to_string();
            let (tx, rx) = mpsc::channel();
            self.session_show_rx = Some((sid.clone(), rx));
            std::thread::spawn(move || {
                let mut text = String::new();
                if let Some(path) = path {
                    text = config::read_file_capped(&path, config::MEMORY_FILE_CAP);
                }
                if text.trim().is_empty() {
                    if let Some(bin) = grokhub_acp::find_grok() {
                        text = grokhub_acp::show_session(&bin, &cwd, &sid).unwrap_or_default();
                    }
                }
                let _ = tx.send(text);
            });
        }
        self.threads.push(t);
        self.apply_switch_thread(self.threads.len() - 1);
        self.acp = None;
        self.nav = Nav::Chat;
        self.status = format!("Opened {title}");
        self.persist();
    }

    fn ensure_acp(&mut self) -> Result<(), String> {
        let idx = self
            .chat_job_thread
            .as_deref()
            .and_then(|id| self.threads.iter().position(|t| t.id == id))
            .unwrap_or(self.thread_idx);
        let bound = self.grok_cwd();
        let cwd = self
            .threads
            .get(idx)
            .and_then(|t| t.grok_cwd.clone())
            .filter(|s| !s.trim().is_empty())
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| bound.clone());
        let resume = self
            .threads
            .get(idx)
            .and_then(|t| t.grok_session.clone())
            .filter(|s| !s.trim().is_empty());
        if let Some(h) = &self.acp {
            if h.cwd == cwd {
                if let Some(id) = resume.as_deref() {
                    if h.session_id == id {
                        return Ok(());
                    }
                }
            }
        }
        if self.acp_spawn_rx.is_some() {
            return Ok(());
        }
        self.acp = None;
        let console = self.console_key().trim();
        let xai_env = if !console.is_empty() && !grokhub_acp::protocol::is_jwt_api_key(console) {
            Some(console.to_string())
        } else {
            None
        };
        let grok_login = grokhub_acp::grok_cli_key()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let auth_key = grok_login.or_else(|| xai_env.clone());
        let perm = self.permission_mode;
        let mode = self.session_mode;
        let foreign = self
            .threads
            .get(idx)
            .and_then(|t| t.grok_cwd.as_ref())
            .map(|p| std::path::PathBuf::from(p) != bound)
            .unwrap_or(false);
        let unknown_cwd = self
            .threads
            .get(idx)
            .map(|t| t.grok_cwd.as_ref().map(|s| s.trim().is_empty()).unwrap_or(true))
            .unwrap_or(true);
        let (tx, rx) = mpsc::channel();
        self.acp_spawn_rx = Some(rx);
        std::thread::spawn(move || {
            if unknown_cwd && resume.as_ref().is_some() {
                let _ = tx.send(Err(grokhub_acp::explain_handshake_error(
                    "session/load refused: History session has no worktree",
                    &cwd,
                )));
                return;
            }
            let spawn = |resume: Option<String>| {
                build_agent::spawn_session(
                    cwd.clone(),
                    auth_key.clone(),
                    xai_env.clone(),
                    perm,
                    mode,
                    resume,
                )
            };
            let out = match spawn(resume.clone()) {
                Ok(h) => Ok(h),
                Err(e) => {
                    if resume.is_none() {
                        Err(grokhub_acp::explain_handshake_error(&e, &cwd))
                    } else if foreign || unknown_cwd || grokhub_acp::is_session_cwd_error(&e) {
                        Err(grokhub_acp::explain_handshake_error(&e, &cwd))
                    } else {
                        spawn(None).map_err(|e2| grokhub_acp::explain_handshake_error(&e2, &cwd))
                    }
                }
            };
            let _ = tx.send(out);
        });
        Ok(())
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
                if self.pick_rx.is_some() {
                    self.status = "Choose a file…".into();
                    return;
                }
                let (tx, rx) = mpsc::channel();
                self.pick_rx = Some(rx);
                self.status = "Choose a file…".into();
                std::thread::spawn(move || {
                    let out = match pick_file() {
                        Some(p) => plus_from_path(target, p),
                        None => PlusPick::NativeMiss,
                    };
                    let _ = tx.send((target, out));
                });
            }
            PlusAct::Paste => {
                if self.pick_rx.is_some() {
                    self.status = "Reading clipboard…".into();
                    return;
                }
                let (tx, rx) = mpsc::channel();
                self.pick_rx = Some(rx);
                self.status = "Reading clipboard…".into();
                std::thread::spawn(move || {
                    let out = if let Some(p) = clipboard_image() {
                        plus_from_path(target, p)
                    } else if let Some(t) = crate::desktop::clipboard_once() {
                        PlusPick::ClipText(t)
                    } else {
                        PlusPick::ClipEmpty
                    };
                    let _ = tx.send((target, out));
                });
            }
        }
    }

    fn poll_pick(&mut self) {
        let Some(rx) = self.pick_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok((target, PlusPick::Ready(ready))) => {
                self.apply_plus_ready(target, ready);
            }
            Ok((target, PlusPick::NativeMiss)) => {
                self.file_pick = Some(target);
                if self.status == "Choose a file…" {
                    self.status.clear();
                }
            }
            Ok((target, PlusPick::ClipText(clip))) => {
                self.apply_clipboard(target, &clip);
            }
            Ok((_, PlusPick::ClipEmpty)) => {
                self.status = plus_empty_status().into();
            }
            Ok((_, PlusPick::Err(e))) => {
                self.status = e;
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.pick_rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                if self.status == "Choose a file…"
                    || self.status == "Reading clipboard…"
                    || self.status == "Reading file…"
                {
                    self.status.clear();
                }
            }
        }
    }

    fn apply_clipboard(&mut self, target: PlusTarget, clip: &str) {
        match target {
            PlusTarget::Chat => {
                self.composer = append_composer(&self.composer, clip);
                self.status = "Pasted clipboard".into();
            }
            PlusTarget::Imagine => {
                self.imagine_prompt = append_composer(&self.imagine_prompt, clip);
                self.status = "Pasted clipboard".into();
            }
        }
    }

    fn apply_plus_ready(&mut self, target: PlusTarget, ready: PlusReady) {
        match target {
            PlusTarget::Chat => match ready.kind {
                AttachKind::Image => {
                    if let Some(url) = ready.image_url {
                        self.attach_url = Some(url);
                        self.attach_name = Some(ready.name.clone());
                        self.status = chat_attach_status(ready.kind, &ready.name);
                    }
                }
                AttachKind::Text => {
                    if let Some(t) = ready.text {
                        self.composer = append_composer(&self.composer, &t);
                        self.status = chat_attach_status(ready.kind, &ready.name);
                    }
                }
                AttachKind::Other => {
                    self.composer = append_composer(&self.composer, &ready.raw);
                    self.status = chat_attach_status(ready.kind, &ready.name);
                }
            },
            PlusTarget::Imagine => match ready.kind {
                AttachKind::Image => {
                    self.imagine_ref = Some(ready.name.clone());
                    let hint = attach_prompt_line(ready.kind, &ready.name);
                    self.imagine_prompt = append_composer(&self.imagine_prompt, &hint);
                    self.status = imagine_ref_status(&ready.name);
                }
                AttachKind::Text => {
                    if let Some(t) = ready.text {
                        self.imagine_prompt = append_composer(&self.imagine_prompt, &t);
                        self.status = chat_attach_status(ready.kind, &ready.name);
                    }
                }
                AttachKind::Other => {
                    self.imagine_prompt = append_composer(&self.imagine_prompt, &ready.raw);
                    self.status = chat_attach_status(ready.kind, &ready.name);
                }
            },
        }
        self.file_pick = None;
    }

    fn start_plus_path(&mut self, target: PlusTarget, path: PathBuf) {
        if self.pick_rx.is_some() {
            self.status = "Reading file…".into();
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.pick_rx = Some(rx);
        self.status = "Reading file…".into();
        std::thread::spawn(move || {
            let _ = tx.send((target, plus_from_path(target, path)));
        });
    }

    fn clear_chat_attach(&mut self) {
        self.attach_url = None;
        self.attach_name = None;
        self.status.clear();
    }

    fn drop_leaving_thread_chrome(&mut self) {
        if self.running {
            self.halt_in_flight();
        }
        self.attach_url = None;
        self.attach_name = None;
        self.followup_step = 0;
        self.active_skill_follow = None;
        self.hands_attach = false;
        self.eyes_attach = false;
        self.last_receipt_ok = None;
        self.acp = None;
        self.acp_spawn_rx = None;
        self.tool_cards.clear();
        self.perm_ask = None;
    }

    fn pick_entries(dir: &Path) -> Vec<(String, bool)> {
        let mut dirs = Vec::new();
        let mut files = Vec::new();
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                if dirs.len() + files.len() >= 400 {
                    break;
                }
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with('.') || name.is_empty() {
                    continue;
                }
                let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                if is_dir {
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

    fn cached_pick_entries(&mut self) -> &[(String, bool)] {
        let dir = self.pick_dir.clone();
        let stale = self
            .pick_cache
            .as_ref()
            .map(|(cached, _)| cached != &dir)
            .unwrap_or(true);
        if stale && self.pick_list_rx.is_none() {
            let (tx, rx) = mpsc::channel();
            self.pick_list_rx = Some(rx);
            std::thread::spawn(move || {
                let entries = Self::pick_entries(Path::new(&dir));
                let _ = tx.send((dir, entries));
            });
        }
        if self
            .pick_cache
            .as_ref()
            .map(|(cached, _)| cached == &self.pick_dir)
            .unwrap_or(false)
        {
            return self
                .pick_cache
                .as_ref()
                .map(|(_, entries)| entries.as_slice())
                .unwrap_or(&[]);
        }
        &[]
    }

    fn poll_pick_list(&mut self) {
        let Some(rx) = self.pick_list_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok((dir, entries)) => {
                if dir == self.pick_dir {
                    self.pick_cache = Some((dir, entries));
                }
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.pick_list_rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {}
        }
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
            let entries = self.cached_pick_entries().to_vec();
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
                self.start_plus_path(target, p);
            } else if paste {
                self.file_pick = None;
                self.run_plus_act(target, PlusAct::Paste);
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
        self.projects_dirty = false;
        let nodes = self.projects.clone();
        let io = self.persist_io.clone();
        std::thread::spawn(move || {
            if let Ok(_g) = io.lock() {
                let _ = crate::store::save_projects(&nodes);
            }
        });
    }

    fn bind_project_id(&mut self, id: &str) {
        let Some(n) = self.projects.iter().find(|n| n.id == id && n.kind == ProjectKind::Project) else {
            return;
        };
        let path = expand_home(&n.path);
        let name = n.name.clone();
        let tree_changed = !path.trim().is_empty() && self.cfg.project_dir != path;
        if !path.trim().is_empty() {
            if !std::path::Path::new(&path).is_dir() {
                let p = path.clone();
                std::thread::spawn(move || {
                    let _ = std::fs::create_dir_all(&p);
                });
            }
            self.cfg.project_dir = path.clone();
        }
        let already = self.project_sel.as_deref() == Some(id);
        self.project_sel = Some(id.to_string());
        if click_project_opens_board(already) {
            self.nav = Nav::Workboard;
        }
        self.status = format!("Bound {name}");
        if self.running {
            self.halt_in_flight();
        }
        self.acp = None;
        self.acp_spawn_rx = None;
        if tree_changed {
            if let Some(t) = self.threads.get_mut(self.thread_idx) {
                t.grok_cwd = None;
                t.grok_session = None;
            }
            self.persist();
        } else {
            self.persist_cfg();
        }
    }

    fn make_project(&mut self, name: &str, parent: Option<&str>) {
        let id = uid("proj");
        let root = self.work_root();
        match create_project(&mut self.projects, &id, name, parent, &root) {
            Ok(i) => {
                let path = self.projects[i].path.clone();
                if !std::path::Path::new(&path).is_dir() {
                    let p = path.clone();
                    std::thread::spawn(move || {
                        let _ = std::fs::create_dir_all(&p);
                    });
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
            if self.running {
                self.halt_in_flight();
            }
            self.acp = None;
            self.acp_spawn_rx = None;
            if let Some(t) = self.threads.get_mut(self.thread_idx) {
                t.grok_cwd = None;
                t.grok_session = None;
            }
        }
        if selected {
            self.project_sel = None;
        }
        self.touch_projects();
        if out.unbound {
            self.persist();
        } else {
            self.flush_projects();
        }
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
                    self.flush_projects();
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
                self.flush_projects();
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
                self.flush_projects();
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
                self.flush_projects();
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
                self.flush_projects();
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
                                let p = path.clone();
                                std::thread::spawn(move || {
                                    let _ = std::fs::create_dir_all(&p);
                                });
                            }
                            self.bind_project_id(&id);
                            bound = true;
                        }
                    }
                }
                if !bound {
                    self.flush_projects();
                }
            }
            Err(e) => {
                if staged {
                    drop_node(&mut self.projects, &id);
                    self.touch_projects();
                    self.flush_projects();
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
                    self.flush_projects();
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
                self.flush_projects();
            }
            Err(e) => self.status = e.into(),
        }
    }

    fn chat_pairs(&self) -> Vec<(String, String)> {
        self.messages
            .iter()
            .map(|m| (m.0.clone(), m.1.clone()))
            .collect()
    }

    /// Chat pairs for chip rebuild: scan a 4KB prefix so an 8MB complete
    /// does not get cloned into `chip_suggest_prompt` / `chip_thread_from_messages`.
    fn chip_chat_pairs(&self) -> Vec<(String, String)> {
        self.messages
            .iter()
            .map(|m| (m.0.clone(), chip_scan(&m.1).to_string()))
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
        if let Some(rx) = self.greeting_files_rx.take() {
            match rx.try_recv() {
                Ok((user_at, user, memory_at, memory)) => {
                    self.greeting_user_md = user;
                    self.greeting_user_at = user_at;
                    self.greeting_memory_md = memory;
                    self.greeting_memory_at = memory_at;
                }
                Err(mpsc::TryRecvError::Empty) => {
                    self.greeting_files_rx = Some(rx);
                }
                Err(mpsc::TryRecvError::Disconnected) => {}
            }
        }
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
        if !self.scratch()
            && (self.greeting_flush_name != self.mem_name
                || self.greeting_flush_len != self.mem_body.len())
        {
            let name = self.mem_name.clone();
            let body = self.mem_body.clone();
            std::thread::spawn(move || {
                if config::read_memory(&name) != body {
                    let _ = config::write_memory(&name, &body);
                }
            });
            self.greeting_flush_name = self.mem_name.clone();
            self.greeting_flush_len = self.mem_body.len();
        }
        let user_at = config::memory_updated_at("USER.md");
        let memory_at = config::memory_updated_at("MEMORY.md");
        if self.greeting_user_at != user_at || self.greeting_memory_at != memory_at {
            if self.greeting_user_at == 0 && self.greeting_memory_at == 0 {
                self.greeting_user_md = config::read_memory("USER.md");
                self.greeting_memory_md = config::read_memory("MEMORY.md");
                self.greeting_user_at = user_at;
                self.greeting_memory_at = memory_at;
            } else if self.greeting_files_rx.is_none() {
                let (tx, rx) = mpsc::channel();
                self.greeting_files_rx = Some(rx);
                std::thread::spawn(move || {
                    let user = config::read_memory("USER.md");
                    let memory = config::read_memory("MEMORY.md");
                    let user_at = config::memory_updated_at("USER.md");
                    let memory_at = config::memory_updated_at("MEMORY.md");
                    let _ = tx.send((user_at, user, memory_at, memory));
                });
            }
        }
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
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_default();
        let hour = Self::chip_hour();
        let last_night = self.last_night_hint();
        let (local, fp, llm_prompt) = {
            let user_md = if self.mem_name == "USER.md" {
                self.mem_body.as_str()
            } else {
                self.greeting_user_md.as_str()
            };
            let memory_md = if self.mem_name == "MEMORY.md" {
                self.mem_body.as_str()
            } else {
                self.greeting_memory_md.as_str()
            };
            let input = GreetingInput {
                user_md,
                memory_md,
                insights: &insights,
                display_name: &display_name,
                hour,
                last_night: &last_night,
            };
            let fp = greeting_fingerprint(&input);
            let local = local_greeting(&input);
            let llm_prompt = if should_refresh_greeting(
                &self.greeting_llm_fp,
                &fp,
                self.greeting_llm_at,
                now_ms(),
                self.llm_ready(),
                self.greeting_busy,
            ) {
                Some(greeting_prompt(&input))
            } else {
                None
            };
            (local, fp, llm_prompt)
        };
        if self.greeting_fp != fp {
            self.greeting = local;
            self.greeting_fp = fp.clone();
        }
        if let Some(prompt) = llm_prompt {
            self.greeting_llm_fp = fp;
            self.greeting_llm_at = now_ms();
            self.spawn_greeting_llm(prompt);
        }
    }

    fn spawn_greeting_llm(&mut self, prompt: String) {
        if self.greeting_busy {
            return;
        }
        let key = self.bearer();
        if key.trim().is_empty() && grokhub_acp::find_grok().is_none() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.greeting_rx = Some(rx);
        self.greeting_busy = true;
        std::thread::spawn(move || {
            let raw = cabin_fast_llm(key, prompt);
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
        let renaming = self
            .rename_idx
            .and_then(|i| self.threads.get(i))
            .is_some_and(|r| r.id == tid);
        {
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
                if apply_auto_title_in(&mut tab, &t.goal.label, renaming) {
                    t.title = tab.title;
                    t.accessed_ms = now_ms();
                }
                if tid == current {
                    self.cfg.goal_pin = t.goal.label.clone();
                }
            }
        }
        self.persist();
    }

    fn spawn_thread_goal(&mut self) {
        self.spawn_thread_goal_on(None);
    }

    fn spawn_thread_goal_on(&mut self, thread_id: Option<&str>) {
        if self.goal_busy {
            self.goal_stale = true;
            return;
        }
        let vis = self.visible_thread_id();
        let tid = thread_id
            .map(|s| s.to_string())
            .or_else(|| self.threads.get(self.thread_idx).map(|t| t.id.clone()))
            .unwrap_or_default();
        let on_visible = tid == vis || tid.is_empty();
        let scratch = if on_visible {
            self.scratch()
        } else {
            self.threads
                .iter()
                .find(|t| t.id == tid)
                .map(|t| t.scratch)
                .unwrap_or(false)
        };
        let pairs = if on_visible {
            self.chip_chat_pairs()
        } else {
            self.threads
                .iter()
                .find(|t| t.id == tid)
                .map(|t| {
                    t.messages
                        .iter()
                        .map(|(r, c)| (r.clone(), chip_scan(c).to_string()))
                        .collect()
                })
                .unwrap_or_default()
        };
        let user_turns = visible_turn_count(&pairs);
        let locked = self
            .threads
            .iter()
            .find(|t| t.id == tid)
            .map(|t| t.title_locked)
            .unwrap_or(false);
        if locked || !should_name_thread(scratch, user_turns) {
            self.goal_stale = false;
            return;
        }
        if !self.llm_ready() {
            self.goal_stale = false;
            return;
        }
        if tid.is_empty() {
            self.goal_stale = false;
            return;
        }
        let prompt = thread_goal_prompt(&pairs);
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
        if self.running {
            return;
        }
        let hour = Self::chip_hour();
        let n = self.messages.len();
        let last = self.messages.last().map(|m| m.1.len()).unwrap_or(0);
        let title = self
            .threads
            .get(self.thread_idx)
            .map(|t| t.title.clone())
            .unwrap_or_default();
        let last_failed = self.last_receipt_ok == Some(false);
        let draft_head: String = self.composer.chars().take(16).collect();
        let draft_tail: String = self.composer.chars().rev().take(16).collect();
        let key = format!(
            "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}",
            self.thread_idx,
            n,
            last,
            self.composer.len(),
            draft_head,
            draft_tail,
            hour,
            last_failed,
            title,
            self.chip_dismissed.len(),
            self.llm_chips.len(),
            self.has_key(),
            self.llm_ready(),
            self.usage.messages
        );
        if self.chip_paint_key == key && !self.visible_chips.is_empty() {
            return;
        }
        self.chip_paint_key = key;
        let chat = self.chip_chat_pairs();
        let others = self.other_chip_threads();
        let input = ChipInput {
            chat: &chat,
            draft: &self.composer,
            grok_connected: self.llm_ready(),
            host_on: false,
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
            self.llm_ready(),
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
        if key.trim().is_empty() && grokhub_acp::find_grok().is_none() {
            return;
        }
        let chat = self.chip_chat_pairs();
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
        let (tx, rx) = mpsc::channel();
        self.chip_rx = Some(rx);
        self.chip_busy = true;
        std::thread::spawn(move || {
            let chips = parse_llm_chips(&cabin_fast_llm(key, prompt));
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
            &self.chip_chat_pairs(),
            &self.composer,
            self.last_receipt_ok == Some(false),
            hour,
            mode,
        );
        remember_chip_click(&mut self.chip_memory, &chip, Some(&tag), now_ms(), hour);
        self.flush_chips();
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
        self.flush_chips();
    }

    fn flush_chips(&self) {
        let chips = self.chip_memory.clone();
        let io = self.persist_io.clone();
        std::thread::spawn(move || {
            if let Ok(_g) = io.lock() {
                let _ = crate::store::save_chips(&chips);
            }
        });
    }

    fn flush_board(&mut self) {
        let board = self.board.clone();
        let io = self.persist_io.clone();
        self.persist_idle_key = self.persist_idle_now();
        self.last_persist = Instant::now();
        std::thread::spawn(move || {
            if let Ok(_g) = io.lock() {
                let _ = config::save_board(&board);
            }
        });
    }

    fn nav_from_id(id: &str) -> Nav {
        match id {
            "settings" => Nav::Settings,
            "imagine" => Nav::Imagine,
            "history" => Nav::History,
            "workboard" => Nav::Workboard,
            "skills" => Nav::Skills,
            "night" | "automations" => Nav::Night,
            "agents" | "queue" => Nav::Agents,
            "devices" => Nav::Devices,
            "memory" => Nav::Memory,
            "connectors" => Nav::Connectors,
            "chat" => Nav::Chat,
            _ => Nav::Chat,
        }
    }

    fn bearer(&mut self) -> String {
        if let Some(k) = grokhub_acp::grok_cli_key() {
            if !k.trim().is_empty() {
                let exp = grokhub_core::jwt_exp_ms(&k);
                let stale = exp
                    .map(|exp| exp.saturating_sub(grokhub_core::TOKEN_REFRESH_SKEW_MS) < now_ms())
                    .unwrap_or(false);
                if stale {
                    if let Some(fresh) = crate::oauth::refresh_grok_login() {
                        return fresh;
                    }
                    let hard_expired = exp.map(|e| e < now_ms()).unwrap_or(false);
                    if !hard_expired {
                        return k;
                    }
                } else {
                    return k;
                }
            }
        }
        let mut oauth_usable = false;
        if let Some(tok) = self.secrets.oauth.clone() {
            let mut tok = tok;
            if grokhub_core::token_needs_refresh(&tok, now_ms()) {
                if let Some(next) = crate::oauth::refresh_cabin_oauth(&tok) {
                    self.secrets.oauth = Some(next.clone());
                    let io = self.persist_io.clone();
                    let secrets = self.secrets.clone();
                    std::thread::spawn(move || {
                        if let Ok(_g) = io.lock() {
                            let _ = secrets::save(&secrets);
                        }
                    });
                    tok = next;
                }
            }
            if oauth_access_live(&tok, now_ms()) {
                oauth_usable = true;
                if self.console_key().trim().is_empty() {
                    return tok.access_token;
                }
            }
        }
        chat_bearer(
            self.console_key(),
            &secrets::access_token(&self.secrets),
            oauth_usable,
        )
        .or_else(grokhub_acp::grok_cli_key)
        .unwrap_or_default()
    }

    fn switch_thread(&mut self, idx: usize) {
        self.apply_switch_thread(idx);
        self.persist_bg();
    }

    fn live_mut(&mut self) -> &mut Vec<(String, String)> {
        Arc::make_mut(&mut self.messages)
    }

    fn apply_switch_thread(&mut self, idx: usize) {
        let idx = idx.min(self.threads.len().saturating_sub(1));
        if idx != self.thread_idx {
            if let Some(t) = self.threads.get_mut(self.thread_idx) {
                t.messages = self.messages.clone();
                flush_visible_goal(&mut t.goal, self.goal_step, &self.cfg.goal_pin);
            }
            self.thread_idx = idx;
            self.messages = self
                .threads
                .get(self.thread_idx)
                .map(|t| t.messages.clone())
                .unwrap_or_else(|| Arc::new(Vec::new()));
        } else if let Some(t) = self.threads.get_mut(self.thread_idx) {
            flush_visible_goal(&mut t.goal, self.goal_step, &self.cfg.goal_pin);
        }
        self.rename_idx = None;
        self.imagine_last =
            last_imagine_receipt(self.messages.iter().map(|(_, c)| c.as_str())).unwrap_or_default();
        self.cfg.goal_pin = self
            .threads
            .get(self.thread_idx)
            .map(|t| t.goal.label.clone())
            .unwrap_or_default();
        self.goal_step = self
            .threads
            .get(self.thread_idx)
            .map(|t| t.goal.step)
            .unwrap_or(0);
        self.drop_leaving_thread_chrome();
        self.stamp_current_access();
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

    fn land_on_real_chat(&mut self) {
        if self.scratch() {
            if let Some(idx) = threads::most_recently_accessed_index(&self.threads) {
                if idx != self.thread_idx {
                    self.apply_switch_thread(idx);
                }
            }
        }
        self.nav = Nav::Chat;
    }

    fn new_thread(&mut self, scratch: bool) {
        let reuse = {
            let views: Vec<ThreadReuseView> = self
                .threads
                .iter()
                .enumerate()
                .map(|(i, t)| ThreadReuseView {
                    title: t.title.as_str(),
                    scratch: t.scratch,
                    empty: if i == self.thread_idx {
                        self.messages.is_empty()
                    } else {
                        t.messages.is_empty()
                    },
                })
                .collect();
            reuse_empty_thread_idx(&views, self.thread_idx, scratch)
        };
        if let Some(idx) = reuse {
            if idx != self.thread_idx {
                self.apply_switch_thread(idx);
            } else {
                self.drop_leaving_thread_chrome();
            }
            if let Some(t) = self.threads.get_mut(self.thread_idx) {
                t.grok_session = None;
                t.grok_cwd = None;
            }
            self.stamp_current_access();
            self.persist();
            self.status = if scratch {
                "Scratch — no memory writes".into()
            } else {
                "New chat".into()
            };
            return;
        }
        if let Some(t) = self.threads.get_mut(self.thread_idx) {
            t.messages = self.messages.clone();
            flush_visible_goal(&mut t.goal, self.goal_step, &self.cfg.goal_pin);
        }
        let title = if scratch { "Scratch" } else { "Chat" };
        self.threads.push(ChatThread::new(title, scratch));
        self.thread_idx = self.threads.len() - 1;
        self.messages = Arc::new(Vec::new());
        self.imagine_last.clear();
        self.cfg.goal_pin.clear();
        self.goal_step = 0;
        self.drop_leaving_thread_chrome();
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
            t.accessed_ms = now_ms();
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
        t.accessed_ms = now_ms();
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
                self.finish_hub_dispatch("Chat deleted", false);
                self.threads.clear();
                self.threads.push(ChatThread::new("Chat", false));
                self.thread_idx = 0;
                self.messages = self.threads[0].messages.clone();
                self.imagine_last.clear();
                self.cfg.goal_pin.clear();
                self.goal_step = 0;
                self.drop_leaving_thread_chrome();
                self.status = "Chat deleted".into();
                self.stamp_current_access();
            }
            DeleteOutcome::Removed { next } => {
                let gone = self.threads.remove(idx);
                if self.chat_job_thread.as_deref() == Some(gone.id.as_str()) {
                    self.halt_in_flight();
                    self.finish_hub_dispatch("Chat deleted", false);
                }
                self.thread_idx = next;
                if was_current {
                    self.messages = self
                        .threads
                        .get(next)
                        .map(|t| t.messages.clone())
                        .unwrap_or_else(|| Arc::new(Vec::new()));
                    self.imagine_last = last_imagine_receipt(
                        self.messages.iter().map(|m| m.1.as_str()),
                    )
                    .unwrap_or_default();
                    self.cfg.goal_pin = self
                        .threads
                        .get(next)
                        .map(|t| t.goal.label.clone())
                        .unwrap_or_default();
                    self.goal_step = self
                        .threads
                        .get(next)
                        .map(|t| t.goal.step)
                        .unwrap_or(0);
                    self.drop_leaving_thread_chrome();
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
        if let Some(slash) = parse_slash(&text) {
            self.run_slash(slash);
            return;
        }
        if unknown_cabin_slash(&text) {
            self.status = "Unknown command — /help".into();
            return;
        }
        match chat_send_kind(
            self.chat_job_thread.as_deref(),
            &self.visible_thread_id(),
            self.running,
        ) {
            ChatSendKind::Redirect => {
                let prev = last_user_scan(
                    self.messages
                        .iter()
                        .map(|m| (m.0.as_str(), m.1.as_str())),
                )
                .unwrap_or_default();
                self.halt_work("Redirected");
                text = redirect_prompt(&prev, &text);
            }
            ChatSendKind::Fresh => {
                if self.running && self.chat_job_thread.is_some() {
                    self.halt_in_flight();
                    self.finish_hub_dispatch("Interrupted", false);
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
        if !persist_user_turn(self.can_agent()) {
            self.hands_attach = false;
            self.eyes_attach = false;
            self.speak_next = false;
            self.status = "Install Grok Build (x.ai/cli) or Connect Grok in Settings".into();
            return;
        }
        if let Some(name) = self.attach_name.as_deref() {
            if !name.trim().is_empty() {
                text = append_composer(&text, &attach_prompt_line(AttachKind::Image, name));
            }
        }
        self.verify_ok_turn = verify_ok_after_user_turn(self.verify_ok_turn, true);
        self.active_skill_follow = None;
        if let Some(sk) = match_skill(&text, &self.skill_list) {
            self.skill_name = sk.name.clone();
            self.status = format!("Skill {}", sk.name);
            if self.policy().injects_skill() {
                self.active_skill_follow = Some(skill_follow_block(sk));
            }
        }
        self.live_mut().push(("user".into(), text.clone()));
        self.eyes_attach = false;
        self.hands_attach = false;
        self.followup_step = 0;
        self.stamp_current_access();
        self.persist();
        self.kick_model(true);
    }

    fn send_followup_turn(&mut self) {
        if self.followup_step >= FOLLOWUP_MAX_STEPS {
            return;
        }
        self.followup_step += 1;
        self.push_bound_msg("user", FOLLOWUP_PROMPT.into());
        self.persist();
        self.kick_model(false);
    }

    fn run_slash(&mut self, slash: Slash) {
        match slash {
            Slash::Forget(topic) => {
                if self.scratch() {
                    self.status = "Scratch — no memory writes".into();
                    return;
                }
                match topic {
                    None => {
                        std::thread::spawn(|| {
                            let _ = config::write_memory("MEMORY.md", "");
                        });
                        if self.mem_name == "MEMORY.md" {
                            self.mem_body.clear();
                        }
                        self.status = "Forgot MEMORY.md".into();
                    }
                    Some(q) => {
                        let name = self.mem_name.clone();
                        let body = self.mem_body.clone();
                        std::thread::spawn(move || {
                            if name != "MEMORY.md" && config::read_memory(&name) != body {
                                let _ = config::write_memory(&name, &body);
                            }
                        });
                        if self.mem_name == "MEMORY.md" {
                            let next = forget_topic(&self.mem_body, &q);
                            let written = next.clone();
                            std::thread::spawn(move || {
                                let _ = config::write_memory("MEMORY.md", &written);
                            });
                            self.mem_body = next;
                            self.status = format!("Forgot {q}");
                        } else {
                            let topic = q.clone();
                            std::thread::spawn(move || {
                                let current = config::read_memory("MEMORY.md");
                                let next = forget_topic(&current, &topic);
                                let _ = config::write_memory("MEMORY.md", &next);
                            });
                            self.status = format!("Forgot {q}");
                        }
                    }
                }
            }
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
                let name = self.mem_name.clone();
                let body = self.mem_body.clone();
                if name != "MEMORY.md" {
                    std::thread::spawn(move || {
                        if config::read_memory(&name) != body {
                            let _ = config::write_memory(&name, &body);
                        }
                    });
                }
                if self.mem_name == "MEMORY.md" {
                    let mut next = self.mem_body.clone();
                    if !next.is_empty() && !next.ends_with('\n') {
                        next.push('\n');
                    }
                    next.push_str(note.trim());
                    next.push('\n');
                    let written = next.clone();
                    std::thread::spawn(move || {
                        let _ = config::write_memory("MEMORY.md", &written);
                    });
                    self.mem_body = next;
                    self.status = "Wrote MEMORY.md".into();
                } else {
                    let note = note.clone();
                    std::thread::spawn(move || {
                        let _ = config::append_memory("MEMORY.md", &note);
                    });
                    self.status = "Wrote MEMORY.md".into();
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
                let pin = self.cfg.goal_pin.trim().to_string();
                let start = compact_keep_start_from(
                    self.messages.iter().map(|m| (m.0.as_str(), m.1.as_str())),
                    8,
                );
                if start > 0 {
                    self.live_mut().drain(..start);
                }
                if !pin.is_empty() {
                    let marked = format!("GOAL PIN: {pin}");
                    if !self
                        .messages
                        .iter()
                        .any(|m| m.1 == marked || m.1.starts_with(&format!("{marked}\n")))
                    {
                        self.live_mut().insert(
                            0,
                            ("system".into(), marked),
                        );
                    }
                }
                self.stamp_current_access();
                self.persist();
                self.status = "Compacted".into();
            }
            Slash::Skill(name) => {
                if let Some(s) = self.skill_list.iter().find(|s| s.name == name || s.slash == name) {
                    self.nav = Nav::Chat;
                    self.send_chat(skill_use_in_chat_prompt(&s.slash, &s.name));
                } else {
                    self.status = format!("No skill {name}");
                }
            }
            Slash::LearnReflect => self.run_reflect(),
            Slash::Update => self.queue_update(),
            Slash::Help => {
                self.live_mut().push(("assistant".into(), mark_slash_result(&slash_help())));
                self.stamp_current_access();
                self.persist();
            }
            Slash::New => {
                self.new_thread(false);
            }
            Slash::Scratch => self.new_thread(true),
            Slash::Clear => {
                if self.running {
                    self.halt_in_flight();
                    self.finish_hub_dispatch("Cleared in-flight reply", false);
                }
                self.drop_leaving_thread_chrome();
                self.messages = Arc::new(Vec::new());
                self.followup_step = 0;
                self.active_skill_follow = None;
                if let Some(t) = self.threads.get_mut(self.thread_idx) {
                    t.grok_session = None;
                    t.grok_cwd = None;
                    t.messages = self.messages.clone();
                }
                self.stamp_current_access();
                self.persist();
                self.status = "Cleared".into();
            }
            Slash::Undo => {
                if self.running {
                    self.halt_in_flight();
                    self.finish_hub_dispatch("Undid in-flight reply", false);
                    self.status = "Undid in-flight reply".into();
                } else if let Some(i) = self.messages.iter().rposition(|m| m.0 == "assistant") {
                    self.live_mut().remove(i);
                    self.followup_step = 0;
                    self.active_skill_follow = None;
                    self.stamp_current_access();
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
                    .find(|m| m.0 == "user" && !is_workload_user(&m.1))
                {
                    let t = m.1.clone();
                    self.kick_model_retry(t);
                } else {
                    self.status = "Nothing to retry".into();
                }
            }
            Slash::Stop => self.halt_work("Stopped"),
            Slash::Sh(cmd) => self.queue_sh(cmd),
            Slash::HostStatus => {
                self.status = build_agent::grok_banner();
            }
            Slash::Rename(title) => self.rename_thread(self.thread_idx, &title),
            Slash::Pin => self.pin_thread(self.thread_idx),
            Slash::Delete => self.delete_thread_at(self.thread_idx),
            Slash::Context => {
                let n = visible_turn_count_from(
                    self.messages.iter().map(|m| (m.0.as_str(), m.1.as_str())),
                );
                let tokens = estimate_messages_from(
                    self.messages.iter().map(|m| (m.0.as_str(), m.1.as_str())),
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
                self.persist_cfg();
                self.status = mode_status_line(&mode, &self.cfg.model);
            }
            Slash::Dream => self.run_dream(),
            Slash::Import => self.import_openclaw(),
            Slash::Consult(q) => self.run_consult(q),
            Slash::Usage => {
                self.status = usage_line(&self.usage);
            }
            Slash::Models => {
                self.live_mut().push(("assistant".into(), mark_slash_result(&catalog_line())));
                self.stamp_current_access();
                self.persist();
            }
            Slash::Palette => self.open_palette(),
            Slash::Plan => {
                if self.running {
                    self.halt_in_flight();
                }
                self.session_mode = SessionMode::Plan;
                self.acp = None;
                self.acp_spawn_rx = None;
                if let Some(t) = self.threads.get_mut(self.thread_idx) {
                    t.grok_session = None;
                }
                self.persist_idle_key = self.persist_idle_now();
                self.status = "Plan mode — Grok Build will plan first".into();
            }
            Slash::AlwaysApprove => {
                self.permission_mode = if self.permission_mode == PermissionMode::AlwaysApprove {
                    PermissionMode::Ask
                } else {
                    PermissionMode::AlwaysApprove
                };
                if self.running {
                    self.halt_in_flight();
                }
                self.acp = None;
                self.acp_spawn_rx = None;
                if let Some(t) = self.threads.get_mut(self.thread_idx) {
                    t.grok_session = None;
                }
                self.persist_idle_key = self.persist_idle_now();
                self.status = format!("Permission {}", self.permission_mode.as_str());
            }
            Slash::AutoPerm => {
                self.permission_mode = PermissionMode::Auto;
                if self.running {
                    self.halt_in_flight();
                }
                self.acp = None;
                self.acp_spawn_rx = None;
                if let Some(t) = self.threads.get_mut(self.thread_idx) {
                    t.grok_session = None;
                }
                self.persist_idle_key = self.persist_idle_now();
                self.status = "Permission auto".into();
            }
            Slash::Effort(level) => {
                self.cfg.mode = match level.to_ascii_lowercase().as_str() {
                    "xhigh" | "max" => "max".into(),
                    "high" | "think" => "think".into(),
                    "low" | "fast" => "fast".into(),
                    _ => "balanced".into(),
                };
                self.persist_cfg();
                self.status = format!("Effort {level}");
            }
            Slash::Sessions => {
                self.nav = Nav::History;
                self.grok_sessions_loaded = false;
                self.reload_grok_sessions();
                self.status = if grokhub_acp::find_grok().is_some() {
                    "Listing Grok sessions…".into()
                } else {
                    build_agent::grok_banner()
                };
            }
            Slash::Inspect => {
                self.nav = Nav::Connectors;
                if let Some(bin) = grokhub_acp::find_grok() {
                    let cwd = self.grok_cwd();
                    if self.inspect_rx.is_none() {
                        let (tx, rx) = mpsc::channel();
                        self.inspect_rx = Some(rx);
                        self.inspect_text = "Inspecting…".into();
                        self.status = "Grok inspect".into();
                        std::thread::spawn(move || {
                            let text = match grokhub_acp::inspect_json(&bin, &cwd) {
                                Ok(v) => serde_json::to_string_pretty(&v)
                                    .unwrap_or_else(|_| v.to_string()),
                                Err(e) => e,
                            };
                            let _ = tx.send(text);
                        });
                    }
                } else {
                    self.inspect_text = build_agent::grok_banner();
                    self.status = self.inspect_text.clone();
                }
            }
            Slash::ProjectBind(path) => {
                let raw = path.unwrap_or_else(|| self.cfg.project_dir.clone());
                let home = std::env::var("HOME").ok();
                let p = resolve_bind_path(
                    &raw,
                    &self.cfg.project_dir,
                    &self.work_root(),
                    home.as_deref(),
                );
                let tree_changed = self.cfg.project_dir != p;
                self.cfg.project_dir = p.clone();
                let dir = p.clone();
                std::thread::spawn(move || {
                    let _ = std::fs::create_dir_all(&dir);
                });
                self.project_sel = upsert_bound(&mut self.projects, &p);
                self.touch_projects();
                if self.running {
                    self.halt_in_flight();
                }
                self.acp = None;
                self.acp_spawn_rx = None;
                if tree_changed {
                    if let Some(t) = self.threads.get_mut(self.thread_idx) {
                        t.grok_cwd = None;
                        t.grok_session = None;
                    }
                    self.persist();
                } else {
                    self.flush_projects();
                    self.persist_cfg();
                }
                self.grok_sessions_loaded = false;
                self.status = format!("Bound {p}");
            }
            Slash::ProjectClear => {
                self.cfg.project_dir.clear();
                self.project_sel = None;
                if self.running {
                    self.halt_in_flight();
                }
                self.acp = None;
                self.acp_spawn_rx = None;
                if let Some(t) = self.threads.get_mut(self.thread_idx) {
                    t.grok_cwd = None;
                    t.grok_session = None;
                }
                self.grok_sessions_loaded = false;
                self.touch_projects();
                self.persist();
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
                        self.flush_projects();
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
                let dir = p.clone();
                std::thread::spawn(move || {
                    let _ = std::fs::create_dir_all(&dir);
                });
                let tree_changed = self.cfg.project_dir != p;
                self.cfg.project_dir = p.clone();
                self.project_sel = upsert_bound(&mut self.projects, &p);
                self.touch_projects();
                if self.running {
                    self.halt_in_flight();
                }
                self.acp = None;
                self.acp_spawn_rx = None;
                if tree_changed {
                    if let Some(t) = self.threads.get_mut(self.thread_idx) {
                        t.grok_cwd = None;
                        t.grok_session = None;
                    }
                    self.persist();
                } else {
                    self.flush_projects();
                    self.persist_cfg();
                }
                self.status = format!("Room {} → {p}", plan.slug);
                self.queue_sh(plan.host_script);
            }
            Slash::Export => {
                self.persist();
                if let Some(t) = self.threads.get(self.thread_idx) {
                    let t = t.clone();
                    let dest = if self.cfg.project_dir.trim().is_empty() {
                        config::config_dir().join("export.md")
                    } else {
                        std::path::PathBuf::from(expand_home(&self.cfg.project_dir)).join("export.md")
                    };
                    let status_path = dest.display().to_string();
                    std::thread::spawn(move || {
                        let md = threads::export_markdown(&t);
                        let _ = std::fs::write(&dest, md);
                    });
                    self.status = format!("Wrote {status_path}");
                }
            }
            Slash::Recall(q) => {
                if self.recall_rx.is_some() {
                    self.status = "Recalling…".into();
                    return;
                }
                if !self.scratch() {
                    let name = self.mem_name.clone();
                    let body = self.mem_body.clone();
                    std::thread::spawn(move || {
                        if config::read_memory(&name) != body {
                            let _ = config::write_memory(&name, &body);
                        }
                    });
                }
                let q_owned = q.clone();
                let mem_name = self.mem_name.clone();
                let mem_body = self.mem_body.clone();
                let vis = self.thread_idx;
                let mut thread_rows = Vec::new();
                for (i, t) in self.threads.iter().enumerate() {
                    let body = if i == vis {
                        search_thread_body(self.messages.iter().map(|m| m.1.as_str()))
                    } else {
                        search_thread_body(t.messages.iter().map(|(_, c)| c.as_str()))
                    };
                    thread_rows.push((t.title.clone(), body));
                }
                let (tx, rx) = mpsc::channel();
                self.recall_rx = Some(rx);
                self.status = "Recalling…".into();
                std::thread::spawn(move || {
                    let soul = if mem_name == "SOUL.md" {
                        mem_body.clone()
                    } else {
                        config::read_memory("SOUL.md")
                    };
                    let user = if mem_name == "USER.md" {
                        mem_body.clone()
                    } else {
                        config::read_memory("USER.md")
                    };
                    let memory = if mem_name == "MEMORY.md" {
                        mem_body.clone()
                    } else {
                        config::read_memory("MEMORY.md")
                    };
                    let corpus = [
                        ("SOUL.md", soul),
                        ("USER.md", user),
                        ("MEMORY.md", memory),
                    ];
                    let refs: Vec<(&str, &str)> =
                        corpus.iter().map(|(n, b)| (*n, b.as_str())).collect();
                    let mut hits = recall_hits(&q_owned, &refs);
                    let mut rows: Vec<(String, String)> = corpus
                        .iter()
                        .map(|(n, b)| ((*n).to_string(), b.clone()))
                        .collect();
                    rows.extend(thread_rows);
                    hits.extend(search_corpus(&q_owned, &rows));
                    hits.sort();
                    hits.dedup();
                    let body = if hits.is_empty() {
                        format!("No recall for {q_owned}")
                    } else {
                        hits.join("\n")
                    };
                    let _ = tx.send(body);
                });
            }
        }
    }

    fn kick_model_retry(&mut self, t: String) {
        self.halt_in_flight();
        self.active_skill_follow = None;
        if let Some(sk) = match_skill(&t, &self.skill_list) {
            if self.policy().injects_skill() {
                self.active_skill_follow = Some(skill_follow_block(sk));
            }
        }
        self.kick_model(true);
    }

    fn policy(&self) -> Policy {
        Policy::max()
    }

    fn job_stored_pairs(
        &self,
        job_thread_id: Option<&str>,
        visible_thread_id: &str,
    ) -> Vec<(String, Vec<(String, String)>)> {
        let Some(id) = job_thread_id else {
            return Vec::new();
        };
        if id == visible_thread_id {
            return Vec::new();
        }
        self.threads
            .iter()
            .find(|t| t.id == id)
            .map(|t| vec![(t.id.clone(), t.messages.as_ref().clone())])
            .unwrap_or_default()
    }

    fn last_user_on_job(&self) -> String {
        let vis = self.visible_thread_id();
        let job = self.chat_job_thread.as_deref();
        let visible = || {
            last_user_scan(
                self.messages
                    .iter()
                    .map(|m| (m.0.as_str(), m.1.as_str())),
            )
        };
        if job.is_none() || job == Some(vis.as_str()) {
            return visible().unwrap_or_default();
        }
        self.threads
            .iter()
            .find(|t| Some(t.id.as_str()) == job)
            .and_then(|t| {
                last_user_scan(t.messages.iter().map(|(r, c)| (r.as_str(), c.as_str())))
            })
            .or_else(visible)
            .unwrap_or_default()
    }

    fn remember_skill(&mut self, skill: SkillMd) {
        if let Some(existing) = self.skill_list.iter_mut().find(|s| s.name == skill.name) {
            *existing = skill;
        } else {
            self.skill_list.push(skill);
            self.skill_list.sort_by(|a, b| a.name.cmp(&b.name));
        }
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
        let written = to_save.clone();
        std::thread::spawn(move || {
            let _ = skills::save_skill(&written);
        });
        self.remember_skill(to_save.clone());
        self.skill_name = to_save.name.clone();
        self.skill_body = grokhub_core::render_skill_md(&to_save);
        self.status = format!("Wrote skill {}", to_save.name);
    }

    fn apply_review_skill_patches(&mut self, raw: &str) {
        let patches = parse_suggest_skill_patches(raw);
        if patches.is_empty() {
            return;
        }
        for p in patches {
            let Some(existing) = self
                .skill_list
                .iter()
                .find(|s| s.name.eq_ignore_ascii_case(&p.name))
                .cloned()
            else {
                continue;
            };
            let proposed = SkillMd {
                name: existing.name.clone(),
                description: existing.description.clone(),
                slash: existing.slash.clone(),
                trigger: p.trigger,
                instructions: p.instructions,
                pitfalls: String::new(),
                verify: String::new(),
                runs: existing.runs,
            };
            let patched = patch_skill(&existing, &proposed);
            if let Some(s) = self.skill_list.iter_mut().find(|s| s.name == patched.name) {
                *s = patched.clone();
            }
            let written = patched;
            std::thread::spawn(move || {
                let _ = skills::save_skill(&written);
            });
        }
    }

    fn append_host_trajectory(&self, ok: bool, block: &str) {
        let line = trajectory_jsonl_line(now_ms(), &self.last_host, ok, block);
        std::thread::spawn(move || {
            let _ = crate::store::append_trajectory(&line);
        });
    }

    fn trim_job_result_dumps(&mut self) {
        let vis = self.visible_thread_id();
        let origin = self
            .chat_job_thread
            .clone()
            .unwrap_or_else(|| vis.clone());
        let here = self.chat_job_thread.as_deref().is_none_or(|id| id == vis);
        let tokens = if here {
            estimate_messages_from(
                self.messages.iter().map(|m| (m.0.as_str(), m.1.as_str())),
            )
        } else {
            self.threads
                .iter()
                .find(|t| t.id == origin)
                .map(|t| estimate_messages(&t.messages))
                .unwrap_or(0)
        };
        if !should_trim_result_bodies(tokens, CONTEXT_BUDGET_TOKENS) {
            return;
        }
        if here {
            trim_result_bodies_in_place(
                self.live_mut()
                    .iter_mut()
                    .map(|m| (m.0.as_str(), &mut m.1)),
                RESULT_TRIM_KEEP_HOPS,
            );
            if let Some(t) = self.threads.iter_mut().find(|t| t.id == origin) {
                t.messages = self.messages.clone();
            }
        } else if let Some(t) = self.threads.iter_mut().find(|t| t.id == origin) {
            trim_result_bodies_in_place(
                t.messages_mut().iter_mut().map(|(r, c)| (r.as_str(), c)),
                RESULT_TRIM_KEEP_HOPS,
            );
        }
    }

    fn queue_sh(&mut self, cmd: String) {
        self.run_cmds(vec![cmd]);
    }

    fn queue_inhabit(&mut self, peer: String) {
        if !inhabit_claim_allowed(&peer) {
            self.status = "will not inhabit onto the phone".into();
            return;
        }
        if self.inhabit_rx.is_some() {
            self.status = "Inhabiting…".into();
            return;
        }
        let target = self.hub.lock().ok().and_then(|st| {
            st.peers
                .iter()
                .find(|p| p.name.eq_ignore_ascii_case(&peer) || p.id.eq_ignore_ascii_case(&peer))
                .cloned()
        });
        let Some(target) = target else {
            self.status = format!("No paired peer named {peer}");
            return;
        };
        if !inhabit_claim_allowed(&target.name) {
            self.status = "will not inhabit onto the phone".into();
            return;
        }
        let peer_count = self
            .hub
            .lock()
            .ok()
            .map(|s| s.peers.len())
            .unwrap_or(0);
        if !inhabit_ready(peer_count, self.running) {
            self.status = "Inhabit needs a paired idle box".into();
            return;
        }
        if !self.scratch() {
            let name = self.mem_name.clone();
            let body = self.mem_body.clone();
            std::thread::spawn(move || {
                if config::read_memory(&name) != body {
                    let _ = config::write_memory(&name, &body);
                }
            });
        }
        let mem_name = self.mem_name.clone();
        let mem_body = self.mem_body.clone();
        let skill_ids = self.skill_list.iter().map(|s| s.name.clone()).collect();
        let goal = self.board.first().map(|c| c.title.clone());
        let from_name = Some(self.cfg.device_name.clone());
        let to_id = Some(target.id.clone());
        let to_name = Some(target.name.clone());
        let at = Some(grokhub_core::now_ms());
        let peer_name = target.name.clone();
        let (tx, rx) = mpsc::channel();
        self.inhabit_rx = Some(rx);
        self.status = format!("Inhabit staging for {peer_name}");
        std::thread::spawn(move || {
            let soul = if mem_name == "SOUL.md" {
                mem_body
            } else {
                config::read_memory("SOUL.md")
            };
            let _ = tx.send(InhabitBundle {
                soul,
                skill_ids,
                goal,
                project_snapshot_id: None,
                from_id: None,
                from_name,
                to_id,
                to_name,
                at,
            });
        });
    }

    fn poll_inhabit(&mut self) {
        let Some(rx) = self.inhabit_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(bundle) => {
                let name = bundle.to_name.clone().unwrap_or_default();
                if let Ok(mut st) = self.hub.lock() {
                    st.inhabit = Some(bundle);
                }
                self.persist_hub();
                self.status = format!("Inhabit staged for {name}");
                self.nav = Nav::Devices;
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.inhabit_rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.status = "Inhabit failed".into();
            }
        }
    }

    fn rewind_project(&mut self) {
        let home = std::env::var("HOME").unwrap_or_default();
        let src = expand_home(self.cfg.project_dir.trim());
        if src.is_empty() {
            self.status = "Bind a project first — /project bind".into();
            return;
        }
        if !rewind_allowed(&src, &home) {
            self.status = "will not rewind $HOME unbound".into();
            return;
        }
        if let Some(last) = self.rewind_rows.first().cloned() {
            if rewind_restore_matches(&expand_home(&last.root), &src)
                && rewind_snapshot_ready(&last.path)
            {
                if let Some(why) = rewind_blocked_reason(self.cfg.host_on, self.running) {
                    self.status = why.into();
                    return;
                }
                self.queue_sh(rewind_copy_cmd(&last.path, &src));
                if self.running {
                    self.status = format!("Restoring {}", last.job_id);
                }
                return;
            }
        }
        if let Some(cmd) = self.snapshot_project() {
            self.queue_sh(cmd);
            if self.running {
                self.status = "No snapshot yet — took one. /rewind again to restore.".into();
            }
        }
    }

    fn snapshot_project(&mut self) -> Option<String> {
        let home = std::env::var("HOME").unwrap_or_default();
        let src = expand_home(self.cfg.project_dir.trim());
        if !rewind_allowed(&src, &home) {
            return None;
        }
        if let Some(why) = rewind_blocked_reason(self.cfg.host_on, self.running) {
            self.status = why.into();
            return None;
        }
        let id = uid("rw");
        let dest = rewind_dest(&config::config_dir().display().to_string(), &id);
        let _ = std::fs::create_dir_all(&dest);
        let cmd = rewind_copy_cmd(&src, &dest);
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
        let rows = self.rewind_rows.clone();
        std::thread::spawn(move || {
            let _ = crate::night::save_rewinds(&rows);
        });
        Some(cmd)
    }

    fn run_grok_extension(&mut self, args: &[&str]) {
        let Some(bin) = grokhub_acp::find_grok() else {
            self.inspect_text = build_agent::grok_banner();
            self.status = self.inspect_text.clone();
            return;
        };
        if self.inspect_rx.is_some() {
            return;
        }
        let cwd = self.grok_cwd();
        let owned: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let (tx, rx) = mpsc::channel();
        self.inspect_rx = Some(rx);
        self.inspect_text = "Inspecting…".into();
        self.status = format!("grok {}", owned.join(" "));
        std::thread::spawn(move || {
            let arg_refs: Vec<&str> = owned.iter().map(|s| s.as_str()).collect();
            let text = match grokhub_acp::grok_stdout(&bin, &cwd, &arg_refs) {
                Ok(t) => {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&t) {
                        serde_json::to_string_pretty(&v).unwrap_or(t)
                    } else {
                        t
                    }
                }
                Err(e) => e,
            };
            let _ = tx.send(text);
        });
    }

    fn doctor_text(&self) -> String {
        let mut lines = grokhub_core::doctor_lines(self.llm_ready(), true, HUB_KIND);
        lines.extend(grokhub_core::doctor_extras(
            self.last_receipt_ok,
            self.skill_list.len(),
        ));
        let (ok, text) = grokhub_acp::doctor_grok_line(grokhub_acp::find_grok().as_deref());
        lines.push(grokhub_core::DoctorLine { ok, text });
        lines
            .into_iter()
            .map(|l| format!("{} {}", if l.ok { "ok" } else { "ERR" }, l.text))
            .collect::<Vec<_>>()
            .join(" · ")
    }

    fn visible_host_receipts(&self) -> Vec<(String, bool)> {
        thread_host_receipts_from(self.messages.iter().map(|m| (m.0.as_str(), m.1.as_str())))
            .into_iter()
            .map(|body| {
                let ok = !crate::update::host_receipt_failed(&body);
                (body, ok)
            })
            .collect()
    }

    fn dream_rewind_id(&self) -> Option<&str> {
        let cur = expand_home(self.cfg.project_dir.trim());
        self.rewind_rows.first().and_then(|r| {
            if rewind_restore_matches(&expand_home(&r.root), &cur) {
                Some(r.job_id.as_str())
            } else {
                None
            }
        })
    }

    fn run_dream(&mut self) {
        if !self.llm_ready() {
            self.status = "Run grok login, or Connect Grok in Settings.".into();
            return;
        }
        if self.running {
            self.status = "Halt the live job before Imagine, or wait.".into();
            return;
        }
        let receipts = self.visible_host_receipts();
        let g = greet_from_last_job(
            if self.cfg.goal_pin.is_empty() {
                None
            } else {
                Some(self.cfg.goal_pin.as_str())
            },
            &receipts,
            self.dream_rewind_id(),
        );
        self.imagine_prompt = g.dream_prompt.clone();
        self.nav = Nav::Imagine;
        self.imagine_want_focus = true;
        self.status = g
            .goal
            .clone()
            .unwrap_or_else(|| "Dream of last night".into());
        self.live_mut().push((
            "assistant".into(),
            format!(
                "{}\n\n{}",
                g.goal.unwrap_or_else(|| "Last night".into()),
                g.dream_prompt
            ),
        ));
        self.stamp_current_access();
        self.persist();
        self.kick_imagine();
    }

    fn dispatch_send(&mut self, task: String) {
        if self.hub_on {
            if let Ok(mut st) = self.hub.lock() {
                if let Err(e) = st.enqueue_local(&task, &task) {
                    self.status = e;
                    return;
                }
            }
            self.persist_hub();
            self.status = "Task queued on hub".into();
            self.nav = Nav::Devices;
            return;
        }
        self.nav = Nav::Chat;
        self.send_chat(task);
    }

    fn sync_hub(&mut self) {
        if self.sync_rx.is_some() {
            self.status = "Syncing…".into();
            return;
        }
        if !self.scratch() {
            let name = self.mem_name.clone();
            let body = self.mem_body.clone();
            std::thread::spawn(move || {
                if config::read_memory(&name) != body {
                    let _ = config::write_memory(&name, &body);
                }
            });
        }
        let mem = ["SOUL.md", "USER.md", "MEMORY.md"]
            .into_iter()
            .map(|n| (n, config::memory_updated_at(n)))
            .collect::<Vec<_>>();
        let mem_name = self.mem_name.clone();
        let mem_body = self.mem_body.clone();
        let mut snap = self.persist_snap();
        if snap.projects.is_some() {
            self.projects_dirty = false;
        }
        self.sync_hub_voice();
        snap.secrets = Some(self.secrets.clone());
        self.last_persist = Instant::now();
        self.geom_dirty = false;
        let skills = self
            .skill_list
            .iter()
            .map(|s| (s.name.clone(), skills::skill_updated_at(&s.name)))
            .collect::<Vec<_>>();
        let autos = self
            .automations
            .iter()
            .filter_map(|a| serde_json::to_value(a).ok())
            .collect::<Vec<_>>();
        let board = serde_json::json!({"items": self.board});
        let device_id = self
            .hub
            .lock()
            .ok()
            .map(|s| s.device_id.clone())
            .unwrap_or_default();
        let device_name = self.cfg.device_name.clone();
        let exported_at = now_ms();
        let hub = self.hub.clone();
        let io = self.persist_io.clone();
        let (tx, rx) = mpsc::channel();
        self.sync_rx = Some(rx);
        self.status = "Syncing…".into();
        std::thread::spawn(move || {
            if let Ok(_g) = io.lock() {
                write_persist_disk(&snap);
            }
            let mem = mem
                .into_iter()
                .map(|(n, at)| HubMemoryFile {
                    name: n.into(),
                    content: if mem_name == n {
                        mem_body.clone()
                    } else {
                        config::read_memory(n)
                    },
                    updated_at: at,
                })
                .collect();
            let threads = snap
                .threads
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "id": t.id,
                        "title": t.title,
                        "updatedAt": t.accessed_ms,
                        "messages": t.messages.iter().map(|(r,c)| serde_json::json!({"role": r, "content": c})).collect::<Vec<_>>(),
                    })
                })
                .collect();
            let skills = skills
                .into_iter()
                .map(|(name, at)| serde_json::json!({"id": name, "name": name, "updatedAt": at}))
                .collect();
            let snap = build_hub_snapshot(
                &device_id,
                &device_name,
                exported_at,
                threads,
                board,
                skills,
                autos,
                mem,
            );
            let remote = hub.lock().ok().and_then(|st| st.snapshot.clone());
            let snap = match remote
                .as_deref()
                .and_then(|v| serde_json::from_value::<HubSnapshot>(v.clone()).ok())
            {
                Some(remote) => merge_hub_snapshots(&snap, &remote),
                None => snap,
            };
            let from = snap.from_device_name.clone();
            let files = snap.memory_files.clone();
            if let Ok(mut st) = hub.lock() {
                st.snapshot = serde_json::to_value(&snap).ok().map(Arc::new);
            }
            let _ = tx.send((from, files));
        });
    }

    fn persist_hub(&self) {
        let hub = self.hub.clone();
        let io = self.persist_io.clone();
        std::thread::spawn(move || {
            if let Ok(_g) = io.lock() {
                let disk = hub.lock().ok().map(|st| state_for_disk(&st));
                if let Some(disk) = disk {
                    let _ = save_hub_state(&config::hub_state_path(), &disk);
                }
            }
        });
    }

    fn persist_cfg(&self) {
        let io = self.persist_io.clone();
        let mut cfg = self.cfg.clone();
        cfg.api_key.clear();
        std::thread::spawn(move || {
            if let Ok(_g) = io.lock() {
                let _ = config::save(&cfg);
            }
        });
    }

    /// Hide/quit must not clone every thread when idle persist already wrote.
    fn persist_if_dirty(&mut self) {
        if !self.projects_dirty && self.persist_idle_key == self.persist_idle_now() {
            self.persist_cfg();
        } else {
            self.persist();
        }
    }

    fn persist_secrets(&self) {
        let io = self.persist_io.clone();
        let secrets = self.secrets.clone();
        std::thread::spawn(move || {
            if let Ok(_g) = io.lock() {
                let _ = secrets::save(&secrets);
            }
        });
    }

    fn persist_usage(&self) {
        let io = self.persist_io.clone();
        let usage = self.usage.clone();
        std::thread::spawn(move || {
            if let Ok(_g) = io.lock() {
                let _ = crate::store::save_usage(&usage);
            }
        });
    }

    fn poll_sync(&mut self) {
        let Some(rx) = self.sync_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok((from, files)) => {
                self.persist_hub();
                self.status = "Hub snapshot written — peers pull /v1/snapshot".into();
                self.apply_inbound_snapshot(from, files);
                self.nav = Nav::Devices;
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.sync_rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.status = "Hub sync failed".into();
            }
        }
    }

    fn date_out(fmt: &str) -> String {
        let mut cmd = std::process::Command::new("date");
        cmd.arg(fmt);
        run_limited(cmd, Duration::from_millis(400))
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default()
    }

    fn local_clock() -> LocalClock {
        if let Ok(g) = LAST_CLOCK.lock() {
            if let Some((at, clock, inflight)) = g.as_ref() {
                let hit = *clock;
                let fresh = at.elapsed() < CLOCK_TTL;
                let busy = *inflight;
                drop(g);
                if !fresh && !busy {
                    Self::kick_local_clock();
                }
                return hit;
            }
        }
        let clock = Self::clock_now();
        if let Ok(mut g) = LAST_CLOCK.lock() {
            *g = Some((Instant::now(), clock, false));
        }
        clock
    }

    fn clock_now() -> LocalClock {
        let out = Self::date_out("+%w %H %M");
        parse_local_clock(&out, now_ms()).unwrap_or(LocalClock {
            now_ms: now_ms(),
            weekday: 1,
            hour: 12,
            minute: 0,
        })
    }

    fn kick_local_clock() {
        if let Ok(mut g) = LAST_CLOCK.lock() {
            if let Some(slot) = g.as_mut() {
                if slot.2 {
                    return;
                }
                slot.2 = true;
            }
        }
        std::thread::spawn(|| {
            let clock = Cabin::clock_now();
            if let Ok(mut g) = LAST_CLOCK.lock() {
                *g = Some((Instant::now(), clock, false));
            }
        });
    }

    fn local_day() -> String {
        if let Ok(g) = LAST_DAY.lock() {
            if let Some((at, day, inflight)) = g.as_ref() {
                let hit = day.clone();
                let fresh = at.elapsed() < CLOCK_TTL;
                let busy = *inflight;
                drop(g);
                if !fresh && !busy {
                    Self::kick_local_day();
                }
                return hit;
            }
        }
        let day = Self::day_now();
        if let Ok(mut g) = LAST_DAY.lock() {
            *g = Some((Instant::now(), day.clone(), false));
        }
        day
    }

    fn day_now() -> String {
        let out = Self::date_out("+%F");
        if out.is_empty() {
            "1970-01-01".into()
        } else {
            out
        }
    }

    fn kick_local_day() {
        if let Ok(mut g) = LAST_DAY.lock() {
            if let Some(slot) = g.as_mut() {
                if slot.2 {
                    return;
                }
                slot.2 = true;
            }
        }
        std::thread::spawn(|| {
            let day = Cabin::day_now();
            if let Ok(mut g) = LAST_DAY.lock() {
                *g = Some((Instant::now(), day, false));
            }
        });
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
                        self.persist_bg();
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
                        && !self.scratch()
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
        if self.scratch() {
            return;
        }
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
        self.roll_today();
        if daily_units_blocked(self.usage.automation, self.cfg.daily_auto_cap) {
            return;
        }
        if !anticipate_consumes_slot(self.can_agent()) {
            return;
        }
        self.last_anticipate_ms = now_ms();
        bump_usage(&mut self.usage, "automation");
        self.daily_auto_used = self.usage.automation;
        self.daily_auto_day = self.usage.day.clone();
        self.send_chat(prompt);
    }

    fn tick_night(&mut self) -> bool {
        if self.running || self.last_night_tick.elapsed() < Duration::from_secs(5) {
            return self.running || self.night_check_rx.is_some();
        }
        self.last_night_tick = Instant::now();
        let clock = Self::local_clock();
        self.roll_today();
        self.daily_auto_day = self.usage.day.clone();
        self.daily_auto_used = self.usage.automation;
        if daily_units_blocked(self.usage.automation, self.cfg.daily_auto_cap) {
            return false;
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
                    self.mark_auto_skipped(&id, now_ms);
                    self.status = format!("Night skipped {name} (check)");
                } else if let Some(a) = self.automations.iter().find(|x| x.id == id).cloned() {
                    if night_check_may_fire(self.running) {
                        self.fire_night(a, now_ms);
                    }
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
        if let Some(why) = forbidden_reason(&cmd) {
            self.mark_auto_skipped(&id, now_ms());
            self.status = format!("Night check blocked: {why}");
            return;
        }
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
        let clock = Self::local_clock();
        let quiet = quiet_hours_active(&clock.hm(), &self.cfg.quiet_start, &self.cfg.quiet_end);
        let destructive = a.instructions.to_ascii_lowercase().contains("rm ")
            || host_risk(&a.instructions) == HostRisk::Destructive;
        if automation_blocked_by_policy(quiet, destructive, 3) {
            self.mark_auto_skipped(&a.id, now_ms);
            self.status = format!("Night skipped {} (quiet/policy)", a.name);
            return;
        }
        if night_unauth_should_skip(self.llm_ready()) {
            self.mark_auto_skipped(&a.id, now_ms);
            self.status = "Connect Grok OAuth in Settings".into();
            return;
        }
        let replay = replay_automation_target(&a.instructions).map(|id| self.replay_saved_recipe(id));
        if !night_counts_run(replay) {
            self.mark_auto_skipped(&a.id, now_ms);
            return;
        }
        if replay.is_none() && !self.can_agent() {
            self.mark_auto_skipped(&a.id, now_ms);
            self.status = "Install Grok Build (x.ai/cli) or Connect Grok in Settings".into();
            return;
        }
        self.mark_auto_ran(&a.id, now_ms);
        bump_usage(&mut self.usage, "automation");
        self.daily_auto_used = self.usage.automation;
        self.daily_auto_day = self.usage.day.clone();
        self.persist_usage();
        self.status = format!("Night: {}", a.name);
        if replay.is_some() {
            return;
        }
        self.land_on_real_chat();
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
        if !self.llm_ready() {
            return;
        }
        self.spawn_review();
    }

    fn review_digest(&self) -> String {
        let (thread_lines, host_receipts) = self.review_chat_digest();
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
            trajectory: summarize_trajectory(
                &parse_trajectory_jsonl(&crate::store::read_trajectory()),
                yesterday_ms(now_ms()),
                12,
            ),
        };
        build_review_digest(&input)
    }

    fn review_chat_digest(&self) -> (Vec<DigestLine>, Vec<String>) {
        let current = self
            .threads
            .get(self.thread_idx)
            .map(|t| t.id.as_str())
            .unwrap_or("");
        let mut thread_lines = Vec::new();
        for m in self.messages.iter().rev() {
            if let Some(line) = digest_line_from(&m.0, &m.1) {
                thread_lines.push(line);
                if thread_lines.len() >= 24 {
                    break;
                }
            }
        }
        for t in self.threads.iter().rev() {
            if t.id == current {
                continue;
            }
            for (role, text) in t.messages.iter().rev() {
                if let Some(line) = digest_line_from(role, text) {
                    thread_lines.push(line);
                    if thread_lines.len() >= 40 {
                        break;
                    }
                }
            }
            if thread_lines.len() >= 40 {
                break;
            }
        }
        thread_lines.reverse();
        let mut host_receipts = thread_host_receipts_from(
            self.messages.iter().map(|m| (m.0.as_str(), m.1.as_str())),
        );
        for t in self.threads.iter().rev() {
            if t.id == current {
                continue;
            }
            host_receipts.extend(thread_host_receipts(&t.messages));
        }
        if host_receipts.len() > 6 {
            host_receipts = host_receipts.split_off(host_receipts.len() - 6);
        }
        (thread_lines, host_receipts)
    }

    fn spawn_review(&mut self) {
        if self.review_busy {
            return;
        }
        let key = self.bearer();
        if key.trim().is_empty() {
            return;
        }
        let mem_name = self.mem_name.clone();
        let mem_body = self.mem_body.clone();
        let (thread_lines, host_receipts) = self.review_chat_digest();
        let insight = insight_pin(&self.learning);
        let skill_names: Vec<String> = self.skill_list.iter().map(|s| s.name.clone()).collect();
        let automation_names: Vec<String> =
            self.automations.iter().map(|a| a.name.clone()).collect();
        let github_pat = !self.secrets.github_token.trim().is_empty();
        let chip_habits = top_habit_labels(&self.chip_memory, 6);
        let now = now_ms();
        let model = model_for_mode("balanced").to_string();
        let prompt = review_system_prompt().to_string();
        let (tx, rx) = mpsc::channel();
        self.review_rx = Some(rx);
        self.review_busy = true;
        std::thread::spawn(move || {
            if config::read_memory(&mem_name) != mem_body {
                let _ = config::write_memory(&mem_name, &mem_body);
            }
            let digest = build_review_digest(&ReviewDigest {
                insight_pin: insight,
                user_md: config::read_memory("USER.md"),
                memory_md: config::read_memory("MEMORY.md"),
                skill_names,
                automation_names,
                github_pat,
                host_receipts,
                chip_habits,
                thread_lines,
                trajectory: summarize_trajectory(
                    &parse_trajectory_jsonl(&crate::store::read_trajectory()),
                    yesterday_ms(now),
                    12,
                ),
            });
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
                self.apply_review_skill_patches(&text);
                let skill_names: Vec<String> =
                    self.skill_list.iter().map(|s| s.name.clone()).collect();
                let auto_names: Vec<String> =
                    self.automations.iter().map(|a| a.name.clone()).collect();
                let live_tools: Vec<String> =
                    CABIN_GITHUB_TOOLS.iter().map(|t| (*t).to_string()).collect();
                let items = dedupe_suggestions(
                    parse_suggest_lines(&text),
                    &skill_names,
                    &auto_names,
                    &live_tools,
                );
                let day = Some(Self::local_day());
                let ms = now_ms();
                if items.is_empty() {
                    self.suggestions.last_review_day = day;
                    self.suggestions.last_review_ms = ms;
                } else {
                    let mut incoming = partition_suggestions(items);
                    incoming.last_review_day = day;
                    incoming.last_review_ms = ms;
                    self.suggestions = merge_suggestion_store(&self.suggestions, incoming);
                }
                prune_live_suggestions(&mut self.suggestions, &live_tools);
                let suggestions = self.suggestions.clone();
                std::thread::spawn(move || {
                    let _ = crate::store::save_suggestions(&suggestions);
                });
            }
            Err(e) => {
                self.status = format!("Nightly review held — {e}");
                self.suggestions.last_review_day = Some(Self::local_day());
                self.suggestions.last_review_ms = now_ms();
                let suggestions = self.suggestions.clone();
                std::thread::spawn(move || {
                    let _ = crate::store::save_suggestions(&suggestions);
                });
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
                self.status = format!("New cover on the wall — {}", gif.title);
                let wall = self.wall.clone();
                let io = self.persist_io.clone();
                std::thread::spawn(move || {
                    if let Ok(_g) = io.lock() {
                        let _ = crate::store::save_wall(&wall);
                    }
                    for old in evicted {
                        let _ = std::fs::remove_file(&old.path_a);
                        let _ = std::fs::remove_file(&old.path_b);
                    }
                });
            }
            Ok(Err(e)) => {
                self.wall_busy = false;
                self.wall.last_ms = now_ms()
                    .saturating_sub(WALL_GIF_EVERY_MS)
                    .saturating_add(15 * 60 * 1000);
                self.status = format!("Wall cover held — {e}");
                let wall = self.wall.clone();
                let io = self.persist_io.clone();
                std::thread::spawn(move || {
                    if let Ok(_g) = io.lock() {
                        let _ = crate::store::save_wall(&wall);
                    }
                });
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
            self.llm_ready(),
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

    fn poll_import_openclaw(&mut self) {
        let Some(rx) = self.import_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(out) => {
                self.status = out.status;
                if out.open_memory {
                    self.mem_name = out.mem_name;
                    self.mem_body = out.mem_body;
                    self.skill_list = out.skill_list;
                    self.nav = Nav::Memory;
                }
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.import_rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.status = "OpenClaw import failed".into();
            }
        }
    }

    fn import_openclaw(&mut self) {
        if self.import_rx.is_some() {
            self.status = "Importing OpenClaw…".into();
            return;
        }
        let home = std::env::var("HOME").unwrap_or_default();
        let mem_name = self.mem_name.clone();
        let mem_body = self.mem_body.clone();
        let scratch = self.scratch();
        let (tx, rx) = mpsc::channel();
        self.import_rx = Some(rx);
        self.status = "Importing OpenClaw…".into();
        std::thread::spawn(move || {
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
                let _ = tx.send(ImportOpenclawOut {
                    status: "No OpenClaw workspace (~/.openclaw/workspace)".into(),
                    mem_name,
                    mem_body,
                    skill_list: Vec::new(),
                    open_memory: false,
                });
                return;
            };
            if !scratch && config::read_memory(&mem_name) != mem_body {
                let _ = config::write_memory(&mem_name, &mem_body);
            }
            let mut imported = 0u32;
            let mut memory = config::read_memory("MEMORY.md");
            if let Ok(rd) = std::fs::read_dir(&root) {
                for e in rd.flatten() {
                    let name = e.file_name().to_string_lossy().into_owned();
                    if let Ok(body) = read_text_capped(&e.path()) {
                        if let Some((dest, content)) = import_memory_file(&name, &body) {
                            if dest == "MEMORY.md" {
                                memory = merge_imported_memory(&memory, &content, &name);
                                imported += 1;
                            } else if config::read_memory(&dest) != content
                                && config::write_memory(&dest, &content).is_ok()
                            {
                                imported += 1;
                            }
                        }
                    }
                }
            }
            if imported > 0 && config::read_memory("MEMORY.md") != memory {
                let _ = config::write_memory("MEMORY.md", &memory);
            }
            let skills_dir = std::path::PathBuf::from(&root).join("skills");
            if let Ok(rd) = std::fs::read_dir(skills_dir) {
                for e in rd.flatten() {
                    let md = e.path().join("SKILL.md");
                    if let Ok(raw) = read_text_capped(&md) {
                        let parsed = grokhub_core::parse_skill_md(&raw);
                        if !parsed.name.is_empty() && skills::save_skill(&parsed).is_ok() {
                            imported += 1;
                        }
                    }
                }
            }
            let skill_list = skills::list_skills();
            let mem_body = config::read_memory("MEMORY.md");
            let _ = tx.send(ImportOpenclawOut {
                status: format!("Imported {imported} files from {root}"),
                mem_name: "MEMORY.md".into(),
                mem_body,
                skill_list,
                open_memory: true,
            });
        });
    }

    fn run_consult(&mut self, q: String) {
        if !self.llm_ready() {
            self.status = "Run grok login, or Connect Grok in Settings.".into();
            return;
        }
        if self.running {
            self.halt_in_flight();
            self.finish_hub_dispatch("Interrupted by consult", false);
        }
        self.running = true;
        if self.chat_job_thread.is_none() {
            self.chat_job_thread = Some(self.visible_thread_id());
        }
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
            "nav:command" => {
                self.open_recent_chat();
                self.nav = Nav::Chat;
            },
            "nav:agents" => self.nav = Nav::Agents,
            "nav:eyes" => {
                self.open_recent_chat();
                self.nav = Nav::Chat;
            }
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

    fn apply_inbound_snapshot(&mut self, from: String, files: Vec<HubMemoryFile>) {
        for f in files {
            if import_memory_file(&f.name, &f.content).is_some() {
                let name = f.name.clone();
                if let Some(i) = Self::mem_file_idx(&name) {
                    self.mem_cache_at[i] = config::memory_updated_at(&name);
                    self.mem_cache_body[i] = f.content.clone();
                }
                if self.mem_name == f.name {
                    self.mem_body = f.content.clone();
                }
                std::thread::spawn(move || {
                    if config::read_memory(&name) != f.content {
                        let _ = config::write_memory(&name, &f.content);
                    }
                });
            }
        }
        self.status = format!("Merged hub snapshot from {from}");
    }

    fn remember_last_frame(&mut self, url: &str) {
        if url.len() > FRAME_CAP {
            return;
        }
        self.last_frame_url = Some(url.to_string());
    }

    /// Decode the JPEG off the hub lock so persist/drain are not frozen on a 400KB clone.
    fn store_hub_frame(&self, url: &str) {
        let Some(frame) = grokhub_core::store_frame(url, now_ms()) else {
            return;
        };
        if let Ok(mut st) = self.hub.lock() {
            st.install_frame(frame);
        }
    }

    fn push_presence(&mut self, url: String) {
        if url.len() > FRAME_CAP {
            return;
        }
        let now = now_ms();
        self.presence_ring.push((now, url));
        self.presence_ring
            .retain(|(ts, _)| should_keep_frame(*ts, now, PRESENCE_RING_MS));
        const PRESENCE_RING_MAX: usize = 32;
        if self.presence_ring.len() > PRESENCE_RING_MAX {
            let drop_n = self.presence_ring.len() - PRESENCE_RING_MAX;
            self.presence_ring.drain(..drop_n);
        }
    }

    fn live_room(&mut self) {
        if self.last_live.elapsed() < Duration::from_millis(900) {
            return;
        }
        self.last_live = Instant::now();
        if !presence_should_stream(false, false) {
            return;
        }
        let rows = collect_rows();
        self.last_window_title = rows
            .iter()
            .map(|r| r.name.as_str())
            .find(|n| !n.is_empty() && *n != "cursor")
            .unwrap_or("")
            .to_string();
        let lock = lock_titles();
        if lock_blocks_hands(&lock.iter().map(|s| s.as_str()).collect::<Vec<_>>()) {
            return;
        }
        if let Some(rx) = self.live_cap_rx.take() {
            match rx.try_recv() {
                Ok(cap) => {
                    if let Some(url) = cap.url {
                        if should_send_screenshot(&self.last_window_title, "") {
                            self.store_hub_frame(&url);
                            self.remember_last_frame(&url);
                            self.push_presence(url);
                        }
                    }
                }
                Err(mpsc::TryRecvError::Empty) => {
                    self.live_cap_rx = Some(rx);
                }
                Err(mpsc::TryRecvError::Disconnected) => {}
            }
        }
        if self.live_cap_rx.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.live_cap_rx = Some(rx);
        std::thread::spawn(move || {
            let url = capture_data_url().ok();
            let _ = capture_webcam();
            let _ = tx.send(LiveCap { url });
        });
    }

    fn tick_mid_thought(&mut self) {
        self.continue_hint = threads::continue_thread_hint(&self.threads);
    }

    fn last_night_hint(&self) -> String {
        let receipts = self.visible_host_receipts();
        let rewind = self.dream_rewind_id();
        if receipts.is_empty() && rewind.is_none() {
            return self.continue_hint.chars().take(80).collect();
        }
        let g = greet_from_last_job(
            if self.cfg.goal_pin.is_empty() {
                None
            } else {
                Some(self.cfg.goal_pin.as_str())
            },
            &receipts,
            rewind,
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
        let list = self.automations.clone();
        std::thread::spawn(move || {
            let _ = crate::night::save(&list);
        });
    }

    fn mark_auto_skipped(&mut self, id: &str, now: u64) {
        let clock = Self::local_clock();
        if let Some(a) = self.automations.iter_mut().find(|x| x.id == id) {
            *a = mark_automation_skipped(a.clone(), now, clock);
        }
        let list = self.automations.clone();
        std::thread::spawn(move || {
            let _ = crate::night::save(&list);
        });
    }

    fn start_oauth(&mut self) {
        if self.oauth_start_rx.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.oauth_start_rx = Some(rx);
        self.status = "Starting Grok OAuth…".into();
        std::thread::spawn(move || {
            let _ = tx.send(crate::oauth::start_device());
        });
    }

    fn poll_oauth(&mut self) {
        if let Some(rx) = self.oauth_start_rx.take() {
            match rx.try_recv() {
                Ok(Ok(start)) => {
                    let uri = start
                        .verification_uri_complete
                        .clone()
                        .unwrap_or_else(|| start.verification_uri.clone());
                    let _ = crate::oauth::open_browser(&uri);
                    self.status =
                        format!("Grok OAuth code {} — approve in the browser", start.user_code);
                    let wait = start.interval.max(1);
                    self.oauth_pending = Some(start);
                    self.oauth_next_poll = Instant::now() + Duration::from_secs(wait);
                }
                Ok(Err(e)) => self.status = e,
                Err(mpsc::TryRecvError::Empty) => {
                    self.oauth_start_rx = Some(rx);
                    return;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.status = "Grok OAuth failed to start".into();
                    return;
                }
            }
        }
        if let Some(rx) = self.oauth_poll_rx.take() {
            match rx.try_recv() {
                Ok(Ok(r)) => match r.status {
                    grokhub_core::PollStatus::Ready => {
                        if let Some(t) = r.tokens {
                            self.secrets.oauth = Some(t);
                            let io = self.persist_io.clone();
                            let secrets = self.secrets.clone();
                            std::thread::spawn(move || {
                                if let Ok(_g) = io.lock() {
                                    let _ = secrets::save(&secrets);
                                }
                            });
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
                    status @ (grokhub_core::PollStatus::Pending | grokhub_core::PollStatus::SlowDown) => {
                        if let Some(p) = self.oauth_pending.as_mut() {
                            if let Some(wait) = grokhub_core::next_oauth_poll_secs(p.interval, status)
                            {
                                p.interval = wait;
                                self.oauth_next_poll = Instant::now() + Duration::from_secs(wait);
                            }
                        }
                    }
                },
                Ok(Err(e)) => self.status = e,
                Err(mpsc::TryRecvError::Empty) => {
                    self.oauth_poll_rx = Some(rx);
                    return;
                }
                Err(mpsc::TryRecvError::Disconnected) => return,
            }
            return;
        }
        let Some(p) = self.oauth_pending.clone() else {
            return;
        };
        if Instant::now() < self.oauth_next_poll {
            return;
        }
        let code = p.device_code.clone();
        let (tx, rx) = mpsc::channel();
        self.oauth_poll_rx = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(crate::oauth::poll_device(&code));
        });
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
        self.oauth_pending = None;
        self.oauth_start_rx = None;
        self.oauth_poll_rx = None;
        self.clear_oauth_photo();
        let io = self.persist_io.clone();
        let secrets = self.secrets.clone();
        std::thread::spawn(move || {
            if let Ok(_g) = io.lock() {
                let _ = secrets::save(&secrets);
            }
        });
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
                            let io = self.persist_io.clone();
                            let secrets = self.secrets.clone();
                            std::thread::spawn(move || {
                                if let Ok(_g) = io.lock() {
                                    let _ = secrets::save(&secrets);
                                }
                            });
                        }
                    }
                    self.oauth_photo_key = out.url;
                    self.oauth_photo = out.image.map(|img| {
                        ctx.load_texture("oauth-avatar", img, TextureOptions::LINEAR)
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
            let image = bytes.as_ref().and_then(|b| oauth_photo_image(b));
            let _ = tx.send(OauthPhotoOut {
                tokens: Some(tokens),
                url,
                image,
            });
        });
    }

    fn kick_model(&mut self, consume_attach: bool) {
        if !self.can_agent() {
            self.status = "Install Grok Build (x.ai/cli) or Connect Grok in Settings".into();
            return;
        }
        if !self.kick_skip
            && self.kick_frame.is_none()
            && (self.kick_cap_rx.is_some()
                || should_capture_before_chat(self.eyes_attach || self.hands_attach))
        {
            match self.poll_cabin_frame() {
                CabinFrame::Pending => {
                    self.pending_kick = Some(consume_attach);
                    if self.chat_job_thread.is_none() {
                        self.chat_job_thread = Some(self.visible_thread_id());
                    }
                    self.running = true;
                    self.status = "Capturing…".into();
                    return;
                }
                CabinFrame::Ready(url) => {
                    self.kick_frame = Some(url);
                }
                CabinFrame::Skip => {}
            }
        }
        if self.verify_rx.is_some() {
            self.pending_kick = Some(consume_attach);
            if self.chat_job_thread.is_none() {
                self.chat_job_thread = Some(self.visible_thread_id());
            }
            self.running = true;
            self.status = "Verifying…".into();
            return;
        }
        self.running = true;
        self.status = "Thinking…".into();
        if self.chat_job_thread.is_none() {
            self.chat_job_thread = Some(self.visible_thread_id());
        }
        let vis = self.visible_thread_id();
        let last_user = {
            let job = self.chat_job_thread.as_deref();
            if job.is_none() || job == Some(vis.as_str()) {
                self.messages
                    .iter()
                    .rev()
                    .find(|m| m.0 == "user" && !is_workload_user(&m.1))
                    .map(|m| m.1.clone())
                    .unwrap_or_default()
            } else {
                self.threads
                    .iter()
                    .find(|t| Some(t.id.as_str()) == job)
                    .and_then(|t| {
                        t.messages
                            .iter()
                            .rev()
                            .find(|(role, content)| role == "user" && !is_workload_user(content))
                            .map(|(_, content)| content.clone())
                    })
                    .unwrap_or_else(|| {
                        self.messages
                            .iter()
                            .rev()
                            .find(|m| m.0 == "user" && !is_workload_user(&m.1))
                            .map(|m| m.1.clone())
                            .unwrap_or_default()
                    })
            }
        };
        if let Err(e) = self.ensure_acp() {
            self.running = false;
            self.status = self.apply_job_fail(&e);
            self.chat_job_thread = None;
            self.persist();
            return;
        }
        if self.acp_spawn_rx.is_some() {
            self.pending_kick = Some(consume_attach);
            return;
        }
        let cabin = self.kick_frame.take();
        self.kick_skip = false;
        self.eyes_attach = false;
        self.hands_attach = false;
        self.stream_buf.clear();
        self.thought_buf.clear();
        self.tool_cards.clear();
        if let Some(p) = self.perm_ask.take() {
            if let Some(h) = &self.acp {
                let _ = h.answer_permission(p.rpc_id, false);
            }
        }
        self.perm_ask = None;
        let image = if consume_attach {
            next_chat_image(self.attach_url.as_deref(), cabin.as_deref()).map(|s| s.to_string())
        } else {
            None
        };
        match self
            .acp
            .as_ref()
            .map(|h| h.prompt_with_image(&last_user, image.as_deref()))
        {
            Some(Ok(())) => {
                if consume_attach {
                    self.attach_url = None;
                    self.attach_name = None;
                }
            }
            Some(Err(e)) => {
                self.running = false;
                self.chat_job_thread = None;
                self.status = e;
            }
            None => {
                self.running = false;
                self.status = "Grok Build session missing".into();
            }
        }
    }

    fn upsert_stream_assistant(&mut self) {
        self.apply_live_assistant();
    }

    fn poll_acp(&mut self) {
        let evs = {
            let Some(h) = &self.acp else { return };
            let mut v = Vec::new();
            while let Ok(ev) = h.try_recv() {
                v.push(ev);
            }
            v
        };
        let auto_allow = self.permission_mode.auto_allows();
        for ev in evs {
            match ev {
                AcpEvent::Ready { session_id } => {
                    if !session_id.trim().is_empty() {
                        let job = self.chat_job_thread.clone();
                        let idx = job
                            .as_deref()
                            .and_then(|id| self.threads.iter().position(|t| t.id == id))
                            .unwrap_or(self.thread_idx);
                        let acp_cwd = self.acp.as_ref().map(|h| h.cwd.display().to_string());
                        if let Some(t) = self.threads.get_mut(idx) {
                            t.grok_session = Some(session_id);
                            if let Some(cwd) = acp_cwd {
                                t.grok_cwd = Some(cwd);
                            }
                        }
                    }
                }
                AcpEvent::Thought(t) => {
                    if !self.running {
                        continue;
                    }
                    let changed = push_stream_capped(&mut self.thought_buf, &t, IMAGE_FILE_CAP);
                    if chat_stream_is_visible(
                        self.chat_job_thread.as_deref(),
                        &self.visible_thread_id(),
                    ) {
                        self.status = "Thinking…".into();
                    }
                    if changed {
                        self.upsert_stream_assistant();
                    }
                }
                AcpEvent::Text(t) => {
                    if !self.running {
                        continue;
                    }
                    let changed = push_stream_capped(&mut self.stream_buf, &t, IMAGE_FILE_CAP);
                    if chat_stream_is_visible(
                        self.chat_job_thread.as_deref(),
                        &self.visible_thread_id(),
                    ) {
                        self.status = "Thinking…".into();
                    }
                    if changed {
                        self.upsert_stream_assistant();
                    }
                }
                AcpEvent::Tool(card) => {
                    if let Some(url) = &card.image_data_url {
                        self.desk_frame = Some(url.clone());
                        self.remember_last_frame(url);
                        self.store_hub_frame(url);
                    }
                    if let Some(old) = self.tool_cards.iter_mut().find(|c| c.id == card.id) {
                        *old = merge_tool_card(old.clone(), card);
                    } else {
                        self.tool_cards.push(card);
                    }
                }
                AcpEvent::Plan(t) => {
                    self.status = format!("Plan · {t}");
                }
                AcpEvent::Permission(p) => {
                    if !self.running {
                        if let Some(h) = &self.acp {
                            let _ = h.answer_permission(p.rpc_id, false);
                        }
                        continue;
                    }
                    if self.permission_mode == PermissionMode::AlwaysApprove {
                        if let Some(h) = &self.acp {
                            let _ = h.answer_permission_always(p.rpc_id);
                        }
                    } else if auto_allow {
                        if let Some(h) = &self.acp {
                            let _ = h.answer_permission(p.rpc_id, true);
                        }
                    } else {
                        if let Some(old) = self.perm_ask.take() {
                            if let Some(h) = &self.acp {
                                let _ = h.answer_permission(old.rpc_id, false);
                            }
                        }
                        self.perm_ask = Some(p);
                        self.status = "Grok wants permission".into();
                    }
                }
                AcpEvent::Done { stop_reason } => {
                    if stop_reason.eq_ignore_ascii_case("cancelled") || !self.running {
                        continue;
                    }
                    let text = if self.thought_buf.is_empty() {
                        self.stream_buf.clone()
                    } else {
                        merge_thinking_capped(
                            &self.thought_buf,
                            &strip_thinking(&self.stream_buf),
                            IMAGE_FILE_CAP as usize,
                        )
                    };
                    self.finish_acp_turn(text);
                }
                AcpEvent::Err(e) => {
                    if let Some(p) = self.perm_ask.take() {
                        if let Some(h) = &self.acp {
                            let _ = h.answer_permission(p.rpc_id, false);
                        }
                    }
                    self.running = false;
                    let e = grokhub_acp::explain_handshake_error(&e, &self.grok_cwd());
                    self.status = self.apply_job_fail(&e);
                    self.chat_job_thread = None;
                    self.persist();
                }
            }
        }
    }

    fn finish_acp_turn(&mut self, text: String) {
        let text = take_ui_text(text, IMAGE_FILE_CAP);
        if let Some(p) = self.perm_ask.take() {
            if let Some(h) = &self.acp {
                let _ = h.answer_permission(p.rpc_id, false);
            }
        }
        self.perm_ask = None;
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
        bump_usage(&mut self.usage, "message");
        self.apply_assistant_snapshot(text.clone());
        self.thought_buf.clear();
        self.stream_buf.clear();
        let origin = self.chat_job_thread.take();
        if here && self.speak_next {
            self.speak_next = false;
            self.speak_reply(&text);
        }
        if let Some(p) = extract_imagine_prompt(&text) {
            self.chat_job_thread = origin.clone();
            self.imagine_prompt = p;
            if here {
                self.nav = Nav::Imagine;
                self.imagine_want_focus = true;
            }
            self.kick_imagine();
        }
        self.persist();
        if !self.running {
            self.finish_hub_dispatch(&text, hub_dispatch_ok(&text));
        }
        let vis = self.visible_thread_id();
        if leftover_empty_thread(
            self.threads
                .get(self.thread_idx)
                .map(|t| t.title.as_str())
                .unwrap_or(""),
            self.scratch(),
            self.messages.is_empty(),
        ) {
            let _ = vis;
        }
        let _ = origin;
    }

    fn poll_host_diff(&mut self) {
        let Some(rx) = self.host_diff_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(Some(msg)) => {
                self.push_bound_msg("user", msg);
                self.persist();
                self.finish_host_diff_kick();
            }
            Ok(None) => self.finish_host_diff_kick(),
            Err(mpsc::TryRecvError::Empty) => {
                self.host_diff_rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => self.finish_host_diff_kick(),
        }
    }

    fn finish_host_diff_kick(&mut self) {
        if !self.host_diff_kick {
            return;
        }
        self.host_diff_kick = false;
        if !self.pending_connectors.is_empty() {
            let origin = self.chat_job_thread.clone();
            let (id, tool, args) = self.pending_connectors.remove(0);
            self.run_connector(&id, &tool, &args);
            if self.chat_job_thread.is_none() {
                self.chat_job_thread = origin;
            }
        } else {
            self.kick_model(false);
        }
    }

    fn poll_job(&mut self) {
        let Some(rx) = self.rx.take() else { return };
        match rx.try_recv() {
            Ok(JobOut::ChatDelta(d)) => {
                self.rx = Some(rx);
                let changed = push_stream_capped(&mut self.stream_buf, &d, IMAGE_FILE_CAP);
                if chat_stream_is_visible(
                    self.chat_job_thread.as_deref(),
                    &self.visible_thread_id(),
                ) {
                    self.status = "Thinking…".into();
                }
                if changed {
                    self.upsert_stream_assistant();
                }
            }
            Ok(JobOut::ThoughtDelta(d)) => {
                self.rx = Some(rx);
                let changed = push_stream_capped(&mut self.thought_buf, &d, IMAGE_FILE_CAP);
                if chat_stream_is_visible(
                    self.chat_job_thread.as_deref(),
                    &self.visible_thread_id(),
                ) {
                    self.status = "Thinking…".into();
                }
                if changed {
                    self.upsert_stream_assistant();
                }
            }
            Ok(JobOut::Chat { text, truncated }) => {
                let text = take_ui_text(text, IMAGE_FILE_CAP);
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
                bump_usage(&mut self.usage, "message");
                let finished = if self.thought_buf.is_empty() {
                    text
                } else {
                    merge_thinking_capped(
                        &self.thought_buf,
                        &strip_thinking(&text),
                        IMAGE_FILE_CAP as usize,
                    )
                };
                let streamed = if self.thought_buf.is_empty() {
                    self.stream_buf.clone()
                } else {
                    merge_thinking_capped(
                        &self.thought_buf,
                        &strip_thinking(&self.stream_buf),
                        IMAGE_FILE_CAP as usize,
                    )
                };
                let text = prefer_complete_reply(&streamed, &finished);
                self.apply_assistant_snapshot(text.clone());
                self.thought_buf.clear();
                self.stream_buf.clear();
                let job = self.chat_job_thread.clone();
                let vis = self.visible_thread_id();
                let last_user = self.last_user_on_job();
                let facts = if job.as_deref().is_none_or(|id| id == vis) {
                    fact_candidates_from(
                        self.messages.iter().map(|m| (m.0.as_str(), m.1.as_str())),
                    )
                } else {
                    self.threads
                        .iter()
                        .find(|t| Some(t.id.as_str()) == job.as_deref())
                        .map(|t| fact_candidates(&t.messages))
                        .unwrap_or_else(|| {
                            fact_candidates_from(
                                self.messages.iter().map(|m| (m.0.as_str(), m.1.as_str())),
                            )
                        })
                };
                let tokens = if job.as_deref().is_none_or(|id| id == vis) {
                    estimate_messages_from(
                        self.messages.iter().map(|m| (m.0.as_str(), m.1.as_str())),
                    )
                } else {
                    self.threads
                        .iter()
                        .find(|t| Some(t.id.as_str()) == job.as_deref())
                        .map(|t| estimate_messages(&t.messages))
                        .unwrap_or(0)
                };
                let stored_scratch: Vec<(String, bool)> = self
                    .threads
                    .iter()
                    .map(|t| (t.id.clone(), t.scratch))
                    .collect();
                let job_scratch = job_is_scratch(
                    job.as_deref(),
                    &vis,
                    self.scratch(),
                    &stored_scratch,
                );
                if self.policy().learns() && !job_scratch {
                    extract_insights(&mut self.learning, &facts);
                }
                let origin = self.chat_job_thread.take();
                if here && self.speak_next {
                    self.speak_next = false;
                    self.speak_reply(&text);
                }
                let scan = bound_scan(&text);
                for card in extract_work_pins(&scan) {
                    self.board.push(card);
                }
                if let Some(r) = parse_recipe(&scan) {
                    self.last_recipe = Some(r);
                }
                if has_verify_ok(&scan) {
                    self.verify_ok_turn = true;
                    self.verify_chip = "VERIFY_OK".into();
                }
                for (key, st) in extract_work_updates(&scan) {
                    let _ = apply_work_update(&mut self.board, &key, st);
                }
                self.persist();
                let mut host_needs_kick = false;
                // Grok Build owns bash, files, and computer-use. Do not parse HOST_CMD / COMPUTER_CMD.
                if let Some(p) = extract_imagine_prompt(&scan) {
                    self.chat_job_thread = origin.clone();
                    self.imagine_prompt = p;
                    if here {
                        self.nav = Nav::Imagine;
                        self.imagine_want_focus = true;
                    }
                    self.kick_imagine();
                }
                if user_asked_to_schedule(&last_user)
                    && chat_may_save_automation(&last_user, &scan)
                {
                    if let Some(mut a) = parse_nl_automation(&scan) {
                        if a.id.is_empty() {
                            a.id = uid("auto");
                        }
                        self.automations.push(a.clone());
                        let list = self.automations.clone();
                        std::thread::spawn(move || {
                            let _ = crate::night::save(&list);
                        });
                        self.persist_idle_key = self.persist_idle_now();
                        if here {
                            self.status = format!("Night saved: {} {}", a.schedule, a.time);
                        }
                    }
                }
                if let Some(q) = parse_consult(&scan) {
                    if !self.running {
                        self.finish_hub_dispatch(&text, hub_dispatch_ok(&text));
                        self.chat_job_thread = origin.clone();
                        self.run_consult(q);
                    }
                }
                let outcome = parse_goal_outcome(&text);
                let stored_pins: Vec<(String, String)> = self
                    .threads
                    .iter()
                    .map(|t| (t.id.clone(), t.goal.label.clone()))
                    .collect();
                let pin = goal_continue_pin(
                    &goal_pin_for_job(
                        job.as_deref(),
                        &vis,
                        &self.cfg.goal_pin,
                        &stored_pins,
                    ),
                    &last_user,
                );
                let job_step = if job.as_deref() == Some(vis.as_str()) || job.is_none() {
                    self.goal_step
                } else {
                    self.threads
                        .iter()
                        .find(|t| Some(t.id.as_str()) == job.as_deref())
                        .map(|t| t.goal.step)
                        .unwrap_or(0)
                };
                if should_auto_continue_goal(
                    outcome,
                    &pin,
                    self.running,
                    job_step,
                    GOAL_MAX_STEPS,
                ) {
                    if let Some(next) = next_goal_prompt(&pin, &text, job_step, GOAL_MAX_STEPS) {
                        let next_step = job_step.saturating_add(1);
                        if let Some(id) = job.as_deref() {
                            if let Some(t) = self.threads.iter_mut().find(|t| t.id == id) {
                                t.goal.step = next_step;
                            }
                        }
                        self.goal_step = visible_goal_step_on_continue(self.goal_step, job_step, here);
                        self.agents.push(AgentJob {
                            title: format!("{} · step {}", pin, next_step + 1),
                            status: "running".into(),
                            prompt: next.clone(),
                            thread_id: origin.clone().unwrap_or_else(|| vis.clone()),
                        });
                        self.chat_job_thread = origin.clone();
                        self.push_bound_msg("user", next);
                        self.persist();
                        self.kick_model(false);
                    }
                } else {
                    let next_step = goal_step_after_outcome(job_step, &outcome, true);
                    if let Some(id) = job.as_deref() {
                        if let Some(t) = self.threads.iter_mut().find(|t| t.id == id) {
                            t.goal.step = next_step;
                        }
                    }
                    if here {
                        self.goal_step = next_step;
                    }
                }
                let compact_step = if let Some(id) = job.as_deref() {
                    self.threads
                        .iter()
                        .find(|t| t.id == id)
                        .map(|t| t.goal.step)
                        .unwrap_or(0)
                } else if here {
                    self.goal_step
                } else {
                    job_step
                };
                if should_auto_compact_now(tokens, CONTEXT_BUDGET_TOKENS, compact_step) {
                    if here {
                        self.run_slash(Slash::Compact);
                        self.status = format!(
                            "Auto-compact · {}% context",
                            context_percent(tokens, CONTEXT_BUDGET_TOKENS)
                        );
                    } else if let Some(id) = job.as_deref() {
                        if let Some(t) = self.threads.iter_mut().find(|t| t.id == id) {
                            let pin = t.goal.label.clone();
                            let start = compact_keep_start_from(
                                t.messages.iter().map(|(r, c)| (r.as_str(), c.as_str())),
                                8,
                            );
                            if start > 0 {
                                t.messages_mut().drain(..start);
                            }
                            let pin = pin.trim();
                            if !pin.is_empty() {
                                let marked = format!("GOAL PIN: {pin}");
                                if !t.messages.iter().any(|(_, c)| {
                                    c == &marked || c.starts_with(&format!("{marked}\n"))
                                }) {
                                    t.messages_mut().insert(0, ("system".into(), marked));
                                }
                            }
                            t.accessed_ms = now_ms();
                        }
                        self.persist();
                    }
                }
                if !self.running
                    && self.followup_step < FOLLOWUP_MAX_STEPS
                    && reply_needs_followup(
                        &last_user,
                        &text,
                        truncated,
                    )
                {
                    if self.chat_job_thread.is_none() {
                        self.chat_job_thread = origin.clone();
                    }
                    self.send_followup_turn();
                }
                if host_needs_kick && !self.running {
                    self.kick_model(false);
                }
                if !self.running {
                    self.finish_hub_dispatch(&text, hub_dispatch_ok(&text));
                }
                self.spawn_thread_goal_on(job.as_deref());
            }
            Ok(JobOut::Consult(detail)) => {
                self.running = false;
                self.push_bound_msg("assistant", detail.clone());
                self.status.clear();
                self.chat_job_thread = None;
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
                self.host_reserved = 0;
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
                        .all(|c| parse_computer_op(c).is_some());
                let any_hands = self
                    .last_host
                    .iter()
                    .any(|c| parse_computer_op(c).is_some());
                let prefix = if all_hands {
                    "COMPUTER_RESULT (facts only):"
                } else {
                    "HOST_RESULT (facts only):"
                };
                let vis = self.visible_thread_id();
                let stored_scratch: Vec<(String, bool)> = self
                    .threads
                    .iter()
                    .map(|t| (t.id.clone(), t.scratch))
                    .collect();
                let job_scratch = job_is_scratch(
                    self.chat_job_thread.as_deref(),
                    &vis,
                    self.scratch(),
                    &stored_scratch,
                );
                self.push_bound_msg("user", format!("{prefix}\n{block}"));
                bump_usage(&mut self.usage, "host");
                self.persist();
                if any_hands {
                    self.hands_attach = true;
                    self.eyes_attach = true;
                    if let Some(url) = self.capture_cabin_frame_this_turn() {
                        self.kick_frame = Some(url);
                    }
                    let cmds = self.last_host.clone();
                    std::thread::spawn(move || {
                        let rows = collect_rows();
                        let _ = lock_titles();
                        if let Some(recipe) = recipe_from_cmds(&cmds, screen_from_rows(&rows)) {
                            let _ = crate::recipes::save_recipe("last", &recipe);
                        }
                    });
                    if let Some(recipe) = recipe_from_cmds(&self.last_host, None) {
                        self.last_recipe = Some(recipe);
                    }
                    if !job_scratch {
                        let user = self.last_user_on_job();
                        let proposed = propose_skill_from_turn(&user, &block, &self.last_host);
                        self.commit_proposed_skill(proposed);
                    }
                }
                self.plan_pending = retain_held_plan(self.plan_pending.take(), &self.last_host);
                self.run_skill_verify();
                let mut defer_kick = false;
                if let Some(cite) = summarize_write(
                    self.last_host.last().map(|s| s.as_str()).unwrap_or(""),
                    &block,
                ) {
                    if let Some(path) = cite.split_whitespace().last() {
                        let path = resolve_host_cite_path(&self.cfg.project_dir, path);
                        let (tx, rx) = mpsc::channel();
                        self.host_diff_rx = Some(rx);
                        defer_kick = true;
                        std::thread::spawn(move || {
                            let out = match read_text_capped(std::path::Path::new(&path)) {
                                Ok(after) => {
                                    Some(format!("HOST_DIFF:\n{}", unified_diff_cite(&path, "", &after)))
                                }
                                Err(_) => None,
                            };
                            let _ = tx.send(out);
                        });
                    }
                }
                if !any_hands
                    && is_hard_run(self.last_host.len() as u32, !ok, false, job_scratch)
                {
                    let user = self.last_user_on_job();
                    let proposed = propose_skill_from_turn(&user, &block, &self.last_host);
                    self.commit_proposed_skill(proposed);
                }
                self.append_host_trajectory(ok, &block);
                self.trim_job_result_dumps();
                if defer_kick {
                    self.host_diff_kick = true;
                } else if !self.pending_connectors.is_empty() {
                    let origin = self.chat_job_thread.clone();
                    let (id, tool, args) = self.pending_connectors.remove(0);
                    self.run_connector(&id, &tool, &args);
                    if self.chat_job_thread.is_none() {
                        self.chat_job_thread = origin;
                    }
                } else {
                    self.kick_model(false);
                }
                if !self.running {
                    self.finish_hub_dispatch(&block, ok);
                }
            }
            Ok(JobOut::Connector(detail)) => {
                self.running = false;
                self.push_bound_msg(
                    "user",
                    format!("CONNECTOR_RESULT (facts only):\n{detail}"),
                );
                self.persist();
                self.trim_job_result_dumps();
                if !self.pending_connectors.is_empty() {
                    let origin = self.chat_job_thread.clone();
                    let (id, tool, args) = self.pending_connectors.remove(0);
                    self.run_connector(&id, &tool, &args);
                    if self.chat_job_thread.is_none() {
                        self.chat_job_thread = origin;
                    }
                } else {
                    self.kick_model(false);
                    if !self.running {
                        self.finish_hub_dispatch(&detail, true);
                    }
                }
            }
            Ok(JobOut::Imagine(url)) => {
                self.running = false;
                self.imagine_last = url.clone();
                self.status = "Imagine ready".into();
                self.push_bound_msg("assistant", format!("IMAGINE: {url}"));
                self.finish_hub_dispatch(&format!("IMAGINE: {url}"), true);
                self.chat_job_thread = None;
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
                remember_chip_outcome(&mut self.chip_memory, false, now_ms());
                self.status = self.apply_job_fail(&e);
                self.finish_hub_dispatch(&e, false);
                self.chat_job_thread = None;
                self.stream_buf.clear();
                self.thought_buf.clear();
                self.persist();
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.running = false;
                self.status = self.apply_job_fail(worker_gone_status());
                self.finish_hub_dispatch(worker_gone_status(), false);
                self.chat_job_thread = None;
                self.stream_buf.clear();
                self.thought_buf.clear();
                self.persist();
            }
        }
    }

    fn run_cmds(&mut self, cmds: Vec<String>) -> bool {
        if !self.cfg.host_on {
            self.status = "Host off — /host on".into();
            return false;
        }
        if self.running {
            self.status = "Busy — wait, then host".into();
            return false;
        }
        if self.host_hour_at.elapsed() > Duration::from_secs(3600) {
            self.host_hour_count = 0;
            self.host_hour_at = Instant::now();
        }
        let mut gated = Vec::new();
        let mut blocked = false;
        for c in &cmds {
            if host_hour_blocked(self.host_hour_count, self.cfg.host_hour_cap) {
                self.status = format!("Host hour cap {}", self.cfg.host_hour_cap);
                self.push_bound_msg(
                    "user",
                    format!(
                        "HOST_RESULT (facts only):\n$ {c}\nblocked: hour cap {}",
                        self.cfg.host_hour_cap
                    ),
                );
                blocked = true;
                break;
            }
            if let Some(why) = forbidden_reason(c) {
                self.push_bound_msg(
                    "user",
                    format!("HOST_RESULT (facts only):\n$ {c}\nblocked: {why}"),
                );
                blocked = true;
                continue;
            }
            if !c.trim().starts_with("COMPUTER_CMD")
                && !is_rewind_copy_cmd_in(c, &self.cfg.project_dir, std::env::var("HOME").ok().as_deref())
                && host_cmd_leaves_project(c, &self.cfg.project_dir)
                && self.permission_mode != PermissionMode::AlwaysApprove
            {
                self.push_bound_msg(
                    "user",
                    format!("HOST_RESULT (facts only):\n$ {c}\nblocked: outside bound project"),
                );
                blocked = true;
                continue;
            }
            self.host_hour_count = self.host_hour_count.saturating_add(1);
            gated.push(c.clone());
        }
        if gated.is_empty() {
            self.plan_pending = None;
            if blocked {
                self.persist();
            }
            return blocked;
        }
        if blocked {
            self.persist();
        }
        if !gated.iter().any(|c| is_rewind_copy_cmd(c)) {
            if let Some(snap) = self.snapshot_project() {
                gated.insert(0, snap);
            }
        }
        self.last_host = gated.clone();
        self.host_halt = mint_host_halt();
        self.running = true;
        self.host_reserved = gated.len() as u32;
        if self.chat_job_thread.is_none() {
            self.chat_job_thread = Some(self.visible_thread_id());
        }
        self.voice_orb = "hands".into();
        self.host_live = gated[0].clone();
        self.status = "Host…".into();
        self.plan_pending = None;
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let cap = self.cfg.host_hour_cap;
        let clock = Self::local_clock();
        let quiet = quiet_hours_active(&clock.hm(), &self.cfg.quiet_start, &self.cfg.quiet_end);
        let halt = self.host_halt.clone();
        let cwd = host_working_dir(&self.cfg.project_dir);
        std::thread::spawn(move || {
            let started = Instant::now();
            let mut inhibit = crate::notify::inhibit_sleep();
            let mut block = String::new();
            let mut count = 0u32;
            for c in &gated {
                if halt.load(Ordering::SeqCst) {
                    block.push_str("HOST_RECEIPT: halted\n");
                    break;
                }
                if host_hour_blocked(count, cap) {
                    block.push_str("hour cap reached\n");
                    break;
                }
                count = count.saturating_add(1);
                let tx_line = tx.clone();
                let cmd = c.clone();
                let receipt = if let Some(op) = parse_computer_op(c) {
                    let _ = tx_line.send(JobOut::HostLine(format!(
                        "Hands: {}",
                        computer_cmd_line(&op)
                    )));
                    run_computer_op_cancel(&op, Some(&halt))
                } else {
                    run_host_stream(
                        c,
                        Duration::from_secs(90),
                        Some(&halt),
                        cwd.as_deref(),
                        move |line| {
                            let _ = tx_line.send(JobOut::HostLine(host_status_line(&cmd, line, 0)));
                        },
                    )
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
        false
    }

    fn run_connector(&mut self, id: &str, tool: &str, args: &str) {
        if id != "github" {
            self.push_bound_msg(
                "user",
                format!(
                    "CONNECTOR_RESULT (facts only):\n{id} {tool} — not wired. GitHub is the only live connector."
                ),
            );
            self.persist();
            self.kick_model(false);
            return;
        }
        if self.running {
            self.pending_connectors
                .push((id.to_string(), tool.to_string(), args.to_string()));
            return;
        }
        self.running = true;
        if self.chat_job_thread.is_none() {
            self.chat_job_thread = Some(self.visible_thread_id());
        }
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
        let prompt = self.imagine_prompt.trim().to_string();
        if prompt.is_empty() {
            return;
        }
        if self.running {
            self.status = "Halt the live job before Imagine, or wait.".into();
            return;
        }
        let kind = self.imagine_kind;
        let key = self.bearer();
        if key.trim().is_empty() {
            self.status = "Run grok login — Imagine uses that token.".into();
            return;
        }
        let aspect = imagine_aspect_label(self.imagine_aspect).to_string();
        let resolution = imagine_image_resolution(self.imagine_quality).to_string();
        let video_res = imagine_video_resolution(imagine_video_res_label(self.imagine_video_res)).to_string();
        let video_dur = imagine_video_duration_secs(imagine_video_dur_label(self.imagine_video_dur));
        let prompt = compose_imagine_prompt(&ImagineSpec {
            prompt: &prompt,
            kind,
            quality: self.imagine_quality,
            style: imagine_style_label(self.imagine_style),
            aspect: &aspect,
            video_res: &video_res,
            video_dur: imagine_video_dur_label(self.imagine_video_dur),
            video_audio: self.imagine_video_audio,
        });
        self.running = true;
        if self.chat_job_thread.is_none() {
            self.chat_job_thread = Some(self.visible_thread_id());
        }
        self.status = match kind {
            ImagineKind::Image => "Imagining…".into(),
            ImagineKind::Video => "Imagining video…".into(),
            ImagineKind::Agent => "Imagining agent still…".into(),
        };
        let image_model = dedicated_imagine_model(&self.cfg.imagine_model);
        let video_model = dedicated_video_model("");
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        std::thread::spawn(move || {
            let r = match kind {
                ImagineKind::Video => {
                    grok_imagine_video(&key, &video_model, &prompt, video_dur, &aspect, &video_res)
                }
                ImagineKind::Image | ImagineKind::Agent => grok_imagine_opts(
                    &key,
                    &image_model,
                    &prompt,
                    Some(&aspect),
                    Some(&resolution),
                ),
            };
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
        let speech = self.bearer();
        let has_local = first_bin(TRANSCRIBERS).is_some();
        let route = hey_grok_route(
            realtime_can_connect(self.console_key()),
            !speech.is_empty(),
            has_local,
        );
        if self.voice_sock.is_none() {
            match route {
                HeyGrokRoute::Realtime => {
                    if let Some(key) = realtime_bearer(self.console_key(), &oauth) {
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
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(JobOut::Voice(listen_turn(&speech)));
        });
    }

    fn capture_cabin_frame_this_turn(&mut self) -> Option<String> {
        match self.poll_cabin_frame() {
            CabinFrame::Ready(url) => Some(url),
            CabinFrame::Skip | CabinFrame::Pending => None,
        }
    }

    fn poll_cabin_frame(&mut self) -> CabinFrame {
        if let Some(rx) = self.kick_cap_rx.take() {
            return match rx.try_recv() {
                Ok(Ok(url)) => {
                    self.store_hub_frame(&url);
                    self.remember_last_frame(&url);
                    CabinFrame::Ready(url)
                }
                Ok(Err(e)) => {
                    if self.status.is_empty()
                        || self.status == "Thinking…"
                        || self.status == "Capturing…"
                    {
                        self.status = format!("eyes: {e}");
                    }
                    self.kick_skip = true;
                    CabinFrame::Skip
                }
                Err(mpsc::TryRecvError::Empty) => {
                    self.kick_cap_rx = Some(rx);
                    CabinFrame::Pending
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.kick_skip = true;
                    CabinFrame::Skip
                }
            };
        }
        if !should_capture_before_chat(self.eyes_attach || self.hands_attach) {
            return CabinFrame::Skip;
        }
        let (tx, rx) = mpsc::channel();
        self.kick_cap_rx = Some(rx);
        std::thread::spawn(move || {
            let rows = collect_rows();
            let title = rows
                .iter()
                .map(|r| r.name.as_str())
                .find(|n| !n.is_empty() && *n != "cursor")
                .unwrap_or("")
                .to_string();
            let lock = lock_titles();
            if lock_blocks_hands(&lock.iter().map(|s| s.as_str()).collect::<Vec<_>>())
                || !should_send_screenshot(&title, "")
            {
                let _ = tx.send(Err("skipped lock/password frame".into()));
                return;
            }
            let _ = tx.send(capture_data_url());
        });
        CabinFrame::Pending
    }

    fn poll_pending_kick(&mut self) {
        let Some(consume) = self.pending_kick else {
            return;
        };
        if self.acp_spawn_rx.is_some() {
            return;
        }
        if self.kick_frame.is_some()
            || (self.kick_cap_rx.is_none()
                && !should_capture_before_chat(self.eyes_attach || self.hands_attach))
        {
            self.pending_kick = None;
            self.kick_model(consume);
            return;
        }
        match self.poll_cabin_frame() {
            CabinFrame::Pending => {}
            CabinFrame::Ready(url) => {
                self.kick_frame = Some(url);
                self.pending_kick = None;
                self.kick_model(consume);
            }
            CabinFrame::Skip => {
                self.pending_kick = None;
                self.kick_model(consume);
            }
        }
    }

    fn apply_job_fail(&mut self, err: &str) -> String {
        if !job_error_goes_to_chat(self.chat_job_thread.as_deref()) {
            return err.to_string();
        }
        let vis = self.visible_thread_id();
        let job = self.chat_job_thread.clone();
        if job.as_deref().map(|id| id == vis).unwrap_or(true) {
            let text = format!("Error: {err}");
            {
                let msgs = self.live_mut();
                if msgs.last().is_some_and(|m| m.0 == "assistant") {
                    if let Some(last) = msgs.last_mut() {
                        last.1 = text;
                    }
                } else {
                    msgs.push(("assistant".into(), text));
                }
            }
            self.stamp_current_access();
            return err.to_string();
        }
        if let Some(id) = job {
            if let Some(t) = self.threads.iter_mut().find(|t| t.id == id) {
                let status = apply_job_error(t.messages_mut(), err);
                t.accessed_ms = now_ms();
                return status;
            }
        }
        err.to_string()
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
        self.persist_cfg();
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
        match crate::update::restart_system(!self.window_visible) {
            Ok(()) => {
                if let Some(tray) = self.tray.take() {
                    crate::tray::drop_off_thread(tray);
                }
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
        if self.reflect_rx.is_some() {
            self.status = "Reflecting…".into();
            return;
        }
        let vis = self.visible_thread_id();
        let job = self.chat_job_thread.as_deref();
        let facts = if job.is_none() || job == Some(vis.as_str()) {
            fact_candidates_from(self.messages.iter().map(|m| (m.0.as_str(), m.1.as_str())))
        } else {
            self.threads
                .iter()
                .find(|t| Some(t.id.as_str()) == job)
                .map(|t| fact_candidates(&t.messages))
                .unwrap_or_else(|| {
                    fact_candidates_from(
                        self.messages.iter().map(|m| (m.0.as_str(), m.1.as_str())),
                    )
                })
        };
        if self.policy().learns() {
            extract_insights(&mut self.learning, &facts);
            let learning = self.learning.clone();
            let io = self.persist_io.clone();
            std::thread::spawn(move || {
                if let Ok(_g) = io.lock() {
                    let _ = crate::store::save_learning(&learning);
                }
            });
        }
        let name = self.mem_name.clone();
        let body = self.mem_body.clone();
        std::thread::spawn(move || {
            if name != "MEMORY.md"
                && name != "USER.md"
                && config::read_memory(&name) != body
            {
                let _ = config::write_memory(&name, &body);
            }
        });
        let mem_name = self.mem_name.clone();
        let mem_body = self.mem_body.clone();
        let writes_user = self.policy().writes_user_md();
        let (tx, rx) = mpsc::channel();
        self.reflect_rx = Some(rx);
        self.status = "Reflecting…".into();
        std::thread::spawn(move || {
            let current = if mem_name == "MEMORY.md" {
                mem_body.clone()
            } else {
                config::read_memory("MEMORY.md")
            };
            let edit = surgical_memory_edit(&current, &facts);
            if !edit.diff.is_empty() {
                let next = edit.next.clone();
                let _ = config::write_memory("MEMORY.md", &next);
            }
            let user_edit = if writes_user {
                let prefs = user_pref_facts(&facts);
                if prefs.is_empty() {
                    None
                } else {
                    let user = if mem_name == "USER.md" {
                        mem_body
                    } else {
                        config::read_memory("USER.md")
                    };
                    let ue = surgical_memory_edit(&user, &prefs);
                    if ue.diff.is_empty() {
                        None
                    } else {
                        let next = ue.next.clone();
                        let _ = config::write_memory("USER.md", &next);
                        Some(ue)
                    }
                }
            } else {
                None
            };
            let _ = tx.send((edit, user_edit));
        });
    }

    fn poll_reflect(&mut self) {
        let Some(rx) = self.reflect_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok((edit, user_edit)) => {
                let mut wrote = !edit.diff.is_empty();
                if wrote {
                    self.reflect_diff = edit.diff;
                    if let Some(i) = Self::mem_file_idx("MEMORY.md") {
                        self.mem_cache_at[i] = config::memory_updated_at("MEMORY.md");
                        if self.mem_name == "MEMORY.md" {
                            self.mem_cache_body[i] = edit.next.clone();
                            self.mem_body = edit.next;
                        } else {
                            self.mem_cache_body[i] = edit.next;
                        }
                    } else if self.mem_name == "MEMORY.md" {
                        self.mem_body = edit.next;
                    }
                }
                if let Some(ue) = user_edit {
                    if self.reflect_diff.is_empty() {
                        self.reflect_diff = ue.diff;
                    }
                    if let Some(i) = Self::mem_file_idx("USER.md") {
                        self.mem_cache_at[i] = config::memory_updated_at("USER.md");
                        if self.mem_name == "USER.md" {
                            self.mem_cache_body[i] = ue.next.clone();
                            self.mem_body = ue.next;
                        } else {
                            self.mem_cache_body[i] = ue.next;
                        }
                    } else if self.mem_name == "USER.md" {
                        self.mem_body = ue.next;
                    }
                    wrote = true;
                }
                self.status = if wrote {
                    "Reflected MEMORY.md".into()
                } else {
                    "Reflect: nothing new".into()
                };
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.reflect_rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.status = "Reflect failed".into();
            }
        }
    }

    fn run_skill_verify(&mut self) {
        if self.skill_name.is_empty() || self.verify_rx.is_some() {
            return;
        }
        let name = self.skill_name.clone();
        let cwd = host_working_dir(&self.cfg.project_dir);
        let (tx, rx) = mpsc::channel();
        self.verify_rx = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(skills::run_verify(&name, cwd.as_deref()));
        });
    }

    fn poll_verify(&mut self) {
        let Some(rx) = self.verify_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(Some(v)) => self.apply_verify_result(v),
            Ok(None) => {}
            Err(mpsc::TryRecvError::Empty) => {
                self.verify_rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {}
        }
    }

    fn apply_verify_result(&mut self, v: VerifyResult) {
        self.verify_ok_turn = v.ok;
        self.verify_chip = if v.ok {
            "verify pass".into()
        } else {
            "verify fail".into()
        };
        self.push_bound_msg("user", format!("VERIFY_RESULT:\n{}", v.detail));
        self.persist();
        if v.ok {
            if let Some(s) = self.skill_list.iter_mut().find(|s| s.name == self.skill_name) {
                s.runs = bump_skill_run(s.runs);
                let bumped = s.clone();
                std::thread::spawn(move || {
                    let _ = skills::save_skill(&bumped);
                });
            }
        }
    }

    fn replay_saved_recipe(&mut self, id: &str) -> bool {
        if self.running {
            self.status = "Busy — wait, then replay".into();
            return false;
        }
        let recipe = if id.eq_ignore_ascii_case("last") {
            crate::recipes::load_last().or_else(|| self.last_recipe.clone())
        } else {
            crate::recipes::load_recipe(id)
        };
        match recipe {
            Some(r) => {
                self.last_recipe = Some(r);
                self.replay_recipe()
            }
            None => {
                self.status = format!("No recipe {id}");
                false
            }
        }
    }

    fn replay_recipe(&mut self) -> bool {
        if self.running {
            self.status = "Busy — wait, then replay".into();
            return false;
        }
        if self.recipe_desk_rx.is_some() || self.recipe_cap_rx.is_some() {
            self.status = "Recipe replay…".into();
            return true;
        }
        if self.last_recipe.is_none() {
            self.last_recipe = crate::recipes::load_last();
        }
        let Some(recipe) = self.last_recipe.clone() else {
            self.status = "No recipe".into();
            return false;
        };
        let (tx, rx) = mpsc::channel();
        self.recipe_desk_rx = Some(rx);
        self.status = "Recipe replay…".into();
        std::thread::spawn(move || {
            let rows = collect_rows();
            let current = screen_from_rows(&rows);
            let ops = replay_ops(&recipe, current);
            let mut t = String::new();
            let mut cmds = Vec::new();
            let mut frame = None;
            if let Some(c) = current {
                t.push_str(&format!("screen {}x{}\n", c.w, c.h));
            }
            for op in ops {
                match op {
                    ReplayOp::Reshoot => {
                        t.push_str("reshoot: screen changed, skip coordinate clicks\n");
                        let titles: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
                        let lock = lock_titles();
                        if !lock_blocks_hands(&titles)
                            && !lock_blocks_hands(&lock.iter().map(|s| s.as_str()).collect::<Vec<_>>())
                        {
                            t.push_str("frame: capturing…\n");
                            frame = Some(capture_data_url());
                        }
                    }
                    ReplayOp::Op(op) => cmds.push(computer_cmd_line(&op)),
                }
            }
            let _ = tx.send(ReplayDeskOut {
                text: t,
                cmds,
                frame,
            });
        });
        true
    }

    fn poll_replay_desk(&mut self) {
        let Some(rx) = self.recipe_desk_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(out) => {
                self.eyes_text = out.text;
                if let Some(cap) = out.frame {
                    match &cap {
                        Ok(url) => {
                            self.store_hub_frame(url);
                            self.remember_last_frame(url);
                            self.eyes_text = self.eyes_text.replace(
                                "frame: capturing…\n",
                                "frame: captured\n",
                            );
                        }
                        Err(e) => {
                            self.eyes_text = self
                                .eyes_text
                                .replace("frame: capturing…\n", &format!("frame: {e}\n"));
                        }
                    }
                }
                if out.cmds.is_empty() {
                    self.status = "Recipe replay".into();
                } else {
                    self.run_cmds(out.cmds);
                }
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.recipe_desk_rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.status = "Recipe replay failed".into();
            }
        }
    }

    fn speak_reply(&mut self, text: &str) {
        let key = self.bearer();
        let cap = TEXT_FILE_CAP as usize;
        let mut end = cap.min(text.len());
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        let text = text[..end].to_string();
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
        let pending = self.poll_eyes_cap();
        let rows = collect_rows();
        let labels: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
        let refused = refused_lock(&labels);
        let ask = self
            .messages
            .iter()
            .rev()
            .find(|m| m.0 == "user")
            .map(|m| m.1.as_str());
        let mut frame_note = None;
        let captured_ok = if self.cfg.cabin_eyes {
            self.last_window_title = rows
                .iter()
                .map(|r| r.name.as_str())
                .find(|n| !n.is_empty() && *n != "cursor")
                .unwrap_or("")
                .to_string();
            let lock = lock_titles();
            if lock_blocks_hands(&lock.iter().map(|s| s.as_str()).collect::<Vec<_>>())
                || !should_send_screenshot(&self.last_window_title, "")
            {
                frame_note = Some("frame: skipped lock/password\n".into());
                false
            } else if let Some(cap) = pending {
                match cap {
                    Ok(_) => {
                        frame_note = Some("frame: captured (on hub, not disk)\n".into());
                        true
                    }
                    Err(e) => {
                        frame_note = Some(format!("frame: {e}\n"));
                        false
                    }
                }
            } else {
                if self.eyes_cap_rx.is_none() {
                    let (tx, rx) = mpsc::channel();
                    self.eyes_cap_rx = Some(rx);
                    std::thread::spawn(move || {
                        let _ = tx.send(capture_data_url());
                    });
                }
                frame_note = Some("frame: capturing…\n".into());
                false
            }
        } else {
            false
        };
        let (rows, header) = prepare_windshield(&rows, ask, captured_ok);
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
        t.push_str(&header);
        for o in &frame.objects {
            t.push_str(&format!("- [{}] {} @{},{} {}x{}\n", o.kind, o.label, o.x, o.y, o.w, o.h));
        }
        if let Some(g) = &frame.goal {
            t.push_str(&format!("goal: {g}\n"));
        }
        if let Some(n) = frame_note {
            t.push_str(&n);
        }
        self.eyes_text = t;
        self.status = format!("{} objects", frame.objects.len());
    }

    fn poll_eyes_cap(&mut self) -> Option<Result<String, String>> {
        let Some(rx) = self.eyes_cap_rx.take() else {
            return None;
        };
        match rx.try_recv() {
            Ok(cap) => {
                if let Ok(url) = &cap {
                    if should_send_screenshot(&self.last_window_title, "") {
                        self.store_hub_frame(url);
                        self.remember_last_frame(url);
                    }
                    self.eyes_text = self.eyes_text.replace(
                        "frame: capturing…\n",
                        "frame: captured (on hub, not disk)\n",
                    );
                } else if let Err(e) = &cap {
                    self.eyes_text =
                        self.eyes_text.replace("frame: capturing…\n", &format!("frame: {e}\n"));
                }
                Some(cap)
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.eyes_cap_rx = Some(rx);
                None
            }
            Err(mpsc::TryRecvError::Disconnected) => None,
        }
    }

    fn poll_recipe_cap(&mut self) {
        let Some(rx) = self.recipe_cap_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(url)) => {
                self.store_hub_frame(&url);
                self.remember_last_frame(&url);
                self.eyes_text = self.eyes_text.replace(
                    "frame: capturing…\n",
                    "frame: captured\n",
                );
            }
            Ok(Err(e)) => {
                self.eyes_text = self
                    .eyes_text
                    .replace("frame: capturing…\n", &format!("frame: {e}\n"));
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.recipe_cap_rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {}
        }
    }

    fn halt_work(&mut self, status: impl Into<String>) {
        let status = status.into();
        self.halt_in_flight();
        self.finish_hub_dispatch(&status, false);
        self.status = status;
    }

    fn drain_inbox(&mut self) {
        if !self.hub_on
            || self.running
            || self.pending_hub_task.is_some()
            || !inbox_claim_ready(self.can_agent())
        {
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
            self.pending_hub_task = Some(t.id.clone());
            self.land_on_real_chat();
            self.send_chat(format!("[from {}] {}", t.from_name, t.prompt));
        }
    }

    fn finish_hub_dispatch(&mut self, result: &str, ok: bool) {
        let Some(id) = self.pending_hub_task.clone() else {
            return;
        };
        {
            let Ok(mut st) = self.hub.lock() else {
                return;
            };
            let peer = st.device_id.clone();
            let status = if ok { "done" } else { "failed" };
            let err = st
                .complete_task(&peer, &id, result, vec![], Some(status))
                .err();
            if clear_pending_after_complete(err) {
                self.pending_hub_task = None;
            }
        }
        self.persist_hub();
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
        self.persist_if_dirty();
        self.geom_dirty = false;
        self.window_visible = false;
        self.tray_saw_unfocused = false;
        self.tray_hid_at = Instant::now();
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
        self.tray_saw_unfocused = false;
        ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
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
                            let push = {
                                let last = self
                                    .live_mut()
                                    .last_mut()
                                    .map(|m| (m.0.as_str(), &mut m.1));
                                fold_stream_fields(last, role, text, kind)
                            };
                            if let Some((role, content)) = push {
                                self.live_mut().push((role, content));
                            }
                            if matches!(kind, StreamTokenKind::Replace) && voice_log_role(&ev).is_some()
                            {
                                self.persist();
                            } else {
                                self.persist_idle_key = self.persist_idle_now();
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
            if start_hub_rotates_pair(st.pair.as_ref().map(|p| p.expires_at), now_ms()) {
                st.rotate_pair();
            }
        }
        self.sync_hub_voice();
        match serve_lan(self.hub.clone(), self.hub_port) {
            Ok(p) => {
                self.hub_port = p;
                self.hub_on = true;
                self.status = format!("Hub live on :{p} ({HUB_KIND})");
                self.persist_hub();
            }
            Err(e) => {
                if let Ok(mut st) = self.hub.lock() {
                    st.sharing = false;
                }
                self.status = e;
            }
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
                        self.flush_projects();
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
        self.poll_host_diff();
        self.poll_acp();
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
        self.poll_persist();
        self.poll_grok_sessions();
        self.poll_inspect();
        self.poll_history_search();
        self.poll_mem_restore();
        self.poll_mem_file();
        self.poll_recall();
        self.poll_sync();
        self.poll_inhabit();
        self.poll_reflect();
        self.poll_session_show();
        self.poll_import_openclaw();
        self.poll_acp_spawn();
        self.poll_pick();
        self.poll_pick_list();
        self.poll_eyes_cap();
        self.poll_recipe_cap();
        self.poll_replay_desk();
        self.poll_verify();
        self.poll_pending_kick();
        self.live_room();
        self.tick_heartbeat();
        let close_requested = ctx.input(|i| i.viewport().close_requested());
        let mut just_hid = false;
        if crate::tray::ignore_close_while_hidden(self.window_visible, close_requested) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        } else if close_requested {
            let hide = crate::tray::should_hide_on_close(
                self.cfg.close_to_tray,
                self.tray.is_some(),
            ) && !self.want_quit;
            if hide {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.hide_to_tray(ctx);
                just_hid = true;
            } else {
                self.capture_window(ctx);
                self.persist_if_dirty();
                self.geom_dirty = false;
            }
        }
        if self.oauth_pending.is_some() || self.oauth_start_rx.is_some() || self.oauth_poll_rx.is_some()
        {
            self.poll_oauth();
            let wait = self
                .oauth_pending
                .as_ref()
                .map(|p| p.interval.max(1))
                .unwrap_or(1);
            ctx.request_repaint_after(Duration::from_secs(wait));
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
            self.persist_bg();
        }
        let wait = next_heartbeat_wait_ms(
            self.last_heartbeat.elapsed().as_millis() as u64,
            HEARTBEAT_MS,
        );
        let live = wants_live_repaint(
            self.running,
            self.chip_busy
                || self.goal_busy
                || self.oauth_photo_busy
                || self.review_busy
                || self.greeting_busy
                || self.greeting_files_rx.is_some()
                || self.pending_kick.is_some()
                || self.kick_cap_rx.is_some()
                || self.recipe_cap_rx.is_some()
                || self.recipe_desk_rx.is_some()
                || self.host_diff_rx.is_some()
                || self.verify_rx.is_some()
                || self.grok_sessions_rx.is_some()
                || self.persist_rx.is_some()
                || self.inspect_rx.is_some()
                || self.history_rx.is_some()
                || self.mem_restore_rx.is_some()
                || self.mem_file_rx.is_some()
                || self.recall_rx.is_some()
                || self.sync_rx.is_some()
                || self.inhabit_rx.is_some()
                || self.reflect_rx.is_some()
                || self.session_show_rx.is_some()
                || self.import_rx.is_some()
                || self.acp_spawn_rx.is_some()
                || self.pick_rx.is_some()
                || self.pick_list_rx.is_some()
                || self.oauth_start_rx.is_some()
                || self.oauth_poll_rx.is_some()
                || self.night_check_rx.is_some()
                || self.eyes_cap_rx.is_some()
                || grokhub_acp::doctor_line_busy(),
            self.hub_on,
            self.window_visible,
            self.page_nav() == Nav::Imagine,
            self.wall_busy,
        );
        if crate::tray::honor_cabin_raise(self.want_quit)
            && !just_hid
            && crate::tray::take_cabin_raise()
        {
            self.show_from_tray(ctx);
        } else if !self.window_visible {
            let focused = ctx.input(|i| i.viewport().focused.unwrap_or(false));
            self.tray_saw_unfocused =
                crate::tray::remember_hidden_unfocus(focused, self.tray_saw_unfocused);
            let since_ms = self.tray_hid_at.elapsed().as_millis() as u64;
            let tick = if crate::tray::hidden_raise_ready(since_ms) {
                crate::tray::hidden_window_tick(
                    true,
                    focused,
                    just_hid,
                    self.tray_saw_unfocused,
                )
            } else {
                crate::tray::HiddenTick::StayHidden
            };
            match tick {
                crate::tray::HiddenTick::Raise => self.show_from_tray(ctx),
                crate::tray::HiddenTick::StayHidden => {
                    if crate::tray::reapply_unmap(true, focused) {
                        apply_tray_window(ctx, crate::tray::hide_to_tray_window());
                    }
                }
            }
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
            Nav::Eyes => self.ui_chat(ctx),
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
        let today = Self::local_day();
        if !today.is_empty() && today != "1970-01-01" {
            let before = self.usage.day.clone();
            roll_usage_day(&mut self.usage, &today);
            if self.usage.day != before {
                self.persist_usage();
                self.persist_idle_key = self.persist_idle_now();
            }
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
                    if self.running {
                        self.status = "Busy — wait, then run".into();
                    } else {
                        self.agents[i].status = "running".into();
                        let p = self.agents[i].prompt.clone();
                        let tid = self.agents[i].thread_id.clone();
                        self.nav = Nav::Chat;
                        if !tid.is_empty() {
                            self.chat_job_thread = Some(tid);
                        }
                        self.push_bound_msg("user", p);
                        self.persist();
                        self.kick_model(false);
                    }
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
            "eyes" => {
                self.open_recent_chat();
                Nav::Chat
            }
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
                    ui.add_space(12.0);
                    ui.label(
                        RichText::new("GrokHub")
                            .size(crate::theme::FONT_CHROME)
                            .color(crate::theme::muted()),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if titlebar_chrome_hit(&titlebar_chrome_btn(ui, "×")) {
                            let hide = crate::tray::should_hide_on_close(
                                self.cfg.close_to_tray,
                                self.tray.is_some(),
                            ) && !self.want_quit;
                            if hide {
                                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
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
                        let (_rect, drag) = ui.allocate_exact_size(
                            ui.available_size(),
                            egui::Sense::click_and_drag(),
                        );
                        if titlebar_should_start_drag(drag.drag_started()) {
                            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                        }
                    });
                });
                let hair = ui.max_rect();
                ui.painter().hline(
                    hair.x_range(),
                    hair.bottom() - 0.5,
                    egui::Stroke::new(1.0_f32, crate::theme::border()),
                );
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
            .filter(|s| !greeting_name("", s).is_empty())
            .unwrap_or_else(|| {
                let n = greeting_name(&self.greeting_user_md, "");
                if n.is_empty() {
                    "Grok".into()
                } else {
                    n
                }
            });
        let email = self
            .secrets
            .oauth
            .as_ref()
            .and_then(|t| t.email.clone())
            .unwrap_or_else(|| {
                if grokhub_acp::grok_cli_key().is_some() {
                    "grok login".into()
                } else {
                    "Run grok login".into()
                }
            });
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
                            if !history_row_visible(
                                &title,
                                self.threads[i].scratch,
                                self.threads[i].messages.is_empty(),
                                i == self.thread_idx,
                                self.threads[i].pinned,
                            ) {
                                continue;
                            }
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
                        if !self.grok_sessions_loaded {
                            // History page loads grok sessions off the first rail paint
                            // so a hung `grok sessions list` cannot freeze the cabin.
                        }
                        let owned: Vec<(String, String)> = self
                            .threads
                            .iter()
                            .filter_map(|t| t.grok_session.clone().map(|id| (id, t.id.clone())))
                            .collect();
                        for s in &self.grok_sessions {
                            if owned.iter().any(|(id, _)| id == &s.id) {
                                continue;
                            }
                            if !q.is_empty() && !s.title.to_ascii_lowercase().contains(&q) {
                                continue;
                            }
                            let resp = Self::nav_row(
                                ui,
                                false,
                                crate::icons::RailIcon::Chat,
                                &display_tab_title(&s.title),
                                false,
                            );
                            if resp.clicked() {
                                act = Some(TabAct::OpenGrok(s.id.clone()));
                            }
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
                            Some(TabAct::OpenGrok(id)) => self.open_grok_session(&id),
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

    fn cached_chat_views(&mut self) -> &[ChatView] {
        let tid = self.visible_thread_id();
        let n = self.messages.len();
        let last = self.messages.last().map(|m| m.1.len()).unwrap_or(0);
        if self.chat_view_tid == tid && self.chat_view_n == n && self.chat_view_last == last {
            return &self.chat_views;
        }
        let refs: Vec<(&str, &str)> = self
            .messages
            .iter()
            .map(|m| (m.0.as_str(), m.1.as_str()))
            .collect();
        if self.chat_view_tid == tid && self.chat_view_n == n && !self.chat_views.is_empty() {
            refresh_last_stretch(&mut self.chat_views, &refs);
        } else {
            self.chat_views = visible_chat_refs(refs.iter().copied());
        }
        self.chat_view_tid = tid;
        self.chat_view_n = n;
        self.chat_view_last = last;
        &self.chat_views
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
                let pane = clamp_row_width(ui.available_width());
                egui::ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.set_max_width(pane);
                        let thinking = self.thinking_here();
                        let mut act = ChatBlockAct::None;
                        let last_kind;
                        {
                            let views = self.cached_chat_views();
                            let last_thought =
                                views.iter().rposition(|v| v.kind == ChatKind::Thought);
                            last_kind = views.last().map(|v| v.kind);
                            for (i, block) in views.iter().enumerate() {
                                match paint_chat_block(
                                    ui,
                                    block,
                                    i,
                                    thinking && last_thought == Some(i),
                                ) {
                                    ChatBlockAct::None => {}
                                    other => act = other,
                                }
                                ui.add_space(10.0);
                            }
                        }
                        match act {
                            ChatBlockAct::Copy(body) => {
                                ui.ctx().copy_text(body);
                                self.status = "Copied".into();
                            }
                            ChatBlockAct::Reply(body) => {
                                self.composer =
                                    append_composer(&self.composer, &quote_for_reply(&body));
                                if !self.composer.ends_with('\n') {
                                    self.composer.push('\n');
                                }
                                self.composer_want_focus = true;
                            }
                            ChatBlockAct::None => {}
                        }
                        if thinking {
                            match last_kind {
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
                        self.paint_tool_cards(ui);
                        self.paint_perm_ask(ui);
                    });
            });
    }

    fn paint_tool_cards(&self, ui: &mut egui::Ui) {
        if self.tool_cards.is_empty() {
            return;
        }
        ui.add_space(8.0);
        for card in &self.tool_cards {
            egui::Frame::none()
                .fill(crate::theme::elevated())
                .rounding(12.0)
                .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                .inner_margin(egui::Margin::same(10.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(&card.title)
                                .size(13.0)
                                .color(crate::theme::fg()),
                        );
                        crate::cards::status_chip(
                            ui,
                            &card.status,
                            if card.status == "failed" {
                                crate::cards::ChipTone::Offline
                            } else if card.is_computer_use() {
                                crate::cards::ChipTone::Live
                            } else {
                                crate::cards::ChipTone::Mute
                            },
                        );
                    });
                    if !card.diff.is_empty() && !card.diff.trim().starts_with('{') && !card.diff.trim().starts_with('[') {
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(card.diff.chars().take(800).collect::<String>())
                                .size(12.0)
                                .monospace()
                                .color(crate::theme::muted()),
                        );
                    } else if !card.detail.is_empty()
                        && !card.detail.trim().starts_with('{')
                        && !card.detail.trim().starts_with('[')
                    {
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(card.detail.chars().take(120).collect::<String>())
                                .size(12.0)
                                .color(crate::theme::muted()),
                        );
                    }
                    if let Some(url) = card.image_data_url.as_deref() {
                        if let Some((tex, size)) = eyes_frame_tex(ui.ctx(), url) {
                            let max_w = ui.available_width().min(360.0);
                            crate::cards::framed_preview(ui, &tex, size, max_w);
                        }
                    }
                });
            ui.add_space(6.0);
        }
    }

    fn paint_perm_ask(&mut self, ui: &mut egui::Ui) {
        let Some(p) = self.perm_ask.clone() else {
            return;
        };
        ui.add_space(8.0);
        egui::Frame::none()
            .fill(crate::theme::elevated())
            .rounding(12.0)
            .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
            .inner_margin(egui::Margin::same(12.0))
            .show(ui, |ui| {
                ui.label(
                    RichText::new(format!("Grok wants permission · {}", p.title))
                        .size(14.0)
                        .color(crate::theme::fg()),
                );
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if crate::cards::white_pill(ui, "Allow") {
                        if let Some(h) = &self.acp {
                            let _ = h.answer_permission(p.rpc_id.clone(), true);
                        }
                        self.perm_ask = None;
                    }
                    if crate::cards::ghost_pill(ui, "Deny") {
                        if let Some(h) = &self.acp {
                            let _ = h.answer_permission(p.rpc_id.clone(), false);
                        }
                        self.perm_ask = None;
                    }
                    if crate::cards::ghost_pill(ui, "Always") {
                        self.permission_mode = PermissionMode::AlwaysApprove;
                        if let Some(h) = &self.acp {
                            let _ = h.answer_permission_always(p.rpc_id.clone());
                        }
                        self.perm_ask = None;
                        self.status = "Permission always-approve".into();
                    }
                });
            });
    }

    fn ui_empty_home(&mut self, ui: &mut egui::Ui) {
        let greet_on = should_paint_greeting(self.messages.is_empty(), self.scratch())
            && !self.greeting.is_empty();
        let greet_h = if greet_on {
            crate::theme::GREET_HERO + 40.0
        } else {
            16.0
        };
        let block = greet_h + crate::theme::QUERY_MIN_H + 96.0;
        let lift = ((ui.available_height() - block) * 0.38).clamp(36.0, 220.0);
        ui.add_space(lift);
        let pane_w = crate::cards::composer_pill_w(ui.ctx().screen_rect().width());
        ui.vertical_centered_justified(|ui| {
            ui.set_max_width(pane_w);
            if greet_on {
                ui.label(
                    RichText::new(&self.greeting)
                        .font(crate::theme::title_font(crate::theme::GREET_HERO))
                        .color(crate::theme::muted()),
                );
                ui.add_space(28.0);
            } else {
                ui.add_space(28.0);
            }
            self.ui_composer_stack(ui);
            self.paint_perm_ask(ui);
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
                        let grok_missing = grokhub_acp::find_grok().is_none();
                        let need_login = grokhub_acp::grok_cli_key().is_none() && !self.has_key();
                        if grok_missing || need_login
                        {
                            ui.horizontal(|ui| {
                                crate::cards::settings_note(
                                    ui,
                                    if grok_missing {
                                        "Install Grok Build (x.ai/cli), then grok login."
                                    } else {
                                        "Run grok login. Chat, Imagine, and Fast chips use that token."
                                    },
                                );
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
            ui.set_width(cap);
            ui.set_max_width(cap);
            let session_now = self.session_mode.as_str().to_string();
            let perm_now = self.permission_mode.as_str().to_string();
            let row = crate::cards::session_row(ui, &session_now, &perm_now);
            if let Some(mode) = row.mode {
                if let Some(m) = SessionMode::parse(&mode) {
                    if self.running {
                        self.halt_in_flight();
                    }
                    self.session_mode = m;
                    self.acp = None;
                    self.acp_spawn_rx = None;
                    if let Some(t) = self.threads.get_mut(self.thread_idx) {
                        t.grok_session = None;
                    }
                    self.persist_idle_key = self.persist_idle_now();
                    self.status = format!("Session {}", m.as_str());
                }
            }
            if let Some(perm) = row.perm {
                if let Some(p) = PermissionMode::parse(&perm) {
                    if self.running {
                        self.halt_in_flight();
                    }
                    self.permission_mode = p;
                    self.acp = None;
                    self.acp_spawn_rx = None;
                    if let Some(t) = self.threads.get_mut(self.thread_idx) {
                        t.grok_session = None;
                    }
                    self.persist_idle_key = self.persist_idle_now();
                    self.status = format!("Permission {}", p.as_str());
                }
            }
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
                        if self.composer_want_focus {
                            ui.memory_mut(|m| m.request_focus(composer_id));
                            self.composer_want_focus = false;
                        }
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
                                        .hint_text("Ask anything")
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
            let mut rotated = false;
            let (name, sharing, pair_code) = if let Ok(mut st) = self.hub.lock() {
                if self.hub_on
                    && st
                        .pair
                        .as_ref()
                        .is_some_and(|p| !pair_code_is_live(p.expires_at, now_ms()))
                {
                    st.rotate_pair();
                    rotated = true;
                }
                (
                    st.device_name.clone(),
                    self.hub_on,
                    st.pair.as_ref().and_then(|p| {
                        devices_shows_pair_code(
                            self.hub_on,
                            pair_code_is_live(p.expires_at, now_ms()),
                        )
                        .then(|| p.code.clone())
                    }),
                )
            } else {
                (String::new(), false, None)
            };
            if rotated {
                self.persist_hub();
            }
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
                    &format!("Open {} on the other device.", discover_hub_pair_url(self.hub_port)),
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
                    self.persist_hub();
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
                        if !self.scratch()
                            && self.mem_name != name
                        {
                            let leaving = self.mem_name.clone();
                            let body = self.mem_body.clone();
                            if let Some(i) = Self::mem_file_idx(&leaving) {
                                self.mem_cache_body[i] = body.clone();
                                self.mem_cache_at[i] = config::memory_updated_at(&leaving);
                            }
                            std::thread::spawn(move || {
                                if config::read_memory(&leaving) != body {
                                    let _ = config::write_memory(&leaving, &body);
                                }
                            });
                        }
                        self.mem_name = name.into();
                        let at = config::memory_updated_at(name);
                        if let Some(i) = Self::mem_file_idx(name) {
                            if self.mem_cache_at[i] == 0 {
                                self.mem_body = config::read_memory(name);
                                self.mem_cache_body[i] = self.mem_body.clone();
                                self.mem_cache_at[i] = at;
                            } else {
                                self.mem_body = self.mem_cache_body[i].clone();
                                if self.mem_cache_at[i] != at && self.mem_file_rx.is_none() {
                                    let n = name.to_string();
                                    let (tx, rx) = mpsc::channel();
                                    self.mem_file_rx = Some((n.clone(), rx));
                                    std::thread::spawn(move || {
                                        let body = config::read_memory(&n);
                                        let at = config::memory_updated_at(&n);
                                        let _ = tx.send((at, body));
                                    });
                                }
                            }
                        } else {
                            self.mem_body = config::read_memory(name);
                        }
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if crate::cards::ghost_pill(ui, "Restore") {
                        if self.scratch() {
                            self.status = "Scratch — no memory writes".into();
                        } else if self.mem_restore_rx.is_some() {
                            self.status = "Restoring…".into();
                        } else {
                            let name = self.mem_name.clone();
                            let (tx, rx) = mpsc::channel();
                            self.mem_restore_rx = Some(rx);
                            self.status = "Restoring…".into();
                            std::thread::spawn(move || {
                                let _ = tx.send((name.clone(), config::restore_memory(&name)));
                            });
                        }
                    }
                    if crate::cards::ghost_pill(ui, "Reflect") {
                        self.run_reflect();
                    }
                    if crate::cards::white_pill(ui, "Save") {
                        if self.scratch() {
                            self.status = "Scratch — no memory writes".into();
                        } else {
                            let name = self.mem_name.clone();
                            let body = self.mem_body.clone();
                            std::thread::spawn(move || {
                                if config::read_memory(&name) != body {
                                    let _ = config::write_memory(&name, &body);
                                }
                            });
                            self.status = format!("Wrote {}", self.mem_name);
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
        self.cfg.api_key.clear();
        if let Ok(mut st) = self.hub.lock() {
            if !self.cfg.device_name.trim().is_empty() {
                st.device_name = self.cfg.device_name.clone();
            }
        }
        let p = expand_home(&self.cfg.project_dir);
        let tree_changed = self
            .threads
            .get(self.thread_idx)
            .and_then(|t| t.grok_cwd.as_deref())
            .map(|cwd| cwd != p)
            .unwrap_or(false);
        self.cfg.project_dir = p.clone();
        if !p.trim().is_empty() {
            let dir = p.clone();
            std::thread::spawn(move || {
                let _ = std::fs::create_dir_all(&dir);
            });
        }
        self.project_sel = upsert_bound(&mut self.projects, &p);
        self.touch_projects();
        self.status = "Saved".into();
        self.sync_hub_voice();
        if tree_changed {
            if self.running {
                self.halt_in_flight();
            }
            self.acp = None;
            self.acp_spawn_rx = None;
            if let Some(t) = self.threads.get_mut(self.thread_idx) {
                t.grok_cwd = None;
                t.grok_session = None;
            }
            self.persist();
        } else {
            self.flush_projects();
            self.persist_cfg();
            self.persist_hub();
            self.persist_secrets();
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
                                                            crate::cards::settings_field(ui, "Console key", "Voice and Imagine only. Agent auth is grok login (cached token). Lives in secrets.json, never markdown.", &mut self.secrets.api_key, true);
                                                            crate::cards::settings_field(ui, "Device name", "How this box shows up on the hub.", &mut self.cfg.device_name, false);
                                                            crate::cards::settings_field(ui, "Chat model", "Unused by Grok Build. Session model is /model in grok. Keep empty.", &mut self.cfg.model, false);
                                                            crate::cards::settings_note(ui, "Session mode is Chat / Plan / Ask on the composer. /effort sets reasoning. The leftover ladder pin below is unused by Grok Build.");
                                                            crate::cards::settings_note(ui, &format!("Live still model: {imagine_live}. Chat models never run here."));
                                                            crate::cards::settings_field(ui, "Imagine override", "Must contain “image” or the cabin keeps grok-imagine-image-2.0. Retired grok-2-image names are rewritten.", &mut self.cfg.imagine_model, false);
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
                                                                            self.persist_cfg();
                                                                            self.status = "Saved".into();
                                                                        }
                                                                    }
                                                                    ui.add_space(10.0);
                                                                }
                                                            });
                                                        }
                                                        SettingsSec::Behavior => {
                                                            if crate::cards::settings_toggle(ui, "Close to tray", "The cabin keeps working in the background.", &mut self.cfg.close_to_tray) {
                                                                self.persist_cfg();
                                                                self.status = "Saved".into();
                                                            }
                                                            if crate::cards::settings_toggle(
                                                                ui,
                                                                "Living wall",
                                                                "Every few hours the cabin paints a new cover. Twenty live. Oldest leaves first.",
                                                                &mut self.cfg.imagine_wall,
                                                            ) {
                                                                self.persist_cfg();
                                                                self.status = "Saved".into();
                                                            }
                                                            crate::cards::settings_note(ui, "Night always runs. Quiet hours and daily caps do not hold work.");
                                                        }
                                                        SettingsSec::Host => {
                                                            crate::cards::settings_note(ui, &format!("{}\nInstall: curl -fsSL https://x.ai/cli/install.sh | bash\nThen grok login --device-auth. Halt cancels the ACP turn.", build_agent::grok_banner()));
                                                        }
                                                        SettingsSec::Imagine => {
                                                            crate::cards::settings_note(ui, &format!("Live still model: {imagine_live}. Chat models never run here."));
                                                            crate::cards::settings_field(ui, "Imagine override", "Must contain “image” or the cabin keeps grok-imagine-image-2.0. Retired grok-2-image names are rewritten.", &mut self.cfg.imagine_model, false);
                                                            if crate::cards::settings_toggle(
                                                                ui,
                                                                "Living wall",
                                                                "Every few hours the cabin paints a new cover. Twenty live. Oldest leaves first. Random seat.",
                                                                &mut self.cfg.imagine_wall,
                                                            ) {
                                                                self.persist_cfg();
                                                                self.status = "Saved".into();
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
                                                            crate::cards::settings_note(ui, "Overlay only — retarget leftover Origin remotes to GitHub, git pull --ff-only origin main, then install.sh --user. The clone must be on main. Does not wipe ~/.config/GrokHub.");
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
                                                                    "Reload hub, then start a new cabin and exit this one.",
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
                                                            ui.label(
                                                                RichText::new(format!(
                                                                    "GrokHub {}",
                                                                    env!("CARGO_PKG_VERSION")
                                                                ))
                                                                .size(crate::theme::FONT_HEADING)
                                                                .color(crate::theme::fg()),
                                                            );
                                                            ui.add_space(6.0);
                                                            crate::cards::settings_note(ui, "Native Grok Build cabin.");
                                                            crate::cards::settings_note(ui, &build_agent::grok_banner());
                                                            crate::cards::settings_note(ui, &usage);
                                                            crate::cards::settings_note(ui, &catalog);
                                                            crate::cards::settings_note(ui, &doctor);
                                                            if crate::cards::settings_action(ui, "Diagnostics", "Copy a redacted bundle. No secrets.", "Copy") {
                                                                copy_diag = true;
                                                            }
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
            let list = self.automations.clone();
            std::thread::spawn(move || {
                let _ = crate::night::save(&list);
            });
            self.persist_idle_key = self.persist_idle_now();
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
                                if ui.checkbox(&mut self.automations[i].enabled, "").changed() {
                                    let list = self.automations.clone();
                                    std::thread::spawn(move || {
                                        let _ = crate::night::save(&list);
                                    });
                                }
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
                    let list = self.automations.clone();
                    std::thread::spawn(move || {
                        let _ = crate::night::save(&list);
                    });
                    self.persist_idle_key = self.persist_idle_now();
                } else {
                    let idx = usize::MAX - i;
                    if let Some(a) = self.automations.get(idx) {
                        let inst = a.instructions.clone();
                        if let Some(id) = replay_automation_target(&inst) {
                            self.replay_saved_recipe(id);
                        } else {
                            self.land_on_real_chat();
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
                    if self.history_rx.is_some() {
                        self.status = "Searching…".into();
                    } else {
                    if !self.scratch() {
                        let name = self.mem_name.clone();
                        let body = self.mem_body.clone();
                        std::thread::spawn(move || {
                            if config::read_memory(&name) != body {
                                let _ = config::write_memory(&name, &body);
                            }
                        });
                    }
                    let q = self.history_q.clone();
                    let mem_name = self.mem_name.clone();
                    let mem_body = self.mem_body.clone();
                    let vis = self.thread_idx;
                    let mut thread_rows = Vec::new();
                    for (i, t) in self.threads.iter().enumerate() {
                        let body = if i == vis {
                            search_thread_body(self.messages.iter().map(|m| m.1.as_str()))
                        } else {
                            search_thread_body(t.messages.iter().map(|(_, c)| c.as_str()))
                        };
                        thread_rows.push((t.title.clone(), body));
                    }
                    let (tx, rx) = mpsc::channel();
                    self.history_rx = Some(rx);
                    self.status = "Searching…".into();
                    std::thread::spawn(move || {
                        let soul = if mem_name == "SOUL.md" {
                            mem_body.clone()
                        } else {
                            config::read_memory("SOUL.md")
                        };
                        let user = if mem_name == "USER.md" {
                            mem_body.clone()
                        } else {
                            config::read_memory("USER.md")
                        };
                        let memory = if mem_name == "MEMORY.md" {
                            mem_body.clone()
                        } else {
                            config::read_memory("MEMORY.md")
                        };
                        let mut rows = vec![
                            ("SOUL.md".into(), soul),
                            ("USER.md".into(), user),
                            ("MEMORY.md".into(), memory),
                        ];
                        rows.extend(thread_rows);
                        let _ = tx.send(search_corpus(&q, &rows));
                    });
                    }
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
            crate::cards::section_label(ui, "Grok Build sessions");
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("ACP sessions on this machine (`grok sessions`). Cabin chats stay below.")
                        .size(12.0)
                        .color(crate::theme::subtle()),
                );
                if crate::cards::ghost_pill(ui, "Refresh") {
                    self.grok_sessions_loaded = false;
                    self.reload_grok_sessions();
                    self.status = if grokhub_acp::find_grok().is_some() {
                        "Listing Grok sessions…".into()
                    } else {
                        build_agent::grok_banner()
                    };
                }
            });
            if !self.grok_sessions_loaded {
                self.reload_grok_sessions();
            }
            if self.grok_sessions_rx.is_some() {
                ui.label(
                    RichText::new("Listing Grok sessions…")
                        .size(13.0)
                        .color(crate::theme::muted()),
                );
            } else if self.grok_sessions.is_empty() {
                ui.label(
                    RichText::new(if grokhub_acp::find_grok().is_some() {
                        "No grok sessions listed yet."
                    } else {
                        "Install Grok Build (x.ai/cli) to list sessions."
                    })
                    .size(13.0)
                    .color(crate::theme::muted()),
                );
            } else {
                let mut open: Option<String> = None;
                for s in &self.grok_sessions {
                    if crate::cards::grok_tile(
                        ui,
                        crate::icons::TileIcon::Chat,
                        &s.title,
                        "Grok session",
                        None,
                        false,
                    ) == crate::cards::TileHit::Body
                    {
                        open = Some(s.id.clone());
                    }
                    ui.add_space(6.0);
                }
                if let Some(id) = open {
                    self.open_grok_session(&id);
                }
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
                        if !history_row_visible(
                            &title,
                            self.threads[i].scratch,
                            self.threads[i].messages.is_empty(),
                            i == self.thread_idx,
                            self.threads[i].pinned,
                        ) {
                            continue;
                        }
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
                        Some(TabAct::OpenGrok(id)) => self.open_grok_session(&id),
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
                                self.flush_board();
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
                self.flush_board();
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
        let authed = self.llm_ready();
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
                                                "Video calls grok-imagine-video-1.5 and saves an mp4."
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
                if !self.skill_body.is_empty() {
                    let parsed = grokhub_core::parse_skill_md(&self.skill_body);
                    let written = parsed.clone();
                    std::thread::spawn(move || {
                        let _ = skills::save_skill(&written);
                    });
                    self.remember_skill(parsed);
                }
                let existing: Vec<String> = self.skill_list.iter().map(|s| s.name.clone()).collect();
                let stub = crate::cards::starter_skill(&next_starter_skill_name(&existing));
                let written = stub.clone();
                std::thread::spawn(move || {
                    let _ = skills::save_skill(&written);
                });
                self.remember_skill(stub.clone());
                self.skill_name = stub.name.clone();
                self.skill_body = grokhub_core::render_skill_md(&stub);
                self.status = format!("Wrote skill {}", stub.name);
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
                crate::cards::section_label(ui, "Grok Build extensions");
                ui.label(
                    RichText::new("MCP, skills, plugins, and hooks live in Grok Build. Inspect is `grok inspect --json`; MCP/Plugins run `grok mcp list` / `grok plugin list`.")
                        .color(crate::theme::muted()),
                );
                ui.add_space(10.0);
                ui.horizontal_wrapped(|ui| {
                    if crate::cards::white_pill(ui, "Inspect") {
                        self.run_grok_extension(&["inspect", "--json"]);
                    }
                    if crate::cards::ghost_pill(ui, "MCP") {
                        self.run_grok_extension(&["mcp", "list"]);
                    }
                    if crate::cards::ghost_pill(ui, "Skills") {
                        self.run_grok_extension(&["inspect", "--json"]);
                    }
                    if crate::cards::ghost_pill(ui, "Plugins") {
                        self.run_grok_extension(&["plugin", "list"]);
                    }
                });
                ui.add_space(12.0);
                if self.inspect_text.is_empty() {
                    let _ = crate::cards::empty_prompt_tile(
                        ui,
                        crate::icons::TileIcon::List,
                        "No inspect yet",
                        "Run Inspect to dump Grok Build config.",
                    );
                } else {
                    egui::Frame::none()
                        .fill(crate::theme::elevated())
                        .rounding(12.0)
                        .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
                        .inner_margin(egui::Margin::same(10.0))
                        .show(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .max_height(280.0)
                                .show(ui, |ui| {
                                    ui.label(
                                        RichText::new(&self.inspect_text)
                                            .size(12.0)
                                            .monospace()
                                            .color(crate::theme::fg()),
                                    );
                                });
                        });
                }
                ui.add_space(20.0);
                crate::cards::section_label(ui, "Suggested");
                let conn_tiles = crate::cards::merge_suggested_connectors(&self.suggestions.connectors);
                crate::cards::tile_row(ui, conn_tiles.len(), |ui, i| {
                    let (icon, title, body, _tool) = &conn_tiles[i];
                    let _ = crate::cards::grok_tile(ui, *icon, title, body, None, false);
                });
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
                    if !self.skill_body.is_empty() {
                        let parsed = grokhub_core::parse_skill_md(&self.skill_body);
                        let written = parsed.clone();
                        std::thread::spawn(move || {
                            let _ = skills::save_skill(&written);
                        });
                        self.remember_skill(parsed);
                    }
                    let sk = crate::cards::skill_from_learned(s);
                    let written = sk.clone();
                    std::thread::spawn(move || {
                        let _ = skills::save_skill(&written);
                    });
                    self.remember_skill(sk.clone());
                    self.skill_name = sk.name.clone();
                    self.skill_body = grokhub_core::render_skill_md(&sk);
                    self.status = format!("Wrote skill {}", sk.name);
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
                    if name != self.skill_name && !self.skill_body.is_empty() {
                        let parsed = grokhub_core::parse_skill_md(&self.skill_body);
                        let written = parsed.clone();
                        std::thread::spawn(move || {
                            let _ = skills::save_skill(&written);
                        });
                        self.remember_skill(parsed);
                    }
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
                        let written = parsed.clone();
                        std::thread::spawn(move || {
                            let _ = skills::save_skill(&written);
                        });
                        self.remember_skill(parsed.clone());
                        self.status = format!("Wrote skill {}", parsed.name);
                    }
                    if crate::cards::ghost_pill(ui, "Use in chat") && !self.skill_name.is_empty() {
                        if !self.skill_body.is_empty() {
                            let parsed = grokhub_core::parse_skill_md(&self.skill_body);
                            let written = parsed.clone();
                            std::thread::spawn(move || {
                                let _ = skills::save_skill(&written);
                            });
                            self.remember_skill(parsed);
                        }
                        let name = self.skill_name.clone();
                        let slash = self
                            .skill_list
                            .iter()
                            .find(|s| s.name == name)
                            .map(|s| {
                                if let Some(r) = parse_recipe(&s.instructions) {
                                    self.last_recipe = Some(r);
                                }
                                s.slash.clone()
                            })
                            .unwrap_or_default();
                        self.nav = Nav::Chat;
                        self.send_chat(skill_use_in_chat_prompt(&slash, &name));
                    }
                    if crate::cards::ghost_pill(ui, "Run verify") && !self.skill_name.is_empty() {
                        if !self.skill_body.is_empty() {
                            let parsed = grokhub_core::parse_skill_md(&self.skill_body);
                            let written = parsed.clone();
                            std::thread::spawn(move || {
                                let _ = skills::save_skill(&written);
                            });
                            self.remember_skill(parsed);
                        }
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
        // Desk was a cabin computer-use menu. Grok Build already drives the
        // desktop; frames land on the chat pane.
        self.ui_chat(ctx);
    }
}


fn eyes_frame_tex(ctx: &egui::Context, url: &str) -> Option<(TextureHandle, [usize; 2])> {
    if url.len() > FRAME_CAP {
        return None;
    }
    let key: String = url.chars().take(48).collect();
    let id = egui::Id::new(("eyes-frame", url.len(), key.as_str()));
    if let Some(hit) = ctx.data(|d| d.get_temp::<(TextureHandle, [usize; 2])>(id)) {
        return Some(hit);
    }
    let cache_key = format!("{}:{key}", url.len());
    if let Some(img) = take_eyes_rgba(&cache_key) {
        let size = [img.width() as usize, img.height() as usize];
        let tex = ctx.load_texture(
            "eyes-last-frame",
            ColorImage::from_rgba_unmultiplied(size, img.as_raw()),
            TextureOptions::LINEAR,
        );
        let hit = (tex, size);
        ctx.data_mut(|d| d.insert_temp(id, hit.clone()));
        return Some(hit);
    }
    kick_eyes_tex(ctx.clone(), cache_key, url.to_string());
    None
}

struct EyesTexGate {
    inflight: HashSet<String>,
    ready: HashMap<String, image::RgbaImage>,
}

fn eyes_tex_gate() -> &'static Mutex<EyesTexGate> {
    static G: OnceLock<Mutex<EyesTexGate>> = OnceLock::new();
    G.get_or_init(|| {
        Mutex::new(EyesTexGate {
            inflight: HashSet::new(),
            ready: HashMap::new(),
        })
    })
}

fn take_eyes_rgba(key: &str) -> Option<image::RgbaImage> {
    let mut g = eyes_tex_gate().lock().ok()?;
    g.ready.remove(key)
}

fn kick_eyes_tex(ctx: egui::Context, key: String, url: String) {
    {
        let Ok(mut g) = eyes_tex_gate().lock() else {
            return;
        };
        if g.ready.contains_key(&key) || !g.inflight.insert(key.clone()) {
            return;
        }
    }
    std::thread::spawn(move || {
        let frame = PresenceFrame {
            data_url: url,
            at: 0,
        };
        let decoded = frame_bytes(&frame).and_then(|(_, buf)| {
            if (buf.len() as u64) > IMAGE_FILE_CAP {
                return None;
            }
            if !crate::desktop::image_pixels_ok_for_bytes(&buf) {
                return None;
            }
            image::load_from_memory(&buf).ok().map(|img| img.to_rgba8())
        });
        if let Ok(mut g) = eyes_tex_gate().lock() {
            g.inflight.remove(&key);
            if let Some(img) = decoded {
                g.ready.insert(key, img);
            }
        }
        ctx.request_repaint();
    });
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

struct LanHostCache {
    at: Instant,
    out: String,
    inflight: bool,
}

static LAN_HOST: Mutex<Option<LanHostCache>> = Mutex::new(None);
const CLOCK_TTL: Duration = Duration::from_secs(15);
static LAST_CLOCK: Mutex<Option<(Instant, LocalClock, bool)>> = Mutex::new(None);
static LAST_DAY: Mutex<Option<(Instant, String, bool)>> = Mutex::new(None);

fn hostname_i() -> String {
    if let Ok(g) = LAN_HOST.lock() {
        if let Some(c) = g.as_ref() {
            let hit = c.out.clone();
            let fresh = c.at.elapsed().as_secs() < 30;
            let busy = c.inflight;
            drop(g);
            if !fresh && !busy {
                kick_hostname();
            }
            return hit;
        }
    }
    let out = hostname_i_now();
    if let Ok(mut g) = LAN_HOST.lock() {
        *g = Some(LanHostCache {
            at: Instant::now(),
            out: out.clone(),
            inflight: false,
        });
    }
    out
}

fn hostname_i_now() -> String {
    let mut cmd = std::process::Command::new("hostname");
    cmd.arg("-I");
    run_limited(cmd, Duration::from_millis(400))
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

fn kick_hostname() {
    if let Ok(mut g) = LAN_HOST.lock() {
        if let Some(c) = g.as_mut() {
            if c.inflight {
                return;
            }
            c.inflight = true;
        }
    }
    std::thread::spawn(|| {
        let out = hostname_i_now();
        if let Ok(mut g) = LAN_HOST.lock() {
            *g = Some(LanHostCache {
                at: Instant::now(),
                out,
                inflight: false,
            });
        }
    });
}

fn discover_hub_pair_url(port: u16) -> String {
    let out = hostname_i();
    let addrs = parse_hostname_i(&out);
    let refs: Vec<&str> = addrs.iter().map(|s| s.as_str()).collect();
    hub_pair_url(port, pick_lan_ipv4(&refs).as_deref())
}


#[cfg(test)]
mod tests {
    use super::select_all_edit;
    use eframe::egui;

    #[test]
    fn devices_pair_url_is_not_a_placeholder() {
        let url = super::discover_hub_pair_url(18766);
        assert!(url.starts_with("http://"), "{url}");
        assert!(url.contains(":18766"), "{url}");
        assert!(!url.contains("<lan>"), "{url}");
    }

    #[test]
    fn devices_hostname_must_not_block_the_ui() {
        let src = include_str!("app.rs");
        let host = src
            .split("fn hostname_i()")
            .nth(1)
            .and_then(|s| s.split("\nfn discover_hub_pair_url(").next())
            .expect("hostname_i");
        assert!(
            host.contains("run_limited("),
            "hostname -I on Devices paint must time out: {host}"
        );
        assert!(
            host.contains("thread::spawn") && host.contains("inflight"),
            "stale hostname -I must refresh off the UI thread: {host}"
        );
        assert!(
            !host.contains(".output()"),
            "hostname -I must not block Devices paint: {host}"
        );
        let disc = src
            .split("fn discover_hub_pair_url(")
            .nth(1)
            .and_then(|s| s.split("\n#[cfg(test)]").next())
            .expect("discover_hub_pair_url");
        assert!(
            disc.contains("hostname_i()"),
            "Devices pair URL must use the timed hostname helper: {disc}"
        );
    }

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
    fn short_user_bubble_sits_on_the_right() {
        with_fonts_ui(|ui| {
            ui.allocate_ui(egui::vec2(800.0, 200.0), |ui| {
                ui.set_max_width(800.0);
                let row = ui.max_rect();
                let resp = super::paint_speech_bubble(ui, "Hi", true, false);
                assert!(
                    resp.rect.width() < 200.0,
                    "short bubble stretched to {}",
                    resp.rect.width()
                );
                assert!(
                    (row.max.x - resp.rect.max.x).abs() < 8.0,
                    "user bubble max.x {} not near row max.x {}",
                    resp.rect.max.x,
                    row.max.x
                );
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
                    resp.rect.width() <= grokhub_core::bubble_max_width(800.0) + 8.0,
                    "pane column {}",
                    resp.rect.width()
                );
                assert!(
                    resp.rect.width() > 500.0,
                    "an 800px pane must not use a 440px column, got {}",
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
    fn long_sentence_stays_inside_the_pane_on_a_wide_row() {
        with_fonts_ui(|ui| {
            ui.allocate_ui(egui::vec2(1600.0, 500.0), |ui| {
                ui.set_max_width(1600.0);
                let body = "the clam gods? oh you know... ancient, briny, and extremely picky about their cream-to-broth ratio. they live in the black void between chowder pots, only emerging when someone dares to say manhattan style in their presence. knock twice and offer a saltine or they won't even open up.";
                let resp = super::paint_speech_bubble(ui, body, false, true);
                assert!(
                    resp.rect.width() <= grokhub_core::bubble_max_width(1600.0) + 8.0,
                    "wide pane stretched the bubble to {}",
                    resp.rect.width()
                );
                assert!(
                    resp.rect.width() > 500.0,
                    "a wide pane must not squeeze the reply, got {}",
                    resp.rect.width()
                );
                assert!(
                    resp.rect.height() > 28.0,
                    "long sentence must wrap, height {}",
                    resp.rect.height()
                );
            });
        });
    }

    #[test]
    fn markdown_reply_grows_past_plain_measure() {
        with_fonts_ui(|ui| {
            ui.allocate_ui(egui::vec2(800.0, 900.0), |ui| {
                ui.set_max_width(800.0);
                let body = "## Heading\n\n- bullet one\n- bullet two\n\nClosing line.";
                let wrap = grokhub_core::bubble_wrap_width(800.0, grokhub_core::BUBBLE_PAD_X);
                let measured = crate::markdown::measure_text(ui, body, wrap);
                let mut md_h = 0.0;
                ui.allocate_ui(egui::vec2(wrap, 800.0), |ui| {
                    let r = ui
                        .scope(|ui| {
                            ui.set_max_width(wrap);
                            crate::markdown::show(ui, body);
                        })
                        .response;
                    md_h = r.rect.height();
                });
                let resp = super::paint_speech_bubble(ui, body, false, true);
                assert!(
                    md_h > measured.y + 2.0,
                    "fixture must be taller as markdown than plain measure: md {md_h} plain {}",
                    measured.y
                );
                assert!(
                    resp.rect.height() + 2.0 >= md_h,
                    "markdown bubble clipped: painted {} markdown {}",
                    resp.rect.height(),
                    md_h
                );
            });
        });
    }

    #[test]
    fn long_user_bubble_stays_inside_the_row() {
        with_fonts_ui(|ui| {
            ui.allocate_ui(egui::vec2(480.0, 400.0), |ui| {
                ui.set_max_width(480.0);
                let row = ui.max_rect();
                let body = "/very/long/path/to/grokhub/lib/systemd/status\" 2>/dev/null && echo ok "
                    .repeat(6);
                let resp = super::paint_speech_bubble(ui, &body, true, false);
                assert!(
                    resp.rect.min.x + 0.5 >= row.min.x,
                    "user bubble clipped off the left: {} < {}",
                    resp.rect.min.x,
                    row.min.x
                );
                assert!(
                    resp.rect.max.x <= row.max.x + 1.0,
                    "user bubble overflowed the right: {} > {}",
                    resp.rect.max.x,
                    row.max.x
                );
                assert!(
                    resp.rect.width() <= grokhub_core::bubble_max_width(480.0) + 8.0,
                    "bubble {}",
                    resp.rect.width()
                );
            });
        });
    }

    #[test]
    fn chat_blocks_offer_copy_and_reply() {
        let src = include_str!("app.rs");
        let start = src.find("fn paint_msg_acts").expect("paint_msg_acts");
        let slice = &src[start..start + 2800];
        assert!(slice.contains("Copy"), "{slice}");
        assert!(slice.contains("Reply"), "{slice}");
        assert!(slice.contains("fn paint_chat_block"), "{slice}");
        assert!(src.contains("ChatBlockAct::Copy"));
        assert!(src.contains("ChatBlockAct::Reply"));
        assert!(src.contains("quote_for_reply"));
        assert!(src.contains("composer_want_focus"));
        assert!(src.contains("copy_text"));
        let bubble = src.find("fn paint_speech_bubble").expect("speech bubble");
        let bubble_fn = &src[bubble..bubble + 1800];
        assert!(
            !bubble_fn.contains("vec2(row_w, 0.0)"),
            "a zero-height row clips the thread: {bubble_fn}"
        );
        assert!(
            !bubble_fn.contains("set_clip_rect"),
            "clip_rect on the row hides wrapped text: {bubble_fn}"
        );
        assert!(
            !bubble_fn.contains("right_to_left"),
            "RTL user rows clip long lines off the left: {bubble_fn}"
        );
        assert!(
            !bubble_fn.contains("row_h"),
            "a measured-height lock clips markdown: {bubble_fn}"
        );
        let chat = src
            .split("fn ui_chat(")
            .nth(1)
            .and_then(|s| s.split("fn ui_empty_home(").next())
            .expect("ui_chat");
        assert!(
            chat.contains("available_width") && chat.contains("set_max_width(pane)"),
            "thread uses the CentralPanel pane: {chat}"
        );
        assert!(
            !chat.contains("composer_pill_w"),
            "bubbles must not lock to the composer pill: {chat}"
        );
        assert!(
            chat.contains("cached_chat_views") && !chat.contains("visible_chat(&pairs)"),
            "idle chat must not clone the whole transcript every paint: {chat}"
        );
    }

    #[test]
    fn cached_chat_views_do_not_clone_the_thread_on_stream_delta() {
        let src = include_str!("app.rs");
        let cache = src
            .split("fn cached_chat_views(")
            .nth(1)
            .and_then(|s| s.split("fn ui_chat(").next())
            .expect("cached_chat_views");
        assert!(
            !cache.contains("m.1.clone()") && !cache.contains("role.clone()"),
            "a stream delta must not clone every message to rebuild chat views: {cache}"
        );
        assert!(
            cache.contains("refresh_last_stretch") || cache.contains("visible_chat_refs"),
            "last-message growth must refresh the trailing stretch without a full transcript clone: {cache}"
        );
        let voice = src
            .split("fn poll_voice(")
            .nth(1)
            .and_then(|s| s.split("fn poll_tray(").next())
            .expect("poll_voice");
        assert!(
            voice.contains("fold_stream_fields") && !voice.contains("content.clone()"),
            "a voice token must not clone an 8MB transcript to append a delta: {voice}"
        );
        assert!(
            voice.contains("persist_idle_key") && voice.contains("self.persist()"),
            "a live voice delta must not clone every thread 2s later — bump the idle key so persist_bg skips: {voice}"
        );
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

    #[test]
    fn about_paints_the_version() {
        let src = include_str!("app.rs");
        let impl_src = src.split("#[cfg(test)]").next().unwrap_or(src);
        let about = impl_src
            .split("SettingsSec::About => {")
            .nth(1)
            .expect("about body");
        let about = about.split("if let Some(s) = next_sec").next().unwrap_or(about);
        assert!(
            about.contains("CARGO_PKG_VERSION"),
            "About must show grokhub --version: {about}"
        );
        assert!(
            about.contains("FONT_HEADING"),
            "version is a heading, not a muted note: {about}"
        );
    }

    #[test]
    fn settings_drops_cabin_tabs() {
        let src = include_str!("app.rs");
        let settings = src
            .split("fn ui_settings(")
            .nth(1)
            .and_then(|s| s.split("fn add_automation_seed(").next())
            .expect("ui_settings");
        assert!(
            !settings.contains("section_label(ui, \"Cabin\")"),
            "Cabin group is gone from Settings: {settings}"
        );
        assert!(
            !settings.contains("Cabin eyes"),
            "Cabin eyes toggle is gone: {settings}"
        );
        assert!(
            !settings.contains("(SettingsSec::Host, \"Host\")"),
            "Host is not a Settings tab: {settings}"
        );
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
    fn always_permission_keeps_the_acp_session() {
        let src = include_str!("app.rs");
        let ask = src
            .split("fn paint_perm_ask(")
            .nth(1)
            .and_then(|s| s.split("fn ui_empty_home").next())
            .expect("paint_perm_ask");
        assert!(
            !ask.contains("self.acp = None"),
            "Always on a live prompt must not drop the ACP session: {ask}"
        );
        let poll = src
            .split("fn poll_acp(")
            .nth(1)
            .and_then(|s| s.split("fn finish_acp_turn").next())
            .expect("poll_acp");
        assert!(
            poll.contains("auto_allows()"),
            "Auto permission must answer ACP prompts, not only Always: {poll}"
        );
        assert!(
            poll.contains("answer_permission_always"),
            "Always must answer allow-always, not allow-once: {poll}"
        );
        let always = src
            .split("Slash::AlwaysApprove =>")
            .nth(1)
            .and_then(|s| s.split("Slash::AutoPerm =>").next())
            .expect("AlwaysApprove");
        assert!(
            always.contains("acp_spawn_rx = None"),
            "/always during handshake must drop the in-flight Ask agent: {always}"
        );
        assert!(
            always.contains("grok_session = None"),
            "/always must session/new or Ask vs Always does not take: {always}"
        );
        assert!(
            always.contains("persist_idle_key") && !always.contains("self.persist()"),
            "/always must not clone every thread — bump the idle key so persist_bg skips: {always}"
        );
        let auto = src
            .split("Slash::AutoPerm =>")
            .nth(1)
            .and_then(|s| s.split("Slash::Effort(").next())
            .expect("AutoPerm");
        assert!(
            auto.contains("acp_spawn_rx = None"),
            "/auto during handshake must drop the in-flight Ask agent: {auto}"
        );
        assert!(
            auto.contains("grok_session = None"),
            "/auto must session/new or permission mode does not take: {auto}"
        );
        assert!(
            auto.contains("persist_idle_key") && !auto.contains("self.persist()"),
            "/auto must not clone every thread — bump the idle key so persist_bg skips: {auto}"
        );
        let mode = src
            .split("Slash::Mode(mode)")
            .nth(1)
            .and_then(|s| s.split("Slash::Dream").next())
            .expect("Mode");
        assert!(
            mode.contains("self.persist_cfg()")
                && !mode.contains("self.persist()")
                && !mode.contains("persist_snap"),
            "/mode must not clone every thread just to write app.json: {mode}"
        );
        let effort = src
            .split("Slash::Effort(level)")
            .nth(1)
            .and_then(|s| s.split("Slash::Sessions").next())
            .expect("Effort");
        assert!(
            effort.contains("self.persist_cfg()")
                && !effort.contains("self.persist()")
                && !effort.contains("persist_snap"),
            "/effort must not clone every thread just to write app.json: {effort}"
        );
        let appearance = src
            .split("SettingsSec::Appearance => {")
            .nth(1)
            .and_then(|s| s.split("SettingsSec::Behavior => {").next())
            .expect("Appearance");
        assert!(
            appearance.contains("self.persist_cfg()")
                && !appearance.contains("save = true")
                && !appearance.contains("self.persist()")
                && !appearance.contains("persist_snap"),
            "Appearance must not clone every thread just to write app.json: {appearance}"
        );
        let behavior = src
            .split("SettingsSec::Behavior => {")
            .nth(1)
            .and_then(|s| s.split("SettingsSec::Host => {").next())
            .expect("Behavior");
        assert!(
            behavior.contains("self.persist_cfg()")
                && !behavior.contains("save = true")
                && !behavior.contains("self.persist()")
                && !behavior.contains("persist_snap"),
            "Close to tray must not clone every thread just to write app.json: {behavior}"
        );
        let imagine_sec = src
            .split("SettingsSec::Imagine => {")
            .nth(1)
            .and_then(|s| s.split("SettingsSec::Voice => {").next())
            .expect("Imagine settings");
        assert!(
            imagine_sec.contains("self.persist_cfg()")
                && !imagine_sec.contains("save = true")
                && !imagine_sec.contains("self.persist()")
                && !imagine_sec.contains("persist_snap"),
            "Living wall must not clone every thread just to write app.json: {imagine_sec}"
        );
        let plan = src
            .split("Slash::Plan =>")
            .nth(1)
            .and_then(|s| s.split("Slash::AlwaysApprove =>").next())
            .expect("Plan");
        assert!(
            plan.contains("acp_spawn_rx = None"),
            "/plan during handshake must drop the in-flight Ask agent: {plan}"
        );
        assert!(
            plan.contains("grok_session = None"),
            "/plan must session/new or Chat vs Plan does not take: {plan}"
        );
        assert!(
            plan.contains("persist_idle_key") && !plan.contains("self.persist()"),
            "/plan must not clone every thread — bump the idle key so persist_bg skips: {plan}"
        );
        assert!(
            plan.contains("halt_in_flight"),
            "/plan mid-turn must halt or Thinking sticks after the agent is dropped: {plan}"
        );
        let row = src
            .split("let row = crate::cards::session_row")
            .nth(1)
            .and_then(|s| s.split("ui.allocate_ui_with_layout").next())
            .expect("session_row");
        assert_eq!(
            row.matches("acp_spawn_rx = None").count(),
            2,
            "session/permission row must drop an in-flight handshake: {row}"
        );
        assert_eq!(
            row.matches("grok_session = None").count(),
            2,
            "session/permission row must session/new so mode takes: {row}"
        );
        assert_eq!(
            row.matches("persist_idle_key").count(),
            2,
            "session/permission row must not clone every thread — bump the idle key so persist_bg skips: {row}"
        );
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
        let src = include_str!("app.rs");
        let live = src
            .split("let live = wants_live_repaint(")
            .nth(1)
            .and_then(|s| s.split("ctx.request_repaint_after").next())
            .expect("wants_live_repaint call");
        assert!(
            live.contains("grok_sessions_rx")
                && live.contains("persist_rx")
                && live.contains("inspect_rx")
                && live.contains("history_rx")
                && live.contains("mem_restore_rx")
                && live.contains("mem_file_rx")
                && live.contains("recall_rx")
                && live.contains("sync_rx")
                && live.contains("inhabit_rx")
                && live.contains("reflect_rx")
                && live.contains("session_show_rx")
                && live.contains("import_rx")
                && live.contains("acp_spawn_rx")
                && live.contains("recipe_desk_rx")
                && live.contains("host_diff_rx")
                && live.contains("pick_rx")
                && live.contains("pick_list_rx")
                && live.contains("oauth_start_rx")
                && live.contains("oauth_poll_rx")
                && live.contains("greeting_busy")
                && live.contains("greeting_files_rx")
                && live.contains("night_check_rx")
                && live.contains("eyes_cap_rx")
                && live.contains("doctor_line_busy"),
            "History listing / inspect / greeting / night check / Eyes capture / plus-upload / Settings doctor must not wait on the 15s heartbeat: {live}"
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
            src.contains("StartDrag") && src.contains("titlebar_should_start_drag"),
            "undecorated cabin must drag from the titlebar body"
        );
        assert!(
            src.contains("force_x11_for_close_to_tray")
                || include_str!("main.rs").contains("force_x11_for_close_to_tray"),
            "winit 0.30 must drop WAYLAND_DISPLAY so × can unmap"
        );
        assert!(
            src.contains("hidden_window_tick"),
            "a pinned taskbar click must raise the hidden cabin instead of re-unmapping it"
        );
        let hide = src
            .split("fn unmap_to_tray")
            .nth(1)
            .and_then(|s| s.split("fn ensure_tray_spawn").next())
            .expect("unmap_to_tray");
        assert!(
            hide.contains("tray_saw_unfocused = false"),
            "× must clear the focus-raise latch so the next focused frame does not map the cabin: {hide}"
        );
        assert!(
            hide.contains("persist_if_dirty") && !hide.contains("self.persist()"),
            "hide to tray must not clone every thread when idle persist already wrote: {hide}"
        );
        let tick = src
            .split("hidden_window_tick(")
            .nth(1)
            .and_then(|s| s.split("match").next())
            .expect("hidden_window_tick call");
        assert!(
            tick.contains("tray_saw_unfocused"),
            "taskbar raise waits until the hidden cabin actually lost focus: {tick}"
        );
        assert!(
            src.contains("hidden_raise_ready") && src.contains("reapply_unmap"),
            "× must not flash back from a FocusLost/FocusGained bounce or Visible(false) spam"
        );
        let night_save = src
            .split("if let Some(mut a) = parse_nl_automation(&scan)")
            .nth(1)
            .and_then(|s| s.split("if let Some(q) = parse_consult").next())
            .expect("chat night save");
        assert!(
            src.contains("user_asked_to_schedule")
                && src.contains("chat_may_save_automation"),
            "ordinary replies that mention every day at / heartbeat every must not become live night jobs"
        );
        let _ = night_save;
        assert!(
            night_save.contains("persist_idle_key") && !night_save.contains("self.persist()"),
            "chat night save must not clone every thread 2s later — bump the idle key so persist_bg skips: {night_save}"
        );
        assert!(
            src.contains("ignore_close_while_hidden"),
            "sticky close_requested must not hide the cabin after a taskbar raise"
        );
        assert!(
            include_str!("main.rs").contains("try_claim_cabin"),
            "a second grokhub from the taskbar must raise the running cabin and exit"
        );
        assert!(
            include_str!("app.rs").contains("honor_cabin_raise(self.want_quit)"),
            "Restart must not CancelClose when a sibling spawn writes cabin.raise"
        );
        assert!(
            show.contains("CancelClose"),
            "Show cabin must clear a sticky close so the window does not hide again"
        );
        let restart = src
            .split("fn restart_after_update")
            .nth(1)
            .and_then(|s| s.split("fn start_overlay_update").next())
            .expect("restart_after_update");
        let spawn_at = restart.find("restart_system").expect("restart_system");
        let drop_at = restart.find("drop_off_thread");
        assert!(
            drop_at.is_none_or(|d| d > spawn_at),
            "dropping the tray before spawn leaves a headless cabin when restart fails: {restart}"
        );
    }

    #[test]
    fn ui_date_spawns_must_time_out() {
        let src = include_str!("app.rs");
        let date = src
            .split("fn date_out(")
            .nth(1)
            .and_then(|s| s.split("\n    fn local_clock()").next())
            .expect("date_out");
        assert!(
            date.contains("run_limited("),
            "date_out must kill a hung date: {date}"
        );
        let clock = src
            .split("fn local_clock()")
            .nth(1)
            .and_then(|s| s.split("\n    fn local_day()").next())
            .expect("local_clock");
        assert!(
            clock.contains("date_out(") && !clock.contains(".output()"),
            "local_clock must use the timed date helper: {clock}"
        );
        assert!(
            clock.contains("CLOCK_TTL"),
            "chips and greeting must not spawn date on every paint: {clock}"
        );
        assert!(
            clock.contains("thread::spawn") && clock.contains("inflight"),
            "stale date must refresh off the UI thread: {clock}"
        );
        let day = src
            .split("fn local_day()")
            .nth(1)
            .and_then(|s| s.split("\n    fn tick_heartbeat").next())
            .expect("local_day");
        assert!(
            day.contains("date_out(") && !day.contains(".output()"),
            "local_day must use the timed date helper: {day}"
        );
        assert!(
            day.contains("thread::spawn") && day.contains("inflight"),
            "stale local_day must refresh off the UI thread: {day}"
        );
        let roll = src
            .split("fn roll_today(")
            .nth(1)
            .and_then(|s| s.split("\n    fn ui_settings_menu").next())
            .expect("roll_today");
        assert!(
            roll.contains("local_day(") && !roll.contains(".output()"),
            "roll_today must reuse the cached day, not spawn date on the UI thread: {roll}"
        );
        assert!(
            roll.contains("persist_usage")
                && roll.contains("persist_idle_key")
                && !roll.contains("self.persist()"),
            "day rollover must not clone every thread just to write usage.json: {roll}"
        );
    }

    #[test]
    fn persist_does_not_hold_hub_lock_across_disk() {
        let src = include_str!("app.rs");
        let persist = src
            .split("fn persist(&mut self)")
            .nth(1)
            .and_then(|s| s.split("\n    fn sync_hub_voice").next())
            .expect("persist");
        assert!(
            persist.contains("self.hub.clone()") && !persist.contains("state_for_disk"),
            "persist must not clone hub snapshot/last_frame on the UI thread: {persist}"
        );
        assert!(
            !persist.contains("if let Ok(st) = self.hub.lock()"),
            "persist must not hold hub.lock() across save_hub_state: {persist}"
        );
        let write = src
            .split("fn write_persist_disk(")
            .nth(1)
            .and_then(|s| s.split("pub struct Cabin").next())
            .expect("write_persist_disk");
        let lock = write.find("hub.lock").expect("worker hub lock");
        let save = write.find("save_hub_state").expect("save_hub_state");
        assert!(
            lock < save && write.contains("state_for_disk(&st)"),
            "persist worker must clone hub state then drop the lock before hub-state.json: {write}"
        );
    }

    #[test]
    fn greeting_and_chips_use_grok_cli() {
        let src = include_str!("app.rs");
        let greet = src
            .split("fn spawn_greeting_llm(")
            .nth(1)
            .and_then(|s| s.split("fn poll_goals(").next())
            .expect("spawn_greeting_llm");
        assert!(
            greet.contains("cabin_fast_llm") && greet.contains("find_grok"),
            "greeting Fast must run through grok -p when cabin OAuth is empty: {greet}"
        );
        let fast = src
            .split("fn cabin_fast_llm(")
            .nth(1)
            .and_then(|s| s.split("fn mode_status_line(").next())
            .expect("cabin_fast_llm");
        assert!(
            fast.contains("CABIN_FAST_MODEL") && fast.contains("grok_cli_key"),
            "chips/greeting Fast is Grok 4.1 via grok login: {fast}"
        );
        assert!(
            fast.contains("CABIN_FAST_FALLBACK"),
            "chips/greeting Fast must fall back if 4.1 Fast is empty: {fast}"
        );
        let chips = src
            .split("fn spawn_chip_llm(")
            .nth(1)
            .and_then(|s| s.split("fn apply_chip(").next())
            .expect("spawn_chip_llm");
        assert!(
            chips.contains("cabin_fast_llm") && chips.contains("find_grok"),
            "chips Fast must run through grok -p when cabin OAuth is empty: {chips}"
        );
        let ready = src
            .split("fn llm_ready(")
            .nth(1)
            .and_then(|s| s.split("fn grok_cwd(").next())
            .expect("llm_ready");
        assert!(
            ready.contains("find_grok"),
            "llm_ready must count the Grok Build CLI: {ready}"
        );
        let chip = src
            .split("fn apply_chip(")
            .nth(1)
            .and_then(|s| s.split("fn nav_from_id").next())
            .expect("apply_chip");
        let chip_spawn = chip.find("thread::spawn").expect("chip save must leave the UI thread");
        let chip_save = chip.find("save_chips").expect("save_chips");
        assert!(
            chip_spawn < chip_save && chip.contains("persist_io"),
            "chip click must not freeze the cabin writing chips.json: {chip}"
        );
    }

    #[test]
    fn grok_login_powers_history_and_imagine() {
        let src = include_str!("app.rs");
        let ensure = src
            .split("fn ensure_acp(")
            .nth(1)
            .and_then(|s| s.split("fn open_plus(").next())
            .expect("ensure_acp");
        assert!(
            ensure.contains("grok_session") && ensure.contains("session_id"),
            "new ACP sessions must bind onto the cabin thread: {ensure}"
        );
        assert!(
            ensure.contains("h.session_id == id"),
            "ACP reuse is exact session id, never substring and never a blank new chat: {ensure}"
        );
        assert!(
            ensure.contains("explain_handshake_error") && ensure.contains("spawn(None)"),
            "a dead grok session id must retry session/new without resume: {ensure}"
        );
        assert!(
            ensure.contains("is_session_cwd_error") && ensure.contains("t.grok_cwd"),
            "session/load in a foreign worktree must fail closed, not spawn(None) into the bound tree: {ensure}"
        );
        assert!(
            ensure.contains("unknown_cwd"),
            "a History file-only session must not spawn(None) into the bound tree: {ensure}"
        );
        assert!(
            ensure.contains("session/load refused") && ensure.contains("no worktree"),
            "a History file-only session must not session/load into the bound tree: {ensure}"
        );
        assert!(
            ensure.contains("chat_job_thread"),
            "ACP handshake must bind the job thread, not whichever tab is visible: {ensure}"
        );
        let ensure_spawn = ensure.find("thread::spawn").expect("handshake must leave the UI thread");
        let ensure_sess = ensure.find("spawn_session").expect("spawn_session");
        assert!(
            ensure_spawn < ensure_sess,
            "ACP handshake must not freeze the cabin: {ensure}"
        );
        assert!(
            !ensure.contains("bearer()"),
            "ACP spawn must not pass Imagine bearer (JWT) as XAI_API_KEY: {ensure}"
        );
        assert!(
            ensure.contains("console_key") && ensure.contains("grok_cli_key") && ensure.contains("xai_env"),
            "ACP auth is grok login; XAI_API_KEY is the secrets console key: {ensure}"
        );
        let bearer = src
            .split("fn bearer(")
            .nth(1)
            .and_then(|s| s.split("fn switch_thread(").next())
            .expect("bearer");
        assert!(
            bearer.contains("grok_cli_key") && bearer.find("grok_cli_key").unwrap() < bearer.find("oauth_usable").unwrap_or(usize::MAX),
            "Imagine/ACP bearer prefers grok login over cabin OAuth: {bearer}"
        );
        assert!(
            bearer.contains("refresh_grok_login"),
            "grok login JWT must refresh before Imagine 401s: {bearer}"
        );
        assert!(
            bearer.contains("} else {") && bearer.contains("return k;"),
            "a dead grok login JWT must fall through to console key, not keep the expired token: {bearer}"
        );
        assert!(
            bearer.contains("hard_expired"),
            "skew-stale grok login must still be used while refresh is off the UI thread: {bearer}"
        );
        assert!(
            bearer.contains("refresh_cabin_oauth") && !bearer.contains("ensure_access"),
            "cabin OAuth refresh HTTP must leave the UI thread: {bearer}"
        );
        assert!(
            bearer.contains("console_key()"),
            "Imagine/ACP console-key fallback must read secrets.json: {bearer}"
        );
        let disk = src
            .split("fn write_persist_disk(")
            .nth(1)
            .and_then(|s| s.split("pub struct Cabin").next())
            .expect("write_persist_disk");
        assert!(
            disk.contains("secrets::save"),
            "persist must write the console key to secrets.json: {disk}"
        );
        assert!(
            disk.contains("if let Some(s) = &snap.secrets") || disk.contains("snap.secrets"),
            "idle persist must not write secrets.json from a stale snap: {disk}"
        );
        assert!(
            src.contains("migrate_console_key"),
            "boot must move a leftover app.json console key into secrets.json"
        );
        assert!(
            src.contains("&mut self.secrets.api_key"),
            "Settings Console key must edit secrets.json, not app.json"
        );
        let settings_save = src
            .split("fn save_settings")
            .nth(1)
            .and_then(|s| s.split("fn ui_settings").next())
            .expect("save_settings");
        assert!(
            settings_save.contains("api_key.clear"),
            "Settings Save must not keep a leftover console key on cfg: {settings_save}"
        );
        assert!(
            settings_save.contains("self.persist()") && !settings_save.contains("secrets::save"),
            "Settings Save must not freeze the cabin writing secrets.json: {settings_save}"
        );
        assert!(
            settings_save.contains("self.persist_cfg()")
                && settings_save.contains("self.flush_projects()")
                && settings_save.contains("self.persist_hub()")
                && settings_save.contains("self.persist_secrets()")
                && settings_save.contains("tree_changed"),
            "Settings Save must not clone every thread when the worktree did not change: {settings_save}"
        );
        let persist_if = src
            .split("fn persist_if_dirty")
            .nth(1)
            .and_then(|s| s.split("fn persist_secrets").next())
            .expect("persist_if_dirty");
        assert!(
            persist_if.contains("persist_idle_key")
                && persist_if.contains("persist_cfg")
                && persist_if.contains("self.persist()"),
            "hide/quit must skip the thread clone when idle persist already wrote: {persist_if}"
        );
        let persist_secrets = src
            .split("fn persist_secrets(")
            .nth(1)
            .and_then(|s| s.split("fn persist_usage").next())
            .expect("persist_secrets");
        let secrets_spawn = persist_secrets
            .find("thread::spawn")
            .expect("secrets write must leave the UI thread");
        let secrets_save = persist_secrets.find("secrets::save").expect("secrets::save");
        assert!(
            secrets_spawn < secrets_save
                && persist_secrets.contains("persist_io")
                && !persist_secrets.contains("self.persist()"),
            "Settings Save must not freeze the cabin writing secrets.json: {persist_secrets}"
        );
        let persist_usage = src
            .split("fn persist_usage(")
            .nth(1)
            .and_then(|s| s.split("fn poll_sync").next())
            .expect("persist_usage");
        let usage_spawn = persist_usage
            .find("thread::spawn")
            .expect("usage write must leave the UI thread");
        let usage_save = persist_usage.find("save_usage").expect("save_usage");
        assert!(
            usage_spawn < usage_save
                && persist_usage.contains("persist_io")
                && !persist_usage.contains("self.persist()"),
            "night usage must not freeze the cabin writing usage.json: {persist_usage}"
        );
        let bg = src
            .split("fn persist_idle_now(")
            .nth(1)
            .and_then(|s| s.split("\n    fn poll_persist").next())
            .expect("persist_bg");
        assert!(
            !bg.contains("secrets.api_key.len"),
            "idle persist must not race a just-saved console key: {bg}"
        );
        assert!(
            bg.contains("grok_session") && bg.contains("grok_cwd"),
            "idle persist must notice a handshake session stamp: {bg}"
        );
        let snap = src
            .split("fn persist_snap(")
            .nth(1)
            .and_then(|s| s.split("fn persist_bg(").next())
            .expect("persist_snap");
        assert!(
            snap.contains("secrets: None"),
            "idle persist_snap must omit secrets.json: {snap}"
        );
        assert!(
            !snap.contains("msgs.clone()"),
            "persist_snap must copy the live pane into the thread once, not again into PersistSnap.msgs: {snap}"
        );
        assert!(
            snap.contains("t.messages = self.messages.clone()")
                && snap.contains("self.threads.clone()"),
            "persist must share the live pane Arc, then bump other threads: {snap}"
        );
        assert!(
            !snap.contains("parked_last") && !snap.contains("live_last"),
            "persist_snap must not recopy bodies when the live pane already is the parked Arc: {snap}"
        );
        assert!(
            snap.contains("self.hub.clone()") && !snap.contains("state_for_disk"),
            "persist_snap must not clone hub last_frame/snapshot on the UI thread: {snap}"
        );
        assert!(
            disk.contains("current_thread") && disk.contains("save_chat"),
            "persist must write chat.json from the snapped thread, not a second 8MB clone: {disk}"
        );
        let persist = src
            .split("fn persist(&mut self)")
            .nth(1)
            .and_then(|s| s.split("fn persist_snap(").next())
            .expect("persist");
        assert!(
            persist.contains("snap.secrets = Some") && persist.contains("persist_io"),
            "foreground persist must write secrets under persist_io: {persist}"
        );
        let persist_spawn = persist.find("thread::spawn").expect("persist must leave the UI thread");
        let persist_write = persist.find("write_persist_disk").expect("persist writes");
        assert!(
            persist_spawn < persist_write && persist.contains("io.lock()"),
            "foreground persist must not freeze the cabin writing threads.json: {persist}"
        );
        assert!(
            persist.contains("persist_idle_key") && persist.contains("persist_idle_now"),
            "persist must bump the idle key or persist_bg clones every thread again 2s later: {persist}"
        );
        assert!(
            bearer.contains("persist_io") && bearer.contains("secrets::save"),
            "OAuth refresh must take persist_io before writing secrets.json: {bearer}"
        );
        let bearer_spawn = bearer.find("thread::spawn").expect("oauth persist must leave the UI thread");
        let bearer_save = bearer.find("secrets::save").expect("oauth persist writes");
        assert!(
            bearer_spawn < bearer_save,
            "OAuth refresh must not freeze the cabin writing secrets.json: {bearer}"
        );
        let kick = src
            .split("fn kick_model(")
            .nth(1)
            .and_then(|s| s.split("fn upsert_stream_assistant").next())
            .expect("kick_model");
        assert!(
            kick.contains("prompt_with_image") && kick.contains("next_chat_image"),
            "a plus-button still must ride the ACP prompt: {kick}"
        );
        assert!(
            kick.contains("consume_attach") && kick.contains("attach_url"),
            "follow-up kicks must leave the attached image for the next send: {kick}"
        );
        let prompt = kick.find("prompt_with_image").expect("prompt_with_image");
        let consume = kick.find("self.attach_url = None").expect("consume attach");
        assert!(
            prompt < consume && kick.contains("Some(Ok(()))"),
            "a plus-button still must survive a failed ACP prompt: {kick}"
        );
        let send_attach = src
            .split("fn send_chat(")
            .nth(1)
            .and_then(|s| s.split("fn send_followup_turn").next())
            .expect("send_chat attach");
        assert!(
            send_attach.contains("attach_prompt_line") && send_attach.contains("attach_name"),
            "the visible user turn must mention the attached still: {send_attach}"
        );
        let cwd = src
            .split("fn grok_cwd(")
            .nth(1)
            .and_then(|s| s.split("fn reload_grok_sessions(").next())
            .expect("grok_cwd");
        assert!(
            cwd.contains("resolve_acp_cwd") && cwd.contains("work_root"),
            "ACP cwd must be the bound project or ~/GrokHub-Work, not the cabin process cwd: {cwd}"
        );
        assert!(
            !cwd.contains("current_dir"),
            "unbound ACP must not inherit the overlay or cargo tree cwd: {cwd}"
        );
        assert!(
            !cwd.contains("ensure_session_cwd"),
            "ACP cwd lookup must not probe disk on the UI thread: {cwd}"
        );
        let inspect = src
            .split("Slash::Inspect =>")
            .nth(1)
            .and_then(|s| s.split("Slash::ProjectBind").next())
            .expect("inspect");
        assert!(
            inspect.contains("grok_cwd") && !inspect.contains("current_dir"),
            "/inspect must use the bound tree or work root, not the cabin process cwd: {inspect}"
        );
        let inspect_spawn = inspect.find("thread::spawn").expect("inspect must leave the UI thread");
        let inspect_json = inspect.find("inspect_json").expect("inspect_json");
        assert!(
            inspect_spawn < inspect_json,
            "/inspect must not block the cabin on grok inspect: {inspect}"
        );
        let bind = src
            .split("Slash::ProjectBind(path)")
            .nth(1)
            .and_then(|s| s.split("Slash::ProjectClear").next())
            .expect("project bind");
        assert!(
            bind.contains("resolve_bind_path"),
            "/project bind . must not inherit the cabin process cwd: {bind}"
        );
        assert!(
            bind.contains("acp_spawn_rx = None"),
            "/project bind during handshake must drop the in-flight agent: {bind}"
        );
        assert!(
            bind.contains("halt_in_flight") && !bind.contains("self.acp.is_some()"),
            "/project bind during handshake must halt, not only drop a live ACP handle: {bind}"
        );
        assert!(
            bind.contains("grok_cwd = None") && bind.contains("grok_session = None"),
            "/project bind must forget the thread worktree or the next send stays in a History tree: {bind}"
        );
        assert!(
            bind.contains("self.persist_cfg()")
                && bind.contains("self.flush_projects()")
                && bind.contains("self.persist()")
                && bind.contains("tree_changed"),
            "/project bind to the current tree must not clone every thread: {bind}"
        );
        let clear = src
            .split("Slash::ProjectClear =>")
            .nth(1)
            .and_then(|s| s.split("Slash::ProjectShow =>").next())
            .expect("ProjectClear handshake");
        assert!(
            clear.contains("acp_spawn_rx = None"),
            "/project clear during handshake must drop the in-flight agent: {clear}"
        );
        assert!(
            clear.contains("halt_in_flight") && !clear.contains("self.acp.is_some()"),
            "/project clear during handshake must halt, not only drop a live ACP handle: {clear}"
        );
        assert!(
            clear.contains("grok_cwd = None") && clear.contains("grok_session = None"),
            "/project clear must forget the thread worktree or the next send stays in a History tree: {clear}"
        );
        let sidebar = src
            .split("fn bind_project_id(")
            .nth(1)
            .and_then(|s| s.split("fn make_project(").next())
            .expect("bind_project_id");
        assert!(
            sidebar.contains("acp_spawn_rx = None") && sidebar.contains("self.acp = None"),
            "sidebar bind during handshake must drop the in-flight agent: {sidebar}"
        );
        assert!(
            sidebar.contains("halt_in_flight") && !sidebar.contains("self.acp.is_some()"),
            "sidebar bind during handshake must halt, not only drop a live ACP handle: {sidebar}"
        );
        assert!(
            sidebar.contains("grok_cwd = None") && sidebar.contains("grok_session = None"),
            "sidebar bind must forget the thread worktree or the next send stays in a History tree: {sidebar}"
        );
        let bind_spawn = sidebar.find("thread::spawn").expect("bind mkdir must leave the UI thread");
        let bind_mkdir = sidebar.find("create_dir_all").expect("create_dir_all");
        assert!(
            bind_spawn < bind_mkdir,
            "sidebar bind must not freeze the cabin creating the project folder: {sidebar}"
        );
        assert!(
            sidebar.contains("self.persist_cfg()")
                && sidebar.contains("self.persist()")
                && sidebar.contains("tree_changed"),
            "re-clicking a bound project must not clone every thread just to write app.json: {sidebar}"
        );
        let room = src
            .split("Slash::Room(name)")
            .nth(1)
            .and_then(|s| s.split("Slash::Export =>").next())
            .expect("Room");
        assert!(
            room.contains("acp_spawn_rx = None"),
            "/room during handshake must drop the in-flight agent: {room}"
        );
        assert!(
            room.contains("halt_in_flight") && !room.contains("self.acp.is_some()"),
            "/room during handshake must halt, not only drop a live ACP handle: {room}"
        );
        assert!(
            room.contains("grok_cwd = None") && room.contains("grok_session = None"),
            "/room must forget the thread worktree or the next send stays in a History tree: {room}"
        );
        assert!(
            room.contains("self.persist_cfg()")
                && room.contains("self.flush_projects()")
                && room.contains("self.persist()")
                && room.contains("tree_changed"),
            "/room to the current tree must not clone every thread: {room}"
        );
        let ext = src
            .split("fn run_grok_extension(")
            .nth(1)
            .and_then(|s| s.split("fn doctor_text(").next())
            .expect("run_grok_extension");
        assert!(
            ext.contains("grok_cwd") && !ext.contains("current_dir"),
            "Connectors inspect must use grok_cwd, not `.`: {ext}"
        );
        let ext_spawn = ext.find("thread::spawn").expect("extension must leave the UI thread");
        let ext_out = ext.find("grok_stdout").expect("grok_stdout");
        assert!(
            ext_spawn < ext_out,
            "Connectors inspect/mcp/plugin must not freeze the cabin: {ext}"
        );
        let fast = src
            .split("fn cabin_fast_llm(")
            .nth(1)
            .and_then(|s| s.split("fn mode_status_line(").next())
            .expect("cabin_fast_llm");
        assert!(
            fast.contains("resolve_acp_cwd") && !fast.contains("current_dir"),
            "grok -p fallback must not inherit the overlay cwd: {fast}"
        );
        let open = src
            .split("fn open_grok_session(")
            .nth(1)
            .and_then(|s| s.split("fn ensure_acp(").next())
            .expect("open_grok_session");
        assert!(
            open.contains("show_session") && open.contains("read_file_capped"),
            "opening a grok session must load the transcript: {open}"
        );
        assert!(
            open.contains("read_file_capped") && !open.contains("read_to_string"),
            "opening a grok session must not slurp a huge markdown dump: {open}"
        );
        assert!(
            open.contains("grok_cwd"),
            "History open must remember the session worktree: {open}"
        );
        let open_spawn = open.find("thread::spawn").expect("show_session must leave the UI thread");
        let open_show = open.find("show_session").expect("show_session");
        let open_read = open.find("read_file_capped").expect("read_file_capped");
        let open_find = open.find("find_grok").expect("find_grok");
        assert!(
            open_spawn < open_show && open_spawn < open_read && open_spawn < open_find,
            "opening a grok session must not block on grok sessions show: {open}"
        );
        assert!(
            open.contains("apply_switch_thread") && open.contains("self.persist()"),
            "opening a grok session must not clone every thread twice: {open}"
        );
        let reload = src
            .split("fn reload_grok_sessions(")
            .nth(1)
            .and_then(|s| s.split("fn poll_grok_sessions(").next())
            .expect("reload_grok_sessions");
        let spawn = reload.find("thread::spawn").expect("reload must leave the UI thread");
        let list = reload.find("list_sessions").expect("reload lists grok sessions");
        assert!(
            spawn < list,
            "History must list grok sessions off the UI thread: {reload}"
        );
        let kick = src
            .split("fn kick_imagine(")
            .nth(1)
            .and_then(|s| s.split("fn listen_voice(").next())
            .expect("kick_imagine");
        assert!(
            kick.contains("bearer()") && !kick.contains("has_key()"),
            "Imagine must use grok login, not cabin OAuth only: {kick}"
        );
        assert_eq!(
            kick.matches("bearer()").count(),
            1,
            "Imagine must not refresh grok login twice on the UI thread: {kick}"
        );
        let poll = src
            .split("fn poll_acp(")
            .nth(1)
            .and_then(|s| s.split("fn finish_acp_turn(").next())
            .expect("poll_acp");
        assert!(
            poll.contains("grok_session") && poll.contains("session_id"),
            "ACP Ready must stamp the grok session id: {poll}"
        );
        assert!(
            poll.contains("cancelled") && poll.contains("!self.running"),
            "session/cancel Done must not finish a live or redirected turn: {poll}"
        );
        assert!(
            poll.contains("answer_permission") && poll.contains("AcpEvent::Err"),
            "ACP Err must deny leftover Ask or the next send hangs: {poll}"
        );
        assert!(
            poll.contains("chat_job_thread"),
            "ACP Ready must stamp the job thread, not whichever tab is visible: {poll}"
        );
        let spawn_poll = src
            .split("fn poll_acp_spawn(")
            .nth(1)
            .and_then(|s| s.split("fn open_grok_session(").next())
            .expect("poll_acp_spawn");
        let spawn_ok = spawn_poll
            .split("Ok(Ok(h))")
            .nth(1)
            .and_then(|s| s.split("Ok(Err(e))").next())
            .expect("spawn ok");
        assert!(
            spawn_ok.contains("grok_session") && spawn_ok.contains("self.persist()"),
            "handshake must persist the session id before the first turn: {spawn_ok}"
        );
        assert!(
            spawn_ok.contains("chat_job_thread"),
            "handshake stamp must follow the job thread, not whichever tab is visible: {spawn_ok}"
        );
        let spawn_drop = spawn_poll
            .split("TryRecvError::Disconnected")
            .nth(1)
            .and_then(|s| s.split("fn open_grok_session").next())
            .expect("spawn disconnected");
        assert!(
            spawn_drop.contains("apply_job_fail") && spawn_drop.contains("self.persist()"),
            "a dropped handshake must persist the fail turn or persist_bg waits 2s: {spawn_drop}"
        );
        let show = src
            .split("fn poll_session_show(")
            .nth(1)
            .and_then(|s| s.split("fn poll_acp_spawn(").next())
            .expect("poll_session_show");
        assert!(
            show.contains("persist_bg") && show.contains("parse_session_markdown"),
            "History show must persist the transcript, not wait for the next idle tick: {show}"
        );
    }

    #[test]
    fn refresh_chips_does_not_rebuild_every_frame() {
        let src = include_str!("app.rs");
        let chips = src
            .split("fn refresh_chips(")
            .nth(1)
            .and_then(|s| s.split("fn spawn_chip_llm(").next())
            .expect("refresh_chips");
        assert!(
            chips.contains("chip_paint_key") && chips.contains("return;"),
            "chips must not clone the transcript and walk other threads on every paint: {chips}"
        );
        let pairs = chips.find("chat_pairs").expect("chip chat_pairs");
        assert!(
            chips[..pairs].contains("self.running") && chips[..pairs].contains("return"),
            "a growing stream must not clone the transcript to rebuild chips: {chips}"
        );
        assert!(
            chips.contains("chip_chat_pairs") || chips.contains("chip_scan"),
            "chip rebuild must not clone an 8MB complete into chat_pairs: {chips}"
        );
        assert!(
            chips.contains("host_on: false"),
            "empty chips must not inject HOST_CMD host_chips: {chips}"
        );
        assert!(
            chips.contains("llm_ready"),
            "chips must treat grok CLI as connected, not only cabin OAuth: {chips}"
        );
    }

    #[test]
    fn speak_reply_does_not_clone_an_8mb_complete() {
        let src = include_str!("app.rs");
        let speak = src
            .split("fn speak_reply(")
            .nth(1)
            .and_then(|s| s.split("fn refresh_eyes(").next())
            .expect("speak_reply");
        let clone = speak.find("to_string()").expect("tts clone");
        assert!(
            speak[..clone].contains("TEXT_FILE_CAP")
                || speak[..clone].contains("chip_scan")
                || speak[..clone].contains("take_ui"),
            "voice speak must not clone an 8MB complete onto the UI thread: {speak}"
        );
    }

    #[test]
    fn periodic_persist_leaves_the_ui_thread() {
        let src = include_str!("app.rs");
        let beat = src
            .split("fn tick_heartbeat")
            .nth(1)
            .and_then(|s| s.split("fn tick_anticipate").next())
            .expect("tick_heartbeat");
        assert!(
            beat.contains("persist_bg(") && !beat.contains("self.persist()"),
            "2s housekeep persist must not block the cabin: {beat}"
        );
        let paint = src
            .split("self.flush_window(ctx)")
            .nth(1)
            .and_then(|s| s.split("next_heartbeat_wait_ms").next())
            .expect("update persist");
        assert!(
            paint.contains("persist_bg(") && !paint.contains("self.persist()"),
            "2s paint persist must not block the cabin: {paint}"
        );
        let flush = src
            .split("fn flush_window(")
            .nth(1)
            .and_then(|s| s.split("fn persist(").next())
            .expect("flush_window");
        let flush_spawn = flush.find("thread::spawn").expect("geom flush must leave the UI thread");
        let flush_save = flush.find("config::save").expect("geom flush writes app.json");
        assert!(
            flush_spawn < flush_save && flush.contains("persist_io"),
            "window geom must not freeze the cabin writing app.json: {flush}"
        );
        let bg = src
            .split("fn persist_bg(")
            .nth(1)
            .and_then(|s| s.split("\n    fn ").next())
            .expect("persist_bg");
        let spawn = bg.find("thread::spawn").expect("persist_bg must leave the UI thread");
        let save = bg
            .find("write_persist_disk")
            .expect("periodic persist must write on the worker");
        assert!(
            spawn < save,
            "periodic persist must write after spawn: {bg}"
        );
        assert!(
            bg.contains("persist_idle_key") && bg.contains("return;"),
            "idle 2s persist must not clone every thread on the UI thread: {bg}"
        );
        let snap = bg.find("persist_snap").expect("persist_snap");
        assert!(
            bg[..snap].contains("self.running") && bg[..snap].contains("return"),
            "a growing stream must not clone every thread to persist an 8MB bubble: {bg}"
        );
        assert!(
            !bg[..snap].contains("geom_dirty"),
            "window drag must not clone every thread — flush_window owns geom: {bg}"
        );
        let idle_key = src
            .split("fn persist_idle_now(")
            .nth(1)
            .and_then(|s| s.split("fn persist_bg(").next())
            .expect("persist idle key");
        assert!(
            !idle_key.contains("projects_dirty"),
            "folder click must not clone every thread twice — persist_idle_key must ignore the dirty flag: {idle_key}"
        );
    }

    #[test]
    fn refresh_eyes_captures_off_the_ui_thread() {
        let src = include_str!("app.rs");
        let eyes = src
            .split("fn refresh_eyes")
            .nth(1)
            .and_then(|s| s.split("fn halt_work").next())
            .expect("refresh_eyes");
        let spawn = eyes
            .find("thread::spawn")
            .expect("Eyes Scan grim must leave the UI thread");
        let shot = eyes.find("capture_data_url").expect("screen capture");
        assert!(
            spawn < shot,
            "Eyes Scan must not block the cabin: {eyes}"
        );
        assert!(
            eyes.contains("lock_titles") && eyes.contains("should_send_screenshot"),
            "Eyes Scan lock gates stay on the UI thread: {eyes}"
        );
    }

    #[test]
    fn chat_capture_leaves_the_ui_thread() {
        let src = include_str!("app.rs");
        let cap = src
            .split("fn capture_cabin_frame_this_turn")
            .nth(1)
            .and_then(|s| s.split("fn apply_job_fail").next())
            .expect("capture_cabin_frame_this_turn");
        let spawn = cap
            .find("thread::spawn")
            .expect("chat grim must leave the UI thread");
        let shot = cap.find("capture_data_url").expect("screen capture");
        let rows = cap.find("collect_rows").expect("desk scan");
        assert!(
            spawn < rows && rows < shot,
            "send/HostDone capture must not block the cabin: {cap}"
        );
        let kick = src
            .split("fn kick_model(")
            .nth(1)
            .and_then(|s| s.split("fn upsert_stream_assistant").next())
            .expect("kick_model");
        assert!(
            kick.contains("ensure_acp") && kick.contains("prompt_with_image"),
            "kick_model must prompt Grok Build over ACP: {kick}"
        );
        assert!(
            kick.contains("apply_job_fail"),
            "session/new failure must land in the chat, not only the 72-char status clip: {kick}"
        );
        assert!(
            kick.contains("pending_kick") && kick.contains("kick_cap_rx") && kick.contains("acp_spawn_rx"),
            "kick_model must wait for the off-thread frame and ACP handshake instead of blocking: {kick}"
        );
    }

    #[test]
    fn plus_upload_does_not_rescan_the_folder_every_frame() {
        let src = include_str!("app.rs");
        let overlay = src
            .split("fn ui_plus_overlays(")
            .nth(1)
            .and_then(|s| s.split("fn ui_imagine_overlays(").next())
            .expect("ui_plus_overlays");
        assert!(
            overlay.contains("cached_pick_entries") && !overlay.contains("Self::pick_entries"),
            "Upload window must not read_dir every paint: {overlay}"
        );
        let cache = src
            .split("fn cached_pick_entries(")
            .nth(1)
            .and_then(|s| s.split("fn ui_plus_overlays(").next())
            .expect("cached_pick_entries");
        assert!(
            cache.contains("pick_cache") && cache.contains("pick_entries("),
            "folder listing must reuse the last scan until pick_dir changes: {cache}"
        );
        let cache_spawn = cache.find("thread::spawn").expect("listing must leave the UI thread");
        let cache_walk = cache.find("pick_entries(").expect("pick_entries");
        assert!(
            cache_spawn < cache_walk,
            "Upload folder listing must not read_dir on the UI thread: {cache}"
        );
        let upload = src
            .split("PlusAct::Upload =>")
            .nth(1)
            .and_then(|s| s.split("PlusAct::Paste =>").next())
            .expect("Upload");
        let spawn = upload.find("thread::spawn").expect("picker worker");
        let pick = upload.find("pick_file()").expect("native picker");
        let load = upload.find("plus_from_path").expect("decode off-thread");
        assert!(
            spawn < pick && pick < load && upload.contains("pick_rx") && !upload.contains("apply_path"),
            "zenity/kdialog and JPEG decode must not freeze the cabin on plus-upload: {upload}"
        );
        let paste = src
            .split("PlusAct::Paste =>")
            .nth(1)
            .and_then(|s| s.split("fn poll_pick(").next())
            .expect("Paste");
        let paste_spawn = paste.find("thread::spawn").expect("clipboard worker");
        let clip = paste.find("clipboard_image()").expect("clipboard image");
        assert!(
            paste_spawn < clip
                && paste.contains("clipboard_once")
                && paste.contains("plus_from_path")
                && !paste.contains("apply_path"),
            "xclip/wl-paste and JPEG decode must not freeze the cabin on plus-paste: {paste}"
        );
        let poll = src
            .split("fn poll_pick(")
            .nth(1)
            .and_then(|s| s.split("fn apply_clipboard(").next())
            .expect("poll_pick");
        assert!(
            poll.contains("apply_plus_ready")
                && poll.contains("file_pick")
                && !poll.contains("load_image_data_url"),
            "plus-upload worker must land the still or fall back to the in-app picker: {poll}"
        );
        assert!(
            src.contains("self.poll_pick()"),
            "plus-upload worker must be polled each frame"
        );
        assert!(
            overlay.contains("start_plus_path") && !overlay.contains("apply_path"),
            "in-app Upload clicks must decode off the UI thread: {overlay}"
        );
    }

    #[test]
    fn recipe_reshoot_leaves_the_ui_thread() {
        let src = include_str!("app.rs");
        let replay = src
            .split("fn replay_recipe(")
            .nth(1)
            .and_then(|s| s.split("fn speak_reply").next())
            .expect("replay_recipe");
        let spawn = replay
            .find("thread::spawn")
            .expect("recipe replay must leave the UI thread");
        let rows = replay.find("collect_rows").expect("desk scan");
        let shot = replay.find("capture_data_url").expect("screen capture");
        assert!(
            spawn < rows && spawn < shot,
            "recipe replay must not block the cabin: {replay}"
        );
        assert!(
            replay.contains("lock_titles") && replay.contains("lock_blocks_hands"),
            "recipe reshoot must still gate on lock windows: {replay}"
        );
    }

    #[test]
    fn live_room_captures_off_the_ui_thread() {
        let src = include_str!("app.rs");
        let live = src
            .split("fn live_room")
            .nth(1)
            .and_then(|s| s.split("fn tick_mid_thought").next())
            .expect("live_room");
        let spawn = live
            .find("thread::spawn")
            .expect("grim/ffmpeg must leave the UI thread");
        let shot = live.find("capture_data_url").expect("screen capture");
        let cam = live.find("capture_webcam").expect("webcam");
        assert!(
            spawn < shot && spawn < cam,
            "presence capture must not block the cabin: {live}"
        );
        assert!(
            live.contains("try_recv") && live.contains("live_cap_rx"),
            "UI thread must apply one in-flight frame without stacking grim: {live}"
        );
        assert!(
            live.contains("collect_rows")
                && live.contains("lock_titles")
                && live.contains("should_send_screenshot"),
            "lock and title gates stay on the UI thread: {live}"
        );
        assert!(
            !live.contains("webcam_url")
                && !live.contains("cam:"),
            "live room must not land an unbounded webcam data URL on the UI thread: {live}"
        );
    }

    #[test]
    fn chat_side_effects_keep_the_origin_thread() {
        let src = include_str!("app.rs");
        assert!(
            src.contains("let origin = self.chat_job_thread.take()"),
            "host/connector/imagine after Chat must rebind the origin tab"
        );
        let agent_job = src
            .split("struct AgentJob")
            .nth(1)
            .and_then(|s| s.split("fn listen_turn").next())
            .expect("AgentJob");
        assert!(
            agent_job.contains("thread_id"),
            "Queue jobs must remember the origin thread: {agent_job}"
        );
        let listen = src
            .split("fn listen_turn(")
            .nth(1)
            .and_then(|s| s.split("fn fit_rail_label").next())
            .expect("listen_turn");
        let wav_read = listen.find("std::fs::read(&wav)").expect("wav read");
        assert!(
            listen.contains("IMAGE_FILE_CAP")
                && listen.find("IMAGE_FILE_CAP").expect("wav cap") < wav_read,
            "voice STT must not slurp a huge wav: {listen}"
        );
        let queue = src
            .split("fn ui_agents")
            .nth(1)
            .and_then(|s| s.split("fn page_nav").next())
            .expect("ui_agents");
        assert!(
            !queue.contains("send_chat")
                && queue.contains("chat_job_thread")
                && queue.contains("push_bound_msg")
                && queue.contains("kick_model"),
            "Queue Run must kick the origin thread, not send_chat on the visible tab: {queue}"
        );
        assert!(
            src.contains("finish_hub_dispatch"),
            "phone dispatch must complete the hub task so GET /v1/results can see it"
        );
        assert!(
            src.contains("hub_dispatch_ok(&text)"),
            "GOAL_BLOCKED must not complete a phone task as done"
        );
        assert!(
            src.contains("visible_goal_step_on_continue"),
            "a background goal continue must not bump the visible tab step"
        );
        assert!(
            src.contains("oauth_access_live"),
            "expired OAuth without refresh must not hide a console key"
        );
        assert!(
            src.contains("next_oauth_poll_secs"),
            "Settings OAuth must honor interval and slow_down"
        );
        let cmds = src
            .split("fn run_cmds")
            .nth(1)
            .and_then(|s| s.split("fn run_connector").next())
            .expect("run_cmds");
        assert!(
            cmds.contains("if self.chat_job_thread.is_none()"),
            "run_cmds must not retarget a job that started on another tab"
        );
        assert!(
            cmds.contains("push_bound_msg"),
            "blocked host receipts must stay on the job thread"
        );
        assert!(
            cmds.contains("host_working_dir(&self.cfg.project_dir)")
                && cmds.contains("run_host_stream"),
            "host shell must start in the bound project, not the cabin process cwd: {cmds}"
        );
        assert!(
            cmds.contains("parse_computer_op") && !cmds.contains("parse_computer_cmd_loose"),
            "HOST_CMD / Command-pane type cargo must stay shell, not desktop type-in: {cmds}"
        );
        let ret = cmds.find("return blocked;").expect("return blocked");
        let rewind = cmds.rfind("is_rewind_copy_cmd").expect("rewind copy");
        assert!(
            ret < rewind && cmds[ret..rewind].contains("self.persist()"),
            "mixed blocked+allowed host must persist block receipts before spawn: {cmds}"
        );
        assert!(
            src.contains("host_needs_kick && !self.running"),
            "an all-blocked host plan must still kick the model after connectors"
        );
        let halt = src
            .split("fn halt_work")
            .nth(1)
            .and_then(|s| s.split("fn drain_inbox").next())
            .expect("halt_work");
        assert!(
            halt.contains("finish_hub_dispatch"),
            "Stop / tray halt must complete a claimed phone task"
        );
        assert!(
            src.contains("self.finish_hub_dispatch(worker_gone_status(), false)"),
            "a dropped worker must fail the claimed phone task"
        );
        assert!(
            src.contains("inbox_claim_ready") && src.contains("requeue_claimed_for"),
            "do not claim a phone task without auth, and unstick claimed rows on boot"
        );
        let inbox = src
            .split("fn drain_inbox")
            .nth(1)
            .and_then(|s| s.split("fn finish_hub_dispatch").next())
            .expect("drain_inbox");
        assert!(
            inbox.contains("pending_hub_task.is_some()"),
            "do not claim a second phone task while one is still pending: {inbox}"
        );
        assert!(
            inbox.contains("land_on_real_chat"),
            "a claimed phone task must not land on Scratch: {inbox}"
        );
        assert!(
            inbox.contains("self.can_agent()") && !inbox.contains("self.llm_ready()"),
            "OAuth-only must not claim a phone task — send_chat needs Grok Build: {inbox}"
        );
        assert!(
            src.contains("night_counts_run"),
            "a night replay that did not start must not consume the slot"
        );
        let fire_night = src
            .split("fn fire_night")
            .nth(1)
            .and_then(|s| s.split("fn tick_review").next())
            .expect("fire_night");
        assert!(
            fire_night.contains("night_unauth_should_skip")
                && fire_night.contains("mark_auto_skipped"),
            "missing OAuth must skip the night slot: {fire_night}"
        );
        let counts = fire_night.find("night_counts_run").expect("night_counts_run");
        assert!(
            fire_night[counts..].contains("mark_auto_skipped"),
            "a missing night recipe must skip the slot, not hammer every 5s: {fire_night}"
        );
        let bump = fire_night.find("bump_usage").expect("night usage");
        let usage_save = fire_night.find("persist_usage").expect("night persist_usage");
        assert!(
            bump < usage_save && !fire_night.contains("self.persist()"),
            "a night replay must stamp usage.json without cloning every thread: {fire_night}"
        );
        assert!(
            fire_night.contains("land_on_real_chat"),
            "a night chat job must not land on Scratch: {fire_night}"
        );
        let agent = fire_night.find("self.can_agent()").expect("night chat needs Grok Build");
        assert!(
            agent < bump && fire_night.contains("replay.is_none()"),
            "OAuth-only must not burn a night chat slot — send_chat needs Grok Build: {fire_night}"
        );
        let night_check = src
            .split("fn poll_night_check")
            .nth(1)
            .and_then(|s| s.split("fn spawn_night_check").next())
            .expect("poll_night_check");
        assert!(
            night_check.contains("night_check_may_fire"),
            "a finished night check must not halt a live job: {night_check}"
        );
        let send = src
            .split("fn send_chat")
            .nth(1)
            .and_then(|s| s.split("fn send_followup_turn").next())
            .expect("send_chat");
        let redirect = send
            .split("ChatSendKind::Redirect")
            .nth(1)
            .and_then(|s| s.split("ChatSendKind::Fresh").next())
            .expect("redirect");
        assert!(
            !redirect.contains("content.clone()"),
            "redirect must not clone the transcript to read the last user turn: {redirect}"
        );
        let slash_at = send.find("parse_slash").expect("parse_slash");
        let kind_at = send.find("chat_send_kind").expect("chat_send_kind");
        assert!(
            slash_at < kind_at,
            "/compact during a live job must stay local, not become a redirect: {send}"
        );
        assert!(
            send.contains("unknown_cabin_slash") && send.contains("Unknown command"),
            "unknown /project binding must stay local, not go to Grok: {send}"
        );
        let compact = src
            .split("Slash::Compact =>")
            .nth(1)
            .and_then(|s| s.split("Slash::Skill").next())
            .expect("Compact");
        assert!(
            compact.contains("stamp_current_access") || compact.contains("accessed_ms"),
            "/compact must bump accessed_ms or /sync LWW can restore the dropped turns: {compact}"
        );
        assert!(
            compact.contains("compact_keep_start_from") && !compact.contains("content.clone()"),
            "/compact must drain dropped turns without cloning an 8MB pane: {compact}"
        );
        let pushed = send.find("live_mut().push").expect("user turn");
        let saved = send.find("self.persist()").expect("send persist");
        assert!(
            send[pushed..saved].contains("stamp_current_access")
                || send[pushed..saved].contains("accessed_ms"),
            "a sent turn must bump accessed_ms or /sync LWW can drop it: {send}"
        );
        let fail = src
            .split("fn apply_job_fail")
            .nth(1)
            .and_then(|s| s.split("fn queue_update").next())
            .expect("apply_job_fail");
        assert!(
            fail.contains("accessed_ms") || fail.contains("stamp_current_access"),
            "a job error on the origin thread must bump accessed_ms or /sync LWW can drop it: {fail}"
        );
        assert!(
            fail.contains("apply_job_error") && !fail.contains("content.clone()"),
            "a job error must not clone an 8MB pane to replace the last assistant: {fail}"
        );
        let queued = src
            .split("fn queue_update(")
            .nth(1)
            .and_then(|s| s.split("fn restart_after_update").next())
            .expect("queue_update");
        assert!(
            queued.contains("self.persist_cfg()")
                && !queued.contains("config::save")
                && !queued.contains("persist_snap"),
            "Update must not clone every thread just to stamp the source path: {queued}"
        );
        let flush_p = src
            .split("fn flush_projects(")
            .nth(1)
            .and_then(|s| s.split("fn bind_project_id").next())
            .expect("flush_projects");
        let flush_spawn = flush_p.find("thread::spawn").expect("folder click must leave the UI thread");
        let flush_save = flush_p.find("save_projects").expect("save_projects");
        assert!(
            flush_spawn < flush_save && flush_p.contains("persist_io"),
            "folder click must not freeze the cabin writing projects.json: {flush_p}"
        );
        let drop_proj = src
            .split("fn remove_project_id(")
            .nth(1)
            .and_then(|s| s.split("fn apply_project_menu(").next())
            .expect("remove_project_id");
        assert!(
            drop_proj.contains("self.flush_projects()")
                && drop_proj.contains("self.persist()")
                && drop_proj.contains("out.unbound"),
            "deleting an unbound project must not clone every thread just to write projects.json: {drop_proj}"
        );
        let folders = src
            .split("fn stage_new_project(")
            .nth(1)
            .and_then(|s| s.split("fn chat_pairs").next())
            .expect("stage_new_project");
        assert!(
            folders.contains("self.flush_projects()")
                && !folders.contains("self.persist()")
                && !folders.contains("persist_snap"),
            "folder create/rename/move must not clone every thread just to write projects.json: {folders}"
        );
        let menu = src
            .split("fn apply_project_menu(")
            .nth(1)
            .and_then(|s| s.split("fn stage_new_project(").next())
            .expect("apply_project_menu");
        assert!(
            menu.contains("self.flush_projects()")
                && !menu.contains("self.persist()")
                && !menu.contains("persist_snap"),
            "Remove from folder must not clone every thread just to write projects.json: {menu}"
        );
        let rename = src
            .split("Slash::ProjectRename")
            .nth(1)
            .and_then(|s| s.split("Slash::ProjectMove").next())
            .expect("ProjectRename");
        assert!(
            rename.contains("self.flush_projects()")
                && !rename.contains("self.persist()")
                && !rename.contains("persist_snap"),
            "/project rename must not clone every thread just to write projects.json: {rename}"
        );
        let overlay = src
            .split("fn ui_project_overlays(")
            .nth(1)
            .and_then(|s| s.split("impl eframe::App").next())
            .expect("ui_project_overlays");
        assert!(
            overlay.contains("self.flush_projects()")
                && !overlay.contains("self.persist()")
                && !overlay.contains("persist_snap"),
            "Add to folder must not clone every thread just to write projects.json: {overlay}"
        );
        let renamed = src
            .split("fn rename_thread")
            .nth(1)
            .and_then(|s| s.split("fn pin_thread").next())
            .expect("rename_thread");
        assert!(
            renamed.contains("accessed_ms"),
            "rename must bump accessed_ms or /sync LWW can drop the new title: {renamed}"
        );
        let pinned = src
            .split("fn pin_thread")
            .nth(1)
            .and_then(|s| s.split("fn delete_thread_at").next())
            .expect("pin_thread");
        assert!(
            pinned.contains("accessed_ms"),
            "pin must bump accessed_ms or /sync LWW can drop the pin: {pinned}"
        );
        let goal = src
            .split("fn apply_thread_goal")
            .nth(1)
            .and_then(|s| s.split("fn spawn_thread_goal").next())
            .expect("apply_thread_goal");
        assert!(
            goal.contains("accessed_ms"),
            "auto-title must bump accessed_ms or /sync LWW can drop the new name: {goal}"
        );
        assert!(
            goal.contains("self.persist()") && !goal.contains("threads::save"),
            "auto-title must not freeze the cabin writing threads.json: {goal}"
        );
        let spawn_goal = src
            .split("fn spawn_thread_goal_on(")
            .nth(1)
            .and_then(|s| s.split("fn refresh_chips(").next())
            .expect("spawn_thread_goal_on");
        assert!(
            spawn_goal.contains("visible_turn_count")
                || spawn_goal.contains("is_workload_user"),
            "auto-title must ignore HOST_RESULT or a Command-pane job names the thread: {spawn_goal}"
        );
        assert!(
            spawn_goal.contains("chip_chat_pairs") || spawn_goal.contains("chip_scan"),
            "auto-title must not clone an 8MB complete into chat_pairs: {spawn_goal}"
        );
        let created = src
            .split("fn new_thread")
            .nth(1)
            .and_then(|s| s.split("fn begin_chat_rename").next())
            .expect("new_thread");
        assert!(
            created.contains("flush_visible_goal"),
            "/new must persist the left tab's goal before clearing it: {created}"
        );
        assert!(
            created.contains("drop_leaving_thread_chrome"),
            "/new must drop plus-attach, followup budget, and skill follow: {created}"
        );
        assert!(
            created.contains("reuse_empty_thread_idx"),
            "/new must reuse an empty Chat instead of stacking leftover tabs: {created}"
        );
        assert!(
            created.contains("grok_session = None"),
            "New chat must forget the last ACP session id so the next send is session/new: {created}"
        );
        assert!(
            created.contains("grok_cwd = None"),
            "New chat must forget the last worktree or the next send session/new stays in a History tree: {created}"
        );
        assert!(
            created.contains("self.persist()"),
            "forgetting the ACP session on New chat must hit disk or restart reloads Chat 1: {created}"
        );
        assert!(
            created.contains("apply_switch_thread") && !created.contains("self.switch_thread("),
            "/new reuse must not clone every thread twice — switch without persist_bg, then persist once: {created}"
        );
        assert!(
            created.contains("self.messages.clone()") && created.contains("Arc::new"),
            "/new must share the leaving pane Arc, not clone an 8MB HOST_RESULT: {created}"
        );
        let boot = src
            .split("pub fn new(hidden: bool)")
            .nth(1)
            .and_then(|s| s.split("fn persist(&mut self)").next())
            .expect("Cabin::new");
        assert!(
            boot.contains("ensure_memory_seeds") && boot.contains("default_device_name"),
            "first run must seed Memory files and a device name: {boot}"
        );
        assert!(
            boot.contains("leftover_empty_thread"),
            "boot must drop leftover empty Chat tabs: {boot}"
        );
        assert!(
            !boot.contains("threads::save"),
            "boot leftover drop must not freeze the cabin writing threads.json: {boot}"
        );
        assert!(
            boot.contains("persist_bg"),
            "boot leftover drop must persist off-thread or restart restores empty Chat tabs: {boot}"
        );
        assert!(
            src.contains("history_row_visible"),
            "History must hide leftover empty Chat rows"
        );
        let switched = src
            .split("fn switch_thread")
            .nth(1)
            .and_then(|s| s.split("fn open_recent_chat").next())
            .expect("switch_thread");
        assert!(
            switched.contains("drop_leaving_thread_chrome"),
            "switching tabs must not send the previous tab's image or skill follow: {switched}"
        );
        assert!(
            switched.contains("persist_bg") && !switched.contains("self.persist()"),
            "tab switch must not freeze the cabin writing threads.json: {switched}"
        );
        assert!(
            switched.contains("apply_switch_thread"),
            "tab switch persist_bg must share the pane swap with /new reuse: {switched}"
        );
        assert!(
            switched.contains("self.messages.clone()")
                && switched.contains("live_mut")
                && switched.contains("Arc::make_mut"),
            "tab switch must share the parked pane Arc; first mutation copy-on-writes: {switched}"
        );
        let chrome = src
            .split("fn drop_leaving_thread_chrome")
            .nth(1)
            .and_then(|s| s.split("fn pick_entries").next())
            .expect("drop_leaving_thread_chrome");
        assert!(
            chrome.contains("hands_attach = false") && chrome.contains("eyes_attach = false"),
            "leaving a tab must not leave windshield/hands armed for the next tab: {chrome}"
        );
        assert!(
            chrome.contains("self.acp = None") && chrome.contains("tool_cards.clear()"),
            "leaving a tab must drop the ACP handle so New chat does not reuse the last session: {chrome}"
        );
        assert!(
            chrome.contains("halt_in_flight"),
            "dropping the ACP handle on tab switch must halt or the cabin stays Thinking on Chat 2: {chrome}"
        );
        assert!(
            chrome.contains("last_receipt_ok = None"),
            "leaving a tab must not put the next composer into the other tab's error chips: {chrome}"
        );
        let deleted = src
            .split("fn delete_thread_at")
            .nth(1)
            .and_then(|s| s.split("fn send_chat").next())
            .expect("delete_thread_at chrome");
        assert!(
            deleted.contains("drop_leaving_thread_chrome"),
            "deleting the visible tab must drop plus-attach and followup budget: {deleted}"
        );
        let verify = src
            .split("fn run_skill_verify")
            .nth(1)
            .and_then(|s| s.split("fn replay_saved_recipe").next())
            .expect("run_skill_verify");
        let pushed = verify.find("push_bound_msg").expect("VERIFY_RESULT push");
        let saved = verify.find("self.persist()").expect("verify persist");
        assert!(
            pushed < saved,
            "VERIFY_RESULT must hit disk or a restart drops it: {verify}"
        );
        assert!(
            verify.contains("host_working_dir") && verify.contains("run_verify"),
            "skill verify must run in the bound project, not the cabin cwd: {verify}"
        );
        let spawn = verify
            .find("thread::spawn")
            .expect("skill verify must leave the UI thread");
        let run = verify.find("run_verify").expect("run_verify");
        assert!(
            spawn < run,
            "HostDone verify must not block the cabin for 12s: {verify}"
        );
        let kick = src
            .split("fn kick_model(")
            .nth(1)
            .and_then(|s| s.split("fn upsert_stream_assistant").next())
            .expect("kick_model");
        assert!(
            kick.contains("verify_rx"),
            "kick_model must wait for off-thread verify before the follow-up turn: {kick}"
        );
        assert!(
            !kick.contains("t.messages.clone()") && !kick.contains("kick_messages_for_job"),
            "kick_model must not clone the transcript to read the last user turn: {kick}"
        );
        let reflect = src
            .split("fn run_reflect")
            .nth(1)
            .and_then(|s| s.split("fn run_skill_verify").next())
            .expect("run_reflect");
        assert!(
            reflect.contains("kick_messages_for_job")
                || (reflect.contains("chat_job_thread") && reflect.contains("self.threads")),
            "/learn reflect must read the origin thread, not only the visible tab: {reflect}"
        );
        assert!(
            !reflect.contains("t.messages.clone()") && !reflect.contains("content.clone()"),
            "/learn reflect must not clone an 8MB transcript to harvest facts: {reflect}"
        );
        let mem = reflect.find("read_memory(\"MEMORY.md\")").expect("reflect memory");
        assert!(
            reflect[..mem].contains("write_memory") && reflect[..mem].contains("mem_body"),
            "/learn reflect must flush the Memory editor before surgical edit: {reflect}"
        );
        let mem_spawn = reflect[..mem]
            .rfind("thread::spawn")
            .expect("reflect memory must leave the UI thread");
        let flush = reflect[..mem].find("write_memory").expect("reflect flush");
        assert!(
            mem_spawn > flush && reflect.contains("reflect_rx"),
            "idle reflect must not freeze the cabin slurping MEMORY.md: {reflect}"
        );
        let insights = reflect.find("extract_insights").expect("reflect insights");
        assert!(
            reflect[insights..].contains("save_learning")
                && reflect[insights..].contains("persist_io")
                && !reflect[insights..].contains("persist_snap")
                && !reflect[insights..].contains("self.persist()"),
            "/learn reflect must persist insights without cloning every thread: {reflect}"
        );
        let impl_src = src.split("#[cfg(test)]").next().unwrap_or(src);
        assert!(
            !impl_src.contains("fn take_over_desktop") && !impl_src.contains("white_pill(ui, \"Take over\")"),
            "Desk Take over is gone — Grok Build computer-use runs from chat"
        );
        assert!(
            !crate::theme::CABIN_MENU.iter().any(|(id, _)| *id == "eyes"),
            "Desk must not sit in the cabin menu"
        );
        assert!(
            !crate::theme::CABIN_MENU.iter().any(|(id, _)| *id == "command"),
            "Command is not a cabin menu row"
        );
        let replay = src
            .split("fn replay_recipe(")
            .nth(1)
            .and_then(|s| s.split("fn speak_reply").next())
            .expect("replay_recipe");
        assert!(
            replay.contains("run_cmds") && !replay.contains("run_computer_op("),
            "recipe replay must use host gates, not raw desktop ops: {replay}"
        );
        assert!(
            replay.contains("self.running"),
            "recipe replay must report whether host actually started: {replay}"
        );
        let reshoot = replay
            .split("ReplayOp::Reshoot")
            .nth(1)
            .expect("reshoot");
        assert!(
            reshoot.contains("lock_blocks_hands"),
            "recipe reshoot must not capture a lock screen: {reshoot}"
        );
        assert!(
            reshoot.contains("lock_titles"),
            "recipe reshoot must see lock windows that collect_rows drops: {reshoot}"
        );
        let saved_replay = src
            .split("fn replay_saved_recipe")
            .nth(1)
            .and_then(|s| s.split("fn replay_recipe(").next())
            .expect("replay_saved_recipe");
        assert!(
            saved_replay.contains("self.replay_recipe()") && !saved_replay.contains("true"),
            "night must not count a blocked recipe replay as started: {saved_replay}"
        );
        let send_auth = src
            .split("fn send_chat")
            .nth(1)
            .and_then(|s| s.split("fn send_followup_turn").next())
            .expect("send_chat auth");
        assert!(
            send_auth.contains("can_agent") && send_auth.contains("kick_model(true)"),
            "typed send must go through Grok Build ACP: {send_auth}"
        );
        let gate = send_auth.find("persist_user_turn").expect("send auth");
        assert!(
            send_auth[gate..].contains("hands_attach = false")
                && send_auth[gate..].contains("eyes_attach = false"),
            "auth-fail send must disarm leftover take-over flags: {send_auth}"
        );
        assert!(
            send_auth[gate..].contains("speak_next = false"),
            "auth-fail send must not leave TTS armed for the next reply: {send_auth}"
        );
        let last_user = src
            .split("fn last_user_on_job")
            .nth(1)
            .and_then(|s| s.split("fn commit_proposed_skill").next())
            .expect("last_user_on_job");
        assert!(
            (last_user.contains("last_user_for_job") || last_user.contains("last_user_scan"))
                && last_user.contains("self.threads")
                && !last_user.contains("t.messages.clone()"),
            "skill draft after host must not clone every thread: {last_user}"
        );
        assert!(
            !last_user.contains("content.clone()"),
            "skill draft after host must not clone an 8MB complete to read the last user: {last_user}"
        );
        let halt_flight = src
            .split("fn halt_in_flight")
            .nth(1)
            .and_then(|s| s.split("fn apply_assistant_snapshot").next())
            .expect("halt_in_flight");
        assert!(
            halt_flight.contains("speak_next = false"),
            "Stop must cancel a pending voice speak: {halt_flight}"
        );
        assert!(
            halt_flight.contains("perm_ask = None"),
            "Stop must drop the permission bar or Allow continues the cancelled turn: {halt_flight}"
        );
        assert!(
            halt_flight.contains("answer_permission"),
            "Stop must deny leftover Ask or the next send hangs on the unanswered RPC: {halt_flight}"
        );
        assert!(
            halt_flight.contains("try_recv"),
            "Stop must drain leftover ACP tokens so they do not paint on the next prompt: {halt_flight}"
        );
        let halt_persist = halt_flight.find("self.persist()").expect("halt persist");
        assert!(
            halt_flight[..halt_persist].contains("accessed_ms")
                || halt_flight[..halt_persist].contains("stamp_current_access"),
            "halt must bump accessed_ms or /sync LWW can restore the dropped assistant: {halt_flight}"
        );
        assert!(
            halt_flight.contains("stamp_current_access") && halt_flight.contains("accessed_ms"),
            "halt must stamp the origin thread when it is not the visible tab: {halt_flight}"
        );
        assert!(
            !halt_flight.contains("t.messages.clone()") && !halt_flight.contains("content.clone()"),
            "Stop must not clone an 8MB transcript to drop one trailing assistant: {halt_flight}"
        );
        let host_done_facts = src
            .split("Ok(JobOut::HostDone(block))")
            .nth(1)
            .and_then(|s| s.split("Ok(JobOut::Connector").next())
            .expect("HostDone facts");
        let diff = host_done_facts
            .find("HOST_DIFF:")
            .expect("HOST_DIFF push");
        assert!(
            host_done_facts.contains("resolve_host_cite_path"),
            "HOST_DIFF must read the write from the bound tree, not the cabin cwd: {host_done_facts}"
        );
        let after_cite = host_done_facts
            .find("bump_usage")
            .expect("host usage");
        let diff_spawn = host_done_facts[after_cite..]
            .find("thread::spawn")
            .expect("HOST_DIFF worker")
            + after_cite;
        let diff_read = host_done_facts[after_cite..]
            .find("read_text_capped")
            .expect("HOST_DIFF read")
            + after_cite;
        assert!(
            diff_spawn < diff_read && !host_done_facts.contains("read_to_string"),
            "HOST_DIFF must not slurp a huge host write on the UI thread: {host_done_facts}"
        );
        let host_diff_poll = src
            .split("fn poll_host_diff(")
            .nth(1)
            .and_then(|s| s.split("fn finish_host_diff_kick(").next())
            .expect("poll_host_diff");
        assert!(
            host_diff_poll.contains("HOST_DIFF") || host_diff_poll.contains("push_bound_msg"),
            "HOST_DIFF must land in the transcript: {host_diff_poll}"
        );
        assert!(
            host_diff_poll.contains("self.persist()"),
            "HOST_DIFF must persist after the cite push: {host_diff_poll}"
        );
        let deleted = src
            .split("fn delete_thread_at")
            .nth(1)
            .and_then(|s| s.split("fn send_chat").next())
            .expect("delete_thread_at");
        assert!(
            deleted.contains("goal.step") && deleted.contains("self.goal_step"),
            "deleting the visible tab must adopt the next tab's goal step: {deleted}"
        );
        assert!(
            src.contains("let goal_step = threads.get(thread_idx)"),
            "boot must restore the current thread's goal step, not always 0"
        );
        let skills_ui = src
            .split("fn ui_skills")
            .nth(1)
            .and_then(|s| s.split("fn ui_eyes").next())
            .expect("ui_skills");
        assert!(
            skills_ui.contains("skill_use_in_chat_prompt"),
            "Use in chat must send a match_skill hit, not Follow skill: {skills_ui}"
        );
        let pick = skills_ui
            .split("if let Some(name) = pick")
            .nth(1)
            .and_then(|s| s.split("if !self.skill_body.is_empty()").next())
            .expect("skill pick");
        let flush = pick.find("save_skill").expect("flush leaving skill");
        let switch = pick.find("skill_name = s.name").expect("switch skill");
        assert!(
            flush < switch,
            "picking another skill must flush the leaving editor: {pick}"
        );
        let use_chat = skills_ui
            .split("Use in chat")
            .nth(1)
            .and_then(|s| s.split("Run verify").next())
            .expect("use in chat");
        let flushed = use_chat.find("save_skill").expect("flush skill before use");
        let sent = use_chat.find("send_chat").expect("use send");
        assert!(
            flushed < sent,
            "Use in chat must flush the editor before the prompt: {use_chat}"
        );
        let verify_btn = skills_ui
            .split("Run verify")
            .nth(1)
            .and_then(|s| s.split("fn ui_eyes").next())
            .expect("run verify");
        let flushed_v = verify_btn.find("save_skill").expect("flush skill before verify");
        let ran = verify_btn.find("run_skill_verify").expect("run verify");
        assert!(
            flushed_v < ran,
            "Run verify must flush the editor so verify.sh matches the open SKILL.md: {verify_btn}"
        );
        let add = skills_ui
            .split("merge_suggested_skills")
            .nth(1)
            .and_then(|s| s.split("section_label").next())
            .expect("add suggested");
        let learned = add.find("skill_from_learned").expect("learned");
        assert!(
            add[..learned].contains("skill_body") && add[..learned].contains("save_skill"),
            "Add suggested must flush the leaving editor: {add}"
        );
        let new_skill = skills_ui
            .split("New Skill")
            .nth(1)
            .and_then(|s| s.split("verify_chip").next())
            .expect("new skill");
        let stub = new_skill.find("starter_skill").expect("new stub");
        assert!(
            new_skill[..stub].contains("skill_body") && new_skill[..stub].contains("save_skill"),
            "New Skill must flush the leaving editor before the stub: {new_skill}"
        );
        let new_spawn = new_skill.find("thread::spawn").expect("new skill write must leave the UI thread");
        let new_save = new_skill.find("save_skill").expect("new skill save");
        assert!(
            new_spawn < new_save && !skills_ui.contains("list_skills"),
            "Skills tab must not freeze the cabin writing SKILL.md or rescanning skills/: {new_skill}"
        );
        let skill_slash = src
            .split("Slash::Skill(name)")
            .nth(1)
            .and_then(|s| s.split("Slash::LearnReflect").next())
            .expect("Skill slash");
        assert!(
            skill_slash.contains("skill_use_in_chat_prompt") && skill_slash.contains("send_chat"),
            "/skill must run the skill, not only open the editor: {skill_slash}"
        );
        let send = src
            .split("fn send_chat")
            .nth(1)
            .and_then(|s| s.split("fn send_followup_turn").next())
            .expect("send_chat attach");
        assert!(
            send.contains("kick_model(true)"),
            "typed send must consume the plus-button image: {send}"
        );
        let retry = src
            .split("fn kick_model_retry")
            .nth(1)
            .and_then(|s| s.split("fn policy(").next())
            .expect("kick_model_retry");
        assert!(
            retry.contains("match_skill") && retry.contains("skill_follow_block"),
            "/retry must re-inject the skill follow that halt_in_flight cleared: {retry}"
        );
        let host_done = src
            .split("Ok(JobOut::HostDone(block))")
            .nth(1)
            .and_then(|s| s.split("Ok(JobOut::Connector").next())
            .expect("HostDone attach");
        assert!(
            host_done.contains("kick_model(false)"),
            "HostDone must not steal the attached image: {host_done}"
        );
        let pushed = host_done.find("push_bound_msg").expect("host result");
        let recipe = host_done.find("save_recipe").expect("host recipe");
        let saved = host_done.find("self.persist()").expect("host persist");
        assert!(
            pushed < saved && saved < recipe,
            "HOST_RESULT must hit disk before recipe/skill side effects: {host_done}"
        );
        let bump = host_done.find("bump_usage").expect("host usage persist");
        assert!(
            bump < saved,
            "HostDone must persist host usage or persist_bg skips the bump: {host_done}"
        );
        assert_eq!(
            host_done.matches("self.persist()").count(),
            1,
            "HostDone must not clone every thread twice: {host_done}"
        );
        assert!(
            host_done.contains("lock_titles"),
            "HostDone capture must see lock windows that collect_rows drops: {host_done}"
        );
        assert!(
            host_done.contains("eyes_attach = true") && host_done.contains("hands_attach = true"),
            "after COMPUTER_CMD, HostDone must re-arm eyes and hands for the next shot: {host_done}"
        );
        let host_scan = host_done.find("collect_rows").expect("HostDone desk scan");
        let host_spawn = host_done.find("thread::spawn").expect("HostDone desk scan worker");
        assert!(
            host_spawn < host_scan,
            "HostDone AT-SPI must not freeze the cabin: {host_done}"
        );
        let import = src
            .split("fn import_openclaw")
            .nth(1)
            .and_then(|s| s.split("fn run_consult").next())
            .expect("import_openclaw");
        assert!(
            import.contains("merge_imported_memory"),
            "/import must merge MEMORY.md instead of last-file-wins: {import}"
        );
        assert!(
            import.contains("mem_name") && import.contains("mem_body"),
            "/import must reload the Memory editor onto MEMORY.md: {import}"
        );
        let merge_read = import.find("read_memory(\"MEMORY.md\")").expect("import memory");
        assert!(
            import[..merge_read].contains("write_memory") && import[..merge_read].contains("mem_body"),
            "/import must flush the Memory editor before merging MEMORY.md: {import}"
        );
        let dest_arm = import
            .split("import_memory_file")
            .nth(1)
            .expect("import files");
        let dest_write = dest_arm.find("write_memory(&dest").expect("import dest write");
        assert!(
            dest_arm[..dest_write].contains("read_memory(&dest)"),
            "/import must not rotate .prev when SOUL/USER already match disk: {dest_arm}"
        );
        let after_loop = import
            .split("if imported > 0")
            .nth(1)
            .expect("import persist memory");
        assert!(
            after_loop.contains("read_memory(\"MEMORY.md\")")
                && after_loop.contains("write_memory(\"MEMORY.md\""),
            "/import must not rotate MEMORY.md.prev when the merge is unchanged: {after_loop}"
        );
        assert!(
            import.contains("read_text_capped") && !import.contains("read_to_string"),
            "/import must not slurp huge OpenClaw files on the UI thread: {import}"
        );
        let import_spawn = import.find("thread::spawn").expect("import must leave the UI thread");
        let import_walk = import.find("read_dir").expect("import walks OpenClaw");
        assert!(
            import_spawn < import_walk,
            "/import must not walk OpenClaw on the UI thread: {import}"
        );
        let sign_out = src
            .split("fn sign_out_oauth")
            .nth(1)
            .and_then(|s| s.split("fn poll_oauth_photo").next())
            .expect("sign_out_oauth");
        assert!(
            sign_out.contains("oauth_pending = None"),
            "Sign out during device-code poll must not reconnect when the browser finishes: {sign_out}"
        );
        assert!(
            sign_out.contains("oauth_start_rx") && sign_out.contains("oauth_poll_rx"),
            "Sign out must drop in-flight OAuth HTTP: {sign_out}"
        );
        assert!(
            sign_out.contains("persist_io") && sign_out.contains("secrets::save"),
            "Sign out must not freeze the cabin writing secrets.json: {sign_out}"
        );
        let start_o = src
            .split("fn start_oauth(")
            .nth(1)
            .and_then(|s| s.split("fn poll_oauth(").next())
            .expect("start_oauth");
        let start_spawn = start_o.find("thread::spawn").expect("start_oauth spawn");
        let start_dev = start_o.find("start_device").expect("start_device");
        assert!(
            start_spawn < start_dev,
            "Connect Grok OAuth must not freeze the cabin on device-code HTTP: {start_o}"
        );
        let poll_o = src
            .split("fn poll_oauth(")
            .nth(1)
            .and_then(|s| s.split("fn clear_oauth_photo(").next())
            .expect("poll_oauth");
        let poll_spawn = poll_o.find("thread::spawn").expect("poll_oauth spawn");
        let poll_dev = poll_o.find("poll_device").expect("poll_device");
        assert!(
            poll_spawn < poll_dev,
            "OAuth poll must not freeze the cabin on token HTTP: {poll_o}"
        );
        assert!(
            poll_o.contains("persist_io") && poll_o.contains("secrets::save"),
            "OAuth Ready must not freeze the cabin writing secrets.json: {poll_o}"
        );
        let photo = src
            .split("fn kick_oauth_photo(")
            .nth(1)
            .and_then(|s| s.split("fn kick_model(").next())
            .expect("kick_oauth_photo");
        let photo_spawn = photo.find("thread::spawn").expect("avatar fetch must leave the UI thread");
        let photo_decode = photo.find("oauth_photo_image").expect("avatar JPEG decode");
        assert!(
            photo_spawn < photo_decode,
            "OAuth avatar JPEG must not decode on the UI thread: {photo}"
        );
        let photo_poll = src
            .split("fn poll_oauth_photo(")
            .nth(1)
            .and_then(|s| s.split("fn kick_oauth_photo(").next())
            .expect("poll_oauth_photo");
        assert!(
            photo_poll.contains("persist_io") && photo_poll.contains("secrets::save"),
            "OAuth profile enrich must not freeze the cabin writing secrets.json: {photo_poll}"
        );
        let kick = src
            .split("fn kick_model(")
            .nth(1)
            .and_then(|s| s.split("fn upsert_stream_assistant").next())
            .expect("kick_model");
        assert!(
            kick.contains("ensure_acp") && kick.contains("prompt_with_image"),
            "kick_model talks to Grok Build ACP, not cabin windshield: {kick}"
        );
        let cap_fn = src
            .split("fn capture_cabin_frame_this_turn")
            .nth(1)
            .and_then(|s| s.split("fn apply_job_fail").next())
            .expect("capture_cabin_frame_this_turn");
        assert!(
            cap_fn.contains("lock_titles"),
            "leftover capture helper must still see lock windows: {cap_fn}"
        );
        let anticipate = src
            .split("fn tick_anticipate")
            .nth(1)
            .and_then(|s| s.split("fn tick_night").next())
            .expect("tick_anticipate");
        let bump = anticipate.find("bump_usage").expect("anticipate usage");
        let gate = anticipate
            .find("anticipate_consumes_slot")
            .expect("anticipate auth");
        assert!(
            gate < bump,
            "anticipate must not burn quota before auth: {anticipate}"
        );
        assert!(
            anticipate.contains("scratch()"),
            "anticipate must not burn a slot on Scratch: {anticipate}"
        );
        assert!(
            anticipate.contains("self.can_agent()") && !anticipate.contains("self.llm_ready()"),
            "OAuth-only must not burn an anticipate slot — send_chat needs Grok Build: {anticipate}"
        );
        let start_hub = src
            .split("fn start_hub")
            .nth(1)
            .and_then(|s| s.split("fn ui_project_overlays").next())
            .expect("start_hub");
        assert!(
            start_hub.contains("start_hub_rotates_pair"),
            "Start share must rotate an expired leftover code: {start_hub}"
        );
        let start_err = start_hub.split("Err(e)").nth(1).expect("start hub err");
        assert!(
            start_err.contains("sharing = false"),
            "Start share must not leave sharing on when serve_lan fails: {start_hub}"
        );
        assert!(
            start_hub.contains("self.persist_hub()") && !start_hub.contains("persist_snap"),
            "Start share must not clone every thread just to stamp sharing: {start_hub}"
        );
        let eyes = src
            .split("fn refresh_eyes")
            .nth(1)
            .and_then(|s| s.split("fn halt_work").next())
            .expect("refresh_eyes");
        let store = eyes.find("store_hub_frame").expect("eyes store");
        assert!(
            eyes[..store].contains("should_send_screenshot")
                || eyes[..store].contains("lock_blocks_hands"),
            "Eyes Scan must not put a lock-screen frame on the hub: {eyes}"
        );
        assert!(
            eyes[..store].contains("lock_titles"),
            "Eyes Scan must see lock windows that collect_rows drops: {eyes}"
        );
        let live = src
            .split("fn live_room")
            .nth(1)
            .and_then(|s| s.split("fn tick_mid_thought").next())
            .expect("live_room");
        let title = live.find("last_window_title").expect("live title");
        let gate = live.find("should_send_screenshot").expect("live gate");
        assert!(
            live.contains("collect_rows") && title < gate,
            "presence stream must refresh the foreground title before sending a frame: {live}"
        );
        assert!(
            live.contains("lock_titles"),
            "presence stream must see lock windows that collect_rows drops: {live}"
        );
        let devices = src
            .split("fn ui_devices")
            .nth(1)
            .and_then(|s| s.split("fn ui_memory").next())
            .expect("ui_devices");
        assert!(
            devices.contains("pair_code_is_live") && devices.contains("devices_shows_pair_code"),
            "Devices must hide a dead pair code and any code while the hub is off: {devices}"
        );
        let new_code = devices
            .split("New code")
            .nth(1)
            .and_then(|s| s.split("empty_prompt_tile").next())
            .expect("new code");
        let rotated = new_code.find("rotate_pair").expect("rotate");
        assert!(
            new_code[rotated..].contains("self.persist_hub()"),
            "New code must persist the rotated pair before a restart: {new_code}"
        );
        let expired = devices
            .split("rotate_pair")
            .nth(1)
            .and_then(|s| s.split("New code").next())
            .expect("expired rotate");
        assert!(
            expired.contains("self.persist_hub()"),
            "an expired pair rotate must persist or restart shows the dead code: {expired}"
        );
        let clear = src
            .split("Slash::Clear =>")
            .nth(1)
            .and_then(|s| s.split("Slash::Undo =>").next())
            .expect("Clear");
        assert!(
            clear.contains("halt_in_flight"),
            "/clear during a job must halt or the stream refills the pane: {clear}"
        );
        assert!(
            clear.contains("followup_step = 0") && clear.contains("active_skill_follow = None"),
            "/clear must reset followup budget and skill follow with the pane: {clear}"
        );
        assert!(
            clear.contains("stamp_current_access") || clear.contains("accessed_ms"),
            "/clear must bump accessed_ms or /sync LWW can restore the cleared turns: {clear}"
        );
        assert!(
            clear.contains("drop_leaving_thread_chrome") && clear.contains("grok_session = None"),
            "/clear must drop ACP and forget the session id or the next send loads Chat 1: {clear}"
        );
        assert!(
            clear.contains("grok_cwd = None"),
            "/clear must forget the worktree or the next send session/new stays in a History tree: {clear}"
        );
        let help = src
            .split("Slash::Help =>")
            .nth(1)
            .and_then(|s| s.split("Slash::New =>").next())
            .expect("Help");
        assert!(
            help.contains("stamp_current_access") || help.contains("accessed_ms"),
            "/help must bump accessed_ms or /sync LWW can drop the help turn: {help}"
        );
        assert!(
            help.contains("mark_slash_result") || help.contains("SLASH_RESULT"),
            "/help must not become the next model turn: {help}"
        );
        let models = src
            .split("Slash::Models =>")
            .nth(1)
            .and_then(|s| s.split("Slash::Palette =>").next())
            .expect("Models");
        assert!(
            models.contains("stamp_current_access") || models.contains("accessed_ms"),
            "/models must bump accessed_ms or /sync LWW can drop the catalog turn: {models}"
        );
        assert!(
            models.contains("mark_slash_result") || models.contains("SLASH_RESULT"),
            "/models must not become the next model turn: {models}"
        );
        let undo = src
            .split("Slash::Undo =>")
            .nth(1)
            .and_then(|s| s.split("Slash::Retry =>").next())
            .expect("Undo");
        assert!(
            undo.contains("followup_step = 0") && undo.contains("active_skill_follow = None"),
            "/undo must reset followup budget like /clear: {undo}"
        );
        assert!(
            undo.contains("stamp_current_access") || undo.contains("accessed_ms"),
            "/undo must bump accessed_ms or /sync LWW can restore the undone turn: {undo}"
        );
        let forget = src
            .split("Slash::Forget")
            .nth(1)
            .and_then(|s| s.split("Slash::MemoryShow").next())
            .expect("Forget");
        assert!(
            forget.contains("self.scratch()") && forget.contains("no memory writes"),
            "/forget on Scratch must not wipe MEMORY.md: {forget}"
        );
        let note = src
            .split("Slash::MemoryNote")
            .nth(1)
            .and_then(|s| s.split("Slash::Board").next())
            .expect("MemoryNote");
        let append = note.find("append_memory").expect("append");
        assert!(
            note[..append].contains("write_memory") && note[..append].contains("mem_body"),
            "/remember must flush the Memory editor before appending to disk: {note}"
        );
        let append_spawn = note.find("thread::spawn").expect("remember append must leave the UI thread");
        assert!(
            append_spawn < append,
            "/remember must not freeze the cabin appending MEMORY.md: {note}"
        );
        let topic = forget
            .split("Some(q)")
            .nth(1)
            .expect("forget topic");
        let read = topic.find("read_memory(\"MEMORY.md\")").expect("forget read");
        assert!(
            topic[..read].contains("write_memory") && topic[..read].contains("mem_body"),
            "/forget topic must flush the Memory editor before editing disk: {topic}"
        );
        let forget_spawn = topic[..read]
            .rfind("thread::spawn")
            .expect("forget slurp must leave the UI thread");
        assert!(
            forget_spawn < read,
            "/forget topic must not freeze the cabin slurping MEMORY.md: {topic}"
        );
        let memory_ui = src
            .split("fn ui_memory")
            .nth(1)
            .and_then(|s| s.split("fn save_settings").next())
            .expect("ui_memory");
        assert!(
            memory_ui.contains("self.scratch()") && memory_ui.contains("no memory writes"),
            "Memory Save on Scratch must not write MEMORY.md: {memory_ui}"
        );
        let restore = memory_ui
            .split("ghost_pill(ui, \"Restore\")")
            .nth(1)
            .and_then(|s| s.split("Reflect").next())
            .expect("memory restore");
        let restore_spawn = restore.find("thread::spawn").expect("restore must leave the UI thread");
        let restore_fn = restore.find("restore_memory").expect("restore_memory");
        assert!(
            restore_spawn < restore_fn,
            "Memory Restore must not freeze the cabin reading MEMORY.md.prev: {restore}"
        );
        let tabs = memory_ui
            .split("tab_pill")
            .nth(1)
            .and_then(|s| s.split("Restore").next())
            .expect("memory tabs");
        let flush = tabs.find("write_memory").expect("flush leaving memory");
        let switch = tabs.find("mem_name = name").expect("switch name");
        assert!(
            flush < switch && tabs.contains("scratch()"),
            "Memory tab switch must flush the leaving file like thread switch: {tabs}"
        );
        assert!(
            tabs[..flush].contains("read_memory"),
            "Memory tab switch must not rotate .prev when the leaving file is unchanged: {tabs}"
        );
        assert!(
            tabs.contains("thread::spawn"),
            "Memory tab switch must flush off the UI thread: {tabs}"
        );
        assert!(
            tabs.contains("memory_updated_at")
                && tabs.contains("mem_file_rx")
                && tabs.contains("mem_cache_at"),
            "Memory tab switch must not slurp SOUL.md on every click after the first miss: {tabs}"
        );
        assert!(
            tabs.contains("read_memory(name)") && tabs.contains("mem_cache_at[i] == 0"),
            "first Memory tab miss must still read on-thread so tests stay deterministic: {tabs}"
        );
        let save = memory_ui
            .split("white_pill(ui, \"Save\")")
            .nth(1)
            .expect("memory save");
        let write = save.find("write_memory").expect("save write");
        assert!(
            save[..write].contains("read_memory") && save[..write].contains("mem_body"),
            "Memory Save must not rotate .prev when the file is unchanged: {save}"
        );
        assert!(
            save.contains("thread::spawn"),
            "Memory Save must not freeze the cabin writing MEMORY.md: {save}"
        );
        let settings_save = src
            .split("fn save_settings")
            .nth(1)
            .and_then(|s| s.split("fn ui_settings").next())
            .expect("save_settings");
        assert!(
            settings_save.contains("sync_hub_voice"),
            "Settings Save must refresh the hub voice mint key: {settings_save}"
        );
        assert!(
            settings_save.contains("upsert_bound") && settings_save.contains("touch_projects"),
            "Settings Save must keep the sidebar selection on the bound path: {settings_save}"
        );
        let settings_spawn = settings_save.find("thread::spawn").expect("settings mkdir must leave the UI thread");
        let settings_mkdir = settings_save.find("create_dir_all").expect("create_dir_all");
        assert!(
            settings_spawn < settings_mkdir,
            "Settings Save must not freeze the cabin creating the bound folder: {settings_save}"
        );
        let hub_name = settings_save.find("device_name").expect("hub device name");
        let saved = settings_save.find("self.persist()").expect("settings persist");
        assert!(
            hub_name < saved,
            "Settings Save must persist hub device_name or restart keeps the old name: {settings_save}"
        );
        let unbound = src
            .split("Slash::ProjectClear =>")
            .nth(1)
            .and_then(|s| s.split("Slash::ProjectShow =>").next())
            .expect("ProjectClear");
        assert!(
            unbound.contains("project_sel = None") && unbound.contains("touch_projects"),
            "/project clear must drop the sidebar selection: {unbound}"
        );
        let export = src
            .split("Slash::Export =>")
            .nth(1)
            .and_then(|s| s.split("Slash::Recall").next())
            .expect("Export");
        let flushed = export.find("self.persist()").expect("export persist");
        let wrote = export.find("export_markdown").expect("export_markdown");
        assert!(
            flushed < wrote,
            "/export must flush the live pane before writing the thread file: {export}"
        );
        assert!(
            export.contains("expand_home"),
            "/export must expand ~ in the bound project or it writes a literal tilde folder: {export}"
        );
        let export_spawn = export.find("thread::spawn").expect("export write must leave the UI thread");
        let export_write = export.find("fs::write").expect("export.md");
        assert!(
            export_spawn < export_write && export_spawn < wrote,
            "/export must not freeze the cabin formatting an 8MB thread into markdown: {export}"
        );
        let recall = src
            .split("Slash::Recall(q)")
            .nth(1)
            .and_then(|s| s.split("fn kick_model_retry").next())
            .expect("Recall");
        let mem = recall.find("read_memory(\"SOUL.md\")").expect("recall soul");
        assert!(
            recall[..mem].contains("write_memory")
                && recall[..mem].contains("mem_body")
                && recall[..mem].contains("scratch()"),
            "/recall must flush the Memory editor before searching disk: {recall}"
        );
        assert!(
            recall[..mem].contains("thread::spawn"),
            "/recall must slurp SOUL/USER/MEMORY off the UI thread: {recall}"
        );
        let recall_poll = src
            .split("fn poll_recall(")
            .nth(1)
            .and_then(|s| s.split("fn poll_session_show").next())
            .expect("poll_recall");
        assert!(
            recall_poll.contains("stamp_current_access") || recall_poll.contains("accessed_ms"),
            "/recall must bump accessed_ms or /sync LWW can drop the recall turn: {recall_poll}"
        );
        assert!(
            recall_poll.contains("mark_slash_result") || recall_poll.contains("SLASH_RESULT"),
            "/recall must not become the next model turn: {recall_poll}"
        );
        let sync = src
            .split("fn sync_hub(&mut self)")
            .nth(1)
            .and_then(|s| s.split("fn local_clock").next())
            .expect("sync_hub");
        assert!(
            sync.contains("merge_hub_snapshots"),
            "/sync must merge the hub snapshot, not replace peer threads: {sync}"
        );
        let flushed = sync.find("persist_snap").expect("sync persist");
        let built = sync.find("build_hub_snapshot").expect("build snapshot");
        assert!(
            flushed < built,
            "/sync must flush the live pane before publishing threads: {sync}"
        );
        let merged = sync.find("st.snapshot =").expect("store merge");
        assert!(
            sync[merged..].contains("self.persist_hub()"),
            "/sync must persist the merged snapshot or a restart drops peer LWW: {sync}"
        );
        let poll = src
            .split("fn poll_sync(")
            .nth(1)
            .and_then(|s| s.split("fn date_out").next())
            .expect("poll_sync");
        assert!(
            poll.contains("persist_hub") && !poll.contains("persist_snap"),
            "/sync inbound must not clone every thread just to flush hub-state.json: {poll}"
        );
        let mem_write = sync.find("write_memory").expect("sync memory flush");
        assert!(
            mem_write < built
                && sync.contains("mem_body")
                && sync.contains("scratch()"),
            "/sync must flush the Memory editor before publishing files: {sync}"
        );
        let sync_spawn = sync.find("thread::spawn").expect("sync worker");
        let sync_read = sync.find("read_memory(n)").expect("sync memory slurp");
        assert!(
            sync_spawn < sync_read,
            "/sync must slurp SOUL/USER/MEMORY off the UI thread: {sync}"
        );
        let thread_rows = sync
            .split("let threads = snap")
            .nth(1)
            .and_then(|s| s.split("let skills = skills").next())
            .expect("sync threads");
        assert!(
            thread_rows.contains("accessed_ms") && !thread_rows.contains("now_ms()"),
            "/sync must not stamp every thread now or local stale data wins LWW: {thread_rows}"
        );
        assert!(
            !sync.contains("t.messages.clone()") && sync.contains("write_persist_disk"),
            "/sync must reuse the persist snap instead of cloning every thread twice: {sync}"
        );
        let push = src
            .split("fn push_bound_msg")
            .nth(1)
            .and_then(|s| s.split("fn apply_live_assistant").next())
            .expect("push_bound_msg");
        assert!(
            push.contains("IMAGE_FILE_CAP") || push.contains("take_ui_text"),
            "host/consult receipts must not land a huge body in the transcript: {push}"
        );
        assert!(
            push.contains("accessed_ms"),
            "background job writes must bump accessed_ms or /sync LWW drops the new messages: {push}"
        );
        assert!(
            !push.contains("t.messages.clone()"),
            "host receipts must not clone every thread: {push}"
        );
        let snap = src
            .split("fn apply_assistant_snapshot")
            .nth(1)
            .and_then(|s| s.split("fn push_bound_msg").next())
            .expect("apply_assistant_snapshot");
        assert!(
            snap.contains("accessed_ms"),
            "background stream writes must bump accessed_ms or /sync LWW drops the new messages: {snap}"
        );
        assert!(
            !snap.contains("t.messages.clone()"),
            "stream deltas must not clone every thread: {snap}"
        );
        let mem_rows = sync
            .split("let mem = ")
            .nth(1)
            .and_then(|s| s.split("let mut snap = self.persist_snap").next())
            .expect("sync mem");
        assert!(
            mem_rows.contains("memory_updated_at") && !mem_rows.contains("now_ms()"),
            "/sync must not stamp MEMORY.md now or stale local wins LWW: {mem_rows}"
        );
        let skill_rows = sync
            .split("let skills = self")
            .nth(1)
            .and_then(|s| s.split("let autos = self").next())
            .expect("sync skills");
        assert!(
            skill_rows.contains("skill_updated_at") && !skill_rows.contains("now_ms()"),
            "/sync must not stamp every skill now or local stale data wins LWW: {skill_rows}"
        );
        let inbound = src
            .split("fn apply_inbound_snapshot")
            .nth(1)
            .and_then(|s| s.split("fn push_presence").next())
            .expect("apply_inbound_snapshot");
        assert!(
            inbound.contains("mem_body") && inbound.contains("mem_name"),
            "inbound MEMORY.md must refresh the open Memory editor: {inbound}"
        );
        assert!(
            inbound.contains("mem_cache_body") && inbound.contains("mem_file_idx"),
            "inbound MEMORY.md must refresh the Memory tab cache or a later click shows a stale slurp: {inbound}"
        );
        let wrote = inbound.find("write_memory").expect("inbound write");
        assert!(
            inbound[..wrote].contains("read_memory"),
            "inbound must not rotate .prev when the merged file is unchanged: {inbound}"
        );
        let inbound_spawn = inbound.find("thread::spawn").expect("inbound write must leave the UI thread");
        assert!(
            inbound_spawn < wrote,
            "inbound MEMORY.md must not freeze the cabin writing markdown: {inbound}"
        );
        assert!(
            !inbound.contains("snapshot.clone()") && !inbound.contains("from_value"),
            "/sync inbound must not clone the hub snapshot on the UI thread: {inbound}"
        );
        let send = src
            .split("fn dispatch_send")
            .nth(1)
            .and_then(|s| s.split("fn sync_hub(&mut self)").next())
            .expect("dispatch_send");
        let queued = send.find("enqueue_local").expect("enqueue");
        assert!(
            send[queued..].contains("self.persist_hub()") && !send[queued..].contains("persist_snap"),
            "/send must persist a queued hub task without cloning every thread: {send}"
        );
        let inhabit = src
            .split("fn queue_inhabit")
            .nth(1)
            .and_then(|s| s.split("fn rewind_project").next())
            .expect("queue_inhabit");
        let staged = inhabit.find("inhabit = Some").expect("stage inhabit");
        assert!(
            inhabit[staged..].contains("self.persist_hub()") && !inhabit.contains("persist_snap"),
            "/inhabit must persist the staged bundle without cloning every thread: {inhabit}"
        );
        assert!(
            inhabit.contains("inhabit_claim_allowed") && inhabit.contains("to_id"),
            "/inhabit must name a real peer and skip headphones-as-phone: {inhabit}"
        );
        let soul = inhabit.find("read_memory(\"SOUL.md\")").expect("inhabit soul");
        assert!(
            inhabit[..soul].contains("write_memory") && inhabit[..soul].contains("mem_body"),
            "/inhabit must flush the Memory editor before packing SOUL.md: {inhabit}"
        );
        let soul_spawn = inhabit[..soul]
            .rfind("thread::spawn")
            .expect("inhabit soul must leave the UI thread");
        let flush = inhabit[..soul].find("write_memory").expect("inhabit flush");
        assert!(
            soul_spawn > flush && inhabit.contains("inhabit_rx"),
            "/inhabit must not freeze the cabin packing a 1MB SOUL.md: {inhabit}"
        );
        let greet = src
            .split("fn refresh_greeting")
            .nth(1)
            .and_then(|s| s.split("fn spawn_greeting_llm").next())
            .expect("refresh_greeting");
        let user = greet.find("read_memory(\"USER.md\")").expect("greet user");
        assert!(
            greet[..user].contains("write_memory")
                && greet[..user].contains("mem_body")
                && greet[..user].contains("scratch()"),
            "empty-chat greeting must flush the Memory editor before reading USER/MEMORY: {greet}"
        );
        assert!(
            greet[..user].contains("memory_updated_at"),
            "empty-chat greeting must not slurp USER/MEMORY on every paint: {greet}"
        );
        assert!(
            greet.contains("greeting_files_rx") && greet.contains("greeting_user_at == 0"),
            "mtime-changed USER/MEMORY must leave the UI thread after the first miss: {greet}"
        );
        let greet_spawn = greet.find("thread::spawn").expect("greeting flush must leave the UI thread");
        let greet_write = greet.find("write_memory").expect("greeting write_memory");
        assert!(
            greet_spawn < greet_write,
            "empty-chat greeting must not freeze the cabin writing MEMORY.md: {greet}"
        );
        assert!(
            !greet.contains("device_name"),
            "hostname must not paint as the empty-home greeting: {greet}"
        );
        assert!(
            greet.contains("as_str()")
                && !greet.contains("greeting_user_md.clone()")
                && !greet.contains("greeting_memory_md.clone()"),
            "empty-chat greeting must not clone USER/MEMORY every paint: {greet}"
        );
        assert!(
            greet.contains("greeting_prompt"),
            "greeting Fast prompt must be built from borrowed USER/MEMORY: {greet}"
        );
        let dream = src
            .split("fn run_dream")
            .nth(1)
            .and_then(|s| s.split("fn dispatch_send").next())
            .expect("run_dream");
        assert!(
            dream.contains("visible_host_receipts") && dream.contains("dream_rewind_id"),
            "/dream must use this tab's host receipts, not cabin-global last_receipts: {dream}"
        );
        let key = dream.find("llm_ready").expect("dream auth");
        let push = dream.find("live_mut().push").expect("dream push");
        assert!(
            key < push && dream.contains("self.running"),
            "/dream must not persist a turn when Imagine cannot start: {dream}"
        );
        assert!(
            dream.contains("stamp_current_access") || dream.contains("accessed_ms"),
            "/dream must bump accessed_ms or /sync LWW can drop the dream turn: {dream}"
        );
        let night = src
            .split("fn last_night_hint")
            .nth(1)
            .and_then(|s| s.split("fn mark_auto_ran").next())
            .expect("last_night_hint");
        assert!(
            night.contains("visible_host_receipts") && !night.contains("last_receipts"),
            "greeting last-night must not mix another tab's receipts: {night}"
        );
        let vis = src
            .split("fn visible_host_receipts(")
            .nth(1)
            .and_then(|s| s.split("fn dream_rewind_id").next())
            .expect("visible_host_receipts");
        assert!(
            vis.contains("thread_host_receipts_from") && !vis.contains("content.clone()"),
            "empty-chat greeting must not clone an 8MB transcript to read host receipts: {vis}"
        );
        let context = src
            .split("Slash::Context =>")
            .nth(1)
            .and_then(|s| s.split("Slash::Health =>").next())
            .expect("Context");
        assert!(
            context.contains("visible_turn_count")
                && context.contains("estimate_messages")
                && !context.contains("content.clone()"),
            "/context must count real turns without cloning an 8MB transcript: {context}"
        );
        let finish = src
            .split("fn finish_hub_dispatch")
            .nth(1)
            .and_then(|s| s.split("fn hide_to_tray").next())
            .expect("finish_hub_dispatch");
        assert!(
            finish.contains("self.pending_hub_task.clone()")
                && finish.contains("clear_pending_after_complete"),
            "do not drop pending_hub_task before the hub mutex is held"
        );
        let complete_at = finish.find("complete_task").expect("complete_task");
        let persist_at = finish.find("self.persist_hub()").expect("persist hub complete");
        assert!(
            complete_at < persist_at && !finish.contains("persist_snap"),
            "phone task completion must hit hub-state.json without cloning every thread: {finish}"
        );
        let host_done = src
            .split("Ok(JobOut::HostDone(block))")
            .nth(1)
            .and_then(|s| s.split("Ok(JobOut::Connector").next())
            .expect("HostDone");
        assert!(
            host_done.contains("pending_connectors"),
            "queued connectors must run after host before the next kick_model"
        );
        let consult = src
            .split("fn run_consult")
            .nth(1)
            .and_then(|s| s.split("fn open_palette").next())
            .expect("run_consult");
        assert!(
            consult.contains("if self.running") && consult.contains("halt_in_flight"),
            "consult must not drop a finished parent reply: {consult}"
        );
        assert!(
            consult.contains("if self.chat_job_thread.is_none()"),
            "consult must stay on the origin thread: {consult}"
        );
        assert!(
            consult.contains("Interrupted by consult"),
            "slash consult during a phone job must fail the dispatch: {consult}"
        );
        let consult_out = src
            .split("Ok(JobOut::Consult(detail))")
            .nth(1)
            .and_then(|s| s.split("Ok(JobOut::HostLine").next())
            .expect("Consult");
        assert!(
            !consult_out.contains("finish_hub_dispatch"),
            "consult must not complete a phone task as the consult reply: {consult_out}"
        );
        assert!(
            consult_out.contains("status.clear()") || consult_out.contains("status ="),
            "consult must not leave the status bar on Consult… after the reply lands: {consult_out}"
        );
        let chat_consult = src
            .split("if let Some(q) = parse_consult")
            .nth(1)
            .and_then(|s| s.split("let outcome = parse_goal_outcome").next())
            .expect("parse_consult");
        let finish_at = chat_consult.find("finish_hub_dispatch");
        let run_at = chat_consult.find("run_consult");
        assert!(
            finish_at.is_some_and(|f| run_at.is_some_and(|r| f < r)),
            "finish the phone task before starting consult: {chat_consult}"
        );
        let rewind = src
            .split("fn rewind_project")
            .nth(1)
            .and_then(|s| s.split("fn snapshot_project").next())
            .expect("rewind_project");
        let restoring = rewind.find("Restoring").expect("Restoring");
        let blocked = rewind.find("rewind_blocked_reason").expect("rewind gate");
        assert!(
            blocked < restoring && !rewind.contains("Restored"),
            "/rewind must not claim Restored before cp finishes or when host cannot start: {rewind}"
        );
        let queued = rewind.find("queue_sh").expect("queue restore");
        assert!(
            queued < restoring && rewind[queued..restoring].contains("self.running"),
            "/rewind must not claim Restoring when host did not start: {rewind}"
        );
        let took = rewind.find("took one").expect("first snapshot");
        let snap_q = rewind.rfind("queue_sh").expect("queue snapshot");
        assert!(
            snap_q < took && rewind[snap_q..took].contains("self.running"),
            "/rewind must not claim a snapshot started when host did not start: {rewind}"
        );
        assert!(
            rewind.contains("rewind_copy_cmd"),
            "/rewind restore must copy snapshot contents into the project, not nest the dest folder: {rewind}"
        );
        assert!(
            rewind.contains("expand_home"),
            "/rewind must expand ~ before quoting the bound tree: {rewind}"
        );
        let snap = src
            .split("fn snapshot_project")
            .nth(1)
            .and_then(|s| s.split("fn doctor_text").next())
            .expect("snapshot_project");
        assert!(
            snap.contains("rewind_blocked_reason")
                && snap.contains("rewind_copy_cmd")
                && !snap.contains("run_cmds"),
            "snapshot must record a dest and return the cp, not nest run_cmds: {snap}"
        );
        assert!(
            snap.contains("expand_home"),
            "snapshot must expand ~ before quoting the bound tree: {snap}"
        );
        let snap_spawn = snap.find("thread::spawn").expect("rewind index must leave the UI thread");
        let snap_write = snap.find("save_rewinds").expect("save_rewinds");
        assert!(
            snap_spawn < snap_write,
            "snapshot must not freeze the cabin writing rewind.json: {snap}"
        );
        assert!(
            src.contains("is_rewind_copy_cmd"),
            "host jobs must prepend a snapshot instead of nesting run_cmds"
        );
        assert!(
            src.contains("expand_home(&restore_bound_path"),
            "boot must expand a tilde-bound project or rewind quotes a literal ~ folder"
        );
        let dream_id = src
            .split("fn dream_rewind_id")
            .nth(1)
            .and_then(|s| s.split("fn run_dream").next())
            .expect("dream_rewind_id");
        assert!(
            dream_id.contains("expand_home"),
            "/dream rewind cite must expand ~ before matching the snapshot root: {dream_id}"
        );
        let host_done = src
            .split("Ok(JobOut::HostDone(block))")
            .nth(1)
            .and_then(|s| s.split("Ok(JobOut::Connector").next())
            .expect("HostDone");
        assert!(
            host_done.contains("job_is_scratch"),
            "HostDone must use the origin thread scratch flag: {host_done}"
        );
        assert!(
            host_done.contains("parse_computer_op")
                && !host_done.contains("parse_computer_cmd_loose"),
            "a leftover type cargo in last_host must not be labeled COMPUTER_RESULT: {host_done}"
        );
        assert!(
            host_done.contains("append_host_trajectory") && host_done.contains("trim_job_result_dumps"),
            "HostDone must record a trajectory line and trim old tool dumps: {host_done}"
        );
        let traj = src
            .split("fn append_host_trajectory(")
            .nth(1)
            .and_then(|s| s.split("fn trim_job_result_dumps").next())
            .expect("append_host_trajectory");
        let traj_spawn = traj.find("thread::spawn").expect("trajectory must leave the UI thread");
        let traj_write = traj.find("append_trajectory").expect("append_trajectory");
        assert!(
            traj_spawn < traj_write,
            "HostDone must not freeze the cabin rewriting a 2MB trajectory.jsonl: {traj}"
        );
        let trim = src
            .split("fn trim_job_result_dumps(")
            .nth(1)
            .and_then(|s| s.split("fn queue_sh(").next())
            .expect("trim_job_result_dumps");
        let est = trim.find("should_trim_result_bodies").expect("estimate before clone");
        assert!(
            trim.contains("trim_result_bodies_in_place") && !trim.contains("content.clone()"),
            "result trim must rewrite old dumps in place, not clone an 8MB pane: {trim}"
        );
        assert!(
            trim[est..].contains("trim_result_bodies_in_place"),
            "result trim must estimate borrowed tokens before touching dumps: {trim}"
        );
        let commit = src
            .split("fn commit_proposed_skill(")
            .nth(1)
            .and_then(|s| s.split("fn apply_review_skill_patches").next())
            .expect("commit_proposed_skill");
        let commit_spawn = commit.find("thread::spawn").expect("skill write must leave the UI thread");
        let commit_save = commit.find("save_skill").expect("save_skill");
        assert!(
            commit_spawn < commit_save && !commit.contains("list_skills"),
            "HostDone must not freeze the cabin writing SKILL.md: {commit}"
        );
        let verify = src
            .split("fn apply_verify_result(")
            .nth(1)
            .and_then(|s| s.split("fn replay_saved_recipe(").next())
            .expect("apply_verify_result");
        let verify_spawn = verify.find("thread::spawn").expect("verify skill write must leave the UI thread");
        let verify_save = verify.find("save_skill").expect("save_skill");
        assert!(
            verify_spawn < verify_save,
            "verify pass must not freeze the cabin writing SKILL.md: {verify}"
        );
        let ran = src
            .split("fn mark_auto_ran(")
            .nth(1)
            .and_then(|s| s.split("fn mark_auto_skipped(").next())
            .expect("mark_auto_ran");
        let ran_spawn = ran.find("thread::spawn").expect("night save must leave the UI thread");
        let ran_save = ran.find("night::save").expect("night::save");
        assert!(
            ran_spawn < ran_save,
            "night run stamp must not freeze the cabin writing automations.json: {ran}"
        );
        let run_cmds = src
            .split("fn run_cmds")
            .nth(1)
            .and_then(|s| s.split("fn run_connector").next())
            .expect("run_cmds");
        assert!(
            run_cmds.contains("mint_host_halt") && !run_cmds.contains("host_halt.store(false"),
            "a new host job must not clear the previous job's halt flag: {run_cmds}"
        );
        assert!(
            run_cmds.contains("!is_rewind_copy_cmd")
                && run_cmds.contains("host_cmd_leaves_project"),
            "cabin rewind copies must run when YOLO is off: {run_cmds}"
        );
        assert!(
            run_cmds.contains("AlwaysApprove") && !run_cmds.contains("cfg.yolo"),
            "bound-tree jail follows the Always pill, not leftover app.json yolo: {run_cmds}"
        );
        let impl_src = src.split("#[cfg(test)]").next().unwrap_or(src);
        assert!(
            !impl_src.contains("if let Some(plan) = plan_from_text"),
            "Chat complete must not parse HOST_CMD / COMPUTER_CMD; Grok Build owns tools"
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
        let slice = &src[home..home + 1800];
        assert!(
            slice.contains("self.greeting"),
            "new chats paint a greeting blurb: {slice}"
        );
        assert!(
            slice.contains("GREET_HERO") && slice.contains("title_font"),
            "greeting is the empty-home hero, not a 56px wordmark: {slice}"
        );
        assert!(
            !slice.contains("italics"),
            "greeting is regular/medium weight: {slice}"
        );
        assert!(
            slice.contains("paint_perm_ask"),
            "empty home must still show a live permission bar: {slice}"
        );
        let greet = slice.find("self.greeting").expect("greeting");
        let composer = slice.find("ui_composer_stack").expect("composer");
        assert!(
            greet < composer,
            "greeting sits above the chat box"
        );
        assert!(
            slice.contains("muted()"),
            "greeting uses secondary paint, not a washed-out whisper"
        );
        assert!(
            !slice.contains("Native Grok Build cabin"),
            "empty home is the greeting, not a product tagline: {slice}"
        );
        assert!(
            !slice.contains("RichText::new(\"GrokHub\")"),
            "empty home must not paint a GrokHub wordmark: {slice}"
        );
        assert!(
            !slice.contains("device_name") && !slice.contains("WORDMARK"),
            "empty home must not paint the hostname: {slice}"
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
        assert!(slice.contains("ui_chat"));
        assert!(!slice.contains("Take over"));
        assert!(!slice.contains("Install hands"));
        assert!(!slice.contains("hands_chip_text"));
        assert!(
            slice.contains("Grok Build") || slice.contains("computer-use") || slice.contains("chat pane"),
            "Eyes leftover must not be a cabin desktop-control menu: {slice}"
        );
    }

    #[test]
    fn presence_ring_drops_a_huge_frame() {
        let src = include_str!("app.rs");
        let push = src
            .split("fn push_presence(")
            .nth(1)
            .and_then(|s| s.split("fn live_room(").next())
            .expect("push_presence");
        assert!(
            push.contains("FRAME_CAP"),
            "live presence must not keep an 8MB JPEG data URL for ten minutes: {push}"
        );
        assert!(
            push.contains("PRESENCE_RING_MAX") || (push.contains("presence_ring.len()") && push.contains("32")),
            "a 10-minute ring of FRAME_CAP JPEGs can still OOM live Eyes: {push}"
        );
    }

    #[test]
    fn eyes_paint_does_not_clone_last_frame_url() {
        let src = include_str!("app.rs");
        let eyes = src
            .split("fn ui_eyes(")
            .nth(1)
            .and_then(|s| s.split("fn project_row_active(").next())
            .expect("ui_eyes");
        assert!(
            !eyes.contains("last_frame_url.clone()"),
            "Eyes paint must not clone a huge last-frame data URL every frame: {eyes}"
        );
    }

    #[test]
    fn last_frame_url_drops_a_huge_capture() {
        let src = include_str!("app.rs");
        let impl_src = src.split("#[cfg(test)]").next().unwrap_or(src);
        let remember = impl_src
            .split("fn remember_last_frame(")
            .nth(1)
            .and_then(|s| s.split("\n    fn ").next())
            .expect("remember_last_frame");
        assert!(
            remember.contains("FRAME_CAP"),
            "last_frame_url must not keep an 8MB grim data URL: {remember}"
        );
        let assigns = impl_src.matches("last_frame_url = Some").count();
        assert!(
            assigns <= 1,
            "every last-frame write must go through remember_last_frame, found {assigns}"
        );
        let hub_frame = impl_src
            .split("fn store_hub_frame(")
            .nth(1)
            .and_then(|s| s.split("\n    fn ").next())
            .expect("store_hub_frame");
        let parse = hub_frame.find("store_frame").expect("parse jpeg");
        let lock = hub_frame.find("hub.lock").expect("hub lock");
        assert!(
            parse < lock && hub_frame.contains("install_frame"),
            "cabin must not decode a 400KB JPEG under hub.lock(): {hub_frame}"
        );
    }

    #[test]
    fn eyes_frame_tex_rejects_a_huge_frame() {
        let src = include_str!("app.rs");
        let tex = src
            .split("fn eyes_frame_tex(")
            .nth(1)
            .and_then(|s| s.split("fn project_row_active(").next())
            .expect("eyes_frame_tex");
        let cap = tex.find("IMAGE_FILE_CAP").expect("size check before decode");
        let decode = tex.find("load_from_memory").expect("decode");
        let spawn = tex.find("thread::spawn").expect("decode must leave the UI thread");
        assert!(
            spawn < decode && cap < decode,
            "Eyes last-frame paint must not decode a huge JPEG on the UI thread: {tex}"
        );
        assert!(
            tex.contains("image_pixels_ok") || tex.contains("IMAGE_PIXEL_CAP"),
            "Eyes last-frame paint must not decode a pixel bomb on the UI thread: {tex}"
        );
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
        let start = src.find("fn ui_composer_stack").expect("composer stack");
        let end = src[start..]
            .find("fn ui_devices")
            .map(|i| start + i)
            .unwrap_or(src.len());
        let stack = &src[start..end];
        assert!(!stack.contains("SkillApprove"), "{stack}");
        assert!(!stack.contains("SaveAsSkill"), "{stack}");
        assert!(!stack.contains("HostPlan"), "{stack}");
        let order = super::composer_stack_order();
        assert_eq!(
            order,
            &[
                super::ComposerStackSlot::AuthBanner,
                super::ComposerStackSlot::SlashPalette,
                super::ComposerStackSlot::Attach,
                super::ComposerStackSlot::Pill,
                super::ComposerStackSlot::Chips,
            ]
        );
    }

    #[test]
    fn chips_sit_below_the_composer_pill() {
        let order = super::composer_stack_order();
        let chips = order
            .iter()
            .position(|s| *s == super::ComposerStackSlot::Chips);
        let pill = order
            .iter()
            .position(|s| *s == super::ComposerStackSlot::Pill)
            .expect("pill");
        assert!(chips.is_some(), "chips belong below the composer pill");
        assert!(chips.unwrap() > pill);
    }

    #[test]
    fn other_chip_threads_skip_current_and_scratch() {
        let mut current = crate::threads::ChatThread::new("Now", false);
        current.id = "cur".into();
        current.messages_mut().push(("user".into(), "this chat".into()));
        let mut prev = crate::threads::ChatThread::new("Night cabin", false);
        prev.id = "prev".into();
        prev.messages_mut().push(("user".into(), "paint the wall".into()));
        prev.messages_mut()
            .push(("assistant".into(), "I can sketch the first coat.".into()));
        let mut scratch = crate::threads::ChatThread::new("Scratch", true);
        scratch.id = "scr".into();
        scratch.messages_mut().push(("user".into(), "ignore me".into()));
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
        let after = &home[cap..];
        assert!(
            after.contains("self.greeting"),
            "greeting paints inside the capped column"
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
        let spawn_at = spawn.find("thread::spawn").expect("review HTTP must leave the UI thread");
        let write = spawn.find("write_memory").expect("flush memory");
        let traj = spawn.find("read_trajectory").expect("trajectory digest");
        assert!(
            spawn_at < write && spawn_at < traj && spawn.contains("mem_body"),
            "nightly review must flush Memory and slurp trajectory off the UI thread: {spawn}"
        );
        assert!(
            !spawn[..spawn_at].contains("scratch()"),
            "Scratch is a chat tab — unsaved Memory editor edits must still reach the nightly digest: {spawn}"
        );
        let digest_fn = src
            .split("fn review_digest(")
            .nth(1)
            .and_then(|s| s.split("fn spawn_review(").next())
            .expect("review_digest");
        assert!(
            digest_fn.contains("thread_host_receipts")
                && !digest_fn.contains("last_receipts")
                && !digest_fn.contains("last_host"),
            "nightly review must take host receipts from the digested threads, not cabin-global last_host: {digest_fn}"
        );
        assert!(
            digest_fn.contains("digest_line_from")
                && !digest_fn.contains("content.clone()")
                && !digest_fn.contains("text.clone()"),
            "nightly review must not clone an 8MB complete into the digest: {digest_fn}"
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
        let held = apply.split("Err(e)").nth(1).expect("review held");
        assert!(
            held.contains("last_review_day") && held.contains("save_suggestions"),
            "a held nightly review must not retry every heartbeat: {apply}"
        );
        assert!(
            apply.contains("thread::spawn") && apply.contains("save_suggestions"),
            "nightly review must not freeze the cabin writing suggestions.json: {apply}"
        );
        assert!(
            apply.contains("merge_suggestion_store"),
            "a partial nightly review must not wipe the other suggestion grids: {apply}"
        );
        assert!(
            apply.contains("CABIN_GITHUB_TOOLS") && !apply.contains("&[]"),
            "nightly review must drop already-wired GitHub tools: {apply}"
        );
        assert!(
            apply.contains("prune_live_suggestions"),
            "a successful review must drop wired GitHub tiles already sitting in the store: {apply}"
        );
        let wall = src
            .split("fn poll_wall(")
            .nth(1)
            .and_then(|s| s.split("fn tick_wall(").next())
            .expect("poll_wall");
        let ok_wall = wall
            .split("Ok(Ok(gif))")
            .nth(1)
            .and_then(|s| s.split("Ok(Err(e))").next())
            .expect("wall ok");
        let ok_spawn = ok_wall
            .find("thread::spawn")
            .expect("wall cover save must leave the UI thread");
        let ok_save = ok_wall.find("save_wall").expect("ok save_wall");
        assert!(
            ok_spawn < ok_save
                && ok_wall.contains("persist_io")
                && !ok_wall.contains("persist_snap")
                && !ok_wall.contains("self.persist()"),
            "a new wall cover must not clone every thread just to write imagine-wall.json: {wall}"
        );
        let held_wall = wall.split("Ok(Err(e))").nth(1).expect("wall held");
        let wall_spawn = held_wall
            .find("thread::spawn")
            .expect("wall save must leave the UI thread");
        let wall_save = held_wall.find("save_wall").expect("save_wall");
        assert!(
            wall_spawn < wall_save && held_wall.contains("persist_io"),
            "a held wall cover must not freeze the cabin writing imagine-wall.json: {wall}"
        );
        assert!(
            apply.contains("apply_review_skill_patches"),
            "nightly review must patch existing skills from SUGGEST_SKILL_PATCH: {apply}"
        );
        assert!(src.contains("self.tick_review()"));
        assert!(
            src.contains("if !night_fired && !self.running"),
            "Review waits if Night just fired or chat is running"
        );
        let history = src
            .split("fn ui_history(")
            .nth(1)
            .and_then(|s| s.split("fn ui_board(").next())
            .expect("ui_history");
        let search = history
            .split("white_pill(ui, \"Search\")")
            .nth(1)
            .and_then(|s| s.split("history_hits").next())
            .expect("history search");
        assert!(
            search.contains("write_memory")
                && search.contains("mem_body")
                && search.contains("scratch()"),
            "History Search must flush the Memory editor before reading disk: {search}"
        );
        assert!(
            search.contains("thread::spawn"),
            "History Search must flush MEMORY.md off the UI thread: {search}"
        );
        assert!(
            search.contains("TEXT_FILE_CAP") || search.contains("search_thread_body"),
            "History Search must not join every 8MB thread on the UI thread: {search}"
        );
        assert!(
            search.contains("thread_idx") && search.contains("self.messages"),
            "History Search must include the live pane, not only persisted thread copies: {search}"
        );
        assert!(
            !search.contains("content.clone()"),
            "History Search must not clone an 8MB pane to include the live tab: {search}"
        );
        let soul = search.find("read_memory(\"SOUL.md\")").expect("history soul");
        assert!(
            search[..soul].contains("thread::spawn") && search.contains("history_rx"),
            "History Search must slurp SOUL/USER/MEMORY off the UI thread: {search}"
        );
        let board = src
            .split("fn ui_board(")
            .nth(1)
            .and_then(|s| s.split("fn ui_imagine(").next())
            .expect("ui_board");
        assert!(
            board.contains("self.flush_board()")
                && !board.contains("self.persist()")
                && !board.contains("persist_snap"),
            "Workboard add/status must not clone every thread just to write board.json: {board}"
        );
        let flush_b = src
            .split("fn flush_board(")
            .nth(1)
            .and_then(|s| s.split("fn nav_from_id(").next())
            .expect("flush_board");
        assert!(
            flush_b.contains("persist_idle_now") && flush_b.contains("save_board"),
            "Workboard flush must bump the idle key or persist_bg clones every thread 2s later: {flush_b}"
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
        let enable = night
            .split("checkbox")
            .nth(1)
            .and_then(|s| s.split("ui.vertical").next())
            .expect("night enable");
        assert!(
            enable.contains(".changed()") && enable.contains("night::save"),
            "toggling an automation must persist enabled before restart: {enable}"
        );
        assert!(
            enable.contains("thread::spawn"),
            "toggling an automation must persist off the UI thread: {enable}"
        );
        assert!(
            night.contains("persist_idle_key") && !night.contains("self.persist()"),
            "removing an automation must not clone every thread 2s later — bump the idle key so persist_bg skips: {night}"
        );
        let added = src
            .split("fn add_automation_seed(")
            .nth(1)
            .and_then(|s| s.split("fn ui_night(").next())
            .expect("add_automation_seed");
        let added_spawn = added.find("thread::spawn").expect("night add must leave the UI thread");
        let added_save = added.find("night::save").expect("night::save");
        assert!(
            added_spawn < added_save,
            "Add automation must not freeze the cabin writing automations.json: {added}"
        );
        assert!(
            added.contains("persist_idle_key") && !added.contains("self.persist()"),
            "Add automation must not clone every thread 2s later — bump the idle key so persist_bg skips: {added}"
        );
        assert!(
            night.contains("land_on_real_chat"),
            "Night Run must not send_chat on Scratch: {night}"
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
    fn stream_deltas_do_not_grow_without_bound() {
        let src = include_str!("app.rs");
        let delta = src
            .split("Ok(JobOut::ChatDelta(d))")
            .nth(1)
            .and_then(|s| s.split("Ok(JobOut::ThoughtDelta(d))").next())
            .expect("ChatDelta");
        assert!(
            delta.contains("IMAGE_FILE_CAP"),
            "stream deltas must not grow stream_buf without bound: {delta}"
        );
        let thought = src
            .split("Ok(JobOut::ThoughtDelta(d))")
            .nth(1)
            .and_then(|s| s.split("Ok(JobOut::Chat {").next())
            .expect("ThoughtDelta");
        assert!(
            thought.contains("IMAGE_FILE_CAP"),
            "thought deltas must not grow thought_buf without bound: {thought}"
        );
        let snap = src
            .split("fn apply_assistant_snapshot(")
            .nth(1)
            .and_then(|s| s.split("fn push_bound_msg(").next())
            .expect("apply_assistant_snapshot");
        assert!(
            snap.contains("IMAGE_FILE_CAP"),
            "a huge complete reply must not land in the transcript unbounded: {snap}"
        );
        let live = src
            .split("fn apply_live_assistant(")
            .nth(1)
            .and_then(|s| s.split("fn has_key(").next())
            .expect("apply_live_assistant");
        assert!(
            live.contains("merge_thinking_capped")
                || live.contains("take_ui_text")
                || live.contains("IMAGE_FILE_CAP"),
            "live thought+stream merge must not allocate two 8MB buffers unbounded: {live}"
        );
        assert!(
            live.contains("TEXT_FILE_CAP"),
            "live snapshot must not copy an 8MB stream into the transcript every delta: {live}"
        );
        assert!(
            delta.contains("if push_stream_capped") || delta.contains("changed"),
            "leftover deltas after the stream cap must not re-merge on the UI thread: {delta}"
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
        let strip = chat.find("strip_thinking(&text)").expect("strip complete");
        assert!(
            chat[..strip].contains("take_ui_text") || chat[..strip].contains("IMAGE_FILE_CAP"),
            "Chat complete must not strip/merge a 64MB worker body on the UI thread: {chat}"
        );
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
        assert!(
            chat.contains("should_auto_continue_goal"),
            "goal continue must not send_chat while host is running: {chat}"
        );
        let queued = chat
            .split("self.agents.push")
            .nth(1)
            .expect("auto-continue queue");
        assert!(
            queued.contains("thread_id") && queued.contains("\"running\""),
            "auto-continue must remember the origin thread and mark the queue row running: {queued}"
        );
        assert!(
            chat.contains("estimate_messages")
                && (chat.contains("chat_job_thread") || chat.contains("job.as_deref()")),
            "auto-compact must use the origin thread, not only the visible tab: {chat}"
        );
        assert!(
            !chat.contains("t.messages.clone()") && !chat.contains("content.clone()"),
            "Chat complete must not clone an 8MB transcript to estimate/compact: {chat}"
        );
        assert!(
            chat.contains("should_auto_compact_now(tokens, CONTEXT_BUDGET_TOKENS, compact_step)"),
            "auto-compact must use the post-outcome goal step, not the pre-outcome job_step: {chat}"
        );
        let bg_compact = chat
            .split("compact_keep_start_from")
            .nth(1)
            .expect("background compact");
        assert!(
            bg_compact.contains("accessed_ms") && !chat.contains("compact_keep_pin(&t.messages"),
            "background auto-compact must drain dropped turns without cloning an 8MB pane: {bg_compact}"
        );
        let pins = chat.find("extract_work_pins").expect("work pins");
        assert!(
            chat[..pins].contains("bound_scan"),
            "Chat complete must not walk an 8MB body for pins/recipe/host/connectors: {chat}"
        );
        assert!(
            chat[pins..].contains("self.persist()")
                && !chat.contains("if let Some(plan) = plan_from_text"),
            "Chat complete must persist pins and must not parse HOST_CMD: {chat}"
        );
        assert!(
            chat.contains("goal_continue_pin"),
            "empty goal_pin must fall back to the last user task: {chat}"
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
            .and_then(|s| s.split("fn land_on_real_chat(").next())
            .expect("open_recent_chat");
        assert!(
            open.contains("most_recently_accessed_index") && open.contains("switch_thread"),
            "Chat rail uses last-access, not leftover thread_idx: {open}"
        );
        let land = src
            .split("fn land_on_real_chat(")
            .nth(1)
            .and_then(|s| s.split("fn new_thread(").next())
            .expect("land_on_real_chat");
        assert!(
            land.contains("scratch()") && land.contains("apply_switch_thread"),
            "background chat must leave Scratch for the last real thread: {land}"
        );
        assert!(
            land.contains("most_recently_accessed_index") && !land.contains("self.persist()"),
            "leaving Scratch for a night/inbox job must not clone every thread twice: {land}"
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
        let idle = src
            .split("HeartbeatAct::Reflect =>")
            .nth(1)
            .and_then(|s| s.split("HeartbeatAct::Anticipate =>").next())
            .expect("idle reflect");
        assert!(
            idle.contains("scratch()"),
            "idle reflect must not consume the slot on Scratch: {idle}"
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
