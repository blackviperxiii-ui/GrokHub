//! `llama-server` router-mode preset generation (`--models-preset` +
//! `--models-max`), the `--mmproj` projector a picture launch carries, and local
//! embedding GGUF discovery (Card 955 / Card 917.6 follow-on).
//!
//! Router mode lets a single loopback `llama-server` supervise multiple models
//! (e.g. a chat/vision model paired with its draft model plus a dedicated
//! embedding model) without restarting the server on every swap, while
//! preserving Loadout's strict draft pairing (`local_spec.rs`), VRAM/RAM
//! residency ledger (`local_resident.rs`), and thermal/gaming governor.

use std::path::{Path, PathBuf};

use super::local_autoconfig::LaunchArgs;

/// Default maximum simultaneously resident models in router mode: allows a
/// chat/vision model and an embedding model (or fast swap pair) without
/// exceeding typical desktop VRAM/RAM budgets.
pub const DEFAULT_MODELS_MAX: u32 = 2;

/// Canonical section ID for the primary chat (and optional vision) slot.
pub const SLOT_CHAT: &str = "chat";
/// Canonical section ID for the managed local embeddings slot.
pub const SLOT_EMBED: &str = "embed";

/// One model entry inside a `llama-server --models-preset` INI file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresetModel {
    pub id: String,
    pub model: PathBuf,
    pub mmproj: Option<PathBuf>,
    pub draft_model: Option<PathBuf>,
    pub draft_max: Option<u32>,
    pub draft_min: Option<u32>,
    pub n_gpu_layers: Option<u32>,
    pub ctx_size: Option<u32>,
    pub embeddings: bool,
    pub extra: Vec<(String, String)>,
}

/// Multi-model preset for `llama-server --models-preset`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouterPreset {
    pub models_max: u32,
    pub models: Vec<PresetModel>,
}

impl RouterPreset {
    /// Build a router preset from a planned chat model, its optional
    /// auto-configured [`LaunchArgs`] (preserving draft pairing, `ngl`, and
    /// `ctx`), the vision projector the launch carries (`--mmproj`, only for
    /// a picture turn), and an optional local embedding GGUF in `models_dir`.
    pub fn from_sidecar_plan(
        chat_model: &Path,
        launch: Option<&LaunchArgs>,
        models_dir: Option<&Path>,
    ) -> Self {
        // The projector a picture turn planned and counted, never one found
        // in the folder.
        let mmproj = launch.and_then(|l| Some(l.projector.as_ref()?.file.clone()));
        let (draft_model, draft_max, draft_min, n_gpu_layers, ctx_size) = match launch {
            Some(l) => {
                let (dm, dmax) = match l.draft.as_deref() {
                    Some(d) => {
                        let max = match d.kind {
                            grokhub_core::local_draft::DraftKind::Simple => {
                                Some(grokhub_core::local_draft::DRAFT_MAX)
                            }
                            grokhub_core::local_draft::DraftKind::Mtp if d.model.is_some() => {
                                Some(grokhub_core::local_draft::MTP_FILE_DRAFT_MAX)
                            }
                            _ => None,
                        };
                        (d.model.clone(), max)
                    }
                    None => (None, None),
                };
                let ngl = if l.cpu_only { Some(0) } else { Some(l.ngl) };
                (dm, dmax, None, ngl, Some(l.ctx))
            }
            None => (None, None, None, None, None),
        };

        let mut models = vec![PresetModel {
            id: SLOT_CHAT.into(),
            model: chat_model.to_path_buf(),
            mmproj,
            draft_model,
            draft_max,
            draft_min,
            n_gpu_layers,
            ctx_size,
            embeddings: false,
            extra: Vec::new(),
        }];

        if let Some(dir) = models_dir
            && let Some(embed_path) = discover_embedding_model(dir)
            && embed_path != chat_model
        {
            models.push(PresetModel {
                id: SLOT_EMBED.into(),
                model: embed_path,
                mmproj: None,
                draft_model: None,
                draft_max: None,
                draft_min: None,
                n_gpu_layers: None,
                ctx_size: Some(2048),
                embeddings: true,
                extra: Vec::new(),
            });
        }

        Self {
            models_max: DEFAULT_MODELS_MAX,
            models,
        }
    }

    /// True if any model in this preset carries an `--mmproj` projector.
    pub fn has_vision(&self) -> bool {
        self.models.iter().any(|m| m.mmproj.is_some())
    }

    /// The preset section ID of the dedicated local embeddings slot, if any.
    pub fn embedding_model_id(&self) -> Option<&str> {
        self.models
            .iter()
            .find(|m| m.embeddings)
            .map(|m| m.id.as_str())
    }

    /// Render a deterministic `llama-server --models-preset` INI document.
    pub fn render_ini(&self) -> String {
        let mut out = String::new();
        out.push_str("# Managed by Loadout local router\n");
        out.push_str("version = 1\n\n");
        for m in &self.models {
            let section = sanitize_section(&m.id);
            out.push('[');
            out.push_str(&section);
            out.push_str("]\n");
            push_kv(&mut out, "model", &m.model.to_string_lossy());
            if let Some(mmproj) = &m.mmproj {
                push_kv(&mut out, "mmproj", &mmproj.to_string_lossy());
            }
            if let Some(draft) = &m.draft_model {
                push_kv(&mut out, "model-draft", &draft.to_string_lossy());
            }
            if let Some(dmax) = m.draft_max {
                push_kv(&mut out, "draft-max", &dmax.to_string());
            }
            if let Some(dmin) = m.draft_min {
                push_kv(&mut out, "draft-min", &dmin.to_string());
            }
            if let Some(ngl) = m.n_gpu_layers {
                push_kv(&mut out, "n-gpu-layers", &ngl.to_string());
            }
            if let Some(ctx) = m.ctx_size {
                push_kv(&mut out, "ctx-size", &ctx.to_string());
            }
            if m.embeddings {
                push_kv(&mut out, "embeddings", "true");
            }
            for (k, v) in &m.extra {
                push_kv(&mut out, k, v);
            }
            out.push('\n');
        }
        out
    }

    /// Atomically write the rendered INI preset to `path`.
    pub fn write_to(&self, path: &Path) -> Result<PathBuf, String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create preset dir {}: {e}", parent.display()))?;
        }
        let body = self.render_ini();
        let tmp = path.with_extension("ini.tmp");
        std::fs::write(&tmp, body.as_bytes())
            .map_err(|e| format!("write preset {}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, path)
            .map_err(|e| format!("rename preset {}: {e}", path.display()))?;
        Ok(path.to_path_buf())
    }
}

fn sanitize_section(raw: &str) -> String {
    let cleaned: String = raw
        .chars()
        .filter(|c| !matches!(c, '[' | ']' | '\r' | '\n'))
        .collect();
    let trimmed = cleaned.trim();
    if trimmed.is_empty() {
        SLOT_CHAT.to_string()
    } else {
        trimmed.to_string()
    }
}

fn push_kv(out: &mut String, key: &str, value: &str) {
    let k: String = key
        .chars()
        .filter(|c| !matches!(c, '=' | '\r' | '\n'))
        .collect();
    let v: String = value
        .chars()
        .filter(|c| !matches!(c, '\r' | '\n'))
        .collect();
    out.push_str(k.trim());
    out.push_str(" = ");
    out.push_str(v.trim());
    out.push('\n');
}

/// Command-line arguments for launching `llama-server` in `--models-preset`
/// router mode locked to loopback (`127.0.0.1` / `localhost`).
pub fn llama_router_args(
    preset_path: &Path,
    host: &str,
    port: u16,
    models_max: u32,
) -> Vec<String> {
    vec![
        "--models-preset".into(),
        preset_path.display().to_string(),
        "--models-max".into(),
        models_max.max(1).to_string(),
        "--host".into(),
        host.into(),
        "--port".into(),
        port.to_string(),
        "--reasoning-format".into(),
        "deepseek".into(),
        // Same tool-call grammar as a single-model start (`base_args`).
        "--jinja".into(),
    ]
}

/// True when the installed `llama-server --help` advertises `--models-preset`.
pub fn supports_router_mode(help_text: &str) -> bool {
    help_text.contains("--models-preset")
}

/// Discover a resident local embedding GGUF in `models_dir` (one level deep or
/// inside immediate model subdirectories). Skips `mmproj` and `draft` files.
pub fn discover_embedding_model(models_dir: &Path) -> Option<PathBuf> {
    let mut candidates = Vec::new();
    collect_embedding_ggufs(models_dir, 0, &mut candidates);
    candidates.sort();
    candidates.into_iter().next()
}

fn collect_embedding_ggufs(dir: &Path, depth: u8, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && depth < 2 {
            collect_embedding_ggufs(&path, depth + 1, out);
        } else if path.is_file() && is_embedding_gguf(&path) {
            out.push(path);
        }
    }
}

pub fn is_embedding_gguf(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    let lower = name.to_ascii_lowercase();
    if !lower.ends_with(".gguf") || lower.contains("mmproj") || lower.contains("draft") {
        return false;
    }
    const MARKERS: &[&str] = &[
        "embed",
        "nomic-embed",
        "bge-",
        "e5-",
        "gte-",
        "minilm",
        "mxbai-embed",
        "snowflake-arctic-embed",
    ];
    MARKERS.iter().any(|m| lower.contains(m))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::route::local_autoconfig::{KvType, LaunchArgs, ProjectorArgs};
    use crate::route::local_spec::DraftArgs;
    use grokhub_core::local_draft::{DraftFlags, DraftKind};
    use grokhub_core::local_profile::Backend;

    #[test]
    fn renders_ini_with_chat_draft_mmproj_and_embeddings() {
        let dir = std::env::temp_dir().join(format!("loadout-preset-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let chat_dir = dir.join("qwen3-vl");
        let embed_dir = dir.join("nomic");
        std::fs::create_dir_all(&chat_dir).unwrap();
        std::fs::create_dir_all(&embed_dir).unwrap();

        let chat = chat_dir.join("Qwen3-VL-4B-Q4_K_M.gguf");
        let mmproj = chat_dir.join("mmproj-Qwen3-VL-4B-F16.gguf");
        let draft = chat_dir.join("Qwen3-0.6B-draft.gguf");
        let embed = embed_dir.join("nomic-embed-text-v1.5-Q8_0.gguf");
        std::fs::write(&chat, b"chat").unwrap();
        std::fs::write(&mmproj, b"proj").unwrap();
        std::fs::write(&draft, b"draft").unwrap();
        std::fs::write(&embed, b"embed").unwrap();

        let launch = LaunchArgs {
            backend: Backend::Cpu,
            ngl: 32,
            all_layers: 32,
            n_cpu_moe: None,
            ctx: 16_384,
            kv: KvType::Q8,
            threads: 8,
            threads_batch: None,
            flash_attn: true,
            mlock: false,
            tensor_split: None,
            draft: Some(Box::new(DraftArgs {
                kind: DraftKind::Simple,
                model: Some(draft.clone()),
                on_gpu: true,
                flags: DraftFlags::default(),
            })),
            cpu_only: false,
            run: Default::default(),
            projector: None,
        };

        // A projector beside the model in the folder isn't loaded on its own:
        // only a launch planned for a picture carries one.
        let plain = RouterPreset::from_sidecar_plan(&chat, Some(&launch), Some(&dir));
        assert!(!plain.has_vision());
        assert!(!plain.render_ini().contains("mmproj"));

        let mut launch = launch;
        launch.projector = Some(Box::new(ProjectorArgs {
            file: mmproj.clone(),
            mb: 1,
            off_gpu_flag: false,
        }));
        let preset = RouterPreset::from_sidecar_plan(&chat, Some(&launch), Some(&dir));
        assert!(preset.has_vision());
        assert_eq!(preset.embedding_model_id(), Some(SLOT_EMBED));
        assert_eq!(preset.models_max, DEFAULT_MODELS_MAX);

        let ini = preset.render_ini();
        assert!(ini.contains("[chat]\n"));
        assert!(ini.contains(&format!("model = {}\n", chat.display())));
        assert!(ini.contains(&format!("mmproj = {}\n", mmproj.display())));
        assert!(ini.contains(&format!("model-draft = {}\n", draft.display())));
        assert!(ini.contains(&format!(
            "draft-max = {}\n",
            grokhub_core::local_draft::DRAFT_MAX
        )));
        assert!(ini.contains("n-gpu-layers = 32\n"));
        assert!(ini.contains("ctx-size = 16384\n"));
        assert!(ini.contains("[embed]\n"));
        assert!(ini.contains(&format!("model = {}\n", embed.display())));
        assert!(ini.contains("embeddings = true\n"));

        let written = preset.write_to(&dir.join("preset.ini")).unwrap();
        assert_eq!(std::fs::read_to_string(written).unwrap(), ini);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn router_args_include_preset_models_max_and_loopback() {
        let args = llama_router_args(Path::new("/tmp/preset.ini"), "127.0.0.1", 8089, 2);
        let after = |flag: &str| {
            args.iter()
                .position(|a| a == flag)
                .and_then(|i| args.get(i + 1))
                .cloned()
        };
        assert_eq!(after("--models-preset").as_deref(), Some("/tmp/preset.ini"));
        assert_eq!(after("--models-max").as_deref(), Some("2"));
        assert_eq!(after("--host").as_deref(), Some("127.0.0.1"));
        assert_eq!(after("--port").as_deref(), Some("8089"));
        assert_eq!(after("--reasoning-format").as_deref(), Some("deepseek"));
        assert!(args.iter().any(|a| a == "--jinja"));
    }

    #[test]
    fn embedding_discovery_skips_mmproj_and_draft() {
        assert!(is_embedding_gguf(Path::new(
            "/m/bge-small-en-v1.5-Q8_0.gguf"
        )));
        assert!(is_embedding_gguf(Path::new("/m/nomic-embed-text.gguf")));
        assert!(!is_embedding_gguf(Path::new("/m/mmproj-embed-f16.gguf")));
        assert!(!is_embedding_gguf(Path::new("/m/embed-draft.gguf")));
        assert!(!is_embedding_gguf(Path::new("/m/qwen3-8b-Q4_K_M.gguf")));
    }

    #[test]
    fn detects_router_mode_flag_in_help() {
        assert!(supports_router_mode(
            "  --models-preset PATH  router presets"
        ));
        assert!(!supports_router_mode("  -m, --model FILE  model path"));
    }
}
