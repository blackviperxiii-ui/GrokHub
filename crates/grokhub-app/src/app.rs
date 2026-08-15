use crate::config::{self, AppConfig};
use crate::desktop::{
    capture_data_url, capture_webcam, collect_rows, first_bin, play_audio, record_once,
    transcribe_local,
};
use crate::host::{run_host, run_host_stream};
use crate::secrets::{self, Secrets};
use crate::skills;
use crate::threads::{self, ChatThread};
use crate::update::{remember_source, resolve_source};
use crate::xai::{grok_chat, grok_chat_stream, grok_imagine, grok_stt, grok_tts, http_status_of};
use eframe::egui::{self, Color32, RichText};
use grokhub_core::{
    approved_cmds, apply_work_update, auth_bearer, automation_blocked_by_policy, build_hub_snapshot,
    build_quick_chips, build_windshield, bump_skill_run, cabin_eyes_for_turn, can_inhabit, can_mark_done,
    bump_usage, catalog_line, chip_suggest_prompt, compact_keep_pin, context_fingerprint,
    context_percent, daily_units_blocked,
    dedicated_imagine_model, dedicated_voice_model, default_openclaw_paths, diagnostics_bundle,
    pick_fresh_seed, wall_can_paint, wall_evict, ImagineWall, WallGif, WALL_GIF_EVERY_MS,
    WALL_GIF_MAX,
    due_automations, ensure_automation_schedule, estimate_messages, extract_connector_cmds,
    extract_imagine_prompt, extract_work_pins, filter_palette, format_consult_reply,
    extract_work_updates, fact_candidates, failover_model, filter_slash_commands, forbidden_reason,
    forget_topic, greet_from_last_job, has_auth, has_goal_complete, has_verify_ok, hey_grok_on_press,
    import_memory_file, insight_pin, is_openclaw_workspace,
    add_to_folder, create_folder, create_project, drop_node, folder_choices, host_cmd_leaves_project,
    host_hour_blocked, host_risk, host_status_line, is_hard_run, rename_node, seed_from_bound,
    settle_project_path, stage_project, toggle_folder, upsert_bound, visible_tree, ProjectKind,
    ProjectNode,
    is_plain_text, is_voice_error, keep_last_rewinds, last_user_text, load_hub_state, mark_automation_ran,
    match_skill, mode_from_chip_value, model_for_mode, move_step, nav_from_chip_value,
    resolve_chat_model,
    needs_auth_banner, next_goal_prompt,
    now_ms, on_wheel_grab, parse_consult, parse_goal_outcome, parse_local_clock, prefer_patch,
    parse_nl_automation, parse_recipe, parse_slash, passenger_label, plan_from_text, plan_room,
    presence_orb_state, presence_should_stream, propose_skill_from_turn, quiet_hours_active,
    parse_llm_chips, record_turn, reduce_voice_state, remember_chip_click, remember_chip_dismiss,
    remember_chip_outcome, remember_typed_prompt, roll_usage_day,
    recall_hits, redirect_prompt, redact_secrets, refused_lock, replay_ops, rewind_allowed,
    rewind_dest, save_hub_state, screen_from_extents, search_corpus, should_attach_cabin_frame,
    should_auto_compact, should_keep_frame, should_refresh_llm, shortcut_help,
    should_capture_before_chat, should_failover_status, should_idle_reflect, should_send_screenshot,
    skip_automation, slash_help, step_from_cmd, summarize_write, surgical_memory_edit,
    top_habit_labels,
    unified_diff_cite, usage_line,
    transcribe_route, uid, update_cmds, update_plan_steps, update_wipes_config, voice_session_url, Automation, BoardCard,
    BoardStatus, ChipInput, ChipKind, ChipMemory, ComputerOp, DeviceCodeStart, HeyGrokAction,
    HostPlanStep, HostRisk, HubMemoryFile, QuickChip,
    HubSnapshot, HubState, InhabitBundle, LearningState, LocalClock, Recipe, ReplayOp, RewindRecord,
    SkillMd, Slash, TranscribeRoute, UsageDay, VoiceEvent, VoiceState, CONTEXT_BUDGET_TOKENS,
    GOAL_MAX_STEPS, HUB_KIND, IDLE_REFLECT_MS,
    PRESENCE_RING_MS, TRANSCRIBERS,
};
use grokhub_hub::serve_lan;
use global_hotkey::{
    hotkey::{Code, HotKey, Modifiers},
    GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState,
};
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

fn wants_live_repaint(
    running: bool,
    chip_busy: bool,
    hub_on: bool,
    window_visible: bool,
    imagine: bool,
) -> bool {
    running || chip_busy || hub_on || !window_visible || imagine
}

const RAIL_FOOTER_H: f32 = 52.0;
const PALETTE_LIST_H: f32 = 280.0;

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
    go_settings: bool,
}

enum JobOut {
    Chat(String),
    ChatDelta(String),
    Imagine(String),
    Voice(String),
    Host(String),
    HostLine(String),
    HostDone(String),
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
            "VOICE_RECEIPT: Connect Grok in Settings, or install whisper".into()
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
    rx: Option<mpsc::Receiver<JobOut>>,
    hub: Arc<Mutex<HubState>>,
    hub_on: bool,
    hub_port: u16,
    task_prompt: String,
    mem_name: String,
    mem_body: String,
    plan_pending: Option<Vec<HostPlanStep>>,
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
    save_skill: bool,
    last_frame_url: Option<String>,
    speak_next: bool,
    verify_ok_turn: bool,
    verify_chip: String,
    reflect_diff: String,
    last_activity: Instant,
    reflected_idle: bool,
    last_recipe: Option<Recipe>,
    pending_update: bool,
    secrets: Secrets,
    threads: Vec<ChatThread>,
    thread_idx: usize,
    oauth_pending: Option<DeviceCodeStart>,
    host_hour_count: u32,
    host_hour_at: Instant,
    approve_risky_only: bool,
    tray: Option<crate::tray::TrayHost>,
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
    learning: LearningState,
    usage: UsageDay,
    palette_open: bool,
    palette_q: String,
    palette_pick: usize,
    palette_focus: bool,
    shortcuts_open: bool,
    pending_skill: Option<SkillMd>,
    goal_step: u32,
    stream_buf: String,
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
    chip_memory: ChipMemory,
    chip_dismissed: Vec<String>,
    llm_chips: Vec<QuickChip>,
    visible_chips: Vec<QuickChip>,
    chip_rx: Option<mpsc::Receiver<Vec<QuickChip>>>,
    chip_busy: bool,
    chip_fp: String,
    chip_llm_at: u64,
    skills_tab_connectors: bool,
    skill_q: String,
    github_args: String,
    pending_connectors: Vec<(String, String, String)>,
    auto_compose: bool,
    board_compose: bool,
    settings_menu_open: bool,
    imagine_want_focus: bool,
    settings_sec: SettingsSec,
    settings_back: Nav,
    imagine_aspect: u8,
    imagine_quality: bool,
    wall: ImagineWall,
    wall_rx: Option<mpsc::Receiver<Result<WallGif, String>>>,
    wall_busy: bool,
    projects: Vec<ProjectNode>,
    project_sel: Option<String>,
    proj_menu: Option<String>,
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
        let messages = threads
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
        let mut cfg = cfg;
        if cfg.source_dir.trim().is_empty() {
            if let Some(src) = resolve_source("") {
                remember_source(&src);
                cfg.source_dir = src.display().to_string();
            }
        }
        if cfg.project_dir.trim().is_empty() {
            if let Ok(home) = std::env::var("HOME") {
                cfg.project_dir = format!("{home}/GrokHub-Work");
            }
        }
        let mut projects = crate::store::load_projects();
        if projects.is_empty() {
            projects = seed_from_bound(&cfg.project_dir);
        }
        let project_sel = projects
            .iter()
            .find(|n| n.kind == ProjectKind::Project && n.path == cfg.project_dir)
            .or_else(|| projects.iter().find(|n| n.kind == ProjectKind::Project))
            .map(|n| n.id.clone());
        let secrets = secrets::load();
        let approve_risky_only = cfg.approve_risky_only;
        let mut c = Self {
            nav: Nav::Chat,
            cfg,
            composer: String::new(),
            messages,
            status: String::new(),
            running: false,
            rx: None,
            hub: Arc::new(Mutex::new(hub)),
            hub_on: false,
            hub_port: grokhub_core::DEFAULT_PORT,
            task_prompt: String::new(),
            mem_name,
            mem_body,
            plan_pending: None,
            last_persist: Instant::now(),
            board: config::load_board(),
            board_title: String::new(),
            imagine_prompt: String::new(),
            imagine_last: String::new(),
            skill_name: String::new(),
            skill_body: String::new(),
            skill_list: skills::list_skills(),
            eyes_text: String::new(),
            last_host: vec![],
            save_skill: false,
            last_frame_url: None,
            speak_next: false,
            verify_ok_turn: false,
            verify_chip: String::new(),
            reflect_diff: String::new(),
            last_activity: Instant::now(),
            reflected_idle: false,
            last_recipe: None,
            pending_update: false,
            secrets,
            threads,
            thread_idx,
            oauth_pending: None,
            host_hour_count: 0,
            host_hour_at: Instant::now(),
            approve_risky_only,
            tray: crate::tray::spawn(),
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
            learning: crate::store::load_learning(),
            usage: crate::store::load_usage(),
            palette_open: false,
            palette_q: String::new(),
            palette_pick: 0,
            palette_focus: false,
            shortcuts_open: false,
            pending_skill: None,
            goal_step: 0,
            stream_buf: String::new(),
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
            chip_memory: crate::store::load_chips(),
            chip_dismissed: vec![],
            llm_chips: vec![],
            visible_chips: vec![],
            chip_rx: None,
            chip_busy: false,
            chip_fp: String::new(),
            chip_llm_at: 0,
            skills_tab_connectors: false,
            skill_q: String::new(),
            github_args: String::new(),
            pending_connectors: vec![],
            auto_compose: false,
            board_compose: false,
            settings_menu_open: false,
            imagine_want_focus: false,
            settings_sec: SettingsSec::Account,
            settings_back: Nav::Chat,
            imagine_aspect: 1,
            imagine_quality: true,
            wall: crate::store::load_wall(),
            wall_rx: None,
            wall_busy: false,
            projects,
            project_sel,
            proj_menu: None,
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
        let _ = crate::store::save_usage(&self.usage);
        let _ = crate::store::save_chips(&self.chip_memory);
        let _ = crate::store::save_wall(&self.wall);
        self.flush_projects();
        let _ = config::save(&self.cfg);
        if let Ok(st) = self.hub.lock() {
            let _ = save_hub_state(&config::hub_state_path(), &st);
        }
        self.last_persist = Instant::now();
    }

    fn scratch(&self) -> bool {
        self.threads
            .get(self.thread_idx)
            .map(|t| t.scratch)
            .unwrap_or(false)
    }

    fn has_key(&self) -> bool {
        has_auth(&self.cfg.api_key, &secrets::access_token(&self.secrets))
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
        let _ = crate::store::save_projects(&self.projects);
        self.projects_dirty = false;
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

    fn refresh_chips(&mut self) {
        let chat = self.chat_pairs();
        let hour = Self::chip_hour();
        let title = self
            .threads
            .get(self.thread_idx)
            .map(|t| t.title.clone())
            .unwrap_or_default();
        let last_failed = self.last_receipt_ok == Some(false);
        let input = ChipInput {
            chat: &chat,
            draft: &self.composer,
            grok_connected: self.has_key(),
            host_on: self.cfg.host_on,
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
            max: 5,
        };
        self.visible_chips = build_quick_chips(input);
        let fp = context_fingerprint(&chat, &self.composer, last_failed, hour);
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
        let prompt = chip_suggest_prompt(
            &chat,
            &title,
            &self.composer,
            &habits,
            &self.chip_dismissed,
        );
        let model = model_for_mode("fast").to_string();
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
        let tag = context_fingerprint(
            &self.chat_pairs(),
            &self.composer,
            self.last_receipt_ok == Some(false),
            hour,
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

    fn mode_label(mode: &str) -> &'static str {
        match mode {
            "max" | "deep" | "heavy" => "Max",
            "think" | "build" | "expert" => "Think",
            "balanced" | "balance" => "Balance",
            "fast" => "Fast",
            _ => "Auto",
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
        self.persist();
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
        self.status = if scratch {
            "Scratch — no memory writes".into()
        } else {
            "New chat".into()
        };
        self.persist();
    }

    fn send_chat(&mut self, text: String) {
        let mut text = text.trim().to_string();
        if text.is_empty() {
            return;
        }
        if self.running {
            let prev = last_user_text(
                &self
                    .messages
                    .iter()
                    .map(|m| (m.role.clone(), m.content.clone()))
                    .collect::<Vec<_>>(),
            )
            .unwrap_or_default();
            self.rx = None;
            self.running = false;
            self.voice_orb = "idle".into();
            text = redirect_prompt(&prev, &text);
            self.status = "Redirected".into();
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
        if let Some(sk) = match_skill(&text, &self.skill_list) {
            self.skill_name = sk.name.clone();
            self.status = format!("Skill {}", sk.name);
        }
        self.messages.push(Msg {
            role: "user".into(),
            content: text.clone(),
        });
        self.persist();
        self.kick_model();
    }

    fn run_slash(&mut self, slash: Slash) {
        match slash {
            Slash::Approve { yolo } => {
                self.cfg.yolo = yolo;
                self.approve_risky_only = false;
                self.cfg.approve_risky_only = false;
                let _ = config::save(&self.cfg);
                self.status = if yolo {
                    "YOLO — /approve off".into()
                } else {
                    "Supervised — /approve on".into()
                };
            }
            Slash::ApproveRisky => {
                self.cfg.yolo = false;
                self.approve_risky_only = true;
                self.cfg.approve_risky_only = true;
                let _ = config::save(&self.cfg);
                self.status = "Confirm destructive only".into();
            }
            Slash::ApproveAll => {
                self.cfg.yolo = false;
                self.approve_risky_only = false;
                self.cfg.approve_risky_only = false;
                let _ = config::save(&self.cfg);
                self.status = "Confirm every host command".into();
            }
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
                if let Some(m) = self.messages.iter().rev().find(|m| m.role == "user") {
                    let t = m.content.clone();
                    self.kick_model_retry(t);
                } else {
                    self.status = "Nothing to retry".into();
                }
            }
            Slash::Stop => {
                self.rx = None;
                self.running = false;
                self.status = "Stopped".into();
            }
            Slash::Sh(cmd) => self.queue_sh(cmd),
            Slash::Host { on } => {
                self.cfg.host_on = on;
                let _ = config::save(&self.cfg);
                self.status = if on { "Host on".into() } else { "Host off".into() };
            }
            Slash::HostStatus => {
                self.status = format!(
                    "Host {} · hour {}/{} · {}",
                    if self.cfg.host_on { "on" } else { "off" },
                    self.host_hour_count,
                    self.cfg.host_hour_cap,
                    if self.host_live.is_empty() {
                        "idle"
                    } else {
                        &self.host_live
                    }
                );
            }
            Slash::Tools { on } => {
                self.cfg.host_on = on;
                let _ = config::save(&self.cfg);
                self.status = if on { "Host on".into() } else { "Host off".into() };
            }
            Slash::Rename(title) => {
                if let Some(t) = self.threads.get_mut(self.thread_idx) {
                    t.title = title.chars().take(80).collect();
                    self.status = format!("Renamed {}", t.title);
                    self.persist();
                }
            }
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
                self.rx = None;
                self.running = false;
                self.voice_orb = "idle".into();
                self.nav = Nav::Settings;
                self.settings_sec = health_settings_sec();
                self.status = self.doctor_text();
            }
            Slash::Remember(note) => self.run_slash(Slash::MemoryNote(note)),
            Slash::Mode(mode) => {
                self.cfg.mode = mode.clone();
                self.cfg.model = model_for_mode(&mode).to_string();
                let _ = config::save(&self.cfg);
                let model = self.cfg.model.as_str();
                self.status = match grokhub_core::reasoning_effort_for_mode(&mode) {
                    Some(effort) => format!("Mode {mode} → {model} · {effort}"),
                    None => format!("Mode {mode} → {model}"),
                };
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
            Slash::Palette => {
                self.palette_open = true;
            }
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

    fn queue_sh(&mut self, cmd: String) {
        if !self.cfg.host_on {
            self.status = "Host off — /host on".into();
            return;
        }
        if self.host_hour_at.elapsed() > Duration::from_secs(3600) {
            self.host_hour_count = 0;
            self.host_hour_at = Instant::now();
        }
        if host_hour_blocked(self.host_hour_count, self.cfg.host_hour_cap) {
            self.status = format!("Host hour cap {}", self.cfg.host_hour_cap);
            return;
        }
        if let Some(why) = forbidden_reason(&cmd) {
            self.status = why.into();
            return;
        }
        let step = step_from_cmd(cmd);
        let outside = host_cmd_leaves_project(&step.cmd, &self.cfg.project_dir);
        if self.cfg.yolo && !(self.approve_risky_only && step.risk == HostRisk::Destructive) && !outside
        {
            self.run_cmds(vec![step.cmd]);
            return;
        }
        if self.approve_risky_only && step.risk != HostRisk::Destructive && !outside {
            self.run_cmds(vec![step.cmd]);
            return;
        }
        self.plan_pending = Some(vec![step]);
        self.status = if outside {
            "Outside bound project — approve".into()
        } else {
            "Host plan needs approval".into()
        };
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

    fn tick_night(&mut self) {
        if self.running || self.last_night_tick.elapsed() < Duration::from_secs(5) {
            return;
        }
        self.last_night_tick = Instant::now();
        let clock = Self::local_clock();
        let day = format!("{}-{}", clock.weekday, clock.hour);
        if self.daily_auto_day != day && clock.hour == 0 && clock.minute < 6 {
            self.daily_auto_used = 0;
            self.daily_auto_day = day;
        }
        if daily_units_blocked(self.daily_auto_used, self.cfg.daily_auto_cap) {
            return;
        }
        let clock_copy = clock;
        self.automations = std::mem::take(&mut self.automations)
            .into_iter()
            .map(|a| ensure_automation_schedule(a, clock_copy))
            .collect();
        let due = due_automations(&self.automations, clock.now_ms);
        let Some(a) = due.into_iter().next() else {
            return;
        };
        if !a.check_command.trim().is_empty() {
            let out = run_host(&a.check_command, Duration::from_secs(20));
            let code = if out.contains("exit 0") { 0 } else { 1 };
            if skip_automation(&out, code) {
                self.mark_auto_ran(&a.id, clock.now_ms);
                return;
            }
        }
        let quiet = quiet_hours_active(&clock.hm(), &self.cfg.quiet_start, &self.cfg.quiet_end);
        let destructive = a.instructions.to_ascii_lowercase().contains("rm ")
            || host_risk(&a.instructions) == HostRisk::Destructive;
        if automation_blocked_by_policy(quiet, destructive, self.cfg.autonomy) {
            self.status = format!("Night skipped {} (quiet/policy)", a.name);
            return;
        }
        self.mark_auto_ran(&a.id, clock.now_ms);
        self.daily_auto_used = self.daily_auto_used.saturating_add(1);
        self.status = format!("Night: {}", a.name);
        self.nav = Nav::Chat;
        self.send_chat(a.instructions);
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

    fn run_palette(&mut self, action: &str) {
        self.palette_open = false;
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

    fn mid_thought_greet(&mut self) {
        if !self.messages.is_empty() {
            return;
        }
        if self.last_receipts.is_empty() && self.rewind_rows.is_empty() {
            return;
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
        self.messages.push(Msg {
            role: "assistant".into(),
            content: format!(
                "You sit down. Last night: {}\n{}",
                g.goal.unwrap_or_else(|| "quiet".into()),
                g.last_fail
                    .map(|f| format!("Failed: {f}"))
                    .unwrap_or_else(|| "Finished clean.".into())
            ),
        });
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

    fn kick_model(&mut self) {
        if !self.has_key() {
            self.status = "Connect Grok OAuth in Settings".into();
            return;
        }
        self.running = true;
        self.status = "Thinking…".into();
        let key = self.bearer();
        let model = resolve_chat_model(&self.cfg.mode, &self.cfg.model);
        let effort = if model == "grok-4.6" {
            grokhub_core::reasoning_effort_for_mode(&self.cfg.mode)
        } else {
            None
        };
        let mut msgs: Vec<(String, String)> = self
            .messages
            .iter()
            .map(|m| (m.role.clone(), m.content.clone()))
            .collect();
        let soul = config::read_memory("SOUL.md");
        let pins = skills::pin_text(&self.skill_list);
        let mut sys = String::new();
        if !soul.trim().is_empty() {
            sys.push_str("SOUL.md\n");
            sys.push_str(&soul);
        }
        if !pins.is_empty() {
            if !sys.is_empty() {
                sys.push_str("\n\n");
            }
            sys.push_str("Skills (names only):\n");
            sys.push_str(&pins);
        }
        if !self.cfg.goal_pin.trim().is_empty() {
            if !sys.is_empty() {
                sys.push_str("\n\n");
            }
            sys.push_str("GOAL PIN: ");
            sys.push_str(&self.cfg.goal_pin);
        }
        if !self.board.is_empty() {
            if !sys.is_empty() {
                sys.push_str("\n\n");
            }
            sys.push_str("Workboard:\n");
            for c in self.board.iter().take(8) {
                sys.push_str(&format!("- [{}] {}\n", c.status.as_str(), c.title));
            }
        }
        if let Some(h) = self.last_host.last() {
            if !sys.is_empty() {
                sys.push_str("\n\n");
            }
            sys.push_str("Last HOST_RESULT (tail):\n");
            sys.push_str(&h.chars().take(400).collect::<String>());
        }
        let pin = insight_pin(&self.learning);
        if !pin.is_empty() {
            if !sys.is_empty() {
                sys.push_str("\n\n");
            }
            sys.push_str("Learned:\n");
            sys.push_str(&pin);
        }
        if !sys.is_empty() {
            msgs.insert(0, ("system".into(), sys));
        }
        self.ensure_cabin_frame();
        let fused = self.last_frame_url.clone().or_else(|| self.webcam_url.clone());
        let image = if should_attach_cabin_frame(
            cabin_eyes_for_turn(self.cfg.cabin_eyes, fused.is_some()),
            self.cfg.cabin_eyes,
        ) {
            fused
        } else {
            None
        };
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        self.stream_buf.clear();
        std::thread::spawn(move || {
            let tx_d = tx.clone();
            let r = grok_chat_stream(&key, &model, &msgs, image.as_deref(), effort, |d| {
                let _ = tx_d.send(JobOut::ChatDelta(d.to_string()));
            });
            let r = match r {
                Err(e) => {
                    if http_status_of(&e).map(should_failover_status).unwrap_or(false) {
                        if let Some(next) = failover_model(&model) {
                            grok_chat(&key, next, &msgs, image.as_deref(), None)
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
                Ok(t) => JobOut::Chat(t),
                Err(e) => JobOut::Err(e),
            });
        });
    }

    fn poll_job(&mut self) {
        let Some(rx) = self.rx.take() else { return };
        match rx.try_recv() {
            Ok(JobOut::ChatDelta(d)) => {
                self.rx = Some(rx);
                self.stream_buf.push_str(&d);
                self.status = format!("streaming… {}", self.stream_buf.chars().count());
                if self.messages.last().map(|m| m.role == "assistant" && m.content == self.stream_buf[..self.stream_buf.len().saturating_sub(d.len())]).unwrap_or(false) {
                    if let Some(last) = self.messages.last_mut() {
                        last.content = self.stream_buf.clone();
                    }
                } else if !self.stream_buf.is_empty()
                    && self.messages.last().map(|m| m.role != "assistant").unwrap_or(true)
                {
                    self.messages.push(Msg {
                        role: "assistant".into(),
                        content: self.stream_buf.clone(),
                    });
                } else if let Some(last) = self.messages.last_mut() {
                    if last.role == "assistant" {
                        last.content = self.stream_buf.clone();
                    }
                }
            }
            Ok(JobOut::Chat(text)) => {
                self.running = false;
                self.voice_orb = "idle".into();
                self.status.clear();
                remember_chip_outcome(&mut self.chip_memory, true, now_ms());
                record_turn(&mut self.learning);
                bump_usage(&mut self.usage, "message");
                let replace_last = self.messages.last().map(|m| m.role == "assistant").unwrap_or(false);
                if replace_last {
                    if let Some(last) = self.messages.last_mut() {
                        last.content = text.clone();
                    }
                } else {
                    self.messages.push(Msg {
                        role: "assistant".into(),
                        content: text.clone(),
                    });
                }
                if self.speak_next {
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
                    if st == BoardStatus::Done
                        && !can_mark_done(self.verify_ok_turn, has_verify_ok(&text))
                    {
                        self.status = "Need VERIFY_OK or a passing verify.sh".into();
                        continue;
                    }
                    let _ = apply_work_update(&mut self.board, &key, st);
                }
                if has_goal_complete(&text)
                    && !can_mark_done(self.verify_ok_turn, has_verify_ok(&text))
                {
                    self.status = "GOAL_COMPLETE held — need verify".into();
                }
                if let Some(plan) = plan_from_text(&text) {
                    self.pending_update = false;
                    if self.cfg.yolo {
                        self.run_cmds(approved_cmds(&plan));
                    } else {
                        self.plan_pending = Some(plan);
                        self.status = "Host plan needs approval".into();
                    }
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
                if let Some(mut a) = parse_nl_automation(&text) {
                    if a.id.is_empty() {
                        a.id = uid("auto");
                    }
                    self.automations.push(a.clone());
                    let _ = crate::night::save(&self.automations);
                    self.status = format!("Night saved: {} {}", a.schedule, a.time);
                }
                if let Some(q) = parse_consult(&text) {
                    self.run_consult(q);
                }
                let outcome = parse_goal_outcome(&text);
                if outcome == "continue" && !self.cfg.goal_pin.is_empty() {
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
                self.status = line;
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
                self.messages.push(Msg {
                    role: "user".into(),
                    content: format!("HOST_RESULT (facts only):\n{block}"),
                });
                self.save_skill = true;
                self.plan_pending = None;
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
                if is_hard_run(self.last_host.len() as u32, !ok, false, self.scratch()) {
                    let user = last_user_text(
                        &self
                            .messages
                            .iter()
                            .map(|m| (m.role.clone(), m.content.clone()))
                            .collect::<Vec<_>>(),
                    )
                    .unwrap_or_default();
                    let proposed = propose_skill_from_turn(&user, &block, &self.last_host);
                    if prefer_patch(&self.skill_list, &proposed).is_some() {
                        self.status = format!("Patch skill {}?", proposed.name);
                    }
                    self.pending_skill = Some(proposed.clone());
                    self.skill_name = proposed.name.clone();
                    self.skill_body = grokhub_core::render_skill_md(&proposed);
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
            Ok(JobOut::Host(block)) => {
                self.running = false;
                self.last_receipt_ok = Some(!crate::update::host_receipt_failed(&block));
                self.messages.push(Msg {
                    role: "user".into(),
                    content: format!("HOST_RESULT (facts only):\n{block}"),
                });
                self.status = if crate::update::host_receipt_failed(&block) {
                    "Update failed".into()
                } else {
                    "Update finished — restart the cabin".into()
                };
                self.persist();
            }
            Ok(JobOut::Err(e)) => {
                self.running = false;
                self.voice_orb = "idle".into();
                remember_chip_outcome(&mut self.chip_memory, false, now_ms());
                self.status = e;
            }
            Err(mpsc::TryRecvError::Empty) => {
                self.rx = Some(rx);
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.running = false;
            }
        }
    }

    fn run_cmds(&mut self, cmds: Vec<String>) {
        if !self.cfg.host_on {
            self.status = "Host off — /host on".into();
            return;
        }
        if self.running {
            self.status = "Busy — wait, then host".into();
            return;
        }
        if self.host_hour_at.elapsed() > Duration::from_secs(3600) {
            self.host_hour_count = 0;
            self.host_hour_at = Instant::now();
        }
        let mut gated = Vec::new();
        for c in &cmds {
            if host_hour_blocked(self.host_hour_count, self.cfg.host_hour_cap) {
                self.status = format!("Host hour cap {}", self.cfg.host_hour_cap);
                break;
            }
            if let Some(why) = forbidden_reason(c) {
                self.messages.push(Msg {
                    role: "user".into(),
                    content: format!("HOST_RESULT (facts only):\n$ {c}\nblocked: {why}"),
                });
                continue;
            }
            if host_cmd_leaves_project(c, &self.cfg.project_dir) && !self.cfg.yolo {
                self.messages.push(Msg {
                    role: "user".into(),
                    content: format!("HOST_RESULT (facts only):\n$ {c}\nblocked: outside bound project"),
                });
                continue;
            }
            self.host_hour_count = self.host_hour_count.saturating_add(1);
            gated.push(c.clone());
        }
        if gated.is_empty() {
            self.plan_pending = None;
            return;
        }
        self.snapshot_project();
        self.last_host = gated.clone();
        self.running = true;
        self.voice_orb = "hands".into();
        self.status = host_status_line(&gated[0], "", 0);
        self.plan_pending = None;
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        let cap = self.cfg.host_hour_cap;
        std::thread::spawn(move || {
            let started = Instant::now();
            let mut inhibit = crate::notify::inhibit_sleep();
            let mut block = String::new();
            let mut count = 0u32;
            for c in &gated {
                if host_hour_blocked(count, cap) {
                    block.push_str("hour cap reached\n");
                    break;
                }
                count = count.saturating_add(1);
                let tx_line = tx.clone();
                let cmd = c.clone();
                let receipt = run_host_stream(c, Duration::from_secs(90), move |line| {
                    let _ = tx_line.send(JobOut::HostLine(host_status_line(&cmd, line, 0)));
                });
                if let Some(cite) = summarize_write(c, &receipt) {
                    block.push_str(&cite);
                    block.push('\n');
                }
                block.push_str(&redact_secrets(&receipt));
                block.push_str("\n\n");
            }
            crate::notify::release_inhibit(&mut inhibit);
            crate::notify::ping_if_long(started.elapsed(), "GrokHub", "Host job finished");
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
        let aspect = crate::cards::imagine_aspect_label(self.imagine_aspect);
        let prompt = if prompt.contains(aspect) {
            prompt
        } else {
            format!("{prompt}, {aspect} still")
        };
        self.running = true;
        self.status = "Imagining…".into();
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
            let (halt, _) = on_wheel_grab(self.running);
            if halt {
                self.rx = None;
                self.running = false;
            }
            if let Some(mut s) = self.voice_sock.take() {
                s.halt();
            }
            self.voice_state = VoiceState::Idle;
            self.voice_orb = "idle".into();
            self.status = "Hands on — halted".into();
            return;
        }
        if self.voice_sock.is_none() {
            let key = self.bearer();
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
        if self.running {
            return;
        }
        self.voice_orb = "listening".into();
        self.voice_state = VoiceState::Listening;
        self.running = true;
        self.status = format!(
            "Listening… {}",
            voice_session_url(&dedicated_voice_model(&self.cfg.voice_model))
        );
        let key = self.bearer();
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        std::thread::spawn(move || {
            let _ = tx.send(JobOut::Voice(listen_turn(&key)));
        });
    }

    fn ensure_cabin_frame(&mut self) {
        if !should_capture_before_chat(self.cfg.cabin_eyes) {
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
        let Some(src) = resolve_source(&self.cfg.source_dir) else {
            self.status = "Set Settings → source (clone path) or GROKHUB_SRC".into();
            self.nav = Nav::Settings;
            return;
        };
        self.cfg.source_dir = src.display().to_string();
        remember_source(&src);
        let _ = config::save(&self.cfg);
        match update_cmds(&src) {
            Ok(cmds) if !update_wipes_config(&cmds) => {
                let plan: Vec<HostPlanStep> = update_plan_steps(cmds);
                if self.cfg.yolo {
                    self.start_overlay_update(approved_cmds(&plan));
                } else {
                    self.pending_update = true;
                    self.plan_pending = Some(plan);
                    self.status = "Update plan needs approval".into();
                    self.nav = Nav::Chat;
                }
            }
            Ok(_) => self.status = "refusing an update that would wipe config".into(),
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
        self.running = true;
        self.status = "Updating install…".into();
        self.nav = Nav::Chat;
        self.last_host = cmds.clone();
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        std::thread::spawn(move || {
            let r = crate::update::run_update_cmds(&cmds);
            let _ = tx.send(match r {
                Ok(block) => JobOut::Host(block),
                Err(e) if crate::update::host_receipt_failed(&e) => JobOut::Host(e),
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
        let current = config::read_memory("MEMORY.md");
        let msgs: Vec<(String, String)> = self
            .messages
            .iter()
            .map(|m| (m.role.clone(), m.content.clone()))
            .collect();
        let edit = surgical_memory_edit(&current, &fact_candidates(&msgs));
        if edit.diff.is_empty() {
            self.status = "Reflect: nothing new".into();
            return;
        }
        match config::write_memory("MEMORY.md", &edit.next) {
            Ok(()) => {
                self.reflect_diff = edit.diff;
                if self.mem_name == "MEMORY.md" {
                    self.mem_body = edit.next;
                }
                self.status = "Reflected MEMORY.md".into();
            }
            Err(e) => self.status = e,
        }
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

    fn replay_recipe(&mut self) {
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
                ReplayOp::Op(ComputerOp::Act { name }) => {
                    t.push_str(&format!("act {name}\n"));
                }
                ReplayOp::Op(ComputerOp::WaitFor { title }) => {
                    t.push_str(&format!("wait_for {}\n", title.unwrap_or_default()));
                }
                ReplayOp::Op(ComputerOp::Click { x, y }) => {
                    t.push_str(&format!("click {x} {y}\n"));
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
            self.cfg.autonomy,
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

    fn stage_last_skill(&mut self) {
        if self.last_host.is_empty() {
            return;
        }
        let s = SkillMd {
            name: "last-host".into(),
            description: "Last approved host run".into(),
            slash: "/last-host".into(),
            trigger: "repeat last host".into(),
            instructions: self.last_host.join("\n"),
            pitfalls: "review before YOLO".into(),
            verify: "receipt exit 0".into(),
            runs: 0,
        };
        match skills::save_skill(&s) {
            Ok(p) => {
                self.skill_list = skills::list_skills();
                self.save_skill = false;
                self.status = format!("Wrote {}", p.display());
            }
            Err(e) => self.status = e,
        }
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
        self.window_visible = false;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        if !self.told_tray {
            self.told_tray = true;
            crate::notify::ping("GrokHub", "Still running in the tray");
            self.status = "In the tray — Show cabin to sit down".into();
        }
    }

    fn show_from_tray(&mut self, ctx: &egui::Context) {
        self.window_visible = true;
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
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
                VoiceEvent::Transcript { text, final_ } if final_ && !text.trim().is_empty() => {
                    self.send_chat(text);
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
        let Some(tray) = &self.tray else {
            return;
        };
        match tray.try_recv() {
            Some(crate::tray::TrayCmd::Show) => self.show_from_tray(ctx),
            Some(crate::tray::TrayCmd::Halt) => {
                self.rx = None;
                self.running = false;
                self.status = "Stopped".into();
            }
            Some(crate::tray::TrayCmd::Quit) => {
                self.want_quit = true;
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
        if let Some(id) = self.proj_menu.clone() {
            let kind = self
                .projects
                .iter()
                .find(|n| n.id == id)
                .map(|n| n.kind);
            let mut act: Option<&'static str> = None;
            let mut menu_rect = egui::Rect::NOTHING;
            egui::Area::new(egui::Id::new("proj-menu"))
                .fixed_pos(self.proj_menu_pos + egui::vec2(0.0, 4.0))
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_min_width(168.0);
                        if ui.selectable_label(false, "Rename").clicked() {
                            act = Some("rename");
                        }
                        if kind == Some(ProjectKind::Project) {
                            if ui.selectable_label(false, "Add to folder").clicked() {
                                act = Some("add");
                            }
                            if ui.selectable_label(false, "Remove from folder").clicked() {
                                act = Some("root");
                            }
                        }
                        if kind == Some(ProjectKind::Folder)
                            && ui.selectable_label(false, "New project here").clicked()
                        {
                            act = Some("new-here");
                        }
                        menu_rect = ui.min_rect();
                    });
                });
            if let Some(a) = act {
                self.proj_menu = None;
                match a {
                    "rename" => {
                        if let Some(n) = self.projects.iter().find(|n| n.id == id) {
                            self.begin_proj_rename(id, n.name.clone());
                        }
                    }
                    "add" => {
                        self.proj_add_for = Some(id.clone());
                        self.project_sel = Some(id);
                        self.proj_ignore_close = true;
                    }
                    "root" => {
                        if add_to_folder(&mut self.projects, &id, None).is_ok() {
                            self.status = "Moved to Projects".into();
                            self.touch_projects();
                            self.persist();
                        }
                    }
                    "new-here" => self.stage_new_project(Some(&id)),
                    _ => {}
                }
            } else if self.proj_ignore_close {
                self.proj_ignore_close = false;
            } else if ctx.input(|i| i.pointer.any_click()) {
                if let Some(pos) = ctx.pointer_interact_pos() {
                    if !menu_rect.expand(8.0).contains(pos) {
                        self.proj_menu = None;
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
                        ui.label(RichText::new("Add to folder").size(12.0).color(crate::theme::MUTED));
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
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_job();
        self.poll_chips();
        self.refresh_chips();
        self.drain_inbox();
        self.poll_tray(ctx);
        self.poll_voice();
        self.poll_global_hotkeys();
        self.tick_night();
        self.poll_wall();
        self.tick_wall();
        self.live_room();
        self.roll_today();
        if self.messages.is_empty() {
            self.mid_thought_greet();
        }
        if ctx.input(|i| i.viewport().close_requested()) {
            let hide = crate::tray::should_hide_on_close(
                self.cfg.close_to_tray,
                self.tray.is_some(),
            ) && !self.want_quit;
            if hide {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.hide_to_tray(ctx);
            }
        }
        if self.oauth_pending.is_some() {
            self.poll_oauth();
            ctx.request_repaint_after(Duration::from_secs(2));
        }
        if ctx.input(|i| i.modifiers.command && i.modifiers.shift && i.key_pressed(egui::Key::Escape))
        {
            let (halt, _) = on_wheel_grab(self.running);
            if halt || self.running {
                self.rx = None;
                self.running = false;
            }
            self.voice_orb = "idle".into();
            self.status = "Stopped".into();
        }
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::G) && !i.modifiers.shift) {
            self.listen_voice();
        }
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::N) && !i.modifiers.shift) {
            self.new_thread(false);
        }
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::K) && !i.modifiers.shift) {
            self.palette_open = !self.palette_open;
            if self.palette_open {
                self.palette_focus = true;
                self.palette_pick = 0;
            }
        }
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Slash)) {
            self.shortcuts_open = !self.shortcuts_open;
        }
        if should_idle_reflect(
            self.last_activity.elapsed().as_millis() as u64,
            self.running,
            IDLE_REFLECT_MS,
        ) && !self.reflected_idle
        {
            self.reflected_idle = true;
            self.run_reflect();
        }
        if self.last_persist.elapsed() > Duration::from_secs(2) {
            self.persist();
        }
        if wants_live_repaint(
            self.running,
            self.chip_busy,
            self.hub_on,
            self.window_visible,
            self.page_nav() == Nav::Imagine,
        ) {
            ctx.request_repaint_after(Duration::from_millis(80));
        }

        crate::theme::apply(ctx);
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
            egui::Window::new("Shortcuts").collapsible(false).show(ctx, |ui| {
                ui.label(shortcut_help());
                if ui.button("Close").clicked() {
                    self.shortcuts_open = false;
                }
            });
        }
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
                let (halt, _) = on_wheel_grab(self.running);
                if halt || self.running {
                    self.rx = None;
                    self.running = false;
                }
                if let Some(mut s) = self.voice_sock.take() {
                    s.halt();
                }
                self.voice_orb = "idle".into();
                self.voice_state = VoiceState::Idle;
                self.status = "Stopped".into();
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
        egui::Window::new("settings-menu")
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::LEFT_BOTTOM, [12.0, -56.0])
            .frame(
                egui::Frame::none()
                    .fill(crate::theme::PANEL)
                    .rounding(12.0)
                    .stroke(egui::Stroke::new(1.0_f32, crate::theme::BORDER))
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
                                    .color(crate::theme::FG),
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
                                .color(crate::theme::FG),
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
                                .color(crate::theme::FG),
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
            self.secrets.oauth = None;
            let _ = crate::secrets::save(&self.secrets);
            self.status = "Signed out".into();
            self.settings_menu_open = false;
        }
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
                if ui.button("Close").clicked() {
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
            .frame(egui::Frame::none().fill(crate::theme::BG).inner_margin(egui::Margin::same(24.0)))
            .show(ctx, |ui| {
            let _ = crate::cards::page_header(ui, "Command", "");
            ui.label(RichText::new("Host shell on this box. Bound project is the world.").color(crate::theme::MUTED));
            ui.add_space(12.0);
            if !self.host_live.is_empty() {
                ui.colored_label(Color32::from_rgb(232, 168, 96), &self.host_live);
            }
            ui.horizontal(|ui| {
                let enter = ui
                    .add(
                        egui::TextEdit::singleline(&mut self.cmd_line)
                            .hint_text("$ ls")
                            .desired_width(480.0),
                    )
                    .lost_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if ui.button("Run").clicked() || enter {
                    let line = self.cmd_line.trim().to_string();
                    if !line.is_empty() {
                        self.cmd_hist.push(line.clone());
                        self.cmd_line.clear();
                        self.queue_sh(line);
                    }
                }
            });
            ui.separator();
            ui.label(RichText::new("History").small());
            for h in self.cmd_hist.iter().rev().take(24) {
                ui.monospace(h);
            }
            if !self.last_host.is_empty() {
                ui.separator();
                ui.label(RichText::new("Last host").small());
                for c in &self.last_host {
                    ui.monospace(c);
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
            .frame(egui::Frame::none().fill(crate::theme::BG).inner_margin(egui::Margin::same(24.0)))
            .show(ctx, |ui| {
            let _ = crate::cards::page_header(ui, "Queue", "");
            ui.label(RichText::new("Goal loop. One writer per thread. Auto-continue stays under GOAL_MAX_STEPS.").color(crate::theme::MUTED));
            ui.add_space(12.0);
            ui.label(format!(
                "Goal pin: {} · step {}/{}",
                if self.cfg.goal_pin.is_empty() {
                    "none"
                } else {
                    self.cfg.goal_pin.as_str()
                },
                self.goal_step,
                GOAL_MAX_STEPS
            ));
            if self.agents.is_empty() {
                ui.label("No queued agent jobs.");
            }
            let mut run_at: Option<usize> = None;
            for (i, a) in self.agents.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(format!("{} · {}", a.status, a.title));
                    if ui.small_button("Run").clicked() {
                        run_at = Some(i);
                    }
                });
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
            .frame(egui::Frame::none().fill(crate::theme::BG))
            .show(ctx, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(RichText::new("×").color(crate::theme::MUTED))
                                    .fill(crate::theme::BG)
                                    .stroke(egui::Stroke::NONE),
                            )
                            .clicked()
                        {
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
                        if ui
                            .add(
                                egui::Button::new(RichText::new("□").color(crate::theme::MUTED))
                                    .fill(crate::theme::BG)
                                    .stroke(egui::Stroke::NONE),
                            )
                            .clicked()
                        {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
                        }
                        if ui
                            .add(
                                egui::Button::new(RichText::new("–").color(crate::theme::MUTED))
                                    .fill(crate::theme::BG)
                                    .stroke(egui::Stroke::NONE),
                            )
                            .clicked()
                        {
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
            crate::theme::NAV_ACTIVE
        } else {
            egui::Color32::TRANSPARENT
        };
        let color = if active {
            crate::theme::FG
        } else {
            crate::theme::MUTED
        };
        let w = ui.available_width();
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, crate::theme::NAV_ROW_H), egui::Sense::click());
        ui.painter().rect_filled(rect, 10.0, fill);
        if outline {
            ui.painter().rect_stroke(
                rect,
                10.0,
                egui::Stroke::new(1.0_f32, crate::theme::BORDER_STRONG),
            );
        }
        let icon_c = egui::pos2(rect.left() + 20.0, rect.center().y);
        let icon_rect = egui::Rect::from_center_size(icon_c, egui::vec2(20.0, 20.0));
        crate::icons::paint_rail_icon_at(ui.painter(), icon_rect, icon, color);
        ui.painter().text(
            egui::pos2(rect.left() + 38.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(crate::theme::FONT_CHROME),
            color,
        );
        resp
    }

    fn cabin_avatar(ui: &mut egui::Ui, account: &str, email: &str) -> egui::Response {
        let (rect, resp) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), RAIL_FOOTER_H), egui::Sense::click());
        let c = egui::pos2(rect.left() + 20.0, rect.center().y);
        ui.painter().circle_filled(c, 14.0, crate::theme::PANEL);
        ui.painter().circle_stroke(
            c,
            14.0,
            egui::Stroke::new(1.0_f32, crate::theme::BORDER_STRONG),
        );
        ui.painter().text(
            egui::pos2(rect.left() + 42.0, rect.center().y - 8.0),
            egui::Align2::LEFT_CENTER,
            account,
            egui::FontId::proportional(crate::theme::FONT_META),
            crate::theme::FG,
        );
        ui.painter().text(
            egui::pos2(rect.left() + 42.0, rect.center().y + 8.0),
            egui::Align2::LEFT_CENTER,
            email,
            egui::FontId::proportional(11.0),
            crate::theme::SUBTLE,
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
            .frame(egui::Frame::none().fill(crate::theme::BG).inner_margin(egui::Margin::same(8.0)))
            .show(ctx, |ui| {
                ui.add_space(4.0);
                if Self::nav_row(ui, false, crate::icons::RailIcon::Search, "Search", false).clicked()
                {
                    self.palette_open = true;
                    self.palette_focus = true;
                    self.palette_pick = 0;
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
                    ui.label(RichText::new("Projects").size(12.0).color(crate::theme::SUBTLE));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let plus = ui.add(
                            egui::Button::new(RichText::new("+").size(16.0).color(crate::theme::MUTED))
                                .fill(egui::Color32::TRANSPARENT)
                                .stroke(egui::Stroke::NONE)
                                .min_size(egui::vec2(22.0, 22.0)),
                        );
                        let plus_pos = plus.rect.left_bottom();
                        if plus.on_hover_text("New project or folder").clicked() {
                            self.proj_plus_open = true;
                            self.proj_plus_pos = plus_pos;
                            self.proj_ignore_close = true;
                            self.proj_menu = None;
                        }
                    });
                });
                let tree = visible_tree(&self.projects);
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
                    let active = self.project_sel.as_deref() == Some(self.projects[idx].id.as_str())
                        && kind == ProjectKind::Project;
                    let row = ui
                        .horizontal(|ui| {
                            ui.add_space(indent);
                            if kind == ProjectKind::Folder {
                                ui.label(
                                    RichText::new(if open { "▾" } else { "▸" })
                                        .size(12.0)
                                        .color(crate::theme::SUBTLE),
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
                    if row.secondary_clicked() {
                        self.proj_menu = Some(self.projects[idx].id.clone());
                        self.proj_menu_pos = row.rect.left_bottom();
                        self.proj_ignore_close = true;
                        self.proj_plus_open = false;
                        self.proj_add_for = None;
                    }
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("History").size(12.0).color(crate::theme::SUBTLE));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(RichText::new("See all").size(11.0).color(crate::theme::SUBTLE))
                                    .fill(egui::Color32::TRANSPARENT)
                                    .stroke(egui::Stroke::NONE),
                            )
                            .clicked()
                        {
                            self.nav = Nav::History;
                        }
                    });
                });
                ui.add(
                    egui::TextEdit::singleline(&mut self.sidebar_q)
                        .hint_text("Filter chats…")
                        .desired_width(ui.available_width())
                        .frame(false),
                );
                let hist_h = (ui.available_height() - RAIL_FOOTER_H).max(36.0);
                egui::ScrollArea::vertical()
                    .id_salt("rail-history")
                    .auto_shrink([false, true])
                    .max_height(hist_h)
                    .show(ui, |ui| {
                        let q = self.sidebar_q.to_ascii_lowercase();
                        let n = self.threads.len();
                        for i in 0..n {
                            let title = self.threads[i].title.as_str();
                            if !q.is_empty() && !title.to_ascii_lowercase().contains(&q) {
                                continue;
                            }
                            if Self::nav_row(
                                ui,
                                i == self.thread_idx && self.nav == Nav::Chat,
                                crate::icons::RailIcon::Chat,
                                title,
                                false,
                            )
                            .clicked()
                            {
                                self.switch_thread(i);
                                self.nav = Nav::Chat;
                            }
                        }
                    });
                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    if Self::cabin_avatar(ui, &account, &email).clicked() {
                        self.settings_menu_open = !self.settings_menu_open;
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
                        .fill(crate::theme::BG)
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
            .frame(egui::Frame::none().fill(crate::theme::BG).inner_margin(egui::Margin::same(20.0)))
            .show(ctx, |ui| {
                if empty {
                    self.ui_empty_home(ui);
                    return;
                }
                egui::ScrollArea::vertical()
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        for m in &self.messages {
                            let user = m.role == "user";
                            let fill = if user {
                                crate::theme::BUBBLE_USER
                            } else if m.role == "system" {
                                crate::theme::PANEL
                            } else {
                                crate::theme::BUBBLE_ASSISTANT
                            };
                            let bubble_w = crate::markdown::bubble_width(ui.available_width());
                            let draw = |ui: &mut egui::Ui| {
                                egui::Frame::none()
                                    .fill(fill)
                                    .stroke(egui::Stroke::new(1.0_f32, crate::theme::BORDER))
                                    .rounding(12.0)
                                    .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                                    .show(ui, |ui| {
                                        ui.set_width(bubble_w);
                                        if m.role == "assistant" {
                                            crate::markdown::show(ui, &m.content);
                                        } else {
                                            ui.label(RichText::new(&m.content).color(crate::theme::FG));
                                        }
                                    });
                            };
                            if user {
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::TOP),
                                    draw,
                                );
                            } else {
                                draw(ui);
                            }
                            ui.add_space(8.0);
                        }
                        if self.running {
                            ui.label(RichText::new("Working…").italics().color(crate::theme::SUBTLE));
                        }
                    });
            });
    }

    fn ui_empty_home(&mut self, ui: &mut egui::Ui) {
        let block = crate::theme::WORDMARK + 28.0 + crate::theme::QUERY_MIN_H;
        let lift = ((ui.available_height() - block) * 0.5).clamp(24.0, 320.0);
        ui.add_space(lift);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new("GrokHub")
                    .font(crate::theme::title_font(crate::theme::WORDMARK))
                    .color(crate::theme::FG),
            );
            ui.add_space(28.0);
            ui.set_max_width(crate::theme::QUERY_MAX_W);
            self.ui_composer_stack(ui);
        });
    }

    fn ui_composer_stack(&mut self, ui: &mut egui::Ui) {
            if needs_auth_banner(self.has_key()) && !self.messages.is_empty() {
                ui.horizontal(|ui| {
                    ui.colored_label(crate::theme::SETUP, "Connect Grok in Settings");
                    if ui.button("Settings").clicked() {
                        self.nav = Nav::Settings;
                    }
                });
            }
            if let Some(sk) = self.pending_skill.clone() {
                ui.horizontal(|ui| {
                    ui.label(format!("Approve skill {}?", sk.name));
                    if ui.button("Save SKILL.md").clicked() {
                        if skills::save_skill(&sk).is_ok() {
                            self.skill_list = skills::list_skills();
                            self.status = format!("Wrote skill {}", sk.name);
                        }
                        self.pending_skill = None;
                    }
                    if ui.button("Skip").clicked() {
                        self.pending_skill = None;
                    }
                });
            }
            if self.save_skill {
                ui.horizontal(|ui| {
                    ui.label("Save last host run as skill?");
                    if ui.button("Save as skill").clicked() {
                        self.stage_last_skill();
                    }
                    if ui.button("Skip").clicked() {
                        self.save_skill = false;
                    }
                });
            }
            if let Some(mut plan) = self.plan_pending.clone() {
                ui.label(format!("Allow this plan ({} steps)? Uncheck or reorder, then approve.", plan.len()));
                let mut up: Option<usize> = None;
                let mut down: Option<usize> = None;
                for (i, step) in plan.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut step.checked, "");
                        ui.label(format!("{} · {}", step.cmd, step.explain));
                        if ui.small_button("up").clicked() {
                            up = Some(i);
                        }
                        if ui.small_button("down").clicked() {
                            down = Some(i);
                        }
                    });
                }
                if let Some(i) = up {
                    move_step(&mut plan, i, true);
                }
                if let Some(i) = down {
                    move_step(&mut plan, i, false);
                }
                self.plan_pending = Some(plan);
                ui.horizontal(|ui| {
                    if ui.button("Approve").clicked() {
                        if let Some(p) = self.plan_pending.take() {
                            let cmds = approved_cmds(&p);
                            if self.pending_update {
                                self.pending_update = false;
                                self.start_overlay_update(cmds);
                            } else {
                                self.run_cmds(cmds);
                            }
                        }
                    }
                    if ui.button("Deny").clicked() {
                        self.plan_pending = None;
                        self.pending_update = false;
                        self.status.clear();
                    }
                });
            }
            let hits = filter_slash_commands(&self.composer);
            if !hits.is_empty() {
                let first = hits.first().map(|s| s.cmd).unwrap_or("");
                let n = hits.len();
                let changed = self.slash_filter_n != n || self.slash_filter_first != first;
                self.slash_pick = slash_pick_retain(self.slash_pick, changed, n);
                self.slash_filter_n = n;
                self.slash_filter_first = first;
                ui.label(RichText::new("↑↓ · Tab accepts · type /").small());
                egui::ScrollArea::vertical()
                    .max_height(148.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        for (i, s) in hits.iter().enumerate() {
                            let row = format!("{} — {}", s.cmd, s.hint);
                            if ui.selectable_label(i == self.slash_pick, row).clicked() {
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
                if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)) {
                    self.slash_pick = slash_pick_step(self.slash_pick, hits.len(), 1);
                } else if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp))
                {
                    self.slash_pick = slash_pick_step(self.slash_pick, hits.len(), -1);
                } else if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Tab)) {
                    let s = hits[self.slash_pick.min(hits.len() - 1)];
                    if let Some(t) = slash_pick_take(&mut self.composer, s.insert, s.run_on_pick) {
                        self.send_chat(t);
                    }
                }
            } else {
                self.slash_pick = 0;
                self.slash_filter_n = 0;
                self.slash_filter_first = "";
            }
            ui.add_space(6.0);
            ui.vertical_centered(|ui| {
            ui.set_max_width(crate::theme::QUERY_MAX_W);
            egui::Frame::none()
                .fill(crate::theme::ELEVATED)
                .rounding(crate::theme::QUERY_RADIUS)
                .stroke(egui::Stroke::new(1.0_f32, crate::theme::BORDER))
                .inner_margin(egui::Margin::same(8.0))
                .show(ui, |ui| {
                    ui.set_min_height(crate::theme::QUERY_MIN_H - 16.0);
                    ui.horizontal(|ui| {
                        if crate::icons::paint_bar_icon(
                            ui,
                            crate::icons::BarIcon::Plus,
                            22.0,
                            crate::theme::MUTED,
                        )
                        .on_hover_text("Paste clipboard")
                        .clicked()
                        {
                            if let Some(clip) = crate::desktop::clipboard_once() {
                                if !self.composer.is_empty() && !self.composer.ends_with('\n') {
                                    self.composer.push('\n');
                                }
                                self.composer.push_str(&clip);
                            } else {
                                self.status = "clipboard empty — install wl-paste or xclip".into();
                            }
                        }
                        let edit = ui.add(
                            egui::TextEdit::multiline(&mut self.composer)
                                .desired_width((ui.available_width() - 180.0).max(80.0))
                                .desired_rows(1)
                                .frame(false)
                                .hint_text("What do you want to know?"),
                        );
                        let mut mode = if self.cfg.mode.trim().is_empty() {
                            "auto".to_string()
                        } else {
                            self.cfg.mode.clone()
                        };
                        let mode_now = mode.clone();
                        egui::ComboBox::from_id_salt("composer-mode")
                            .selected_text(Self::mode_label(&mode_now))
                            .width(84.0)
                            .show_ui(ui, |ui| {
                                for (id, label) in [
                                    ("auto", "Auto"),
                                    ("fast", "Fast"),
                                    ("balanced", "Balance"),
                                    ("think", "Think"),
                                    ("max", "Max"),
                                ] {
                                    ui.selectable_value(&mut mode, id.to_string(), label);
                                }
                            });
                        if mode != mode_now {
                            self.run_slash(Slash::Mode(mode));
                        }
                        if crate::icons::paint_bar_icon(
                            ui,
                            crate::icons::BarIcon::Mic,
                            22.0,
                            crate::theme::MUTED,
                        )
                        .on_hover_text("Hey Grok")
                        .clicked()
                        {
                            self.listen_voice();
                        }
                        let ready = !self.composer.trim().is_empty();
                        let send = crate::icons::paint_bar_icon(
                            ui,
                            if ready {
                                crate::icons::BarIcon::Send
                            } else {
                                crate::icons::BarIcon::ArrowUp
                            },
                            if ready { 28.0 } else { 22.0 },
                            if ready {
                                crate::theme::FG
                            } else {
                                crate::theme::MUTED
                            },
                        )
                        .on_hover_text("Send");
                        if send.clicked()
                            || (edit.has_focus()
                                && ui.input(|i| {
                                    i.key_pressed(egui::Key::Enter) && i.modifiers.command
                                }))
                        {
                            let t = std::mem::take(&mut self.composer);
                            self.send_chat(t);
                        }
                    });
                });
            });
    }

    fn ui_devices(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(crate::theme::BG).inner_margin(egui::Margin::same(24.0)))
            .show(ctx, |ui| {
            if crate::cards::page_header(ui, "Devices", if self.hub_on { "Sharing" } else { "Start share" }) {
                self.start_hub();
            }
            ui.label(RichText::new("Pairing stays the same. Receipts come back here.").color(crate::theme::MUTED));
            ui.add_space(12.0);
            if let Ok(st) = self.hub.lock() {
                ui.label(format!("This box: {} ({})", st.device_name, st.device_id));
                if let Some(p) = &st.pair {
                    ui.label(RichText::new(format!("Pair code  {}", p.code)).strong().monospace());
                    ui.label(format!("http://<lan>:{}", self.hub_port));
                } else if self.hub_on {
                    ui.label("Paired — generate a new code after a phone joins.");
                    if ui.button("New code").clicked() {
                        drop(st);
                        if let Ok(mut s) = self.hub.lock() {
                            s.rotate_pair();
                        }
                        return;
                    }
                }
                if let Some(f) = &st.last_frame {
                    ui.label(format!("Last frame at {}", f.at));
                }
            }
            ui.add_space(8.0);
            ui.label("Task for this computer");
            ui.add(
                egui::TextEdit::multiline(&mut self.task_prompt)
                    .desired_rows(3)
                    .desired_width(f32::INFINITY),
            );
            if ui.button("Send a task home").clicked() {
                let t = std::mem::take(&mut self.task_prompt);
                self.nav = Nav::Chat;
                self.send_chat(t);
            }
        });
    }

    fn ui_memory(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(crate::theme::BG).inner_margin(egui::Margin::same(24.0)))
            .show(ctx, |ui| {
            let _ = crate::cards::page_header(ui, "Memory", "");
            ui.horizontal(|ui| {
                for name in ["SOUL.md", "USER.md", "MEMORY.md"] {
                    if ui.selectable_label(self.mem_name == name, name).clicked() {
                        self.mem_name = name.into();
                        self.mem_body = config::read_memory(name);
                    }
                }
                if ui.button("Save").clicked() {
                    match config::write_memory(&self.mem_name, &self.mem_body) {
                        Ok(()) => self.status = format!("Wrote {}", self.mem_name),
                        Err(e) => self.status = e,
                    }
                }
                if ui.button("Reflect").clicked() {
                    self.run_reflect();
                }
                if ui.button("Restore").clicked() {
                    match config::restore_memory(&self.mem_name) {
                        Ok(body) => {
                            self.mem_body = body;
                            self.status = format!("Restored {}.prev", self.mem_name);
                        }
                        Err(e) => self.status = e,
                    }
                }
            });
            if !self.reflect_diff.is_empty() {
                ui.label(RichText::new("Last reflect").small());
                ui.label(RichText::new(&self.reflect_diff).monospace());
            }
            ui.add(
                egui::TextEdit::multiline(&mut self.mem_body)
                    .desired_rows(24)
                    .desired_width(f32::INFINITY)
                    .font(egui::TextStyle::Monospace),
            );
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
        let passenger = passenger_label(self.cfg.autonomy);
        let doctor = self.doctor_text();
        let usage = usage_line(&self.usage);
        let catalog = catalog_line();
        let mut close = false;
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
                        .fill(crate::theme::BG)
                        .rounding(16.0)
                        .stroke(egui::Stroke::new(1.0_f32, crate::theme::BORDER))
                        .inner_margin(egui::Margin::ZERO)
                        .show(ui, |ui| {
                            ui.set_min_size(modal.size());
                            ui.horizontal(|ui| {
                                ui.allocate_ui_with_layout(
                                    egui::vec2(220.0, modal.height()),
                                    egui::Layout::top_down(egui::Align::Min),
                                    |ui| {
                                        egui::Frame::none()
                                            .fill(crate::theme::SURFACE)
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
                                                    .color(crate::theme::FG),
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
                                                                    .color(crate::theme::MUTED),
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
                                                            crate::cards::settings_field(ui, "Console key", "Optional. Never in markdown.", &mut self.cfg.api_key, true);
                                                            crate::cards::settings_field(ui, "Device name", "How this box shows up on the hub.", &mut self.cfg.device_name, false);
                                                            crate::cards::settings_field(ui, "Chat model", "Empty means the cabin default. Imagine never shares this.", &mut self.cfg.model, false);
                                                            crate::cards::settings_field(ui, "Composer mode", "auto, fast, Balance (Grok 4.3), Think (Grok 4.6 high), or max (Grok 4.6 xhigh).", &mut self.cfg.mode, false);
                                                        }
                                                        SettingsSec::Appearance => {
                                                            crate::cards::settings_note(ui, "The cabin is dark. Light and System stay on grok.com.");
                                                            ui.horizontal(|ui| {
                                                                for (label, on) in [("Light", false), ("Dark", true), ("System", false)] {
                                                                    let fill = if on { crate::theme::NAV_ACTIVE } else { crate::theme::SURFACE };
                                                                    egui::Frame::none()
                                                                        .fill(fill)
                                                                        .rounding(12.0)
                                                                        .stroke(egui::Stroke::new(1.0_f32, if on { crate::theme::FG } else { crate::theme::BORDER }))
                                                                        .inner_margin(egui::Margin::same(10.0))
                                                                        .show(ui, |ui| {
                                                                            let preview = if label == "Light" {
                                                                                Color32::from_rgb(0xF4, 0xF4, 0xF5)
                                                                            } else {
                                                                                crate::theme::BG
                                                                            };
                                                                            ui.allocate_ui(egui::vec2(88.0, 56.0), |ui| {
                                                                                ui.painter().rect_filled(ui.max_rect(), 6.0, preview);
                                                                            });
                                                                            ui.label(RichText::new(label).size(13.0).color(crate::theme::FG));
                                                                        });
                                                                    ui.add_space(10.0);
                                                                }
                                                            });
                                                        }
                                                        SettingsSec::Behavior => {
                                                            if crate::cards::settings_toggle(ui, "Close to tray", "The cabin keeps working in the background.", &mut self.cfg.close_to_tray) {
                                                                save = true;
                                                            }
                                                            if crate::cards::settings_toggle(ui, "Cabin eyes", "Attach a room frame when seeing.", &mut self.cfg.cabin_eyes) {
                                                                save = true;
                                                            }
                                                        }
                                                        SettingsSec::Host => {
                                                            if crate::cards::settings_toggle(ui, "Host tools", "Unsandboxed commands on this box.", &mut self.cfg.host_on) {
                                                                save = true;
                                                            }
                                                            if crate::cards::settings_toggle(ui, "YOLO", "/approve off — host runs without a prompt.", &mut self.cfg.yolo) {
                                                                save = true;
                                                            }
                                                            crate::cards::settings_note(ui, &format!("Passenger: {passenger}"));
                                                            ui.horizontal(|ui| {
                                                                ui.label(RichText::new("Autonomy").size(15.0).color(crate::theme::FG));
                                                                ui.add(egui::Slider::new(&mut self.cfg.autonomy, 0..=4).show_value(true));
                                                            });
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
                                                            crate::cards::settings_field(ui, "Voice override", "Must contain “voice” or “realtime”.", &mut self.cfg.voice_model, false);
                                                        }
                                                        SettingsSec::Night => {
                                                            crate::cards::settings_field(ui, "Quiet start", "Local time. Automations hold here.", &mut self.cfg.quiet_start, false);
                                                            crate::cards::settings_field(ui, "Quiet end", "Local time.", &mut self.cfg.quiet_end, false);
                                                            ui.horizontal(|ui| {
                                                                ui.label(RichText::new("Daily cap").size(15.0).color(crate::theme::FG));
                                                                ui.add(egui::Slider::new(&mut self.cfg.daily_auto_cap, 0..=200));
                                                            });
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
                                                            crate::cards::settings_note(ui, "This is the Rust cabin. Not Electron. Not Tauri.");
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
            self.secrets.oauth = None;
            let _ = secrets::save(&self.secrets);
            self.status = "Signed out".into();
        }
        if update {
            self.queue_update();
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
            ctx.output_mut(|o| o.copied_text = bundle.clone());
            self.status = bundle;
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
            .frame(egui::Frame::none().fill(crate::theme::BG).inner_margin(egui::Margin::same(24.0)))
            .show(ctx, |ui| {
            if crate::cards::page_header(ui, "Automations", "New Automation") {
                self.auto_compose = true;
            }
            egui::ScrollArea::vertical().show(ui, |ui| {
            if self.auto_compose {
                ui.add_space(12.0);
                egui::Frame::none()
                    .fill(crate::theme::ELEVATED)
                    .rounding(12.0)
                    .stroke(egui::Stroke::new(1.0_f32, crate::theme::BORDER))
                    .inner_margin(egui::Margin::same(14.0))
                    .show(ui, |ui| {
                        ui.label(RichText::new("New automation").strong());
                        ui.add(
                            egui::TextEdit::singleline(&mut self.night_nl)
                                .hint_text("every weekday at 9, summarize the workboard")
                                .desired_width(f32::INFINITY),
                        );
                        ui.horizontal(|ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Add").strong().color(crate::theme::BG),
                                    )
                                    .fill(crate::theme::FG)
                                    .rounding(12.0),
                                )
                                .clicked()
                            {
                                let seed = std::mem::take(&mut self.night_nl);
                                self.add_automation_seed(&seed);
                                if self.status == "Automation added" {
                                    self.auto_compose = false;
                                }
                            }
                            if ui.button("Cancel").clicked() {
                                self.auto_compose = false;
                            }
                        });
                    });
            }
            ui.add_space(8.0);
            crate::cards::section_label(ui, "Active");
            let mut drop: Option<usize> = None;
            if self.automations.is_empty() {
                ui.label(
                    RichText::new("None yet — pick a suggestion or New Automation.")
                        .color(crate::theme::MUTED),
                );
                ui.add_space(16.0);
            } else {
                let n = self.automations.len();
                crate::cards::tile_row(ui, n, |ui, i| {
                    let a = &mut self.automations[i];
                    let title = a.name.chars().take(40).collect::<String>();
                    let body = format!("{} {} · {} runs", a.schedule, a.time, a.run_count);
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut a.enabled, "");
                    });
                    if crate::cards::grok_tile(ui, crate::icons::TileIcon::Bolt, &title, &body, Some("Run"), false)
                        == crate::cards::TileHit::Add
                    {
                        drop = Some(usize::MAX - i);
                    }
                    if crate::cards::ghost_pill(ui, "Remove") {
                        drop = Some(i);
                    }
                });
                ui.add_space(20.0);
            }
            crate::cards::section_label(ui, "Suggested");
            crate::cards::tile_row(ui, crate::cards::SUGGESTED_AUTOS.len(), |ui, i| {
                let s = crate::cards::SUGGESTED_AUTOS[i];
                if matches!(
                    crate::cards::grok_tile(ui, s.icon, s.title, s.body, Some("Add"), false),
                    crate::cards::TileHit::Add | crate::cards::TileHit::Body
                ) {
                    self.add_automation_seed(s.seed);
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
                        self.nav = Nav::Chat;
                        self.send_chat(inst);
                    }
                }
            }
            });
        });
    }

    fn ui_history(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(crate::theme::BG).inner_margin(egui::Margin::same(24.0)))
            .show(ctx, |ui| {
            let _ = crate::cards::page_header(ui, "History", "");
            ui.horizontal(|ui| {
                ui.add(
                    egui::TextEdit::singleline(&mut self.history_q)
                        .hint_text("search chats and memory")
                        .desired_width(320.0),
                );
                if ui.button("Search").clicked() {
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
            for h in &self.history_hits {
                ui.label(h);
            }
        });
    }

    fn ui_board(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(crate::theme::BG).inner_margin(egui::Margin::same(24.0)))
            .show(ctx, |ui| {
            if crate::cards::page_header(ui, "Workboard", "New card") {
                self.board_compose = true;
            }
            if self.board_compose {
                egui::Frame::none()
                    .fill(crate::theme::ELEVATED)
                    .rounding(16.0)
                    .stroke(egui::Stroke::new(1.0_f32, crate::theme::BORDER))
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
                ui.label(RichText::new("No cards yet.").color(crate::theme::MUTED));
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
                        if crate::cards::ghost_pill(ui, "Approve") {
                            bump = Some((i, BoardStatus::Approved));
                        }
                        if crate::cards::ghost_pill(ui, "Start") {
                            bump = Some((i, BoardStatus::InProgress));
                        }
                        if crate::cards::ghost_pill(ui, "Done") {
                            if can_mark_done(self.verify_ok_turn, false) {
                                bump = Some((i, BoardStatus::Done));
                            } else {
                                self.status = "Need VERIFY_OK or a passing verify.sh".into();
                            }
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
        let mut new_project = false;
        let mut go_settings = false;
        let mut seed: Option<String> = None;
        let word = crate::cards::imagine_word(now_ms());
        let selected = self.imagine_prompt.clone();
        let panel = egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(crate::theme::BG).inner_margin(egui::Margin::ZERO))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing = egui::vec2(0.0, 0.0);
                        crate::cards::imagine_masonry(ui, &selected, now_ms(), &self.wall.gifs, |p| {
                            seed = Some(p);
                        });
                    });
            });
        let content = panel.response.rect;
        let bar_w = (content.width() - 48.0)
            .min(crate::theme::IMAGINE_BAR_W)
            .max(280.0);
        egui::Area::new(egui::Id::new("imagine-new"))
            .fixed_pos(egui::pos2(content.right() - 148.0, content.top() + 12.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                if crate::cards::white_pill(ui, "+ New project") {
                    new_project = true;
                }
            });
        egui::Area::new(egui::Id::new("imagine-composer"))
            .fixed_pos(egui::pos2(
                content.center().x - bar_w * 0.5,
                content.top() + 36.0,
            ))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.set_width(bar_w);
                let fade = egui::Rect::from_min_size(
                    egui::pos2(ui.next_widget_position().x - 24.0, content.top()),
                    egui::vec2(bar_w + 48.0, 220.0),
                );
                ui.painter()
                    .rect_filled(fade, 0.0, Color32::from_black_alpha(110));
                ui.vertical_centered(|ui| {
                    ui.label(
                        RichText::new(format!("Imagine {word}"))
                            .font(crate::theme::title_font(crate::theme::IMAGINE_TITLE))
                            .color(crate::theme::FG),
                    );
                    ui.add_space(crate::theme::IMAGINE_GAP);
                    let bar = self.ui_imagine_bar(ui);
                    generate = bar.generate;
                    go_settings = bar.go_settings;
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
        if generate {
            self.kick_imagine();
        }
    }

    fn ui_imagine_bar(&mut self, ui: &mut egui::Ui) -> ImagineBarOut {
        let mut out = ImagineBarOut::default();
        let bar_w = ui.available_width().min(crate::theme::IMAGINE_BAR_W);
        let focused = ui.memory(|m| m.has_focus(egui::Id::new("imagine-prompt")));
        let stroke = if focused {
            crate::theme::BORDER_STRONG
        } else {
            crate::theme::BORDER
        };
        let model = dedicated_imagine_model(&self.cfg.imagine_model);
        let ready = !self.imagine_prompt.trim().is_empty();
        let authed = self.has_key();
        egui::Frame::none()
            .fill(crate::theme::SURFACE)
            .rounding(crate::theme::IMAGINE_BAR_RADIUS)
            .stroke(egui::Stroke::new(1.0_f32, stroke))
            .inner_margin(egui::Margin::same(12.0))
            .show(ui, |ui| {
                ui.set_width(bar_w);
                ui.set_min_height(crate::theme::IMAGINE_BAR_H - 20.0);
                let edit = ui.add(
                    egui::TextEdit::multiline(&mut self.imagine_prompt)
                        .id(egui::Id::new("imagine-prompt"))
                        .desired_width((ui.available_width() - 8.0).max(80.0))
                        .desired_rows(1)
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
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    let (plus_r, plus) = ui.allocate_exact_size(
                        egui::vec2(crate::theme::IMAGINE_HIT, crate::theme::IMAGINE_HIT),
                        egui::Sense::click(),
                    );
                    ui.painter()
                        .circle_filled(plus_r.center(), 18.0, crate::theme::PANEL);
                    crate::icons::paint_plus_at(ui.painter(), plus_r, crate::theme::MUTED);
                    if plus.on_hover_text("Upload — paste clipboard into the still").clicked() {
                        if let Some(clip) = crate::desktop::clipboard_once() {
                            if !self.imagine_prompt.is_empty() && !self.imagine_prompt.ends_with('\n')
                            {
                                self.imagine_prompt.push('\n');
                            }
                            self.imagine_prompt.push_str(&clip);
                        } else {
                            self.status = "clipboard empty — install wl-paste or xclip".into();
                        }
                    }
                    crate::cards::imagine_seg_track(ui, |ui| {
                        for kind in [
                            crate::cards::ImagineKind::Image,
                            crate::cards::ImagineKind::Video,
                            crate::cards::ImagineKind::Agent,
                        ] {
                            let on = kind == crate::cards::ImagineKind::Image;
                            let label = crate::cards::imagine_kind_label(kind);
                            if crate::cards::imagine_seg_chip(ui, on, |ui| {
                                match kind {
                                    crate::cards::ImagineKind::Image => {
                                        crate::icons::paint_image_mode(ui, 16.0, crate::theme::FG);
                                        ui.add_space(4.0);
                                        ui.label(
                                            RichText::new(label)
                                                .size(crate::theme::FONT_CHROME)
                                                .color(crate::theme::FG),
                                        );
                                    }
                                    crate::cards::ImagineKind::Video => {
                                        crate::icons::paint_video_mode(ui, 16.0, crate::theme::MUTED);
                                    }
                                    crate::cards::ImagineKind::Agent => {
                                        crate::icons::paint_agent_mode(ui, 16.0, crate::theme::MUTED);
                                    }
                                }
                            }) && kind != crate::cards::ImagineKind::Image
                            {
                                self.status =
                                    "Cabin stills only — Video and Agent stay on grok.com.".into();
                            }
                        }
                    });
                    crate::cards::imagine_seg_track(ui, |ui| {
                        for quality in [false, true] {
                            let on = self.imagine_quality == quality;
                            let label = crate::cards::imagine_quality_label(quality);
                            if crate::cards::imagine_seg_chip(ui, on, |ui| {
                                ui.label(
                                    RichText::new(label)
                                        .size(crate::theme::FONT_CHROME)
                                        .color(if on {
                                            crate::theme::FG
                                        } else {
                                            crate::theme::MUTED
                                        }),
                                );
                            }) {
                                self.imagine_quality = quality;
                            }
                        }
                    });
                    let style = egui::Frame::none()
                        .fill(crate::theme::PANEL)
                        .rounding(crate::theme::IMAGINE_HIT)
                        .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                        .show(ui, |ui| {
                            ui.set_height(crate::theme::IMAGINE_HIT - 12.0);
                            ui.horizontal_centered(|ui| {
                                crate::icons::paint_style_auto(ui, 16.0, crate::theme::FG);
                                ui.add_space(4.0);
                                ui.label(
                                    RichText::new("Auto")
                                        .size(crate::theme::FONT_CHROME)
                                        .color(crate::theme::FG),
                                );
                            });
                        })
                        .response
                        .interact(egui::Sense::click())
                        .on_hover_text("Style Auto — the cabin still");
                    if style.clicked() {
                        self.status = "Auto is the cabin still.".into();
                    }
                    let aspect = crate::cards::imagine_aspect_label(self.imagine_aspect);
                    let aspect_hit = egui::Frame::none()
                        .fill(crate::theme::PANEL)
                        .rounding(crate::theme::IMAGINE_HIT)
                        .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                        .show(ui, |ui| {
                            ui.set_height(crate::theme::IMAGINE_HIT - 12.0);
                            ui.horizontal_centered(|ui| {
                                crate::icons::paint_aspect_rect(
                                    ui,
                                    self.imagine_aspect,
                                    16.0,
                                    crate::theme::FG,
                                );
                                ui.add_space(4.0);
                                ui.label(
                                    RichText::new(aspect)
                                        .size(crate::theme::FONT_CHROME)
                                        .color(crate::theme::FG),
                                );
                            });
                        })
                        .response
                        .interact(egui::Sense::click())
                        .on_hover_text(format!("Still frame {aspect} · {model}"));
                    if aspect_hit.clicked() {
                        self.imagine_aspect = (self.imagine_aspect + 1) % 3;
                    }
                    if !authed && crate::cards::ghost_pill(ui, "Connect Grok") {
                        out.go_settings = true;
                    } else if self.running && self.page_nav() == Nav::Imagine {
                        ui.label(
                            RichText::new("Imagining…")
                                .size(crate::theme::FONT_META)
                                .color(crate::theme::MUTED),
                        );
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        let send = crate::icons::paint_bar_icon(
                            ui,
                            if ready && !self.running {
                                crate::icons::BarIcon::Send
                            } else {
                                crate::icons::BarIcon::ArrowUp
                            },
                            crate::theme::IMAGINE_HIT,
                            if ready && !self.running {
                                crate::theme::FG
                            } else {
                                crate::theme::MUTED
                            },
                        )
                        .on_hover_text(if self.running {
                            "Imagining…"
                        } else {
                            "Generate still · Enter"
                        });
                        if send.clicked() && ready && !self.running {
                            out.generate = true;
                        }
                        if crate::icons::paint_bar_icon(
                            ui,
                            crate::icons::BarIcon::Mic,
                            crate::theme::IMAGINE_HIT,
                            crate::theme::MUTED,
                        )
                        .on_hover_text("Hey Grok")
                        .clicked()
                        {
                            self.listen_voice();
                        }
                    });
                });
            });
        out
    }

    fn ui_skills(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(crate::theme::BG).inner_margin(egui::Margin::same(24.0)))
            .show(ctx, |ui| {
            if crate::cards::page_header(ui, "Skills and Connectors", "New Skill") {
                self.skills_tab_connectors = false;
                let stub = crate::cards::starter_skill("new-skill");
                self.skill_name = stub.name.clone();
                self.skill_body = grokhub_core::render_skill_md(&stub);
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
                        .color(crate::theme::MUTED),
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
                                    egui::Button::new(RichText::new(*label).color(crate::theme::FG))
                                        .fill(crate::theme::ELEVATED)
                                        .rounding(14.0),
                                )
                                .clicked()
                            {
                                gh_tool = Some(*tool);
                            }
                        }
                        if ui
                            .add(
                                egui::Button::new(RichText::new("Settings").color(crate::theme::BG))
                                    .fill(crate::theme::FG)
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
            } else {
            crate::cards::section_label(ui, "Suggested");
            let pending: Vec<_> = crate::cards::SUGGESTED_SKILLS
                .iter()
                .copied()
                .filter(|s| !self.skill_list.iter().any(|e| e.name == s.name))
                .collect();
            crate::cards::tile_row(ui, pending.len(), |ui, i| {
                let s = pending[i];
                if matches!(
                    crate::cards::grok_tile(ui, s.icon, s.title, s.body, Some("Add"), false),
                    crate::cards::TileHit::Add | crate::cards::TileHit::Body
                ) {
                    let sk = crate::cards::skill_from_suggested(&s);
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
                        .color(crate::theme::MUTED),
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
                ui.add(
                    egui::TextEdit::multiline(&mut self.skill_body)
                        .desired_rows(12)
                        .desired_width(f32::INFINITY)
                        .font(egui::TextStyle::Monospace),
                );
                ui.horizontal(|ui| {
                    if ui.button("Save SKILL.md").clicked() {
                        let parsed = grokhub_core::parse_skill_md(&self.skill_body);
                        match skills::save_skill(&parsed) {
                            Ok(p) => {
                                self.skill_list = skills::list_skills();
                                self.status = format!("Wrote {}", p.display());
                            }
                            Err(e) => self.status = e,
                        }
                    }
                    if ui.button("Use in chat").clicked() && !self.skill_name.is_empty() {
                        let name = self.skill_name.clone();
                        if let Some(s) = self.skill_list.iter().find(|s| s.name == name) {
                            if let Some(r) = parse_recipe(&s.instructions) {
                                self.last_recipe = Some(r);
                            }
                        }
                        self.nav = Nav::Chat;
                        self.send_chat(format!("Follow skill {name}"));
                    }
                    if ui.button("Run verify").clicked() && !self.skill_name.is_empty() {
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
            .frame(egui::Frame::none().fill(crate::theme::BG).inner_margin(egui::Margin::same(24.0)))
            .show(ctx, |ui| {
            let _ = crate::cards::page_header(ui, "Eyes", "");
            ui.label(RichText::new("AT-SPI via pyatspi when present, else wmctrl + xdotool cursor. Lock screens are won’ts. Cabin eyes captures a frame on each chat send.").color(crate::theme::MUTED));
            ui.add_space(12.0);
            ui.horizontal(|ui| {
                if ui.button("Scan + frame").clicked() {
                    self.refresh_eyes();
                }
                if ui.button("Replay recipe").clicked() {
                    self.replay_recipe();
                }
            });
            ui.horizontal_wrapped(|ui| {
                for line in self.eyes_text.lines() {
                    let Some(rest) = line.strip_prefix("- [") else {
                        continue;
                    };
                    let Some((kind, rest)) = rest.split_once(']') else {
                        continue;
                    };
                    let label = rest.trim().split(" @").next().unwrap_or(rest).trim();
                    let c = match kind {
                        "wont" => Color32::from_rgb(220, 80, 70),
                        "cursor" => Color32::from_rgb(96, 180, 232),
                        _ => Color32::from_rgb(232, 168, 96),
                    };
                    ui.colored_label(c, format!("{kind} {label}"));
                }
            });
            if let Some(r) = &self.last_recipe {
                if let Some(s) = r.screen {
                    ui.label(format!("Recipe screen {}x{}", s.w, s.h));
                }
            }
            ui.label(format!(
                "Presence ring {} frames (memory only, never lastFrame on disk)",
                self.presence_ring.len()
            ));
            if self.webcam_url.is_some() {
                ui.label("Webcam fused this session");
            }
            if let Some((ts, _)) = self.presence_ring.last() {
                ui.label(format!("Last live JPEG at {ts}"));
            }
            ui.add(
                egui::TextEdit::multiline(&mut self.eyes_text)
                    .desired_rows(18)
                    .desired_width(f32::INFINITY)
                    .font(egui::TextStyle::Monospace),
            );
        });
    }
}

fn click_project_opens_board(already_selected: bool) -> bool {
    already_selected
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

fn expand_home(p: &str) -> String {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return format!("{home}/{rest}");
        }
    }
    if p == "~" {
        return std::env::var("HOME").unwrap_or_else(|_| p.to_string());
    }
    p.to_string()
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
    fn click_other_project_stays_on_this_pane() {
        assert!(!super::click_project_opens_board(false));
    }

    #[test]
    fn click_bound_project_opens_the_board() {
        assert!(super::click_project_opens_board(true));
    }

    #[test]
    fn health_opens_the_about_page() {
        assert_eq!(super::health_settings_sec(), super::SettingsSec::About);
    }

    #[test]
    fn about_section_opens_update() {
        assert_eq!(
            super::settings_group_home(super::SettingsGroup::About),
            super::SettingsSec::Update
        );
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
        assert!(!super::wants_live_repaint(false, false, false, true, false));
        assert!(super::wants_live_repaint(false, false, false, false, false));
        assert!(super::wants_live_repaint(true, false, false, true, false));
    }

    #[test]
    fn rail_footer_is_reserved() {
        assert_eq!(super::RAIL_FOOTER_H, 52.0);
        assert!(super::PALETTE_LIST_H < 400.0);
    }
}
