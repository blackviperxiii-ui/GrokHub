//! Reads the shape of a GGUF model from its header: architecture, layer
//! count, attention heads, context length and experts. The auto-config sizes
//! weights, KV cache and offload from this. Only the key/value section is
//! read; tensor data is never touched. The tokenizer's vocabulary is hashed
//! so a draft model is only paired with a model that tokenizes the same.

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use sha2::{Digest, Sha256};

/// What the auto-config needs from a GGUF header.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GgufMeta {
    pub arch: String,
    pub block_count: u32,
    pub context_length: u32,
    pub embedding_length: u32,
    pub head_count: u32,
    /// The largest per-layer KV head count.
    pub head_count_kv: u32,
    pub key_length: u32,
    pub value_length: u32,
    /// Zero for a dense model.
    pub expert_count: u32,
    pub file_mb: u64,
    /// SHA-256 of `tokenizer.ggml.model` and every `tokenizer.ggml.tokens`
    /// entry; `None` when the file carries no vocabulary.
    pub tokenizer: Option<String>,
    /// Multi-token-prediction layers (`{arch}.nextn_predict_layers`).
    pub nextn_layers: u32,
}

impl GgufMeta {
    pub fn is_moe(&self) -> bool {
        self.expert_count > 1
    }

    fn head_dim(&self) -> u32 {
        self.embedding_length
            .checked_div(self.head_count)
            .unwrap_or(0)
    }

    /// K + V width per layer per token.
    pub fn kv_width(&self) -> u64 {
        let kv_heads = u64::from(if self.head_count_kv > 0 {
            self.head_count_kv
        } else {
            self.head_count
        });
        let k = u64::from(if self.key_length > 0 {
            self.key_length
        } else {
            self.head_dim()
        });
        let v = u64::from(if self.value_length > 0 {
            self.value_length
        } else {
            self.head_dim()
        });
        kv_heads * (k + v)
    }
}

const MAX_STRING: u64 = 1 << 20;
const MAX_KV: u64 = 1 << 16;

fn u32_le(r: &mut impl Read) -> Result<u32, String> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b).map_err(|e| e.to_string())?;
    Ok(u32::from_le_bytes(b))
}

fn u64_le(r: &mut impl Read) -> Result<u64, String> {
    let mut b = [0u8; 8];
    r.read_exact(&mut b).map_err(|e| e.to_string())?;
    Ok(u64::from_le_bytes(b))
}

fn skip(r: &mut impl Read, n: u64) -> Result<(), String> {
    let got = std::io::copy(&mut r.take(n), &mut std::io::sink()).map_err(|e| e.to_string())?;
    (got == n)
        .then_some(())
        .ok_or_else(|| "GGUF header ends early".to_string())
}

fn string(r: &mut impl Read) -> Result<String, String> {
    let n = u64_le(r)?;
    if n > MAX_STRING {
        return Err(format!("GGUF string of {n} bytes"));
    }
    let mut b = vec![0u8; n as usize];
    r.read_exact(&mut b).map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&b).into_owned())
}

/// A value: its integer when it is one (the largest item of an integer
/// array), its text when it is a string.
enum Value {
    Int(u64),
    Text(String),
    Other,
}

fn scalar_size(t: u32) -> Option<u64> {
    match t {
        0 | 1 | 7 => Some(1),
        2 | 3 => Some(2),
        4..=6 => Some(4),
        10..=12 => Some(8),
        _ => None,
    }
}

fn int_of(t: u32, r: &mut impl Read) -> Result<Option<u64>, String> {
    let size = scalar_size(t).ok_or_else(|| format!("GGUF type {t}"))?;
    let mut b = [0u8; 8];
    r.read_exact(&mut b[..size as usize])
        .map_err(|e| e.to_string())?;
    let v = u64::from_le_bytes(b);
    Ok(matches!(t, 0..=5 | 10 | 11).then_some(v))
}

/// `tokens` hashes each item of a string array instead of skipping it.
fn value(t: u32, r: &mut impl Read, tokens: Option<&mut Sha256>) -> Result<Value, String> {
    match t {
        8 => Ok(Value::Text(string(r)?)),
        9 => {
            let item = u32_le(r)?;
            let len = u64_le(r)?;
            if item == 8 {
                match tokens {
                    Some(h) => {
                        for _ in 0..len {
                            let s = string(r)?;
                            h.update((s.len() as u64).to_le_bytes());
                            h.update(s.as_bytes());
                        }
                    }
                    None => {
                        for _ in 0..len {
                            let n = u64_le(r)?;
                            skip(r, n)?;
                        }
                    }
                }
                return Ok(Value::Other);
            }
            if item == 9 {
                return Err("GGUF nested array".into());
            }
            let mut best = None;
            for _ in 0..len {
                if let Some(v) = int_of(item, r)? {
                    best = best.max(Some(v));
                }
            }
            Ok(best.map_or(Value::Other, Value::Int))
        }
        _ => Ok(int_of(t, r)?.map_or(Value::Other, Value::Int)),
    }
}

/// The header of a GGUF v2/v3 stream.
pub fn parse_meta(r: &mut impl Read, file_bytes: u64) -> Result<GgufMeta, String> {
    let mut magic = [0u8; 4];
    r.read_exact(&mut magic).map_err(|e| e.to_string())?;
    if &magic != b"GGUF" {
        return Err("not a GGUF file".into());
    }
    let version = u32_le(r)?;
    if !(2..=3).contains(&version) {
        return Err(format!("GGUF version {version} is not supported"));
    }
    let _tensors = u64_le(r)?;
    let kv = u64_le(r)?;
    if kv > MAX_KV {
        return Err(format!("GGUF header lists {kv} keys"));
    }
    let mut ints = Vec::new();
    let mut arch = String::new();
    let mut vocab_model = String::new();
    let mut vocab: Option<Sha256> = None;
    for _ in 0..kv {
        let key = string(r)?;
        let t = u32_le(r)?;
        let sink = (key == "tokenizer.ggml.tokens").then(|| vocab.insert(Sha256::new()));
        match value(t, r, sink)? {
            Value::Text(s) if key == "general.architecture" => arch = s,
            Value::Text(s) if key == "tokenizer.ggml.model" => vocab_model = s,
            Value::Int(v) => ints.push((key, v)),
            _ => {}
        }
    }
    let tokenizer = vocab.map(|h| {
        let tokens = hex::encode(h.finalize());
        hex::encode(Sha256::digest(format!("{vocab_model}\0{tokens}")))
    });
    let get = |suffix: &str| -> u32 {
        let key = format!("{arch}.{suffix}");
        ints.iter()
            .find(|(k, _)| *k == key)
            .map_or(0, |(_, v)| u32::try_from(*v).unwrap_or(u32::MAX))
    };
    if arch.is_empty() {
        return Err("GGUF header has no general.architecture".into());
    }
    Ok(GgufMeta {
        block_count: get("block_count"),
        context_length: get("context_length"),
        embedding_length: get("embedding_length"),
        head_count: get("attention.head_count"),
        head_count_kv: get("attention.head_count_kv"),
        key_length: get("attention.key_length"),
        value_length: get("attention.value_length"),
        expert_count: get("expert_count"),
        file_mb: file_bytes / (1_024 * 1_024),
        tokenizer,
        nextn_layers: get("nextn_predict_layers"),
        arch,
    })
}

pub fn read_meta(path: &Path) -> Result<GgufMeta, String> {
    let f = File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let bytes = f.metadata().map(|m| m.len()).unwrap_or(0);
    parse_meta(&mut BufReader::new(f), bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Writes a GGUF v3 header with these keys.
    struct Header(Vec<u8>, u64);

    impl Header {
        fn new() -> Self {
            Header(Vec::new(), 0)
        }

        fn key(mut self, k: &str, t: u32) -> Self {
            self.0.extend((k.len() as u64).to_le_bytes());
            self.0.extend(k.as_bytes());
            self.0.extend(t.to_le_bytes());
            self.1 += 1;
            self
        }

        fn text(self, k: &str, v: &str) -> Self {
            let mut s = self.key(k, 8);
            s.0.extend((v.len() as u64).to_le_bytes());
            s.0.extend(v.as_bytes());
            s
        }

        fn u32(self, k: &str, v: u32) -> Self {
            let mut s = self.key(k, 4);
            s.0.extend(v.to_le_bytes());
            s
        }

        fn f32(self, k: &str, v: f32) -> Self {
            let mut s = self.key(k, 6);
            s.0.extend(v.to_le_bytes());
            s
        }

        fn i32_array(self, k: &str, v: &[i32]) -> Self {
            let mut s = self.key(k, 9);
            s.0.extend(5u32.to_le_bytes());
            s.0.extend((v.len() as u64).to_le_bytes());
            for x in v {
                s.0.extend(x.to_le_bytes());
            }
            s
        }

        fn text_array(self, k: &str, v: &[&str]) -> Self {
            let mut s = self.key(k, 9);
            s.0.extend(8u32.to_le_bytes());
            s.0.extend((v.len() as u64).to_le_bytes());
            for x in v {
                s.0.extend((x.len() as u64).to_le_bytes());
                s.0.extend(x.as_bytes());
            }
            s
        }

        fn bytes(&self) -> Vec<u8> {
            let mut out = b"GGUF".to_vec();
            out.extend(3u32.to_le_bytes());
            out.extend(0u64.to_le_bytes());
            out.extend(self.1.to_le_bytes());
            out.extend(&self.0);
            out
        }
    }

    #[test]
    fn a_header_gives_the_shape_the_auto_config_needs() {
        let h = Header::new()
            .text("general.architecture", "gemma4")
            .text_array("tokenizer.ggml.tokens", &["<pad>", "<eos>", "hello"])
            .u32("gemma4.block_count", 48)
            .u32("gemma4.context_length", 131_072)
            .u32("gemma4.embedding_length", 3_840)
            .u32("gemma4.attention.head_count", 16)
            .i32_array("gemma4.attention.head_count_kv", &[8, 8, 16, 8])
            .u32("gemma4.attention.key_length", 256)
            .u32("gemma4.attention.value_length", 256)
            .f32("gemma4.rope.freq_base", 1_000_000.0)
            .u32("general.alignment", 32);
        let meta = parse_meta(&mut h.bytes().as_slice(), 7_381_382_944).unwrap();
        assert_eq!(
            meta,
            GgufMeta {
                arch: "gemma4".into(),
                block_count: 48,
                context_length: 131_072,
                embedding_length: 3_840,
                head_count: 16,
                head_count_kv: 16,
                key_length: 256,
                value_length: 256,
                expert_count: 0,
                file_mb: 7_039,
                tokenizer: meta.tokenizer.clone(),
                nextn_layers: 0,
            }
        );
        assert_eq!(
            meta.tokenizer.as_deref().map(str::len),
            Some(64),
            "a vocabulary gives a SHA-256"
        );
        assert!(!meta.is_moe());
        assert_eq!(meta.kv_width(), 16 * 512);
    }

    #[test]
    fn head_dim_falls_back_to_embedding_over_heads_and_moe_reads_experts() {
        let h = Header::new()
            .text("general.architecture", "qwen3moe")
            .u32("qwen3moe.block_count", 48)
            .u32("qwen3moe.embedding_length", 2_048)
            .u32("qwen3moe.attention.head_count", 32)
            .u32("qwen3moe.attention.head_count_kv", 4)
            .u32("qwen3moe.expert_count", 128);
        let meta = parse_meta(&mut h.bytes().as_slice(), 0).unwrap();
        assert!(meta.is_moe());
        assert_eq!(meta.kv_width(), 4 * (64 + 64));
    }

    #[test]
    fn the_tokenizer_hash_matches_only_the_same_vocabulary() {
        let with = |model: &str, tokens: &[&str]| {
            let h = Header::new()
                .text("general.architecture", "gemma4")
                .text("tokenizer.ggml.model", model)
                .text_array("tokenizer.ggml.tokens", tokens)
                .u32("gemma4.nextn_predict_layers", 1);
            parse_meta(&mut h.bytes().as_slice(), 0).unwrap()
        };
        let big = with("gemma4", &["<pad>", "<eos>", "hello", "world"]);
        let small = with("gemma4", &["<pad>", "<eos>", "hello", "world"]);
        assert_eq!(big.nextn_layers, 1);
        assert!(big.tokenizer.is_some());
        assert_eq!(big.tokenizer, small.tokenizer);
        assert_ne!(
            big.tokenizer,
            with("gemma4", &["<pad>", "<eos>", "hello", "world!"]).tokenizer
        );
        assert_ne!(
            big.tokenizer,
            with("llama", &["<pad>", "<eos>", "hello", "world"]).tokenizer
        );
        // Splitting differently is a different vocabulary.
        assert_ne!(
            big.tokenizer,
            with("gemma4", &["<pad>", "<eos>", "helloworld"]).tokenizer
        );
        let none = Header::new().text("general.architecture", "llama");
        assert_eq!(
            parse_meta(&mut none.bytes().as_slice(), 0)
                .unwrap()
                .tokenizer,
            None
        );
    }

    #[test]
    fn bad_headers_are_refused() {
        assert_eq!(
            parse_meta(&mut b"GGML\x03\0\0\0".as_slice(), 0),
            Err("not a GGUF file".into())
        );
        let mut v1 = b"GGUF".to_vec();
        v1.extend(1u32.to_le_bytes());
        assert_eq!(
            parse_meta(&mut v1.as_slice(), 0),
            Err("GGUF version 1 is not supported".into())
        );
        let no_arch = Header::new().u32("x.block_count", 1);
        assert_eq!(
            parse_meta(&mut no_arch.bytes().as_slice(), 0),
            Err("GGUF header has no general.architecture".into())
        );
        let mut cut = Header::new().text("general.architecture", "llama").bytes();
        cut.truncate(cut.len() - 3);
        assert!(parse_meta(&mut cut.as_slice(), 0).is_err());
    }

    #[test]
    fn read_meta_opens_a_file_on_disk() {
        let dir = std::env::temp_dir().join(format!("grokhub-gguf-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("m.gguf");
        let h = Header::new()
            .text("general.architecture", "llama")
            .u32("llama.block_count", 16);
        std::fs::write(&path, h.bytes()).unwrap();
        assert_eq!(read_meta(&path).unwrap().block_count, 16);
        assert!(read_meta(&dir.join("missing.gguf")).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }
}
