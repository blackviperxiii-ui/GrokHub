use serde::{Deserialize, Serialize};

pub const FRAME_CAP: usize = 400_000;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PresenceFrame {
    pub data_url: String,
    pub at: u64,
}

pub enum FrameGet {
    Missing,
    NotModified { at: u64 },
    Bytes { mime: String, buf: Vec<u8>, at: u64 },
}

pub fn store_frame(data_url: &str, at: u64) -> Option<PresenceFrame> {
    if data_url.len() > FRAME_CAP {
        return None;
    }
    if !data_url.starts_with("data:image") {
        return None;
    }
    let frame = PresenceFrame {
        data_url: data_url.to_string(),
        at,
    };
    frame_bytes(&frame)?;
    Some(frame)
}

pub fn frame_bytes(frame: &PresenceFrame) -> Option<(String, Vec<u8>)> {
    let rest = frame.data_url.strip_prefix("data:")?;
    let (mime, b64) = rest.split_once(";base64,")?;
    if !mime.starts_with("image/") {
        return None;
    }
    let buf = decode_b64(b64)?;
    Some((mime.to_string(), buf))
}

pub fn get_jpeg(frame: Option<&PresenceFrame>, since: u64) -> FrameGet {
    let Some(frame) = frame else {
        return FrameGet::Missing;
    };
    if since > 0 && frame.at <= since {
        return FrameGet::NotModified { at: frame.at };
    }
    match frame_bytes(frame) {
        Some((mime, buf)) => FrameGet::Bytes {
            mime,
            buf,
            at: frame.at,
        },
        None => FrameGet::Missing,
    }
}

fn decode_b64(s: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u8> {
        match c {
            b'A'..=b'Z' => Some(c - b'A'),
            b'a'..=b'z' => Some(c - b'a' + 26),
            b'0'..=b'9' => Some(c - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let clean: Vec<u8> = s.bytes().filter(|b| !b.is_ascii_whitespace() && *b != b'=').collect();
    let mut out = Vec::with_capacity(clean.len() * 3 / 4);
    for chunk in clean.chunks(4) {
        if chunk.len() < 2 {
            break;
        }
        let a = val(chunk[0])?;
        let b = val(chunk[1])?;
        out.push((a << 2) | (b >> 4));
        if chunk.len() > 2 {
            let c = val(chunk[2])?;
            out.push((b << 4) | (c >> 2));
            if chunk.len() > 3 {
                let d = val(chunk[3])?;
                out.push((c << 6) | d);
            }
        }
    }
    Some(out)
}

pub fn encode_b64(bytes: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        let b0 = bytes[i];
        let b1 = bytes.get(i + 1).copied();
        let b2 = bytes.get(i + 2).copied();
        out.push(T[(b0 >> 2) as usize] as char);
        out.push(T[(((b0 & 3) << 4) | (b1.unwrap_or(0) >> 4)) as usize] as char);
        if b1.is_some() {
            out.push(T[(((b1.unwrap_or(0) & 15) << 2) | (b2.unwrap_or(0) >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if b2.is_some() {
            out.push(T[(b2.unwrap_or(0) & 63) as usize] as char);
        } else {
            out.push('=');
        }
        i += 3;
    }
    out
}

pub fn jpeg_data_url(bytes: &[u8]) -> String {
    format!("data:image/jpeg;base64,{}", encode_b64(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_non_image() {
        assert!(store_frame("hello", 1).is_none());
        let huge = format!("data:image/jpeg;base64,{}", "A".repeat(FRAME_CAP));
        assert!(
            store_frame(&huge, 1).is_none(),
            "over-cap frames must not store a truncated unusable JPEG"
        );
    }

    #[test]
    fn jpeg_304() {
        let f = store_frame(
            "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg==",
            50,
        )
        .unwrap();
        match get_jpeg(Some(&f), 50) {
            FrameGet::NotModified { at } => assert_eq!(at, 50),
            _ => panic!("expected 304"),
        }
        match get_jpeg(Some(&f), 0) {
            FrameGet::Bytes { mime, buf, .. } => {
                assert!(mime.starts_with("image/"));
                assert_eq!(encode_b64(&buf).len() % 4, 0);
            }
            _ => panic!("expected bytes"),
        }
        let url = jpeg_data_url(&[0xFF, 0xD8, 0xFF, 0xD9]);
        assert!(url.starts_with("data:image/jpeg;base64,"));
        let stored = store_frame(&url, 1).unwrap();
        let (_, raw) = frame_bytes(&stored).unwrap();
        assert_eq!(raw, vec![0xFF, 0xD8, 0xFF, 0xD9]);
    }
}
