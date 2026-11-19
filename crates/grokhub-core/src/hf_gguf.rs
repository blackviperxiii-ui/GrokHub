//! Live Hugging Face GGUF browse for Download models: search, list quants,
//! and a coarse pre-download VRAM/RAM fit. Acquisition only — downloads land
//! in the models folder and join the Auto pool; nothing here picks a model
//! for a chat. Clean-room (Hermes `hf_browse` shape only).

use std::collections::BTreeMap;

use crate::model_library::Fit;

const HF: &str = "https://huggingface.co";
/// Rough KV + runtime overhead on top of GGUF file size (≈ in-memory weights).
const ROUGH_KV_AND_OVERHEAD_MIB: u64 = 4 * 1_024;
const MIB: u64 = 1_024 * 1_024;

/// One Hub model that ships GGUF files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub repo: String,
    pub downloads: u64,
    pub likes: u64,
    pub updated: String,
    pub gated: bool,
}

/// One downloadable quant: a single GGUF or all parts of a split file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileGroup {
    /// e.g. `Q4_K_M` or the file stem when no quant tag is found.
    pub label: String,
    /// Repo-relative paths; split parts stay in order.
    pub paths: Vec<String>,
    pub total_bytes: u64,
    pub fit: Fit,
}

/// Full-text search URL over Hub models that ship GGUF, most downloaded first.
pub fn search_url(query: &str, limit: usize) -> String {
    let q = urlencoding_loose(query.trim());
    let n = limit.clamp(1, 50);
    format!("{HF}/api/models?search={q}&filter=gguf&sort=downloads&direction=-1&limit={n}")
}

/// Recursive tree listing for `repo` on `main`.
pub fn tree_url(repo: &str) -> String {
    let repo = repo.trim().trim_matches('/');
    format!("{HF}/api/models/{repo}/tree/main?recursive=true")
}

/// Resolve URL for one file in `repo`.
pub fn file_url(repo: &str, path: &str) -> String {
    let repo = repo.trim().trim_matches('/');
    format!("{HF}/{repo}/resolve/main/{path}")
}

/// Hits from a `/api/models` search body.
pub fn parse_search(body: &str) -> Vec<Hit> {
    let Ok(serde_json::Value::Array(items)) = serde_json::from_str::<serde_json::Value>(body)
    else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|m| {
            let repo = m.get("id")?.as_str()?.to_string();
            if !repo.contains('/') {
                return None;
            }
            Some(Hit {
                repo,
                downloads: m.get("downloads").and_then(|v| v.as_u64()).unwrap_or(0),
                likes: m.get("likes").and_then(|v| v.as_u64()).unwrap_or(0),
                updated: m
                    .get("lastModified")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
                gated: m.get("gated").and_then(|v| v.as_bool()).unwrap_or(false),
            })
        })
        .collect()
}

/// Servable GGUFs in a tree listing, grouped; MTP / mmproj / draft skipped.
/// Largest first. `fit` starts as [`Fit::No`] until [`price_groups`] runs.
pub fn parse_tree(body: &str) -> Vec<FileGroup> {
    let Ok(serde_json::Value::Array(files)) = serde_json::from_str::<serde_json::Value>(body)
    else {
        return Vec::new();
    };
    let mut singles: Vec<(String, u64)> = Vec::new();
    let mut splits: BTreeMap<String, Vec<(u32, String, u64)>> = BTreeMap::new();
    for f in files {
        let path = match f.get("path").and_then(|v| v.as_str()) {
            Some(p) if p.to_ascii_lowercase().ends_with(".gguf") => p.to_string(),
            _ => continue,
        };
        let name = path
            .rsplit('/')
            .next()
            .unwrap_or(path.as_str())
            .to_ascii_lowercase();
        if name.starts_with("mmproj")
            || name.starts_with("mtp-")
            || name.contains("draft")
            || name.starts_with("dspark")
        {
            continue;
        }
        let size = f.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
        if let Some((stem, part, of)) = split_of(&path) {
            if part >= 1 && of >= 1 {
                splits
                    .entry(stem.to_string())
                    .or_default()
                    .push((part, path, size));
            }
        } else {
            singles.push((path, size));
        }
    }
    let mut groups: Vec<FileGroup> = singles
        .into_iter()
        .map(|(path, size)| FileGroup {
            label: quant_label(&path),
            paths: vec![path],
            total_bytes: size,
            fit: Fit::No,
        })
        .collect();
    for (stem, mut parts) in splits {
        parts.sort_by_key(|(n, _, _)| *n);
        let total: u64 = parts.iter().map(|(_, _, s)| *s).sum();
        groups.push(FileGroup {
            label: quant_label(&stem),
            paths: parts.into_iter().map(|(_, p, _)| p).collect(),
            total_bytes: total,
            fit: Fit::No,
        });
    }
    groups.sort_by_key(|g| std::cmp::Reverse(g.total_bytes));
    groups
}

/// Stamp each group with a coarse fit for this PC's usable VRAM and spare RAM.
pub fn price_groups(groups: &[FileGroup], usable_vram_mb: u64, ram_available_mb: u64) -> Vec<FileGroup> {
    groups
        .iter()
        .map(|g| {
            let mut next = g.clone();
            next.fit = rough_fit(g.total_bytes, usable_vram_mb, ram_available_mb);
            next
        })
        .collect()
}

/// Coarse pre-download verdict from file size alone.
pub fn rough_fit(total_bytes: u64, usable_vram_mb: u64, ram_available_mb: u64) -> Fit {
    let need_mb = total_bytes / MIB + ROUGH_KV_AND_OVERHEAD_MIB;
    if need_mb <= usable_vram_mb {
        Fit::Yes
    } else if need_mb <= usable_vram_mb.saturating_add(ram_available_mb) {
        Fit::Tight
    } else {
        Fit::No
    }
}

/// Quant tag from a filename, or the stem.
pub fn quant_label(filename: &str) -> String {
    let name = filename.rsplit('/').next().unwrap_or(filename);
    let upper = name.to_ascii_uppercase();
    // IQ / Q variants, then F16 / BF16 / F32.
    for (i, _) in upper.char_indices() {
        let rest = &upper[i..];
        if let Some(tag) = take_quant(rest) {
            return tag;
        }
    }
    name.trim_end_matches(".gguf")
        .trim_end_matches(".GGUF")
        .to_string()
}

fn take_quant(s: &str) -> Option<String> {
    const FIXED: [&str; 3] = ["BF16", "F16", "F32"];
    for tag in FIXED {
        if s.starts_with(tag)
            && s[tag.len()..]
                .chars()
                .next()
                .is_none_or(|c| !c.is_ascii_alphanumeric() && c != '_')
        {
            return Some(tag.into());
        }
    }
    // Q4_K_M, IQ3_XXS, Q5_0, …
    let bytes = s.as_bytes();
    let start = if bytes.starts_with(b"IQ") {
        2
    } else if bytes.first() == Some(&b'Q') {
        1
    } else {
        return None;
    };
    let mut i = start;
    if i >= bytes.len() || !bytes[i].is_ascii_digit() {
        return None;
    }
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i < bytes.len() && bytes[i] == b'_' {
        i += 1;
        while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
            i += 1;
        }
    }
    let tag = &s[..i];
    if tag.len() > start {
        Some(tag.to_string())
    } else {
        None
    }
}

/// `-00001-of-00002.gguf` → (stem, part, of).
fn split_of(path: &str) -> Option<(&str, u32, u32)> {
    let lower = path.to_ascii_lowercase();
    let end = lower.strip_suffix(".gguf")?;
    let (left, of_s) = end.rsplit_once("-of-")?;
    let (_stem, part_s) = left.rsplit_once('-')?;
    let part: u32 = part_s.parse().ok()?;
    let of: u32 = of_s.parse().ok()?;
    // Keep the original casing stem up to the `-NNNNN-of-…` marker.
    let marker_len = part_s.len() + "-of-".len() + of_s.len() + 1 + ".gguf".len();
    let cut = path.len().checked_sub(marker_len)?;
    Some((&path[..cut], part, of))
}

/// Encode a query the Hub accepts: alphanumeric and a few marks stay.
fn urlencoding_loose(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            out.push(b as char);
        } else if b == b' ' {
            out.push_str("%20");
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_url_clamps_and_encodes() {
        assert_eq!(
            search_url("qwen 3", 0),
            "https://huggingface.co/api/models?search=qwen%203&filter=gguf&sort=downloads&direction=-1&limit=1"
        );
        assert!(search_url("x", 99).ends_with("limit=50"));
    }

    #[test]
    fn parse_search_keeps_repo_hits() {
        let body = r#"[
          {"id":"unsloth/Qwen3-8B-GGUF","downloads":10,"likes":2,"lastModified":"2026-01-01","gated":false},
          {"id":"solo","downloads":1}
        ]"#;
        let hits = parse_search(body);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].repo, "unsloth/Qwen3-8B-GGUF");
        assert_eq!(hits[0].downloads, 10);
    }

    #[test]
    fn tree_groups_quants_and_skips_helpers() {
        let body = r#"[
          {"type":"file","path":"model-Q4_K_M.gguf","size":4000000000},
          {"type":"file","path":"model-Q8_0.gguf","size":8000000000},
          {"type":"file","path":"mmproj-f16.gguf","size":100},
          {"type":"file","path":"mtp-Q4_0.gguf","size":100},
          {"type":"file","path":"model-00001-of-00002.gguf","size":1000},
          {"type":"file","path":"model-00002-of-00002.gguf","size":500}
        ]"#;
        let groups = parse_tree(body);
        assert_eq!(groups.len(), 3);
        assert_eq!(groups[0].label, "Q8_0");
        assert_eq!(groups[0].total_bytes, 8_000_000_000);
        assert_eq!(groups[1].label, "Q4_K_M");
        let split = groups.iter().find(|g| g.paths.len() == 2).unwrap();
        assert_eq!(split.total_bytes, 1_500);
        assert_eq!(
            split.paths,
            [
                "model-00001-of-00002.gguf".to_string(),
                "model-00002-of-00002.gguf".to_string()
            ]
        );
    }

    #[test]
    fn rough_fit_bands_match_fit_labels() {
        // 2 GiB file + 4 GiB overhead = 6 GiB need.
        let bytes = 2 * MIB * 1_024;
        assert_eq!(rough_fit(bytes, 8 * 1_024, 0), Fit::Yes);
        assert_eq!(rough_fit(bytes, 4 * 1_024, 4 * 1_024), Fit::Tight);
        assert_eq!(rough_fit(bytes, 1_024, 1_024), Fit::No);
        assert_eq!(Fit::Yes.label(), "Fits this PC");
        assert_eq!(Fit::Tight.label(), "Fits, may be slow");
        assert_eq!(Fit::No.label(), "Won't run well here");
    }

    #[test]
    fn quant_label_reads_common_tags() {
        assert_eq!(quant_label("Llama-3-8B.Q4_K_M.gguf"), "Q4_K_M");
        assert_eq!(quant_label("x-IQ3_XXS.gguf"), "IQ3_XXS");
        assert_eq!(quant_label("weights-F16.gguf"), "F16");
        assert_eq!(quant_label("plain.gguf"), "plain");
    }
}
