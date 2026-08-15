//! Shared GrokHub brain. Linux, Windows, and Android must call this — not a second protocol.

pub mod automation;
pub mod chat;
pub mod chips;
pub mod connector;
pub mod consult;
pub mod context;
pub mod diagnostics;
pub mod doctor;
pub mod frame;
pub mod goal;
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
pub mod rewind;
pub mod shortcuts;
pub mod skill;
pub mod slash;
pub mod state;
pub mod stream;
pub mod task;
pub mod update;
pub mod usage;
pub mod verify;
pub mod voice;
pub mod windshield;
pub mod workboard;

pub use chat::{
    chat_request_body, chat_request_body_for_mode, chat_request_body_vision, chat_timeout_secs,
    effective_chat_mode, extract_host_cmds, failover_model, model_for_mode, needs_auth_banner,
    parse_chat_content, reasoning_effort_for_mode, resolve_chat_model, route_auto_mode,
    should_failover_status,
    DEFAULT_MODEL, XAI_BASE,
};
pub use chips::{
    build_quick_chips, chip_memory_key, chip_suggest_prompt, context_fingerprint, detect_chip_context,
    detect_chip_stage, empty_chip_memory, mode_from_chip_value, nav_from_chip_value, parse_llm_chips,
    predict_intents, remember_chip_click, remember_chip_dismiss, remember_chip_outcome,
    remember_typed_prompt, should_refresh_llm, top_habit_labels, ChipInput, ChipKind, ChipMemory,
    ChipStage, PredictedIntent, QuickChip, CHIP_LLM_DEBOUNCE_MS, CHIP_VISIBLE_MAX,
};
pub use doctor::{doctor_extras, doctor_lines, doctor_ok, DoctorLine};
pub use frame::{encode_b64, frame_bytes, jpeg_data_url, FrameGet, PresenceFrame};
pub use host_plan::{
    approved_cmds, explain_host_risk, host_risk, move_step, parse_host_plan, plan_from_text,
    step_from_cmd, HostPlanStep, HostRisk,
};
pub use host_safety::{forbidden_reason, recall_hits};
pub use imagine::{
    curate_wall, dedicated_imagine_model, extract_imagine_prompt, imagine_dest, imagine_request_body,
    imagine_slug, parse_imagine_url, pick_fresh_seed, wall_can_paint, wall_curate_seed, wall_due,
    wall_evict, ImagineWall, WallGif, WallSeed, WallSlot, DEFAULT_IMAGINE_MODEL, WALL_GIF_EVERY_MS,
    WALL_GIF_MAX,
    WALL_SEEDS,
};
pub use inhabit::{can_inhabit, InhabitBundle};
pub use recipe::{
    needs_reshoot, parse_computer_op, parse_recipe, parse_screen, replay_ops, screen_from_extents,
    ComputerOp, Recipe, ReplayOp, ScreenSize,
};
pub use reflect::{
    fact_candidates, restore_memory_prev, should_idle_reflect, surgical_memory_edit, MemoryEdit,
    IDLE_REFLECT_MS,
};
pub use pair::{make_pair_code, normalize_code, CODE_ALPH, PAIR_TTL_MS};
pub use automation::{
    automation_blocked_by_policy, compute_next_run, due_automations, ensure_automation_schedule,
    mark_automation_ran, parse_nl_automation, skip_automation, Automation,
};
pub use connector::{
    connector_url_allowed, extract_connector_cmds, github_api_path, map_website_connector_name,
    parse_connector_args, ConnectorCmd, DEFAULT_CONNECTOR_HOSTS,
};
pub use consult::{format_consult_reply, parse_consult};
pub use context::{
    context_percent, estimate_messages, estimate_tokens, should_auto_compact, CONTEXT_BUDGET_TOKENS,
    RECENT_MIN_MESSAGES,
};
pub use diagnostics::diagnostics_bundle;
pub use goal::{compact_keep_pin, looks_incomplete, next_goal_prompt, parse_goal_outcome, GOAL_MAX_STEPS};
pub use history::search_corpus;
pub use host_cite::{host_status_line, last_host_line, summarize_write, unified_diff_cite};
pub use learning::{insight_pin, record_turn, upsert_insight, LearningState};
pub use models::{catalog_line, sanitize_chat_model, MODEL_CATALOG};
pub use openclaw::{default_openclaw_paths, import_memory_file, is_openclaw_workspace};
pub use shortcuts::{filter_palette, shortcut_help, SHORTCUTS};
pub use stream::{chat_stream_flag, parse_sse_delta, sse_done};
pub use usage::{bump_usage, roll_usage_day, usage_blocked, usage_line, UsageDay};
pub use hub_sync::{build_hub_snapshot, is_hub_snapshot, merge_hub_snapshots, HubMemoryFile, HubSnapshot};
pub use hygiene::{lockish, should_send_screenshot};
pub use organs::{
    clipboard_context_block, daily_units_blocked, greet_from_last_job, last_user_text,
    on_wheel_grab, parse_local_clock, passenger_label, plan_room, presence_orb_state,
    presence_should_stream, quiet_hours_active, redirect_prompt, replay_frame_delay,
    should_keep_frame, LocalClock, MidThoughtGreet, RoomPlan, PRESENCE_RING_MS, PRESENCE_WIPE_MS,
};
pub use rewind::{keep_last_rewinds, rewind_allowed, rewind_dest, RewindRecord};
pub use oauth::{
    auth_bearer, has_auth, parse_device_start, parse_poll_result, parse_token_json,
    token_needs_refresh, trusted_xai_url, DeviceCodeStart, PollResult, PollStatus, XaiOAuthTokens,
    TOKEN_REFRESH_SKEW_MS, XAI_DEVICE_CODE_GRANT, XAI_OAUTH_CLIENT_ID, XAI_OAUTH_DISCOVERY,
    XAI_OAUTH_ISSUER, XAI_OAUTH_SCOPE,
};
pub use project::{
    add_to_folder, clean_project_name, create_folder, create_project, drop_node, folder_choices,
    host_cmd_leaves_project, host_hour_blocked, is_under_project, project_name_from_path,
    project_slug, project_work_path, rename_node, seed_from_bound, settle_project_path,
    stage_project, toggle_folder, upsert_bound, visible_tree, ProjectKind, ProjectNode,
};
pub use redact::{forget_topic, is_plain_text, redact_secrets};
pub use skill::{
    bump_skill_run, is_hard_run, match_skill, parse_skill_md, prefer_patch, propose_skill_from_turn,
    render_skill_md, skill_dir_name, skill_safe, SkillMd,
};
pub use slash::{
    filter_slash_commands, parse_slash, resolve_mode_arg, slash_help, slash_kind, Slash, SlashDef,
    SLASH_COMMANDS,
};
pub use verify::{
    can_mark_done, has_goal_complete, has_verify_ok, interpret_verify, verify_script_path,
    VerifyResult,
};
pub use voice::{
    cabin_eyes_for_turn, dedicated_voice_model, encode_input_audio_append, encode_session_update,
    hey_grok_on_press, is_voice_error, parse_realtime_event, parse_stt_text, parse_voice_event_text,
    redact_cabin_from_memory, reduce_voice_state, should_attach_cabin_frame,
    should_capture_before_chat, should_mute_speaker, stt_multipart, stt_url, transcribe_route,
    tts_request_body, tts_url, voice_can_connect, voice_session_url, CabinEyesState, HeyGrokAction,
    TranscribeRoute, VoiceEvent, VoiceState, DEFAULT_VOICE_MODEL, RECORDERS, TRANSCRIBERS,
};
pub use windshield::{
    build_windshield, parse_atspi_line, parse_wmctrl_line, parse_xdotool_mouse, refused_lock,
    AtspiRow, PendingStep, WindshieldFrame,
};
pub use workboard::{
    apply_work_update, extract_work_pins, extract_work_updates, parse_work_pin, parse_work_update,
    BoardCard, BoardStatus,
};
pub use state::{
    load_hub_state, save_hub_state, state_for_disk, HubState, PairError, DEFAULT_PORT, HUB_KIND,
};
pub use task::{HubTask, Receipt};
pub use update::{
    discover_source, is_grokhub_source, update_cmds, update_plan_steps, update_wipes_config,
    walk_up_source,
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

pub fn uid(prefix: &str) -> String {
    let mut buf = [0u8; 8];
    let _ = getrandom::getrandom(&mut buf);
    format!("{prefix}-{}", hex::encode(buf))
}

pub fn new_token() -> String {
    let mut buf = [0u8; 24];
    let _ = getrandom::getrandom(&mut buf);
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
    }

    #[test]
    fn disk_omits_frame() {
        let mut st = HubState::empty();
        st.last_frame = Some(PresenceFrame {
            data_url: "data:image/jpeg;base64,AAAA".into(),
            at: 1,
        });
        let disk = state_for_disk(&st);
        let s = serde_json::to_string(&disk).unwrap();
        assert!(!s.contains("data:image"));
        assert!(s.contains(&st.device_id));
    }
}
