// Stubbed Tauri internals for the screenshot / color-share harness.
// Injected before the app boots so `inShell()` is true and every `invoke`
// resolves from fixtures (no real desktop).

/** Stable fake settings the Appearance and top bar read. */
export function settings(theme = "ops-dark") {
  return {
    closeToTray: true,
    hasApiKey: true,
    signedIn: { name: "user", email: null },
    workspace: "",
    workspacePath: "/home/user/GrokHub",
    approvals: "ask",
    desktopControl: false,
    theme,
    version: "3.5.0",
    summonHotkey: false,
    notifications: true,
    planNote: null,
  };
}

/** Caps that show Chat, Home, Pulse, Code and Settings (no first-run). */
export function capabilities() {
  return {
    desktopControl: true,
    pulse: true,
    localAi: true,
    tray: true,
    webAccess: true,
    plugins: true,
    grokSignIn: true,
    autostart: true,
    summonHotkey: true,
    notifications: true,
    voice: true,
    library: true,
    remote: false,
    autoSetup: false,
    code: true,
    mods: true,
    windowControls: true,
  };
}

const BEHAVIOR = {
  closeToTray: true,
  behavior: {
    autocomplete: false,
    suggestions: true,
    quietStart: "",
    quietEnd: "",
    dailyTokenBudget: 0,
    pauseOverBudget: false,
    lane: "coding",
  },
  budgets: [
    { value: 0, label: "Off" },
    { value: 1000000, label: "1.0M tokens a day" },
  ],
};

const EMPTY_BOARD = {
  columns: [],
  cards: [],
  swept: [],
};

const EMPTY_PULSE = {
  feed: [
    {
      id: "shot-post",
      source: "pulse:writer",
      title: "Screenshot harness post",
      summary: "A fixed Pulse card so the screen is not empty.",
      because: "",
      icon: "🚀",
      why: "Harness fixture.",
      images: [],
      createdAt: Date.now() - 60_000,
      ago: "1m ago",
      link: "",
      loved: false,
    },
    {
      id: "shot-post-2",
      source: "pulse:writer",
      title: "Season patch: your main got a buff",
      summary: "Faster reload and a smaller recoil kick.",
      because: "",
      icon: "🎮",
      why: "Harness fixture.",
      images: [],
      createdAt: Date.now() - 3 * 3_600_000,
      ago: "3h ago",
      link: "",
      loved: true,
    },
  ],
  ideas: [],
};

const EMPTY_TRANSCRIPT = {
  id: "shot-chat",
  title: "Hangout",
  lines: [
    { role: "user", text: "hey" },
    { role: "assistant", text: "Hello from the screenshot harness." },
  ],
  earlier: 0,
};

const EMPTY_TAB = { id: "shot-chat", title: "Hangout", pinned: false, updated: 0, main: false };
const SECOND_TAB = { id: "shot-chat-2", title: "Fan curve", pinned: false, updated: 0, main: false };

/** Composer and Home chip row: two plain chips and the one primary. */
const QUICK_CHIPS = [
  { id: "c1", label: "Plan my weekend", value: "Plan my weekend", action: "ask", hint: "", primary: false },
  { id: "c2", label: "Roast my build", value: "Roast my build", action: "ask", hint: "", primary: false },
  { id: "c3", label: "Fix my stutter", value: "Fix my stutter", action: "ask", hint: "", primary: true },
];

const XP = {
  level: 14,
  title: "Grinder",
  xp: 2340,
  nextLevelXp: 2600,
  frac: 0.78,
  today: 110,
  week: { days: [true, true, false, true, false, false, false], today: 3, active: 3, target: 4, inARow: 5, played: 7, freezes: 1 },
};

function homeView(over) {
  return {
    state: "ready",
    tag: null,
    text: { big: "", caption: "", detail: null, tone: "normal" },
    rows: [],
    progress: null,
    buttons: [],
    tap: null,
    updatedMs: null,
    ...over,
  };
}

/** Mission Control with the tiles the shell fills (same shape as Home.test.tsx). */
const HOME = {
  preset: "mission_control",
  xpBar: true,
  xp: XP,
  glance: { memories: 12, proposed: 2, connectors: 3, connected: 2, failing: 0 },
  slots: [
    {
      slot: "large",
      size: "large",
      module: "objectives",
      name: "Objectives",
      purpose: "",
      refreshSecs: null,
      tile: {
        kind: "objectives",
        xp: XP,
        mission: {
          conversationId: "shot-chat",
          title: "Diamond by Sunday",
          status: "active",
          done: 1,
          total: 3,
          canAccept: false,
          accepted: false,
          steps: [
            { id: "1", text: "Aim routine 12/20", status: "verified" },
            { id: "2", text: "Three ranked wins", status: "attempted" },
            { id: "3", text: "Review one replay", status: "attempted" },
          ],
        },
      },
    },
    {
      slot: "medium-1",
      size: "medium",
      module: "pc_loadout",
      name: "PC Loadout",
      purpose: "",
      refreshSecs: null,
      tile: {
        kind: "view",
        view: homeView({
          text: { big: "92%", caption: "GPU 74° · driver latest", detail: null, tone: "normal" },
          rows: [
            { label: "GPU", value: "74°", tone: "warn", tap: null },
            { label: "Driver", value: "Latest", tone: "normal", tap: null },
          ],
          buttons: [{ label: "Tune-up", action: { kind: "ask", ask: "Check this PC and suggest a tune-up." }, primary: true }],
        }),
      },
    },
    {
      slot: "medium-2",
      size: "medium",
      module: "recent_chats",
      name: "Recent Chats",
      purpose: "",
      refreshSecs: null,
      tile: { kind: "recentChats", chats: [{ id: "shot-chat", title: "Hangout", updated: 2 }, { id: "shot-chat-2", title: "Fan curve", updated: 1 }] },
    },
    {
      slot: "small-1",
      size: "small",
      module: "automations",
      name: "Automations",
      purpose: "",
      refreshSecs: null,
      tile: { kind: "automations", active: 2, next: [{ id: "a1", name: "Morning brief", summary: "daily at 08:00" }] },
    },
    {
      slot: "small-2",
      size: "small",
      module: "pulse",
      name: "Pulse · For You",
      purpose: "",
      refreshSecs: null,
      tile: {
        kind: "pulse",
        post: { id: "shot-post", title: "New GPU driver", summary: "", ago: "2h ago" },
        idea: { id: "i1", title: "I can tidy your downloads", action: "tidy my downloads folder" },
      },
    },
    {
      slot: "small-3",
      size: "small",
      module: "streak",
      name: "Streak",
      purpose: "",
      refreshSecs: null,
      tile: { kind: "streak", xp: XP },
    },
  ],
};

const EMPTY_MEMORY = {
  global: "",
  workspace: "",
  globalPath: "",
  workspacePath: "",
  amr: false,
  writesOn: false,
  rows: [],
};

const DAYS = [
  { days: 30, label: "30 days" },
  { days: 60, label: "60 days" },
  { days: 90, label: "90 days" },
  { days: 180, label: "180 days" },
  { days: 365, label: "365 days" },
  { days: 0, label: "Keep everything" },
];

const EMPTY_PRIVACY = {
  offlineOnly: false,
  localReady: false,
  sent: "0 calls sent off-device this week",
  sentCalls: 0,
  hosts: [],
  encrypted: true,
  encryptedNote: "Personal memory and the logs are sealed with your keyring.",
  dataFolder: "/home/user/.config/GrokHub",
  filesFolder: "/home/user/Documents/Loadout",
  canOpenFolder: true,
};

const EMPTY_BACKGROUND = {
  runner: "Here a job runs when you choose Run now.",
  nextDue: null,
  quiet: "No quiet hours",
  budget: "Daily token budget: Off",
  rows: [],
};

const SKILLS = {
  rows: [
    { name: "tidy", description: "Tidy the repo", trigger: "Use the tidy skill.", source: "user", folder: "/home/user/.config/GrokHub/skills/tidy", on: true },
    { name: "summarize", description: "Summarize a file", trigger: "Use the summarize skill.", source: "builtin", folder: "", on: false },
  ],
};

const MEMORY_FILES = {
  soul: "# Soul\n\nCalm and direct.\n",
  user: "# User\n\nPlays ranked on weekends.\n",
  memory: "# Memory\n\n- Prefers short answers.\n",
  soulPath: "/home/user/.config/GrokHub/SOUL.md",
  userPath: "/home/user/.config/GrokHub/USER.md",
  memoryPath: "/home/user/.config/GrokHub/MEMORY.md",
  soulPrev: false,
  userPrev: false,
  memoryPrev: false,
  rawLines: [],
  rawTidyDone: true,
};

const EMPTY_PERSONALITY = {
  presets: [
    {
      id: "default",
      name: "Default",
      description: "Brief, warm and plain.",
      sample: "",
      toneRules: "",
      emoji: "none",
      verbosity: "normal",
      explicit: false,
      custom: false,
    },
  ],
  current: "default",
  problems: [],
  explicit: false,
  ageCheck: false,
  heat: "clean",
  dynamicChat: false,
};

const EMPTY_PLAYBOOKS = { rows: [], problems: [] };
const EMPTY_PERMISSIONS = { rules: [], imported: [], grants: [] };
const EMPTY_INTERESTS = { topics: [], note: "" };
const EMPTY_PLUGINS = { rows: [], marketUrl: "", note: "" };
const EMPTY_WEB = { on: false, code: "", links: [], fingerprint: "" };
const EMPTY_LEARNED = {
  rows: [],
  singleChips: 0,
  paused: false,
  trust: {
    dials: [],
    goals: [],
    now: { quiet: false, busy: false, halted: false, gaming: false, onBattery: false },
    askingFirst: false,
    leftToday: 5,
    asks: [],
    decisions: [],
  },
};
const EMPTY_AI_MODELS = {
  providers: [],
  spend: {
    weeklyCapUsd: 0,
    priceCeilingUsdPerM: 15,
    caps: [0, 5, 10, 25, 50, 100],
    ceilings: [5, 15, 30, 60],
  },
  premium: [],
  table: [],
  tableBuiltAt: 0,
  locked: "",
  status: "",
};
const EMPTY_LOCAL_AI = {
  config: {
    mode: "auto",
    pauseForGames: true,
    useIdleTime: true,
    throttleOnBattery: true,
    manual: {
      backend: "auto",
      ngl: null,
      ctx: null,
      threads: null,
      kv: "auto",
      idleMinutes: 10,
      caps: { rampUp: 90, normal: 80, throttle: 50 },
      neverUnload: false,
      allowApps: [],
      denyApps: [],
    },
    localModel: { selected: "" },
    noticeSeen: [],
  },
  status: {
    tier: "T2",
    model: "",
    state: "Normal",
    reason: "normal use",
    tokS: "",
    memory: "",
  },
  picker: { models: [], onThisPc: [], selected: "" },
  limits: [],
  maxThreads: 8,
  engine: {
    state: "missing",
    version: "b0",
    sizeLabel: "",
    hint: "Local engine is off in the harness.",
    button: "",
    progress: null,
    error: null,
  },
  draftOffer: null,
  draftOffers: [],
  draftProgress: null,
  running: {
    lines: ["Running now: nothing. The model loads with the next reply."],
    loaded: false,
    keptLoaded: false,
    dynamic: true,
    dynamicHint: "On: each task runs on the model on disk that's best at it.",
  },
};

const EMPTY_PULSE_SOURCES = {
  dailyDrop: {
    enabled: false,
    hackerNews: false,
    reddit: false,
    webSearch: false,
    github: false,
    youtube: false,
    rss: false,
  },
  askOffersEnabled: false,
  hobbyPaths: { tips: false, projects: false },
  releaseRadarOn: false,
};
const EMPTY_PULSE_AUTO = {
  fixes: [],
  reminders: false,
  pause: {
    on: false,
    label: "Pause an automation that keeps failing",
    note: "Changes an app setting, not your computer. Only after 5 failed runs in a row. A card says what it did, with Undo.",
    held: null,
  },
};
const EMPTY_STEAM_PULSE = {
  achievementsOn: false,
  wishlistOn: false,
  threshold: "",
  thresholds: [],
  hasId: false,
  canUseLocal: false,
  achNote: "",
  wishNote: "",
  achPrivate: false,
  wishPrivate: false,
};
const EMPTY_KNOW_ME = { on: false, signals: [], lastScan: null, tags: [] };
const EMPTY_KNOW_ME_ANSWER = { view: EMPTY_KNOW_ME, status: "" };

const EMPTY_CODE_SHIP = {
  mode: "ask",
  autoBase: "beta",
  hosts: { github: true, other: false },
  extraProtected: ["main"],
  activityKeep: 2000,
};
const EMPTY_CODE_TEST = { command: null };
const EMPTY_CODE_DEPLOY = {
  deployEnabled: false,
  command: "",
  needsConfirm: true,
  remembered: null,
  mode: "ask",
  blocked: "Deploy is off for this project. Turn it on in Settings → Code.",
};
const EMPTY_CODE_SECRETS = { names: [] };
const EMPTY_CODE_REPO = {
  workspace: "/home/user/GrokHub",
  isRepo: true,
  branch: "beta",
  upstream: "origin/beta",
  ahead: 0,
  behind: 0,
  files: [],
  more: false,
};
const EMPTY_CODE_PROJECTS = { bound: "/home/user/GrokHub", projects: [] };
const EMPTY_CODE_TEST_PLAN = {
  command: "cargo test --workspace --locked",
  needsConfirm: false,
  remembered: "cargo test --workspace --locked",
};
const EMPTY_MODEL_LIBRARY = {
  tabs: [
    { label: "Chat", rows: [], empty: "No chat models in the harness.", all: null },
    { label: "Drafts", rows: [], empty: null, all: null },
    { label: "Image", rows: [], empty: "No image models yet.", all: null },
    { label: "Voice", rows: [], empty: "No voice models yet.", all: null },
    { label: "Other", rows: [], empty: "Nothing here yet.", all: null },
  ],
  lineup: null,
  everything: null,
  status: { job: null, queued: 0, message: null, errors: {}, running: false },
};

/** Browser init script: installs `__TAURI_INTERNALS__` for one theme. */
export function initScript(theme = "ops-dark") {
  const payload = JSON.stringify({
    theme,
    settings: settings(theme),
    caps: capabilities(),
    board: EMPTY_BOARD,
    pulse: EMPTY_PULSE,
    chat: EMPTY_TRANSCRIPT,
    tab: EMPTY_TAB,
    tab2: SECOND_TAB,
    chips: QUICK_CHIPS,
    home: HOME,
    behavior: BEHAVIOR,
    memory: EMPTY_MEMORY,
    privacy: EMPTY_PRIVACY,
    background: EMPTY_BACKGROUND,
    skills: SKILLS,
    memoryFiles: MEMORY_FILES,
    xp: XP,
    days: DAYS,
    personality: EMPTY_PERSONALITY,
    playbooks: EMPTY_PLAYBOOKS,
    permissions: EMPTY_PERMISSIONS,
    interests: EMPTY_INTERESTS,
    plugins: EMPTY_PLUGINS,
    web: EMPTY_WEB,
    learned: EMPTY_LEARNED,
    aiModels: EMPTY_AI_MODELS,
    localAi: EMPTY_LOCAL_AI,
    pulseSources: EMPTY_PULSE_SOURCES,
    pulseAuto: EMPTY_PULSE_AUTO,
    steamPulse: EMPTY_STEAM_PULSE,
    knowMe: EMPTY_KNOW_ME,
    knowMeAnswer: EMPTY_KNOW_ME_ANSWER,
    codeShip: EMPTY_CODE_SHIP,
    codeTest: EMPTY_CODE_TEST,
    codeDeploy: EMPTY_CODE_DEPLOY,
    codeSecrets: EMPTY_CODE_SECRETS,
    modelLibrary: EMPTY_MODEL_LIBRARY,
    codeRepo: EMPTY_CODE_REPO,
    codeProjects: EMPTY_CODE_PROJECTS,
    codeTestPlan: EMPTY_CODE_TEST_PLAN,
  });
  return `(() => {
    const fix = ${payload};
    const emptyList = [];
    const emptyObj = {};
    function reply(cmd, args) {
      switch (cmd) {
        case "plugin:event|listen":
          return 1;
        case "plugin:event|unlisten":
        case "plugin:event|emit":
        case "plugin:event|emit-to":
          return null;
        case "get_settings":
          return { ...fix.settings, theme: fix.theme };
        case "set_theme": {
          fix.theme = (args && args.theme) || fix.theme;
          return { ...fix.settings, theme: fix.theme };
        }
        case "save_settings":
          return { ...fix.settings, theme: fix.theme };
        case "get_capabilities":
          return fix.caps;
        case "get_behavior":
        case "save_behavior":
          return fix.behavior;
        case "get_halt_keys":
          return { global: false };
        case "get_autostart":
        case "set_autostart":
          return false;
        case "get_privacy":
          return fix.privacy;
        case "get_background":
          return fix.background;
        case "get_memory":
          return fix.memory;
        case "get_memory_retention":
        case "set_memory_retention":
        case "get_chat_archive":
        case "set_chat_archive":
          return { days: 90, label: "90 days", choices: fix.days };
        case "get_game_watch":
        case "set_game_watch":
          return { on: false, note: "" };
        case "get_workboard_tidy":
        case "set_workboard_tidy":
          return { autoArchiveDays: 30, autoDeleteDays: 0 };
        case "get_screenshot_offer":
        case "set_screenshot_explain":
        case "dismiss_screenshot_offer":
          return { enabled: false, offer: null };
        case "get_personalities":
        case "set_personality":
        case "set_explicit_personalities":
        case "confirm_adult":
        case "set_quip_heat":
        case "set_dynamic_chat":
        case "save_personality":
        case "delete_personality":
          return fix.personality;
        case "get_playbooks":
        case "reset_playbook":
        case "delete_playbook":
          return fix.playbooks;
        case "get_permissions":
        case "add_permission_rule":
        case "remove_permission_rule":
        case "forget_grant":
          return fix.permissions;
        case "get_interests":
        case "add_interest":
        case "interest_act":
          return fix.interests;
        case "get_plugins":
        case "plugin_act":
          return fix.plugins;
        case "get_web_access":
        case "set_web_access":
          return fix.web;
        case "get_learned":
        case "learned_act":
          return fix.learned;
        case "get_ai_models":
        case "answer_ai_models_card":
        case "remove_ai_provider":
        case "save_ai_spend":
        case "revoke_ai_premium":
          return fix.aiModels;
        case "get_local_ai":
        case "save_local_ai":
        case "reset_local_ai":
        case "set_local_keep_loaded":
        case "set_local_dynamic":
        case "unload_local_model":
          return fix.localAi;
        case "get_pulse_sources":
          return fix.pulseSources;
        case "get_pulse_auto":
        case "set_pulse_auto_fix":
        case "set_proactive_reminders":
        case "set_pulse_auto_pause":
          return fix.pulseAuto;
        case "get_steam_pulse":
        case "set_steam_achievements":
        case "set_steam_wishlist":
        case "set_steam_wishlist_threshold":
        case "save_steam_id":
        case "forget_steam_id":
        case "steam_check_now":
          return fix.steamPulse;
        case "get_know_me":
          return fix.knowMe;
        case "set_know_me_on":
        case "set_know_me_signal":
        case "know_me_scan":
        case "know_me_delete":
        case "know_me_wipe":
          return fix.knowMeAnswer;
        case "get_workboard":
        case "list_workboard":
          return fix.board;
        case "get_pulse":
        case "pulse_page":
        case "list_pulse":
          return fix.pulse;
        case "list_chats":
          return [{ id: "shot-chat", title: "Hangout", updated: 0 }];
        case "list_chat_tabs":
          return [{ ...fix.tab }, { ...fix.tab2 }];
        case "list_archived_chats":
        case "list_deleted_chats":
          return emptyList;
        case "get_start_chat":
        case "load_chat":
          return fix.chat;
        case "chat_tab_switch":
        case "chat_tab_archive":
        case "chat_tab_unarchive":
        case "chat_tab_soft_delete":
        case "chat_tab_restore_deleted":
          return { ...fix.tab, id: (args && args.id) || fix.tab.id };
        case "chat_tab_pin":
          return false;
        case "episode_state":
          return null;
        case "episode_armed":
        case "episode_pause":
          return false;
        case "episode_answer":
        case "episode_stop":
        case "episode_halt":
        case "episode_dismiss":
          return null;
        case "get_hangout_mode":
          return "type";
        case "set_hangout_mode":
          return (args && args.mode) || "type";
        case "composer_ghost_suggest":
          return "";
        case "auto_setup":
          // caps.autoSetup is false; still answer so applyLook does not see null.
          return {
            firstRun: false,
            textSize: 100,
            reduceMotion: false,
            theme: fix.theme,
            question: null,
            summary: false,
            made: [],
            offered: [],
            note: null,
            rows: [],
            said: null,
          };
        case "list_automations":
        case "list_connectors":
        case "list_skills":
        case "list_agents":
        case "list_projects":
        case "list_repos":
        case "list_code_repos":
        case "decision_inbox":
        case "helper_questions":
        case "plugin_list":
        case "list_plugins":
        case "list_notices":
        case "get_notices":
        case "list_playbooks":
        case "list_personalities":
        case "list_permissions":
        case "list_learned":
        case "list_local_models":
        case "list_ai_models":
        case "list_web_hosts":
        case "list_labs":
        case "list_interests":
        case "list_pulse_sources":
        case "list_steam_sources":
        case "list_devices":
        case "list_library":
        case "list_mods":
          return emptyList;
        case "quick_chips":
          return fix.chips;
        case "get_home":
        case "home_view":
          return fix.home;
        case "get_code":
        case "code_page":
          return { repos: [], sessions: [], note: "" };
        case "code_repo":
          return fix.codeRepo;
        case "code_projects":
        case "code_project_open":
        case "code_project_new":
          return fix.codeProjects;
        case "code_project_pick":
          return null;
        case "code_builtins":
          return emptyList;
        case "code_test_plan":
          return fix.codeTestPlan;
        case "code_session_bind":
          return fix.codeRepo.workspace;
        case "code_session_lead":
          return false;
        case "code_session_left":
          return null;
        case "code_diff":
          return { path: "", hunks: [], binary: false };
        case "get_code_ship":
        case "save_code_ship":
          return fix.codeShip;
        case "code_test_remembered":
        case "code_test_forget":
          return fix.codeTest;
        case "code_deploy_plan":
        case "code_deploy_forget":
          return fix.codeDeploy;
        case "code_deploy_save":
          return fix.codeDeploy;
        case "code_secret_list":
        case "code_secret_set":
        case "code_secret_delete":
          return fix.codeSecrets;
        case "get_model_library":
        case "download_models":
        case "remove_library_model":
          return fix.modelLibrary;
        case "pause_model_downloads":
          return null;
        case "get_personality":
          return { id: "default", name: "Default", note: "" };
        case "get_defaults":
          return { model: "grok", temperature: 0.7 };
        case "get_skills":
          return fix.skills;
        case "get_memory_files":
          return fix.memoryFiles;
        case "get_battlenet":
          return { found: false, games: [] };
        case "nyx_look":
          // The companion docked beside the chat, as on a desktop with her art.
          return { set: "hanging-out", poses: ["brand/nyx-hanging-out.jpg"], explicit: false, mood: "hanging-out" };
        case "get_mission":
          // The same mission Home's Objectives tile shows, for Chat's side column.
          return { mission: fix.home.slots.find((s) => s.tile.kind === "objectives")?.tile.mission ?? null, xp: fix.xp };
        case "get_about":
          return { version: fix.settings.version, channel: "beta" };
        default:
          // Prefer empty list for list_* / *_list; null otherwise so optional
          // panels (episode, offers) stay hidden instead of half-shaped.
          if (/^list_/.test(cmd) || /_list$/.test(cmd) || /_inbox$/.test(cmd)) {
            return emptyList;
          }
          return null;
      }
    }
    const callbacks = new Map();
    let next = 1;
    window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
      unregisterListener: () => {},
    };
    window.__TAURI_INTERNALS__ = {
      invoke: async (cmd, args) => reply(cmd, args || {}),
      transformCallback: (cb, once) => {
        const id = next++;
        callbacks.set(id, { cb, once: !!once });
        return id;
      },
      unregisterCallback: (id) => callbacks.delete(id),
      runCallback: (id, data) => {
        const row = callbacks.get(id);
        if (!row) return;
        row.cb(data);
        if (row.once) callbacks.delete(id);
      },
      metadata: {
        currentWindow: { label: "main" },
        currentWebview: { label: "main" },
      },
    };
    // Seed the theme cache so applyCachedTheme paints before get_settings.
    // Do not touch documentElement here — it can be null during init scripts.
    try { localStorage.setItem("shell.theme", fix.theme); } catch (_) {}
  })();`;
}
