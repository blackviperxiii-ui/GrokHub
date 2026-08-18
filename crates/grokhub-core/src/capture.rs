//! Desktop / webcam grab plan. X11 root + one ffmpeg frame is a black void on Wayland.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureKind {
    Grim,
    GnomeScreenshot,
    Spectacle,
    GnomeShell,
    Maim,
    Scrot,
    Import,
    FfmpegX11,
}

pub fn has_bin(bins: &[&str], name: &str) -> bool {
    bins.iter().any(|b| *b == name)
}

/// Wayland-native tools first. X11 grabbers last — they see a black root on GNOME/KDE.
pub fn capture_kinds(bins: &[&str], wayland: bool, x11: bool) -> Vec<CaptureKind> {
    let mut out = Vec::new();
    if wayland && has_bin(bins, "grim") {
        out.push(CaptureKind::Grim);
    }
    if has_bin(bins, "gnome-screenshot") {
        out.push(CaptureKind::GnomeScreenshot);
    }
    if has_bin(bins, "spectacle") {
        out.push(CaptureKind::Spectacle);
    }
    if has_bin(bins, "gdbus") {
        out.push(CaptureKind::GnomeShell);
    }
    if x11 && has_bin(bins, "maim") {
        out.push(CaptureKind::Maim);
    }
    if x11 && has_bin(bins, "scrot") {
        out.push(CaptureKind::Scrot);
    }
    if x11 && has_bin(bins, "import") {
        out.push(CaptureKind::Import);
    }
    if x11 && has_bin(bins, "ffmpeg") {
        out.push(CaptureKind::FfmpegX11);
    }
    if !wayland && has_bin(bins, "grim") && !out.contains(&CaptureKind::Grim) {
        out.push(CaptureKind::Grim);
    }
    out
}

pub fn parse_xdpy_size(text: &str) -> Option<(u32, u32)> {
    for line in text.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("dimensions:") else {
            continue;
        };
        let rest = rest.trim();
        let dim = rest.split_whitespace().next()?;
        return parse_wxh(dim);
    }
    None
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DisplayOutput {
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub primary: bool,
}

/// `eDP-1 connected primary 1920x1080+0+0` / `HDMI-1 connected 1920x1080+1920+0`.
pub fn parse_xrandr_outputs(text: &str) -> Vec<DisplayOutput> {
    let mut out = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if !line.contains(" connected") || line.contains("disconnected") {
            continue;
        }
        let name = match line.split_whitespace().next() {
            Some(n) if !n.is_empty() => n.to_string(),
            _ => continue,
        };
        let primary = line.split_whitespace().any(|p| p == "primary");
        let Some((w, h, x, y)) = parse_xrandr_geom(line) else {
            continue;
        };
        out.push(DisplayOutput {
            name,
            x,
            y,
            w,
            h,
            primary,
        });
    }
    out
}

fn parse_xrandr_geom(line: &str) -> Option<(i32, i32, i32, i32)> {
    for part in line.split_whitespace() {
        let Some((wh, rest)) = part.split_once('+') else {
            continue;
        };
        let Some((w, h)) = wh.split_once('x') else {
            continue;
        };
        let w: i32 = w.parse().ok()?;
        let h: i32 = h.parse().ok()?;
        if w < 64 || h < 64 {
            continue;
        }
        let Some((xs, ys)) = rest.split_once('+') else {
            continue;
        };
        let x: i32 = xs.parse().ok()?;
        let y: i32 = ys.parse().ok()?;
        return Some((w, h, x, y));
    }
    None
}

pub fn virtual_desktop_bounds(outputs: &[DisplayOutput]) -> Option<(i32, i32, u32, u32)> {
    if outputs.is_empty() {
        return None;
    }
    let mut min_x = i32::MAX;
    let mut min_y = i32::MAX;
    let mut max_x = i32::MIN;
    let mut max_y = i32::MIN;
    for o in outputs {
        min_x = min_x.min(o.x);
        min_y = min_y.min(o.y);
        max_x = max_x.max(o.x + o.w);
        max_y = max_y.max(o.y + o.h);
    }
    let w = max_x.saturating_sub(min_x);
    let h = max_y.saturating_sub(min_y);
    if w < 64 || h < 64 {
        return None;
    }
    Some((min_x, min_y, w as u32, h as u32))
}

pub fn virtual_desktop_size(outputs: &[DisplayOutput]) -> Option<(u32, u32)> {
    virtual_desktop_bounds(outputs).map(|(_, _, w, h)| (w, h))
}

/// JPEG pixel → global desktop. `frame_origin` is the captured output's top-left.
pub fn image_to_global(
    jx: i32,
    jy: i32,
    jpeg_w: u32,
    jpeg_h: u32,
    outputs: &[DisplayOutput],
    frame_origin: Option<(i32, i32)>,
) -> (i32, i32) {
    if jpeg_w == 0 || jpeg_h == 0 {
        return (jx, jy);
    }
    let (min_x, min_y, desk_w, desk_h) =
        virtual_desktop_bounds(outputs).unwrap_or((0, 0, jpeg_w, jpeg_h));
    if (jx >= jpeg_w as i32 || jy >= jpeg_h as i32 || jx < 0 || jy < 0)
        && jx >= min_x
        && jy >= min_y
        && jx < min_x + desk_w as i32
        && jy < min_y + desk_h as i32
        && (desk_w > jpeg_w || desk_h > jpeg_h || min_x < 0 || min_y < 0)
    {
        return (jx, jy);
    }
    if jpeg_w == desk_w && jpeg_h == desk_h {
        return scale_to_desk(jx, jy, jpeg_w, jpeg_h, desk_w, desk_h);
    }
    if let Some((ox, oy)) = frame_origin {
        return (jx + ox, jy + oy);
    }
    let matched: Vec<&DisplayOutput> = outputs
        .iter()
        .filter(|o| o.w as u32 == jpeg_w && o.h as u32 == jpeg_h)
        .collect();
    let origin = if matched.len() == 1 {
        (matched[0].x, matched[0].y)
    } else if let Some(p) = matched.iter().copied().find(|o| o.primary) {
        (p.x, p.y)
    } else {
        (0, 0)
    };
    (jx + origin.0, jy + origin.1)
}

fn scale_to_desk(jx: i32, jy: i32, jpeg_w: u32, jpeg_h: u32, desk_w: u32, desk_h: u32) -> (i32, i32) {
    if jpeg_w == desk_w && jpeg_h == desk_h {
        return (jx, jy);
    }
    if jpeg_w == 0 || jpeg_h == 0 {
        return (jx, jy);
    }
    let gx = (jx as f32 * desk_w as f32 / jpeg_w as f32).round() as i32;
    let gy = (jy as f32 * desk_h as f32 / jpeg_h as f32).round() as i32;
    (gx, gy)
}

/// Top-left of a captured JPEG. A full-desktop frame is always (0, 0), even
/// when we *intended* to grim one output — gnome-screenshot and grim-without
/// `-o` must not inherit that hint or clicks land on the wrong monitor.
pub fn frame_origin_for(
    jpeg_w: u32,
    jpeg_h: u32,
    outputs: &[DisplayOutput],
    captured_output: Option<&str>,
) -> (i32, i32) {
    if let Some((dw, dh)) = virtual_desktop_size(outputs) {
        if jpeg_w == dw && jpeg_h == dh {
            return (0, 0);
        }
    }
    captured_output
        .and_then(|n| outputs.iter().find(|o| o.name == n))
        .filter(|o| o.w as u32 == jpeg_w && o.h as u32 == jpeg_h)
        .map(|o| (o.x, o.y))
        .unwrap_or((0, 0))
}

/// Prefer a non-primary output that already has a window center on it.
pub fn pick_capture_output<'a>(
    outputs: &'a [DisplayOutput],
    points: &[(i32, i32)],
) -> Option<&'a DisplayOutput> {
    for (x, y) in points {
        if let Some(o) = output_containing(outputs, *x, *y) {
            if !o.primary {
                return Some(o);
            }
        }
    }
    None
}

pub fn output_containing(outputs: &[DisplayOutput], x: i32, y: i32) -> Option<&DisplayOutput> {
    outputs.iter().find(|o| x >= o.x && y >= o.y && x < o.x + o.w && y < o.y + o.h)
}

pub fn cursor_on_output(outputs: &[DisplayOutput], x: i32, y: i32) -> Option<&DisplayOutput> {
    output_containing(outputs, x, y)
}

/// Inclusive last pixel of the virtual desktop. Overflow must not wrap to the left output.
pub fn clamp_to_desktop(x: i32, y: i32, outputs: &[DisplayOutput]) -> (i32, i32) {
    let Some((min_x, min_y, w, h)) = virtual_desktop_bounds(outputs) else {
        return (x, y);
    };
    let max_x = min_x + w as i32 - 1;
    let max_y = min_y + h as i32 - 1;
    (x.clamp(min_x, max_x), y.clamp(min_y, max_y))
}

pub fn clamp_to_output(x: i32, y: i32, output: &DisplayOutput) -> (i32, i32) {
    let max_x = output.x + output.w - 1;
    let max_y = output.y + output.h - 1;
    (x.clamp(output.x, max_x), y.clamp(output.y, max_y))
}

pub fn output_for_point(outputs: &[DisplayOutput], x: i32, y: i32) -> Option<&DisplayOutput> {
    if let Some(o) = output_containing(outputs, x, y) {
        return Some(o);
    }
    outputs.iter().min_by_key(|o| {
        let (cx, cy) = clamp_to_output(x, y, o);
        (x - cx).abs() + (y - cy).abs()
    })
}

pub fn pointer_hop_plan(
    from: Option<(i32, i32)>,
    to: (i32, i32),
    outputs: &[DisplayOutput],
) -> Vec<(i32, i32)> {
    let dest = match output_for_point(outputs, to.0, to.1) {
        Some(o) => o,
        None => return vec![clamp_to_desktop(to.0, to.1, outputs)],
    };
    let target = clamp_to_output(to.0, to.1, dest);
    let Some((fx, fy)) = from else {
        return vec![target];
    };
    let src = output_for_point(outputs, fx, fy);
    if src.map(|o| o.name.as_str()) == Some(dest.name.as_str()) {
        return vec![target];
    }
    let center = (dest.x + dest.w / 2, dest.y + dest.h / 2);
    if center == target {
        vec![target]
    } else {
        vec![center, target]
    }
}

pub fn relative_needed(
    abs_ok: bool,
    intended: (i32, i32),
    actual: Option<(i32, i32)>,
    slop: i32,
) -> Option<(i32, i32)> {
    let actual = actual?;
    if abs_ok && !pointer_slop_miss(intended, actual, slop) {
        return None;
    }
    let dx = intended.0 - actual.0;
    let dy = intended.1 - actual.1;
    if dx == 0 && dy == 0 {
        None
    } else {
        Some((dx, dy))
    }
}

/// After a failed absolute, correct from the last successful cursor when the
/// post-move read is empty.
pub fn relative_needed_or_last(
    abs_ok: bool,
    intended: (i32, i32),
    after_move: Option<(i32, i32)>,
    last_known: Option<(i32, i32)>,
    slop: i32,
) -> Option<(i32, i32)> {
    relative_needed(abs_ok, intended, after_move.or(last_known), slop)
}

pub fn should_click_after_hop(
    intended: (i32, i32),
    actual: Option<(i32, i32)>,
    slop: i32,
) -> bool {
    match actual {
        Some(a) => !pointer_slop_miss(intended, a, slop),
        None => false,
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputCalib {
    pub name: String,
    pub dx: i32,
    pub dy: i32,
    pub sx: f32,
    pub sy: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeskCalib {
    pub fingerprint: String,
    pub outputs: Vec<OutputCalib>,
}

pub fn desk_fingerprint(outputs: &[DisplayOutput]) -> String {
    let mut rows: Vec<String> = outputs
        .iter()
        .map(|o| format!("{},{},{},{},{}", o.name, o.x, o.y, o.w, o.h))
        .collect();
    rows.sort();
    rows.join(";")
}

pub fn calib_stale(calib: &DeskCalib, outputs: &[DisplayOutput]) -> bool {
    calib.fingerprint != desk_fingerprint(outputs)
}

pub fn apply_output_calib(
    x: i32,
    y: i32,
    outputs: &[DisplayOutput],
    calib: &DeskCalib,
) -> (i32, i32) {
    let Some(o) = output_for_point(outputs, x, y) else {
        return (x, y);
    };
    let Some(c) = calib
        .outputs
        .iter()
        .find(|c| c.name.eq_ignore_ascii_case(&o.name))
    else {
        return (x, y);
    };
    (x + c.dx, y + c.dy)
}

pub fn calib_probe_points(output: &DisplayOutput) -> Vec<(i32, i32)> {
    vec![
        (output.x + output.w / 2, output.y + output.h / 2),
        (output.x, output.y),
        (output.x + output.w - 1, output.y + output.h - 1),
    ]
}

pub fn median_calib_offset(pairs: &[((i32, i32), (i32, i32))], slop: i32) -> (i32, i32) {
    let mut dxs = Vec::new();
    let mut dys = Vec::new();
    for (intended, actual) in pairs {
        let dx = intended.0 - actual.0;
        let dy = intended.1 - actual.1;
        if dx.abs() + dy.abs() > slop {
            dxs.push(dx);
            dys.push(dy);
        }
    }
    (median_i32(&mut dxs), median_i32(&mut dys))
}

fn median_i32(vals: &mut [i32]) -> i32 {
    if vals.is_empty() {
        return 0;
    }
    vals.sort_unstable();
    vals[vals.len() / 2]
}

pub fn desk_status_line(calibrated: bool) -> &'static str {
    if calibrated {
        "desk: calibrated"
    } else {
        "desk: needs calibrate"
    }
}

pub fn output_scale_from_jpeg(jpeg_w: u32, jpeg_h: u32, out_w: i32, out_h: i32) -> (f32, f32) {
    if jpeg_w == 0 || jpeg_h == 0 || out_w <= 0 || out_h <= 0 {
        return (1.0, 1.0);
    }
    (jpeg_w as f32 / out_w as f32, jpeg_h as f32 / out_h as f32)
}

pub fn assemble_desk_calib(
    outputs: &[DisplayOutput],
    samples: &[(String, Vec<((i32, i32), (i32, i32))>)],
    slop: i32,
) -> Result<DeskCalib, String> {
    let mut assembled = Vec::new();
    for o in outputs {
        let Some((_, pairs)) = samples
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(&o.name))
        else {
            return Err(format!("desk not calibrated — output {} unread", o.name));
        };
        let center = calib_probe_points(o)
            .into_iter()
            .next()
            .unwrap_or((o.x, o.y));
        let Some((_, actual)) = pairs.iter().find(|(intended, _)| *intended == center) else {
            return Err(format!("desk not calibrated — output {} unread", o.name));
        };
        if pointer_slop_miss(center, *actual, slop) {
            return Err(format!("desk not calibrated — output {} unread", o.name));
        }
        let (dx, dy) = median_calib_offset(pairs, slop);
        assembled.push(OutputCalib {
            name: o.name.clone(),
            dx,
            dy,
            sx: 1.0,
            sy: 1.0,
        });
    }
    Ok(DeskCalib {
        fingerprint: desk_fingerprint(outputs),
        outputs: assembled,
    })
}

pub fn format_pointer_hint(x: i32, y: i32, monitor: Option<&str>) -> String {
    match monitor {
        Some(name) if !name.is_empty() => format!("hint {x},{y} monitor={name}"),
        _ => format!("hint {x},{y}"),
    }
}

pub fn monitor_local_to_global(
    outputs: &[DisplayOutput],
    name: &str,
    local: Option<(i32, i32)>,
) -> Option<(i32, i32)> {
    let o = outputs.iter().find(|o| o.name.eq_ignore_ascii_case(name))?;
    let (lx, ly) = local.unwrap_or((o.w / 2, o.h / 2));
    Some(clamp_to_desktop(o.x + lx, o.y + ly, outputs))
}

pub fn format_cursor_line(x: i32, y: i32, monitor: Option<&str>) -> String {
    format_cursor_line_miss(x, y, monitor, false)
}

pub fn format_cursor_line_miss(x: i32, y: i32, monitor: Option<&str>, miss: bool) -> String {
    let mut s = match monitor {
        Some(name) if !name.is_empty() => format!("cursor {x},{y} monitor={name}"),
        _ => format!("cursor {x},{y}"),
    };
    if miss {
        s.push_str(" miss");
    }
    s
}

pub fn pointer_slop_miss(intended: (i32, i32), actual: (i32, i32), slop: i32) -> bool {
    (intended.0 - actual.0).abs() + (intended.1 - actual.1).abs() > slop
}

pub fn grim_capture_args(dest: &str, output: Option<&str>) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(name) = output {
        args.push("-o".into());
        args.push(name.to_string());
    }
    args.push(dest.to_string());
    args
}

pub fn layout_prompt(
    outputs: &[DisplayOutput],
    frame_w: u32,
    frame_h: u32,
    origin_x: i32,
    origin_y: i32,
    cursor: Option<(i32, i32)>,
    hint: Option<(i32, i32)>,
) -> String {
    let mut s = String::new();
    if let Some((min_x, min_y, w, h)) = virtual_desktop_bounds(outputs) {
        s.push_str(&format!("desk: {min_x},{min_y} {w}x{h}\n"));
    }
    if !outputs.is_empty() {
        s.push_str("outputs: ");
        s.push_str(
            &outputs
                .iter()
                .map(|o| format!("{} {},{} {}x{}", o.name, o.x, o.y, o.w, o.h))
                .collect::<Vec<_>>()
                .join("; "),
        );
        s.push('\n');
    }
    if let Some((cx, cy)) = cursor {
        let mon = cursor_on_output(outputs, cx, cy).map(|o| o.name.as_str());
        let line = format_cursor_line(cx, cy, mon);
        s.push_str("cursor: ");
        s.push_str(line.strip_prefix("cursor ").unwrap_or(&line));
        s.push('\n');
    }
    if let Some((hx, hy)) = hint {
        let mon = output_for_point(outputs, hx, hy).map(|o| o.name.as_str());
        s.push_str("hint: ");
        s.push_str(
            format_pointer_hint(hx, hy, mon)
                .strip_prefix("hint ")
                .unwrap_or(""),
        );
        s.push('\n');
    }
    if frame_w > 0 && frame_h > 0 {
        s.push_str(&format!(
            "frame: {frame_w}x{frame_h} origin {origin_x},{origin_y}\n"
        ));
    }
    s
}

/// Leftover JPEG size must not appear when this turn did not capture.
pub fn windshield_frame_geom(
    captured_this_turn: bool,
    leftover: (u32, u32, i32, i32),
) -> (u32, u32, i32, i32) {
    if captured_this_turn {
        leftover
    } else {
        (0, 0, 0, 0)
    }
}

pub fn parse_xrandr_size(text: &str) -> Option<(u32, u32)> {
    for line in text.lines() {
        if let Some(rest) = line.split("current").nth(1) {
            let mut nums = rest.split(|c: char| !c.is_ascii_digit()).filter(|s| !s.is_empty());
            let w = nums.next()?.parse().ok()?;
            let h = nums.next()?.parse().ok()?;
            if w >= 64 && h >= 64 {
                return Some((w, h));
            }
        }
    }
    None
}

pub fn parse_wxh(s: &str) -> Option<(u32, u32)> {
    let (w, h) = s.split_once('x')?;
    let w = w.trim().parse().ok()?;
    let h = h.trim().parse().ok()?;
    if w >= 64 && h >= 64 {
        Some((w, h))
    } else {
        None
    }
}

pub fn x11_grab_size(xdpy: Option<&str>, xrandr: Option<&str>) -> (u32, u32) {
    parse_xdpy_size(xdpy.unwrap_or(""))
        .or_else(|| parse_xrandr_size(xrandr.unwrap_or("")))
        .unwrap_or((1920, 1080))
}

/// Several frames so x11grab / v4l2 can leave the black warmup buffer.
pub fn ffmpeg_x11_args(display: &str, w: u32, h: u32, dest: &str) -> Vec<String> {
    let size = format!("{w}x{h}");
    vec![
        "-y".into(),
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-f".into(),
        "x11grab".into(),
        "-draw_mouse".into(),
        "1".into(),
        "-video_size".into(),
        size,
        "-i".into(),
        display.to_string(),
        "-frames:v".into(),
        "12".into(),
        "-update".into(),
        "1".into(),
        "-q:v".into(),
        "5".into(),
        dest.to_string(),
    ]
}

pub fn ffmpeg_webcam_args(device: &str, dest: &str) -> Vec<String> {
    vec![
        "-y".into(),
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-f".into(),
        "v4l2".into(),
        "-i".into(),
        device.to_string(),
        "-frames:v".into(),
        "20".into(),
        "-update".into(),
        "1".into(),
        "-q:v".into(),
        "6".into(),
        dest.to_string(),
    ]
}

pub fn gnome_shell_screenshot_args(dest: &str) -> Vec<String> {
    vec![
        "call".into(),
        "--session".into(),
        "--dest".into(),
        "org.gnome.Shell.Screenshot".into(),
        "--object-path".into(),
        "/org/gnome/Shell/Screenshot".into(),
        "--method".into(),
        "org.gnome.Shell.Screenshot.Screenshot".into(),
        "false".into(),
        "false".into(),
        dest.to_string(),
    ]
}

pub fn infer_wayland_display(explicit: Option<&str>, runtime_dir: Option<&str>) -> Option<String> {
    if let Some(name) = explicit.map(str::trim).filter(|s| !s.is_empty()) {
        return Some(name.to_string());
    }
    let dir = runtime_dir.filter(|s| !s.is_empty())?;
    for name in ["wayland-0", "wayland-1", "wayland-2"] {
        let path = std::path::Path::new(dir).join(name);
        if path.exists() {
            return Some(name.to_string());
        }
    }
    None
}

pub fn session_is_wayland(wayland_display: Option<&str>, session_type: Option<&str>) -> bool {
    wayland_display.map(str::trim).is_some_and(|s| !s.is_empty())
        || session_type.map(str::trim).is_some_and(|s| s.eq_ignore_ascii_case("wayland"))
}

/// Near-black + no structure. A dark cabin UI still has variance.
pub fn frame_is_blank(mean_luma: f32, luma_var: f32) -> bool {
    mean_luma < 12.0 && luma_var < 25.0
}

pub fn luma_mean_var(pixels: &[[u8; 3]]) -> (f32, f32) {
    if pixels.is_empty() {
        return (0.0, 0.0);
    }
    let n = pixels.len() as f32;
    let mut sum = 0.0f32;
    let mut lumas = Vec::with_capacity(pixels.len());
    for p in pixels {
        let y = 0.2126 * p[0] as f32 + 0.7152 * p[1] as f32 + 0.0722 * p[2] as f32;
        lumas.push(y);
        sum += y;
    }
    let mean = sum / n;
    let var = lumas.iter().map(|y| {
        let d = y - mean;
        d * d
    }).sum::<f32>() / n;
    (mean, var)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wayland_prefers_native_tools_before_x11grab() {
        let bins = [
            "grim",
            "gnome-screenshot",
            "spectacle",
            "gdbus",
            "ffmpeg",
            "scrot",
        ];
        let plan = capture_kinds(&bins, true, true);
        assert_eq!(plan[0], CaptureKind::Grim);
        assert_eq!(plan[1], CaptureKind::GnomeScreenshot);
        let ff = plan.iter().position(|k| *k == CaptureKind::FfmpegX11).unwrap();
        let grim = plan.iter().position(|k| *k == CaptureKind::Grim).unwrap();
        assert!(grim < ff);
        assert!(plan.contains(&CaptureKind::GnomeShell));
        assert!(plan.contains(&CaptureKind::Scrot));
    }

    #[test]
    fn gnome_without_grim_does_not_start_on_ffmpeg() {
        let plan = capture_kinds(&["gdbus", "ffmpeg", "gnome-screenshot"], true, true);
        assert_eq!(plan[0], CaptureKind::GnomeScreenshot);
        assert_eq!(plan[1], CaptureKind::GnomeShell);
        assert_eq!(*plan.last().unwrap(), CaptureKind::FfmpegX11);
    }

    #[test]
    fn x11_only_still_has_a_grabber() {
        let plan = capture_kinds(&["ffmpeg"], false, true);
        assert_eq!(plan, vec![CaptureKind::FfmpegX11]);
    }

    #[test]
    fn xdpy_and_xrandr_sizes() {
        assert_eq!(
            parse_xdpy_size("dimensions:    1920x1080 pixels (508x285 millimeters)\n"),
            Some((1920, 1080))
        );
        assert_eq!(
            parse_xrandr_size("Screen 0: minimum 320 x 200, current 2560 x 1440, maximum 8192 x 8192\n"),
            Some((2560, 1440))
        );
        assert_eq!(x11_grab_size(None, None), (1920, 1080));
        assert_eq!(
            x11_grab_size(Some("dimensions: 1280x800 pixels\n"), None),
            (1280, 800)
        );
    }

    const DUAL_XRANDR: &str = "\
Screen 0: minimum 320 x 200, current 3840 x 1080, maximum 16384 x 16384
eDP-1 connected primary 1920x1080+0+0 (normal left inverted right x axis y axis) 344mm x 194mm
HDMI-1 connected 1920x1080+1920+0 (normal left inverted right x axis y axis) 480mm x 270mm
DP-1 disconnected (normal left inverted right x axis y axis)
";

    #[test]
    fn xrandr_outputs_and_jpeg_map_to_the_other_monitor() {
        let outs = parse_xrandr_outputs(DUAL_XRANDR);
        assert_eq!(outs.len(), 2);
        assert_eq!(outs[0].name, "eDP-1");
        assert!(outs[0].primary);
        assert_eq!((outs[0].x, outs[0].y, outs[0].w, outs[0].h), (0, 0, 1920, 1080));
        assert_eq!(outs[1].name, "HDMI-1");
        assert_eq!((outs[1].x, outs[1].y), (1920, 0));
        assert_eq!(virtual_desktop_size(&outs), Some((3840, 1080)));
        assert_eq!(
            image_to_global(40, 18, 1920, 1080, &outs, Some((0, 0))),
            (40, 18)
        );
        assert_eq!(
            image_to_global(40, 18, 1920, 1080, &outs, Some((1920, 0))),
            (1960, 18)
        );
        assert_eq!(image_to_global(40, 18, 1920, 1080, &outs, None), (40, 18));
        assert_eq!(
            image_to_global(2000, 40, 1920, 1080, &outs, Some((0, 0))),
            (2000, 40)
        );
        let hint = pick_capture_output(&outs, &[(2000, 40)]);
        assert_eq!(hint.map(|o| o.name.as_str()), Some("HDMI-1"));
        assert_eq!(
            grim_capture_args("/tmp/desk.png", Some("HDMI-1")),
            vec!["-o", "HDMI-1", "/tmp/desk.png"]
        );
        let glass = layout_prompt(&outs, 1920, 1080, 1920, 0, None, None);
        assert!(glass.contains("eDP-1 0,0 1920x1080"));
        assert!(glass.contains("HDMI-1 1920,0 1920x1080"));
        assert!(glass.contains("frame: 1920x1080 origin 1920,0"));
        assert!(glass.contains("desk: 0,0 3840x1080"));
        assert_eq!(
            windshield_frame_geom(false, (1920, 1080, 1920, 0)),
            (0, 0, 0, 0),
            "a skipped capture must not advertise last turn's frame"
        );
        assert_eq!(
            windshield_frame_geom(true, (1920, 1080, 1920, 0)),
            (1920, 1080, 1920, 0)
        );
    }

    const LEFT_DUAL_XRANDR: &str = "\
Screen 0: minimum 320 x 200, current 3840 x 1080, maximum 16384 x 16384
HDMI-1 connected 1920x1080+-1920+0 (normal left inverted right x axis y axis) 480mm x 270mm
eDP-1 connected primary 1920x1080+0+0 (normal left inverted right x axis y axis) 344mm x 194mm
";

    #[test]
    fn left_of_primary_stays_on_the_virtual_desktop() {
        let outs = parse_xrandr_outputs(LEFT_DUAL_XRANDR);
        assert_eq!(outs.len(), 2);
        assert_eq!((outs[0].x, outs[0].y), (-1920, 0));
        assert_eq!(
            virtual_desktop_size(&outs),
            Some((3840, 1080)),
            "a monitor at -1920 must widen the desk, not clip to the primary"
        );
        assert_eq!(
            image_to_global(40, 18, 1920, 1080, &outs, Some((-1920, 0))),
            (-1880, 18)
        );
        assert_eq!(
            image_to_global(-1880, 18, 1920, 1080, &outs, Some((-1920, 0))),
            (-1880, 18),
            "already-global left-monitor coords must not get a second origin"
        );
    }

    #[test]
    fn full_desktop_jpeg_must_not_inherit_a_single_monitor_origin() {
        let outs = parse_xrandr_outputs(DUAL_XRANDR);
        assert_eq!(
            image_to_global(40, 18, 3840, 1080, &outs, Some((1920, 0))),
            (40, 18),
            "gnome-screenshot / grim-without -o is the whole desk; +1920 would click HDMI"
        );
        assert_eq!(
            image_to_global(2000, 40, 3840, 1080, &outs, Some((1920, 0))),
            (2000, 40),
            "a pixel already on HDMI in the stitched JPEG must stay there"
        );
        assert_eq!(
            frame_origin_for(3840, 1080, &outs, Some("HDMI-1")),
            (0, 0),
            "a 3840x1080 frame is the virtual desk, not HDMI-1"
        );
        assert_eq!(
            frame_origin_for(1920, 1080, &outs, Some("HDMI-1")),
            (1920, 0)
        );
        assert_eq!(
            frame_origin_for(1920, 1080, &outs, None),
            (0, 0),
            "without a confirmed grim -o, do not guess the other monitor"
        );
    }

    const TRIPLE_XRANDR: &str = "\
Screen 0: minimum 320 x 200, current 7280 x 1440, maximum 16384 x 16384
HDMI-A-2 connected primary 2560x1440+0+0 (normal left inverted right x axis y axis) 597mm x 336mm
DP-1 connected 2800x1440+2560+0 (normal left inverted right x axis y axis) 697mm x 392mm
DP-2 connected 1920x1440+5360+0 (normal left inverted right x axis y axis) 527mm x 296mm
";

    #[test]
    fn triple_desk_clamps_past_the_right_edge_onto_dp2() {
        let outs = parse_xrandr_outputs(TRIPLE_XRANDR);
        assert_eq!(outs.len(), 3);
        assert_eq!(virtual_desktop_size(&outs), Some((7280, 1440)));
        assert_eq!(outs[2].name, "DP-2");
        assert_eq!((outs[2].x, outs[2].y, outs[2].w, outs[2].h), (5360, 0, 1920, 1440));
        assert_eq!(
            clamp_to_desktop(7285, 25, &outs),
            (7279, 25),
            "7285 wraps to the left monitor; clamp must stay on DP-2"
        );
        assert_eq!(
            cursor_on_output(&outs, 7279, 25).map(|o| o.name.as_str()),
            Some("DP-2")
        );
        assert_eq!(
            monitor_local_to_global(&outs, "DP-2", None),
            Some((6320, 720))
        );
        assert_eq!(
            monitor_local_to_global(&outs, "DP-2", Some((100, 20))),
            Some((5460, 20))
        );
        assert_eq!(format_cursor_line(7279, 25, Some("DP-2")), "cursor 7279,25 monitor=DP-2");
        assert!(!pointer_slop_miss((7279, 25), (7275, 25), 8));
        assert!(pointer_slop_miss((7279, 25), (5, 25), 8));
        assert_eq!(
            format_cursor_line_miss(5, 25, Some("HDMI-A-2"), true),
            "cursor 5,25 monitor=HDMI-A-2 miss"
        );
        let glass = layout_prompt(&outs, 1920, 1440, 5360, 0, Some((7279, 25)), None);
        assert!(glass.contains("desk: 0,0 7280x1440"), "{glass}");
        assert!(glass.contains("cursor: 7279,25 monitor=DP-2"), "{glass}");
        assert!(glass.contains("DP-2 5360,0 1920x1440"), "{glass}");
    }

    #[test]
    fn hop_plan_stages_through_the_destination_monitor_center() {
        let outs = parse_xrandr_outputs(TRIPLE_XRANDR);
        let dp1 = outs.iter().find(|o| o.name == "DP-1").unwrap();
        assert_eq!(
            clamp_to_output(7285, 25, dp1),
            (5359, 25),
            "a point past DP-1 must stay on DP-1, not wrap"
        );
        assert_eq!(
            clamp_to_output(7285, 25, outs.iter().find(|o| o.name == "DP-2").unwrap()),
            (7279, 25)
        );
        assert_eq!(
            output_for_point(&outs, 5350, 15).map(|o| o.name.as_str()),
            Some("DP-1")
        );
        assert_eq!(
            pointer_hop_plan(Some((100, 100)), (5350, 15), &outs),
            vec![(3960, 720), (5350, 15)],
            "HDMI-A-2 → DP-1 must hop via DP-1 center 3960,720"
        );
        assert_eq!(
            pointer_hop_plan(Some((5400, 40)), (5460, 20), &outs),
            vec![(5460, 20)],
            "same-output hop is just the clamped target"
        );
        assert_eq!(
            relative_needed(false, (5350, 15), Some((1920, 0)), 8),
            Some((3430, 15))
        );
        assert_eq!(relative_needed(true, (100, 20), Some((102, 20)), 8), None);
        assert_eq!(
            relative_needed(false, (5350, 15), None, 8),
            None,
            "an unread cursor must not invent a delta"
        );
        assert_eq!(
            relative_needed_or_last(false, (5350, 15), None, Some((1920, 0)), 8),
            Some((3430, 15)),
            "absolute exit 1 still relative-corrects from the last successful cursor"
        );
        assert!(should_click_after_hop((5350, 15), Some((5352, 16)), 8));
        assert!(
            !should_click_after_hop((5350, 15), Some((1920, 0)), 8),
            "a leftover miss must not click"
        );
        assert!(!should_click_after_hop((5350, 15), None, 8));
        assert_eq!(
            format_pointer_hint(5350, 15, Some("DP-1")),
            "hint 5350,15 monitor=DP-1"
        );
        let glass = layout_prompt(&outs, 0, 0, 0, 0, Some((1920, 0)), Some((5350, 15)));
        assert!(glass.contains("hint: 5350,15 monitor=DP-1"), "{glass}");
    }

    #[test]
    fn desk_calib_fingerprint_and_offsets() {
        let outs = parse_xrandr_outputs(TRIPLE_XRANDR);
        let fp = desk_fingerprint(&outs);
        assert_eq!(fp, desk_fingerprint(&outs), "same layout must be stable");
        assert!(fp.contains("HDMI-A-2"), "{fp}");
        assert!(fp.contains("DP-1"), "{fp}");
        assert!(fp.contains("DP-2"), "{fp}");
        let mut extra = outs.clone();
        extra.push(DisplayOutput {
            name: "HDMI-3".into(),
            x: 7280,
            y: 0,
            w: 1920,
            h: 1080,
            primary: false,
        });
        assert_ne!(
            desk_fingerprint(&outs),
            desk_fingerprint(&extra),
            "adding a monitor must change the fingerprint"
        );
        let identity = DeskCalib {
            fingerprint: fp.clone(),
            outputs: outs
                .iter()
                .map(|o| OutputCalib {
                    name: o.name.clone(),
                    dx: 0,
                    dy: 0,
                    sx: 1.0,
                    sy: 1.0,
                })
                .collect(),
        };
        assert!(!calib_stale(&identity, &outs));
        let mut shrunk = outs.clone();
        shrunk[2].w = 1600;
        assert!(calib_stale(&identity, &shrunk));
        let mut shifted = identity.clone();
        shifted.outputs[1].dx = -12;
        assert_eq!(
            apply_output_calib(5350, 15, &outs, &shifted),
            (5338, 15),
            "DP-1 target must pick up that output's dx"
        );
        assert_eq!(
            apply_output_calib(100, 100, &outs, &shifted),
            (100, 100),
            "HDMI-A-2 must stay unshifted"
        );
        let dp1 = outs.iter().find(|o| o.name == "DP-1").unwrap();
        assert_eq!(
            calib_probe_points(dp1),
            vec![(3960, 720), (2560, 0), (5359, 1439)]
        );
        assert_eq!(
            median_calib_offset(
                &[
                    ((3960, 720), (3972, 720)),
                    ((2560, 0), (2572, 0)),
                    ((5359, 1439), (5371, 1439)),
                ],
                8
            ),
            (-12, 0)
        );
        assert_eq!(
            median_calib_offset(&[((100, 20), (102, 20))], 8),
            (0, 0),
            "in-slop samples stay identity"
        );
        assert_eq!(desk_status_line(true), "desk: calibrated");
        assert_eq!(desk_status_line(false), "desk: needs calibrate");
        let samples: Vec<(String, Vec<((i32, i32), (i32, i32))>)> = outs
            .iter()
            .map(|o| {
                let pts = calib_probe_points(o);
                (
                    o.name.clone(),
                    pts.into_iter().map(|p| (p, p)).collect(),
                )
            })
            .collect();
        let assembled = assemble_desk_calib(&outs, &samples, 8).unwrap();
        assert_eq!(assembled.fingerprint, desk_fingerprint(&outs));
        assert!(assembled.outputs.iter().all(|c| c.dx == 0 && c.dy == 0));
        let mut unread = samples.clone();
        unread[1].1.clear();
        let err = assemble_desk_calib(&outs, &unread, 8).unwrap_err();
        assert!(err.contains("desk not calibrated"), "{err}");
        assert!(err.contains("DP-1"), "{err}");
        assert_eq!(output_scale_from_jpeg(2400, 1800, 1920, 1440), (1.25, 1.25));
        assert_eq!(output_scale_from_jpeg(1920, 1440, 1920, 1440), (1.0, 1.0));
    }

    #[test]
    fn ffmpeg_skips_the_black_warmup_frame() {
        let args = ffmpeg_x11_args(":1", 1920, 1080, "/tmp/desk.jpg");
        assert!(args.contains(&"x11grab".into()));
        assert!(args.contains(&"1920x1080".into()));
        assert!(args.contains(&":1".into()));
        let frames = args.windows(2).find(|w| w[0] == "-frames:v").unwrap();
        assert!(frames[1].parse::<u32>().unwrap() >= 8);
        assert!(args.contains(&"-update".into()));
        let cam = ffmpeg_webcam_args("/dev/video0", "/tmp/cam.jpg");
        let frames = cam.windows(2).find(|w| w[0] == "-frames:v").unwrap();
        assert!(frames[1].parse::<u32>().unwrap() >= 8);
    }

    #[test]
    fn blank_frame_is_near_black_with_no_structure() {
        let black = vec![[0, 0, 0]; 64];
        let (mean, var) = luma_mean_var(&black);
        assert!(frame_is_blank(mean, var));
        let dark_ui = [[18, 18, 20], [22, 24, 30], [40, 38, 36], [16, 16, 18]];
        let (mean, var) = luma_mean_var(&dark_ui);
        assert!(!frame_is_blank(mean, var), "mean={mean} var={var}");
        let desk = [[80, 90, 100], [200, 180, 40], [30, 120, 60], [10, 10, 10]];
        let (mean, var) = luma_mean_var(&desk);
        assert!(!frame_is_blank(mean, var));
    }

    #[test]
    fn wayland_session_and_socket_inference() {
        assert!(session_is_wayland(Some("wayland-0"), None));
        assert!(session_is_wayland(None, Some("wayland")));
        assert!(!session_is_wayland(None, Some("x11")));
        assert_eq!(
            infer_wayland_display(Some("wayland-1"), None),
            Some("wayland-1".into())
        );
        assert_eq!(infer_wayland_display(Some("  "), None), None);
        let args = gnome_shell_screenshot_args("/tmp/desk.png");
        assert!(args.contains(&"org.gnome.Shell.Screenshot.Screenshot".into()));
        assert_eq!(args.last().unwrap(), "/tmp/desk.png");
    }
}
