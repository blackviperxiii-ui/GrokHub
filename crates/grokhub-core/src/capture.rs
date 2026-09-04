//! Desktop / webcam grab plan. X11 root + one ffmpeg frame is a black void on Wayland.

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
    bins.contains(&name)
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
        let glass = layout_prompt(&outs, 1920, 1080, 1920, 0, None);
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
        let glass = layout_prompt(&outs, 1920, 1440, 5360, 0, Some((7279, 25)));
        assert!(glass.contains("desk: 0,0 7280x1440"), "{glass}");
        assert!(glass.contains("cursor: 7279,25 monitor=DP-2"), "{glass}");
        assert!(glass.contains("DP-2 5360,0 1920x1440"), "{glass}");
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
