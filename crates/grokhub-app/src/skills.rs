use grokhub_core::{
    interpret_verify, parse_skill_md, render_skill_md, skill_dir_name, skill_safe,
    verify_script_path, SkillMd, VerifyResult,
};
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use crate::config;

pub fn skills_dir() -> PathBuf {
    config::config_dir().join("skills")
}

pub fn list_skills() -> Vec<SkillMd> {
    let mut out = vec![];
    let Ok(rd) = fs::read_dir(skills_dir()) else {
        return out;
    };
    for e in rd.flatten() {
        let p = e.path().join("SKILL.md");
        if let Ok(raw) = crate::desktop::read_text_capped(&p) {
            if skill_safe(&raw) {
                out.push(parse_skill_md(&raw));
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

pub fn save_skill(s: &SkillMd) -> Result<PathBuf, String> {
    if !skill_safe(&s.instructions) || !skill_safe(&s.pitfalls) {
        return Err("Secrets never in markdown".into());
    }
    let dir = skills_dir().join(skill_dir_name(&s.name));
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("SKILL.md");
    crate::config::atomic_write(&path, render_skill_md(s).as_bytes())?;
    if let Some(script) = verify_as_script(&s.verify) {
        let scripts = dir.join("scripts");
        fs::create_dir_all(&scripts).map_err(|e| e.to_string())?;
        let sh = scripts.join("verify.sh");
        fs::write(&sh, script).map_err(|e| e.to_string())?;
        #[cfg(unix)]
        {
            let _ = fs::set_permissions(&sh, fs::Permissions::from_mode(0o755));
        }
        if let Some(cmd) = verify_as_cmd(&s.verify) {
            fs::write(scripts.join("verify.cmd"), cmd).map_err(|e| e.to_string())?;
        }
    }
    Ok(path)
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
    if file.is_empty() || file.bytes().any(|b| matches!(b, b'&' | b'|' | b'>' | b'<' | b'%')) {
        return None;
    }
    Some(format!(
        "@echo off\r\nif exist \"{file}\" (exit /b 0) else (exit /b 1)\r\n"
    ))
}

pub fn skill_folder(name: &str) -> PathBuf {
    skills_dir().join(skill_dir_name(name))
}

pub fn skill_updated_at(name: &str) -> u64 {
    let path = skill_folder(name).join("SKILL.md");
    fs::metadata(&path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub fn run_verify(name: &str, cwd: Option<&str>) -> Option<VerifyResult> {
    let folder = skill_folder(name);
    let sh = verify_script_path(&folder);
    if !sh.exists() {
        return None;
    }
    #[cfg(windows)]
    let mut cmd = {
        let bat = folder.join("scripts").join("verify.cmd");
        if !bat.is_file() {
            return Some(interpret_verify(
                Some(127),
                "skill verify on Windows uses cmd, not bash/WSL",
            ));
        }
        let mut c = Command::new("cmd.exe");
        c.arg("/C").arg(&bat);
        crate::host::hide_windows_console(&mut c);
        c
    };
    #[cfg(not(windows))]
    let mut cmd = {
        let mut c = Command::new("bash");
        c.arg(&sh);
        c
    };
    if let Some(dir) = cwd.filter(|d| !d.is_empty()) {
        cmd.current_dir(dir);
    }
    let out = match crate::desktop::run_limited(cmd, Duration::from_secs(12)) {
        Some(o) => o,
        None => return Some(interpret_verify(Some(124), "verify timed out")),
    };
    Some(interpret_verify(
        out.status.code(),
        &String::from_utf8_lossy(&out.stdout),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_and_list() {
        let _g = crate::config::hold_test_config();
        let root = crate::config::test_config_root("sk");
        let _ = fs::remove_dir_all(&root);
        std::env::set_var("GROKHUB_CONFIG", &root);
        let s = SkillMd {
            name: "flash-pi".into(),
            description: "write an image".into(),
            slash: "/flash".into(),
            trigger: "flash the pi".into(),
            instructions: "1. dd the image".into(),
            pitfalls: "do not wipe the boot disk".into(),
            verify: "lsblk".into(),
            runs: 0,
        };
        save_skill(&s).expect("save");
        let listed = list_skills();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "flash-pi");
        assert!(
            skill_updated_at("flash-pi") > 0,
            "sync LWW needs a real skill file time, not now_ms"
        );
        assert!(skill_folder("flash-pi").join("scripts/verify.sh").exists());
        assert_eq!(listed[0].description, "write an image");
        let patched = grokhub_core::patch_skill(
            &listed[0],
            &SkillMd {
                name: "other".into(),
                description: "write a new image".into(),
                slash: "/other".into(),
                trigger: "flash the pi again".into(),
                instructions: "1. dd the newer image".into(),
                pitfalls: "still the boot disk".into(),
                verify: "lsblk -f".into(),
                runs: 3,
            },
        );
        assert_eq!(patched.name, "flash-pi");
        assert!(patched.instructions.contains("newer"));
        save_skill(&patched).expect("patch save");
        let listed = list_skills();
        assert_eq!(listed.len(), 1);
        assert!(listed[0].instructions.contains("newer"));
        let _ = fs::remove_dir_all(&root);
        std::env::remove_var("GROKHUB_CONFIG");
    }

    #[test]
    fn verify_runs_in_the_bound_tree() {
        let _g = crate::config::hold_test_config();
        let root = crate::config::test_config_root("sk-cwd");
        let _ = fs::remove_dir_all(&root);
        std::env::set_var("GROKHUB_CONFIG", &root);
        let s = SkillMd {
            name: "cwd-check".into(),
            description: "check the bound tree".into(),
            slash: "/cwd".into(),
            trigger: "verify cwd".into(),
            instructions: "1. look in the bound project".into(),
            pitfalls: "do not check the cabin cwd".into(),
            verify: "test -f grokhub-verify-marker".into(),
            runs: 0,
        };
        save_skill(&s).expect("save");
        let project = root.join("bound");
        fs::create_dir_all(&project).expect("project");
        fs::write(project.join("grokhub-verify-marker"), "ok").expect("marker");
        let miss = run_verify("cwd-check", None).expect("ran");
        assert!(
            !miss.ok,
            "verify without a bound cwd must not see the project marker"
        );
        let hit = run_verify("cwd-check", project.to_str()).expect("ran bound");
        assert!(hit.ok, "verify must run in the bound project: {hit:?}");
        let _ = fs::remove_dir_all(&root);
        std::env::remove_var("GROKHUB_CONFIG");
    }

    #[test]
    fn verify_as_cmd_translates_test_f() {
        assert_eq!(
            verify_as_cmd("test -f grokhub-verify-marker"),
            Some("@echo off\r\nif exist \"grokhub-verify-marker\" (exit /b 0) else (exit /b 1)\r\n".into())
        );
        assert_eq!(
            verify_as_cmd("test -e notes.md"),
            Some("@echo off\r\nif exist \"notes.md\" (exit /b 0) else (exit /b 1)\r\n".into())
        );
        assert!(verify_as_cmd("lsblk").is_none());
        assert!(verify_as_cmd("test -f file & calc").is_none());
    }

    #[test]
    fn run_verify_must_time_out() {
        let src = include_str!("skills.rs");
        let verify = src
            .split("pub fn run_verify(")
            .nth(1)
            .and_then(|s| s.split("\n#[cfg(test)]").next())
            .expect("run_verify");
        assert!(
            verify.contains("run_limited("),
            "HostDone skill verify must not freeze the UI on a hung script: {verify}"
        );
        assert!(
            !verify.contains(".output()"),
            "run_verify must not block the UI on Command::output: {verify}"
        );
        assert!(
            verify.contains("cmd.exe") && verify.contains("cfg(not(windows))"),
            "Windows verify must use cmd, not WSL bash: {verify}"
        );
    }

    #[test]
    fn list_skills_does_not_slurp_huge_skill_md() {
        let src = include_str!("skills.rs");
        let list = src
            .split("pub fn list_skills(")
            .nth(1)
            .and_then(|s| s.split("pub fn save_skill(").next())
            .expect("list_skills");
        assert!(
            list.contains("read_text_capped") && !list.contains("read_to_string"),
            "listing skills must not slurp a huge SKILL.md on the UI thread: {list}"
        );
    }
}
