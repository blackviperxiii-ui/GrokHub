//! "Check my audio" in the window app, as the cabin does it: `/audiocheck
//! [input]` lists the outputs and inputs, listens to the chosen input for a
//! few seconds and says what is wrong (level, clipping, silence, dropouts,
//! crackle). Asking for it is the consent; the clip stays in memory, is never
//! saved or sent, and nothing on the computer is changed. The report also
//! lands on a Pulse card that names the input, as the cabin's did.

use crate::screen_record::{push_event, Event, Line};
use grokhub_core::audio_check as ac;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Instant;

type Devices = (Vec<ac::AudioDevice>, Vec<ac::AudioDevice>);

static LIVE: Mutex<Option<Instant>> = Mutex::new(None);

fn live() -> std::sync::MutexGuard<'static, Option<Instant>> {
    LIVE.lock().unwrap_or_else(|e| e.into_inner())
}

/// While the microphone is open, the top bar says so.
pub fn listening() -> Option<Line> {
    let started = (*live())?;
    let secs = started.elapsed().as_secs().min(u64::from(ac::CLIP_SECS));
    Some(Line {
        secs,
        text: format!(
            "● Audio check: listening 0:{secs:02} of 0:{:02}",
            ac::CLIP_SECS
        ),
    })
}

/// The check: pick the input, record it, measure it. The default input is
/// recorded as "default" so any recorder can take it. Comes back with the
/// input's device name ("" when no input was listed) for the card.
pub fn run_check(
    want: &str,
    list: impl FnOnce() -> Devices,
    record: impl FnOnce(Option<&str>) -> Result<(u32, Vec<u8>), String>,
) -> Result<(String, ac::AudioReport), String> {
    let (outputs, inputs) = list();
    let input = ac::pick_input(&inputs, want)?;
    let target = input.filter(|d| !d.is_default).map(|d| d.name.as_str());
    let label = input
        .map(ac::AudioDevice::label)
        .unwrap_or("the default input");
    let (rate, pcm) = record(target).map_err(|e| format!("Couldn't record from {label}: {e}"))?;
    let levels = ac::measure(&ac::pcm_samples(&pcm), rate);
    let name = input.map(|d| d.name.clone()).unwrap_or_default();
    Ok((name, ac::audio_report(&outputs, &inputs, input, &levels)))
}

/// A finished check: a good one also goes on a Pulse card naming the input,
/// posted before the event so Pulse has it when the bell rings. A new check
/// of the same input folds into that input's card. A failed check posts no
/// card. `chat` is the chat that asked, for the card's Open.
fn finish(
    config_dir: &Path,
    chat: &str,
    done: Result<(String, ac::AudioReport), String>,
    now: u64,
) -> Event {
    match done {
        Ok((input, report)) => {
            let mut card = grokhub_core::audio_check_card(
                &input,
                &report.card_title,
                &report.summary,
                chat,
                now,
            );
            // The full report opens under the card, with Fix it when it has steps.
            card.pulse.report = Some(grokhub_core::fix_plan::diagnosis_report(
                &report.card_title,
                &report.text,
                &grokhub_core::fix_plan::from_audio(&report),
            ));
            if let Err(why) = crate::pulse::add_post(config_dir, card) {
                eprintln!("audio check card: {why}");
            }
            Event {
                kind: "audio",
                chat: String::new(),
                ok: true,
                text: report.text,
            }
        }
        Err(why) => Event {
            kind: "audio",
            chat: String::new(),
            ok: false,
            text: why,
        },
    }
}

/// The recorders and the device list are Linux tools (`parecord`, `pw-record`,
/// `pactl`), so only Linux can run the check for now.
fn supported(os: &str) -> bool {
    os == "linux"
}

/// `/audiocheck [input]` typed in `chat`. The report arrives as an event when
/// the clip is done, and on a Pulse card.
pub fn start(config_dir: &Path, chat: &str, want: &str, remote: bool) -> Result<String, String> {
    start_on(std::env::consts::OS, config_dir, chat, want, remote)
}

fn start_on(
    os: &str,
    config_dir: &Path,
    chat: &str,
    want: &str,
    remote: bool,
) -> Result<String, String> {
    if remote {
        return Err("The audio check works on your computer.".into());
    }
    if !supported(os) {
        return Err("The audio check works on Linux for now. On this computer, use the system's sound settings to test the microphone.".into());
    }
    {
        let mut at = live();
        if at.is_some() {
            return Err(ac::ALREADY_CHECKING.into());
        }
        *at = Some(Instant::now());
    }
    let want = want.to_string();
    let (dir, chat): (PathBuf, String) = (config_dir.to_path_buf(), chat.to_string());
    std::thread::spawn(move || {
        let done = run_check(&want, list_devices, record_clip);
        *live() = None;
        push_event(finish(&dir, &chat, done, grokhub_core::now_ms()));
    });
    Ok(format!(
        "Checking your audio: finding the input, then listening for {} seconds. Talk or play something at your usual loudness. The clip stays in memory and isn't saved or sent.",
        ac::CLIP_SECS
    ))
}

#[cfg(desktop)]
fn tool_path(bin: &str) -> Option<std::path::PathBuf> {
    let names: Vec<String> = if cfg!(windows) {
        vec![format!("{bin}.exe"), bin.to_string()]
    } else {
        vec![bin.to_string()]
    };
    std::env::split_paths(&std::env::var_os("PATH")?)
        .flat_map(|dir| names.iter().map(move |n| dir.join(n)))
        .find(|p| p.is_file())
}

#[cfg(desktop)]
fn quiet(cmd: &mut std::process::Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

/// `pactl <args>` output, `None` where pactl isn't installed or fails.
#[cfg(desktop)]
fn pactl_text(args: &[&str]) -> Option<String> {
    let bin = tool_path("pactl")?;
    let mut cmd = std::process::Command::new(bin);
    cmd.args(args)
        .env("LC_ALL", "C")
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    quiet(&mut cmd);
    let out = cmd.output().ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Outputs and inputs from `pactl`, empty where it isn't available.
#[cfg(desktop)]
fn list_devices() -> Devices {
    let Some(info) = pactl_text(&["info"]) else {
        return (Vec::new(), Vec::new());
    };
    let (sink, source) = ac::parse_pactl_defaults(&info);
    let list = |what: &str, default: Option<&str>| {
        pactl_text(&["list", what])
            .map(|t| ac::parse_pactl_list(&t, default))
            .unwrap_or_default()
    };
    (
        list("sinks", sink.as_deref()),
        list("sources", source.as_deref()),
    )
}

/// `CLIP_SECS` of raw s16le mono from `device` (`None` = the default input),
/// kept in memory only. The recorder is stopped once it has enough, or at the
/// deadline if it stalls.
#[cfg(desktop)]
fn record_clip(device: Option<&str>) -> Result<(u32, Vec<u8>), String> {
    use std::io::Read;
    let picked = ac::CLIP_RECORDERS.iter().find_map(|b| {
        let path = tool_path(b)?;
        ac::clip_argv(b, device).map(|argv| (*b, path, argv))
    });
    let Some((bin, path, argv)) = picked else {
        return Err(if device.is_some() {
            "No recorder here can pick an input: install pulseaudio-utils (parecord) or pipewire (pw-record).".into()
        } else {
            "No recorder found: install pulseaudio-utils (parecord), pipewire (pw-record), ffmpeg or alsa-utils (arecord).".into()
        });
    };
    let want = (ac::CLIP_RATE * 2 * ac::CLIP_SECS) as usize;
    let mut cmd = std::process::Command::new(path);
    cmd.args(&argv)
        .env("LC_ALL", "C")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    quiet(&mut cmd);
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("{bin} didn't start: {e}"))?;
    let mut out = child.stdout.take().ok_or("the recorder gave no output")?;
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::with_capacity(want);
        let mut chunk = [0u8; 8192];
        while buf.len() < want {
            match out.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
            }
        }
        buf.truncate(want);
        buf
    });
    let deadline = Instant::now() + std::time::Duration::from_secs(u64::from(ac::CLIP_SECS) + 3);
    while !reader.is_finished() && Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
    let pcm = reader.join().unwrap_or_default();
    if pcm.len() < 64 {
        return Err(format!("{bin} recorded no sound data"));
    }
    Ok((ac::CLIP_RATE, pcm))
}

#[cfg(not(desktop))]
fn list_devices() -> Devices {
    (Vec::new(), Vec::new())
}

#[cfg(not(desktop))]
fn record_clip(_device: Option<&str>) -> Result<(u32, Vec<u8>), String> {
    Err("The audio check runs on a computer.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(name: &str, desc: &str, default: bool) -> ac::AudioDevice {
        ac::AudioDevice {
            name: name.into(),
            description: desc.into(),
            muted: false,
            volume_pct: Some(100),
            is_default: default,
        }
    }

    fn devices() -> Devices {
        (
            vec![dev("hdmi", "Navi 21 HDMI / DisplayPort", true)],
            vec![
                dev("yeti", "Yeti Stereo Microphone", true),
                dev("builtin", "Built-in Microphone", false),
            ],
        )
    }

    /// Full-scale square wave: clipping all the way.
    fn clipped_pcm() -> Vec<u8> {
        (0..48_000)
            .flat_map(|i| {
                if (i / 50) % 2 == 0 {
                    32_767i16
                } else {
                    -32_768
                }
                .to_le_bytes()
            })
            .collect()
    }

    #[test]
    fn the_default_input_is_recorded_as_default_and_a_named_one_by_name() {
        let mut asked = Vec::new();
        let (name, report) = run_check("", devices, |d| {
            asked.push(d.map(str::to_string));
            Ok((48_000, clipped_pcm()))
        })
        .unwrap();
        assert_eq!(asked, vec![None]);
        assert_eq!(name, "yeti");
        assert_eq!(
            report.title,
            "Audio check: Yeti Stereo Microphone input clipping at 0.0 dBFS; default output is Navi 21 HDMI / DisplayPort"
        );
        let mut asked = Vec::new();
        let (name, _) = run_check("built-in", devices, |d| {
            asked.push(d.map(str::to_string));
            Ok((48_000, clipped_pcm()))
        })
        .unwrap();
        assert_eq!(asked, vec![Some("builtin".to_string())]);
        assert_eq!(name, "builtin");
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-shell-audio-card-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn audio_cards(dir: &Path) -> Vec<grokhub_core::UpdateCard> {
        crate::pulse::cards(dir)
            .unwrap()
            .into_iter()
            .filter(|c| c.source_id.starts_with("audiocheck:"))
            .collect()
    }

    const NOW: u64 = 1_800_000_000_000;

    #[test]
    fn a_finished_check_posts_a_card_naming_the_device() {
        let dir = temp("posts");
        let _config = grokhub_agent::perm::ConfigGuard::set(&dir);
        let done = run_check("", devices, |_| Ok((48_000, clipped_pcm())));
        let text = done.as_ref().unwrap().1.text.clone();
        assert_eq!(
            finish(&dir, "chat-1", done, NOW),
            Event {
                kind: "audio",
                chat: String::new(),
                ok: true,
                text: text.clone()
            }
        );
        let cards = audio_cards(&dir);
        assert_eq!(cards.len(), 1);
        let card = &cards[0];
        assert_eq!(card.source_id, "audiocheck:yeti");
        assert_eq!(
            card.title,
            "Audio check: Yeti Stereo Microphone input clipping at 0.0 dBFS"
        );
        assert_eq!(
            card.body.as_deref(),
            Some("Lower Yeti Stereo Microphone's input volume or gain knob until loud speech peaks around -6 dBFS. Default output: Navi 21 HDMI / DisplayPort.")
        );
        assert_eq!(
            card.action,
            Some(grokhub_core::UpdateAction::OpenSession {
                thread_id: "chat-1".into()
            })
        );
        let page = crate::pulse::peek(&dir, NOW).unwrap();
        let post = page
            .feed
            .iter()
            .find(|p| p.title == card.title)
            .expect("on the feed");
        assert_eq!(post.source, "Audio check");
        // It opens to the full report, and Fix it asks the steps in its chat.
        let report = card.pulse.report.as_ref().expect("a report");
        // Its first line is the card's title, which the card already shows.
        let (first, opened) = text.trim().split_once("\n\n").unwrap();
        assert!(first.starts_with(&card.title), "{first}");
        assert_eq!(report.details, opened);
        assert_eq!(post.chips[0].label, "Fix it");
        let seed = crate::pulse::offer(&dir, &card.id, 0, NOW).unwrap();
        assert_eq!(
            seed.text,
            "Fix what my audio check found, using the report's steps, one at a time:\n1. Lower Yeti Stereo Microphone's input volume or gain knob until loud speech peaks around -6 dBFS. (Undo: Lower the volume or gain again to where it was.)\n\nBefore each step, say what it will change. Ask me before anything that changes settings, files or devices, and stop if a step fails. Tell me which steps I have to do by hand."
        );
    }

    #[test]
    fn a_second_check_of_the_same_input_folds_into_its_card() {
        let dir = temp("again");
        let _config = grokhub_agent::perm::ConfigGuard::set(&dir);
        finish(
            &dir,
            "chat-1",
            run_check("", devices, |_| Ok((48_000, clipped_pcm()))),
            NOW,
        );
        let quiet: Vec<u8> = (0..48_000)
            .flat_map(|i| (((i % 40) as i16 - 20) * 400).to_le_bytes())
            .collect();
        finish(
            &dir,
            "chat-2",
            run_check("", devices, |_| Ok((48_000, quiet))),
            NOW + 60_000,
        );
        let cards = audio_cards(&dir);
        assert_eq!(cards.len(), 1, "{cards:?}");
        assert_eq!(
            cards[0].title,
            "Audio check: Yeti Stereo Microphone input sounds fine (peak -12.2 dBFS)"
        );
        assert_eq!(cards[0].runs, 2);
        assert_eq!(cards[0].created_at, NOW + 60_000);
        // The clean check's report replaces the clipping one, so no Fix it is left.
        let report = cards[0].pulse.report.as_ref().expect("a report");
        assert!(
            report.details.contains("No problems found."),
            "{}",
            report.details
        );
        assert!(report.offers.is_empty(), "{:?}", report.offers);
        assert_eq!(
            cards[0].action,
            Some(grokhub_core::UpdateAction::OpenSession {
                thread_id: "chat-2".into()
            })
        );
    }

    #[test]
    fn a_failed_check_posts_no_card() {
        let dir = temp("failed");
        assert_eq!(
            finish(
                &dir,
                "chat-1",
                Err("No input matches \"rode\".".into()),
                NOW
            ),
            Event {
                kind: "audio",
                chat: String::new(),
                ok: false,
                text: "No input matches \"rode\".".into()
            }
        );
        assert_eq!(audio_cards(&dir), Vec::new());
    }

    #[test]
    fn an_unknown_input_lists_the_ones_there_are_and_records_nothing() {
        let err = run_check("webcam", devices, |_| panic!("no recording")).unwrap_err();
        assert_eq!(
            err,
            "No input matches \"webcam\". Inputs: Yeti Stereo Microphone, Built-in Microphone."
        );
    }

    #[test]
    fn a_recorder_that_fails_names_the_input() {
        let err = run_check("", devices, |_| Err("no recorder found".into())).unwrap_err();
        assert_eq!(
            err,
            "Couldn't record from Yeti Stereo Microphone: no recorder found"
        );
    }

    #[test]
    fn windows_and_macos_are_told_the_check_is_linux_only_and_nothing_listens() {
        assert!(supported("linux"));
        assert!(!supported("windows"));
        assert!(!supported("macos"));
        for os in ["windows", "macos"] {
            assert_eq!(
                start_on(os, &std::env::temp_dir(), "chat-1", "", false).unwrap_err(),
                "The audio check works on Linux for now. On this computer, use the system's sound settings to test the microphone.",
                "{os}"
            );
            assert!(listening().is_none(), "{os}");
        }
        // A browser is told about the computer first, whatever that computer runs.
        assert_eq!(
            start_on("windows", &std::env::temp_dir(), "chat-1", "", true).unwrap_err(),
            "The audio check works on your computer."
        );
    }

    #[test]
    fn a_browser_cannot_open_the_microphone() {
        let dir = temp("browser");
        assert_eq!(
            start(&dir, "chat-1", "", true).unwrap_err(),
            "The audio check works on your computer."
        );
        assert!(listening().is_none());
        assert_eq!(audio_cards(&dir), Vec::new());
    }
}
