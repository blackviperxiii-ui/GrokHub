//! "Check my audio" (card 11, Phase B). Lists the outputs and inputs PipeWire
//! knows (through `pactl`), records a short clip from the chosen input and
//! measures its level, clipping, silence, dropouts and crackle. The report
//! names the device and the problem, with fixes to try. Nothing on the
//! computer is changed, and the clip is never saved or sent anywhere.

pub const CLIP_SECS: u32 = 5;
pub const CLIP_RATE: u32 = 48_000;
pub const ALREADY_CHECKING: &str = "Already checking your audio.";
/// Below this a sample counts as no sound at all.
pub const FLOOR_DBFS: f32 = -96.0;
/// A peak at or above this is hot enough to clip.
pub const CLIP_DBFS: f32 = -0.5;
/// A clip whose loudest moment stays under this heard nothing.
pub const SILENT_DBFS: f32 = -60.0;
/// A clip whose loudest moment stays under this is too quiet to use.
pub const QUIET_DBFS: f32 = -30.0;
/// An input volume under this percent is turned too far down.
pub const LOW_VOLUME_PCT: u32 = 20;
/// A run of exact zeros this long (ms) between sounds is a dropout.
pub const DROPOUT_MS: u32 = 10;
/// A jump between neighboring samples bigger than this is a click.
pub const CLICK_JUMP: i32 = 20_000;
/// This many clicks in one clip is crackle, not chance.
pub const CRACKLE_CLICKS: usize = 3;

/// One output (sink) or input (source) as `pactl list` shows it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AudioDevice {
    pub name: String,
    pub description: String,
    pub muted: bool,
    pub volume_pct: Option<u32>,
    pub is_default: bool,
}

impl AudioDevice {
    pub fn label(&self) -> &str {
        if self.description.is_empty() {
            &self.name
        } else {
            &self.description
        }
    }
}

/// `pactl info`: the default output and input names.
pub fn parse_pactl_defaults(info: &str) -> (Option<String>, Option<String>) {
    let field = |key: &str| {
        info.lines()
            .find_map(|l| l.trim().strip_prefix(key))
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
    };
    (field("Default Sink:"), field("Default Source:"))
}

/// `pactl list sinks` or `pactl list sources` (long form, `LC_ALL=C`).
/// Monitor sources record an output, not a microphone, so they are dropped.
pub fn parse_pactl_list(text: &str, default: Option<&str>) -> Vec<AudioDevice> {
    let mut out = Vec::new();
    let mut cur: Option<(AudioDevice, bool)> = None;
    let finish = |cur: Option<(AudioDevice, bool)>, out: &mut Vec<AudioDevice>| {
        if let Some((mut dev, monitor)) = cur {
            if !monitor && !dev.name.is_empty() {
                dev.is_default = default == Some(dev.name.as_str());
                out.push(dev);
            }
        }
    };
    for line in text.lines() {
        if line.starts_with("Sink #") || line.starts_with("Source #") {
            finish(cur.take(), &mut out);
            cur = Some((AudioDevice::default(), false));
            continue;
        }
        // Only the block's own fields: one tab deep, not its properties or ports.
        let Some(field) = line.strip_prefix('\t').filter(|l| !l.starts_with('\t')) else {
            continue;
        };
        let Some((dev, monitor)) = cur.as_mut() else {
            continue;
        };
        let Some((key, value)) = field.split_once(':') else {
            continue;
        };
        let value = value.trim();
        match key {
            "Name" => {
                dev.name = value.to_string();
                *monitor |= value.ends_with(".monitor");
            }
            "Description" => dev.description = value.to_string(),
            "Mute" => dev.muted = value == "yes",
            "Volume" => dev.volume_pct = first_percent(value),
            "Monitor of Sink" => *monitor |= value != "n/a",
            _ => {}
        }
    }
    finish(cur.take(), &mut out);
    out
}

fn first_percent(volume: &str) -> Option<u32> {
    let pct = volume.find('%')?;
    let digits: String = volume[..pct]
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    digits.parse().ok()
}

/// The input to record. No name: the default input (or the first one).
/// A name matches an input's description or name, ignoring case and
/// treating `_`, `-` and `.` as spaces ("qa usb" finds "QA_USB_Microphone").
/// `Ok(None)` means GrokHub couldn't list inputs and records the default.
pub fn pick_input<'a>(inputs: &'a [AudioDevice], want: &str) -> Result<Option<&'a AudioDevice>, String> {
    let want = want.trim();
    if want.is_empty() {
        return Ok(inputs.iter().find(|d| d.is_default).or(inputs.first()));
    }
    if inputs.is_empty() {
        return Err(format!(
            "GrokHub couldn't list your inputs, so it can't pick \"{want}\". Run /audiocheck on its own to check the default input."
        ));
    }
    let loose = |s: &str| s.to_lowercase().replace(['_', '-', '.'], " ");
    let low = loose(want);
    inputs
        .iter()
        .find(|d| loose(&d.description).contains(&low) || loose(&d.name).contains(&low))
        .map(Some)
        .ok_or_else(|| {
            let names: Vec<&str> = inputs.iter().map(AudioDevice::label).collect();
            format!("No input matches \"{want}\". Inputs: {}.", names.join(", "))
        })
}

/// Recorders tried in order. All write raw s16le mono at 48 kHz to stdout.
pub const CLIP_RECORDERS: &[&str] = &["parecord", "pw-record", "ffmpeg", "arecord"];

/// Arguments for `bin` to record from `device` (`None` = the default input).
/// `arecord` can only use the default input, so it returns `None` for a named one.
pub fn clip_argv(bin: &str, device: Option<&str>) -> Option<Vec<String>> {
    let rate = CLIP_RATE.to_string();
    let secs = CLIP_SECS.to_string();
    let mut argv: Vec<String> = match bin {
        "parecord" => vec!["--raw".into(), "--format=s16le".into(), format!("--rate={rate}"), "--channels=1".into()],
        "pw-record" => vec!["--rate".into(), rate, "--channels".into(), "1".into(), "--format".into(), "s16".into()],
        "ffmpeg" => {
            return Some(
                [
                    "-hide_banner", "-loglevel", "error", "-f", "pulse", "-i", device.unwrap_or("default"),
                    "-t", &secs, "-ac", "1", "-ar", &rate, "-f", "s16le", "-",
                ]
                .iter()
                .map(|s| s.to_string())
                .collect(),
            )
        }
        "arecord" if device.is_none() => {
            return Some(
                ["-q", "-t", "raw", "-f", "S16_LE", "-r", &rate, "-c", "1", "-d", &secs]
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            )
        }
        _ => return None,
    };
    match (bin, device) {
        ("parecord", Some(d)) => argv.push(format!("--device={d}")),
        ("pw-record", Some(d)) => argv.extend(["--target".to_string(), d.to_string()]),
        _ => {}
    }
    if bin == "pw-record" {
        argv.push("-".into());
    }
    Some(argv)
}

/// Raw s16le bytes to samples (a trailing odd byte is dropped).
pub fn pcm_samples(bytes: &[u8]) -> Vec<i16> {
    bytes.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect()
}

/// A 16-bit PCM WAV: its rate and the first channel's samples.
pub fn wav_samples(bytes: &[u8]) -> Option<(u32, Vec<i16>)> {
    if bytes.len() < 12 || &bytes[..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return None;
    }
    let (mut rate, mut channels, mut bits) = (0u32, 0usize, 0u16);
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let len = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().ok()?) as usize;
        let body = bytes.get(at + 8..(at + 8 + len).min(bytes.len()))?;
        if id == b"fmt " && body.len() >= 16 {
            channels = u16::from_le_bytes([body[2], body[3]]) as usize;
            rate = u32::from_le_bytes(body[4..8].try_into().ok()?);
            bits = u16::from_le_bytes([body[14], body[15]]);
        } else if id == b"data" {
            if bits != 16 || channels == 0 || rate == 0 {
                return None;
            }
            let all = pcm_samples(body);
            return Some((rate, all.into_iter().step_by(channels).collect()));
        }
        at += 8 + len + (len & 1);
    }
    None
}

fn dbfs(level: f64) -> f32 {
    if level <= 0.0 {
        return FLOOR_DBFS;
    }
    ((20.0 * (level / 32768.0).log10()) as f32).max(FLOOR_DBFS)
}

/// What the clip sounded like.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Levels {
    pub secs: f32,
    pub peak_dbfs: f32,
    pub rms_dbfs: f32,
    /// Samples at or above `CLIP_DBFS`.
    pub clipped: usize,
    pub clipped_pct: f32,
    /// Runs of exact zeros of at least `DROPOUT_MS` between sounds.
    pub dropouts: usize,
    /// Jumps bigger than `CLICK_JUMP` between neighboring samples.
    pub clicks: usize,
}

pub fn measure(samples: &[i16], rate: u32) -> Levels {
    let n = samples.len();
    let peak = samples.iter().map(|&s| (s as i32).unsigned_abs()).max().unwrap_or(0);
    let sum_sq: f64 = samples.iter().map(|&s| (s as f64) * (s as f64)).sum();
    let rms = if n == 0 { 0.0 } else { (sum_sq / n as f64).sqrt() };
    let hot = (32768.0 * 10f64.powf(CLIP_DBFS as f64 / 20.0)) as u32;
    let clipped = samples.iter().filter(|&&s| (s as i32).unsigned_abs() >= hot).count();
    let min_gap = (rate * DROPOUT_MS / 1000).max(1) as usize;
    let mut dropouts = 0;
    let mut run = 0usize;
    let mut heard = false;
    for &s in samples {
        if s == 0 {
            run += 1;
            continue;
        }
        if heard && run >= min_gap {
            dropouts += 1;
        }
        heard = true;
        run = 0;
    }
    let clicks = samples
        .windows(2)
        .filter(|w| {
            let (a, b) = (w[0] as i32, w[1] as i32);
            (b - a).abs() > CLICK_JUMP && a.unsigned_abs() < hot && b.unsigned_abs() < hot
        })
        .count();
    Levels {
        secs: if rate == 0 { 0.0 } else { n as f32 / rate as f32 },
        peak_dbfs: dbfs(peak as f64),
        rms_dbfs: dbfs(rms),
        clipped,
        clipped_pct: if n == 0 { 0.0 } else { clipped as f32 * 100.0 / n as f32 },
        dropouts,
        clicks,
    }
}

/// One thing wrong, most serious first.
#[derive(Debug, Clone, PartialEq)]
pub enum Problem {
    Muted,
    VolumeLow(u32),
    Silent,
    Clipping { peak_dbfs: f32, pct: f32 },
    TooQuiet { peak_dbfs: f32 },
    Dropouts(usize),
    Crackle(usize),
    OutputMuted,
}

pub fn find_problems(input: Option<&AudioDevice>, output: Option<&AudioDevice>, levels: &Levels) -> Vec<Problem> {
    let mut out = Vec::new();
    if input.is_some_and(|d| d.muted) {
        out.push(Problem::Muted);
    }
    if let Some(v) = input.and_then(|d| d.volume_pct).filter(|&v| v < LOW_VOLUME_PCT) {
        out.push(Problem::VolumeLow(v));
    }
    if levels.peak_dbfs < SILENT_DBFS {
        out.push(Problem::Silent);
    } else if levels.peak_dbfs >= CLIP_DBFS {
        out.push(Problem::Clipping { peak_dbfs: levels.peak_dbfs, pct: levels.clipped_pct });
    } else if levels.peak_dbfs < QUIET_DBFS {
        out.push(Problem::TooQuiet { peak_dbfs: levels.peak_dbfs });
    }
    if levels.dropouts > 0 {
        out.push(Problem::Dropouts(levels.dropouts));
    }
    if levels.clicks >= CRACKLE_CLICKS {
        out.push(Problem::Crackle(levels.clicks));
    }
    if output.is_some_and(|d| d.muted) {
        out.push(Problem::OutputMuted);
    }
    out
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn short(p: &Problem) -> String {
    match p {
        Problem::Muted => "is muted".into(),
        Problem::VolumeLow(v) => format!("volume is at {v}%"),
        Problem::Silent => "heard nothing".into(),
        Problem::Clipping { peak_dbfs, .. } => format!("clipping at {peak_dbfs:.1} dBFS"),
        Problem::TooQuiet { peak_dbfs } => format!("too quiet (peak {peak_dbfs:.1} dBFS)"),
        Problem::Dropouts(n) => format!("has {}", plural(*n, "dropout", "dropouts")),
        Problem::Crackle(n) => format!("crackles ({})", plural(*n, "click", "clicks")),
        Problem::OutputMuted => "is fine".into(),
    }
}

fn explain(p: &Problem, input: &str, output: &str) -> (String, String) {
    match p {
        Problem::Muted => (
            format!("{input} is muted, so apps hear nothing from it."),
            format!("Unmute {input} in System Settings → Sound, or with its own mute button."),
        ),
        Problem::VolumeLow(v) => (
            format!("{input}'s input volume is at {v}%."),
            format!("Raise {input}'s input volume in System Settings → Sound to about 70–100%."),
        ),
        Problem::Silent => (
            format!("The clip from {input} was silent (peak below {SILENT_DBFS:.0} dBFS)."),
            format!(
                "Check {input}'s cable or USB connection, any hardware mute or gain knob on it, and that apps use it as their input."
            ),
        ),
        Problem::Clipping { peak_dbfs, pct } => (
            format!("{input} hits {peak_dbfs:.1} dBFS, and {pct:.1}% of the clip is at full scale, so loud parts distort."),
            format!("Lower {input}'s input volume or gain knob until loud speech peaks around -6 dBFS."),
        ),
        Problem::TooQuiet { peak_dbfs } => (
            format!("{input}'s loudest moment was only {peak_dbfs:.1} dBFS."),
            format!("Raise {input}'s input volume or gain, or move closer; loud speech should peak around -6 dBFS."),
        ),
        Problem::Dropouts(n) => (
            format!("The sound from {input} cut out {}.", plural(*n, "time", "times")),
            "Gaps like these usually mean the audio buffer ran dry: close heavy apps, try another USB port, or raise PipeWire's quantum.".into(),
        ),
        Problem::Crackle(n) => (
            format!("{input} has {} (sudden jumps in the sound).", plural(*n, "click", "clicks")),
            "Clicks usually mean a loose cable, a bad USB port or a sample-rate mismatch: try another cable or port.".into(),
        ),
        Problem::OutputMuted => (
            format!("The default output, {output}, is muted."),
            format!("Unmute {output} in System Settings → Sound."),
        ),
    }
}

/// The finished check: a title naming the device, the problem and the
/// default output; a shorter one for the card (device and problem); the card
/// body (the first fix, then the default output); and the full report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioReport {
    pub title: String,
    pub card_title: String,
    pub summary: String,
    pub text: String,
}

pub fn audio_report(outputs: &[AudioDevice], inputs: &[AudioDevice], input: Option<&AudioDevice>, levels: &Levels) -> AudioReport {
    let output = outputs.iter().find(|d| d.is_default).or(outputs.first());
    let in_label = input.map(AudioDevice::label).unwrap_or("your default input");
    let out_label = output.map(AudioDevice::label).unwrap_or("the default output");
    let problems = find_problems(input, output, levels);
    let head = match problems.first() {
        Some(p) => short(p),
        None => format!("sounds fine (peak {:.1} dBFS)", levels.peak_dbfs),
    };
    let card_title = match input {
        Some(d) => format!("Audio check: {} input {head}", d.label()),
        None => format!("Audio check: default input {head}"),
    };
    let routing = output.map(|o| format!("default output is {}{}", o.label(), if o.muted { " (muted)" } else { "" }));
    let title = match &routing {
        Some(r) => format!("{card_title}; {r}"),
        None => card_title.clone(),
    };
    let mut text = format!("{title}\n\n");
    match input {
        Some(d) => {
            let volume = d.volume_pct.map(|v| format!(", volume {v}%")).unwrap_or_default();
            let mute = if d.muted { "muted" } else { "not muted" };
            text.push_str(&format!("Input: {}{volume}, {mute}\n", d.label()));
        }
        None => text.push_str("Input: the default input (GrokHub couldn't list devices here)\n"),
    }
    text.push_str(&format!(
        "Clip: {:.1} s, peak {:.1} dBFS, average {:.1} dBFS, {:.1}% at full scale, {}, {}\n",
        levels.secs,
        levels.peak_dbfs,
        levels.rms_dbfs,
        levels.clipped_pct,
        plural(levels.dropouts, "dropout", "dropouts"),
        plural(levels.clicks, "click", "clicks"),
    ));
    let list = |devs: &[AudioDevice], skip: Option<&AudioDevice>| -> String {
        let names: Vec<String> = devs
            .iter()
            .filter(|d| Some(*d) != skip)
            .map(|d| {
                let mut s = d.label().to_string();
                if d.is_default {
                    s.push_str(" (default)");
                }
                if d.muted {
                    s.push_str(" (muted)");
                }
                s
            })
            .collect();
        if names.is_empty() { "none".into() } else { names.join(", ") }
    };
    if !outputs.is_empty() {
        text.push_str(&format!("Outputs: {}\n", list(outputs, None)));
    }
    if !inputs.is_empty() {
        text.push_str(&format!("Other inputs: {}\n", list(inputs, input)));
    }
    let first = if problems.is_empty() {
        text.push_str("\nNo problems found.\n");
        "No problems found.".to_string()
    } else {
        let lines: Vec<(String, String)> = problems.iter().map(|p| explain(p, in_label, out_label)).collect();
        text.push_str("\nProblems:\n");
        for (what, _) in &lines {
            text.push_str(&format!("- {what}\n"));
        }
        text.push_str("Suggested fixes:\n");
        for (i, (_, fix)) in lines.iter().enumerate() {
            text.push_str(&format!("{}. {fix}\n", i + 1));
        }
        lines[0].1.clone()
    };
    text.push_str("\nNothing was changed on this computer. The clip wasn't saved or sent anywhere.");
    let summary = match output {
        Some(o) => format!("{first} Default output: {}.", o.label()),
        None => first,
    };
    AudioReport { title, card_title, summary, text }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INFO: &str = "Server String: /run/user/1000/pulse/native\nServer Name: PulseAudio (on PipeWire 1.2.7)\nDefault Sink: alsa_output.pci-0000_03_00.1.hdmi-stereo\nDefault Source: alsa_input.usb-Blue_Microphones_Yeti-00.analog-stereo\n";

    const SINKS: &str = "Sink #47\n\tState: RUNNING\n\tName: alsa_output.pci-0000_03_00.1.hdmi-stereo\n\tDescription: Navi 21 HDMI / DisplayPort\n\tDriver: PipeWire\n\tMute: no\n\tVolume: front-left: 65536 / 100% / 0.00 dB,   front-right: 65536 / 100% / 0.00 dB\n\tBase Volume: 65536 / 100% / 0.00 dB\n\tProperties:\n\t\tdevice.description = \"Navi 21 HDMI / DisplayPort\"\n\nSink #52\n\tState: SUSPENDED\n\tName: alsa_output.pci-0000_0c_00.4.analog-stereo\n\tDescription: Starship/Matisse HD Audio Controller Analog Stereo\n\tMute: yes\n\tVolume: front-left: 26214 /  40% / -23.88 dB,   front-right: 26214 /  40% / -23.88 dB\n";

    const SOURCES: &str = "Source #48\n\tState: SUSPENDED\n\tName: alsa_output.pci-0000_03_00.1.hdmi-stereo.monitor\n\tDescription: Monitor of Navi 21 HDMI / DisplayPort\n\tMute: no\n\tVolume: front-left: 65536 / 100% / 0.00 dB\n\tMonitor of Sink: alsa_output.pci-0000_03_00.1.hdmi-stereo\n\nSource #60\n\tState: RUNNING\n\tName: alsa_input.usb-Blue_Microphones_Yeti-00.analog-stereo\n\tDescription: Yeti Stereo Microphone Analog Stereo\n\tMute: no\n\tVolume: front-left: 65536 / 100% / 0.00 dB,   front-right: 65536 / 100% / 0.00 dB\n\tMonitor of Sink: n/a\n\tPorts:\n\t\tanalog-input-mic: Microphone (type: Mic, priority: 8700, available)\n\nSource #61\n\tState: SUSPENDED\n\tName: alsa_input.pci-0000_0c_00.4.analog-stereo\n\tDescription: Starship/Matisse HD Audio Controller Analog Stereo\n\tMute: yes\n\tVolume: front-left: 9830 /  15% / -49.44 dB\n\tMonitor of Sink: n/a\n";

    fn devices() -> (Vec<AudioDevice>, Vec<AudioDevice>) {
        let (sink, source) = parse_pactl_defaults(INFO);
        (parse_pactl_list(SINKS, sink.as_deref()), parse_pactl_list(SOURCES, source.as_deref()))
    }

    /// A 16-bit mono WAV fixture, built the way a recorder writes one.
    fn wav(rate: u32, channels: u16, samples: &[i16]) -> Vec<u8> {
        let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let mut out = b"RIFF".to_vec();
        out.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        out.extend_from_slice(b"WAVEfmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&(rate * 2 * channels as u32).to_le_bytes());
        out.extend_from_slice(&(2 * channels).to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());
        out.extend_from_slice(b"data");
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&data);
        out
    }

    /// One second of a 441 Hz-ish sine at `amp`, at 48 kHz.
    fn sine(amp: f64, rate: u32) -> Vec<i16> {
        (0..rate)
            .map(|i| (amp * (i as f64 * 2.0 * std::f64::consts::PI * 441.0 / rate as f64).sin()).round() as i16)
            .collect()
    }

    #[test]
    fn pactl_lists_outputs_and_microphones_with_the_defaults_and_no_monitors() {
        let (outputs, inputs) = devices();
        assert_eq!(
            outputs,
            vec![
                AudioDevice {
                    name: "alsa_output.pci-0000_03_00.1.hdmi-stereo".into(),
                    description: "Navi 21 HDMI / DisplayPort".into(),
                    muted: false,
                    volume_pct: Some(100),
                    is_default: true,
                },
                AudioDevice {
                    name: "alsa_output.pci-0000_0c_00.4.analog-stereo".into(),
                    description: "Starship/Matisse HD Audio Controller Analog Stereo".into(),
                    muted: true,
                    volume_pct: Some(40),
                    is_default: false,
                },
            ]
        );
        assert_eq!(inputs.len(), 2, "the HDMI monitor is not a microphone");
        assert_eq!(inputs[0].description, "Yeti Stereo Microphone Analog Stereo");
        assert!(inputs[0].is_default);
        assert_eq!(inputs[1].volume_pct, Some(15));
        assert!(inputs[1].muted);
        assert_eq!(parse_pactl_defaults("nothing here"), (None, None));
        assert_eq!(parse_pactl_list("", None), Vec::new());
    }

    #[test]
    fn the_chosen_input_matches_by_name_and_defaults_to_the_default() {
        let (_, inputs) = devices();
        assert_eq!(pick_input(&inputs, "").unwrap().map(|d| d.label()), Some("Yeti Stereo Microphone Analog Stereo"));
        assert_eq!(pick_input(&inputs, "matisse").unwrap().map(|d| d.name.as_str()), Some("alsa_input.pci-0000_0c_00.4.analog-stereo"));
        assert_eq!(pick_input(&inputs, "blue microphones").unwrap().map(|d| d.name.as_str()), Some("alsa_input.usb-Blue_Microphones_Yeti-00.analog-stereo"));
        assert_eq!(
            pick_input(&inputs, "rode"),
            Err("No input matches \"rode\". Inputs: Yeti Stereo Microphone Analog Stereo, Starship/Matisse HD Audio Controller Analog Stereo.".into())
        );
        assert_eq!(pick_input(&[], ""), Ok(None));
        assert!(pick_input(&[], "yeti").unwrap_err().starts_with("GrokHub couldn't list your inputs"));
    }

    #[test]
    fn each_recorder_writes_raw_48k_mono_from_the_chosen_input() {
        assert_eq!(
            clip_argv("parecord", Some("mic")).unwrap(),
            vec!["--raw", "--format=s16le", "--rate=48000", "--channels=1", "--device=mic"]
        );
        assert_eq!(
            clip_argv("pw-record", None).unwrap(),
            vec!["--rate", "48000", "--channels", "1", "--format", "s16", "-"]
        );
        assert_eq!(
            clip_argv("pw-record", Some("mic")).unwrap(),
            vec!["--rate", "48000", "--channels", "1", "--format", "s16", "--target", "mic", "-"]
        );
        let ff = clip_argv("ffmpeg", Some("mic")).unwrap();
        assert_eq!(ff[3..7], ["-f".to_string(), "pulse".into(), "-i".into(), "mic".into()][..]);
        assert!(ff.ends_with(&["-t".into(), "5".into(), "-ac".into(), "1".into(), "-ar".into(), "48000".into(), "-f".into(), "s16le".into(), "-".into()]));
        assert_eq!(clip_argv("arecord", None).unwrap().last().map(String::as_str), Some("5"));
        assert_eq!(clip_argv("arecord", Some("mic")), None, "arecord only records the default input");
        assert_eq!(clip_argv("sox", None), None);
    }

    #[test]
    fn a_wav_fixture_reads_back_its_rate_and_first_channel() {
        assert_eq!(wav_samples(&wav(48_000, 1, &[1, -2, 3])), Some((48_000, vec![1, -2, 3])));
        assert_eq!(wav_samples(&wav(44_100, 2, &[10, 99, 20, 99])), Some((44_100, vec![10, 20])));
        assert_eq!(wav_samples(b"RIFF\0\0\0\0WAVE"), None);
        assert_eq!(wav_samples(b"not a wav"), None);
        assert_eq!(pcm_samples(&[1, 0, 0xff, 0xff, 7]), vec![1, -1]);
    }

    #[test]
    fn a_clipped_wav_reports_the_peak_and_the_share_at_full_scale() {
        let mut s = sine(16_000.0, 48_000);
        s[..4_800].iter_mut().for_each(|x| *x = if *x >= 0 { 32_767 } else { -32_768 });
        let (rate, samples) = wav_samples(&wav(48_000, 1, &s)).unwrap();
        let l = measure(&samples, rate);
        assert_eq!(l.secs, 1.0);
        assert_eq!(format!("{:.1}", l.peak_dbfs), "0.0", "-32768 is exactly full scale");
        assert_eq!(l.clipped, 4_800);
        assert_eq!(format!("{:.1}", l.clipped_pct), "10.0");
        assert_eq!(l.dropouts, 0);
        let (_, inputs) = devices();
        assert_eq!(
            find_problems(inputs.first(), None, &l),
            vec![Problem::Clipping { peak_dbfs: l.peak_dbfs, pct: l.clipped_pct }]
        );
    }

    #[test]
    fn a_silent_wav_and_a_quiet_wav_are_named_as_such() {
        let l = measure(&vec![0i16; 48_000], 48_000);
        assert_eq!(l.peak_dbfs, FLOOR_DBFS);
        assert_eq!(l.rms_dbfs, FLOOR_DBFS);
        assert_eq!(l.dropouts, 0, "silence from the start is not a dropout");
        assert_eq!(find_problems(None, None, &l), vec![Problem::Silent]);

        let l = measure(&sine(500.0, 48_000), 48_000);
        assert_eq!(format!("{:.1}", l.peak_dbfs), "-36.3");
        assert_eq!(format!("{:.1}", l.rms_dbfs), "-39.3");
        assert_eq!(find_problems(None, None, &l), vec![Problem::TooQuiet { peak_dbfs: l.peak_dbfs }]);

        let l = measure(&sine(16_000.0, 48_000), 48_000);
        assert_eq!(format!("{:.1}", l.peak_dbfs), "-6.2");
        assert_eq!((l.clipped, l.dropouts, l.clicks), (0, 0, 0));
        assert_eq!(find_problems(None, None, &l), Vec::new());
    }

    #[test]
    fn gaps_and_clicks_inside_the_sound_are_dropouts_and_crackle() {
        let mut s = sine(16_000.0, 48_000);
        s[10_000..10_960].iter_mut().for_each(|x| *x = 0); // 20 ms
        s[30_000..30_480].iter_mut().for_each(|x| *x = 0); // 10 ms
        s[40_000..40_100].iter_mut().for_each(|x| *x = 0); // 2 ms: too short
        for at in [20_000, 22_000, 24_000, 26_000] {
            s[at - 1] = 0;
            s[at] = 30_000;
            s[at + 1] = 0;
        }
        let l = measure(&s, 48_000);
        assert_eq!(l.dropouts, 2);
        assert_eq!(l.clicks, 8, "each spike jumps up and back down");
        assert_eq!(
            find_problems(None, None, &l),
            vec![Problem::Dropouts(2), Problem::Crackle(8)]
        );
    }

    #[test]
    fn the_report_names_the_device_the_problem_and_the_default_output() {
        let (outputs, inputs) = devices();
        let mut s = sine(16_000.0, 48_000);
        s.iter_mut().take(4_800).for_each(|x| *x = if *x >= 0 { 32_545 } else { -32_545 });
        let l = measure(&s, 48_000);
        let r = audio_report(&outputs, &inputs, inputs.first(), &l);
        assert_eq!(
            r.title,
            "Audio check: Yeti Stereo Microphone Analog Stereo input clipping at -0.1 dBFS; default output is Navi 21 HDMI / DisplayPort"
        );
        assert_eq!(r.card_title, "Audio check: Yeti Stereo Microphone Analog Stereo input clipping at -0.1 dBFS");
        assert_eq!(
            r.summary,
            "Lower Yeti Stereo Microphone Analog Stereo's input volume or gain knob until loud speech peaks around -6 dBFS. Default output: Navi 21 HDMI / DisplayPort."
        );
        let pct = format!("{:.1}", l.clipped_pct);
        assert_eq!(
            r.text,
            format!(
                "{}\n\nInput: Yeti Stereo Microphone Analog Stereo, volume 100%, not muted\nClip: 1.0 s, peak -0.1 dBFS, average {:.1} dBFS, {pct}% at full scale, 0 dropouts, 0 clicks\nOutputs: Navi 21 HDMI / DisplayPort (default), Starship/Matisse HD Audio Controller Analog Stereo (muted)\nOther inputs: Starship/Matisse HD Audio Controller Analog Stereo (muted)\n\nProblems:\n- Yeti Stereo Microphone Analog Stereo hits -0.1 dBFS, and {pct}% of the clip is at full scale, so loud parts distort.\nSuggested fixes:\n1. Lower Yeti Stereo Microphone Analog Stereo's input volume or gain knob until loud speech peaks around -6 dBFS.\n\nNothing was changed on this computer. The clip wasn't saved or sent anywhere.",
                r.title, l.rms_dbfs
            )
        );

        let muted = audio_report(&outputs, &inputs, inputs.get(1), &measure(&vec![0; 4_800], 48_000));
        assert_eq!(
            muted.title,
            "Audio check: Starship/Matisse HD Audio Controller Analog Stereo input is muted; default output is Navi 21 HDMI / DisplayPort"
        );
        assert!(muted.text.contains("\nProblems:\n- Starship/Matisse HD Audio Controller Analog Stereo is muted, so apps hear nothing from it.\n- Starship/Matisse HD Audio Controller Analog Stereo's input volume is at 15%.\n- The clip from Starship/Matisse HD Audio Controller Analog Stereo was silent (peak below -60 dBFS).\n"), "{}", muted.text);

        let fine = audio_report(&[], &[], None, &measure(&sine(16_000.0, 48_000), 48_000));
        assert_eq!(fine.title, "Audio check: default input sounds fine (peak -6.2 dBFS)");
        assert_eq!(fine.card_title, fine.title, "no outputs listed, nothing to add");
        assert_eq!(fine.summary, "No problems found.");
        assert!(fine.text.contains("Input: the default input (GrokHub couldn't list devices here)\n"));
        assert!(fine.text.contains("\nNo problems found.\n"));
        assert!(!fine.text.contains("Outputs:"));
    }
}
