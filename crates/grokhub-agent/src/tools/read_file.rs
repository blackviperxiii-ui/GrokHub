use image::ImageEncoder;
use std::path::Path;

use base64::Engine;
use serde_json::{json, Value};

use super::{confine, str_field, u32_field, ToolOutput};

const DEFAULT_LIMIT: u32 = 2000;
const MAX_LIMIT: u32 = 5000;
const IMAGE_MAX_BYTES: usize = 1_500_000;
/// One read returns at most this much text (about 12k tokens). Whole lines
/// only; the note says which offset continues, so nothing is out of reach.
const READ_CHARS: usize = 50_000;
/// A single line longer than this (minified code, a base64 blob) is cut.
const LINE_CHARS: usize = 2_000;

pub fn schema() -> Value {
    json!({
        "type": "function",
        "name": "read_file",
        "description": "Read a workspace text file, or return a PNG or JPEG as an input_image data URL.",
        "parameters": {
            "type": "object",
            "properties": {
                "target_file": {"type": "string", "description": "Path relative to the workspace."},
                "offset": {"type": "integer", "description": "1-based line to start at."},
                "limit": {"type": "integer", "description": "How many lines to return. Default 2000, max 5000."}
            },
            "required": ["target_file"],
            "additionalProperties": false
        }
    })
}

pub fn run(workspace: &Path, args: &Value) -> ToolOutput {
    let raw = str_field(args, "target_file");
    if raw.is_empty() {
        return ToolOutput::err("target_file is required");
    }
    let path = match confine(workspace, &raw) {
        Ok(p) => p,
        Err(e) => return ToolOutput::err(e),
    };
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => return ToolOutput::err(format!("read {}: {e}", path.display())),
    };
    if let Some(mime) = image_mime(&bytes) {
        return encode_image(&bytes, mime);
    }
    let text = match String::from_utf8(bytes) {
        Ok(t) => t,
        Err(_) => return ToolOutput::err("file is binary and not a PNG or JPEG"),
    };
    let offset = u32_field(args, "offset").unwrap_or(1).max(1);
    let limit = u32_field(args, "limit")
        .unwrap_or(DEFAULT_LIMIT)
        .clamp(1, MAX_LIMIT);
    let lines: Vec<&str> = text.lines().collect();
    let start = (offset as usize).saturating_sub(1);
    if start >= lines.len() {
        return ToolOutput::ok(format!(
            "(no lines at offset {offset}; file has {} lines)",
            lines.len()
        ));
    }
    let end = (start + limit as usize).min(lines.len());
    let out = numbered_lines(&lines, start, end);
    // Workspace reads stay plain for coding. Outside-workspace text (if a
    // future path allows it) is fenced as untrusted data.
    ToolOutput::ok(super::fence::fence_if_outside(
        workspace,
        &path,
        "read_file",
        &out,
    ))
}

/// `n|line` rows from `start` to `end`, stopping early at [`READ_CHARS`] with a
/// note naming the offset that continues.
fn numbered_lines(lines: &[&str], start: usize, end: usize) -> String {
    let mut out = String::new();
    let mut used = 0usize;
    for (i, line) in lines[start..end].iter().enumerate() {
        let n = start + i + 1;
        let row = match line.char_indices().nth(LINE_CHARS) {
            Some((cut, _)) => format!(
                "{n}|{} …[line cut at {LINE_CHARS} of {} chars]\n",
                &line[..cut],
                line.chars().count()
            ),
            None => format!("{n}|{line}\n"),
        };
        let len = row.chars().count();
        if i > 0 && used + len > READ_CHARS {
            out.push_str(&format!(
                "(stopped at line {} of {} to keep this read small; read again with offset {n} to continue)\n",
                n - 1,
                lines.len()
            ));
            return out;
        }
        used += len;
        out.push_str(&row);
    }
    out
}

fn image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        Some("image/png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else {
        None
    }
}

fn encode_image(bytes: &[u8], mime: &str) -> ToolOutput {
    let decoded = match image::load_from_memory(bytes) {
        Ok(img) => img,
        Err(e) => return ToolOutput::err(format!("image decode failed: {e}")),
    };
    let (nw, nh) = grokhub_core::desktop_mcp::fit_downscale(decoded.width(), decoded.height());
    let within_pixels = nw == decoded.width() && nh == decoded.height();
    if within_pixels && bytes.len() <= IMAGE_MAX_BYTES {
        let b64 = base64::engine::general_purpose::STANDARD.encode(bytes);
        let url = format!("data:{mime};base64,{b64}");
        return ToolOutput {
            text: url.clone(),
            image_data_url: Some(url),
            failed: false,
        };
    }
    let scaled = if within_pixels {
        decoded
    } else {
        decoded.resize(nw, nh, image::imageops::FilterType::Triangle)
    };
    match jpeg_under_budget(&scaled) {
        Ok(jpg) => {
            let b64 = base64::engine::general_purpose::STANDARD.encode(&jpg);
            let url = format!("data:image/jpeg;base64,{b64}");
            ToolOutput {
                text: url.clone(),
                image_data_url: Some(url),
                failed: false,
            }
        }
        Err(e) => ToolOutput::err(e),
    }
}

fn jpeg_under_budget(img: &image::DynamicImage) -> Result<Vec<u8>, String> {
    let rgb = img.to_rgb8();
    let mut quality = 85u8;
    loop {
        let mut buf = Vec::new();
        let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality);
        enc.write_image(
            rgb.as_raw(),
            rgb.width(),
            rgb.height(),
            image::ExtendedColorType::Rgb8,
        )
        .map_err(|e| e.to_string())?;
        if buf.len() <= IMAGE_MAX_BYTES || quality <= 40 {
            return Ok(buf);
        }
        quality = quality.saturating_sub(10);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::execute;
    use image::ImageEncoder;
    use std::fs;

    #[test]
    fn a_big_file_reads_in_capped_pages_that_reach_the_end() {
        let dir = crate::harness::test_dir("read-capped");
        let body: String = (1..=3000)
            .map(|i| {
                format!(
                    "2026-10-31 INFO service heartbeat ok seq={i:05} {}\n",
                    "x".repeat(40)
                )
            })
            .collect();
        fs::write(dir.join("big.log"), &body).unwrap();
        let first = run(&dir, &json!({"target_file": "big.log"}));
        assert!(!first.failed);
        assert!(
            first.text.chars().count() <= READ_CHARS + 200,
            "{}",
            first.text.len()
        );
        assert!(first.text.starts_with("1|"), "{}", &first.text[..40]);
        let note = first.text.lines().last().unwrap();
        assert!(note.contains("read again with offset"), "{note}");
        let next: u32 = note
            .split("offset ")
            .nth(1)
            .and_then(|s| s.split_whitespace().next())
            .unwrap()
            .parse()
            .unwrap();
        // Paging on from the offset the note names reaches the last line.
        let mut offset = next;
        let mut last = String::new();
        for _ in 0..20 {
            let page = run(&dir, &json!({"target_file": "big.log", "offset": offset}));
            last = page.text.clone();
            match page
                .text
                .lines()
                .last()
                .and_then(|l| l.split("offset ").nth(1))
            {
                Some(rest) => offset = rest.split_whitespace().next().unwrap().parse().unwrap(),
                None => break,
            }
        }
        assert!(last.contains("seq=03000"), "{last}");
    }

    #[test]
    fn a_huge_single_line_is_cut_and_says_so() {
        let dir = crate::harness::test_dir("read-long-line");
        fs::write(dir.join("min.js"), format!("{}\nend\n", "é".repeat(10_000))).unwrap();
        let out = run(&dir, &json!({"target_file": "min.js"}));
        assert!(
            out.text.contains("[line cut at 2000 of 10000 chars]"),
            "{}",
            &out.text[..80]
        );
        assert!(out.text.contains("2|end"), "{}", out.text);
    }

    #[test]
    fn a_small_file_reads_whole_with_no_note() {
        let dir = crate::harness::test_dir("read-small");
        fs::write(dir.join("a.txt"), "one\ntwo\n").unwrap();
        let out = run(&dir, &json!({"target_file": "a.txt"}));
        assert_eq!(out.text, "1|one\n2|two\n");
    }

    #[test]
    fn workspace_read_is_not_fenced() {
        let dir = crate::harness::test_dir("read-no-fence");
        fs::write(dir.join("notes.txt"), "alpha\nbeta\n").unwrap();
        let out = run(&dir, &json!({"target_file": "notes.txt"}));
        assert!(!out.failed, "{}", out.text);
        assert!(out.text.contains("1|alpha"), "{}", out.text);
        assert!(
            !crate::tools::fence::looks_fenced(&out.text),
            "{}",
            out.text
        );
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn read_file_offsets_lines_and_encodes_a_tiny_png() {
        let dir = std::env::temp_dir().join(format!("gh-read-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "one\ntwo\nthree\nfour\n").unwrap();
        let text = execute(
            &dir,
            "read_file",
            r#"{"target_file":"a.txt","offset":2,"limit":2}"#,
        );
        assert!(!text.failed, "{}", text.text);
        assert!(text.text.contains("2|two"), "{}", text.text);
        assert!(text.text.contains("3|three"), "{}", text.text);
        assert!(!text.text.contains("1|one"));

        let mut png = Vec::new();
        image::codecs::png::PngEncoder::new(&mut png)
            .write_image(&[1, 2, 3], 1, 1, image::ExtendedColorType::Rgb8)
            .unwrap();
        std::fs::write(dir.join("dot.png"), &png).unwrap();
        let img = execute(&dir, "read_file", r#"{"target_file":"dot.png"}"#);
        assert!(!img.failed, "{}", img.text);
        assert!(
            img.text.starts_with("data:image/png;base64,"),
            "{}",
            img.text
        );
        assert_eq!(img.image_data_url.as_deref(), Some(img.text.as_str()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
