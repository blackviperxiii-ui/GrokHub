//! The on-device speech engine: sherpa-onnx's `sherpa-onnx-offline`, pinned
//! per OS, downloaded next to llama.cpp in `{config}/bin/sherpa-onnx`, and the
//! command that turns one recorded clip into text with a catalog speech
//! model (Parakeet). It runs once per clip and exits, so nothing stays loaded
//! between clips and a game keeps its memory.
//!
//! The archive keeps its own `bin/` and `lib/` layout: the Linux binary finds
//! its libraries through `$ORIGIN/../lib`, the Windows one beside it in `bin/`.

use std::path::{Path, PathBuf};

use crate::local_setup::{LocalModel, ModelKind};
use crate::model_download::{self as md, Control, Progress};
use crate::runtime_download::{Fetch, Outcome, PlatformAsset};
use crate::tool_engine::{Build, Tool};

/// The sherpa-onnx release every platform pins.
pub const VERSION: &str = "v2.0.1";

/// The CPU build for each platform it ships for (the shared-library builds,
/// with ONNX Runtime beside the binary). Sizes and SHA-256 are the release's.
pub const SHERPA: Tool = Tool {
    name: "sherpa-onnx",
    version: VERSION,
    program: "bin/sherpa-onnx-offline",
    one_top_folder: true,
    builds: &[
        Build {
            key: "linux-x86_64",
            url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/v2.0.1/sherpa-onnx-v2.0.1-linux-x64-shared.tar.bz2",
            size: 28_003_506,
            sha256: "2e823376987c3b6960d50726b6560155e2ad92da0b8b3886325773225ec6bdec",
            format: "tar.bz2",
        },
        Build {
            key: "windows-x86_64",
            url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/v2.0.1/sherpa-onnx-v2.0.1-win-x64-shared-MT-Release.tar.bz2",
            size: 26_664_192,
            sha256: "dc703575a68594435f53dec93df775ff4913af851a196c0537163f21d8d2364e",
            format: "tar.bz2",
        },
    ],
};

/// Threads for one clip: enough for a short clip to come back in a second or
/// two, few enough to leave a running game its cores.
const THREADS: u32 = 2;

/// The engine archive for a platform key, or none where sherpa-onnx ships no
/// build (speech then falls back to whisper on PATH).
pub fn asset_for(key: &str) -> Option<PlatformAsset> {
    SHERPA.asset_for(key)
}

pub fn offline_path(config_dir: &Path) -> PathBuf {
    SHERPA.program_path(config_dir)
}

pub fn installed(config_dir: &Path) -> bool {
    SHERPA.installed(config_dir)
}

/// Download, check and unpack the engine for `key`; a no-op once installed.
pub fn download(
    config_dir: &Path,
    key: &str,
    fetch: &dyn Fetch,
    ctl: &Control,
    on: &mut dyn FnMut(Progress),
) -> Result<PathBuf, Outcome> {
    SHERPA.download(config_dir, key, fetch, ctl, on)
}

/// The first catalog speech model whose files are all in `models_dir`'s
/// voice folder.
pub fn speech_model(models_dir: &Path) -> Option<&'static LocalModel> {
    let dir = md::kind_dir(models_dir, ModelKind::SpeechToText);
    crate::local_setup::local_models()
        .iter()
        .find(|m| m.kind == ModelKind::SpeechToText && md::complete(&dir, m))
}

/// The arguments for one clip with a transducer model (Parakeet): the encoder
/// is the model's main file; the decoder, joiner and token list are parts
/// told apart by name. None when a file is missing from the row.
pub fn transcribe_args(models_dir: &Path, m: &LocalModel, wav: &Path) -> Option<Vec<String>> {
    let dir = md::kind_dir(models_dir, m.kind);
    let part = |end: &str| {
        m.parts
            .iter()
            .find(|p| p.file.ends_with(end))
            .map(|p| dir.join(p.file))
    };
    let arg = |k: &str, p: PathBuf| format!("--{k}={}", p.display());
    Some(vec![
        arg("encoder", md::final_path(&dir, m)),
        arg("decoder", part("decoder.int8.onnx")?),
        arg("joiner", part("joiner.int8.onnx")?),
        arg("tokens", part("tokens.txt")?),
        "--model-type=nemo_transducer".into(),
        format!("--num-threads={THREADS}"),
        wav.display().to_string(),
    ])
}

/// The transcript in the engine's output: its result line is JSON with a
/// `text` field (it may go to stdout or stderr, so both are passed in).
pub fn transcript_from(output: &str) -> Option<String> {
    output
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with('{'))
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find_map(|v| v.get("text")?.as_str().map(|t| t.trim().to_string()))
        .filter(|t| !t.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_download as rd;

    #[test]
    fn linux_and_windows_x64_pin_the_release_build_and_other_platforms_have_none() {
        let lin = asset_for("linux-x86_64").unwrap();
        assert_eq!(
            lin.url,
            "https://github.com/k2-fsa/sherpa-onnx/releases/download/v2.0.1/sherpa-onnx-v2.0.1-linux-x64-shared.tar.bz2"
        );
        assert_eq!(lin.archive_format(), Some(rd::ArchiveFormat::TarBz2));
        for b in SHERPA.builds {
            assert!(crate::ssrf::runtime_download_url_ok(b.url).is_ok());
            assert!(b.url.contains(VERSION));
            assert!(b.size > 1_000_000);
            assert!(b.sha256.len() == 64 && b.sha256.bytes().all(|c| c.is_ascii_hexdigit()));
        }
        assert!(asset_for("windows-x86_64")
            .unwrap()
            .url
            .ends_with("win-x64-shared-MT-Release.tar.bz2"));
        assert!(asset_for("macos-aarch64").is_none());
        assert!(offline_path(Path::new("/c"))
            .starts_with(rd::bin_dir(Path::new("/c")).join("sherpa-onnx/bin")));
    }

    #[test]
    fn the_transcript_is_the_text_field_of_the_result_line() {
        let out = "/tmp/a.wav\n{\"lang\": \"\", \"emotion\": \"\", \"event\": \"\", \"text\": \" Pull the boss to the pillar.\", \"timestamps\": [0.1]}\n----\n";
        assert_eq!(
            transcript_from(out).as_deref(),
            Some("Pull the boss to the pillar.")
        );
        assert_eq!(transcript_from("Done!\n{\"text\": \"  \"}"), None);
        assert_eq!(transcript_from("no result"), None);
    }

    #[test]
    fn a_parakeet_style_row_turns_into_the_transducer_command() {
        let models = Path::new("/m");
        let enc = crate::local_setup::local_models()
            .iter()
            .find(|m| m.kind == ModelKind::SpeechToText)
            .expect("a speech row");
        let args = transcribe_args(models, enc, Path::new("/t/clip.wav")).unwrap();
        let voice = md::kind_dir(models, ModelKind::SpeechToText);
        assert_eq!(
            args[0],
            format!("--encoder={}", voice.join(enc.file).display())
        );
        assert!(args[1].starts_with("--decoder=") && args[1].ends_with("decoder.int8.onnx"));
        assert!(args[2].starts_with("--joiner="));
        assert!(args[3].ends_with("tokens.txt"));
        assert_eq!(args[4], "--model-type=nemo_transducer");
        assert_eq!(args.last().unwrap(), "/t/clip.wav");
        assert_eq!(speech_model(Path::new("/nowhere")), None);
    }
}
