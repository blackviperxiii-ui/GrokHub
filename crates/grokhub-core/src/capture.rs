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
