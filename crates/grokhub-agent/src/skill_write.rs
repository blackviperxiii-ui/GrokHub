//! The skill write path: creating and editing the skills under
//! `{config}/skills`, shared by the cabin's Skills tab and the shell's
//! Skills page so a skill one of them writes is byte for byte the one the
//! other would write. Moved out of the cabin, next to `self_manage`; the
//! cabin keeps thin wrappers over these. Each function takes the profile's
//! config dir, the way the off-list functions in `skills` take the skills
//! dir, so a caller (or a test) can pin a different profile.
//!
//! Every create and edit goes through the ChangeLedger, so `/skills undo`
//! can put back what it replaced. A skill's verify script is written from
//! its `## Verify` section on save; nothing here ever runs one — running a
//! verify script still takes the usual approval in chat.

use std::fs;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use grokhub_core::{render_skill_md, skill_dir_name, skill_safe, SkillMd};

use crate::harness::{self as hx, Origin};

/// The folder a profile's skills live in: `{config}/skills`.
pub fn skills_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("skills")
}

pub fn skill_folder(config_dir: &Path, name: &str) -> PathBuf {
    skills_dir(config_dir).join(skill_dir_name(name))
}

/// A skill made from a name and what it should do. The words become its
/// description, when to use it and its steps; Edit adds more.
pub fn new_skill(config_dir: &Path, name: &str, what: &str) -> Result<SkillMd, String> {
    let name = name.trim();
    let what = what.trim();
    if name.is_empty() {
        return Err("Give the skill a name.".into());
    }
    if what.is_empty() {
        return Err("Say what the skill should do.".into());
    }
    if skill_folder(config_dir, name).join("SKILL.md").exists() {
        return Err(format!("There's already a skill called {name}."));
    }
    let folder = skill_dir_name(name);
    let skill = SkillMd {
        name: name.to_string(),
        description: what.to_string(),
        slash: format!("/{folder}"),
        trigger: what.to_string(),
        instructions: what.to_string(),
        ..SkillMd::default()
    };
    save_skill_logged(config_dir, &skill, Origin::User, "made on the Skills tab")?;
    Ok(skill)
}

pub fn save_skill(config_dir: &Path, s: &SkillMd) -> Result<PathBuf, String> {
    if !skill_safe(&s.instructions) || !skill_safe(&s.pitfalls) {
        return Err("Secrets never in markdown".into());
    }
    let dir = skill_folder(config_dir, &s.name);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("SKILL.md");
    grokhub_core::work_file::write_private_atomic(&path, render_skill_md(s).as_bytes())?;
    write_verify_scripts(&dir, &s.verify)?;
    Ok(path)
}

/// `save_skill` with the ChangeLedger around it: the version it replaces is
/// kept and the write is logged, so `/skills undo` can put it back. For
/// writes the app makes on its own (`Origin::SelfManage`) and Suggested Add.
pub fn save_skill_logged(
    config_dir: &Path,
    s: &SkillMd,
    origin: Origin,
    reason: &str,
) -> Result<(), String> {
    save_skill_versioned(config_dir, s, origin, reason).map(|_| ())
}

/// [`save_skill_logged`], returning the ledger line (`None` when the bytes
/// did not change), so the weekly pass can put the change on trial.
pub fn save_skill_versioned(
    config_dir: &Path,
    s: &SkillMd,
    origin: Origin,
    reason: &str,
) -> Result<Option<hx::Change>, String> {
    hx::record_skill_change(
        config_dir,
        &skills_dir(config_dir),
        &s.name,
        origin,
        reason,
        || save_skill(config_dir, s).map(|_| ()),
    )
}

/// After an undo or restore put `SKILL.md` back byte for byte, its verify
/// scripts follow the `## Verify` it now holds.
pub fn rewrite_verify_scripts(config_dir: &Path, s: &SkillMd) -> Result<(), String> {
    write_verify_scripts(&skill_folder(config_dir, &s.name), &s.verify)
}

pub fn skill_updated_at(config_dir: &Path, name: &str) -> u64 {
    let path = skill_folder(config_dir, name).join("SKILL.md");
    fs::metadata(&path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn write_verify_scripts(dir: &Path, verify: &str) -> Result<(), String> {
    if let Some(script) = verify_as_script(verify) {
        let scripts = dir.join("scripts");
        fs::create_dir_all(&scripts).map_err(|e| e.to_string())?;
        let sh = scripts.join("verify.sh");
        fs::write(&sh, script).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            let _ = fs::set_permissions(&sh, fs::Permissions::from_mode(0o755));
        }
        if let Some(cmd) = verify_as_cmd(verify) {
            fs::write(scripts.join("verify.cmd"), cmd).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

fn verify_as_script(verify: &str) -> Option<String> {
    let t = verify.trim();
    if t.is_empty() {
        return None;
    }
    if t.contains("#!/") {
        return Some(t.to_string());
    }
    let first = t.lines().next().unwrap_or("").trim();
    if first.starts_with("test ")
        || first.starts_with('[')
        || first.starts_with("ls")
        || first.starts_with("exit")
        || first.contains("grokhub")
        || first.starts_with("echo ")
    {
        Some(format!("#!/bin/sh\nset -e\n{t}\n"))
    } else {
        None
    }
}

/// Windows verify must not spawn WSL/`bash`. `test -f FILE` becomes `if exist`.
fn verify_as_cmd(verify: &str) -> Option<String> {
    let first = verify.trim().lines().next()?.trim();
    let file = first
        .strip_prefix("test -f ")
        .or_else(|| first.strip_prefix("test -e "))?
        .trim()
        .trim_matches('"');
    if file.is_empty()
        || file
            .bytes()
            .any(|b| matches!(b, b'&' | b'|' | b'>' | b'<' | b'%'))
    {
        return None;
    }
    Some(format!(
        "@echo off\r\nif exist \"{file}\" (exit /b 0) else (exit /b 1)\r\n"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use grokhub_core::parse_skill_md;

    fn scratch(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("gh-agent-skill-write-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn a_created_skill_is_the_one_the_cabin_would_write() {
        let cfg = scratch("create");
        let made = new_skill(&cfg, "Clip Trimmer", "Trim my last game clip to 30 seconds").unwrap();
        assert_eq!(made.slash, "/clip-trimmer");
        assert_eq!(made.description, "Trim my last game clip to 30 seconds");
        assert_eq!(made.trigger, made.description);
        assert_eq!(made.instructions, made.description);

        // Same folder, same bytes the cabin's save writes for this SkillMd.
        let path = cfg.join("skills/clip-trimmer/SKILL.md");
        assert_eq!(
            skill_folder(&cfg, "Clip Trimmer"),
            cfg.join("skills/clip-trimmer")
        );
        let bytes = fs::read_to_string(&path).unwrap();
        assert_eq!(bytes, render_skill_md(&made));
        let parsed = parse_skill_md(&bytes);
        assert_eq!(parsed.name, "Clip Trimmer");
        assert_eq!(parsed.slash, "/clip-trimmer");
        assert!(skill_updated_at(&cfg, "Clip Trimmer") > 0);

        // The create went through the ledger as the user's own change.
        let ledger = hx::ChangeLedger::load(&cfg);
        let lines = ledger.all();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].op, hx::ChangeOp::Create);
        assert_eq!(lines[0].origin, Origin::User);
        assert_eq!(lines[0].reason, "made on the Skills tab");

        assert!(
            new_skill(&cfg, "clip trimmer", "again").is_err(),
            "no second skill of the same name"
        );
        assert!(new_skill(&cfg, "", "x").is_err());
        assert!(new_skill(&cfg, "x", " ").is_err());
        let _ = fs::remove_dir_all(&cfg);
    }

    #[test]
    fn save_writes_verify_scripts_and_an_edit_keeps_the_rest() {
        let cfg = scratch("edit");
        let s = SkillMd {
            name: "flash-pi".into(),
            description: "write an image".into(),
            slash: "/flash".into(),
            trigger: "flash the pi".into(),
            instructions: "1. dd the image".into(),
            pitfalls: "do not wipe the boot disk".into(),
            verify: "test -f grokhub-verify-marker".into(),
            runs: 3,
        };
        let path = save_skill(&cfg, &s).expect("save");
        assert_eq!(path, cfg.join("skills/flash-pi/SKILL.md"));
        let folder = skill_folder(&cfg, "flash-pi");
        let sh = folder.join("scripts/verify.sh");
        assert!(sh.exists(), "the verify script is written, never run");
        assert!(fs::read_to_string(&sh).unwrap().starts_with("#!/bin/sh"));
        assert_eq!(
            fs::read_to_string(folder.join("scripts/verify.cmd")).unwrap(),
            "@echo off\r\nif exist \"grokhub-verify-marker\" (exit /b 0) else (exit /b 1)\r\n"
        );

        // The cabin tab's edit: parse what's on disk, set the three fields,
        // save it logged. Everything else survives.
        let raw = fs::read_to_string(&path).unwrap();
        let mut edited = parse_skill_md(&raw);
        edited.description = "write a new image".into();
        edited.trigger = "flash the pi again".into();
        edited.instructions = "1. dd the newer image".into();
        save_skill_logged(&cfg, &edited, Origin::User, "edited on the Skills tab").unwrap();
        let after = parse_skill_md(&fs::read_to_string(&path).unwrap());
        assert_eq!(after.description, "write a new image");
        assert_eq!(after.trigger, "flash the pi again");
        assert!(after.instructions.contains("newer"));
        assert_eq!(after.pitfalls.trim(), "do not wipe the boot disk");
        assert_eq!(after.verify, "test -f grokhub-verify-marker");
        assert_eq!(after.runs, 3);
        let ledger = hx::ChangeLedger::load(&cfg);
        assert!(ledger
            .all()
            .iter()
            .any(|c| c.op == hx::ChangeOp::Modify && c.reason == "edited on the Skills tab"));
        let _ = fs::remove_dir_all(&cfg);
    }

    #[test]
    fn secrets_never_reach_the_disk() {
        let cfg = scratch("secrets");
        let s = SkillMd {
            name: "leaky".into(),
            description: "d".into(),
            slash: "/leaky".into(),
            trigger: "t".into(),
            instructions: "use key sk-abcdefghijklmnopqrstuv here".into(),
            ..SkillMd::default()
        };
        assert_eq!(
            save_skill(&cfg, &s).unwrap_err(),
            "Secrets never in markdown"
        );
        assert!(!skill_folder(&cfg, "leaky").join("SKILL.md").exists());
        let _ = fs::remove_dir_all(&cfg);
    }

    #[test]
    fn verify_as_cmd_translates_test_f() {
        assert_eq!(
            verify_as_cmd("test -f grokhub-verify-marker"),
            Some(
                "@echo off\r\nif exist \"grokhub-verify-marker\" (exit /b 0) else (exit /b 1)\r\n"
                    .into()
            )
        );
        assert_eq!(
            verify_as_cmd("test -e notes.md"),
            Some("@echo off\r\nif exist \"notes.md\" (exit /b 0) else (exit /b 1)\r\n".into())
        );
        assert!(verify_as_cmd("lsblk").is_none());
        assert!(verify_as_cmd("test -f file & calc").is_none());
    }
}
