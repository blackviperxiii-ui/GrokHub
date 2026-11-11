//! "Check my audio" in the window app, as the cabin does it: `/audiocheck
//! [input]` lists the outputs and inputs, listens to the chosen input for a
//! few seconds and says what is wrong (level, clipping, silence, dropouts,
//! crackle). Asking for it is the consent; the clip stays in memory, is never
//! saved or sent, and nothing on the computer is changed.

use crate::screen_record::{push_event, Event, Line};
use grokhub_core::audio_check as ac;
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
/// recorded as "default" so any recorder can take it.
pub fn run_check(
    want: &str,
    list: impl FnOnce() -> Devices,
    record: impl FnOnce(Option<&str>) -> Result<(u32, Vec<u8>), String>,
) -> Result<ac::AudioReport, String> {
    let (outputs, inputs) = list();
    let input = ac::pick_input(&inputs, want)?;
    let target = input.filter(|d| !d.is_default).map(|d| d.name.as_str());
    let label = input
        .map(ac::AudioDevice::label)
        .unwrap_or("the default input");
    let (rate, pcm) = record(target).map_err(|e| format!("Couldn't record from {label}: {e}"))?;
    let levels = ac::measure(&ac::pcm_samples(&pcm), rate);
    Ok(ac::audio_report(&outputs, &inputs, input, &levels))
}

/// `/audiocheck [input]`. The report arrives as an event when the clip is done.
pub fn start(want: &str, remote: bool) -> Result<String, String> {
    if remote {
        return Err("The audio check works on your computer.".into());
    }
    {
        let mut at = live();
        if at.is_some() {
            return Err(ac::ALREADY_CHECKING.into());
        }
        *at = Some(Instant::now());
    }
    let want = want.to_string();
    std::thread::spawn(move || {
        let done = run_check(&want, list_devices, record_clip);
        *live() = None;
        push_event(match done {
            Ok(report) => Event {
                kind: "audio",
                ok: true,
                text: report.text,
            },
            Err(why) => Event {
                kind: "audio",
                ok: false,
                text: why,
            },
        });
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
        let report = run_check("", devices, |d| {
            asked.push(d.map(str::to_string));
            Ok((48_000, clipped_pcm()))
        })
        .unwrap();
        assert_eq!(asked, vec![None]);
        assert_eq!(
            report.title,
            "Audio check: Yeti Stereo Microphone input clipping at 0.0 dBFS; default output is Navi 21 HDMI / DisplayPort"
        );
        let mut asked = Vec::new();
        run_check("built-in", devices, |d| {
            asked.push(d.map(str::to_string));
            Ok((48_000, clipped_pcm()))
        })
        .unwrap();
        assert_eq!(asked, vec![Some("builtin".to_string())]);
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
    fn a_browser_cannot_open_the_microphone() {
        assert_eq!(
            start("", true).unwrap_err(),
            "The audio check works on your computer."
        );
        assert!(listening().is_none());
    }
}
