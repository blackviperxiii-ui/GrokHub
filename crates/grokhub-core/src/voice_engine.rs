//! The on-device speech engine: sherpa-onnx's `sherpa-onnx-offline`, pinned
//! per OS, downloaded next to llama.cpp in `{config}/bin/sherpa-onnx`, and the
//! command that turns one recorded clip into text with a catalog speech
//! model (Parakeet). It runs once per clip and exits, so nothing stays loaded
//! between clips and a game keeps its memory.
//!
//! The archive keeps its own `bin/` and `lib/` layout: the Linux binary finds
//! its libraries through `$ORIGIN/../lib`, the Windows one beside it in `bin/`.

use std::fs;
use std::path::{Path, PathBuf};

use crate::local_setup::{LocalModel, ModelKind};
use crate::model_download::{self as md, Control, Progress};
use crate::runtime_download::{self as rd, Fetch, Outcome, PlatformAsset};

/// The sherpa-onnx release every platform pins.
pub const VERSION: &str = "v2.0.1";

/// The CPU build for each platform it ships for (the shared-library builds,
/// with ONNX Runtime beside the binary). Sizes and SHA-256 are the release's.
const ASSETS: &[(&str, &str, u64, &str)] = &[
    (
        "linux-x86_64",
        "sherpa-onnx-v2.0.1-linux-x64-shared.tar.bz2",
        28_003_506,
        "2e823376987c3b6960d50726b6560155e2ad92da0b8b3886325773225ec6bdec",
    ),
    (
        "windows-x86_64",
        "sherpa-onnx-v2.0.1-win-x64-shared-MT-Release.tar.bz2",
        26_664_192,
        "dc703575a68594435f53dec93df775ff4913af851a196c0537163f21d8d2364e",
    ),
];

/// Threads for one clip: enough for a short clip to come back in a second or
/// two, few enough to leave a running game its cores.
const THREADS: u32 = 2;

/// The engine archive for a platform key (`linux-x86_64`), or none where
/// sherpa-onnx ships no build (speech then falls back to whisper on PATH).
pub fn asset_for(key: &str) -> Option<PlatformAsset> {
    ASSETS
        .iter()
        .find(|(k, ..)| *k == key)
        .map(|(_, name, size, sha)| PlatformAsset {
            url: format!(
                "https://github.com/k2-fsa/sherpa-onnx/releases/download/{VERSION}/{name}"
            ),
            size: *size,
            sha256: (*sha).into(),
            format: "tar.bz2".into(),
        })
}

pub fn engine_dir(config_dir: &Path) -> PathBuf {
    rd::bin_dir(config_dir).join("sherpa-onnx")
}

fn binary_name() -> &'static str {
    if cfg!(windows) {
        "sherpa-onnx-offline.exe"
    } else {
        "sherpa-onnx-offline"
    }
}

pub fn offline_path(config_dir: &Path) -> PathBuf {
    engine_dir(config_dir).join("bin").join(binary_name())
}

const MARK: &str = "version.txt";

/// The pinned engine is unpacked and marked with this version.
pub fn installed(config_dir: &Path) -> bool {
    offline_path(config_dir).is_file()
        && fs::read_to_string(engine_dir(config_dir).join(MARK)).is_ok_and(|v| v.trim() == VERSION)
}

/// Download, check and unpack the engine for `key`; a no-op once installed.
pub fn download(
    config_dir: &Path,
    key: &str,
    fetch: &dyn Fetch,
    ctl: &Control,
    on: &mut dyn FnMut(Progress),
) -> Result<PathBuf, Outcome> {
    if installed(config_dir) {
        return Ok(offline_path(config_dir));
    }
    let Some(asset) = asset_for(key) else {
        return Err(Outcome::Failed(
            "The on-device speech engine has no build for this computer.".into(),
        ));
    };
    install(config_dir, &asset, fetch, ctl, on)
}

fn install(
    config_dir: &Path,
    asset: &PlatformAsset,
    fetch: &dyn Fetch,
    ctl: &Control,
    on: &mut dyn FnMut(Progress),
) -> Result<PathBuf, Outcome> {
    let fail = |e: String| Outcome::Failed(format!("Couldn't set up the speech engine: {e}"));
    let bin = rd::bin_dir(config_dir);
    fs::create_dir_all(&bin).map_err(|e| fail(e.to_string()))?;
    let part = rd::fetch_asset(&bin, asset, fetch, ctl, on)?;
    let dest = engine_dir(config_dir);
    let staging = dest.with_extension("new");
    let _ = fs::remove_dir_all(&staging);
    let fmt = asset
        .archive_format()
        .ok_or_else(|| fail("archive".into()))?;
    rd::extract_archive(&part, fmt, &staging).map_err(fail)?;
    // The archive holds one folder (`sherpa-onnx-v2.0.1-linux-x64-shared`).
    let top = only_dir(&staging).ok_or_else(|| fail("unexpected archive layout".into()))?;
    if !top.join("bin").join(binary_name()).is_file() {
        let _ = fs::remove_dir_all(&staging);
        return Err(fail(format!(
            "{} is missing from the archive",
            binary_name()
        )));
    }
    let _ = fs::remove_dir_all(&dest);
    fs::rename(&top, &dest).map_err(|e| fail(e.to_string()))?;
    let _ = fs::remove_dir_all(&staging);
    let _ = fs::remove_file(&part);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        for e in fs::read_dir(dest.join("bin"))
            .into_iter()
            .flatten()
            .flatten()
        {
            let _ = fs::set_permissions(e.path(), fs::Permissions::from_mode(0o755));
        }
    }
    fs::write(dest.join(MARK), VERSION).map_err(|e| fail(e.to_string()))?;
    Ok(offline_path(config_dir))
}

fn only_dir(dir: &Path) -> Option<PathBuf> {
    let mut it = fs::read_dir(dir).ok()?.flatten();
    let first = it.next()?.path();
    (it.next().is_none() && first.is_dir()).then_some(first)
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
    use std::io::{Cursor, Write as _};

    #[test]
    fn linux_and_windows_x64_pin_the_release_build_and_other_platforms_have_none() {
        let lin = asset_for("linux-x86_64").unwrap();
        assert_eq!(
            lin.url,
            "https://github.com/k2-fsa/sherpa-onnx/releases/download/v2.0.1/sherpa-onnx-v2.0.1-linux-x64-shared.tar.bz2"
        );
        assert_eq!(lin.archive_format(), Some(rd::ArchiveFormat::TarBz2));
        assert!(crate::ssrf::runtime_download_url_ok(&lin.url).is_ok());
        let win = asset_for("windows-x86_64").unwrap();
        assert!(win.url.ends_with("win-x64-shared-MT-Release.tar.bz2"));
        for (_, _, size, sha) in ASSETS {
            assert!(*size > 1_000_000);
            assert!(sha.len() == 64 && sha.bytes().all(|b| b.is_ascii_hexdigit()));
        }
        assert!(asset_for("macos-aarch64").is_none());
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

    /// A served `.tar.bz2` with the release's layout.
    struct Archive(Vec<u8>);
    impl Fetch for Archive {
        fn open(&self, _url: &str, offset: u64) -> Result<md::Opened, String> {
            let b = self.0[offset as usize..].to_vec();
            Ok((Box::new(Cursor::new(b)), offset, Some(self.0.len() as u64)))
        }
    }

    fn archive() -> Vec<u8> {
        let mut tar = tar::Builder::new(Vec::new());
        for (path, body) in [
            (
                format!("sherpa-onnx-{VERSION}-x/bin/{}", binary_name()),
                &b"bin"[..],
            ),
            (
                format!("sherpa-onnx-{VERSION}-x/lib/libonnxruntime.so"),
                &b"lib"[..],
            ),
        ] {
            let mut h = tar::Header::new_gnu();
            h.set_size(body.len() as u64);
            h.set_mode(0o644);
            h.set_cksum();
            tar.append_data(&mut h, path, body).unwrap();
        }
        let raw = tar.into_inner().unwrap();
        let mut bz = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::fast());
        bz.write_all(&raw).unwrap();
        bz.finish().unwrap()
    }

    #[test]
    fn the_engine_unpacks_into_its_own_folder_keeping_bin_and_lib() {
        let cfg = std::env::temp_dir().join(format!("grokhub-voice-engine-{}", std::process::id()));
        let _ = fs::remove_dir_all(&cfg);
        let body = archive();
        // The test pins its own archive in place of the release's.
        let asset = PlatformAsset {
            url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/v2.0.1/t.tar.bz2".into(),
            size: body.len() as u64,
            sha256: md::sha256_hex(&body),
            format: "tar.bz2".into(),
        };
        let exe = install(
            &cfg,
            &asset,
            &Archive(body.clone()),
            &Control::default(),
            &mut |_| {},
        )
        .unwrap();
        assert_eq!(exe, offline_path(&cfg));
        assert!(installed(&cfg));
        assert!(engine_dir(&cfg).join("lib/libonnxruntime.so").is_file());
        assert!(!engine_dir(&cfg).with_extension("new").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&exe).unwrap().permissions().mode() & 0o111,
                0o111
            );
        }
        // A wrong pin downloads nothing usable.
        let bad = PlatformAsset {
            sha256: "0".repeat(64),
            ..asset
        };
        let other = cfg.join("other");
        assert_eq!(
            install(
                &other,
                &bad,
                &Archive(body),
                &Control::default(),
                &mut |_| {}
            ),
            Err(Outcome::BadChecksum)
        );
        assert!(!installed(&other));
        let _ = fs::remove_dir_all(&cfg);
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
