//! Settings → Bot Desktop: the desktop tools work on a private screen of
//! their own (`grokhub_desktop::bot_desktop`), so the user's cursor, keys
//! and windows stay theirs. Linux only, with Xvnc or Xvfb installed. The
//! switch is kept in `shell.json` and the screen starts again at launch.

use std::path::Path;

use serde::Serialize;

/// What Settings shows for the Bot Desktop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BotDesktopView {
    /// This computer can run one (Linux).
    pub supported: bool,
    /// The switch.
    pub on: bool,
    /// The screen is up: `:N`.
    pub display: Option<String>,
    /// Why it isn't running though the switch is on, or what to install.
    pub problem: Option<String>,
}

/// The folder for the screen's cookie and preview socket.
pub fn dir(config_dir: &Path) -> std::path::PathBuf {
    config_dir.join("bot-desktop")
}

#[cfg(target_os = "linux")]
mod imp {
    use super::*;
    use grokhub_desktop::bot_desktop::{self, Server};

    pub fn view(on: bool, problem: Option<String>) -> BotDesktopView {
        let display = bot_desktop::shared().map(|d| d.display_name());
        let problem = problem.or_else(|| {
            Server::installed().is_none().then(|| {
                "Needs Xvnc (tigervnc) or Xvfb: sudo apt-get install tigervnc-standalone-server"
                    .to_string()
            })
        });
        BotDesktopView {
            supported: true,
            on,
            display,
            problem,
        }
    }

    /// Starts or stops the screen. An error says why it couldn't start.
    pub fn apply(config_dir: &Path, on: bool) -> Result<(), String> {
        if on {
            bot_desktop::start_shared(&dir(config_dir)).map(|_| ())
        } else {
            bot_desktop::stop_shared();
            Ok(())
        }
    }

    pub fn stop() {
        bot_desktop::stop_shared();
    }

    /// The screen as it is now, scaled for the preview.
    pub fn frame() -> Option<String> {
        let desk = bot_desktop::shared()?;
        let shot = desk.backend().ok()?.screenshot("all").ok()?;
        preview_jpeg(&shot.bytes)
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    use super::*;

    pub fn view(_on: bool, _problem: Option<String>) -> BotDesktopView {
        BotDesktopView {
            supported: false,
            on: false,
            display: None,
            problem: None,
        }
    }

    pub fn apply(_config_dir: &Path, on: bool) -> Result<(), String> {
        if on {
            Err("Bot Desktop runs on Linux only.".into())
        } else {
            Ok(())
        }
    }

    pub fn stop() {}

    pub fn frame() -> Option<String> {
        None
    }
}

pub use imp::{apply, frame, stop, view};

/// How wide the preview is drawn.
pub const PREVIEW_WIDTH: u32 = 640;

/// `bytes` (any image) as a JPEG data URL at most [`PREVIEW_WIDTH`] wide.
pub fn preview_jpeg(bytes: &[u8]) -> Option<String> {
    use base64::Engine as _;
    let img = image::load_from_memory(bytes).ok()?;
    let img = if img.width() > PREVIEW_WIDTH {
        img.resize(
            PREVIEW_WIDTH,
            u32::MAX,
            image::imageops::FilterType::Triangle,
        )
    } else {
        img
    };
    let mut out = std::io::Cursor::new(Vec::new());
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 70)
        .encode_image(&img.to_rgb8())
        .ok()?;
    Some(format!(
        "data:image/jpeg;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(out.into_inner())
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_preview_is_a_jpeg_scaled_to_its_width() {
        let big = image::RgbImage::from_pixel(1280, 800, image::Rgb([200, 30, 30]));
        let mut png = std::io::Cursor::new(Vec::new());
        big.write_to(&mut png, image::ImageFormat::Png).unwrap();
        let url = preview_jpeg(png.get_ref()).expect("a preview");
        let b64 = url
            .strip_prefix("data:image/jpeg;base64,")
            .expect("jpeg url");
        use base64::Engine as _;
        let jpeg = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .unwrap();
        let back = image::load_from_memory(&jpeg).unwrap();
        assert_eq!((back.width(), back.height()), (640, 400));
        assert_eq!(preview_jpeg(b"not an image"), None);
    }

    #[test]
    fn the_screen_files_live_in_their_own_config_folder() {
        assert_eq!(
            dir(Path::new("/home/user/.config/GrokHub")),
            Path::new("/home/user/.config/GrokHub/bot-desktop")
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn the_switch_starts_and_stops_the_screen() {
        if grokhub_desktop::bot_desktop::Server::installed().is_none() {
            eprintln!("skipped: no Xvnc or Xvfb");
            return;
        }
        let config =
            std::env::temp_dir().join(format!("grokhub-bot-screen-{}", std::process::id()));
        assert_eq!(view(false, None).display, None);
        apply(&config, true).expect("starts");
        let on = view(true, None);
        assert!(on.supported && on.on && on.problem.is_none());
        let shown = on.display.expect("a display");
        assert!(shown.starts_with(':'), "{shown}");
        assert!(dir(&config).is_dir());
        let shot = frame().expect("a frame while it runs");
        assert!(
            shot.starts_with("data:image/jpeg;base64,"),
            "{}",
            &shot[..40]
        );
        apply(&config, false).unwrap();
        assert_eq!(frame(), None);
        assert_eq!(
            view(false, None),
            BotDesktopView {
                supported: true,
                on: false,
                display: None,
                problem: None,
            }
        );
        let _ = std::fs::remove_dir_all(&config);
    }
}
