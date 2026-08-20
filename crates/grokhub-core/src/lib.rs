//! Shared GrokHub brain. Linux, Windows, and Android must call this — not a second protocol.

pub mod appearance;
pub mod attach;
pub mod autonomy;
pub mod browser;
pub mod feel;
pub mod automation;
pub mod chat;
pub mod chat_bubble;
pub mod chat_job;
pub mod capture;
pub mod chat_view;
pub mod chips;
pub mod connector;
pub mod consult;
pub mod context;
pub mod diagnostics;
pub mod doctor;
pub mod frame;
pub mod goal;
pub mod greeting;
pub mod hands;
pub mod heartbeat;
pub mod history;
pub mod host_cite;
pub mod host_plan;
pub mod host_safety;
pub mod hub_sync;
pub mod hygiene;
pub mod imagine;
pub mod inhabit;
pub mod learning;
pub mod models;
pub mod oauth;
pub mod openclaw;
pub mod organs;
pub mod pair;
pub mod project;
pub mod recipe;
pub mod redact;
pub mod reflect;
pub mod review;
pub mod rewind;
pub mod shortcuts;
pub mod skill;
pub mod slash;
pub mod state;
pub mod stream;
pub mod task;
pub mod trajectory;
pub mod thread_tab;
pub mod update;
pub mod usage;
pub mod verify;
pub mod voice;
pub mod windshield;
pub mod workboard;

pub use appearance::{
    appearance_choices, appearance_hint, os_prefers_dark, parse_theme, pick_theme, resolve_dark,
    theme_id, theme_label, ThemeChoice,
};
pub use feel::{
    feel_scale, felt_rect, hover_alpha, hover_mix, lift_rgb, mix_channel, HOVER_EXPANSION,
    HOVER_SECS, PRESS_EXPANSION, PRESS_SECS,
};
pub use autonomy::{
    anticipate_consumes_slot, anticipated_need, autonomy_policy, cabin_system_prompt,
    host_plan_autorun, host_step_autorun, should_anticipate, HostAuto, LearnMode, Policy,
    SkillFollow, SkillWrite,
};
pub use attach::{
    append_composer, attach_kind, attach_name, attach_prompt_line, chat_attach_status,
    cabin_eyes_request_text, cabin_frame_only, clip_image_args, imagine_ref_status, list_pick_names,
    kick_consumes_attach, next_chat_image, parse_picker_stdout, this_turn_cabin_frame,
    picker_args, plus_empty_status, plus_menu_rows, take_text_body, AttachKind, PlusAct, PlusTarget,
    IMAGE_FILE_CAP, IMAGE_PIXEL_CAP, MEDIA_FILE_CAP, TEXT_FILE_CAP, bound_scan, image_pixels_ok, png_ihdr_size,
};
pub use chat::{
    chat_request_body, chat_request_body_for_mode, chat_request_body_vision, chat_timeout_secs,
    effective_chat_mode, extract_host_cmds, failover_model, is_composer_ladder_model, model_for_mode,
    needs_auth_banner, paint_connect_banner, parse_chat_content, parse_chat_reasoning, parse_model_reasoning, parse_model_text,
    parse_responses_reasoning, parse_responses_text, reasoning_effort_for_mode, resolve_chat_model,
    responses_request_body, responses_url, route_auto_mode, settings_pin_blocks_auto,
    should_failover_status, CABIN_FAST_FALLBACK, CABIN_FAST_MODEL, DEFAULT_MODEL, XAI_BASE,
};
pub use chat_view::{
    assistant_prose, is_workload_user, merge_thinking, merge_thinking_capped, quote_for_reply, scrub_thought, strip_thinking,
    refresh_last_stretch, visible_chat, visible_chat_refs, visible_turn_count, visible_turn_count_from, ChatKind, ChatView,
};
pub use chat_bubble::{
    bubble_max_width, bubble_outer_height, bubble_outer_width, bubble_wrap_width, clamp_row_width,
    BUBBLE_MAX_FRAC, BUBBLE_PAD_X, BUBBLE_PAD_Y, BUBBLE_RADIUS,
};
pub use chat_job::{
    apply_job_error, apply_stream_snapshot, chat_send_kind, chat_shows_thinking, chat_stream_is_visible,
    drop_trailing_assistant, drop_trailing_assistant_on, job_error_goes_to_chat, job_is_scratch,
    kick_messages_for_job,
    last_user_for_job,
    persist_user_turn, push_bound_message, upsert_assistant_turn, worker_gone_status, ChatSendKind,
};
pub use chips::{
    build_quick_chips, chip_memory_key, chip_scan, chip_suggest_prompt, chip_thread_from_messages,
    context_fingerprint, detect_chip_context, detect_chip_stage, empty_chip_memory,
    mode_from_chip_value, nav_from_chip_value, parse_llm_chips, predict_intents, prune_retired_chip_memory, remember_chip_click,
    remember_chip_dismiss, remember_chip_outcome, remember_typed_prompt, should_refresh_llm,
    top_habit_labels, ChipInput, ChipKind, ChipMemory, ChipStage, ChipThread, PredictedIntent,
    QuickChip, CHIP_LLM_DEBOUNCE_MS, CHIP_LLM_MODE, CHIP_VISIBLE_MAX,
};
pub use doctor::{doctor_extras, doctor_hands_line, doctor_lines, doctor_ok, DoctorLine};
pub use capture::{
    capture_kinds, clamp_to_desktop, cursor_on_output, ffmpeg_webcam_args, ffmpeg_x11_args,
    format_cursor_line, format_cursor_line_miss, frame_is_blank, gnome_shell_screenshot_args,
    frame_origin_for, pointer_slop_miss,
    grim_capture_args, image_to_global, infer_wayland_display, layout_prompt, luma_mean_var,
    monitor_local_to_global, output_containing, parse_xdpy_size, parse_xrandr_outputs,
    parse_xrandr_size, pick_capture_output, session_is_wayland, virtual_desktop_size,
    windshield_frame_geom, x11_grab_size, CaptureKind, DisplayOutput,
};
pub use frame::{encode_b64, frame_bytes, jpeg_data_url, store_frame, FrameGet, PresenceFrame, FRAME_CAP};
pub use host_plan::{
    approved_cmds, explain_host_risk, host_risk, move_step, parse_host_plan, plan_from_text,
    retain_held_plan, step_from_cmd, strip_host_cmd_line, yolo_plan_split, HostPlanStep, HostRisk,
};
pub use host_safety::{forbidden_reason, mint_host_halt, recall_hits};
pub use imagine::{
    compose_imagine_prompt, curate_wall, dedicated_imagine_model, dedicated_video_model,
    extract_imagine_prompt, imagine_aspect_label, imagine_aspect_name, imagine_dest,
    imagine_image_body, imagine_image_fallback_model, imagine_image_quality,
    imagine_image_resolution, imagine_image_shaped, imagine_is_video_path, imagine_receipt_path,
    imagine_request_body, imagine_should_retry_model, imagine_slug, imagine_style_label,
    imagine_toolbox_dock, imagine_toolbox_shows_title, imagine_toolbox_top, imagine_result_fit,
    imagine_shows_result_above, imagine_video_fallback_model, imagine_wall_bounds,
    imagine_wall_overlaps_toolbox, imagine_video_dur_label, imagine_video_duration_secs,
    imagine_video_res_label, imagine_video_resolution, last_imagine_receipt, media_ext_from_bytes,
    parse_imagine_url, parse_video_job_status, parse_video_request_id, parse_video_url,
    pick_fresh_seed, retired_imagine_model, video_moderation_blocked, video_request_body,
    wall_can_paint, wall_curate_seed, wall_due, wall_evict, ImagineKind, ImagineSpec,
    ImagineToolboxDock, ImagineWall, VideoJobStatus, WallGif, WallSeed, WallSlot,
    DEFAULT_IMAGINE_MODEL, DEFAULT_VIDEO_MODEL, FALLBACK_IMAGINE_MODEL, FALLBACK_VIDEO_MODEL,
    IMAGINE_ASPECTS, IMAGINE_STYLES, IMAGINE_TOOLBOX_PAD, IMAGINE_VIDEO_DURS, IMAGINE_VIDEO_RES,
    IMAGINE_WALL_GAP,
    WALL_GIF_EVERY_MS, WALL_GIF_MAX, WALL_SEEDS,
};
pub use inhabit::{
    can_inhabit, inhabit_bundle_usable, inhabit_claim_allowed, inhabit_ready, InhabitBundle,
};
pub use hands::{
    diagnose_hands, extra_bin_dirs, hands_chip_label, hands_chip_live, hands_down_receipt,
    hands_windshield_line, resolve_bin_in, ydotool_socket_path, HandsDown, HANDS_PACMAN,
    PYATSPI_MISSING,
};
pub use browser::{
    browser_windshield_line, cdp_activate_payload, cdp_new_tab_path, cdp_page_close_payload,
    cdp_page_focus_payload, format_tab_list, match_browser_tabs, parse_cdp_targets, pick_browser_tab,
    BrowserTab, CDP_DOWN, CDP_PORTS,
};
pub use recipe::{
    act_window_search_bin, bin_on_path, computer_cmd_line, computer_drive, computer_drive_for, default_bin_extra_dirs,
    empty_hands_steps_error, extract_computer_ops, hands_backend_name,
    hands_blocked_by_lock, hands_protocol, lock_blocks_hands, pointer_op_blocked_on_lock,
    needs_reshoot, parse_computer_cmd_loose, parse_computer_op, parse_recipe, parse_screen,
    pick_hands_backend, recipe_from_cmds, recipe_from_json, recipe_to_json, relative_move_steps,
    replay_ops,
    screen_from_extents, see_drive_attach, should_attach_hands_frame, user_asks_cabin_eyes,
    user_asks_desktop_hands, user_asks_gui_help, user_asks_guide_only, user_asks_takeover,
    hands_step_label, ComputerDrive, ComputerOp, HandsBackend, Recipe, RecipeDoc, ReplayOp,
    ScreenSize, TabAction,
};
pub use heartbeat::{
    heartbeat_acts, heartbeat_due, heartbeat_repaint_ms, next_heartbeat_wait_ms, HeartbeatAct,
    HEARTBEAT_MS,
};
pub use reflect::{
    fact_candidates, fact_candidates_from, restore_memory_prev, should_idle_reflect, surgical_memory_edit, MemoryEdit,
    IDLE_REFLECT_MS,
};
pub use review::{
    build_review_digest, cabin_real_text, dedupe_suggestions, digest_line_from, merge_suggestion_store,
    parse_suggest_lines, parse_suggest_skill_patches, partition_suggestions, prune_live_suggestions, review_due,
    review_status_line, review_system_prompt,
    DigestLine, LearnedSuggestion, SkillPatch,
    ReviewDigest, SuggestionKind, SuggestionStore, CABIN_GITHUB_TOOLS, DIGEST_LINE_CAP, REVIEW_NIGHT_HOUR, SUGGEST_CAP,
};
pub use pair::{
    devices_shows_pair_code, hub_pair_url, make_pair_code, normalize_code, pair_code_is_live,
    parse_hostname_i, pick_lan_ipv4, start_hub_rotates_pair, CODE_ALPH, PAIR_TTL_MS,
};
pub use automation::{
    automation_blocked_by_policy, compute_next_run, due_automations, ensure_automation_schedule,
    mark_automation_ran, mark_automation_skipped, night_check_command, night_check_exit_code,
    night_check_may_fire, night_counts_run, night_unauth_should_skip,
    night_check_stdout, parse_nl_automation, replay_automation_target, skip_automation,
    chat_may_save_automation, user_asked_to_schedule,
    skip_night_check_receipt, Automation,
};
pub use connector::{
    connector_url_allowed, extract_connector_cmds, github_api_path, map_website_connector_name,
    parse_connector_args, ConnectorCmd, DEFAULT_CONNECTOR_HOSTS,
};
pub use consult::{format_consult_reply, parse_consult};
pub use context::{
    context_percent, estimate_messages, estimate_messages_from, estimate_tokens, is_result_turn, should_auto_compact,
    should_auto_compact_now, should_trim_result_bodies, trim_result_bodies, trim_result_bodies_in_place, CONTEXT_BUDGET_TOKENS,
    RECENT_MIN_MESSAGES, RESULT_TRIM_KEEP_HOPS, RESULT_TRIM_THRESHOLD,
};
pub use trajectory::{
    clip_excerpt, parse_trajectory_jsonl, rotate_trajectory, summarize_trajectory,
    trajectory_jsonl_line, yesterday_ms, TrajectoryEvent, TRAJECTORY_EXCERPT_CHARS,
    TRAJECTORY_MAX_BYTES,
};
pub use diagnostics::diagnostics_bundle;
pub use goal::{
    blend_thread_goal, compact_keep_pin, compact_keep_start_from, flush_visible_goal, goal_continue_pin, goal_pin_for_job, hub_dispatch_ok,
    goal_step_after_outcome, is_auto_continue_prompt, looks_incomplete, next_goal_prompt,
    visible_goal_step_on_continue,
    parse_fast_topics, parse_goal_outcome, reply_needs_followup, should_auto_continue_goal,
    should_name_thread, thread_goal_prompt, ThreadGoal, FOLLOWUP_MAX_STEPS, FOLLOWUP_PROMPT,
    GOAL_DROP_AFTER, GOAL_MAX_STEPS,
};
pub use greeting::{
    greeting_fingerprint, greeting_name, greeting_prompt, local_greeting, parse_llm_greeting,
    pick_greeting, should_paint_greeting, should_refresh_greeting, GreetingInput, GREETING_LLM_DEBOUNCE_MS, GREETING_LLM_MODE,
    GREETING_MAX_CHARS,
};
pub use history::{search_corpus, search_text, search_thread_body};
pub use host_cite::{host_status_line, last_host_line, summarize_write, unified_diff_cite};
pub use learning::{
    extract_insights, insight_key_for_fact, insight_pin, is_actionable_need, is_durable_fact, looks_like_user_pref,
    prune_ephemeral_insights, record_turn, upsert_insight, user_pref_facts, LearningInsight,
    LearningState,
};
pub use models::{catalog_line, sanitize_chat_model, MODEL_CATALOG};
pub use openclaw::{
    default_openclaw_paths, import_memory_file, is_openclaw_workspace, merge_imported_memory,
};
pub use shortcuts::{
    apply_composer_enter, composer_enter, composer_go, composer_go_tip, filter_palette,
    shortcut_help, ComposerEnter, ComposerGo, SHORTCUTS,
};
pub use stream::{
    chat_include_usage, chat_stream_flag, fold_sse_acc, fold_stream_fields, fold_stream_token, keep_sse_acc,
    parse_sse_delta, parse_sse_finish, parse_sse_text, parse_sse_thought, parse_sse_usage,
    prefer_complete_reply, sse_done, sse_live_delta, should_replace_stream_acc, stream_was_truncated,
    StreamTokenKind, StreamUsage,
};
pub use usage::{bump_usage, roll_usage_day, usage_blocked, usage_line, UsageDay};
pub use hub_sync::{build_hub_snapshot, is_hub_snapshot, merge_hub_snapshots, HubMemoryFile, HubSnapshot};
pub use hygiene::{lockish, should_send_screenshot};
pub use organs::{
    clipboard_context_block, daily_units_blocked, greet_from_last_job, last_user_scan, last_user_text,
    thread_host_receipts, thread_host_receipts_from,
    on_wheel_grab, parse_local_clock, passenger_label, plan_room, presence_orb_state,
    presence_should_stream, quiet_hours_active, redirect_prompt, replay_frame_delay,
    should_keep_frame, LocalClock, MidThoughtGreet, RoomPlan, PRESENCE_RING_MS, PRESENCE_WIPE_MS,
};
pub use rewind::{
    is_rewind_copy_cmd, is_rewind_copy_cmd_in, keep_last_rewinds, rewind_allowed, rewind_blocked_reason, rewind_can_queue,
    rewind_copy_cmd, rewind_dest, rewind_restore_matches, rewind_snapshot_ready, RewindRecord,
};
pub use oauth::{
    apply_profile, auth_bearer, chat_bearer, has_auth, merge_refreshed, next_oauth_poll_secs, parse_device_start, parse_poll_result,
    parse_token_json, parse_userinfo_profile, oauth_access_live, realtime_bearer, token_needs_refresh, trusted_profile_photo_url,
    trusted_xai_url, jwt_exp_ms, DeviceCodeStart, OAuthProfile, PollResult, PollStatus, XaiOAuthTokens,
    TOKEN_REFRESH_SKEW_MS, XAI_DEVICE_CODE_GRANT, XAI_OAUTH_CLIENT_ID, XAI_OAUTH_DISCOVERY,
    XAI_OAUTH_ISSUER, XAI_OAUTH_SCOPE, XAI_OAUTH_USERINFO,
};
pub use project::{
    add_to_folder, clean_project_name, create_folder, create_project, drop_node, drop_selected,
    folder_choices, expand_host_path_token, expand_project_root, host_cmd_leaves_project, host_hour_blocked,
    normalize_host_path, refund_host_reserved, is_under_project,
    project_menu_acts, project_menu_label, project_name_from_path, project_slug, project_work_path,
    rename_node, restore_bound_path, seed_from_bound, settle_project_path, should_seed_sidebar,
    resolve_acp_cwd, resolve_bind_path,
    stage_project, toggle_folder, upsert_bound, visible_tree, DropOutcome, ProjectKind,
    ProjectMenuAct, ProjectNode,
};
pub use redact::{forget_topic, is_plain_text, redact_secrets};
pub use skill::{
    bump_skill_run, is_hard_run, match_skill, parse_skill_md, patch_skill, prefer_patch,
    propose_skill_from_turn, render_skill_md, skill_dir_name, skill_follow_block,
    skill_use_in_chat_prompt, skill_safe,
    SkillMd,
};
pub use slash::{
    filter_slash_commands, is_cabin_slash_turn, mark_slash_result, parse_slash, resolve_mode_arg,
    slash_help, slash_kind, strip_slash_result, unknown_cabin_slash, Slash, SlashDef,
    SLASH_COMMANDS, SLASH_RESULT_PREFIX,
};
pub use verify::{
    can_mark_done, has_goal_complete, has_verify_ok, interpret_verify, verify_ok_after_user_turn,
    verify_script_path,
    VerifyResult,
};
pub use voice::{
    cabin_eyes_for_turn, client_secret_ws_protocol, client_secrets_body, client_secrets_url,
    dedicated_voice_model, encode_input_audio_append, encode_session_update, hey_grok_on_press,
    hey_grok_route, hey_grok_starts_ptt, is_voice_error, parse_client_secret, parse_realtime_event,
    parse_stt_text, parse_voice_event_text, pcm_from_capture, redact_cabin_from_memory,
    realtime_can_connect, reduce_voice_state, should_attach_cabin_frame, should_capture_before_chat,
    should_mute_speaker, speech_can_connect, stt_multipart, stt_url, transcribe_route,
    tts_request_body, tts_url, voice_can_connect, voice_client_secret_denied, voice_log_role,
    voice_session_url, voice_stream_token, voice_transcript_sends_chat, live_pcm_argv, live_pcm_frame_bytes,
    CabinEyesState, HeyGrokAction, HeyGrokRoute, TranscribeRoute, VoiceEvent, VoiceRole, VoiceState,
    DEFAULT_VOICE_MODEL, RECORDERS, TRANSCRIBERS,
};
pub use windshield::{
    build_windshield, filter_atspi_rows, is_interactive_role, keep_atspi_row, lock_check_titles,
    parse_atspi_line, parse_wmctrl_line, parse_xdotool_mouse, pick_named_row, rank_atspi_rows,
    refused_lock, tab_list_from_rows, window_name_from_atspi, window_name_from_wmctrl,
    windshield_browser_line, windshield_prompt, AtspiRow,
    PendingStep, WindshieldFrame,
};
pub use workboard::{
    apply_work_update, extract_work_pins, extract_work_updates, parse_work_pin, parse_work_update,
    BoardCard, BoardStatus,
};
pub use state::{
    clear_pending_after_complete, inbox_claim_ready, load_hub_state, merge_put_snapshot, save_hub_state, state_for_disk, CompleteError, HubState, MintRealtimeFn, PairError,
    DEFAULT_PORT, HUB_KIND,
};
pub use task::{HubTask, Receipt};
pub use thread_tab::{
    apply_auto_title, apply_auto_title_in, apply_manual_rename, auto_title_blocked, clean_tab_title,
    default_thread_title, delete_thread, display_tab_title, history_order, history_row_visible,
    leftover_empty_thread, reuse_empty_thread_idx, short_auto_title, toggle_pin, DeleteOutcome,
    ThreadReuseView, ThreadTab, AUTO_TITLE_MAX,
};
pub use update::{
    discover_source, is_grokhub_source, overlay_update_begin, overlay_update_can_restart,
    overlay_update_finish, overlay_update_progress, restart_acts, restart_argv, restart_bin,
    origin_needs_retarget, stale_github_origin, systemd_user_restart_args, update_cmds,
    update_plan_steps, update_progress_pct, update_step_label, update_wipes_config, walk_up_source,
    OverlayUpdateView, RestartAct, GITHUB_REMOTE_URL, ORIGIN_REMOTE_URL,
};

pub const PRESENCE_PUSH_MIN_MS: u64 = 400;

pub fn should_push_presence(now: u64, last_push_at: u64, min_ms: u64) -> bool {
    now.saturating_sub(last_push_at) >= min_ms
}

pub fn cap_history_images<T: Clone>(
    messages: &[T],
    images_of: impl Fn(&T) -> Option<Vec<String>>,
    with_images: impl Fn(&T, Option<Vec<String>>) -> T,
    max: usize,
) -> Vec<T> {
    let mut kept = 0usize;
    let mut out = messages.to_vec();
    for i in (0..out.len()).rev() {
        let Some(imgs) = images_of(&out[i]) else { continue };
        if imgs.is_empty() {
            continue;
        }
        if kept >= max {
            out[i] = with_images(&out[i], None);
            continue;
        }
        let room = max - kept;
        if imgs.len() > room {
            let slice = imgs[imgs.len() - room..].to_vec();
            out[i] = with_images(&out[i], Some(slice));
            kept = max;
        } else {
            kept += imgs.len();
        }
    }
    out
}

pub fn next_failover_tier(tier: &str) -> &'static str {
    match tier {
        "max" | "think" | "deep" | "heavy" | "expert" | "build" => "balanced",
        "balanced" | "balance" => "fast",
        _ => "fast",
    }
}

pub fn fill_random(buf: &mut [u8]) {
    if getrandom::getrandom(buf).is_err() {
        panic!("getrandom failed — refusing to mint a zero token");
    }
}

pub fn uid(prefix: &str) -> String {
    let mut buf = [0u8; 8];
    fill_random(&mut buf);
    format!("{prefix}-{}", hex::encode(buf))
}

pub fn new_token() -> String {
    let mut buf = [0u8; 24];
    fill_random(&mut buf);
    hex::encode(buf)
}

pub fn now_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pair_code_roundtrip() {
        let code = make_pair_code();
        assert!(
            regex_like_pair(&code),
            "pair format {code}"
        );
        assert_eq!(normalize_code("abc-234"), "ABC234");
        assert_eq!(normalize_code("ab c-23 4"), "ABC234");
        let tok = new_token();
        assert_eq!(tok.len(), 48);
        assert_ne!(tok, "0".repeat(48), "pair tokens must not be all-zero");
    }

    fn regex_like_pair(code: &str) -> bool {
        let b = code.as_bytes();
        b.len() == 7 && b[3] == b'-' && code.chars().filter(|c| *c != '-').all(|c| CODE_ALPH.contains(c))
    }

    #[test]
    fn presence_floor() {
        assert!(should_push_presence(1000, 0, PRESENCE_PUSH_MIN_MS));
        assert!(!should_push_presence(1000, 900, PRESENCE_PUSH_MIN_MS));
        assert!(should_push_presence(1000, 1000 - PRESENCE_PUSH_MIN_MS, PRESENCE_PUSH_MIN_MS));
    }

    #[test]
    fn failover() {
        assert_eq!(next_failover_tier("max"), "balanced");
        assert_eq!(next_failover_tier("think"), "balanced");
        assert_eq!(next_failover_tier("balanced"), "fast");
        assert_eq!(next_failover_tier("fast"), "fast");
        assert_eq!(next_failover_tier("auto"), "fast");
    }

    #[test]
    fn secrets() {
        assert!(is_plain_text("editor: nvim"));
        assert!(!is_plain_text("token sk-abcdefghijklmnopqrstuv"));
    }

    #[test]
    fn inhabit_gate() {
        assert!(can_inhabit(true, true, true));
        assert!(!can_inhabit(true, false, true));
        assert!(!can_inhabit(false, true, true));
        assert!(!can_inhabit(true, true, false));
    }

    #[test]
    fn disk_omits_frame() {
        let mut st = HubState::empty();
        st.last_frame = Some(std::sync::Arc::new(PresenceFrame {
            data_url: "data:image/jpeg;base64,AAAA".into(),
            at: 1,
        }));
        st.console_api_key = "xai-should-not-persist".into();
        let disk = state_for_disk(&st);
        let s = serde_json::to_string(&disk).unwrap();
        assert!(!s.contains("data:image"));
        assert!(!s.contains("xai-should-not-persist"));
        assert!(disk.console_api_key.is_empty());
        assert!(s.contains(&st.device_id));
    }
}
