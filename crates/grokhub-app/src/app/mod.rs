use crate::build_agent;
use crate::config::{self, AppConfig};
use crate::desktop::{
    capture_data_url, capture_webcam, clipboard_image, collect_rows, first_bin,
    load_image_data_url, lock_titles, pick_file, play_audio, prepare_windshield, read_text_capped,
    record_once, run_computer_op_cancel, run_limited, transcribe_local,
};
use crate::helpers::{
    cabin_menu_should_dismiss, click_project_opens_board, collect_other_chip_threads, expand_home,
    next_maximized, wants_live_repaint,
};
use crate::host::{host_working_dir, resolve_host_cite_path, run_host, run_host_stream};
use crate::secrets::{self, Secrets};
use crate::skills;
use crate::threads::{self, ChatThread};
use crate::titlebar::{
    apply_tray_window, titlebar_chrome_btn, titlebar_chrome_hit, titlebar_should_start_drag,
    ChromeBtn,
};
use crate::update::{remember_source, resolve_source};
use crate::xai::{grok_chat, grok_imagine_opts, grok_imagine_video, grok_stt, grok_tts};
use eframe::egui::{self, Color32, ColorImage, RichText, TextureHandle, TextureOptions};
use global_hotkey::{
    hotkey::{Code, HotKey, Modifiers},
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
};
use grokhub_acp::{
    classify_stream_error, grok_context_line, grok_usage_line, inspect_advisory, kill_pid,
    merge_tool_card, retry_status_line, rewrite_truncation_error, turn_footer, AcpEvent,
    GrokPEvent, GrokUsage, PermissionMode, SessionMode, StreamErrorKind, ToolCard,
};
#[allow(unused_imports)]
use grokhub_core::{
    add_to_folder, add_tokens, anticipate_consumes_slot, anticipated_need, appearance_choices,
    appearance_hint, append_composer, append_say, append_thought, append_tool, apply_auto_title_in,
    apply_job_error, apply_manual_rename, attach_kind, attach_name,
    attach_prompt_line, automation_blocked_by_policy, automation_schedule_label,
    automation_summary_line, blend_thread_goal, bound_scan, btw_queues_without_interrupt,
    bubble_outer_width, bubble_wrap_width,
    build_hub_snapshot, build_quick_chips, build_review_digest, build_windshield, bump_skill_run,
    bump_usage, cabin_overlay_step, cabin_update_notice, cap_from_text, catalog_line,
    chat_attach_status, chat_bearer, chat_run_action, chat_run_hint,
    chat_run_label, chat_run_phase, chat_send_kind, chat_shows_thinking, chat_stream_is_visible,
    chip_scan, chip_suggest_prompt, clamp_bubble_outer, clamp_row_width,
    clear_pending_after_complete, cli_update_notice, cluster_gap, combined_update_cmds,
    compact_keep_start_from, compose_imagine_prompt, composer_enter,
    composer_go, composer_go_tip, computer_cmd_line, context_fingerprint, context_percent,
    create_folder, create_project, daily_units_blocked, dedicated_imagine_model,
    dedicated_video_model, dedupe_hits, dedupe_suggestions, default_openclaw_paths, delete_thread,
    dismiss_accepted_auto,
    devices_shows_pair_code, diagnostics_bundle, digest_line_from, drop_node,
    drop_selected, drop_trailing_assistant, due_automations, due_loops, ensure_automation_schedule,
    estimate_messages, estimate_messages_from, extract_imagine_prompt, extract_insights,
    apply_assistant_work_marks, extract_work_pins, extract_work_updates, fact_candidates,
    fact_candidates_from, filter_palette, inflight_card_title, settle_inflight_card,
    upsert_inflight_card,
    filter_slash_hits, flush_visible_goal, fold_stream_fields, folder_choices, forbidden_reason,
    forget_topic, fork_offer_why, format_consult_reply, frame_bytes, goal_continue_pin, goal_pin_for_job,
    goal_step_after_outcome, greet_from_last_job, greeting_fingerprint, greeting_name,
    greeting_prompt, grok_cli_update_cmd, grok_command_hits, has_auth, has_verify_ok,
    history_list_refresh_due,
    heartbeat_acts, heartbeat_due, heartbeat_repaint_ms, hey_grok_on_press, hey_grok_route,
    hey_grok_starts_ptt, home_slash_cmd, home_surface_from_nav, host_cmd_leaves_project,
    host_hour_blocked, host_risk, host_status_line, hub_dispatch_ok, hub_kind_from_health,
    hub_pair_url, imagine_aspect_label, imagine_aspect_name, imagine_image_resolution,
    imagine_ref_status, imagine_stage_h, imagine_stage_visible, imagine_style_label,
    imagine_toolbox_dock, imagine_toolbox_shows_title, imagine_toolbox_top,
    imagine_video_dur_label, imagine_video_duration_secs, imagine_video_res_label,
    imagine_video_resolution, imagine_wall_bounds, import_memory_file, inbox_claim_ready,
    inhabit_claim_allowed, inhabit_ready, insight_pin, is_cabin_first_run, is_hard_run,
    is_openclaw_workspace, is_plain_text, is_rewind_copy_cmd, is_rewind_copy_cmd_in,
    is_voice_error, is_workload_user, job_error_goes_to_chat, job_is_scratch, keep_last_rewinds,
    lan_bind_in_use, last_imagine_receipt, last_user_scan, last_user_text, leftover_empty_thread, load_hub_state,
    local_greeting, lock_blocks_hands, mark_automation_ran, mark_automation_skipped, mark_loop_ran,
    mark_slash_result, match_skill, merge_hub_snapshots, merge_imported_memory,
    merge_suggestion_store, merge_thinking_capped, mint_host_halt, mode_from_chip_value,
    model_for_mode, nav_from_chip_value, new_loop, next_chat_image, next_goal_prompt,
    next_heartbeat_wait_ms, night_check_command, night_check_exit_code, night_check_may_fire,
    night_counts_run, night_unauth_should_skip, normalize_hm, now_ms, oauth_access_live,
    overlay_update_begin, overlay_update_finish, pair_code_is_live, parse_computer_op,
    parse_consult, parse_fast_topics, parse_goal_outcome, parse_hostname_i, parse_llm_chips,
    parse_local_clock, parse_recipe, parse_slash, parse_suggest_lines, parse_suggest_skill_patches,
    parse_theme, parse_trajectory_jsonl, partition_suggestions, patch_skill,
    pending_for_manual_update, perm_key,
    palette_file_shown, palette_forget_stale_walk, palette_row_action,
    palette_search_is_saved, persist_user_turn, pick_fresh_seed, pick_greeting, pick_lan_ipv4, pick_theme, plan_room,
    plus_empty_status, plus_menu_rows, push_stream_capped, prefer_patch, presence_should_stream,
    project_menu_acts, project_menu_label, project_title_from_hint, propose_skill_from_turn,
    prune_live_suggestions, ptt_after_speak, ptt_after_stt, quiet_hours_active,
    quiet_hours_choice_label, quiet_hours_menu, quote_for_reply, realtime_can_connect, recall_hits,
    recipe_from_cmds, record_turn, redact_held_secrets, redact_secrets, redirect_prompt,
    reduce_voice_state, refresh_last_stretch, refund_host_reserved, refused_lock,
    remember_chip_click, remember_chip_dismiss, remember_chip_outcome, remember_home_slash,
    remember_home_surface, remember_typed_prompt, rename_node, replay_automation_target,
    replay_ops, reply_needs_followup, resolve_acp_cwd, resolve_bind_path, resolve_chat_model,
    resolve_dark, restore_bound_path, retain_held_plan, reuse_empty_thread_idx, review_due,
    review_status_line, review_system_prompt, rewind_allowed, rewind_blocked_reason,
    rewind_copy_cmd, rewind_dest, rewind_restore_matches, rewind_snapshot_ready, roll_usage_day,
    route_schedule, save_hub_state, screen_from_extents, scrolled_off_tail, search_corpus,
    search_corpus_tagged, search_place, search_thread_body, seed_from_bound, settings_pin_blocks_auto,
    settings_update_hint, settings_update_label,
    settle_project_path, shortcut_help, should_anticipate, should_auto_compact_now,
    should_auto_continue_goal, should_capture_before_chat, should_idle_reflect, should_keep_frame,
    should_name_thread, should_notify_cabin_update, should_paint_greeting, should_refresh_greeting,
    should_refresh_llm, should_seed_sidebar, should_send_screenshot, should_trim_result_bodies,
    should_update_cli_alpha, apply_skill_follow, skill_follow_block, skill_from_suggestion,
    skill_offer_chip, skill_use_in_chat_prompt,
    skip_night_check_receipt, slash_help, slash_kind, stage_project, start_hub_rotates_pair,
    state_for_disk, stretch_saved_skill, strip_thinking, summarize_trajectory, summarize_write,
    suggestions_from_sessions, surgical_memory_edit, take_ui_text, teach_routine, teachable_steps, theme_id, theme_label,
    thought_body_key, thought_control_act, thought_fold_controls, thought_fold_draws,
    thought_shows_acts, thought_shows_label, thread_goal_prompt, thread_host_receipts,
    thread_host_receipts_from, toggle_folder, toggle_pin, token_delta, top_habit_labels,
    trajectory_jsonl_line, transcribe_route, trim_result_bodies_in_place, uid, unified_diff_cite,
    unknown_cabin_slash, update_check_due, update_chip_label, update_pending, update_wipes_config,
    upsert_assistant_turn, upsert_bound, usage_line, user_asked_to_schedule, user_pref_facts,
    verify_ok_after_user_turn, views_up_to_last_user, visible_chat_refs,
    visible_goal_step_on_continue, visible_tree, visible_turn_count, visible_turn_count_from,
    voice_log_role, voice_mode_active, voice_mode_label, voice_state_after_ptt_stt,
    voice_stream_token, voice_strip_visible,
    voice_transcript_sends_chat, voice_tts_script, wall_can_paint, wall_evict,
    wall_gif_from_generation, worker_gone_status, yesterday_ms, AttachKind, Automation, BoardCard,
    BoardStatus, ChatKind, ChatRunPhase, ChatSendKind, ChatView, ChipInput, ChipKind, ChipMemory,
    KanbanColumn,
    ChipThread, ComposerEnter, ComposerGo, DeleteOutcome, DeviceCodeStart, DigestLine,
    GreetingInput, GrokLoop, HeartbeatAct, HeyGrokAction, HeyGrokRoute, HostPlanStep, HostRisk,
    HubMemoryFile, HubSnapshot, HubState, ImagineKind, ImagineSpec, ImagineToolboxDock,
    ImagineWall, InhabitBundle, LearningState, LiveBlock, LiveKind, LocalClock, MemoryEdit,
    MintRealtimeFn, PermKey, PlusAct, PlusTarget, Policy, PresenceFrame, ProjectKind,
    ProjectMenuAct, ProjectNode, PttLine, QuickChip, Recipe, ReplayOp, ReviewDigest, RewindRecord,
    ScheduleRoute, SkillMd, Slash, SlashHit, StreamTokenKind, SuggestionStore, ThreadReuseView,
    ThreadTab, ThoughtFold, TranscribeRoute, UpdatePending, UsageDay,
    VerifyResult, VoiceEvent, VoiceState,
    WallGif, BUBBLE_PAD_X, BUBBLE_PAD_Y, CABIN_FAST_FALLBACK, CABIN_FAST_MODEL, CABIN_GITHUB_TOOLS,
    CHAT_TAIL_FRAMES, CHAT_TAIL_SLACK, CHIP_VISIBLE_MAX, CONTEXT_BUDGET_TOKENS, FOLLOWUP_MAX_STEPS,
    FORK_EXPLAINER,
    FOLLOWUP_PROMPT, FRAME_CAP, GOAL_DROP_AFTER, GOAL_MAX_STEPS, HEARTBEAT_MS, HUB_KIND,
    IDLE_REFLECT_MS, IMAGE_FILE_CAP, IMAGINE_ASPECTS, IMAGINE_STYLES, IMAGINE_WALL_GAP, LOOP_MAX,
    PRESENCE_RING_MS, RESULT_TRIM_KEEP_HOPS, REVIEW_NIGHT_HOUR, SKILL_SAVED_MARK, SKILL_SAVED_NOTE,
    TEXT_FILE_CAP, THOUGHT_ROW_LABEL, TRANSCRIBERS, UPDATE_CHECK_EVERY, WALL_GIF_EVERY_MS,
    WALL_GIF_MAX,
};
use grokhub_hub::serve_lan;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

mod persist;
mod acp;
mod chat_kick;
mod palette;
mod settings;
mod plus;
mod projects;
mod slash;
mod oauth;
mod imagine;
mod night;
mod chat_ui;
mod pulse;
mod confirm;
mod glance;
mod sidebar;
mod pages;
mod jobs;
mod chips;
mod voice;
mod threads_nav;
#[cfg(test)]
mod tests;

#[allow(unused_imports)]
use acp::*;
#[allow(unused_imports)]
use chat_ui::*;
#[allow(unused_imports)]
use chips::*;
#[allow(unused_imports)]
use imagine::*;
#[allow(unused_imports)]
use oauth::*;
#[allow(unused_imports)]
use plus::*;
#[allow(unused_imports)]
use pulse::*;
#[allow(unused_imports)]
use confirm::*;
#[allow(unused_imports)]
use glance::*;
#[allow(unused_imports)]
use settings::*;
#[allow(unused_imports)]
use sidebar::*;
#[allow(unused_imports)]
use persist::*;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Nav {
    Chat,
    Devices,
    Memory,
    Workboard,
    Imagine,
    Skills,
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
    Update,
    About,
    Defaults,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SettingsGroup {
    General,
    About,
}

const HIDDEN_HEARTBEAT_MS: u64 = 400;

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

/// How long a History query sits still before it runs. Every keystroke walks every
/// thread, so the cabin waits for the typing to settle.
const HISTORY_TYPE_DELAY: Duration = Duration::from_millis(250);
const RAIL_FOOTER_H: f32 = 52.0;
const PALETTE_LIST_H: f32 = 280.0;
const PROFILE_NAME_MAX: usize = 64;

/// Latest cabin config a background save may write. Name keystrokes and picture
/// changes each publish a generation. A writer saves only its own generation.
struct CfgSlot {
    gen: u64,
    cfg: AppConfig,
}

fn publish_cfg(slot: &mut CfgSlot, mut cfg: AppConfig) -> u64 {
    cfg.api_key.clear();
    let mut gen = slot.gen.wrapping_add(1);
    if gen == 0 {
        gen = 1;
    }
    slot.gen = gen;
    slot.cfg = cfg;
    gen
}

fn cfg_if_current(slot: &CfgSlot, gen: u64) -> Option<AppConfig> {
    if gen != 0 && slot.gen == gen {
        Some(slot.cfg.clone())
    } else {
        None
    }
}

enum TabAct {
    Switch(usize),
    Pin(usize),
    StartRename(usize),
    CommitRename(usize),
    CancelRename,
    Delete(usize),
    OpenGrok(String),
    DeleteGrok(String),
}

enum JobOut {
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

/// `(directory listed, [(entry name, is_dir)])` from the Plus file picker.
type PickList = (String, Vec<(String, bool)>);
/// History search: needle + `[(path_or_id, snippet)]`.
type HistoryHitsRx = mpsc::Receiver<(String, Vec<(String, String)>)>;

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
    cfg_slot: Arc<Mutex<CfgSlot>>,
    board: Vec<BoardCard>,
    board_title: String,
    board_notes: String,
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
    grok_loops: Vec<GrokLoop>,
    grok_loop_rx: Option<(String, mpsc::Receiver<String>)>,
    night_nl: String,
    /// One-shot watch on the Automations page. Not a second clock.
    watch_once: bool,
    watched_steps: Vec<String>,
    teach_nl: String,
    /// Frames left pulling the chat pane to its newest message after a chat opens.
    chat_tail_frames: u8,
    /// Cap fields are typed, so they hold text until Save parses them.
    cap_auto_buf: String,
    cap_host_buf: String,
    /// Quiet-hour clocks are typed, so they hold text until Save parses them.
    quiet_start_buf: String,
    quiet_end_buf: String,
    history_q: String,
    /// Last query the debounce saw, so typing kicks a search without a button.
    history_q_seen: String,
    history_q_at: Option<Instant>,
    history_hits: Vec<(String, String)>,
    last_receipt_ok: Option<bool>,
    last_receipts: Vec<(String, bool)>,
    try_again: bool,
    last_rewind_id: Option<String>,
    rewind_rows: Vec<RewindRecord>,
    host_live: String,
    daily_auto_used: u32,
    daily_auto_day: String,
    slash_pick: usize,
    slash_filter_n: usize,
    slash_filter_first: String,
    last_window_title: String,
    voice_orb: String,
    last_night_tick: Instant,
    last_auto_tick: Instant,
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
    /// Relative paths from the last palette file walk. Empty is a finished result.
    palette_files: Vec<String>,
    palette_files_q: String,
    palette_files_root: String,
    palette_file_rx: Option<mpsc::Receiver<(String, String, Vec<String>)>>,
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
    voice_ready_at: Option<Instant>,
    voice_hold_rx: Option<mpsc::Receiver<()>>,
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
    mcp_nl: String,
    mcp_compose: bool,
    pending_connectors: Vec<(String, String, String)>,
    auto_compose: bool,
    board_compose: bool,
    board_edit: Option<String>,
    board_link: bool,
    settings_menu_open: bool,
    settings_menu_ignore: bool,
    win_max: bool,
    geom_dirty: bool,
    geom_applied: bool,
    geom_apply_frames: u8,
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
    imagine_expand: bool,
    imagine_job_prompt: String,
    imagine_error: String,
    imagine_pending: bool,
    imagine_save_rx: Option<mpsc::Receiver<Result<String, String>>>,
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
    pick_list_rx: Option<mpsc::Receiver<PickList>>,
    pick_dir: String,
    pick_cache: Option<PickList>,
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
    profile_photo: Option<TextureHandle>,
    profile_photo_key: String,
    profile_photo_rx: Option<mpsc::Receiver<ProfilePhotoOut>>,
    profile_photo_busy: bool,
    profile_pick_rx: Option<mpsc::Receiver<(u64, ProfilePick)>>,
    profile_pick_token: Arc<AtomicU64>,
    profile_file_io: Arc<Mutex<()>>,
    grok_install_rx: Option<mpsc::Receiver<Result<std::path::PathBuf, String>>>,
    grok_install_err: String,
    /// Official alpha install this session (missing/unusable at boot or retry).
    grok_install_wait: bool,
    /// Stay on Get Started after this session's official install lands.
    official_cli_session: bool,
    cabin_latest: Option<String>,
    /// Published Grok Build CLI alpha (`x.ai/cli/alpha`), not the stable channel.
    cli_alpha: Option<String>,
    /// `grok --version` from the last probe. Missing means Install, not Update.
    cli_installed: Option<String>,
    last_update_probe: Option<Instant>,
    update_probe_rx: Option<mpsc::Receiver<crate::update::UpdateProbe>>,
    /// Cabin files were just overlaid. The running binary is still old until Restart.
    cabin_overlay_done: bool,
    /// Update clicked while a chat or other job holds `running`. Starts when idle.
    queued_overlay: Option<Vec<String>>,
    /// Both was pending and the cabin half could not be built. CLI still runs.
    update_cabin_note: Option<String>,
    acp: Option<grokhub_acp::AcpHandle>,
    acp_spawn_rx: Option<mpsc::Receiver<Result<grokhub_acp::AcpHandle, String>>>,
    grok_p_rx: Option<mpsc::Receiver<GrokPEvent>>,
    grok_p_pid: Option<u32>,
    grok_usage: GrokUsage,
    /// Last Grok token totals already banked into `usage.json`.
    tokens_seen: (u64, u64, u64),
    grok_commands: Vec<SlashHit>,
    grok_tasks: Vec<(String, String, bool)>,
    followup_queue: Vec<String>,
    /// btw questions waiting until the live turn ends. They do not cancel it.
    side_ask_queue: Vec<String>,
    /// Next kick uses SessionMode::Ask even if the pill changes before spawn.
    side_ask_kick: bool,
    /// Plan card open. The body lives on the thread (`plan_body`).
    plan_open: bool,
    /// Once-only fork how-it-works. Disk flag `fork_explainer_seen`, not AppConfig.
    fork_explainer_seen: bool,
    tool_cards: Vec<ToolCard>,
    live_blocks: Vec<LiveBlock>,
    desk_frame: Option<String>,
    perm_ask: Option<grokhub_acp::PermissionAsk>,
    /// Ask-card Always second beat for this `rpc_id` only. Not the composer pill.
    perm_always_confirm: Option<serde_json::Value>,
    /// Session Always escalate / destructive host. Ask Always stays on `perm_always_confirm`.
    confirm: Option<ConfirmKind>,
    /// History "Last you" scroll once the thread is open.
    jump_last_you: bool,
    elicit_ask: Option<grokhub_acp::ElicitAsk>,
    elicit_draft: String,
    /// Secret values typed into an elicit. Memory only — never persisted.
    secret_hold: Vec<String>,
    session_mode: SessionMode,
    permission_mode: PermissionMode,
    /// Night / loop / phone `/v1/task` inherit the composer PermissionMode pill.
    scheduled_perm: bool,
    grok_sessions: Vec<grokhub_acp::GrokSession>,
    grok_sessions_loaded: bool,
    grok_sessions_tx: mpsc::Sender<GrokSessMsg>,
    grok_sessions_rx: mpsc::Receiver<GrokSessMsg>,
    grok_list_gen: u64,
    grok_sessions_inflight: u32,
    grok_sessions_refresh_pending: bool,
    last_grok_list_at: Instant,
    pending_grok_deletes: HashSet<String>,
    inspect_rx: Option<mpsc::Receiver<String>>,
    history_rx: Option<HistoryHitsRx>,
    mem_restore_rx: Option<mpsc::Receiver<(String, Result<String, String>)>>,
    mem_file_rx: Option<(String, mpsc::Receiver<(u64, String)>)>,
    recall_rx: Option<mpsc::Receiver<String>>,
    sync_rx: Option<mpsc::Receiver<(String, Vec<HubMemoryFile>)>>,
    inhabit_rx: Option<mpsc::Receiver<InhabitBundle>>,
    reflect_rx: Option<mpsc::Receiver<(MemoryEdit, Option<MemoryEdit>)>>,
    session_show_rx: Option<(String, mpsc::Receiver<String>)>,
    import_rx: Option<mpsc::Receiver<ImportOpenclawOut>>,
    inspect_text: String,
    grok_catalog: grokhub_acp::GrokCatalog,
    grok_catalog_loaded: bool,
    grok_catalog_rx: Option<mpsc::Receiver<Result<grokhub_acp::GrokCatalog, String>>>,
    grok_ext_rx: Option<mpsc::Receiver<String>>,
}

fn fork_explainer_path() -> PathBuf {
    config::config_dir().join("fork_explainer_seen")
}

fn fork_explainer_seen_on_disk() -> bool {
    std::fs::read_to_string(fork_explainer_path())
        .map(|s| s.trim() == "1")
        .unwrap_or(false)
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
        if cfg.source_dir.trim().is_empty() {
            if let Some(src) = resolve_source("") {
                remember_source(&src);
                cfg.source_dir = src.display().to_string();
            }
        }
        let mut projects = crate::store::load_projects();
        let sidebar_file = crate::store::projects_path().exists();
        let home = std::env::var("HOME").ok();
        let profile = std::env::var("USERPROFILE").ok();
        let work =
            grokhub_core::cabin_work_root(cfg!(windows), home.as_deref(), profile.as_deref());
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
        secrets::ensure_private();
        let win_max = cfg.window.maximized;
        let cfg_auto_cap = cfg.daily_auto_cap;
        let cfg_host_cap = cfg.host_hour_cap;
        let cfg_quiet_start = cfg.quiet_start.clone();
        let cfg_quiet_end = cfg.quiet_end.clone();
        let boot_session = SessionMode::parse(&cfg.session_mode).unwrap_or(SessionMode::Chat);
        let boot_perm = PermissionMode::parse(&cfg.permission_mode).unwrap_or(PermissionMode::Ask);
        let goal_step = threads.get(thread_idx).map(|t| t.goal.step).unwrap_or(0);
        let (grok_sessions_tx, grok_sessions_rx) = mpsc::channel();
        let cfg_slot = Arc::new(Mutex::new(CfgSlot {
            gen: 0,
            cfg: cfg.clone(),
        }));
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
            cfg_slot,
            board: config::load_board(),
            board_title: String::new(),
            board_notes: String::new(),
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
            grok_loops: crate::loops::load(),
            grok_loop_rx: None,
            night_nl: String::new(),
            watch_once: false,
            watched_steps: Vec::new(),
            teach_nl: String::new(),
            chat_tail_frames: CHAT_TAIL_FRAMES,
            cap_auto_buf: cfg_auto_cap.to_string(),
            cap_host_buf: cfg_host_cap.to_string(),
            quiet_start_buf: cfg_quiet_start,
            quiet_end_buf: cfg_quiet_end,
            history_q: String::new(),
            history_q_seen: String::new(),
            history_q_at: None,
            history_hits: vec![],
            last_receipt_ok: None,
            last_receipts: vec![],
            try_again: false,
            last_rewind_id: None,
            rewind_rows: crate::night::load_rewinds(),
            host_live: String::new(),
            daily_auto_used: 0,
            daily_auto_day: String::new(),
            slash_pick: 0,
            slash_filter_n: 0,
            slash_filter_first: String::new(),
            last_window_title: String::new(),
            voice_orb: "idle".into(),
            last_night_tick: Instant::now(),
            last_auto_tick: Instant::now(),
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
            palette_files: Vec::new(),
            palette_files_q: String::new(),
            palette_files_root: String::new(),
            palette_file_rx: None,
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
            voice_ready_at: None,
            voice_hold_rx: None,
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
            mcp_nl: String::new(),
            mcp_compose: false,
            pending_connectors: vec![],
            auto_compose: false,
            board_compose: false,
            board_edit: None,
            board_link: false,
            settings_menu_open: false,
            settings_menu_ignore: false,
            win_max,
            geom_dirty: false,
            geom_applied: false,
            geom_apply_frames: 0,
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
            imagine_expand: false,
            imagine_job_prompt: String::new(),
            imagine_error: String::new(),
            imagine_pending: false,
            imagine_save_rx: None,
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
            profile_photo: None,
            profile_photo_key: String::new(),
            profile_photo_rx: None,
            profile_photo_busy: false,
            profile_pick_rx: None,
            profile_pick_token: Arc::new(AtomicU64::new(0)),
            profile_file_io: Arc::new(Mutex::new(())),
            grok_install_rx: None,
            grok_install_err: String::new(),
            grok_install_wait: false,
            official_cli_session: false,
            cabin_latest: None,
            cli_alpha: None,
            cli_installed: None,
            last_update_probe: None,
            update_probe_rx: None,
            cabin_overlay_done: false,
            queued_overlay: None,
            update_cabin_note: None,
            acp: None,
            acp_spawn_rx: None,
            grok_p_rx: None,
            grok_p_pid: None,
            grok_usage: GrokUsage::default(),
            tokens_seen: (0, 0, 0),
            grok_commands: Vec::new(),
            grok_tasks: Vec::new(),
            followup_queue: Vec::new(),
            side_ask_queue: Vec::new(),
            side_ask_kick: false,
            plan_open: false,
            fork_explainer_seen: fork_explainer_seen_on_disk(),
            tool_cards: Vec::new(),
            live_blocks: Vec::new(),
            desk_frame: None,
            perm_ask: None,
            perm_always_confirm: None,
            confirm: None,
            jump_last_you: false,
            elicit_ask: None,
            elicit_draft: String::new(),
            secret_hold: Vec::new(),
            session_mode: boot_session,
            permission_mode: boot_perm,
            scheduled_perm: false,
            grok_sessions: Vec::new(),
            grok_sessions_loaded: false,
            grok_sessions_tx,
            grok_sessions_rx,
            grok_list_gen: 0,
            grok_sessions_inflight: 0,
            grok_sessions_refresh_pending: false,
            last_grok_list_at: Instant::now(),
            pending_grok_deletes: HashSet::new(),
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
            grok_catalog: grokhub_acp::GrokCatalog::default(),
            grok_catalog_loaded: false,
            grok_catalog_rx: None,
            grok_ext_rx: None,
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
        grokhub_acp::silence_windows_hard_errors();
        // Official alpha when missing/unusable; pin a working CLI. UAC is expected on Windows.
        c.grok_install_wait =
            grokhub_core::should_kick_alpha_install(grokhub_acp::find_grok().is_some());
        c.official_cli_session = c.grok_install_wait;
        c.grok_install_rx = Some(grokhub_acp::begin_ensure_grok_alpha());
        c.sync_cli_auth_from_oauth();
        if grokhub_acp::grok_cli_key().is_some() && !c.official_cli_session {
            c.mark_get_started_done();
        }
        c.last_update_probe = Some(Instant::now());
        c.update_probe_rx = Some(crate::update::begin_update_probe());
        c
    }

    fn apply_saved_geom(&mut self, ctx: &egui::Context) {
        let g = crate::window::clamp_geom(self.cfg.window);
        #[cfg(windows)]
        if g.maximized && self.window_visible {
            if let Some((x, y, w, h)) = crate::win_native::work_area() {
                let ppp = ctx.pixels_per_point().max(0.5);
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                    w as f32 / ppp,
                    h as f32 / ppp,
                )));
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
                    x as f32 / ppp,
                    y as f32 / ppp,
                )));
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));
                let _ = crate::win_native::show_cabin(x, y, w, h, true);
                let dark = grokhub_core::resolve_dark(
                    grokhub_core::parse_theme(&self.cfg.theme),
                    crate::theme::desktop_prefers_dark(),
                );
                crate::win_native::polish_hwnd(dark);
                self.win_max = true;
                self.cfg.window.maximized = true;
                self.geom_applied = true;
                self.geom_apply_frames = 0;
                return;
            }
            self.win_max = true;
            return;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(g.w, g.h)));
        if let Some([x, y]) = crate::window::launch_pos(&g) {
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(x, y)));
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(g.maximized));
        self.win_max = g.maximized;
        self.geom_applied = true;
        self.geom_apply_frames = 0;
    }

    fn capture_window(&mut self, ctx: &egui::Context) {
        if !self.window_visible {
            return;
        }
        if !self.geom_applied {
            self.apply_saved_geom(ctx);
            return;
        }
        self.geom_apply_frames = self
            .geom_apply_frames
            .saturating_add(1)
            .min(crate::window::GEOM_SETTLE_FRAMES);
        if !crate::window::geom_can_remember(self.geom_applied, self.geom_apply_frames) {
            return;
        }
        let (outer, inner, egui_max) = ctx.input(|i| {
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
        #[cfg(windows)]
        let maximized = {
            let _ = egui_max;
            self.win_max
        };
        #[cfg(not(windows))]
        let maximized = egui_max.unwrap_or(self.win_max);
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

    fn run_phase_here(&self) -> ChatRunPhase {
        if !self.thinking_here() {
            return ChatRunPhase::Idle;
        }
        let waiting = self.perm_ask.is_some() || self.elicit_ask.is_some();
        let has_output = !self.stream_buf.trim().is_empty()
            || self
                .live_blocks
                .iter()
                .any(|b| matches!(b.kind, LiveKind::Say | LiveKind::Tool));
        chat_run_phase(true, waiting, has_output)
    }

    fn run_action_here(&self) -> String {
        let waiting = self
            .perm_ask
            .as_ref()
            .map(|p| p.title.as_str())
            .or_else(|| self.elicit_ask.as_ref().map(|p| p.server_name.as_str()));
        let tool = self
            .live_blocks
            .iter()
            .rev()
            .find(|b| b.kind == LiveKind::Tool && !b.tool_title.is_empty())
            .map(|b| b.tool_title.as_str())
            .or_else(|| {
                self.tool_cards
                    .iter()
                    .rev()
                    .find(|c| !c.title.is_empty())
                    .map(|c| c.title.as_str())
            });
        chat_run_action(waiting, tool).to_string()
    }

    fn halt_in_flight(&mut self) {
        self.host_halt.store(true, Ordering::SeqCst);
        if let Some(h) = &self.acp {
            if let Some(p) = self.perm_ask.take() {
                let _ = h.answer_permission(p.rpc_id, false);
            }
            self.perm_always_confirm = None;
            self.confirm = None;
            if let Some(p) = self.elicit_ask.take() {
                let _ = h.answer_elicit(p.rpc_id, "cancel", None);
            }
            let _ = h.cancel();
            while let Ok(ev) = h.try_recv() {
                match ev {
                    AcpEvent::Permission(p) => {
                        let _ = h.answer_permission(p.rpc_id, false);
                    }
                    AcpEvent::Elicit(p) => {
                        let _ = h.answer_elicit(p.rpc_id, "cancel", None);
                    }
                    _ => {}
                }
            }
        }
        self.rx = None;
        self.voice_hold_rx = None;
        self.running = false;
        self.imagine_pending = false;
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
        if let Some(pid) = self.grok_p_pid.take() {
            kill_pid(pid);
        }
        self.grok_p_rx = None;
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
        self.scheduled_perm = false;
        self.stream_buf.clear();
        self.thought_buf.clear();
        self.perm_ask = None;
        self.perm_always_confirm = None;
        self.confirm = None;
        self.elicit_ask = None;
        self.elicit_draft.clear();
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
            self.voice_state = VoiceState::Idle;
            self.voice_orb = "idle".into();
        }
    }

    fn scrub_transcript(&self, content: String) -> String {
        redact_held_secrets(&content, &self.secret_hold)
    }

    fn scrub_live_blocks(&mut self) {
        if self.secret_hold.is_empty() {
            return;
        }
        for b in &mut self.live_blocks {
            if !b.body.is_empty() {
                b.body = redact_held_secrets(&b.body, &self.secret_hold);
            }
            if !b.tool_title.is_empty() {
                b.tool_title = redact_held_secrets(&b.tool_title, &self.secret_hold);
            }
            if !b.tool_detail.is_empty() {
                b.tool_detail = redact_held_secrets(&b.tool_detail, &self.secret_hold);
            }
        }
    }

    fn hold_secret(&mut self, value: &str) {
        let t = value.trim();
        if t.chars().count() < 4 || self.secret_hold.iter().any(|s| s == t) {
            return;
        }
        self.secret_hold.push(t.to_string());
    }

    fn apply_assistant_snapshot(&mut self, content: String) {
        let content = self.scrub_transcript(take_ui_text(content, IMAGE_FILE_CAP));
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
        let target = self.chat_job_thread.clone().unwrap_or(vis);
        if let Some(t) = self.threads.iter_mut().find(|t| t.id == target) {
            t.accessed_ms = now_ms();
        }
    }

    fn push_bound_msg(&mut self, role: &str, content: String) {
        let content = self.scrub_transcript(take_ui_text(content, IMAGE_FILE_CAP));
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
        self.has_key()
            || grokhub_acp::find_grok().is_some()
            || grokhub_acp::grok_cli_key().is_some()
    }

    fn grok_cwd(&self) -> std::path::PathBuf {
        let home = std::env::var("HOME").ok();
        let profile = std::env::var("USERPROFILE").ok();
        std::path::PathBuf::from(grokhub_core::cabin_session_cwd(
            &self.cfg.project_dir,
            cfg!(windows),
            home.as_deref(),
            profile.as_deref(),
        ))
    }

    /// `grok sessions list` returns only sessions stored under its cwd.
    /// That directory is the chat `--cwd`, not HOME and not the install dir.
    fn grok_cli_cwd(&self) -> std::path::PathBuf {
        self.grok_cwd()
    }

    fn poll_history_search(&mut self) {
        let Some(rx) = self.history_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok((q, hits)) => {
                if q == self.history_q {
                    self.history_hits = hits;
                    self.status = format!("{} hits", self.history_hits.len());
                }
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
                self.live_mut()
                    .push(("assistant".into(), mark_slash_result(&body)));
                self.stamp_current_access();
                self.persist();
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.recall_rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {}
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
        let visible = || last_user_scan(self.messages.iter().map(|m| (m.0.as_str(), m.1.as_str())));
        if job.is_none() || job == Some(vis.as_str()) {
            return visible().unwrap_or_default();
        }
        self.threads
            .iter()
            .find(|t| Some(t.id.as_str()) == job)
            .and_then(|t| last_user_scan(t.messages.iter().map(|(r, c)| (r.as_str(), c.as_str()))))
            .or_else(visible)
            .unwrap_or_default()
    }

    pub(super) fn remember_skill(&mut self, skill: SkillMd) {
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
        self.push_bound_msg("user", SKILL_SAVED_MARK.into());
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
        let origin = self.chat_job_thread.clone().unwrap_or_else(|| vis.clone());
        let here = self.chat_job_thread.as_deref().is_none_or(|id| id == vis);
        let tokens = if here {
            estimate_messages_from(self.messages.iter().map(|m| (m.0.as_str(), m.1.as_str())))
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
                self.live_mut().iter_mut().map(|m| (m.0.as_str(), &mut m.1)),
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
        if should_confirm_destructive_host(&cmd) {
            self.confirm = Some(ConfirmKind::DestructiveHost { cmd });
            self.status = "Confirm host…".into();
            return;
        }
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
        let peer_count = self.hub.lock().ok().map(|s| s.peers.len()).unwrap_or(0);
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

    /// The composer pills survive a restart: Ask/Auto/Plan is a preference, not a per-run
    /// choice. Always-approve is the exception — `config::load` drops it back to Ask.
    fn set_session_mode(&mut self, mode: SessionMode) {
        self.session_mode = mode;
        self.cfg.session_mode = mode.as_str().to_string();
        self.persist_cfg();
    }

    /// `replace` overwrites a plan event. Turn-end fill keeps a structured plan.
    pub(super) fn store_session_plan(&mut self, text: &str, replace: bool) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        let id = self
            .chat_job_thread
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| self.visible_thread_id());
        let Some(t) = self.threads.iter_mut().find(|t| t.id == id) else {
            return;
        };
        if !replace && !t.plan_body.trim().is_empty() {
            return;
        }
        if t.plan_body == text {
            return;
        }
        let first = t.plan_body.trim().is_empty();
        t.plan_body = text.to_string();
        if first {
            self.plan_open = true;
        }
    }

    pub(super) fn dismiss_fork_explainer(&mut self) {
        self.fork_explainer_seen = true;
        let _ = std::fs::write(fork_explainer_path(), b"1");
    }

    fn set_permission_mode(&mut self, mode: PermissionMode) {
        self.permission_mode = mode;
        self.cfg.permission_mode = crate::config::persistable_permission_mode(mode.as_str());
        self.persist_cfg();
    }

    /// Grok reports session totals; `/usage` wants a day. Bank the delta so the token
    /// line survives a restart and does not double-count a resumed session.
    fn merge_grok_usage(&mut self, u: &GrokUsage) {
        self.grok_usage.merge(u);
        self.roll_today();
        let seen = self.tokens_seen;
        let now = (
            self.grok_usage.input_tokens,
            self.grok_usage.output_tokens,
            self.grok_usage.reasoning_tokens,
        );
        let spent = (
            token_delta(seen.0, now.0),
            token_delta(seen.1, now.1),
            token_delta(seen.2, now.2),
        );
        if spent == (0, 0, 0) {
            return;
        }
        self.tokens_seen = now;
        add_tokens(&mut self.usage, spent.0, spent.1, spent.2);
        self.persist_usage();
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
                HeartbeatAct::Night => {
                    // Grok `/loop` intervals and clock-time cabin automations both live on
                    // this slot. Loops go first; automations still get the pulse when no
                    // loop is due, so a 09:00 job is not starved by an idle loop list.
                    night_fired = self.tick_loops();
                    if !night_fired {
                        night_fired = self.tick_night();
                    }
                }
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
        self.send_scheduled_chat(prompt);
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

    fn tick_home_surface(&mut self) {
        let Some(surface) = home_surface_from_nav(self.nav_id()) else {
            return;
        };
        if self.chip_memory.last_surface.as_deref() == Some(surface) {
            return;
        }
        remember_home_surface(&mut self.chip_memory, surface, now_ms());
    }

    fn cabin_signed_in(&self) -> bool {
        self.secrets
            .oauth
            .as_ref()
            .is_some_and(|t| !t.access_token.trim().is_empty())
            || grokhub_acp::grok_cli_key().is_some()
    }

    fn has_real_history(&self) -> bool {
        self.threads.iter().any(|t| {
            !t.scratch
                && !t.messages.is_empty()
                && !t.title.trim().is_empty()
                && !t.title.eq_ignore_ascii_case("chat")
                && !t.title.eq_ignore_ascii_case("scratch")
        })
    }

    fn last_project_title_from(last_night: &str, continue_hint: &str, goal_pin: &str) -> String {
        let from_hint = project_title_from_hint(continue_hint);
        if !from_hint.is_empty() {
            return from_hint;
        }
        let from_night = project_title_from_hint(last_night);
        if !from_night.is_empty() {
            return from_night;
        }
        let pin = goal_pin.trim();
        if pin.chars().count() <= 28 && !pin.contains('.') {
            return project_title_from_hint(pin);
        }
        String::new()
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

    fn sync_cli_auth_from_oauth(&self) {
        let Some(tokens) = self.secrets.oauth.clone() else {
            return;
        };
        std::thread::spawn(
            move || match grokhub_acp::write_cli_auth_if_needed(&tokens) {
                Ok(_) => {}
                Err(e) => eprintln!("grok auth.json: {e}"),
            },
        );
    }

    fn mark_get_started_done(&mut self) {
        self.official_cli_session = false;
        if self.cfg.get_started_done {
            return;
        }
        self.cfg.get_started_done = true;
        self.persist_cfg();
    }

    fn queue_grok_cli_install(&mut self) {
        if self.grok_install_rx.is_some() {
            return;
        }
        self.grok_install_err.clear();
        self.grok_install_wait = true;
        self.official_cli_session = true;
        self.status = "Installing Grok Build CLI (alpha)…".into();
        self.grok_install_rx = Some(grokhub_acp::begin_grok_install_force());
    }

    /// GitHub Latest and Grok Build CLI alpha. First check is at launch; then every 2 hours.
    fn poll_update_probe(&mut self, ctx: &egui::Context) {
        if let Some(rx) = self.update_probe_rx.take() {
            match rx.try_recv() {
                Ok(probe) => {
                    if let Some(tag) = probe.cabin_tag {
                        self.cabin_latest = Some(tag);
                    }
                    if let Some(alpha) = probe.cli_alpha {
                        self.cli_alpha = Some(alpha);
                    }
                    if let Some(installed) = probe.cli_installed {
                        self.cli_installed = Some(installed);
                    }
                }
                Err(mpsc::TryRecvError::Empty) => {
                    self.update_probe_rx = Some(rx);
                }
                Err(mpsc::TryRecvError::Disconnected) => {}
            }
        }
        if self.update_probe_rx.is_none()
            && update_check_due(self.last_update_probe, Instant::now(), UPDATE_CHECK_EVERY)
        {
            self.last_update_probe = Some(Instant::now());
            self.update_probe_rx = Some(crate::update::begin_update_probe());
        }
        if self.update_probe_rx.is_some() {
            ctx.request_repaint_after(Duration::from_millis(250));
        } else if let Some(started) = self.last_update_probe {
            let wait = UPDATE_CHECK_EVERY.saturating_sub(started.elapsed());
            ctx.request_repaint_after(wait.max(Duration::from_secs(1)));
        }
    }

    fn cabin_update_available(&self) -> bool {
        !self.cabin_overlay_done
            && should_notify_cabin_update(env!("CARGO_PKG_VERSION"), self.cabin_latest.as_deref())
    }

    fn update_pending_now(&self) -> UpdatePending {
        let cli = should_update_cli_alpha(self.cli_installed.as_deref(), self.cli_alpha.as_deref());
        update_pending(cli, self.cabin_update_available())
    }

    fn open_update_overlay(&mut self) {
        self.nav = Nav::Settings;
        self.settings_sec = SettingsSec::Update;
    }

    /// One control: CLI alpha first when it is newer, then the cabin when it is newer.
    /// A Settings / `/update` click with nothing pending still overlays both.
    fn queue_combined_update(&mut self) {
        self.open_update_overlay();
        let pending = pending_for_manual_update(self.update_pending_now());
        let src = resolve_source(&self.cfg.source_dir);
        if pending != UpdatePending::Cli
            && src
                .as_ref()
                .is_some_and(|p| grokhub_core::overlay_clone_usable(p))
        {
            if let Some(src) = src.as_ref() {
                self.cfg.source_dir = src.display().to_string();
                remember_source(src);
                self.persist_cfg();
            }
        }
        let plan = match combined_update_cmds(src.as_deref(), pending) {
            Ok(plan) => plan,
            Err(e) if pending == UpdatePending::Both => {
                match combined_update_cmds(src.as_deref(), UpdatePending::Cli) {
                    Ok(mut plan) => {
                        plan.cabin_skipped = Some(e);
                        plan
                    }
                    Err(cli_e) => {
                        self.open_update_overlay();
                        self.status = cli_e;
                        return;
                    }
                }
            }
            Err(e) => {
                self.open_update_overlay();
                self.status = e;
                return;
            }
        };
        if update_wipes_config(&plan.cmds) {
            self.open_update_overlay();
            self.status = "refusing an update that would wipe config".into();
            return;
        }
        self.update_cabin_note = plan.cabin_skipped;
        self.start_overlay_update(plan.cmds);
    }

    fn note_combined_update_landed(&mut self) {
        if self.last_host.iter().any(|c| grok_cli_update_cmd(c)) {
            if let Some(alpha) = self.cli_alpha.clone() {
                self.cli_installed = Some(alpha);
            }
        }
        if self.last_host.iter().any(|c| cabin_overlay_step(c)) {
            self.cabin_overlay_done = true;
        }
    }

    fn poll_grok_install(&mut self) {
        let Some(rx) = self.grok_install_rx.take() else {
            return;
        };
        match rx.try_recv() {
            Ok(Ok(_)) => {
                grokhub_acp::clear_grok_unusable();
                grokhub_acp::invalidate_grok_bin_cache();
                self.grok_install_err.clear();
                self.grok_install_wait = false;
                self.status = "Grok Build CLI (alpha) installed".into();
            }
            Ok(Err(e)) => {
                self.grok_install_err = e.clone();
                self.grok_install_wait = true;
                self.status = e;
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.grok_install_rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {}
        }
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

    fn apply_compact_status(&mut self, started: bool, usage: GrokUsage, error: Option<String>) {
        self.merge_grok_usage(&usage);
        if let Some(e) = error.filter(|s| !s.trim().is_empty()) {
            self.status = format!("Compact failed: {e}");
            return;
        }
        let ctx = grok_context_line(&self.grok_usage);
        self.status = if started {
            if ctx.is_empty() {
                "Compacting…".into()
            } else {
                format!("Compacting… {ctx}")
            }
        } else if ctx.is_empty() {
            "Compacted".into()
        } else {
            format!("Compacted · {ctx}")
        };
    }

    fn apply_job_fail(&mut self, err: &str) -> String {
        if grokhub_acp::is_sigterm_status(err) {
            return "Stopped".into();
        }
        if classify_stream_error(err) == StreamErrorKind::CreditLimit {
            self.try_again = true;
            self.last_receipt_ok = Some(false);
        }
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
        self.queue_combined_update();
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
                    crate::tray::drop_tray(tray);
                }
                self.want_quit = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Err(e) => self.status = e,
        }
    }

    fn start_overlay_update(&mut self, cmds: Vec<String>) {
        self.nav = Nav::Settings;
        self.settings_sec = SettingsSec::Update;
        if self.running {
            self.queued_overlay = Some(cmds);
            self.status = "Update queued — it starts when this job finishes.".into();
            return;
        }
        self.queued_overlay = None;
        if cmds.is_empty() {
            self.status = "Update plan empty".into();
            return;
        }
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
                Err(e) if crate::update::host_receipt_failed(&e) => {
                    JobOut::UpdateDone { ok: false }
                }
                Err(e) => JobOut::Err(e),
            });
        });
    }

    fn drain_queued_update(&mut self) {
        if self.running {
            return;
        }
        let Some(cmds) = self.queued_overlay.take() else {
            return;
        };
        self.start_overlay_update(cmds);
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
                    fact_candidates_from(self.messages.iter().map(|m| (m.0.as_str(), m.1.as_str())))
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
            if name != "MEMORY.md" && name != "USER.md" && config::read_memory(&name) != body {
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
            if let Some(s) = self
                .skill_list
                .iter_mut()
                .find(|s| s.name == self.skill_name)
            {
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
                            && !lock_blocks_hands(
                                &lock.iter().map(|s| s.as_str()).collect::<Vec<_>>(),
                            )
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
                            self.eyes_text = self
                                .eyes_text
                                .replace("frame: capturing…\n", "frame: captured\n");
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
        let text = voice_tts_script(text);
        if text.is_empty() {
            self.maybe_continue_ptt();
            return;
        }
        if self.voice_is_on() {
            self.voice_state = VoiceState::Speaking;
            self.voice_orb = "speaking".into();
            self.status = voice_mode_label(VoiceState::Speaking).into();
        }
        let key = self.bearer();
        let cap = TEXT_FILE_CAP;
        let mut end = cap.min(text.len());
        while end > 0 && !text.is_char_boundary(end) {
            end -= 1;
        }
        let text = text[..end].to_string();
        let hold = self.voice_is_on();
        let tx = if hold {
            let (tx, rx) = mpsc::channel();
            self.voice_hold_rx = Some(rx);
            Some(tx)
        } else {
            None
        };
        std::thread::spawn(move || {
            if let Ok(bytes) = grok_tts(&key, &text) {
                let path = std::env::temp_dir().join("grokhub-speak.mp3");
                if std::fs::write(&path, bytes).is_ok() {
                    let _ = play_audio(&path);
                }
            }
            if let Some(tx) = tx {
                let _ = tx.send(());
            }
        });
    }

    #[allow(dead_code)]
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
            t.push_str(&format!(
                "- [{}] {} @{},{} {}x{}\n",
                o.kind, o.label, o.x, o.y, o.w, o.h
            ));
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
        let rx = self.eyes_cap_rx.take()?;
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
                    self.eyes_text = self
                        .eyes_text
                        .replace("frame: capturing…\n", &format!("frame: {e}\n"));
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
                self.eyes_text = self
                    .eyes_text
                    .replace("frame: capturing…\n", "frame: captured\n");
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
        self.maybe_continue_ptt();
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
        let task = self
            .hub
            .lock()
            .ok()
            .and_then(|mut s| s.take_next_queued(&id));
        if let Some(t) = task {
            self.pending_hub_task = Some(t.id.clone());
            self.land_on_real_chat();
            self.send_scheduled_chat(format!("[from {}] {}", t.from_name, t.prompt));
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
        let already = self.told_tray || self.cfg.close_to_tray_tip_seen;
        match crate::tray::hide_action(self.window_visible, already) {
            crate::tray::HideAction::Skip => {}
            crate::tray::HideAction::Hide => {
                self.unmap_to_tray(ctx);
            }
            crate::tray::HideAction::HideAndPing => {
                let clock = Self::local_clock();
                let quiet =
                    quiet_hours_active(&clock.hm(), &self.cfg.quiet_start, &self.cfg.quiet_end);
                let tip = crate::tray::tray_tip_on_hide(already, quiet);
                // Mark seen only when the toast is actually shown, and do it
                // before unmap so the hide persist writes the flag.
                if tip.show {
                    crate::notify::ping("GrokHub", "Still running in the tray");
                }
                if tip.mark_seen {
                    self.told_tray = true;
                    self.cfg.close_to_tray_tip_seen = true;
                }
                self.unmap_to_tray(ctx);
                self.status = "In the tray — Show cabin to sit down".into();
            }
        }
    }

    fn unmap_to_tray(&mut self, ctx: &egui::Context) {
        self.capture_window(ctx);
        self.persist_if_dirty();
        self.geom_dirty = false;
        self.window_visible = false;
        self.tray_saw_unfocused = false;
        self.tray_hid_at = Instant::now();
        apply_tray_window(ctx, crate::tray::hide_to_tray_window());
        #[cfg(windows)]
        crate::win_native::hide_cabin();
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
        #[cfg(windows)]
        {
            let g = crate::window::clamp_geom(self.cfg.window);
            if let Some((x, y, w, h)) = crate::win_native::work_area() {
                if self.win_max {
                    let _ = crate::win_native::show_cabin(x, y, w, h, true);
                } else {
                    let ppp = ctx.pixels_per_point().max(0.5);
                    let x = (g.x.unwrap_or(100.0) * ppp).round() as i32;
                    let y = (g.y.unwrap_or(100.0) * ppp).round() as i32;
                    let w = (g.w * ppp).round() as i32;
                    let h = (g.h * ppp).round() as i32;
                    let _ = crate::win_native::show_cabin(x, y, w, h, false);
                }
            }
        }
        self.apply_saved_geom(ctx);
        self.ensure_tray_spawn();
        ctx.request_repaint();
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
                    crate::tray::drop_tray(tray);
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
        match self.bind_lan_hub() {
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

    fn bind_lan_hub(&self) -> Result<u16, String> {
        match serve_lan(self.hub.clone(), self.hub_port) {
            Ok(p) => Ok(p),
            Err(e) if lan_bind_in_use(&e) => {
                let ours = hub_kind_from_health(
                    crate::desktop::probe_hub_health_body(self.hub_port).as_deref(),
                ) == HUB_KIND;
                if ours && crate::update::stop_user_unit("grokhub-hub.service") {
                    serve_lan(self.hub.clone(), self.hub_port)
                } else {
                    Err(e)
                }
            }
            Err(e) => Err(e),
        }
    }
}

impl eframe::App for Cabin {
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        let mut cfg = self.cfg.clone();
        cfg.api_key.clear();
        let _ = crate::config::save(&cfg);
        if let Some(tray) = self.tray.take() {
            crate::tray::drop_tray(tray);
        }
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_job();
        self.poll_imagine_save();
        self.poll_host_diff();
        self.poll_acp();
        self.poll_chips();
        self.poll_review();
        self.poll_greeting();
        self.poll_goals();
        self.tick_home_surface();
        self.refresh_chips();
        self.refresh_greeting();
        self.drain_inbox();
        self.poll_tray(ctx);
        self.poll_voice();
        self.poll_voice_hold();
        self.poll_global_hotkeys();
        self.poll_night_check(now_ms());
        self.poll_grok_loop();
        self.poll_wall();
        self.poll_persist();
        self.poll_grok_sessions();
        self.poll_inspect();
        self.poll_grok_catalog();
        self.poll_grok_ext();
        self.poll_history_search();
        self.poll_palette_search();
        self.poll_mem_restore();
        self.poll_mem_file();
        self.poll_recall();
        self.poll_sync();
        self.poll_inhabit();
        self.poll_reflect();
        self.poll_session_show();
        self.poll_import_openclaw();
        self.poll_acp_spawn();
        self.poll_single();
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
        if crate::tray::ignore_close_request(self.window_visible, close_requested, self.want_quit) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
        } else if close_requested {
            let hide =
                crate::tray::should_hide_on_close(self.cfg.close_to_tray, self.tray.is_some())
                    && !self.want_quit;
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
        self.poll_grok_install();
        self.poll_update_probe(ctx);
        if self.grok_install_rx.is_some() {
            ctx.request_repaint_after(Duration::from_millis(250));
        }
        if self.oauth_pending.is_some()
            || self.oauth_start_rx.is_some()
            || self.oauth_poll_rx.is_some()
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
        self.poll_profile_pick();
        self.poll_profile_photo(ctx);
        if !self.composer.trim().is_empty()
            || ctx.input(|i| {
                i.pointer.any_pressed()
                    || i.events.iter().any(|e| {
                        matches!(
                            e,
                            egui::Event::Text(_) | egui::Event::Key { pressed: true, .. }
                        )
                    })
            })
        {
            self.touch();
        }
        if ctx
            .input(|i| i.modifiers.command && i.modifiers.shift && i.key_pressed(egui::Key::Escape))
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
                || self.grok_sessions_inflight > 0
                || self.persist_rx.is_some()
                || self.inspect_rx.is_some()
                || self.grok_catalog_rx.is_some()
                || self.grok_ext_rx.is_some()
                || self.grok_loop_rx.is_some()
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
                || self.grok_p_rx.is_some()
                || self.pick_rx.is_some()
                || self.pick_list_rx.is_some()
                || self.profile_pick_rx.is_some()
                || self.profile_photo_busy
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
            #[cfg(windows)]
            crate::win_native::hide_cabin_stubs_only();
            let focused = ctx.input(|i| i.viewport().focused.unwrap_or(false));
            self.tray_saw_unfocused =
                crate::tray::remember_hidden_unfocus(focused, self.tray_saw_unfocused);
            let since_ms = self.tray_hid_at.elapsed().as_millis() as u64;
            let tick = if crate::tray::hidden_raise_ready(since_ms) {
                crate::tray::hidden_window_tick(true, focused, just_hid, self.tray_saw_unfocused)
            } else {
                crate::tray::HiddenTick::StayHidden
            };
            match tick {
                crate::tray::HiddenTick::Raise => self.show_from_tray(ctx),
                crate::tray::HiddenTick::StayHidden => {
                    if crate::tray::reapply_unmap(true, focused) {
                        apply_tray_window(ctx, crate::tray::hide_to_tray_window());
                        #[cfg(windows)]
                        crate::win_native::hide_cabin();
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
        self.drain_queued_update();
        self.ui_titlebar(ctx);
        // First-run wait / Get Started takes CentralPanel — a Foreground Area over chat does not paint on Windows.
        if !self.ui_get_started(ctx) {
            self.ui_sidebar(ctx);
            self.ui_settings_menu(ctx);

            match self.page_nav() {
                Nav::Chat => self.ui_chat(ctx),
                Nav::Devices => self.ui_devices(ctx),
                Nav::Memory => self.ui_memory(ctx),
                Nav::Workboard => self.ui_board(ctx),
                Nav::Imagine => self.ui_imagine(ctx),
                Nav::Skills => self.ui_skills(ctx),
                Nav::Night => self.ui_night(ctx),
                Nav::History => self.ui_history(ctx),
                Nav::Command => self.ui_command(ctx),
                Nav::Connectors => self.ui_connectors(ctx),
                Nav::Agents => self.ui_agents(ctx),
                Nav::Settings => self.ui_chat(ctx),
            }
        }
        // Latest chip → Settings → Update must paint on top of Get Started.
        // A first-frame Area over empty chat does not draw; this overlay opens
        // after GitHub Latest returns, when the window is already sized.
        if self.nav == Nav::Settings {
            self.ui_settings(ctx);
        }
        if self.palette_open {
            self.ui_palette(ctx);
        }
        if self.confirm.as_ref().is_some_and(|c| c.paints_overlay()) {
            self.paint_confirm_overlay(ctx);
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
}

fn paint_running(ui: &mut egui::Ui, label: &str, hint: &str) {
    crate::cards::paint_run_pulse(ui, label, hint)
}

fn paint_one_tool_card(ui: &mut egui::Ui, card: &ToolCard) {
    let label = if card.title.is_empty() {
        "Work"
    } else {
        card.title.as_str()
    };
    egui::CollapsingHeader::new(
        RichText::new(label)
            .size(crate::theme::FONT_META)
            .color(crate::theme::muted()),
    )
    .id_salt(("tool-card", card.id.as_str(), label))
    .default_open(false)
    .show(ui, |ui| {
        paint_tool_card_body(ui, card);
    });
}

fn paint_tool_card_body(ui: &mut egui::Ui, card: &ToolCard) {
    egui::Frame::none()
        .fill(egui::Color32::TRANSPARENT)
        .rounding(crate::theme::CHROME_RADIUS)
        .stroke(egui::Stroke::new(1.0_f32, crate::theme::border()))
        .inner_margin(egui::Margin::same(8.0))
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
            if !card.diff.is_empty()
                && !card.diff.trim().starts_with('{')
                && !card.diff.trim().starts_with('[')
            {
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

fn project_row_active(selected: bool, is_project: bool, _nav: Nav) -> bool {
    selected && is_project
}

fn health_settings_sec() -> SettingsSec {
    SettingsSec::About
}

fn select_all_edit(ui: &egui::Ui, id: egui::Id, text: &str) {
    let mut state = egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
    let end = text.chars().count();
    state
        .cursor
        .set_char_range(Some(egui::text::CCursorRange::two(
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
