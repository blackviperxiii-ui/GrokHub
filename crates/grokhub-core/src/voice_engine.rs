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

/// The speaking program in the same archive, with `.exe` on Windows.
pub fn tts_path(config_dir: &Path) -> PathBuf {
    let p = SHERPA.dir(config_dir).join("bin/sherpa-onnx-offline-tts");
    if cfg!(windows) {
        p.with_extension("exe")
    } else {
        p
    }
}

/// The engine and its speaking program are both there.
pub fn tts_installed(config_dir: &Path) -> bool {
    installed(config_dir) && tts_path(config_dir).is_file()
}

/// The first catalog voice whose files are all in `models_dir`'s voice folder.
pub fn voice_model(models_dir: &Path) -> Option<&'static LocalModel> {
    let dir = md::kind_dir(models_dir, ModelKind::TextToSpeech);
    crate::local_setup::local_models()
        .iter()
        .find(|m| m.kind == ModelKind::TextToSpeech && md::complete(&dir, m))
}

/// Kokoro's speaker for each of the app's voices (the speaker list of the
/// multi-language v1.0 export): Ara is af_heart, Eve af_bella, Leo
/// am_michael, Rex am_fenrir, Sal am_puck.
pub fn kokoro_speaker(voice: &str) -> u32 {
    match voice {
        "eve" => 2,
        "leo" => 16,
        "rex" => 14,
        "sal" => 18,
        _ => 3,
    }
}

/// The arguments that speak `text` with a Kokoro row into `out` (a WAV
/// file). None when a file is missing from the row.
pub fn speak_args(
    models_dir: &Path,
    m: &LocalModel,
    voice: &str,
    text: &str,
    out: &Path,
) -> Option<Vec<String>> {
    let dir = md::kind_dir(models_dir, m.kind);
    let part = |end: &str| {
        m.parts
            .iter()
            .find(|p| p.file.ends_with(end))
            .map(|p| dir.join(p.file))
    };
    let arg = |k: &str, p: PathBuf| format!("--{k}={}", p.display());
    Some(vec![
        arg("kokoro-model", md::final_path(&dir, m)),
        arg("kokoro-voices", part("voices.bin")?),
        arg("kokoro-tokens", part("tokens.txt")?),
        arg("kokoro-lexicon", part("lexicon-us-en.txt")?),
        format!("--sid={}", kokoro_speaker(voice)),
        format!("--num-threads={THREADS}"),
        arg("output-filename", out.to_path_buf()),
        text_arg(text),
    ])
}

/// The text as the engine's last argument. Its option parser takes an
/// argument that starts with `--` as a setting, so a reply line like
/// "--sid=7 first item." would fail to speak or change a setting. A leading
/// space keeps it text; the engine ignores the space.
fn text_arg(text: &str) -> String {
    if text.starts_with('-') {
        format!(" {text}")
    } else {
        text.to_string()
    }
}

/// A new, empty WAV file in `dir` for one spoken piece. Pieces are spoken
/// side by side, so each gets its own name, and the file is created fresh
/// (never opened through a file or link already at that name).
pub fn claim_speak_file(dir: &Path) -> std::io::Result<PathBuf> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    claim_in(dir, &NEXT)
}

fn claim_in(dir: &Path, next: &std::sync::atomic::AtomicU64) -> std::io::Result<PathBuf> {
    let mut last = None;
    for _ in 0..64 {
        let n = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = dir.join(format!("grokhub-speak-{}-{n}.wav", std::process::id()));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(_) => return Ok(path),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => last = Some(e),
            Err(e) => return Err(e),
        }
    }
    Err(last.unwrap_or_else(|| std::io::Error::other("no free name")))
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

    #[test]
    fn each_spoken_piece_gets_its_own_new_file() {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-claim-speak-{}-{}",
            std::process::id(),
            crate::now_ms()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let claims: Vec<PathBuf> = std::thread::scope(|s| {
            let jobs: Vec<_> = (0..8)
                .map(|_| s.spawn(|| claim_speak_file(&dir).unwrap()))
                .collect();
            jobs.into_iter().map(|j| j.join().unwrap()).collect()
        });
        let mut names: Vec<_> = claims
            .iter()
            .map(|p| p.file_name().unwrap().to_owned())
            .collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), 8, "{claims:?}");
        for p in &claims {
            assert_eq!(p.parent(), Some(dir.as_path()));
            assert_eq!(std::fs::metadata(p).unwrap().len(), 0);
        }
        // A name already taken (a leftover, or a link planted in a shared
        // temp folder) is skipped, and what it points at is left alone.
        let pid = std::process::id();
        let dir = dir.join("planted");
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("keep.txt");
        std::fs::write(&target, "mine").unwrap();
        let taken = dir.join(format!("grokhub-speak-{pid}-0.wav"));
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, &taken).unwrap();
        #[cfg(not(unix))]
        std::fs::write(&taken, "mine").unwrap();
        let counter = std::sync::atomic::AtomicU64::new(0);
        assert_eq!(
            claim_in(&dir, &counter).unwrap(),
            dir.join(format!("grokhub-speak-{pid}-1.wav"))
        );
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "mine");
        assert_eq!(
            claim_speak_file(&dir.join("missing")).map_err(|e| e.kind()),
            Err(std::io::ErrorKind::NotFound)
        );
        let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    }

    #[test]
    fn the_kokoro_row_turns_into_the_speaking_command() {
        let models = Path::new("/m");
        let k = crate::local_setup::local_models()
            .iter()
            .find(|m| m.kind == ModelKind::TextToSpeech)
            .expect("a voice row");
        let args = speak_args(models, k, "leo", "Hello there.", Path::new("/t/out.wav")).unwrap();
        let voice = md::kind_dir(models, ModelKind::TextToSpeech);
        assert_eq!(
            args[0],
            format!("--kokoro-model={}", voice.join(k.file).display())
        );
        assert!(args[1].starts_with("--kokoro-voices=") && args[1].ends_with("voices.bin"));
        assert!(args[2].ends_with("tokens.txt"));
        assert!(args[3].starts_with("--kokoro-lexicon=") && args[3].ends_with("lexicon-us-en.txt"));
        assert_eq!(args[4], "--sid=16");
        assert_eq!(args[6], "--output-filename=/t/out.wav");
        assert_eq!(args.last().unwrap(), "Hello there.");
        // A reply line that looks like a setting stays the text to speak.
        let dashed = speak_args(
            models,
            k,
            "leo",
            "--sid=7 first item.",
            Path::new("/t/o.wav"),
        )
        .unwrap();
        assert_eq!(dashed.last().unwrap(), " --sid=7 first item.");
        assert_eq!(dashed[4], "--sid=16");
        assert_eq!(dashed.iter().filter(|a| a.starts_with("--sid=")).count(), 1);
        // No espeak-ng data: sherpa-onnx v2 ignores it.
        assert!(!args.iter().any(|a| a.contains("data-dir")));
        assert_eq!(voice_model(Path::new("/nowhere")), None);
        assert_eq!(kokoro_speaker("ara"), 3);
        assert_eq!(kokoro_speaker("unknown"), 3);
        assert_eq!(
            crate::tool_engine::engine_for(ModelKind::TextToSpeech).map(|t| t.name),
            Some("sherpa-onnx")
        );
        assert!(tts_path(Path::new("/c")).ends_with(if cfg!(windows) {
            "bin/sherpa-onnx-offline-tts.exe"
        } else {
            "bin/sherpa-onnx-offline-tts"
        }));
    }
}
