use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScreenSize {
    pub w: i32,
    pub h: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ComputerOp {
    Click { x: i32, y: i32 },
    DoubleClick { x: i32, y: i32 },
    Move { x: i32, y: i32 },
    Type { text: String },
    Key { name: String },
    Scroll { dy: i32 },
    Act { name: String },
    WaitFor { title: Option<String> },
    Tab { action: TabAction, query: String },
    Cursor,
    MoveMonitor {
        name: String,
        x: Option<i32>,
        y: Option<i32>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabAction {
    List,
    Close,
    Focus,
    New,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recipe {
    pub screen: Option<ScreenSize>,
    pub ops: Vec<ComputerOp>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandsBackend {
    Xdotool,
    Ydotool,
}

/// Wayland prefers ydotool. X11 and missing-ydotool fall back to xdotool.
pub fn pick_hands_backend(wayland: bool, has_ydotool: bool, has_xdotool: bool) -> Option<HandsBackend> {
    if wayland && has_ydotool {
        Some(HandsBackend::Ydotool)
    } else if has_xdotool {
        Some(HandsBackend::Xdotool)
    } else if has_ydotool {
        Some(HandsBackend::Ydotool)
    } else {
        None
    }
}

pub fn hands_backend_name(backend: Option<HandsBackend>) -> &'static str {
    match backend {
        Some(HandsBackend::Ydotool) => "ydotool",
        Some(HandsBackend::Xdotool) => "xdotool",
        None => "missing",
    }
}

/// Window-name search for `COMPUTER_CMD: act` is xdotool-only. ydotool cannot look up titles.
pub fn act_window_search_bin(has_xdotool: bool) -> Option<&'static str> {
    has_xdotool.then_some("xdotool")
}

/// PATH lookup that does not spawn `which` (missing on some GUI PATHs).
pub fn bin_on_path(name: &str, path_env: &str, extra_dirs: &[&str]) -> bool {
    let name = name.trim();
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains('\0') {
        return false;
    }
    path_env
        .split(':')
        .chain(extra_dirs.iter().copied())
        .filter(|d| !d.is_empty())
        .any(|d| std::path::Path::new(d).join(name).is_file())
}

pub fn default_bin_extra_dirs(home: &str) -> Vec<String> {
    let mut dirs = vec![
        "/usr/bin".into(),
        "/usr/local/bin".into(),
        "/bin".into(),
    ];
    let home = home.trim();
    if !home.is_empty() {
        dirs.push(format!("{home}/.local/bin"));
    }
    dirs
}

/// Empty ydotool key maps must fail closed — not look like a successful press.
pub fn empty_hands_steps_error(op: &ComputerOp, steps: &[Vec<String>]) -> Option<String> {
    if !steps.is_empty() {
        return None;
    }
    match op {
        ComputerOp::Key { name } => Some(format!("unknown key {name}")),
        ComputerOp::Scroll { dy } if *dy == 0 => None,
        ComputerOp::Tab { .. }
        | ComputerOp::Act { .. }
        | ComputerOp::WaitFor { .. }
        | ComputerOp::Cursor
        | ComputerOp::MoveMonitor { .. } => None,
        _ => Some("empty hands step".into()),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayOp {
    Reshoot,
    Op(ComputerOp),
}

pub fn parse_screen(s: &str) -> Option<ScreenSize> {
    let s = s.trim().strip_prefix("screen=").unwrap_or(s.trim());
    let (w, h) = s.split_once('x')?;
    Some(ScreenSize {
        w: w.trim().parse().ok()?,
        h: h.trim().parse().ok()?,
    })
}

pub fn needs_reshoot(recipe: Option<ScreenSize>, current: Option<ScreenSize>) -> bool {
    match (recipe, current) {
        (Some(r), Some(c)) => r != c,
        (Some(_), None) => true,
        _ => false,
    }
}

pub fn parse_computer_op(line: &str) -> Option<ComputerOp> {
    let rest = line
        .trim()
        .strip_prefix("COMPUTER_CMD:")
        .or_else(|| line.trim().strip_prefix("COMPUTER_CMD"))?;
    let rest = rest.trim().trim_start_matches(':').trim();
    if rest.is_empty() {
        return None;
    }
    let mut bits = rest.split_whitespace();
    let op = bits.next()?.to_ascii_lowercase();
    match op.as_str() {
        "click" => {
            let x = bits.next()?.parse().ok()?;
            let y = bits.next()?.parse().ok()?;
            Some(ComputerOp::Click { x, y })
        }
        "dblclick" | "doubleclick" | "double-click" => {
            let x = bits.next()?.parse().ok()?;
            let y = bits.next()?.parse().ok()?;
            Some(ComputerOp::DoubleClick { x, y })
        }
        "move" | "mousemove" => {
            let a = bits.next()?;
            if a.eq_ignore_ascii_case("monitor") {
                let name = bits.next()?.to_string();
                if name.is_empty() {
                    return None;
                }
                match (bits.next(), bits.next()) {
                    (None, None) => Some(ComputerOp::MoveMonitor {
                        name,
                        x: None,
                        y: None,
                    }),
                    (Some(xs), Some(ys)) => {
                        let x = xs.parse().ok()?;
                        let y = ys.parse().ok()?;
                        Some(ComputerOp::MoveMonitor {
                            name,
                            x: Some(x),
                            y: Some(y),
                        })
                    }
                    _ => None,
                }
            } else {
                let x = a.parse().ok()?;
                let y = bits.next()?.parse().ok()?;
                Some(ComputerOp::Move { x, y })
            }
        }
        "cursor" | "getmouselocation" | "mouse" => Some(ComputerOp::Cursor),
        "type" => {
            let text = bits.collect::<Vec<_>>().join(" ");
            if text.is_empty() {
                None
            } else {
                Some(ComputerOp::Type { text })
            }
        }
        "key" => {
            let name = bits.collect::<Vec<_>>().join(" ");
            if name.is_empty() {
                None
            } else {
                Some(ComputerOp::Key { name })
            }
        }
        "scroll" => {
            let dy = bits.next()?.parse().ok()?;
            Some(ComputerOp::Scroll { dy })
        }
        "act" => {
            let name = bits.collect::<Vec<_>>().join(" ");
            if name.is_empty() {
                None
            } else {
                Some(ComputerOp::Act { name })
            }
        }
        "wait_for" | "wait-for" => {
            let rest = bits.collect::<Vec<_>>().join(" ");
            let title = rest
                .strip_prefix("title=")
                .or_else(|| rest.strip_prefix("title:"))
                .map(|s| s.to_string())
                .filter(|s| !s.is_empty());
            Some(ComputerOp::WaitFor { title })
        }
        "tab" => {
            let action = bits.next()?.to_ascii_lowercase();
            match action.as_str() {
                "list" => Some(ComputerOp::Tab {
                    action: TabAction::List,
                    query: String::new(),
                }),
                "close" => {
                    let query = bits.collect::<Vec<_>>().join(" ");
                    if query.is_empty() {
                        None
                    } else {
                        Some(ComputerOp::Tab {
                            action: TabAction::Close,
                            query,
                        })
                    }
                }
                "focus" => {
                    let query = bits.collect::<Vec<_>>().join(" ");
                    if query.is_empty() {
                        None
                    } else {
                        Some(ComputerOp::Tab {
                            action: TabAction::Focus,
                            query,
                        })
                    }
                }
                "new" | "open" => Some(ComputerOp::Tab {
                    action: TabAction::New,
                    query: bits.collect::<Vec<_>>().join(" "),
                }),
                _ => None,
            }
        }
        _ => None,
    }
}

pub fn parse_recipe(text: &str) -> Option<Recipe> {
    let mut screen = None;
    let mut ops = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("RECIPE:") {
            screen = parse_screen(rest);
            continue;
        }
        if let Some(op) = parse_computer_op(t) {
            ops.push(op);
        }
    }
    if screen.is_none() && ops.is_empty() {
        None
    } else {
        Some(Recipe { screen, ops })
    }
}

pub fn replay_ops(recipe: &Recipe, current: Option<ScreenSize>) -> Vec<ReplayOp> {
    let reshoot = needs_reshoot(recipe.screen, current);
    let mut out = Vec::new();
    if reshoot {
        out.push(ReplayOp::Reshoot);
    }
    for op in &recipe.ops {
        match op {
            ComputerOp::Click { .. } | ComputerOp::DoubleClick { .. } | ComputerOp::Move { .. }
                if reshoot => {}
            other => out.push(ReplayOp::Op(other.clone())),
        }
    }
    out
}

pub fn screen_from_extents(max_x: i32, max_y: i32) -> Option<ScreenSize> {
    if max_x > 0 && max_y > 0 {
        Some(ScreenSize { w: max_x, h: max_y })
    } else {
        None
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComputerDrive {
    Xdotool(Vec<Vec<String>>),
    Ydotool(Vec<Vec<String>>),
    Act(String),
    WaitFor(Option<String>),
    Tab(TabAction, String),
    Cursor,
    MoveMonitor {
        name: String,
        x: Option<i32>,
        y: Option<i32>,
    },
}

pub fn computer_drive(op: &ComputerOp) -> ComputerDrive {
    computer_drive_for(HandsBackend::Xdotool, op)
}

pub fn computer_drive_for(backend: HandsBackend, op: &ComputerOp) -> ComputerDrive {
    match op {
        ComputerOp::Act { name } => ComputerDrive::Act(name.clone()),
        ComputerOp::WaitFor { title } => ComputerDrive::WaitFor(title.clone()),
        ComputerOp::Tab { action, query } => ComputerDrive::Tab(*action, query.clone()),
        ComputerOp::Cursor => ComputerDrive::Cursor,
        ComputerOp::MoveMonitor { name, x, y } => ComputerDrive::MoveMonitor {
            name: name.clone(),
            x: *x,
            y: *y,
        },
        other => match backend {
            HandsBackend::Xdotool => ComputerDrive::Xdotool(xdotool_steps(other)),
            HandsBackend::Ydotool => ComputerDrive::Ydotool(ydotool_steps(other)),
        },
    }
}

fn xdotool_steps(op: &ComputerOp) -> Vec<Vec<String>> {
    match op {
        ComputerOp::Click { x, y } => vec![
            vec!["mousemove".into(), x.to_string(), y.to_string()],
            vec!["click".into(), "--clearmodifiers".into(), "1".into()],
        ],
        ComputerOp::DoubleClick { x, y } => vec![
            vec!["mousemove".into(), x.to_string(), y.to_string()],
            vec![
                "click".into(),
                "--clearmodifiers".into(),
                "--repeat".into(),
                "2".into(),
                "1".into(),
            ],
        ],
        ComputerOp::Move { x, y } => {
            vec![vec!["mousemove".into(), x.to_string(), y.to_string()]]
        }
        ComputerOp::Type { text } => vec![vec![
            "type".into(),
            "--clearmodifiers".into(),
            "--".into(),
            text.clone(),
        ]],
        ComputerOp::Key { name } => vec![vec![
            "key".into(),
            "--clearmodifiers".into(),
            name.clone(),
        ]],
        ComputerOp::Scroll { dy } => {
            if *dy == 0 {
                vec![]
            } else {
                let btn = if *dy < 0 { "5" } else { "4" };
                vec![vec![
                    "click".into(),
                    "--clearmodifiers".into(),
                    "--repeat".into(),
                    dy.unsigned_abs().to_string(),
                    btn.into(),
                ]]
            }
        }
        ComputerOp::Act { .. }
        | ComputerOp::WaitFor { .. }
        | ComputerOp::Tab { .. }
        | ComputerOp::Cursor
        | ComputerOp::MoveMonitor { .. } => vec![],
    }
}

fn ydotool_steps(op: &ComputerOp) -> Vec<Vec<String>> {
    match op {
        ComputerOp::Click { x, y } => vec![
            vec!["mousemove".into(), "--absolute".into(), x.to_string(), y.to_string()],
            vec!["click".into(), "0xC0".into()],
        ],
        ComputerOp::DoubleClick { x, y } => vec![
            vec!["mousemove".into(), "--absolute".into(), x.to_string(), y.to_string()],
            vec!["click".into(), "--repeat".into(), "2".into(), "0xC0".into()],
        ],
        ComputerOp::Move { x, y } => {
            vec![vec!["mousemove".into(), "--absolute".into(), x.to_string(), y.to_string()]]
        }
        ComputerOp::Type { text } => vec![vec!["type".into(), "--".into(), text.clone()]],
        ComputerOp::Key { name } => match ydotool_key_tokens(name) {
            Some(tokens) if !tokens.is_empty() => {
                let mut step = vec!["key".into()];
                step.extend(tokens);
                vec![step]
            }
            _ => vec![],
        }
        ComputerOp::Scroll { dy } => {
            if *dy == 0 {
                vec![]
            } else {
                vec![vec![
                    "mousemove".into(),
                    "--wheel".into(),
                    "0".into(),
                    dy.to_string(),
                ]]
            }
        }
        ComputerOp::Act { .. }
        | ComputerOp::WaitFor { .. }
        | ComputerOp::Tab { .. }
        | ComputerOp::Cursor
        | ComputerOp::MoveMonitor { .. } => vec![],
    }
}

pub fn relative_move_steps(backend: HandsBackend, dx: i32, dy: i32) -> Vec<Vec<String>> {
    if dx == 0 && dy == 0 {
        return vec![];
    }
    match backend {
        HandsBackend::Ydotool => vec![vec![
            "mousemove".into(),
            dx.to_string(),
            dy.to_string(),
        ]],
        HandsBackend::Xdotool => vec![vec![
            "mousemove_relative".into(),
            "--".into(),
            dx.to_string(),
            dy.to_string(),
        ]],
    }
}

fn ydotool_key_tokens(name: &str) -> Option<Vec<String>> {
    let parts: Vec<&str> = name
        .split('+')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if parts.is_empty() {
        return None;
    }
    let codes: Option<Vec<u16>> = parts.iter().map(|p| linux_keycode(p)).collect();
    let codes = codes?;
    let mut tokens = Vec::new();
    for c in &codes {
        tokens.push(format!("{c}:1"));
    }
    for c in codes.iter().rev() {
        tokens.push(format!("{c}:0"));
    }
    Some(tokens)
}

fn linux_keycode(name: &str) -> Option<u16> {
    match name.to_ascii_lowercase().as_str() {
        "return" | "enter" | "kp_enter" => Some(28),
        "esc" | "escape" => Some(1),
        "tab" => Some(15),
        "space" | "spacebar" => Some(57),
        "backspace" => Some(14),
        "ctrl" | "control" | "control_l" | "ctrl_l" => Some(29),
        "shift" | "shift_l" => Some(42),
        "alt" | "alt_l" => Some(56),
        "super" | "super_l" | "meta" | "win" => Some(125),
        "up" => Some(103),
        "down" => Some(108),
        "left" => Some(105),
        "right" => Some(106),
        "delete" | "del" => Some(111),
        "home" => Some(102),
        "end" => Some(107),
        "pageup" | "prior" => Some(104),
        "pagedown" | "next" => Some(109),
        "f1" => Some(59),
        "f2" => Some(60),
        "f3" => Some(61),
        "f4" => Some(62),
        "f5" => Some(63),
        "a" => Some(30),
        "s" => Some(31),
        "d" => Some(32),
        "c" => Some(46),
        "v" => Some(47),
        "x" => Some(45),
        "z" => Some(44),
        "q" => Some(16),
        "w" => Some(17),
        other if other.len() == 1 => {
            let ch = other.chars().next()?;
            match ch {
                '0' => Some(11),
                '1'..='9' => Some(2 + (ch as u16 - b'1' as u16)),
                'b' => Some(48),
                'e' => Some(18),
                'f' => Some(33),
                'g' => Some(34),
                'h' => Some(35),
                'i' => Some(23),
                'j' => Some(36),
                'k' => Some(37),
                'l' => Some(38),
                'm' => Some(50),
                'n' => Some(49),
                'o' => Some(24),
                'p' => Some(25),
                'r' => Some(19),
                't' => Some(20),
                'u' => Some(22),
                'y' => Some(21),
                _ => None,
            }
        }
        _ => None,
    }
}

pub fn computer_cmd_line(op: &ComputerOp) -> String {
    match op {
        ComputerOp::Click { x, y } => format!("COMPUTER_CMD: click {x} {y}"),
        ComputerOp::DoubleClick { x, y } => format!("COMPUTER_CMD: dblclick {x} {y}"),
        ComputerOp::Move { x, y } => format!("COMPUTER_CMD: move {x} {y}"),
        ComputerOp::Type { text } => format!("COMPUTER_CMD: type {text}"),
        ComputerOp::Key { name } => format!("COMPUTER_CMD: key {name}"),
        ComputerOp::Scroll { dy } => format!("COMPUTER_CMD: scroll {dy}"),
        ComputerOp::Act { name } => format!("COMPUTER_CMD: act {name}"),
        ComputerOp::WaitFor { title } => match title {
            Some(t) => format!("COMPUTER_CMD: wait_for title={t}"),
            None => "COMPUTER_CMD: wait_for".into(),
        },
        ComputerOp::Tab { action, query } => match action {
            TabAction::List => "COMPUTER_CMD: tab list".into(),
            TabAction::Close => format!("COMPUTER_CMD: tab close {query}"),
            TabAction::Focus => format!("COMPUTER_CMD: tab focus {query}"),
            TabAction::New => {
                if query.is_empty() {
                    "COMPUTER_CMD: tab new".into()
                } else {
                    format!("COMPUTER_CMD: tab new {query}")
                }
            }
        },
        ComputerOp::Cursor => "COMPUTER_CMD: cursor".into(),
        ComputerOp::MoveMonitor { name, x, y } => match (x, y) {
            (Some(x), Some(y)) => format!("COMPUTER_CMD: move monitor {name} {x} {y}"),
            _ => format!("COMPUTER_CMD: move monitor {name}"),
        },
    }
}

pub fn parse_computer_cmd_loose(cmd: &str) -> Option<ComputerOp> {
    let t = cmd.trim();
    parse_computer_op(t).or_else(|| parse_computer_op(&format!("COMPUTER_CMD: {t}")))
}

pub fn extract_computer_ops(text: &str) -> Vec<ComputerOp> {
    text.lines().filter_map(parse_computer_op).collect()
}

pub fn recipe_from_cmds(cmds: &[String], screen: Option<ScreenSize>) -> Option<Recipe> {
    let ops: Vec<ComputerOp> = cmds.iter().filter_map(|c| parse_computer_op(c)).collect();
    if ops.is_empty() {
        None
    } else {
        Some(Recipe { screen, ops })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecipeDoc {
    pub id: String,
    pub screen: Option<ScreenSize>,
    pub ops: Vec<ComputerOp>,
}

pub fn recipe_to_json(id: &str, recipe: &Recipe) -> Result<String, String> {
    let doc = RecipeDoc {
        id: id.to_string(),
        screen: recipe.screen,
        ops: recipe.ops.clone(),
    };
    serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())
}

pub fn recipe_from_json(raw: &str) -> Result<(String, Recipe), String> {
    let doc: RecipeDoc = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    Ok((
        doc.id,
        Recipe {
            screen: doc.screen,
            ops: doc.ops,
        },
    ))
}

pub fn hands_protocol() -> &'static str {
    "You run unsandboxed on this Linux desktop. The cabin has full host and GUI hands when the user asks.\n\
     HOST_CMD: <shell> — runs via bash -lc immediately. The cabin drives; there is no approve step.\n\
     COMPUTER_CMD: click X Y\n\
     COMPUTER_CMD: dblclick X Y\n\
     COMPUTER_CMD: move X Y\n\
     COMPUTER_CMD: type <text>\n\
     COMPUTER_CMD: key <name>\n\
     COMPUTER_CMD: scroll <dy>\n\
     COMPUTER_CMD: act <accessible-name>\n\
     COMPUTER_CMD: wait_for title=<window>\n\
     COMPUTER_CMD: tab list\n\
     COMPUTER_CMD: tab new [url]\n\
     COMPUTER_CMD: tab close <title-or-url>\n\
     COMPUTER_CMD: tab focus <title-or-url>\n\
     COMPUTER_CMD: cursor\n\
     COMPUTER_CMD: move monitor <name> [x y]\n\
     Prefer act, wait_for, tab list/new/close/focus, and key over guessed JPEG coordinates. After each COMPUTER_CMD hop, look at the new JPEG and Windshield before the next click.\n\
     Coordinates are the global desktop (xrandr). A JPEG may be one output — use Windshield desk / outputs / frame. After move or click, COMPUTER_RESULT cursor X,Y monitor=NAME is the real pointer — treat it as ground truth and correct if it missed. Prefer move monitor <name> over hard-coded 7000-wide numbers. When Windshield says browser: cdp, open/close/focus a tab with tab new / tab close / tab focus. New tab: act the New Tab or + control; if Windshield has no such row, wait_for title=Firefox then key ctrl+t. Otherwise wait_for that window then key ctrl+w to close; do not guess the × from the still.\n\
     Never emit XML or JSON <tool_call> tags. The first reply must include a COMPUTER_CMD line — do not only plan.\n\
     A JPEG is attached when the user asks for hands, cabin eyes, or GUI help (close a tab, Settings, turn this on, how do I, for me). If the thing is on the glass and there is no honest shell, use COMPUTER_CMD — do not invent a CLI.\n\
     Guide-only (just tell me / don't click / walk me through without do it): describe the control from the Windshield; do not emit COMPUTER_CMD unless they then say do it.\n\
     Do-it / I can't / default GUI help: drive, then end with a short how-to the user can repeat (Settings → Bluetooth → the switch).\n\
     Lock/password screens are won'ts — never click them or type into them. Do not read ~/.ssh or /etc/shadow.\n\
     If Windshield says hands: daemon, hands: uinput, or hands: missing — or COMPUTER_RESULT says hands are down (not installed, uinput, or ydotoold) — tell the user how to enable them. Do not pkill, kill, or otherwise terminate apps as a stand-in for mouse or keyboard control."
}

pub fn user_asks_cabin_eyes(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    const NEEDLES: &[&str] = &[
        "cabin eyes",
        "look at my screen",
        "look at the screen",
        "look at this screen",
        "look at my desktop",
        "look at the desktop",
        "what's on my screen",
        "whats on my screen",
        "what's on the screen",
        "whats on the screen",
        "what's on my desktop",
        "whats on my desktop",
        "what do you see",
        "what can you see",
        "see my screen",
        "see the screen",
        "see my desktop",
        "take a screenshot",
        "grab a screenshot",
        "use your eyes",
        "open your eyes",
        "wake your eyes",
        "look at this",
        "what's wrong on",
        "whats wrong on",
    ];
    NEEDLES.iter().any(|n| t.contains(n))
}

pub fn user_asks_takeover(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    const NEEDLES: &[&str] = &[
        "take over",
        "takeover",
        "fix this",
        "fix what's on",
        "fix whats on",
        "help me with this window",
        "this is broken",
        "handle it",
        "drive the desktop",
        "take the wheel",
        "take control",
    ];
    NEEDLES.iter().any(|n| t.contains(n))
}

pub fn user_asks_desktop_hands(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    const NEEDLES: &[&str] = &[
        "click the",
        "click on",
        "double click",
        "double-click",
        "mouse",
        "keyboard",
        "take control",
        "type into",
        "type in the",
        "press enter",
        "hit enter",
        "move the mouse",
        "move the cursor",
        "use the mouse",
        "control the ui",
        "control the screen",
        "desktop hands",
        "take over",
        "takeover",
        "fix this",
        "help me with this window",
        "this is broken",
        "drive the desktop",
        "take the wheel",
    ];
    NEEDLES.iter().any(|n| t.contains(n)) || user_asks_takeover(text)
}

fn has_tab_token(t: &str) -> bool {
    t.split(|c: char| !c.is_ascii_alphanumeric())
        .any(|w| w == "tab" || w == "tabs")
}

fn has_gui_context(t: &str) -> bool {
    t.contains("tab")
        || t.contains("window")
        || t.contains("screen")
        || t.contains("desktop")
        || t.contains("settings")
        || t.contains("bluetooth")
        || t.contains("firefox")
        || t.contains("chrome")
        || t.contains("browser")
        || t.contains("click")
        || t.contains("button")
        || t.contains("toggle")
        || t.contains("switch")
        || t.contains("this")
        || t.contains("that")
}

fn user_asks_do_it(t: &str) -> bool {
    t.contains("for me")
        || t.contains("do it")
        || t.contains("i can't")
        || t.contains("i cant")
        || t.contains("i cannot")
}

/// Everyday GUI help: close a tab, Settings, turn this on, how do I, for me.
pub fn user_asks_gui_help(text: &str) -> bool {
    if user_asks_desktop_hands(text) || user_asks_takeover(text) {
        return true;
    }
    let t = text.to_ascii_lowercase();
    if (t.contains("close") || t.contains("shut"))
        && (t.contains("tab")
            || t.contains("window")
            || t.contains("firefox")
            || t.contains("chrome")
            || t.contains("browser"))
    {
        return true;
    }
    if has_tab_token(&t)
        && (t.contains("new tab")
            || t.contains("open a tab")
            || t.contains("open the tab")
            || t.contains("select a tab")
            || t.contains("select a new tab")
            || t.contains("switch tab")
            || t.contains("switch the tab")
            || t.contains("open")
            || t.contains("select")
            || t.contains("switch"))
    {
        return true;
    }
    if t.contains("settings")
        || t.contains("turn on")
        || t.contains("turn off")
        || t.contains("turn this")
        || t.contains("enable")
        || t.contains("disable")
        || t.contains("toggle")
    {
        return true;
    }
    if (t.contains("for me")
        || t.contains("do it")
        || t.contains("take care of")
        || t.contains("how do i")
        || t.contains("show me")
        || t.contains("walk me through")
        || t.contains("i can't")
        || t.contains("i cant")
        || t.contains("i cannot"))
        && has_gui_context(&t)
    {
        return true;
    }
    false
}

/// Walkthrough only: look, do not click, unless they also said do it / for me.
pub fn user_asks_guide_only(text: &str) -> bool {
    let t = text.to_ascii_lowercase();
    if user_asks_do_it(&t) || user_asks_desktop_hands(text) || user_asks_takeover(text) {
        return false;
    }
    t.contains("just tell me")
        || t.contains("don't click")
        || t.contains("dont click")
        || t.contains("do not click")
        || t.contains("walk me through")
}

/// Eyes for GUI help or a walkthrough. Hands unless the ask is guide-only.
pub fn see_drive_attach(text: &str) -> (bool, bool) {
    if user_asks_guide_only(text) {
        return (true, false);
    }
    if user_asks_gui_help(text) {
        return (true, true);
    }
    (false, false)
}

fn pretty_key(name: &str) -> String {
    name.split('+')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|p| {
            if p.len() == 1 {
                return p.to_ascii_uppercase();
            }
            match p.to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "control_l" | "ctrl_l" => "Ctrl".into(),
                "alt" | "alt_l" => "Alt".into(),
                "shift" | "shift_l" => "Shift".into(),
                "super" | "meta" | "win" => "Super".into(),
                "return" | "enter" | "kp_enter" => "Enter".into(),
                other => {
                    let mut c = other.chars();
                    match c.next() {
                        Some(f) => format!("{}{}", f.to_ascii_uppercase(), c.as_str()),
                        None => String::new(),
                    }
                }
            }
        })
        .collect::<Vec<_>>()
        .join("+")
}

fn label_for_op(op: &ComputerOp) -> String {
    match op {
        ComputerOp::Act { name } => format!("Clicked {name}"),
        ComputerOp::Tab {
            action: TabAction::Close,
            query,
        } => format!("Closed tab {query}"),
        ComputerOp::Tab {
            action: TabAction::Focus,
            query,
        } => format!("Focused tab {query}"),
        ComputerOp::Tab {
            action: TabAction::List,
            ..
        } => "Listed tabs".into(),
        ComputerOp::Tab {
            action: TabAction::New,
            query,
        } => {
            if query.is_empty() {
                "Opened a new tab".into()
            } else {
                format!("Opened tab {query}")
            }
        }
        ComputerOp::Key { name } => format!("Pressed {}", pretty_key(name)),
        ComputerOp::Click { x, y } => format!("Clicked {x},{y}"),
        ComputerOp::DoubleClick { x, y } => format!("Double-clicked {x},{y}"),
        ComputerOp::Move { x, y } => format!("Moved to {x},{y}"),
        ComputerOp::Type { text } => {
            let n = text.chars().count();
            format!("Typed {n} characters")
        }
        ComputerOp::Scroll { dy } => format!("Scrolled {dy}"),
        ComputerOp::WaitFor { title } => match title {
            Some(t) if !t.is_empty() => format!("Waited for {t}"),
            _ => "Waited".into(),
        },
        ComputerOp::Cursor => "Read cursor".into(),
        ComputerOp::MoveMonitor { name, .. } => format!("Moved to monitor {name}"),
    }
}

/// Human Hands chip from a COMPUTER_RESULT / COMPUTER_CMD receipt. Not a dump.
pub fn hands_step_label(receipt: &str) -> Option<String> {
    let t = receipt.trim_start();
    if t.starts_with("HOST_RESULT") || t.starts_with("CONNECTOR_RESULT") || t.starts_with("HOST_DIFF")
    {
        return None;
    }
    if !t.starts_with("COMPUTER_RESULT") && !t.contains("COMPUTER_CMD") {
        return None;
    }
    for line in receipt.lines() {
        let line = line.trim().trim_start_matches('$').trim();
        if let Some(op) = parse_computer_op(line)
            .or_else(|| parse_computer_op(&format!("COMPUTER_CMD: {line}")))
        {
            return Some(label_for_op(&op));
        }
    }
    None
}

/// Attach a room frame only when this turn asked for eyes or hands.
/// The Cabin eyes setting being on is not a trigger.
pub fn should_attach_hands_frame(eyes_turn: bool, hands_turn: bool, has_frame: bool) -> bool {
    has_frame && (eyes_turn || hands_turn)
}

pub fn lock_blocks_hands(titles: &[&str]) -> bool {
    titles.iter().copied().any(crate::hygiene::lockish)
}

/// Wait-for may poll a lock title. Pointer, type, key, and act must not.
pub fn pointer_op_blocked_on_lock(op: &ComputerOp) -> bool {
    !matches!(op, ComputerOp::WaitFor { .. } | ComputerOp::Cursor)
}

pub fn hands_blocked_by_lock(op: &ComputerOp, titles: &[&str]) -> bool {
    pointer_op_blocked_on_lock(op) && lock_blocks_hands(titles)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn act_search_skips_missing_xdotool() {
        assert_eq!(act_window_search_bin(true), Some("xdotool"));
        assert_eq!(
            act_window_search_bin(false),
            None,
            "ydotool-only installs cannot search window titles"
        );
    }

    #[test]
    fn reshoot_skips_coordinates() {
        assert_eq!(
            parse_computer_op("COMPUTER_CMD: act Refresh"),
            Some(ComputerOp::Act {
                name: "Refresh".into()
            })
        );
        assert_eq!(
            parse_computer_op("COMPUTER_CMD: wait_for title=Settings"),
            Some(ComputerOp::WaitFor {
                title: Some("Settings".into())
            })
        );
        let recipe = parse_recipe(
            "RECIPE: screen=1920x1080\nCOMPUTER_CMD: click 10 20\nCOMPUTER_CMD: act Refresh\n",
        )
        .unwrap();
        assert_eq!(recipe.screen, Some(ScreenSize { w: 1920, h: 1080 }));
        assert!(needs_reshoot(recipe.screen, Some(ScreenSize { w: 1280, h: 720 })));
        assert!(!needs_reshoot(recipe.screen, Some(ScreenSize { w: 1920, h: 1080 })));
        let replay = replay_ops(&recipe, Some(ScreenSize { w: 800, h: 600 }));
        assert_eq!(replay[0], ReplayOp::Reshoot);
        assert!(!replay.iter().any(|r| matches!(r, ReplayOp::Op(ComputerOp::Click { .. }))));
        assert!(replay
            .iter()
            .any(|r| matches!(r, ReplayOp::Op(ComputerOp::Act { name }) if name == "Refresh")));
    }

    #[test]
    fn hands_parse_type_key_and_click_argv() {
        assert_eq!(
            parse_computer_op("COMPUTER_CMD: type hello cabin"),
            Some(ComputerOp::Type {
                text: "hello cabin".into()
            })
        );
        assert_eq!(
            parse_computer_op("COMPUTER_CMD: key Return"),
            Some(ComputerOp::Key {
                name: "Return".into()
            })
        );
        assert_eq!(
            parse_computer_op("COMPUTER_CMD: dblclick 40 80"),
            Some(ComputerOp::DoubleClick { x: 40, y: 80 })
        );
        assert_eq!(
            parse_computer_op("COMPUTER_CMD: move 12 34"),
            Some(ComputerOp::Move { x: 12, y: 34 })
        );
        assert_eq!(
            parse_computer_op("COMPUTER_CMD: scroll -3"),
            Some(ComputerOp::Scroll { dy: -3 })
        );
        let click = computer_drive(&ComputerOp::Click { x: 100, y: 200 });
        match click {
            ComputerDrive::Xdotool(steps) => {
                assert_eq!(steps[0], vec!["mousemove", "100", "200"]);
                assert!(!steps.iter().any(|s| s.iter().any(|a| a == "--sync")));
                assert_eq!(steps[1], vec!["click", "--clearmodifiers", "1"]);
            }
            other => panic!("click must be xdotool, got {other:?}"),
        }
        match computer_drive(&ComputerOp::Type {
            text: "hi there".into(),
        }) {
            ComputerDrive::Xdotool(steps) => {
                assert_eq!(
                    steps[0],
                    vec!["type", "--clearmodifiers", "--", "hi there"]
                );
            }
            other => panic!("{other:?}"),
        }
        match computer_drive(&ComputerOp::Key { name: "ctrl+s".into() }) {
            ComputerDrive::Xdotool(steps) => {
                assert_eq!(steps[0], vec!["key", "--clearmodifiers", "ctrl+s"]);
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(
            computer_drive(&ComputerOp::Act { name: "Save".into() }),
            ComputerDrive::Act(n) if n == "Save"
        ));
        assert_eq!(
            computer_cmd_line(&ComputerOp::Click { x: 1, y: 2 }),
            "COMPUTER_CMD: click 1 2"
        );
        assert_eq!(
            parse_computer_op("COMPUTER_CMD: tab list"),
            Some(ComputerOp::Tab {
                action: TabAction::List,
                query: String::new()
            })
        );
        assert_eq!(
            parse_computer_op("COMPUTER_CMD: tab close GitHub"),
            Some(ComputerOp::Tab {
                action: TabAction::Close,
                query: "GitHub".into()
            })
        );
        assert_eq!(
            parse_computer_op("COMPUTER_CMD: tab focus news.ycombinator"),
            Some(ComputerOp::Tab {
                action: TabAction::Focus,
                query: "news.ycombinator".into()
            })
        );
        assert_eq!(
            parse_computer_op("COMPUTER_CMD: tab new"),
            Some(ComputerOp::Tab {
                action: TabAction::New,
                query: String::new()
            })
        );
        assert_eq!(
            parse_computer_op("COMPUTER_CMD: tab open https://example.com"),
            Some(ComputerOp::Tab {
                action: TabAction::New,
                query: "https://example.com".into()
            })
        );
        assert_eq!(
            computer_cmd_line(&ComputerOp::Tab {
                action: TabAction::New,
                query: String::new()
            }),
            "COMPUTER_CMD: tab new"
        );
        assert!(parse_computer_op("COMPUTER_CMD: tab close").is_none());
        assert!(matches!(
            computer_drive(&ComputerOp::Tab {
                action: TabAction::Close,
                query: "GitHub".into()
            }),
            ComputerDrive::Tab(TabAction::Close, q) if q == "GitHub"
        ));
        let proto = hands_protocol();
        assert!(proto.contains("HOST_CMD:"));
        assert!(proto.contains("COMPUTER_CMD:"));
        assert!(proto.contains("tab close"));
        assert!(
            proto.contains("tab new") && proto.contains("ctrl+t") && proto.contains("<tool_call>"),
            "new tab and no XML tool_call: {proto}"
        );
        assert!(proto.to_ascii_lowercase().contains("unsandboxed"));
        assert!(
            proto.contains("Do not pkill") && proto.contains("hands are down"),
            "missing hands must not become a kill fallback: {proto}"
        );
        assert!(user_asks_desktop_hands("click the Save button for me"));
        assert!(user_asks_desktop_hands("type into the settings window"));
        assert!(user_asks_desktop_hands("take over this desktop"));
        assert!(user_asks_desktop_hands("take control of my mouse"));
        assert!(user_asks_takeover("take control of the window"));
        assert!(user_asks_takeover("this is broken, handle it"));
        assert!(user_asks_takeover("help me with this window"));
        assert!(!user_asks_desktop_hands("what is rust ownership?"));
        assert!(user_asks_gui_help("close that firefox tab"));
        assert!(user_asks_gui_help("turn bluetooth on in settings"));
        assert!(user_asks_gui_help("how do I enable this"));
        assert!(user_asks_gui_help("show me that tab"));
        assert!(!user_asks_gui_help("what is rust ownership?"));
        assert!(!user_asks_gui_help("show me how rust ownership works"));
        assert!(user_asks_guide_only("just tell me don't click"));
        assert!(user_asks_guide_only("walk me through enabling bluetooth"));
        assert!(!user_asks_guide_only("walk me through this and do it for me"));
        assert!(!user_asks_guide_only("click the Save button"));
        assert_eq!(see_drive_attach("close that firefox tab"), (true, true));
        assert_eq!(
            see_drive_attach("select a new tab in firefox"),
            (true, true)
        );
        assert_eq!(see_drive_attach("open a new tab"), (true, true));
        assert_eq!(see_drive_attach("open a table in postgres"), (false, false));
        assert_eq!(see_drive_attach("just tell me don't click"), (true, false));
        assert_eq!(see_drive_attach("what is rust ownership?"), (false, false));
        assert_eq!(
            hands_step_label("COMPUTER_RESULT (facts only):\n$ COMPUTER_CMD: act Bluetooth\nact Bluetooth @10,20\n"),
            Some("Clicked Bluetooth".into())
        );
        assert_eq!(
            hands_step_label("$ COMPUTER_CMD: tab close GitHub\nexit 0 · 8ms\nclosed GitHub"),
            Some("Closed tab GitHub".into())
        );
        assert_eq!(
            hands_step_label("$ COMPUTER_CMD: key ctrl+w\nexit 0\nkey ctrl+w"),
            Some("Pressed Ctrl+W".into())
        );
        assert!(hands_step_label("HOST_RESULT (facts only):\n$ ls\n").is_none());
        assert!(
            proto.contains("how-to") && proto.contains("Guide-only"),
            "protocol must teach after drive: {proto}"
        );
        assert_eq!(
            pick_hands_backend(true, true, true),
            Some(HandsBackend::Ydotool)
        );
        assert_eq!(
            pick_hands_backend(true, false, true),
            Some(HandsBackend::Xdotool)
        );
        assert_eq!(
            pick_hands_backend(false, true, true),
            Some(HandsBackend::Xdotool)
        );
        assert_eq!(hands_backend_name(None), "missing");
        match computer_drive_for(HandsBackend::Ydotool, &ComputerOp::Click { x: 10, y: 20 }) {
            ComputerDrive::Ydotool(steps) => {
                assert_eq!(steps[0], vec!["mousemove", "--absolute", "10", "20"]);
                assert_eq!(steps[1], vec!["click", "0xC0"]);
            }
            other => panic!("{other:?}"),
        }
        match computer_drive_for(
            HandsBackend::Ydotool,
            &ComputerOp::Key { name: "ctrl+s".into() },
        ) {
            ComputerDrive::Ydotool(steps) => {
                assert_eq!(steps[0], vec!["key", "29:1", "31:1", "31:0", "29:0"]);
            }
            other => panic!("{other:?}"),
        }
        match computer_drive_for(
            HandsBackend::Ydotool,
            &ComputerOp::Key { name: "F12".into() },
        ) {
            ComputerDrive::Ydotool(steps) => {
                assert!(
                    steps.is_empty(),
                    "unknown keys must not press Enter: {steps:?}"
                );
                assert_eq!(
                    empty_hands_steps_error(&ComputerOp::Key { name: "F12".into() }, &steps)
                        .as_deref(),
                    Some("unknown key F12")
                );
            }
            other => panic!("{other:?}"),
        }
        match computer_drive_for(
            HandsBackend::Ydotool,
            &ComputerOp::Key { name: "Alt+F4".into() },
        ) {
            ComputerDrive::Ydotool(steps) => {
                assert_eq!(steps[0], vec!["key", "56:1", "62:1", "62:0", "56:0"]);
                assert!(empty_hands_steps_error(
                    &ComputerOp::Key { name: "Alt+F4".into() },
                    &steps
                )
                .is_none());
            }
            other => panic!("{other:?}"),
        }
        let tmp = std::env::temp_dir().join(format!("grokhub-bin-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&tmp);
        let fake = tmp.join("ydotool");
        let _ = std::fs::write(&fake, b"");
        assert!(bin_on_path(
            "ydotool",
            "/no/such/path",
            &[tmp.to_str().unwrap()]
        ));
        assert!(!bin_on_path("ydotool", "/no/such/path", &[]));
        assert!(!bin_on_path("../ydotool", tmp.to_str().unwrap(), &[]));
        let extras = default_bin_extra_dirs("/home/viper");
        assert!(extras.iter().any(|d| d.ends_with("/.local/bin")));
        let _ = std::fs::remove_file(&fake);
        let _ = std::fs::remove_dir(&tmp);
        let rec = recipe_from_cmds(
            &["COMPUTER_CMD: act Save".into(), "HOST_CMD: ls".into()],
            Some(ScreenSize { w: 1920, h: 1080 }),
        )
        .unwrap();
        let json = recipe_to_json("last", &rec).unwrap();
        let (id, loaded) = recipe_from_json(&json).unwrap();
        assert_eq!(id, "last");
        assert_eq!(loaded, rec);
        assert!(user_asks_cabin_eyes("look at my screen"));
        assert!(user_asks_cabin_eyes("what do you see?"));
        assert!(user_asks_cabin_eyes("Cabin eyes — what's on the desktop"));
        assert!(!user_asks_cabin_eyes("what is rust ownership?"));
        assert!(!user_asks_cabin_eyes("tell me about chowder"));
        assert!(should_attach_hands_frame(false, true, true));
        assert!(!should_attach_hands_frame(false, false, true));
        assert!(should_attach_hands_frame(true, false, true));
        assert!(lock_blocks_hands(&["Lock screen", "nvim"]));
        assert!(!lock_blocks_hands(&["GrokHub", "Terminal"]));
        assert_eq!(
            parse_computer_cmd_loose("click 9 8"),
            Some(ComputerOp::Click { x: 9, y: 8 })
        );
        assert_eq!(
            parse_computer_cmd_loose("COMPUTER_CMD: key Return"),
            Some(ComputerOp::Key {
                name: "Return".into()
            })
        );
        match computer_drive(&ComputerOp::Scroll { dy: -3 }) {
            ComputerDrive::Xdotool(steps) => {
                assert_eq!(
                    steps[0],
                    vec!["click", "--clearmodifiers", "--repeat", "3", "5"]
                );
            }
            other => panic!("{other:?}"),
        }
        match computer_drive(&ComputerOp::WaitFor {
            title: Some("Settings".into()),
        }) {
            ComputerDrive::WaitFor(t) => assert_eq!(t.as_deref(), Some("Settings")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn lock_screen_blocks_pointer_but_allows_wait() {
        let lock = ["Lock screen"];
        assert!(hands_blocked_by_lock(
            &ComputerOp::Click { x: 10, y: 20 },
            &lock
        ));
        assert!(hands_blocked_by_lock(
            &ComputerOp::Type {
                text: "secret".into()
            },
            &["Password"]
        ));
        assert!(hands_blocked_by_lock(
            &ComputerOp::Key {
                name: "Return".into()
            },
            &["greeter"]
        ));
        assert!(hands_blocked_by_lock(
            &ComputerOp::Act {
                name: "Unlock".into()
            },
            &["polkit agent"]
        ));
        assert!(
            !hands_blocked_by_lock(
                &ComputerOp::WaitFor {
                    title: Some("Lock screen".into())
                },
                &lock
            ),
            "wait_for may poll; it must not click or type into a lock"
        );
        assert!(!hands_blocked_by_lock(
            &ComputerOp::Click { x: 10, y: 20 },
            &["GrokHub", "Terminal"]
        ));
        let only_lock = crate::windshield::lock_check_titles(&[
            "0x02 0 0 0 1920 1080 Lock screen",
        ]);
        let only_lock_refs: Vec<&str> = only_lock.iter().map(|s| s.as_str()).collect();
        assert!(
            hands_blocked_by_lock(&ComputerOp::Click { x: 10, y: 20 }, &only_lock_refs),
            "a locker that was filtered from click targets must still block hands"
        );
        assert!(pointer_op_blocked_on_lock(&ComputerOp::Scroll { dy: 1 }));
        assert!(!pointer_op_blocked_on_lock(&ComputerOp::WaitFor { title: None }));
    }

    #[test]
    fn recipe_from_cmds_ignores_shell_type() {
        assert!(
            recipe_from_cmds(&["type cargo".into()], None).is_none(),
            "bash type must not become a desktop type-in recipe"
        );
        assert!(
            recipe_from_cmds(&["key Return".into()], None).is_none(),
            "unprefixed key must not become desktop hands"
        );
        assert!(
            recipe_from_cmds(&["click 10 20".into()], None).is_none(),
            "unprefixed click must not become desktop hands"
        );
        let typed = recipe_from_cmds(&["COMPUTER_CMD: type hello".into()], None);
        assert!(typed.is_some(), "prefixed COMPUTER_CMD type stays hands");
        assert_eq!(
            typed.unwrap().ops,
            vec![ComputerOp::Type {
                text: "hello".into()
            }]
        );
    }

    #[test]
    fn rejects_incomplete_computer_ops() {
        assert!(parse_computer_op("COMPUTER_CMD: type").is_none());
        assert!(parse_computer_op("COMPUTER_CMD: key").is_none());
        assert!(parse_computer_op("COMPUTER_CMD: click").is_none());
        assert!(parse_computer_op("COMPUTER_CMD: click x y").is_none());
        assert!(parse_computer_op("COMPUTER_CMD: nope 1 2").is_none());
        assert!(parse_computer_op("COMPUTER_CMD:").is_none());
        assert_eq!(
            extract_computer_ops("noise\nCOMPUTER_CMD: move 1 2\nHOST_CMD: ls\n"),
            vec![ComputerOp::Move { x: 1, y: 2 }]
        );
        assert_eq!(
            screen_from_extents(1920, 1080),
            Some(ScreenSize { w: 1920, h: 1080 })
        );
        assert!(screen_from_extents(0, 1080).is_none());
        assert!(screen_from_extents(1920, 0).is_none());
    }

    #[test]
    fn cabin_eyes_stay_dormant_until_called() {
        assert!(!should_attach_hands_frame(false, false, true));
        assert!(!should_attach_hands_frame(false, false, false));
        assert!(should_attach_hands_frame(true, false, true));
        assert!(should_attach_hands_frame(false, true, true));
        assert!(!user_asks_cabin_eyes("what's in the bowl"));
        assert!(!user_asks_cabin_eyes("tell me about rust"));
        assert!(user_asks_cabin_eyes("look at my screen"));
        assert!(hands_protocol().contains("GUI help"));
        assert!(hands_protocol().contains("global desktop"));
        assert!(hands_protocol().contains("ctrl+w"));
    }

    #[test]
    fn hands_parse_cursor_and_move_monitor() {
        assert_eq!(
            parse_computer_op("COMPUTER_CMD: cursor"),
            Some(ComputerOp::Cursor)
        );
        assert_eq!(
            parse_computer_op("COMPUTER_CMD: move monitor DP-2"),
            Some(ComputerOp::MoveMonitor {
                name: "DP-2".into(),
                x: None,
                y: None
            })
        );
        assert_eq!(
            parse_computer_op("COMPUTER_CMD: move monitor DP-2 100 20"),
            Some(ComputerOp::MoveMonitor {
                name: "DP-2".into(),
                x: Some(100),
                y: Some(20)
            })
        );
        assert_eq!(
            computer_cmd_line(&ComputerOp::Cursor),
            "COMPUTER_CMD: cursor"
        );
        assert_eq!(
            computer_cmd_line(&ComputerOp::MoveMonitor {
                name: "DP-2".into(),
                x: None,
                y: None
            }),
            "COMPUTER_CMD: move monitor DP-2"
        );
        assert_eq!(
            computer_cmd_line(&ComputerOp::MoveMonitor {
                name: "DP-2".into(),
                x: Some(100),
                y: Some(20)
            }),
            "COMPUTER_CMD: move monitor DP-2 100 20"
        );
        assert!(matches!(
            computer_drive(&ComputerOp::Cursor),
            ComputerDrive::Cursor
        ));
        assert!(matches!(
            computer_drive(&ComputerOp::MoveMonitor {
                name: "DP-2".into(),
                x: Some(100),
                y: Some(20)
            }),
            ComputerDrive::MoveMonitor { name, x, y } if name == "DP-2" && x == Some(100) && y == Some(20)
        ));
        assert!(!pointer_op_blocked_on_lock(&ComputerOp::Cursor));
        assert!(pointer_op_blocked_on_lock(&ComputerOp::MoveMonitor {
            name: "DP-2".into(),
            x: None,
            y: None
        }));
        assert_eq!(
            empty_hands_steps_error(&ComputerOp::Cursor, &[]),
            None
        );
        let proto = hands_protocol();
        assert!(proto.contains("COMPUTER_CMD: cursor"), "{proto}");
        assert!(proto.contains("move monitor"), "{proto}");
        assert!(proto.contains("monitor="), "{proto}");
        assert_eq!(label_for_op(&ComputerOp::Cursor), "Read cursor");
        assert_eq!(
            label_for_op(&ComputerOp::MoveMonitor {
                name: "DP-2".into(),
                x: None,
                y: None
            }),
            "Moved to monitor DP-2"
        );
        assert_eq!(
            relative_move_steps(HandsBackend::Ydotool, 5600, -10),
            vec![vec![
                "mousemove".to_string(),
                "5600".to_string(),
                "-10".to_string()
            ]]
        );
        assert_eq!(
            relative_move_steps(HandsBackend::Xdotool, -12, 4),
            vec![vec![
                "mousemove_relative".to_string(),
                "--".to_string(),
                "-12".to_string(),
                "4".to_string()
            ]]
        );
        assert!(relative_move_steps(HandsBackend::Xdotool, 0, 0).is_empty());
    }
}
