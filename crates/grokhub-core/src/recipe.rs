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
            let x = bits.next()?.parse().ok()?;
            let y = bits.next()?.parse().ok()?;
            Some(ComputerOp::Move { x, y })
        }
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
}

pub fn computer_drive(op: &ComputerOp) -> ComputerDrive {
    computer_drive_for(HandsBackend::Xdotool, op)
}

pub fn computer_drive_for(backend: HandsBackend, op: &ComputerOp) -> ComputerDrive {
    match op {
        ComputerOp::Act { name } => ComputerDrive::Act(name.clone()),
        ComputerOp::WaitFor { title } => ComputerDrive::WaitFor(title.clone()),
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
        ComputerOp::Act { .. } | ComputerOp::WaitFor { .. } => vec![],
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
        ComputerOp::Key { name } => {
            let mut step = vec!["key".into()];
            step.extend(ydotool_key_tokens(name));
            vec![step]
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
        ComputerOp::Act { .. } | ComputerOp::WaitFor { .. } => vec![],
    }
}

fn ydotool_key_tokens(name: &str) -> Vec<String> {
    let parts: Vec<&str> = name
        .split('+')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    let codes: Vec<u16> = parts.iter().map(|p| linux_keycode(p)).collect();
    let mut tokens = Vec::new();
    for c in &codes {
        tokens.push(format!("{c}:1"));
    }
    for c in codes.iter().rev() {
        tokens.push(format!("{c}:0"));
    }
    tokens
}

fn linux_keycode(name: &str) -> u16 {
    match name.to_ascii_lowercase().as_str() {
        "return" | "enter" | "kp_enter" => 28,
        "esc" | "escape" => 1,
        "tab" => 15,
        "space" | "spacebar" => 57,
        "backspace" => 14,
        "ctrl" | "control" | "control_l" | "ctrl_l" => 29,
        "shift" | "shift_l" => 42,
        "alt" | "alt_l" => 56,
        "super" | "super_l" | "meta" | "win" => 125,
        "up" => 103,
        "down" => 108,
        "left" => 105,
        "right" => 106,
        "delete" | "del" => 111,
        "home" => 102,
        "end" => 107,
        "pageup" | "prior" => 104,
        "pagedown" | "next" => 109,
        "f1" => 59,
        "f2" => 60,
        "f3" => 61,
        "f4" => 62,
        "f5" => 63,
        "a" => 30,
        "s" => 31,
        "d" => 32,
        "c" => 46,
        "v" => 47,
        "x" => 45,
        "z" => 44,
        "q" => 16,
        "w" => 17,
        other if other.len() == 1 => {
            let ch = other.chars().next().unwrap_or('a');
            match ch {
                '0' => 11,
                '1'..='9' => 2 + (ch as u16 - b'1' as u16),
                'b' => 48,
                'e' => 18,
                'f' => 33,
                'g' => 34,
                'h' => 35,
                'i' => 23,
                'j' => 36,
                'k' => 37,
                'l' => 38,
                'm' => 50,
                'n' => 49,
                'o' => 24,
                'p' => 25,
                'r' => 19,
                't' => 20,
                'u' => 22,
                'y' => 21,
                _ => 28,
            }
        }
        _ => 28,
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
    let ops: Vec<ComputerOp> = cmds.iter().filter_map(|c| parse_computer_cmd_loose(c)).collect();
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
     Prefer act and wait_for over raw clicks. Coordinates are the current screen; a JPEG frame is attached only when the user asks for hands or cabin eyes. Lock/password screens are won'ts — never click them or type into them. Do not read ~/.ssh or /etc/shadow."
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
    !matches!(op, ComputerOp::WaitFor { .. })
}

pub fn hands_blocked_by_lock(op: &ComputerOp, titles: &[&str]) -> bool {
    pointer_op_blocked_on_lock(op) && lock_blocks_hands(titles)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let proto = hands_protocol();
        assert!(proto.contains("HOST_CMD:"));
        assert!(proto.contains("COMPUTER_CMD:"));
        assert!(proto.to_ascii_lowercase().contains("unsandboxed"));
        assert!(user_asks_desktop_hands("click the Save button for me"));
        assert!(user_asks_desktop_hands("type into the settings window"));
        assert!(user_asks_desktop_hands("take over this desktop"));
        assert!(user_asks_takeover("this is broken, handle it"));
        assert!(user_asks_takeover("help me with this window"));
        assert!(!user_asks_desktop_hands("what is rust ownership?"));
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
        assert!(pointer_op_blocked_on_lock(&ComputerOp::Scroll { dy: 1 }));
        assert!(!pointer_op_blocked_on_lock(&ComputerOp::WaitFor { title: None }));
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
        assert!(hands_protocol().contains("only when the user asks"));
    }
}
