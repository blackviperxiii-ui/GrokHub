//! A pinned helper program beside llama.cpp in `{config}/bin/<name>`: the
//! speech engine (sherpa-onnx) and the image engine (stable-diffusion.cpp).
//! Same rules as the llama.cpp download: a pinned size and SHA-256 per OS,
//! a resumable `.part`, a safe extract, and a version mark written last, so
//! a half-finished install never counts.

use std::fs;
use std::path::{Path, PathBuf};

use crate::model_download::{Control, Progress};
use crate::runtime_download::{self as rd, Fetch, Outcome, PlatformAsset};

/// One build for one platform key (`linux-x86_64`).
pub struct Build {
    pub key: &'static str,
    pub url: &'static str,
    pub size: u64,
    pub sha256: &'static str,
    /// `tar.bz2` or `zip`.
    pub format: &'static str,
}

pub struct Tool {
    /// The folder under `{config}/bin`.
    pub name: &'static str,
    pub version: &'static str,
    /// The program's path inside the folder, without `.exe`.
    pub program: &'static str,
    /// The archive wraps everything in one top folder (sherpa-onnx) rather
    /// than holding the files flat (stable-diffusion.cpp).
    pub one_top_folder: bool,
    pub builds: &'static [Build],
}

const MARK: &str = "version.txt";

/// The program a specialty model runs in: the speech engine for a speech
/// model, the picture engine for a picture model. Chat models run in
/// llama.cpp, which has its own setup, so they have none here.
pub fn engine_for(kind: crate::local_setup::ModelKind) -> Option<&'static Tool> {
    use crate::local_setup::ModelKind as K;
    match kind {
        K::SpeechToText | K::TextToSpeech => Some(&crate::voice_engine::SHERPA),
        K::Image => Some(&crate::image_engine::SD_CPP),
        _ => None,
    }
}

impl Tool {
    pub fn asset_for(&self, key: &str) -> Option<PlatformAsset> {
        self.builds
            .iter()
            .find(|b| b.key == key)
            .map(|b| PlatformAsset {
                url: b.url.into(),
                size: b.size,
                sha256: b.sha256.into(),
                format: b.format.into(),
            })
    }

    pub fn dir(&self, config_dir: &Path) -> PathBuf {
        rd::bin_dir(config_dir).join(self.name)
    }

    /// The program inside the folder, with `.exe` on Windows.
    fn program_rel(&self) -> PathBuf {
        let p = PathBuf::from(self.program);
        if cfg!(windows) {
            p.with_extension("exe")
        } else {
            p
        }
    }

    pub fn program_path(&self, config_dir: &Path) -> PathBuf {
        self.dir(config_dir).join(self.program_rel())
    }

    /// The pinned build is unpacked and marked with this version.
    pub fn installed(&self, config_dir: &Path) -> bool {
        self.program_path(config_dir).is_file()
            && fs::read_to_string(self.dir(config_dir).join(MARK))
                .is_ok_and(|v| v.trim() == self.version)
    }

    /// Download, check and unpack the build for `key`; a no-op once installed.
    pub fn download(
        &self,
        config_dir: &Path,
        key: &str,
        fetch: &dyn Fetch,
        ctl: &Control,
        on: &mut dyn FnMut(Progress),
    ) -> Result<PathBuf, Outcome> {
        if self.installed(config_dir) {
            return Ok(self.program_path(config_dir));
        }
        let Some(asset) = self.asset_for(key) else {
            return Err(Outcome::Failed(format!(
                "{} has no build for this computer.",
                self.name
            )));
        };
        self.install(config_dir, &asset, fetch, ctl, on)
    }

    pub(crate) fn install(
        &self,
        config_dir: &Path,
        asset: &PlatformAsset,
        fetch: &dyn Fetch,
        ctl: &Control,
        on: &mut dyn FnMut(Progress),
    ) -> Result<PathBuf, Outcome> {
        let fail = |e: String| Outcome::Failed(format!("Couldn't set up {}: {e}", self.name));
        let bin = rd::bin_dir(config_dir);
        fs::create_dir_all(&bin).map_err(|e| fail(e.to_string()))?;
        let part = rd::fetch_asset(&bin, asset, fetch, ctl, on)?;
        let dest = self.dir(config_dir);
        let staging = dest.with_extension("new");
        let _ = fs::remove_dir_all(&staging);
        let fmt = asset
            .archive_format()
            .ok_or_else(|| fail("archive".into()))?;
        rd::extract_archive(&part, fmt, &staging).map_err(fail)?;
        let root = if self.one_top_folder {
            only_dir(&staging).ok_or_else(|| fail("unexpected archive layout".into()))?
        } else {
            staging.clone()
        };
        let program = self.program_rel();
        if !root.join(&program).is_file() {
            let _ = fs::remove_dir_all(&staging);
            return Err(fail(format!(
                "{} is missing from the archive",
                program.display()
            )));
        }
        let _ = fs::remove_dir_all(&dest);
        fs::rename(&root, &dest).map_err(|e| fail(e.to_string()))?;
        let _ = fs::remove_dir_all(&staging);
        let _ = fs::remove_file(&part);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Some(dir) = self.program_path(config_dir).parent() {
                for e in fs::read_dir(dir).into_iter().flatten().flatten() {
                    let p = e.path();
                    // Programs only: a library keeps its mode.
                    if p.extension().is_none() && p.is_file() {
                        let _ = fs::set_permissions(&p, fs::Permissions::from_mode(0o755));
                    }
                }
            }
        }
        fs::write(dest.join(MARK), self.version).map_err(|e| fail(e.to_string()))?;
        Ok(self.program_path(config_dir))
    }
}

fn only_dir(dir: &Path) -> Option<PathBuf> {
    let mut it = fs::read_dir(dir).ok()?.flatten();
    let first = it.next()?.path();
    (it.next().is_none() && first.is_dir()).then_some(first)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model_download as md;
    use std::io::{Cursor, Write as _};

    struct Served(Vec<u8>);
    impl Fetch for Served {
        fn open(&self, _url: &str, offset: u64) -> Result<md::Opened, String> {
            let b = self.0[offset as usize..].to_vec();
            Ok((Box::new(Cursor::new(b)), offset, Some(self.0.len() as u64)))
        }
    }

    fn exe(name: &str) -> String {
        if cfg!(windows) {
            format!("{name}.exe")
        } else {
            name.into()
        }
    }

    fn tar_bz2(files: &[(String, &[u8])]) -> Vec<u8> {
        let mut tar = tar::Builder::new(Vec::new());
        for (path, body) in files {
            let mut h = tar::Header::new_gnu();
            h.set_size(body.len() as u64);
            h.set_mode(0o644);
            h.set_cksum();
            tar.append_data(&mut h, path, *body).unwrap();
        }
        let raw = tar.into_inner().unwrap();
        let mut bz = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::fast());
        bz.write_all(&raw).unwrap();
        bz.finish().unwrap()
    }

    fn zip(files: &[(String, &[u8])]) -> Vec<u8> {
        let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (path, body) in files {
            z.start_file(path.as_str(), zip::write::SimpleFileOptions::default())
                .unwrap();
            z.write_all(body).unwrap();
        }
        z.finish().unwrap().into_inner()
    }

    fn asset(body: &[u8], format: &str) -> PlatformAsset {
        PlatformAsset {
            url: format!("https://github.com/acme/tool/releases/download/v1/t.{format}"),
            size: body.len() as u64,
            sha256: md::sha256_hex(body),
            format: format.into(),
        }
    }

    fn tmp(label: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("grokhub-tool-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn a_speech_or_picture_model_has_its_engine_and_a_chat_one_has_none() {
        use crate::local_setup::ModelKind as K;
        let name = |k| engine_for(k).map(|t| t.name);
        assert_eq!(name(K::SpeechToText), Some("sherpa-onnx"));
        assert_eq!(name(K::Image), Some("sd-cpp"));
        for k in [K::Chat, K::Drafter, K::ImageGuard] {
            assert_eq!(name(k), None, "{k:?}");
        }
    }

    #[test]
    fn a_tar_bz2_with_one_top_folder_unpacks_keeping_bin_and_lib() {
        let tool = Tool {
            name: "speech",
            version: "v1",
            program: "bin/speak",
            one_top_folder: true,
            builds: &[],
        };
        let cfg = tmp("bz2");
        let body = tar_bz2(&[
            (format!("speech-v1-x/bin/{}", exe("speak")), b"bin"),
            ("speech-v1-x/lib/libonnxruntime.so".into(), b"lib"),
        ]);
        let a = asset(&body, "tar.bz2");
        let got = tool
            .install(
                &cfg,
                &a,
                &Served(body.clone()),
                &Control::default(),
                &mut |_| {},
            )
            .unwrap();
        assert_eq!(got, tool.program_path(&cfg));
        assert!(tool.installed(&cfg));
        assert!(tool.dir(&cfg).join("lib/libonnxruntime.so").is_file());
        assert!(!tool.dir(&cfg).with_extension("new").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&got).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111);
        }
        // A wrong pin leaves nothing usable.
        let bad = PlatformAsset {
            sha256: "0".repeat(64),
            ..a
        };
        let other = cfg.join("other");
        assert_eq!(
            tool.install(
                &other,
                &bad,
                &Served(body),
                &Control::default(),
                &mut |_| {}
            ),
            Err(Outcome::BadChecksum)
        );
        assert!(!tool.installed(&other));
        let _ = fs::remove_dir_all(&cfg);
    }

    #[test]
    fn a_flat_zip_unpacks_in_place_and_one_without_the_program_is_refused() {
        let tool = Tool {
            name: "images",
            version: "v2",
            program: "draw",
            one_top_folder: false,
            builds: &[],
        };
        let cfg = tmp("zip");
        let body = zip(&[(exe("draw"), b"bin"), ("libdraw.so".into(), b"lib")]);
        tool.install(
            &cfg,
            &asset(&body, "zip"),
            &Served(body),
            &Control::default(),
            &mut |_| {},
        )
        .unwrap();
        assert!(tool.installed(&cfg));
        assert!(tool.dir(&cfg).join("libdraw.so").is_file());
        let other = cfg.join("other");
        let empty = zip(&[("readme.txt".into(), b"hi")]);
        let out = tool.install(
            &other,
            &asset(&empty, "zip"),
            &Served(empty),
            &Control::default(),
            &mut |_| {},
        );
        assert!(
            matches!(out, Err(Outcome::Failed(ref e)) if e.contains("missing")),
            "{out:?}"
        );
        assert!(!tool.installed(&other));
        let _ = fs::remove_dir_all(&cfg);
    }
}
