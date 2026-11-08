//! The on-device picture engine: stable-diffusion.cpp's `sd-cli`, pinned per
//! OS, downloaded next to llama.cpp in `{config}/bin/sd-cpp`. It runs once
//! per picture and exits, so the image model loads on demand and nothing
//! stays in memory beside a game.
//!
//! Both builds are the Vulkan ones (any recent GPU, no CUDA install). The
//! archives are flat: the program and its libraries side by side, found
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_download as rd;

    #[test]
    fn linux_and_windows_x64_pin_the_vulkan_release_and_other_platforms_have_none() {
        for b in SD_CPP.builds {
            let a = asset_for(b.key).unwrap();
            assert_eq!(a.archive_format(), Some(rd::ArchiveFormat::Zip));
            assert!(crate::ssrf::runtime_download_url_ok(b.url).is_ok());
            assert!(b.url.contains(VERSION) && b.url.contains("vulkan"));
            assert!(b.size > 1_000_000);
            assert!(b.sha256.len() == 64 && b.sha256.bytes().all(|c| c.is_ascii_hexdigit()));
        }
        assert!(asset_for("macos-aarch64").is_none());
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
}
