#[derive(Debug, Clone, PartialEq)]
pub struct WindshieldObject {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WindshieldNext {
    pub label: String,
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WindshieldFrame {
    pub objects: Vec<WindshieldObject>,
    pub next: Option<WindshieldNext>,
    pub goal: Option<String>,
    pub skill_id: Option<String>,
    pub autonomy: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AtspiRow {
    pub name: String,
    pub role: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

pub struct PendingStep {
    pub op: String,
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub text: Option<String>,
}

pub fn build_windshield(
    atspi: &[AtspiRow],
    pending: Option<&PendingStep>,
    refused: Option<&str>,
    goal: Option<&str>,
    skill_id: Option<&str>,
    autonomy: u8,
) -> WindshieldFrame {
    let mut objects: Vec<WindshieldObject> = atspi
        .iter()
        .enumerate()
        .map(|(i, r)| WindshieldObject {
            id: format!("w{i}"),
            kind: windshield_kind(&r.role),
            label: if r.name.is_empty() {
                r.role.clone()
            } else {
                r.name.clone()
            },
            x: r.x,
            y: r.y,
            w: r.w,
            h: r.h,
        })
        .collect();
    if let Some(why) = refused {
        objects.push(WindshieldObject {
            id: "wont".into(),
            kind: "wont".into(),
            label: why.to_string(),
            x: 0,
            y: 0,
            w: 0,
            h: 0,
        });
    }
    let next = pending.map(|p| WindshieldNext {
        label: p.text.clone().unwrap_or_else(|| p.op.clone()),
        x: p.x.unwrap_or(0),
        y: p.y.unwrap_or(0),
    });
    WindshieldFrame {
        objects,
        next,
        goal: goal.map(|s| s.to_string()),
        skill_id: skill_id.map(|s| s.to_string()),
        autonomy,
    }
}

/// Compact object list for the model. Prefer `act <label>` over guess-clicks.
pub fn windshield_prompt(frame: &WindshieldFrame) -> String {
    if frame.objects.is_empty() {
        return String::new();
    }
    let mut s = String::new();
    for o in frame.objects.iter().take(40) {
        s.push_str(&format!(
            "- [{}] {} @{},{} {}x{}\n",
            o.kind, o.label, o.x, o.y, o.w, o.h
        ));
    }
    s
}

fn windshield_kind(role: &str) -> String {
    let r = role.trim().to_ascii_lowercase().replace(' ', "-");
    if r.is_empty() {
        "object".into()
    } else {
        r
    }
}

pub fn is_interactive_role(role: &str) -> bool {
    let r = role.to_ascii_lowercase();
    r.contains("push button")
        || r.contains("push-button")
        || r.contains("page tab")
        || r.contains("page-tab")
        || r.contains("menu item")
        || r.contains("menu-item")
        || r.contains("check box")
        || r.contains("check-box")
        || r.contains("radio")
        || r.contains("combo")
        || r.contains("toggle")
        || r == "link"
        || r == "slider"
        || r == "entry"
        || r == "text"
        || r.contains("close")
}

fn closeish(s: &str) -> bool {
    s.to_ascii_lowercase().contains("close")
}

pub fn keep_atspi_row(row: &AtspiRow, desk_w: i32, desk_h: i32) -> bool {
    if row.role.eq_ignore_ascii_case("cursor") {
        return true;
    }
    if row.w <= 0 || row.h <= 0 {
        return false;
    }
    if desk_w > 0 && (row.x >= desk_w || row.x + row.w <= 0) {
        return false;
    }
    if desk_h > 0 && (row.y >= desk_h || row.y + row.h <= 0) {
        return false;
    }
    if is_interactive_role(&row.role) || closeish(&row.name) || closeish(&row.role) {
        return true;
    }
    !row.name.is_empty()
}

pub fn filter_atspi_rows(rows: &[AtspiRow], desk_w: i32, desk_h: i32) -> Vec<AtspiRow> {
    rows.iter()
        .filter(|r| keep_atspi_row(r, desk_w, desk_h))
        .cloned()
        .take(80)
        .collect()
}

fn row_rank(row: &AtspiRow, ask: Option<&str>, cursor: Option<(i32, i32)>) -> i64 {
    let mut s = 0i64;
    if is_interactive_role(&row.role) {
        s += 100;
    }
    if !row.name.is_empty() {
        s += 50;
    }
    if closeish(&row.name) || closeish(&row.role) {
        s += 80;
    }
    if let Some(ask) = ask {
        let a = ask.to_ascii_lowercase();
        let n = row.name.to_ascii_lowercase();
        if !n.is_empty() && a.contains(&n) {
            s += 200;
        }
        for w in n.split_whitespace() {
            if w.len() >= 3 && a.contains(w) {
                s += 40;
            }
        }
    }
    if let Some((cx, cy)) = cursor {
        let x = row.x + row.w / 2;
        let y = row.y + row.h / 2;
        let d = (x - cx).abs() as i64 + (y - cy).abs() as i64;
        if d < 400 {
            s += 40;
        }
    }
    s
}

pub fn rank_atspi_rows(rows: &[AtspiRow], ask: Option<&str>, limit: usize) -> Vec<AtspiRow> {
    let cursor = rows.iter().find(|r| r.role.eq_ignore_ascii_case("cursor"));
    let cursor_xy = cursor.map(|r| (r.x, r.y));
    let mut scored: Vec<(i64, &AtspiRow)> = rows
        .iter()
        .map(|r| (row_rank(r, ask, cursor_xy), r))
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored
        .into_iter()
        .take(limit)
        .map(|(_, r)| r.clone())
        .collect()
}

/// Exact / prefix name, then the smallest matching control (tab × beats the window).
pub fn pick_named_row<'a>(rows: &'a [AtspiRow], name: &str) -> Option<&'a AtspiRow> {
    let q = name.trim().to_ascii_lowercase();
    if q.is_empty() {
        return None;
    }
    rows.iter()
        .filter(|r| {
            r.role != "cursor"
                && (r.name.to_ascii_lowercase().contains(&q)
                    || r.role.to_ascii_lowercase().contains(&q))
        })
        .min_by_key(|r| {
            let n = r.name.to_ascii_lowercase();
            let exact = if n == q {
                0u8
            } else if n.starts_with(&q) {
                1
            } else {
                2
            };
            let area = (r.w as i64).saturating_mul(r.h as i64).max(1);
            (exact, area)
        })
}

/// `role=push button name=Install x=10 y=20 w=80 h=24`
pub fn parse_atspi_line(line: &str) -> Option<AtspiRow> {
    let line = line.trim();
    if !line.contains("role=") {
        return None;
    }
    let mut role = String::new();
    let mut name = String::new();
    let mut x = 0;
    let mut y = 0;
    let mut w = 0;
    let mut h = 0;
    for part in line.split_whitespace() {
        if let Some(v) = part.strip_prefix("role=") {
            role = v.replace('-', " ");
        } else if let Some(v) = part.strip_prefix("name=") {
            name = v.replace('_', " ");
        } else if let Some(v) = part.strip_prefix("x=") {
            x = v.parse().ok()?;
        } else if let Some(v) = part.strip_prefix("y=") {
            y = v.parse().ok()?;
        } else if let Some(v) = part.strip_prefix("w=") {
            w = v.parse().ok()?;
        } else if let Some(v) = part.strip_prefix("h=") {
            h = v.parse().ok()?;
        }
    }
    if role.is_empty() && name.is_empty() {
        return None;
    }
    if lockish(&name) || lockish(&role) {
        return None;
    }
    Some(AtspiRow { name, role, x, y, w, h })
}

/// Window title from an AT-SPI line, including lock / greeter surfaces.
pub fn window_name_from_atspi(line: &str) -> Option<String> {
    let line = line.trim();
    if !line.contains("role=") {
        return None;
    }
    let mut name = String::new();
    for part in line.split_whitespace() {
        if let Some(v) = part.strip_prefix("name=") {
            name = v.replace('_', " ");
        }
    }
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

pub fn parse_xdotool_mouse(line: &str) -> Option<AtspiRow> {
    // x:123 y:456 screen:0 window:123
    let mut x = 0;
    let mut y = 0;
    let mut seen = false;
    for part in line.split_whitespace() {
        if let Some(v) = part.strip_prefix("x:") {
            x = v.parse().ok()?;
            seen = true;
        }
        if let Some(v) = part.strip_prefix("y:") {
            y = v.parse().ok()?;
            seen = true;
        }
    }
    if !seen {
        return None;
    }
    Some(AtspiRow {
        name: "cursor".into(),
        role: "cursor".into(),
        x,
        y,
        w: 1,
        h: 1,
    })
}

pub fn refused_lock(labels: &[&str]) -> Option<&'static str> {
    if labels.iter().any(|s| lockish(s)) {
        Some("lock screen")
    } else {
        None
    }
}

fn lockish(s: &str) -> bool {
    let s = s.to_ascii_lowercase();
    s.contains("lock") || s.contains("password") || s.contains("greeter")
}

/// `wmctrl -lG` line → row. Also accepts `id x y w h name`.
pub fn parse_wmctrl_line(line: &str) -> Option<AtspiRow> {
    let mut bits = line.split_whitespace();
    let _id = bits.next()?;
    let _desk = bits.next();
    let x = bits.next()?.parse().ok()?;
    let y = bits.next()?.parse().ok()?;
    let w = bits.next()?.parse().ok()?;
    let h = bits.next()?.parse().ok()?;
    let name = bits.collect::<Vec<_>>().join(" ");
    if lockish(&name) {
        return None;
    }
    Some(AtspiRow {
        name,
        role: "window".into(),
        x,
        y,
        w,
        h,
    })
}

/// Window title from a `wmctrl -lG` line, including lock / greeter surfaces.
pub fn window_name_from_wmctrl(line: &str) -> Option<String> {
    let mut bits = line.split_whitespace();
    let _id = bits.next()?;
    let _desk = bits.next();
    let _x: i32 = bits.next()?.parse().ok()?;
    let _y: i32 = bits.next()?.parse().ok()?;
    let _w: i32 = bits.next()?.parse().ok()?;
    let _h: i32 = bits.next()?.parse().ok()?;
    let name = bits.collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// Titles for the lock-screen hands gate. Lock windows stay in this list
/// even though they are dropped as click targets.
pub fn lock_check_titles(lines: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    for line in lines {
        if let Some(name) = window_name_from_atspi(line).or_else(|| window_name_from_wmctrl(line)) {
            if name.eq_ignore_ascii_case("cursor") {
                continue;
            }
            if !out.iter().any(|e| e == &name) {
                out.push(name);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn objects_next_and_wont() {
        let rows = [AtspiRow {
            name: "Terminal".into(),
            role: "window".into(),
            x: 10,
            y: 20,
            w: 800,
            h: 600,
        }];
        let pending = PendingStep {
            op: "click".into(),
            x: Some(40),
            y: Some(50),
            text: Some("Install".into()),
        };
        let f = build_windshield(&rows, Some(&pending), Some("lock screen"), Some("flash pi"), None, 1);
        assert_eq!(f.objects[0].label, "Terminal");
        assert_eq!(f.objects.iter().find(|o| o.kind == "wont").unwrap().label, "lock screen");
        assert_eq!(f.next.as_ref().unwrap().label, "Install");
        assert_eq!(f.autonomy, 1);
        let row = parse_atspi_line("role=push-button name=Install x=10 y=20 w=80 h=24").unwrap();
        assert_eq!(row.name, "Install");
        assert_eq!(row.x, 10);
        assert!(parse_atspi_line("role=window name=Lock x=0 y=0 w=1 h=1").is_none());
        assert_eq!(refused_lock(&["Lock screen"]), Some("lock screen"));
        let cursor = parse_xdotool_mouse("x:123 y:456 screen:0 window:9").unwrap();
        assert_eq!((cursor.x, cursor.y, cursor.role.as_str()), (123, 456, "cursor"));
        assert!(parse_xdotool_mouse("").is_none());
        let win = parse_wmctrl_line("0x01 0 10 20 800 600 GrokHub").unwrap();
        assert_eq!(win.name, "GrokHub");
        assert_eq!((win.x, win.y, win.w, win.h), (10, 20, 800, 600));
        assert!(
            parse_wmctrl_line("0x02 0 0 0 1920 1080 Lock screen").is_none(),
            "lock windows must not become click targets"
        );
        assert!(parse_wmctrl_line("0x03 0 0 0 1920 1080 GDM Greeter").is_none());
        let prompt = windshield_prompt(&f);
        assert!(prompt.contains("[window] Terminal"));
        assert!(prompt.contains("[wont] lock screen"));
        let btn = parse_atspi_line("role=push-button name=Save x=10 y=20 w=80 h=24").unwrap();
        let glass = windshield_prompt(&build_windshield(&[btn], None, None, None, None, 4));
        assert!(glass.contains("[push-button] Save"));
    }

    fn row(name: &str, role: &str, x: i32, y: i32, w: i32, h: i32) -> AtspiRow {
        AtspiRow {
            name: name.into(),
            role: role.into(),
            x,
            y,
            w,
            h,
        }
    }

    #[test]
    fn act_picks_the_small_close_on_the_other_monitor() {
        let rows = [
            row("Firefox", "frame", 1920, 0, 1920, 1080),
            row("Close", "push button", 3720, 12, 16, 16),
            row("", "filler", 1920, 0, 1920, 1080),
            row("cursor", "cursor", 2000, 40, 1, 1),
        ];
        let kept = filter_atspi_rows(&rows, 3840, 1080);
        assert!(kept.iter().any(|r| r.name == "Close"));
        assert!(kept.iter().any(|r| r.name == "Firefox"));
        assert!(!kept.iter().any(|r| r.role == "filler"));
        let hit = pick_named_row(&kept, "Close").unwrap();
        assert_eq!(hit.name, "Close");
        assert_eq!((hit.x + hit.w / 2, hit.y + hit.h / 2), (3728, 20));
        let ranked = rank_atspi_rows(&kept, Some("close the firefox tab"), 40);
        assert!(ranked.iter().any(|r| r.name == "Close"));
        let glass = windshield_prompt(&build_windshield(&ranked, None, None, None, None, 4));
        assert!(glass.contains("Close"));
        assert!(glass.contains("@3720,12"));
    }

    #[test]
    fn lock_check_titles_keep_lock_windows() {
        let titles = lock_check_titles(&[
            "0x01 0 10 20 800 600 GrokHub",
            "0x02 0 0 0 1920 1080 Lock screen",
            "role=window name=GDM_Greeter x=0 y=0 w=1920 h=1080",
        ]);
        assert!(titles.iter().any(|t| t.eq_ignore_ascii_case("Lock screen")));
        assert!(titles.iter().any(|t| t.to_ascii_lowercase().contains("greeter")));
        assert!(titles.iter().any(|t| t == "GrokHub"));
        assert_eq!(
            window_name_from_wmctrl("0x02 0 0 0 1920 1080 Lock screen").as_deref(),
            Some("Lock screen")
        );
    }
}
