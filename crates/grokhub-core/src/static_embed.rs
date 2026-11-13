//! Small local text embeddings for memory recall (LO-44 phase 2, HX-12).
//!
//! The model is a Model2Vec static model, `minishlab/potion-base-8M` (MIT):
//! one table of 256-wide token vectors, about 30 MB. Encoding a note is a
//! WordPiece tokenize, a lookup per token, a mean and an L2 normalize. There
//! is no neural network at run time, so it costs microseconds on the CPU and
//! never touches the GPU a game is using.
//!
//! The tokenizer is BERT's uncased WordPiece (the model's `vocab.txt`), the
//! same steps `tokenizers`' `BertTokenizer` runs: clean, split CJK, lowercase,
//! strip accents, split on whitespace and punctuation, then greedy longest
//! match with `##` pieces. Model2Vec encodes without special tokens and drops
//! `[UNK]`, so this does too. Accent stripping covers Latin-1, Latin
//! Extended-A and combining marks, which is every accent the vocab keeps apart.
//!
//! Files are checked before they load: the model by its pinned SHA-256, the
//! vocab by its pinned git blob hash (Hugging Face lists no SHA-256 for files
//! that aren't in LFS) and both by size.

use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

use sha1::Sha1;
use sha2::{Digest, Sha256};

/// How a downloaded file is checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pin {
    /// Lowercase hex SHA-256 of the bytes (LFS files).
    Sha256(&'static str),
    /// Lowercase hex git blob id: SHA-1 of `blob <len>\0` and the bytes.
    GitBlob(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelFile {
    pub name: &'static str,
    pub bytes: u64,
    pub pin: Pin,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmbedModel {
    pub id: &'static str,
    pub repo: &'static str,
    /// The commit the pins were read from, so a later upload can't swap a file.
    pub revision: &'static str,
    pub dim: usize,
    pub files: &'static [ModelFile],
}

pub const VOCAB_FILE: &str = "vocab.txt";
pub const WEIGHTS_FILE: &str = "model.safetensors";

/// The one model memory recall uses.
pub const POTION_BASE_8M: EmbedModel = EmbedModel {
    id: "potion-base-8M",
    repo: "minishlab/potion-base-8M",
    revision: "main",
    dim: 256,
    files: &[
        ModelFile {
            name: VOCAB_FILE,
            bytes: 219_690,
            pin: Pin::GitBlob("2f988cb392c3cd1152363bf737816e3530179fa1"),
        },
        ModelFile {
            name: WEIGHTS_FILE,
            bytes: 30_236_760,
            pin: Pin::Sha256("f65d0f325faadc1e121c319e2faa41170d3fa07d8c89abd48ca5358d9a223de2"),
        },
    ],
};

/// Tokens past this are dropped, as Model2Vec's default `max_length` does.
pub const MAX_TOKENS: usize = 512;
/// BERT's `max_input_chars_per_word`: a longer word is `[UNK]`.
const MAX_WORD_CHARS: usize = 100;

impl EmbedModel {
    /// Where this model's files live under the config dir.
    pub fn dir(&self, config_dir: &Path) -> PathBuf {
        config_dir.join("models").join("embed").join(self.id)
    }

    pub fn url(&self, file: &ModelFile) -> String {
        format!(
            "https://huggingface.co/{}/resolve/{}/{}",
            self.repo, self.revision, file.name
        )
    }

    /// True when every file is on disk at its pinned size. The hash is checked
    /// once, when the download finishes, and again by [`StaticModel::load`].
    pub fn present(&self, dir: &Path) -> bool {
        self.files.iter().all(|f| {
            fs::metadata(dir.join(f.name)).is_ok_and(|m| m.is_file() && m.len() == f.bytes)
        })
    }
}

/// Whether `bytes` match `file`'s size and pin.
pub fn verify(file: &ModelFile, bytes: &[u8]) -> bool {
    if bytes.len() as u64 != file.bytes {
        return false;
    }
    match file.pin {
        Pin::Sha256(want) => hex(&Sha256::digest(bytes)) == want,
        Pin::GitBlob(want) => {
            let mut h = Sha1::new();
            h.update(format!("blob {}\0", bytes.len()).as_bytes());
            h.update(bytes);
            hex(&h.finalize()) == want
        }
    }
}

/// [`verify`] for a file on disk, streamed so the model isn't read twice.
pub fn verify_path(file: &ModelFile, path: &Path) -> bool {
    let Ok(mut f) = fs::File::open(path) else {
        return false;
    };
    if f.metadata().map(|m| m.len()).ok() != Some(file.bytes) {
        return false;
    }
    let mut buf = vec![0u8; 256 * 1024];
    let digest = match file.pin {
        Pin::Sha256(_) => {
            let mut h = Sha256::new();
            loop {
                match f.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => h.update(&buf[..n]),
                    Err(_) => return false,
                }
            }
            hex(&h.finalize())
        }
        Pin::GitBlob(_) => {
            let mut h = Sha1::new();
            h.update(format!("blob {}\0", file.bytes).as_bytes());
            loop {
                match f.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => h.update(&buf[..n]),
                    Err(_) => return false,
                }
            }
            hex(&h.finalize())
        }
    };
    match file.pin {
        Pin::Sha256(want) | Pin::GitBlob(want) => digest == want,
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A loaded static model: vocab ids and one `[rows × dim]` table.
pub struct StaticModel {
    vocab: HashMap<String, u32>,
    unk: Option<u32>,
    dim: usize,
    table: Vec<f32>,
}

impl std::fmt::Debug for StaticModel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StaticModel")
            .field("tokens", &self.vocab.len())
            .field("dim", &self.dim)
            .finish()
    }
}

impl StaticModel {
    /// Load `model` from `dir`, checking every file against its pin first.
    pub fn load(model: &EmbedModel, dir: &Path) -> Result<Self, String> {
        for file in model.files {
            if !verify_path(file, &dir.join(file.name)) {
                return Err(format!("{} failed its check", file.name));
            }
        }
        let vocab = fs::read_to_string(dir.join(VOCAB_FILE)).map_err(|e| e.to_string())?;
        let weights = fs::read(dir.join(WEIGHTS_FILE)).map_err(|e| e.to_string())?;
        let m = Self::from_parts(&vocab, &weights)?;
        if m.dim != model.dim {
            return Err(format!("expected {} dims, found {}", model.dim, m.dim));
        }
        Ok(m)
    }

    /// Build from a `vocab.txt` body and a safetensors file holding one F32
    /// `embeddings` tensor with a row per vocab line. No pin check: tests and
    /// [`load`](Self::load) after it checked.
    pub fn from_parts(vocab_txt: &str, safetensors: &[u8]) -> Result<Self, String> {
        let mut vocab = HashMap::new();
        for (i, line) in vocab_txt.lines().enumerate() {
            let tok = line.trim_end_matches('\r');
            // The first id wins, as BERT's loader keeps it.
            vocab.entry(tok.to_string()).or_insert(i as u32);
        }
        let rows = vocab_txt.lines().count();
        let (shape, data) = read_f32_tensor(safetensors, "embeddings")?;
        let [n, dim] = shape[..] else {
            return Err(format!("embeddings has shape {shape:?}, not 2-D"));
        };
        if n != rows {
            return Err(format!("{n} vectors for {rows} vocab lines"));
        }
        if dim == 0 {
            return Err("embeddings are 0 wide".into());
        }
        let unk = vocab.get("[UNK]").copied();
        Ok(Self {
            vocab,
            unk,
            dim,
            table: data,
        })
    }

    pub fn dim(&self) -> usize {
        self.dim
    }

    /// Token ids for `text`, without special tokens or `[UNK]`, capped at
    /// [`MAX_TOKENS`].
    pub fn tokenize(&self, text: &str) -> Vec<u32> {
        let mut ids = Vec::new();
        for word in basic_tokens(text) {
            self.wordpiece(&word, &mut ids);
            if ids.len() >= MAX_TOKENS {
                ids.truncate(MAX_TOKENS);
                break;
            }
        }
        ids
    }

    fn wordpiece(&self, word: &str, out: &mut Vec<u32>) {
        let chars: Vec<char> = word.chars().collect();
        if chars.len() > MAX_WORD_CHARS {
            return; // [UNK], dropped
        }
        let mut pieces = Vec::new();
        let mut start = 0;
        while start < chars.len() {
            let mut end = chars.len();
            let mut found = None;
            while start < end {
                let mut sub: String = chars[start..end].iter().collect();
                if start > 0 {
                    sub.insert_str(0, "##");
                }
                if let Some(&id) = self.vocab.get(&sub) {
                    found = Some(id);
                    break;
                }
                end -= 1;
            }
            match found {
                Some(id) => {
                    pieces.push(id);
                    start = end;
                }
                None => return, // the whole word is [UNK]
            }
        }
        out.extend(pieces.into_iter().filter(|id| Some(*id) != self.unk));
    }

    /// The unit-length mean of `text`'s token vectors, or `None` when no
    /// token is in the vocab.
    pub fn encode(&self, text: &str) -> Option<Vec<f32>> {
        let ids = self.tokenize(text);
        if ids.is_empty() {
            return None;
        }
        let mut sum = vec![0f32; self.dim];
        for id in &ids {
            let row = &self.table[*id as usize * self.dim..(*id as usize + 1) * self.dim];
            for (s, v) in sum.iter_mut().zip(row) {
                *s += v;
            }
        }
        let n = ids.len() as f32;
        sum.iter_mut().for_each(|s| *s /= n);
        let norm = sum.iter().map(|v| v * v).sum::<f32>().sqrt();
        if !norm.is_finite() || norm == 0.0 {
            return None;
        }
        sum.iter_mut().for_each(|s| *s /= norm);
        Some(sum)
    }
}

/// Cosine of two unit vectors (their dot). 0 when the widths differ.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() {
        return 0.0;
    }
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Little-endian bytes for an index BLOB.
pub fn to_blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// A vector back from [`to_blob`]; `None` when the length is off.
pub fn from_blob(b: &[u8], dim: usize) -> Option<Vec<f32>> {
    if b.len() != dim * 4 {
        return None;
    }
    Some(
        b.as_chunks::<4>()
            .0
            .iter()
            .map(|c| f32::from_le_bytes(*c))
            .collect(),
    )
}

/// One F32 tensor out of a safetensors file: its shape and values.
fn read_f32_tensor(bytes: &[u8], name: &str) -> Result<(Vec<usize>, Vec<f32>), String> {
    let Some(len_bytes) = bytes.first_chunk::<8>() else {
        return Err("safetensors file is too short".into());
    };
    let head_len = u64::from_le_bytes(*len_bytes) as usize;
    let body_at = 8usize
        .checked_add(head_len)
        .filter(|end| *end <= bytes.len())
        .ok_or("safetensors header runs past the file")?;
    let header: serde_json::Value = serde_json::from_slice(&bytes[8..body_at])
        .map_err(|e| format!("safetensors header: {e}"))?;
    let entry = header.get(name).ok_or(format!("no {name} tensor"))?;
    let dtype = entry.get("dtype").and_then(|d| d.as_str()).unwrap_or("");
    if dtype != "F32" {
        return Err(format!("{name} is {dtype}, only F32 is read"));
    }
    let shape: Vec<usize> = entry
        .get("shape")
        .and_then(|s| s.as_array())
        .ok_or("tensor has no shape")?
        .iter()
        .map(|d| d.as_u64().map(|d| d as usize).ok_or("bad shape"))
        .collect::<Result<_, _>>()?;
    let offs = entry
        .get("data_offsets")
        .and_then(|o| o.as_array())
        .filter(|o| o.len() == 2)
        .ok_or("tensor has no data_offsets")?;
    let (Some(a), Some(b)) = (offs[0].as_u64(), offs[1].as_u64()) else {
        return Err("bad data_offsets".into());
    };
    let (a, b) = (body_at + a as usize, body_at + b as usize);
    let want = shape
        .iter()
        .try_fold(4usize, |acc, d| acc.checked_mul(*d))
        .ok_or("tensor is too large")?;
    if a > b || b > bytes.len() || b - a != want {
        return Err("tensor data doesn't match its shape".into());
    }
    let data = bytes[a..b]
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect();
    Ok((shape, data))
}

/// BERT's basic tokenizer for an uncased vocab.
fn basic_tokens(text: &str) -> Vec<String> {
    let mut spaced = String::with_capacity(text.len() + 8);
    for c in text.chars() {
        if c == '\0' || c == '\u{fffd}' || is_control(c) {
            continue;
        }
        if is_cjk(c) {
            spaced.push(' ');
            spaced.push(c);
            spaced.push(' ');
        } else if c.is_whitespace() {
            spaced.push(' ');
        } else {
            spaced.push(c);
        }
    }
    let mut out = Vec::new();
    for word in spaced.split_whitespace() {
        let mut cur = String::new();
        for c in word.chars().flat_map(char::to_lowercase) {
            let Some(c) = strip_accent(c) else {
                continue;
            };
            if is_punct(c) {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                out.push(c.to_string());
            } else {
                cur.push(c);
            }
        }
        if !cur.is_empty() {
            out.push(cur);
        }
    }
    out
}

fn is_control(c: char) -> bool {
    if c == '\t' || c == '\n' || c == '\r' {
        return false;
    }
    c.is_control()
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32,
        0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0x20000..=0x2A6DF | 0x2A700..=0x2B73F
        | 0x2B740..=0x2B81F | 0x2B820..=0x2CEAF | 0xF900..=0xFAFF | 0x2F800..=0x2FA1F)
}

/// BERT counts every non-alphanumeric ASCII symbol as punctuation, plus
/// Unicode category P (symbols such as `©` or `×` stay part of the word).
fn is_punct(c: char) -> bool {
    if c.is_ascii() {
        return c.is_ascii_punctuation();
    }
    matches!(c as u32,
        0x00A1 | 0x00A7 | 0x00AB | 0x00B6 | 0x00B7 | 0x00BB | 0x00BF
        | 0x2010..=0x2027 | 0x2030..=0x2043 | 0x2045..=0x2051 | 0x2053..=0x205E
        | 0x3001..=0x3003 | 0x3008..=0x3011 | 0x3014..=0x301F
        | 0xFF01..=0xFF03 | 0xFF05..=0xFF0A | 0xFF0C..=0xFF0F | 0xFF1A | 0xFF1B
        | 0xFF1F | 0xFF20 | 0xFF3B..=0xFF3D | 0xFF3F | 0xFF5B | 0xFF5D | 0xFF5F..=0xFF65)
}

/// A lowercased char without its accent; `None` for a combining mark.
fn strip_accent(c: char) -> Option<char> {
    let u = c as u32;
    if (0x0300..=0x036F).contains(&u) {
        return None;
    }
    let base = match c {
        'à'..='å' | 'ā' | 'ă' | 'ą' => 'a',
        'ç' | 'ć' | 'ĉ' | 'ċ' | 'č' => 'c',
        'ď' => 'd',
        'è'..='ë' | 'ē' | 'ĕ' | 'ė' | 'ę' | 'ě' => 'e',
        'ĝ' | 'ğ' | 'ġ' | 'ģ' => 'g',
        'ĥ' => 'h',
        'ì'..='ï' | 'ĩ' | 'ī' | 'ĭ' | 'į' => 'i',
        'ĵ' => 'j',
        'ķ' => 'k',
        'ĺ' | 'ļ' | 'ľ' => 'l',
        'ñ' | 'ń' | 'ņ' | 'ň' => 'n',
        'ò'..='ö' | 'ō' | 'ŏ' | 'ő' => 'o',
        'ŕ' | 'ŗ' | 'ř' => 'r',
        'ś' | 'ŝ' | 'ş' | 'š' => 's',
        'ţ' | 'ť' => 't',
        'ù'..='ü' | 'ũ' | 'ū' | 'ŭ' | 'ů' | 'ű' | 'ų' => 'u',
        'ŵ' => 'w',
        'ý' | 'ÿ' | 'ŷ' => 'y',
        'ź' | 'ż' | 'ž' => 'z',
        other => other,
    };
    Some(base)
}

#[cfg(test)]
mod tests;
