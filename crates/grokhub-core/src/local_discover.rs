//! GGUF models already on this PC: LM Studio's folders, Ollama's blobs and
//! any folder the user adds. Read-only: files are listed where they are and
//! never copied, moved or written.

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    LmStudio,
    Ollama,
    Folder,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundModel {
    /// `publisher/file.gguf` (LM Studio), `name:tag` (Ollama) or the file name.
    pub name: String,
    pub path: PathBuf,
    pub bytes: u64,
    pub source: Source,
    /// Ollama names each blob by its SHA-256.
    pub sha256: Option<String>,
}

/// Folders deeper than this under a root are not walked.
const MAX_DEPTH: usize = 4;

/// LM Studio's model folders under `home`.
pub fn lm_studio_roots(home: &Path) -> Vec<PathBuf> {
    vec![
        home.join(".lmstudio").join("models"),
        home.join(".cache").join("lm-studio").join("models"),
    ]
}

/// Ollama's models folder: `OLLAMA_MODELS` when set, else `~/.ollama/models`.
pub fn ollama_root(home: &Path, env: Option<&str>) -> PathBuf {
    match env.map(str::trim).filter(|s| !s.is_empty()) {
        Some(dir) => PathBuf::from(dir),
        None => home.join(".ollama").join("models"),
    }
}

fn is_model_gguf(path: &Path) -> bool {
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    // Vision projectors ride along with a model; they are not one.
    name.ends_with(".gguf") && !name.starts_with("mmproj")
}

fn walk(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let Ok(kind) = e.file_type() else {
            continue;
        };
        let path = e.path();
        if kind.is_dir() && depth < MAX_DEPTH {
            walk(&path, depth + 1, out);
        } else if kind.is_file() && is_model_gguf(&path) {
            out.push(path);
        }
    }
}

fn found(root: &Path, path: PathBuf, source: Source) -> Option<FoundModel> {
    let bytes = fs::metadata(&path).ok()?.len();
    let name = path
        .strip_prefix(root)
        .ok()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|| path.display().to_string());
    Some(FoundModel {
        name,
        path,
        bytes,
        source,
        sha256: None,
    })
}

/// Every model GGUF under `roots`, named by its path under the root.
pub fn scan_folders(roots: &[PathBuf], source: Source) -> Vec<FoundModel> {
    let mut out = Vec::new();
    for root in roots {
        let mut files = Vec::new();
        walk(root, 0, &mut files);
        out.extend(files.into_iter().filter_map(|p| found(root, p, source)));
    }
    out
}

/// Ollama models: each manifest under `manifests/<registry>/<namespace>/<name>/<tag>`
/// points at its weights blob (`application/vnd.ollama.image.model`).
pub fn scan_ollama(root: &Path) -> Vec<FoundModel> {
    let mut manifests = Vec::new();
    walk_all(&root.join("manifests"), 0, &mut manifests);
    manifests
        .into_iter()
        .filter_map(|m| {
            let rel = m.strip_prefix(root.join("manifests")).ok()?;
            let parts: Vec<String> = rel
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect();
            let [_, ns, name, tag] = &parts[..] else {
                return None;
            };
            let json: serde_json::Value =
                serde_json::from_str(&fs::read_to_string(&m).ok()?).ok()?;
            let digest = json
                .get("layers")?
                .as_array()?
                .iter()
                .find(|l| {
                    l.get("mediaType").and_then(|t| t.as_str())
                        == Some("application/vnd.ollama.image.model")
                })?
                .get("digest")?
                .as_str()?;
            let hex = digest.strip_prefix("sha256:")?;
            if hex.len() != 64 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            let path = root.join("blobs").join(format!("sha256-{hex}"));
            let bytes = fs::metadata(&path).ok()?.len();
            let name = if ns == "library" {
                format!("{name}:{tag}")
            } else {
                format!("{ns}/{name}:{tag}")
            };
            Some(FoundModel {
                name,
                path,
                bytes,
                source: Source::Ollama,
                sha256: Some(hex.to_ascii_lowercase()),
            })
        })
        .collect()
}

fn walk_all(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        match e.file_type() {
            Ok(k) if k.is_dir() && depth < MAX_DEPTH => walk_all(&e.path(), depth + 1, out),
            Ok(k) if k.is_file() => out.push(e.path()),
            _ => {}
        }
    }
}

/// LM Studio, then Ollama, then the user's folders.
pub fn discover(home: &Path, ollama_env: Option<&str>, user_dirs: &[PathBuf]) -> Vec<FoundModel> {
    let mut out = scan_folders(&lm_studio_roots(home), Source::LmStudio);
    out.extend(scan_ollama(&ollama_root(home, ollama_env)));
    out.extend(scan_folders(user_dirs, Source::Folder));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(label: &str) -> PathBuf {
        let d =
            std::env::temp_dir().join(format!("grokhub-discover-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn put(path: &Path, body: &[u8]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    #[test]
    fn lm_studio_ollama_and_user_folders_are_listed_in_place() {
        let home = tmp("home");
        put(
            &home.join(".lmstudio/models/lmstudio-community/gemma-4-E4B-it-GGUF/gemma-4-E4B-it-Q4_K_M.gguf"),
            b"gguf-1",
        );
        put(
            &home.join(".lmstudio/models/lmstudio-community/gemma-4-E4B-it-GGUF/mmproj-gemma-4-E4B-it-BF16.gguf"),
            b"proj",
        );
        put(&home.join(".lmstudio/models/notes.txt"), b"x");
        put(
            &home.join(".cache/lm-studio/models/meta-models/Muse-Glimmer-30B-GGUF/m.gguf"),
            b"gguf-22",
        );
        let hex = "a".repeat(64);
        put(
            &home.join(format!(".ollama/models/blobs/sha256-{hex}")),
            b"blob-333",
        );
        let manifest = format!(
            r#"{{"layers":[{{"mediaType":"application/vnd.ollama.image.template","digest":"sha256:{t}"}},
                {{"mediaType":"application/vnd.ollama.image.model","digest":"sha256:{hex}"}}]}}"#,
            t = "b".repeat(64)
        );
        put(
            &home.join(".ollama/models/manifests/registry.ollama.ai/library/gemma4/e4b"),
            manifest.as_bytes(),
        );
        put(
            &home.join(".ollama/models/manifests/registry.ollama.ai/someone/tiny/latest"),
            manifest.as_bytes(),
        );
        // A manifest whose blob is gone lists nothing.
        put(
            &home.join(".ollama/models/manifests/registry.ollama.ai/library/gone/latest"),
            format!(
                r#"{{"layers":[{{"mediaType":"application/vnd.ollama.image.model","digest":"sha256:{}"}}]}}"#,
                "c".repeat(64)
            )
            .as_bytes(),
        );
        let mine = tmp("mine");
        put(&mine.join("Picked.GGUF"), b"gguf-4444");

        let got = discover(&home, None, std::slice::from_ref(&mine));
        let rows: Vec<(&str, u64, Source)> = got
            .iter()
            .map(|m| (m.name.as_str(), m.bytes, m.source))
            .collect();
        assert_eq!(
            rows,
            [
                (
                    "lmstudio-community/gemma-4-E4B-it-GGUF/gemma-4-E4B-it-Q4_K_M.gguf",
                    6,
                    Source::LmStudio
                ),
                (
                    "meta-models/Muse-Glimmer-30B-GGUF/m.gguf",
                    7,
                    Source::LmStudio
                ),
                ("gemma4:e4b", 8, Source::Ollama),
                ("someone/tiny:latest", 8, Source::Ollama),
                ("Picked.GGUF", 9, Source::Folder),
            ]
        );
        assert_eq!(got[2].sha256.as_deref(), Some(hex.as_str()));
        assert_eq!(
            got[2].path,
            home.join(".ollama/models/blobs")
                .join(format!("sha256-{hex}"))
        );
        // Read-only: nothing new appeared in the scanned folders.
        assert!(!home.join(".lmstudio/models/installed.json").exists());
        let _ = fs::remove_dir_all(home);
        let _ = fs::remove_dir_all(mine);
    }

    #[test]
    fn ollama_models_env_moves_the_root_and_missing_folders_list_nothing() {
        let home = Path::new("/nowhere/home");
        assert_eq!(
            ollama_root(home, Some("/srv/ollama")),
            PathBuf::from("/srv/ollama")
        );
        assert_eq!(ollama_root(home, Some("  ")), home.join(".ollama/models"));
        assert_eq!(
            discover(home, None, &[PathBuf::from("/nowhere/else")]),
            vec![]
        );
    }
}
