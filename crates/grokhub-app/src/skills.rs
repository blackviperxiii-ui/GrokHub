use grokhub_core::{
    interpret_verify, parse_skill_md, render_skill_md, skill_dir_name, skill_safe,
    verify_script_path, SkillMd, VerifyResult,
};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

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
        if let Ok(raw) = fs::read_to_string(p) {
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

pub fn skill_folder(name: &str) -> PathBuf {
    skills_dir().join(skill_dir_name(name))
}

pub fn run_verify(name: &str) -> Option<VerifyResult> {
    let path = verify_script_path(skill_folder(name));
    if !path.exists() {
        return None;
    }
    let out = Command::new("bash").arg(&path).output().ok()?;
    Some(interpret_verify(
        out.status.code(),
        &String::from_utf8_lossy(&out.stdout),
    ))
}

pub fn pin_text(skills: &[SkillMd]) -> String {
    let mut s = String::new();
    for sk in skills.iter().take(12) {
        if !s.is_empty() {
            s.push('\n');
        }
        s.push_str(&format!("- {} — {}", sk.name, sk.trigger));
    }
    s.chars().take(1000).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_and_list() {
        let _g = crate::config::TEST_CONFIG_LOCK.lock().unwrap();
        let root = std::env::temp_dir().join(format!("grokhub-sk-{}", std::process::id()));
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
        assert!(skill_folder("flash-pi").join("scripts/verify.sh").exists());
        let pins = pin_text(&listed);
        assert!(pins.contains("flash-pi"));
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
}
