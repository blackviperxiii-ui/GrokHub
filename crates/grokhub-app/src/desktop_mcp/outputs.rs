//! Wayland monitor lists, parsed without calling a compositor.

#[cfg(any(unix, test))]
use grokhub_core::desktop_mcp::MonitorGeom;

#[cfg(any(unix, test))]
pub(crate) fn parse_sway_outputs(text: &str) -> Option<Vec<MonitorGeom>> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    let arr = v.as_array()?;
    let mut out = Vec::new();
    for (i, item) in arr.iter().enumerate() {
        let name = item.get("name").and_then(|n| n.as_str()).unwrap_or("").trim();
        if name.is_empty() {
            continue;
        }
        let rect = item.get("rect")?;
        let x = rect.get("x").and_then(|n| n.as_i64()).unwrap_or(0) as i32;
        let y = rect.get("y").and_then(|n| n.as_i64()).unwrap_or(0) as i32;
        let w = rect.get("width").and_then(|n| n.as_u64()).unwrap_or(0) as u32;
        let h = rect.get("height").and_then(|n| n.as_u64()).unwrap_or(0) as u32;
        if w == 0 || h == 0 {
            continue;
        }
        let scale = item.get("scale").and_then(|n| n.as_f64()).unwrap_or(1.0);
        let primary = item.get("primary").and_then(|n| n.as_bool()).unwrap_or(i == 0);
        out.push(MonitorGeom {
            id: name.to_string(),
            name: name.to_string(),
            x,
            y,
            width: w,
            height: h,
            scale_factor: if scale.is_finite() && scale > 0.0 { scale } else { 1.0 },
            primary,
        });
    }
    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}

#[cfg(any(unix, test))]
pub(crate) fn parse_wlr_randr(text: &str) -> Vec<MonitorGeom> {
    let mut out = Vec::new();
    let mut cur: Option<Partial> = None;
    for line in text.lines() {
        let indented = line.starts_with(' ') || line.starts_with('\t');
        if indented {
            let Some(slot) = cur.as_mut() else {
                continue;
            };
            let t = line.trim();
            if let Some(rest) = t.strip_prefix("Position:") {
                let mut bits = rest.trim().split(',');
                slot.x = bits.next().unwrap_or("0").trim().parse().unwrap_or(0);
                slot.y = bits.next().unwrap_or("0").trim().parse().unwrap_or(0);
            } else if let Some(rest) = t.strip_prefix("Scale:") {
                slot.scale = rest.trim().parse().unwrap_or(1.0);
            } else if let Some(rest) = t.strip_prefix("Enabled:") {
                slot.enabled = rest.trim().eq_ignore_ascii_case("yes");
            } else if mode_is_current(t) {
                if let Some((w, h)) = mode_size(t) {
                    slot.w = w;
                    slot.h = h;
                }
            }
            continue;
        }
        if let Some(done) = cur.take() {
            push_partial(&mut out, done);
        }
        let name = line.split_whitespace().next().unwrap_or("").trim();
        if name.is_empty() || name.ends_with(':') {
            continue;
        }
        cur = Some(Partial {
            name: name.to_string(),
            enabled: true,
            x: 0,
            y: 0,
            w: 0,
            h: 0,
            scale: 1.0,
        });
    }
    if let Some(done) = cur.take() {
        push_partial(&mut out, done);
    }
    if let Some(first) = out.first_mut() {
        first.primary = true;
    }
    out
}

#[cfg(any(unix, test))]
struct Partial {
    name: String,
    enabled: bool,
    x: i32,
    y: i32,
    w: u32,
    h: u32,
    scale: f64,
}

#[cfg(any(unix, test))]
fn push_partial(out: &mut Vec<MonitorGeom>, slot: Partial) {
    if !slot.enabled || slot.w == 0 || slot.h == 0 {
        return;
    }
    out.push(MonitorGeom {
        id: slot.name.clone(),
        name: slot.name,
        x: slot.x,
        y: slot.y,
        width: slot.w,
        height: slot.h,
        scale_factor: if slot.scale.is_finite() && slot.scale > 0.0 {
            slot.scale
        } else {
            1.0
        },
        primary: false,
    });
}

// wlr-randr tags the live mode "(current)" or "(preferred, current)".
#[cfg(any(unix, test))]
fn mode_is_current(line: &str) -> bool {
    line.split(|c: char| c == '(' || c == ')' || c == ',' || c.is_whitespace())
        .any(|word| word.eq_ignore_ascii_case("current"))
}

#[cfg(any(unix, test))]
fn mode_size(line: &str) -> Option<(u32, u32)> {
    let token = line.split_whitespace().next()?;
    let (w, h) = token.split_once('x')?;
    let w: u32 = w.parse().ok()?;
    let h: u32 = h.trim_end_matches(',').parse().ok()?;
    if w == 0 || h == 0 {
        None
    } else {
        Some((w, h))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_mcp_sway_outputs_keep_origin_and_scale() {
        let text = r#"[{"name":"eDP-1","primary":true,"scale":1.25,"rect":{"x":0,"y":0,"width":1920,"height":1080}},{"name":"DP-2","scale":1,"rect":{"x":-1920,"y":0,"width":1920,"height":1080}}]"#;
        let mons = parse_sway_outputs(text).unwrap();
        assert_eq!(mons.len(), 2);
        assert_eq!(mons[1].x, -1920);
        assert!((mons[0].scale_factor - 1.25).abs() < 1e-9);
        assert!(mons[0].primary);
        assert!(parse_sway_outputs("nope").is_none());
    }

    #[test]
    fn desktop_mcp_wlr_randr_current_mode() {
        let text = "\
eDP-1 \"Built-in\"
  Enabled: yes
  Position: 0,0
  Scale: 1.50
  Modes:
    1920x1080 px, 60 Hz (preferred, current)
DP-2 \"Left\"
  Enabled: yes
  Position: -1920,0
  Scale: 1.00
  Modes:
    1280x720 px, 60 Hz (current)
HDMI-A-1 \"Off\"
  Enabled: no
  Position: 0,1080
  Modes:
    1920x1080 px, 60 Hz (current)
";
        let mons = parse_wlr_randr(text);
        assert_eq!(mons.len(), 2);
        assert_eq!(mons[0].name, "eDP-1");
        assert!(mons[0].primary);
        assert_eq!(mons[0].width, 1920);
        assert!((mons[0].scale_factor - 1.5).abs() < 1e-9);
        assert_eq!(mons[1].x, -1920);
        assert_eq!(mons[1].height, 720);
    }
}
