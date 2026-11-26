//! `loadout://skill/install?identifier=` deep-link parsing and preview.
//!
//! Identifiers are source-qualified, matching the Hermes Marketplace style:
//! `clawhub/<slug>`, `skills-sh/<owner>/<repo>/<skill>`, `github/<owner>/<repo>[/path]`,
//! `library/<id>`, or `bundle/<slug>`.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::skill::skill_dir_name;
use crate::skill_bundles::{bundle_by_slug, SkillBundle};
use crate::skill_pack::library_skill_by_id;
use crate::skills_hub::{HubHit, HubSource};

/// A parsed install target from a deep link or an explicit identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillInstallTarget {
    Hub(HubHit),
    Library {
        id: String,
        name: String,
        description: String,
    },
    Bundle(SkillBundle),
}

/// What the confirm dialog shows before an install runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillInstallPreview {
    pub identifier: String,
    /// Human source label (ClawHub, Skills Library, Skill bundle, …).
    pub source: String,
    pub name: String,
    pub description: String,
    /// Absolute destination folder (skill dir or skill-bundles dir).
    pub destination: String,
    /// `"skill"` or `"bundle"`.
    pub kind: String,
    /// For bundles: the skill names that will load together.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<String>,
}

/// True when `url` is a Loadout skill-install deep link.
pub fn is_skill_install_url(url: &str) -> bool {
    let u = url.trim();
    let lower = u.to_ascii_lowercase();
    lower.starts_with("loadout://skill/install")
        || lower.starts_with("loadout:skill/install")
}

/// Pull `identifier` from `loadout://skill/install?identifier=…`.
pub fn parse_skill_install_url(url: &str) -> Result<String, String> {
    let raw = url.trim();
    if !is_skill_install_url(raw) {
        return Err("Not a loadout://skill/install link".into());
    }
    let rest = raw
        .split_once("://")
        .or_else(|| raw.split_once(':'))
        .map(|(_, r)| r)
        .unwrap_or(raw);
    let path_and_query = rest.trim_start_matches('/');
    let (path, query) = path_and_query
        .split_once('?')
        .map(|(p, q)| (p, Some(q)))
        .unwrap_or((path_and_query, None));
    if !path.eq_ignore_ascii_case("skill/install")
        && !path.to_ascii_lowercase().starts_with("skill/install/")
    {
        return Err("loadout skill install link path must be skill/install".into());
    }
    if let Some(q) = query {
        for pair in q.split('&') {
            let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
            if k.eq_ignore_ascii_case("identifier") {
                let id = decode_query(v).trim().to_string();
                if id.is_empty() {
                    return Err("identifier is empty".into());
                }
                return Ok(id);
            }
        }
    }
    if let Some(tail) = path
        .strip_prefix("skill/install/")
        .or_else(|| path.strip_prefix("Skill/install/"))
    {
        let id = decode_query(tail).trim().to_string();
        if !id.is_empty() {
            return Ok(id);
        }
    }
    Err("loadout://skill/install needs ?identifier=".into())
}

fn decode_query(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let h = |c: u8| -> Option<u8> {
                    match c {
                        b'0'..=b'9' => Some(c - b'0'),
                        b'a'..=b'f' => Some(c - b'a' + 10),
                        b'A'..=b'F' => Some(c - b'A' + 10),
                        _ => None,
                    }
                };
                if let (Some(a), Some(b)) = (h(bytes[i + 1]), h(bytes[i + 2])) {
                    out.push((a << 4 | b) as char);
                    i += 3;
                } else {
                    out.push('%');
                    i += 1;
                }
            }
            c => {
                out.push(c as char);
                i += 1;
            }
        }
    }
    out
}

/// Resolve a source-qualified identifier into an install target.
pub fn resolve_skill_install_identifier(
    config_dir: &Path,
    identifier: &str,
) -> Result<SkillInstallTarget, String> {
    let id = identifier.trim();
    if id.is_empty() {
        return Err("identifier is empty".into());
    }
    let (source, rest) = split_qualified(id)?;
    match source.as_str() {
        "clawhub" => {
            let slug = rest.trim().trim_matches('/');
            if slug.is_empty() || slug.contains('/') {
                return Err("clawhub identifier must be clawhub/<slug>".into());
            }
            Ok(SkillInstallTarget::Hub(HubHit {
                source: HubSource::ClawHub,
                id: slug.to_string(),
                name: slug.to_string(),
                description: String::new(),
            }))
        }
        "skills-sh" | "skills.sh" | "skillssh" => {
            let path = rest.trim().trim_matches('/');
            if path.split('/').count() < 3 {
                return Err("skills-sh identifier must be skills-sh/<owner>/<repo>/<skill>".into());
            }
            let leaf = path.rsplit('/').next().unwrap_or(path);
            Ok(SkillInstallTarget::Hub(HubHit {
                source: HubSource::SkillsSh,
                id: path.to_string(),
                name: leaf.to_string(),
                description: String::new(),
            }))
        }
        "github" => {
            let path = rest.trim().trim_matches('/');
            if path.split('/').count() < 2 {
                return Err("github identifier must be github/<owner>/<repo>[/path]".into());
            }
            let leaf = path.rsplit('/').next().unwrap_or(path);
            Ok(SkillInstallTarget::Hub(HubHit {
                source: HubSource::GitHub,
                id: path.to_string(),
                name: leaf.to_string(),
                description: String::new(),
            }))
        }
        "library" => {
            let key = rest.trim().trim_matches('/');
            let item = library_skill_by_id(key)
                .ok_or_else(|| format!("Unknown library skill `{key}`"))?;
            Ok(SkillInstallTarget::Library {
                id: item.id.to_string(),
                name: item.name.to_string(),
                description: item.description.to_string(),
            })
        }
        "bundle" => {
            let slug = rest.trim().trim_matches('/');
            let bundle = bundle_by_slug(config_dir, slug)
                .ok_or_else(|| format!("Unknown skill bundle `{slug}`"))?;
            Ok(SkillInstallTarget::Bundle(bundle))
        }
        other => Err(format!(
            "Unknown install source `{other}`. Use clawhub/, skills-sh/, github/, library/, or bundle/."
        )),
    }
}

fn split_qualified(id: &str) -> Result<(String, String), String> {
    let (head, rest) = id
        .split_once('/')
        .or_else(|| id.split_once(':'))
        .ok_or_else(|| {
            "identifier must be source-qualified (e.g. clawhub/slug or library/weekly-meal-plan)"
                .to_string()
        })?;
    Ok((head.trim().to_ascii_lowercase(), rest.to_string()))
}

/// Build the confirm-dialog payload for an identifier.
pub fn preview_skill_install(
    config_dir: &Path,
    identifier: &str,
) -> Result<SkillInstallPreview, String> {
    let target = resolve_skill_install_identifier(config_dir, identifier)?;
    Ok(preview_for_target(config_dir, identifier, &target))
}

fn preview_for_target(
    config_dir: &Path,
    identifier: &str,
    target: &SkillInstallTarget,
) -> SkillInstallPreview {
    match target {
        SkillInstallTarget::Hub(hit) => {
            let key = skill_dir_name(if hit.name.trim().is_empty() {
                &hit.id
            } else {
                &hit.name
            });
            let dest = config_dir.join("skills").join(&key);
            SkillInstallPreview {
                identifier: identifier.to_string(),
                source: hub_source_label(hit.source),
                name: if hit.name.trim().is_empty() {
                    hit.id.clone()
                } else {
                    hit.name.clone()
                },
                description: hit.description.clone(),
                destination: dest.display().to_string(),
                kind: "skill".into(),
                skills: Vec::new(),
            }
        }
        SkillInstallTarget::Library { id, name, description } => {
            let key = skill_dir_name(id);
            let dest = config_dir.join("skills").join(&key);
            SkillInstallPreview {
                identifier: identifier.to_string(),
                source: "Skills Library".into(),
                name: name.clone(),
                description: description.clone(),
                destination: dest.display().to_string(),
                kind: "skill".into(),
                skills: Vec::new(),
            }
        }
        SkillInstallTarget::Bundle(bundle) => {
            let dest = bundles_dir(config_dir).join(format!("{}.yaml", bundle.slug()));
            SkillInstallPreview {
                identifier: identifier.to_string(),
                source: "Skill bundle".into(),
                name: bundle.name.clone(),
                description: bundle.description.clone(),
                destination: dest.display().to_string(),
                kind: "bundle".into(),
                skills: bundle.skills.clone(),
            }
        }
    }
}

fn hub_source_label(source: HubSource) -> String {
    match source {
        HubSource::ClawHub => "ClawHub".into(),
        HubSource::SkillsSh => "SkillsSH".into(),
        HubSource::GitHub => "GitHub".into(),
        HubSource::WellKnown => "Well-known skills index".into(),
    }
}

pub fn bundles_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("skill-bundles")
}

/// Cabin skills folder for install destinations.
pub fn skills_install_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("skills")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn parses_query_identifier_and_url_encoding() {
        assert_eq!(
            parse_skill_install_url(
                "loadout://skill/install?identifier=library%2Fweekly-meal-plan"
            )
            .unwrap(),
            "library/weekly-meal-plan"
        );
        assert_eq!(
            parse_skill_install_url(
                "loadout://skill/install?identifier=clawhub/good-skill&x=1"
            )
            .unwrap(),
            "clawhub/good-skill"
        );
        assert!(is_skill_install_url(
            "loadout://skill/install?identifier=bundle/backend-dev"
        ));
        assert!(!is_skill_install_url("https://example.com"));
    }

    #[test]
    fn preview_library_shows_source_and_destination() {
        let root = std::env::temp_dir().join(format!(
            "gh-skill-link-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let preview =
            preview_skill_install(&root, "library/weekly-meal-plan").unwrap();
        assert_eq!(preview.source, "Skills Library");
        assert_eq!(preview.name, "Weekly Meal Plan");
        assert_eq!(preview.kind, "skill");
        assert!(
            preview.destination.ends_with("skills/weekly-meal-plan"),
            "{}",
            preview.destination
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn preview_backend_dev_bundle_lists_three_skills() {
        let root = std::env::temp_dir().join(format!(
            "gh-skill-bundle-prev-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let preview = preview_skill_install(&root, "bundle/backend-dev").unwrap();
        assert_eq!(preview.source, "Skill bundle");
        assert_eq!(preview.kind, "bundle");
        assert_eq!(
            preview.skills,
            vec![
                "tdd".to_string(),
                "land-it".to_string(),
                "follow-through".to_string()
            ]
        );
        assert!(
            preview.destination.ends_with("skill-bundles/backend-dev.yaml"),
            "{}",
            preview.destination
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn clawhub_and_github_identifiers_resolve() {
        let root = Path::new("/tmp/unused-config");
        match resolve_skill_install_identifier(root, "clawhub/tidy-inbox").unwrap() {
            SkillInstallTarget::Hub(h) => {
                assert_eq!(h.source, HubSource::ClawHub);
                assert_eq!(h.id, "tidy-inbox");
            }
            other => panic!("expected hub, got {other:?}"),
        }
        match resolve_skill_install_identifier(
            root,
            "github/acme/skills/review",
        )
        .unwrap()
        {
            SkillInstallTarget::Hub(h) => {
                assert_eq!(h.source, HubSource::GitHub);
                assert_eq!(h.id, "acme/skills/review");
            }
            other => panic!("expected hub, got {other:?}"),
        }
    }
}
