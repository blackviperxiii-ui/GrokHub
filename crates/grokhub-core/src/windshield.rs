#[derive(Debug, Clone, PartialEq)]
pub struct WindshieldObject {
    pub id: String,
    pub kind: &'static str,
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
            kind: if r.role.eq_ignore_ascii_case("cursor") {
                "cursor"
            } else {
                "window"
            },
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
            kind: "wont",
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
    }
}
