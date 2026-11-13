//! The on-device picture engine: stable-diffusion.cpp's `sd-cli`, pinned per
//! OS, downloaded next to llama.cpp in `{config}/bin/sd-cpp`. It runs once
//! per picture and exits, so the image model loads on demand and nothing
//! stays in memory beside a game.
//!
//! Linux and Windows use the Vulkan builds (any recent GPU, no CUDA
//! install); Apple silicon uses Metal. The archives are flat: the program and its libraries side by side, found
//! through `$ORIGIN` on Linux and the program's folder on Windows.

use std::path::{Path, PathBuf};

use crate::local_setup::{LocalModel, ModelKind};
use crate::model_download::{self as md, Control, Progress};
use crate::runtime_download::{Fetch, Outcome, PlatformAsset};
use crate::tool_engine::{Build, Tool};

/// The stable-diffusion.cpp release every platform pins.
pub const VERSION: &str = "master-929-3f8527a";

pub const SD_CPP: Tool = Tool {
    name: "sd-cpp",
    version: VERSION,
    program: "sd-cli",
    one_top_folder: false,
    builds: &[
        Build {
            key: "linux-x86_64",
            url: "https://github.com/leejet/stable-diffusion.cpp/releases/download/master-929-3f8527a/sd-master-3f8527a-bin-Linux-Ubuntu-24.04-x86_64-vulkan.zip",
            size: 36_886_217,
            sha256: "e35cc73cf5ba9637d1dc1d717760e7b8428376a4905d57e72ec8c871864f62c7",
            format: "zip",
        },
        Build {
            key: "windows-x86_64",
            url: "https://github.com/leejet/stable-diffusion.cpp/releases/download/master-929-3f8527a/sd-master-3f8527a-bin-win-vulkan-x64.zip",
            size: 30_067_605,
            sha256: "60e6850d650417409f18c2170ab5e27335db96da70cd3d1ad1e930bcffc2fd35",
            format: "zip",
        },
        // Apple silicon: Metal is built in (ggml's Apple default).
        Build {
            key: "macos-aarch64",
            url: "https://github.com/leejet/stable-diffusion.cpp/releases/download/master-929-3f8527a/sd-master-3f8527a-bin-Darwin-macOS-26.6.2-arm64.zip",
            size: 35_049_086,
            sha256: "1c8ee6c8e413e3335b1223bbc657ea5d86dae1819f8426a98b84c265587eb192",
            format: "zip",
        },
    ],
};

pub fn asset_for(key: &str) -> Option<PlatformAsset> {
    SD_CPP.asset_for(key)
}

pub fn cli_path(config_dir: &Path) -> PathBuf {
    SD_CPP.program_path(config_dir)
}

pub fn installed(config_dir: &Path) -> bool {
    SD_CPP.installed(config_dir)
}

/// Download, check and unpack the engine for `key`; a no-op once installed.
pub fn download(
    config_dir: &Path,
    key: &str,
    fetch: &dyn Fetch,
    ctl: &Control,
    on: &mut dyn FnMut(Progress),
) -> Result<PathBuf, Outcome> {
    SD_CPP.download(config_dir, key, fetch, ctl, on)
}

/// The first catalog picture model whose files are all in `models_dir`'s
/// image folder.
pub fn image_model(models_dir: &Path) -> Option<&'static LocalModel> {
    let dir = md::kind_dir(models_dir, ModelKind::Image);
    crate::local_setup::local_models()
        .iter()
        .find(|m| m.kind == ModelKind::Image && md::complete(&dir, m))
}

/// The first catalog picture guard whose files are all on disk.
pub fn guard_model(models_dir: &Path) -> Option<&'static LocalModel> {
    let dir = md::kind_dir(models_dir, ModelKind::ImageGuard);
    crate::local_setup::local_models()
        .iter()
        .find(|m| m.kind == ModelKind::ImageGuard && md::complete(&dir, m))
}

/// A part of `m` told apart by the end of its file name.
fn part(models_dir: &Path, m: &LocalModel, end: &str) -> Option<PathBuf> {
    let dir = md::kind_dir(models_dir, m.kind);
    m.parts
        .iter()
        .find(|p| p.file.ends_with(end))
        .map(|p| dir.join(p.file))
}

/// Picture size for an aspect ratio: about a megapixel, both sides a
/// multiple of 64, which Z-Image draws best. Unknown or blank is square.
pub fn size_for(aspect: &str) -> (u32, u32) {
    match aspect.trim() {
        "16:9" | "19.5:9" | "20:9" | "21:9" | "2:1" | "5:2" => (1344, 768),
        "9:16" | "9:19.5" | "9:20" | "1:2" => (768, 1344),
        "4:3" => (1152, 896),
        "3:4" => (896, 1152),
        "3:2" => (1216, 832),
        "2:3" => (832, 1216),
        _ => (1024, 1024),
    }
}

/// Threads for one picture: most of the work is on the graphics card, so a
/// few CPU threads are plenty and leave the rest of the machine free.
const THREADS: u32 = 4;

/// The `sd-cli` arguments for one picture. The diffusion model is the main
/// file and the VAE is a part. A Z-Image style row has a Qwen text encoder
/// (`--llm`) and runs with guidance off (cfg 1) and 8 steps; a Chroma row
/// has a T5 encoder (`--t5xxl`), guidance 4 and the Euler sampler, as
/// stable-diffusion.cpp's Chroma notes ask. Encoders and VAE wait in system
/// memory until their stage, so a game keeps its VRAM.
pub fn generate_args(
    models_dir: &Path,
    m: &LocalModel,
    prompt: &str,
    (width, height): (u32, u32),
    seed: u64,
    out: &Path,
) -> Option<Vec<String>> {
    let dir = md::kind_dir(models_dir, m.kind);
    let s = |p: PathBuf| p.display().to_string();
    let vae = part(models_dir, m, "ae.safetensors")?;
    let t5 = m
        .parts
        .iter()
        .find(|p| p.file.contains("t5"))
        .map(|p| dir.join(p.file));
    let mut a: Vec<String> = vec!["--diffusion-model".into(), s(md::final_path(&dir, m))];
    match t5 {
        Some(t5) => a.extend([
            "--t5xxl".into(),
            s(t5),
            "--cfg-scale".into(),
            "4.0".into(),
            "--sampling-method".into(),
            "euler".into(),
            "--steps".into(),
            "26".into(),
            "--model-args".into(),
            "chroma_use_dit_mask=false".into(),
            "--clip-on-cpu".into(),
        ]),
        None => a.extend([
            "--llm".into(),
            s(part(models_dir, m, ".gguf")?),
            "--cfg-scale".into(),
            "1.0".into(),
            "--steps".into(),
            "8".into(),
        ]),
    }
    a.extend([
        "--vae".into(),
        s(vae),
        "-p".into(),
        prompt.into(),
        "-W".into(),
        width.to_string(),
        "-H".into(),
        height.to_string(),
        "--seed".into(),
        seed.to_string(),
        "-t".into(),
        THREADS.to_string(),
        "--diffusion-fa".into(),
        "--offload-to-cpu".into(),
        "-o".into(),
        s(out.to_path_buf()),
    ]);
    Some(a)
}

/// The guard's model and image reader, for `llama-server -m … --mmproj …`.
pub fn guard_files(models_dir: &Path, g: &LocalModel) -> Option<(PathBuf, PathBuf)> {
    let dir = md::kind_dir(models_dir, g.kind);
    Some((md::final_path(&dir, g), part(models_dir, g, ".gguf")?))
}

/// What the guard is asked about every finished picture: the hard bans, one
/// word back.
pub const GUARD_QUESTION: &str = "You check pictures before anyone sees them. Look at this picture and answer with one word. Answer BANNED if it shows any of these: anyone who is or looks under 18 in a sexual, nude or suggestive way; sexual violence or anything sexual without consent; anything sexual with an animal; a real, recognizable person (a celebrity or public figure) nude or in sexual content. Otherwise answer SAFE.";

/// The guard's answer: `Some(true)` banned, `Some(false)` safe, `None` when
/// it said neither, which the caller treats as banned.
pub fn guard_verdict(reply: &str) -> Option<bool> {
    let up = reply.to_ascii_uppercase();
    if up.contains("BANNED") {
        Some(true)
    } else if up
        .trim_start()
        .trim_start_matches(['*', '"', '\''])
        .starts_with("SAFE")
    {
        Some(false)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_download as rd;

    #[test]
    fn linux_windows_and_apple_silicon_pin_the_release_and_others_have_none() {
        for b in SD_CPP.builds {
            let a = asset_for(b.key).unwrap();
            assert_eq!(a.archive_format(), Some(rd::ArchiveFormat::Zip));
            assert!(crate::ssrf::runtime_download_url_ok(b.url).is_ok());
            assert!(b.url.contains(VERSION));
            assert_eq!(b.url.contains("vulkan"), !b.key.starts_with("macos"));
            assert!(b.size > 1_000_000);
            assert!(b.sha256.len() == 64 && b.sha256.bytes().all(|c| c.is_ascii_hexdigit()));
        }
        assert!(asset_for("macos-aarch64").is_some());
        assert!(asset_for("macos-x86_64").is_none());
        assert!(asset_for("linux-aarch64").is_none());
        assert!(cli_path(Path::new("/c")).starts_with(rd::bin_dir(Path::new("/c")).join("sd-cpp")));
        assert!(!installed(Path::new("/nowhere")));
    }

    #[test]
    fn the_picture_model_ships_its_encoder_and_vae_in_its_own_folder() {
        let m = crate::local_setup::local_models()
            .iter()
            .find(|m| m.kind == ModelKind::Image)
            .expect("a picture row");
        assert!(!m.is_chat() && !crate::local_setup::counts_toward_cap(m));
        let folder = Path::new(m.file).parent().unwrap();
        assert!(m
            .parts
            .iter()
            .all(|p| Path::new(p.file).parent() == Some(folder)));
        assert!(m.parts.iter().any(|p| p.file.ends_with("ae.safetensors")));
        assert!(m.parts.iter().any(|p| p.file.contains("Qwen3-4B")));
        assert_eq!(image_model(Path::new("/nowhere")), None);
        let guard = crate::local_setup::local_models()
            .iter()
            .find(|m| m.kind == ModelKind::ImageGuard)
            .expect("a picture guard row");
        assert!(guard.parts.iter().any(|p| p.file.contains("mmproj")));
    }

    #[test]
    fn one_picture_is_one_turbo_run_with_the_encoder_and_vae_beside_it() {
        let models = Path::new("/m");
        let m = crate::local_setup::local_models()
            .iter()
            .find(|m| m.kind == ModelKind::Image)
            .unwrap();
        let out = Path::new("/out/a.png");
        let a = generate_args(models, m, "a red fox in snow", size_for("16:9"), 7, out).unwrap();
        let after = |k: &str| a[a.iter().position(|x| x == k).unwrap() + 1].clone();
        let img = md::kind_dir(models, ModelKind::Image);
        assert_eq!(
            after("--diffusion-model"),
            img.join(m.file).display().to_string()
        );
        assert!(after("--llm").ends_with("Qwen3-4B-Instruct-2507-Q4_K_M.gguf"));
        assert!(after("--vae").ends_with("ae.safetensors"));
        assert_eq!(after("-p"), "a red fox in snow");
        assert_eq!(after("--cfg-scale"), "1.0");
        assert_eq!(after("--steps"), "8");
        assert_eq!((after("-W"), after("-H")), ("1344".into(), "768".into()));
        assert_eq!(after("-o"), "/out/a.png");
        assert!(a.iter().any(|x| x == "--offload-to-cpu"));
        for aspect in ["", "1:1", "9:16", "4:3", "3:2", "21:9", "weird"] {
            let (w, h) = size_for(aspect);
            assert!(w % 64 == 0 && h % 64 == 0 && w * h <= 1_100_000, "{aspect}");
        }
    }

    #[test]
    fn the_guard_says_banned_or_safe_and_anything_else_counts_as_banned() {
        let g = crate::local_setup::local_models()
            .iter()
            .find(|m| m.kind == ModelKind::ImageGuard)
            .unwrap();
        let (model, mmproj) = guard_files(Path::new("/m"), g).unwrap();
        assert!(model.ends_with(g.file));
        assert!(mmproj
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("mmproj-"));
        assert_eq!(guard_verdict("SAFE"), Some(false));
        assert_eq!(guard_verdict(" **Safe**."), Some(false));
        assert_eq!(guard_verdict("BANNED"), Some(true));
        assert_eq!(guard_verdict("Safe? No: BANNED"), Some(true));
        assert_eq!(guard_verdict("I can't tell"), None);
        assert_eq!(guard_model(Path::new("/nowhere")), None);
    }

    #[test]
    fn a_chroma_picture_uses_its_t5_encoder_and_guidance() {
        let models = Path::new("/m");
        let m = crate::local_setup::model_by_id("chroma1-hd-q4_k_m").expect("chroma row");
        assert!(m.optional && m.kind == ModelKind::Image);
        let first = crate::local_setup::local_models()
            .iter()
            .find(|x| x.kind == ModelKind::Image)
            .unwrap();
        assert_ne!(first.id, m.id, "Z-Image stays the first picture model");
        let a = generate_args(
            models,
            m,
            "a lighthouse",
            size_for(""),
            1,
            Path::new("/o.png"),
        )
        .unwrap();
        let after = |k: &str| a[a.iter().position(|x| x == k).unwrap() + 1].clone();
        assert!(after("--t5xxl").ends_with("t5-v1_1-xxl-encoder-Q5_K_M.gguf"));
        assert!(after("--vae").starts_with(
            &md::kind_dir(models, ModelKind::Image)
                .join("chroma1-hd")
                .display()
                .to_string()
        ));
        assert_eq!(after("--cfg-scale"), "4.0");
        assert_eq!(after("--sampling-method"), "euler");
        assert_eq!(after("--model-args"), "chroma_use_dit_mask=false");
        assert!(!a.iter().any(|x| x == "--llm"));
    }
}
