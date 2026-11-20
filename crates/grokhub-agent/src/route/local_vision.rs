//! Pictures in a local chat (local router, step 7a & 7b).
//!
//! When the running `llama-server` has no `--mmproj` vision projector, every
//! `image_url` part is swapped for [`BLIND_NOTE`] so the turn is answered
//! without failing.
//!
//! When `--mmproj` is active (Card 955 / Step 7b), [`prepare_images`] keeps
//! pictures in the request and transcodes WebP / non-PNG/JPEG payloads to PNG
//! before sending to `llama-server` (whose `stb_image` decoder silently fails
//! on WebP inputs).

use std::collections::BTreeMap;
use std::io::Cursor;

use base64::Engine as _;
use grokhub_core::model_registry::profile::ModelProfile;
use grokhub_core::model_registry::Registry;

use crate::client::{ContentPart, InputItem};

use super::RouteInput;

/// What stands in for a picture the running model can't see.
pub const BLIND_NOTE: &str =
    "[A picture was attached here. The model on this computer can't see pictures, so it wasn't read.]";

/// The router's rule when [`reads_online`] took the device out of the pick.
pub const NO_VISION_RULE: &str = "local:no_vision";

/// A picture the device can't see goes to an online model that can, when
/// the turn may leave this computer: never an 18+ turn, a voice set to
/// Local only, sensitive data without your grant, or offline. Online, only
/// background steps reach the device through the router (a chat is never in
/// [`super::local::LOCAL_CLASSES`]), so a chat picture already goes online
/// by the image fit in [`super::policy::fits`].
pub fn reads_online(input: &RouteInput<'_>) -> bool {
    input.needs_image
        && !input.adult
        && !matches!(
            input.voice_reply,
            Some(super::voice_route::ReplyPref::LocalOnly)
        )
        && !(input.local.sensitive && !input.local.cloud_grant)
        && !input.local.offline
        && !input.local.offline_switch
}

/// `id` is listed as reading pictures. A model whose input kinds are
/// unknown may not, so it never stands in for the device on a picture:
/// the chat would say an online model read a picture nothing read.
pub fn reads_pictures(reg: &Registry, profiles: &BTreeMap<String, ModelProfile>, id: &str) -> bool {
    profiles
        .get(id)
        .and_then(|p| p.metadata.input_modalities.as_ref())
        .or(reg.get(id).and_then(|r| r.meta.input_modalities.as_ref()))
        .is_some_and(|m| m.iter().any(|x| x == "image"))
}

/// Summary of preparing images for a local `llama-server` turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct VisionOutcome {
    /// Images kept in the request for `--mmproj` vision.
    pub kept: usize,
    /// Images transcoded or normalized to PNG/JPEG before `llama-server`.
    pub transcoded: usize,
    /// Images replaced with [`BLIND_NOTE`] (no `--mmproj`, or undecodable format).
    pub stripped: usize,
}

/// Prepare every image in `input` for `llama-server`.
///
/// - When `has_vision` is `false`, strips all images to [`BLIND_NOTE`].
/// - When `has_vision` is `true`, keeps PNG/JPEG images and transcodes WebP or
///   other non-PNG/JPEG data URLs to PNG so `llama.cpp`'s `stb_image` never
///   fails silently. Any image that cannot be decoded falls back to
///   [`BLIND_NOTE`].
pub fn prepare_images(input: &mut [InputItem], has_vision: bool) -> VisionOutcome {
    if !has_vision {
        return VisionOutcome {
            kept: 0,
            transcoded: 0,
            stripped: strip_images(input),
        };
    }
    let mut out = VisionOutcome::default();
    for item in input {
        let InputItem::Message { content, .. } = item else {
            continue;
        };
        for part in content.iter_mut() {
            if let ContentPart::InputImage(url) = part {
                match transcode_image_data_url(url) {
                    Ok(normalized) => {
                        if normalized != *url {
                            *url = normalized;
                            out.transcoded += 1;
                        }
                        out.kept += 1;
                    }
                    Err(_) => {
                        *part = ContentPart::InputText(BLIND_NOTE.into());
                        out.stripped += 1;
                    }
                }
            }
        }
    }
    out
}

/// Normalize a `data:image/...;base64,...` URL so `llama-server` (`stb_image`)
/// always receives valid PNG or JPEG bytes. WebP and other non-PNG/JPEG
/// formats are transcoded to PNG.
pub fn transcode_image_data_url(url: &str) -> Result<String, String> {
    let Some(rest) = url.strip_prefix("data:") else {
        return Ok(url.to_string());
    };
    let Some((header, b64)) = rest.split_once(',') else {
        return Err("malformed data URL".into());
    };
    if !header.ends_with(";base64") {
        return Err("non-base64 data URL".into());
    }
    let mime = header
        .trim_end_matches(";base64")
        .trim()
        .to_ascii_lowercase();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .map_err(|e| format!("invalid base64 image: {e}"))?;

    if is_png(&bytes) {
        if mime == "image/png" {
            return Ok(url.to_string());
        }
        return Ok(format!("data:image/png;base64,{}", b64.trim()));
    }
    if is_jpeg(&bytes) {
        if mime == "image/jpeg" || mime == "image/jpg" {
            return Ok(url.to_string());
        }
        return Ok(format!("data:image/jpeg;base64,{}", b64.trim()));
    }

    // WebP or any other non-PNG/JPEG format: decode and re-encode as PNG.
    let png_bytes = decode_to_png(&bytes)?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(png_bytes);
    Ok(format!("data:image/png;base64,{encoded}"))
}

fn is_png(bytes: &[u8]) -> bool {
    bytes.len() >= 8 && bytes[..8] == [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]
}

fn is_jpeg(bytes: &[u8]) -> bool {
    bytes.len() >= 3 && bytes[..3] == [0xFF, 0xD8, 0xFF]
}

pub fn is_webp(bytes: &[u8]) -> bool {
    bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP"
}

fn decode_to_png(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if let Ok(img) = image::load_from_memory(bytes) {
        let mut buf = Cursor::new(Vec::new());
        img.write_to(&mut buf, image::ImageFormat::Png)
            .map_err(|e| format!("encode png: {e}"))?;
        return Ok(buf.into_inner());
    }
    if is_webp(bytes) {
        return transcode_webp_via_tool(bytes);
    }
    Err("unsupported image format for local vision".into())
}

fn transcode_webp_via_tool(bytes: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    // Try ffmpeg first (stdin -> stdout PNG, zero temp files).
    if let Ok(mut child) = Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-f",
            "webp_pipe",
            "-i",
            "pipe:0",
            "-f",
            "image2pipe",
            "-vcodec",
            "png",
            "pipe:1",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    {
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(bytes);
        }
        if let Ok(out) = child.wait_with_output()
            && out.status.success()
            && is_png(&out.stdout)
        {
            return Ok(out.stdout);
        }
    }

    // Fallback: python3 + Pillow if installed (stdin -> stdout PNG).
    const PY_WEBP_TO_PNG: &str = "import io,sys;from PIL import Image;im=Image.open(io.BytesIO(sys.stdin.buffer.read()));b=io.BytesIO();im.save(b,'PNG');sys.stdout.buffer.write(b.getvalue())";
    for py in ["python3", "python"] {
        if let Ok(mut child) = Command::new(py)
            .args(["-I", "-B", "-c", PY_WEBP_TO_PNG])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(bytes);
            }
            if let Ok(out) = child.wait_with_output()
                && out.status.success()
                && is_png(&out.stdout)
            {
                return Ok(out.stdout);
            }
        }
    }

    Err("no WebP-to-PNG decoder available".into())
}

/// Turn every picture in `input` into [`BLIND_NOTE`]. Returns how many it
/// replaced.
pub fn strip_images(input: &mut [InputItem]) -> usize {
    let mut n = 0;
    for item in input {
        let InputItem::Message { content, .. } = item else {
            continue;
        };
        for part in content.iter_mut() {
            if matches!(part, ContentPart::InputImage(_)) {
                *part = ContentPart::InputText(BLIND_NOTE.into());
                n += 1;
            }
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::ResponsesRequest;

    fn user(parts: Vec<ContentPart>) -> InputItem {
        InputItem::Message {
            role: "user".into(),
            content: parts,
        }
    }

    fn img() -> ContentPart {
        ContentPart::InputImage("data:image/png;base64,YQ==".into())
    }

    #[test]
    fn every_picture_becomes_the_note_and_text_stays() {
        let mut input = vec![
            user(vec![ContentPart::InputText("what is this?".into()), img()]),
            InputItem::FunctionCallOutput {
                call_id: "c1".into(),
                output: "done".into(),
            },
            user(vec![
                ContentPart::InputText("image from screenshot".into()),
                img(),
            ]),
        ];
        assert_eq!(strip_images(&mut input), 2);
        assert_eq!(
            input[0],
            user(vec![
                ContentPart::InputText("what is this?".into()),
                ContentPart::InputText(BLIND_NOTE.into()),
            ])
        );
        assert_eq!(
            input[2],
            user(vec![
                ContentPart::InputText("image from screenshot".into()),
                ContentPart::InputText(BLIND_NOTE.into()),
            ])
        );
        assert_eq!(strip_images(&mut input), 0);
    }

    #[test]
    fn a_stripped_request_posts_no_image_url() {
        let mut req = ResponsesRequest {
            model: "local".into(),
            effort: None,
            input: vec![user(vec![ContentPart::InputText("hi".into()), img()])],
            conversation_id: "c".into(),
            tools: Vec::new(),
            hosted_search: false,
            call_timeout: None,
            text_format: None,
        };
        strip_images(&mut req.input);
        let body = super::super::providers::openai_body("local", None, &req);
        assert_eq!(
            body["messages"][0]["content"],
            serde_json::json!(format!("hi\n{BLIND_NOTE}"))
        );
        assert!(!body.to_string().contains("image_url"));
    }

    fn real_png_data_url() -> String {
        let img = image::RgbaImage::from_pixel(1, 1, image::Rgba([255, 0, 0, 255]));
        let mut buf = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut buf, image::ImageFormat::Png)
            .unwrap();
        let b64 = base64::engine::general_purpose::STANDARD.encode(buf.into_inner());
        format!("data:image/png;base64,{b64}")
    }

    #[test]
    fn vision_keeps_valid_png_and_relabels_mismatched_mime() {
        let png_url = real_png_data_url();
        let mislabeled = png_url.replacen("image/png", "image/webp", 1);
        let mut input = vec![user(vec![
            ContentPart::InputText("describe".into()),
            ContentPart::InputImage(png_url.clone()),
            ContentPart::InputImage(mislabeled),
        ])];
        let out = prepare_images(&mut input, true);
        assert_eq!(
            out,
            VisionOutcome {
                kept: 2,
                transcoded: 1,
                stripped: 0,
            }
        );
        let InputItem::Message { content, .. } = &input[0] else {
            panic!("expected message");
        };
        assert_eq!(content[1], ContentPart::InputImage(png_url.clone()));
        assert_eq!(content[2], ContentPart::InputImage(png_url));
    }

    #[test]
    fn vision_falls_back_to_blind_note_on_corrupt_image() {
        let bad = "data:image/webp;base64,bm90LWEtdmFsaWQtaW1hZ2U=";
        let mut input = vec![user(vec![ContentPart::InputImage(bad.into())])];
        let out = prepare_images(&mut input, true);
        assert_eq!(
            out,
            VisionOutcome {
                kept: 0,
                transcoded: 0,
                stripped: 1,
            }
        );
        assert_eq!(
            input[0],
            user(vec![ContentPart::InputText(BLIND_NOTE.into())])
        );
    }

    #[test]
    fn detects_webp_magic_header() {
        assert!(is_webp(b"RIFF\x12\x00\x00\x00WEBPVP8 "));
        assert!(!is_webp(b"\x89PNG\r\n\x1a\n0000"));
    }
}
