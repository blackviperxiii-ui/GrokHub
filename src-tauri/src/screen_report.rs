//! The screen recording's report, as the cabin wrote it: when a recording
//! stops, a few of its stills and the note go to Grok once, and what comes
//! back is the report in the chat that asked and on the recording's Pulse
//! card. The call goes through the model client, so the send check logs it
//! like any chat turn. With no sign-in, or stills that can't be read back,
//! nothing is sent and the report says why.

use std::path::{Path, PathBuf};
use std::time::Duration;

use grokhub_agent::{CancelToken, ContentPart, InputItem, ModelClient, ResponsesRequest};
use grokhub_core::screen_record as sr;
use std::io::Read;

/// The longest edge a still is sent at. A 4K grab is scaled down first.
const SEND_EDGE: u32 = 2048;
/// JPEG quality for a sent still.
const SEND_QUALITY: u8 = 80;
/// How long the one call may take.
const CALL_TIMEOUT: Duration = Duration::from_secs(120);

/// The why when nothing could look at the stills.
pub const NO_SIGN_IN: &str = "Connect Grok in Settings";
pub const UNREADABLE: &str = "the stills could not be read back";
pub const HALTED_FIRST: &str = "Halt stopped it before anything was sent";
pub const HALTED: &str = "Halt stopped it";

/// The model client for the one call, or why there is none.
pub type Client = Result<Box<dyn ModelClient>, String>;

/// What one recording's report says: the full text for the chat, and one
/// line for its Pulse card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finished {
    pub text: String,
    pub summary: String,
}

/// One line for the feed card under the recording's title.
pub fn report_summary(report: &sr::ScreenReport) -> String {
    if !report.cause.is_empty() {
        format!("Likely cause: {}", report.cause)
    } else if !report.seen.is_empty() {
        report.seen.clone()
    } else {
        "Nothing stood out in the stills.".into()
    }
}

/// The report when nothing was diagnosed.
fn local_only(title: &str, dir: &Path, why: &str) -> Finished {
    Finished {
        text: sr::local_only_text(title, dir, why),
        summary: format!("Saved on this computer, not diagnosed: {}", why.trim()),
    }
}

/// One still as a JPEG data URL, at most [`SEND_EDGE`] on its long edge.
/// `None` for a file that is too big, isn't a PNG or JPEG, or claims more
/// pixels than a picture may have.
fn still_url(path: &Path) -> Option<String> {
    let cap = grokhub_core::attach::IMAGE_FILE_CAP;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(cap + 1).read_to_end(&mut bytes))
        .ok()?;
    if bytes.len() as u64 > cap {
        return None;
    }
    let (w, h) = crate::attach::image_size(&bytes)?;
    if !grokhub_core::image_pixels_ok(w, h) {
        return None;
    }
    let img = image::load_from_memory(&bytes).ok()?;
    let img = if w.max(h) > SEND_EDGE {
        img.resize(SEND_EDGE, SEND_EDGE, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, SEND_QUALITY)
        .encode_image(&img.to_rgb8())
        .ok()?;
    Some(grokhub_core::jpeg_data_url(&out))
}

/// The keyframes that go: spread over the recording, first and last
/// included, each read back from its file.
pub fn keyframe_urls(frames: &[PathBuf]) -> Vec<String> {
    sr::sample_keyframes(frames.len(), sr::KEYFRAMES)
        .into_iter()
        .filter_map(|i| still_url(&frames[i]))
        .collect()
}

/// The one call: the cabin's system prompt, then the note and the stills in
/// order, on the fast model.
pub fn vision_request(note: &str, secs: u64, stills: &[String]) -> ResponsesRequest {
    let model = grokhub_core::model_for_mode("fast");
    let mut input = Vec::new();
    for (role, text) in sr::diagnosis_messages(note, secs, stills.len()) {
        let mut content = vec![ContentPart::InputText(text)];
        if role == "user" {
            content.extend(stills.iter().cloned().map(ContentPart::InputImage));
        }
        input.push(InputItem::Message { role, content });
    }
    ResponsesRequest {
        text_format: None,
        model: model.to_string(),
        effort: grokhub_core::quick_effort(model).map(Into::into),
        input,
        conversation_id: "screen-record".into(),
        tools: Vec::new(),
        hosted_search: false,
        call_timeout: Some(CALL_TIMEOUT),
    }
}

/// Look at a finished recording once. `client` is `Err` with no sign-in.
/// Halt (`cancel`) before the call sends nothing; during it, the report says
/// it was stopped.
pub fn diagnose(
    title: &str,
    note: &str,
    secs: u64,
    dir: &Path,
    frames: &[PathBuf],
    client: impl FnOnce() -> Client,
    cancel: &CancelToken,
) -> Finished {
    if cancel.is_cancelled() {
        return local_only(title, dir, HALTED_FIRST);
    }
    let stills = keyframe_urls(frames);
    if stills.is_empty() {
        return local_only(title, dir, UNREADABLE);
    }
    let Ok(client) = client() else {
        return local_only(title, dir, NO_SIGN_IN);
    };
    if cancel.is_cancelled() {
        return local_only(title, dir, HALTED_FIRST);
    }
    let req = vision_request(note, secs, &stills);
    match client.stream(&req, cancel, &mut |_| {}) {
        _ if cancel.is_cancelled() => local_only(title, dir, HALTED),
        Ok(out) if !out.text.trim().is_empty() => {
            let report = sr::parse_report(&out.text);
            Finished {
                text: sr::report_text(title, &report, dir),
                summary: report_summary(&report),
            }
        }
        Ok(_) => local_only(title, dir, "Grok sent back nothing"),
        Err(e) => local_only(title, dir, &e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_agent::{ClientError, StreamEvent, TurnOutput};
    use std::sync::{Arc, Mutex};

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "gh-screenreport-{tag}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A real PNG still, `w` by `h`, one colour.
    fn png(path: &Path, w: u32, h: u32) {
        image::RgbImage::from_pixel(w, h, image::Rgb([40, 90, 200]))
            .save(path)
            .unwrap();
    }

    /// Answers with `reply` and keeps what it was asked.
    struct Canned {
        reply: Result<String, String>,
        asked: Arc<Mutex<Vec<ResponsesRequest>>>,
    }

    impl ModelClient for Canned {
        fn stream(
            &self,
            req: &ResponsesRequest,
            _cancel: &CancelToken,
            _sink: &mut dyn FnMut(StreamEvent),
        ) -> Result<TurnOutput, ClientError> {
            self.asked.lock().unwrap().push(req.clone());
            match &self.reply {
                Ok(text) => Ok(TurnOutput {
                    text: text.clone(),
                    ..TurnOutput::default()
                }),
                Err(e) => Err(ClientError::Protocol(e.clone())),
            }
        }
    }

    type Asked = Arc<Mutex<Vec<ResponsesRequest>>>;

    fn canned(reply: Result<&str, &str>) -> (Asked, impl FnOnce() -> Client) {
        let asked = Arc::new(Mutex::new(Vec::new()));
        let client = Canned {
            reply: reply.map(str::to_string).map_err(str::to_string),
            asked: Arc::clone(&asked),
        };
        (asked, move || Ok(Box::new(client) as Box<dyn ModelClient>))
    }

    fn frames(dir: &Path, n: usize) -> Vec<PathBuf> {
        (0..n)
            .map(|i| {
                let path = dir.join(sr::frame_name(i, "png"));
                png(&path, 64, 36);
                path
            })
            .collect()
    }

    const REPLY: &str = "Seen: Firefox plays a video and the picture freezes.\nLikely cause: Hardware video decoding is off.\nFix steps:\n1. Open about:support.\n2. Turn on hardware decoding.";

    #[test]
    fn six_spread_stills_and_the_note_go_once_and_come_back_as_the_report() {
        let dir = scratch("ok");
        let frames = frames(&dir, 10);
        let (asked, client) = canned(Ok(REPLY));
        let got = diagnose(
            "Screen recording 0:20: video stutters",
            "video stutters",
            20,
            &dir,
            &frames,
            client,
            &CancelToken::new(),
        );
        assert_eq!(got.summary, "Likely cause: Hardware video decoding is off.");
        assert_eq!(
            got.text,
            format!(
                "Screen recording 0:20: video stutters\n\nWhat I saw: Firefox plays a video and the picture freezes.\nLikely cause: Hardware video decoding is off.\nSuggested fix steps:\n1. Open about:support.\n2. Turn on hardware decoding.\n\nNothing was changed on this computer. The recording is in {}.",
                dir.display()
            )
        );
        let asked = asked.lock().unwrap();
        assert_eq!(asked.len(), 1, "one call");
        let req = &asked[0];
        assert_eq!(req.model, grokhub_core::model_for_mode("fast"));
        assert_eq!(req.conversation_id, "screen-record");
        assert!(req.tools.is_empty() && !req.hosted_search);
        let InputItem::Message { role, content } = &req.input[1] else {
            panic!("user message");
        };
        assert_eq!(role, "user");
        let ContentPart::InputText(first) = &content[0] else {
            panic!("text first");
        };
        assert_eq!(
            first,
            "Recording: 0:20, 6 stills in order.\nWhat the user says is wrong: video stutters"
        );
        let images: Vec<&String> = content
            .iter()
            .filter_map(|c| match c {
                ContentPart::InputImage(url) => Some(url),
                ContentPart::InputText(_) => None,
            })
            .collect();
        assert_eq!(images.len(), 6);
        assert!(images
            .iter()
            .all(|u| u.starts_with("data:image/jpeg;base64,")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_big_still_is_scaled_to_the_send_edge() {
        let dir = scratch("big");
        let path = dir.join("frame-001.png");
        png(&path, 3840, 2160);
        let url = still_url(&path).expect("url");
        use base64::Engine;
        let b64 = url.strip_prefix("data:image/jpeg;base64,").expect("jpeg");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .unwrap();
        let img = image::load_from_memory(&bytes).unwrap();
        assert_eq!((img.width(), img.height()), (2048, 1152));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn with_no_sign_in_or_unreadable_stills_nothing_is_asked() {
        let dir = scratch("none");
        let good = frames(&dir, 3);
        let got = diagnose(
            "Screen recording 0:06 (no note)",
            "",
            6,
            &dir,
            &good,
            || Err("no key".into()),
            &CancelToken::new(),
        );
        assert_eq!(
            got,
            Finished {
                text: format!(
                    "Screen recording 0:06 (no note)\n\nSaved on this computer only, not diagnosed: Connect Grok in Settings. The recording is in {}.",
                    dir.display()
                ),
                summary: "Saved on this computer, not diagnosed: Connect Grok in Settings".into(),
            }
        );
        let junk = dir.join("frame-009.png");
        std::fs::write(&junk, b"not a picture").unwrap();
        let (asked, client) = canned(Ok(REPLY));
        let got = diagnose("t", "", 2, &dir, &[junk], client, &CancelToken::new());
        assert_eq!(
            got.summary,
            "Saved on this computer, not diagnosed: the stills could not be read back"
        );
        assert!(asked.lock().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn halt_before_the_call_sends_nothing() {
        let dir = scratch("halt");
        let frames = frames(&dir, 2);
        let (asked, client) = canned(Ok(REPLY));
        let cancel = CancelToken::new();
        cancel.cancel();
        let got = diagnose("t", "", 2, &dir, &frames, client, &cancel);
        assert_eq!(
            got.summary,
            "Saved on this computer, not diagnosed: Halt stopped it before anything was sent"
        );
        assert!(asked.lock().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_call_keeps_the_recording_and_says_why() {
        let dir = scratch("fail");
        let frames = frames(&dir, 1);
        let (_, client) = canned(Err("rate limited"));
        let got = diagnose("t", "", 2, &dir, &frames, client, &CancelToken::new());
        assert_eq!(
            got.text,
            format!(
                "t\n\nSaved on this computer only, not diagnosed: rate limited. The recording is in {}.",
                dir.display()
            )
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The real client, offline: the send check refuses before any request
    /// leaves, and the report says so.
    #[test]
    fn offline_the_real_client_sends_nothing() {
        let dir = scratch("offline");
        let cfg = dir.join("cfg");
        std::fs::create_dir_all(&cfg).unwrap();
        let _config = grokhub_agent::perm::ConfigGuard::set(&cfg);
        let _offline = grokhub_core::offline::force_for_thread(true);
        let frames = frames(&dir, 2);
        let client = || {
            Ok(Box::new(grokhub_agent::XaiClient::new(
                "sk-abcdefghijklmnopqrstuv",
                grokhub_agent::AuthKind::ApiKey,
                Duration::from_secs(5),
            )) as Box<dyn ModelClient>)
        };
        let got = diagnose("t", "", 2, &dir, &frames, client, &CancelToken::new());
        let offline = grokhub_core::offline::refuse().unwrap_err();
        assert_eq!(
            got.summary,
            format!("Saved on this computer, not diagnosed: {offline}")
        );
        assert!(
            grokhub_agent::harness::read_egress(&cfg).is_empty(),
            "nothing left, nothing logged"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_card_line_is_the_cause_else_what_was_seen() {
        let mut r = sr::ScreenReport {
            seen: "Firefox freezes.".into(),
            cause: "Decoding is off.".into(),
            steps: vec![],
        };
        assert_eq!(report_summary(&r), "Likely cause: Decoding is off.");
        r.cause.clear();
        assert_eq!(report_summary(&r), "Firefox freezes.");
        r.seen.clear();
        assert_eq!(report_summary(&r), "Nothing stood out in the stills.");
    }
}
