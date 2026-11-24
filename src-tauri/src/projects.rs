//! `/project`: the shell's side of the project list the cabin keeps in
//! `{config}/projects.json` (`grokhub_core::project` rows). A bind here writes
//! the same row the cabin reads, and the folder the chat works in is the
//! bound project, as in the cabin.

use std::path::{Path, PathBuf};

use grokhub_core::{ProjectKind, ProjectNode};

/// What each `/project` form asks for. Delete is two steps: `yes` makes it
/// real.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    Show,
    New(String),
    Bind(String),
    /// `/room <name>`: bind a folder named for the room under the work root.
    Room(String),
    Clear,
    Rename(String),
    Move(String),
    Folder(String),
    Delete {
        yes: bool,
    },
}

/// What a `/project` form did: the line to show, and the folder the chat
/// works in now when the bind changed (`Some("")` is unbound).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Done {
    pub text: String,
    pub workspace: Option<String>,
    /// A delete that still waits for its yes: the question to ask.
    pub ask: Option<String>,
}

fn said(text: impl Into<String>) -> Done {
    Done {
        text: text.into(),
        workspace: None,
        ask: None,
    }
}

pub fn path(config_dir: &Path) -> PathBuf {
    config_dir.join("projects.json")
}

/// The saved rows; none when the file isn't there yet. A row's remote URL
/// comes back without credentials, whatever wrote it.
pub fn load(config_dir: &Path) -> Result<Vec<ProjectNode>, String> {
    let mut nodes: Vec<ProjectNode> = match std::fs::read_to_string(path(config_dir)) {
        Ok(raw) if raw.trim().is_empty() => Vec::new(),
        Ok(raw) => serde_json::from_str(&raw).map_err(|_| {
            "The project list couldn't be read, so nothing was changed.".to_string()
        })?,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(e.to_string()),
    };
    for url in nodes.iter_mut().filter_map(|n| n.remote_url.as_mut()) {
        *url = grokhub_code::link::sanitize_remote_url(url);
    }
    Ok(nodes)
}

/// Writes through a temp file of its own and a rename, so a reader never
/// sees half a list and two writers never share a temp file.
pub fn save(config_dir: &Path, nodes: &[ProjectNode]) -> Result<(), String> {
    let raw = serde_json::to_string(nodes).map_err(|e| e.to_string())?;
    grokhub_core::work_file::write_private_atomic(&path(config_dir), raw.as_bytes())
}

/// Held for each load, change and save of the list, so two of them at once
/// (two windows, a chat's `/project` and the Code tab) never drop each
/// other's row.
fn edit_lock() -> std::sync::MutexGuard<'static, ()> {
    static EDIT: std::sync::Mutex<()> = std::sync::Mutex::new(());
    EDIT.lock().unwrap_or_else(|e| e.into_inner())
}

fn work_root() -> String {
    let home = std::env::var("HOME").ok();
    let profile = std::env::var("USERPROFILE").ok();
    grokhub_core::cabin_work_root(cfg!(windows), home.as_deref(), profile.as_deref())
}

fn home() -> Option<String> {
    std::env::var("HOME").ok()
}

pub(crate) fn same_folder(a: &str, b: &str, home: Option<&str>) -> bool {
    let key = |p: &str| {
        grokhub_core::normalize_host_path(&grokhub_core::expand_project_root(p, home))
            .trim_end_matches('/')
            .to_string()
    };
    !a.trim().is_empty() && key(a) == key(b)
}

/// True when `folder` is the folder the app works in now (`bound`).
pub fn is_open(folder: &str, bound: &str) -> bool {
    same_folder(folder, bound, home().as_deref())
}

/// The row for the bound folder, when the list has one.
fn selected(nodes: &[ProjectNode], bound: &str, home: Option<&str>) -> Option<String> {
    nodes
        .iter()
        .find(|n| n.kind == ProjectKind::Project && same_folder(&n.path, bound, home))
        .map(|n| n.id.clone())
}

const SELECT_FIRST: &str = "Select a project first: /project bind <folder>.";

/// Runs one `/project` form against the saved list. `bound` is the folder
/// the chat works in now.
pub fn run(config_dir: &Path, bound: &str, op: Op) -> Result<Done, String> {
    run_in(config_dir, bound, op, &work_root(), home().as_deref())
}

/// [`run`] with the work root and home folder named, so a test never touches
/// the real ones.
pub fn run_in(
    config_dir: &Path,
    bound: &str,
    op: Op,
    work_root: &str,
    home: Option<&str>,
) -> Result<Done, String> {
    let _edit = edit_lock();
    let mut nodes = load(config_dir)?;
    match op {
        Op::Show => Ok(said(if bound.trim().is_empty() {
            "No bound project".to_string()
        } else {
            format!("Project {bound}")
        })),
        Op::New(name) => {
            let id = grokhub_core::uid("proj");
            let i = grokhub_core::create_project(&mut nodes, &id, &name, None, work_root)
                .map_err(str::to_string)?;
            let dir = nodes[i].path.clone();
            let label = grokhub_core::project_display_name(&nodes[i].name).to_string();
            save(config_dir, &nodes)?;
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            Ok(said(format!("Project {label}")))
        }
        Op::Bind(raw) => {
            let p = grokhub_core::resolve_bind_path(&raw, bound, work_root, home);
            std::fs::create_dir_all(&p).map_err(|e| e.to_string())?;
            grokhub_core::upsert_bound(&mut nodes, &p);
            save(config_dir, &nodes)?;
            Ok(Done {
                text: format!("Bound {p}"),
                workspace: Some(p),
                ask: None,
            })
        }
        Op::Room(name) => {
            // The cabin also ran a desktop-switch script here, which its own
            // project guard always blocked; only the bind is kept.
            let plan = grokhub_core::plan_room(&name, home.unwrap_or("."));
            let p = Path::new(work_root)
                .join(&plan.workspace)
                .to_string_lossy()
                .into_owned();
            std::fs::create_dir_all(&p).map_err(|e| e.to_string())?;
            grokhub_core::upsert_bound(&mut nodes, &p);
            save(config_dir, &nodes)?;
            Ok(Done {
                text: format!("Room {} → {p}", plan.slug),
                workspace: Some(p),
                ask: None,
            })
        }
        Op::Clear => Ok(Done {
            text: "Unbound — full desktop".into(),
            workspace: Some(String::new()),
            ask: None,
        }),
        Op::Rename(name) => {
            let id = selected(&nodes, bound, home).ok_or(SELECT_FIRST)?;
            grokhub_core::rename_node(&mut nodes, &id, &name).map_err(str::to_string)?;
            save(config_dir, &nodes)?;
            Ok(said(format!("Renamed {name}")))
        }
        Op::Folder(name) => {
            let id = grokhub_core::uid("fold");
            let i = grokhub_core::create_folder(&mut nodes, &id, &name, None)
                .map_err(str::to_string)?;
            let label = nodes[i].name.clone();
            save(config_dir, &nodes)?;
            Ok(said(format!("Folder {label}")))
        }
        Op::Move(folder) => {
            let id = selected(&nodes, bound, home).ok_or(SELECT_FIRST)?;
            if folder.eq_ignore_ascii_case("root") {
                grokhub_core::add_to_folder(&mut nodes, &id, None).map_err(str::to_string)?;
                save(config_dir, &nodes)?;
                return Ok(said("Moved to Projects"));
            }
            let fid = nodes
                .iter()
                .find(|n| n.kind == ProjectKind::Folder && n.name.eq_ignore_ascii_case(&folder))
                .map(|n| n.id.clone())
                .ok_or_else(|| format!("No folder named {folder}"))?;
            grokhub_core::add_to_folder(&mut nodes, &id, Some(&fid)).map_err(str::to_string)?;
            save(config_dir, &nodes)?;
            Ok(said(format!("Moved to {folder}")))
        }
        Op::Delete { yes } => {
            let id = selected(&nodes, bound, home).ok_or(SELECT_FIRST)?;
            let name = nodes
                .iter()
                .find(|n| n.id == id)
                .map(|n| n.name.clone())
                .unwrap_or_default();
            if !yes {
                return Ok(Done {
                    text: String::new(),
                    workspace: None,
                    ask: Some(format!(
                        "Remove project {name} ({bound}) from your list? Its files stay on disk."
                    )),
                });
            }
            let out = grokhub_core::drop_selected(&mut nodes, &id, bound);
            if !out.dropped {
                return Err("Project not found".into());
            }
            save(config_dir, &nodes)?;
            Ok(Done {
                text: format!("Removed project {}", out.name),
                workspace: out.unbound.then(String::new),
                ask: None,
            })
        }
    }
}

/// Sync state for the open project's git repo.
#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct CodeProjectSync {
    pub ahead: u32,
    pub behind: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub upstream: Option<String>,
}

/// One discovered rule file shown on the project bar.
#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodeProjectRule {
    pub path: String,
    pub kind: String,
}

/// One project on the Code tab's project bar.
#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodeProject {
    pub id: String,
    pub name: String,
    pub path: String,
    /// The folder the app works in now.
    pub current: bool,
    /// The folder is no longer on disk.
    pub missing: bool,
    /// Linked remote URL when set (never includes credentials).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_url: Option<String>,
    /// Working / PR-base branch for this project.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preferred_branch: Option<String>,
    /// Checked-out branch when this is a git repo.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sync: Option<CodeProjectSync>,
    /// Rule files in force (paths + kinds).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<CodeProjectRule>,
}

/// The Code tab's project bar: every project in the list, and which one is
/// open. `bound` is the open folder, `""` when none is.
#[derive(serde::Serialize, Debug, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CodeProjects {
    pub bound: String,
    pub projects: Vec<CodeProject>,
}

/// The saved projects in the file's order, for the Code tab.
pub fn listing(config_dir: &Path, bound: &str) -> Result<CodeProjects, String> {
    listing_in(config_dir, bound, home().as_deref())
}

fn listing_in(config_dir: &Path, bound: &str, home: Option<&str>) -> Result<CodeProjects, String> {
    let projects = load(config_dir)?
        .into_iter()
        .filter(|n| n.kind == ProjectKind::Project)
        .map(|n| enrich_project(n, bound, home))
        .collect();
    Ok(CodeProjects {
        bound: bound.trim().to_string(),
        projects,
    })
}

fn enrich_project(n: ProjectNode, bound: &str, home: Option<&str>) -> CodeProject {
    let dir = grokhub_core::expand_project_root(&n.path, home);
    let current = same_folder(&n.path, bound, home);
    let missing = !Path::new(&dir).is_dir();
    let mut remote_url = n.remote_url.clone();
    let mut preferred_branch = n.preferred_branch.clone();
    let mut active_branch = None;
    let mut sync = None;
    let mut rules = Vec::new();
    // Deep git + rules only for the open project so the list stays cheap.
    if !missing && current {
        let path = Path::new(&dir);
        if remote_url.is_none() {
            remote_url = grokhub_code::link::remote_url(path);
        }
        let has_beta = grokhub_code::link::has_beta(path);
        let default_branch = grokhub_code::ship::default_branch(path);
        let repo = grokhub_code::repo::repo(path);
        if repo.is_repo {
            active_branch = repo.branch.clone();
            sync = Some(CodeProjectSync {
                ahead: repo.ahead,
                behind: repo.behind,
                upstream: repo.upstream.clone(),
            });
        }
        preferred_branch = Some(grokhub_agent::resolve_preferred_branch(
            &grokhub_agent::RepoRulesInput {
                preferred_branch: n.preferred_branch.as_deref(),
                has_beta,
                default_branch: default_branch.as_deref(),
                active_branch: active_branch.as_deref(),
                ..Default::default()
            },
        ));
        let bundle = grokhub_agent::discover_repo_rules(
            path,
            grokhub_agent::RepoRulesInput {
                project_id: &n.id,
                remote_url: remote_url.as_deref(),
                preferred_branch: preferred_branch.as_deref(),
                active_branch: active_branch.as_deref(),
                default_branch: default_branch.as_deref(),
                has_beta,
                sync: sync.as_ref().map(|s| grokhub_agent::RepoSync {
                    ahead: s.ahead,
                    behind: s.behind,
                    upstream: s.upstream.clone(),
                }),
            },
        );
        rules = bundle
            .files
            .into_iter()
            .map(|f| CodeProjectRule {
                path: f.path,
                kind: rule_kind_label(f.kind).into(),
            })
            .collect();
    }
    CodeProject {
        id: n.id,
        name: grokhub_core::project_display_name(&n.name).to_string(),
        path: n.path,
        current,
        missing,
        remote_url,
        preferred_branch,
        active_branch,
        sync,
        rules,
    }
}

fn rule_kind_label(kind: grokhub_agent::RuleKind) -> &'static str {
    match kind {
        grokhub_agent::RuleKind::Agents => "agents",
        grokhub_agent::RuleKind::Claude => "claude",
        grokhub_agent::RuleKind::CursorRules => "cursorRules",
        grokhub_agent::RuleKind::Contributing => "contributing",
        grokhub_agent::RuleKind::PrTemplate => "prTemplate",
        grokhub_agent::RuleKind::Codeowners => "codeowners",
        grokhub_agent::RuleKind::Workflow => "workflow",
        grokhub_agent::RuleKind::PreCommit => "preCommit",
        grokhub_agent::RuleKind::CommitConvention => "commitConvention",
        grokhub_agent::RuleKind::Other => "other",
    }
}

/// Set or clear the per-project preferred branch override.
pub fn set_preferred_branch(
    config_dir: &Path,
    project_id: &str,
    branch: Option<&str>,
) -> Result<(), String> {
    let _edit = edit_lock();
    let mut nodes = load(config_dir)?;
    let node = nodes
        .iter_mut()
        .find(|n| n.id == project_id && n.kind == ProjectKind::Project)
        .ok_or_else(|| "Project not found.".to_string())?;
    let branch = branch.map(str::trim).filter(|s| !s.is_empty());
    if let Some(b) = branch {
        grokhub_code::ship::branch_ok(b)
            .map_err(|_| "That isn't a branch name git accepts.".to_string())?;
        node.preferred_branch = Some(b.to_string());
    } else {
        node.preferred_branch = None;
    }
    save(config_dir, &nodes)
}

/// Persist a remote URL (sanitized) on a project row.
pub fn set_remote_url(
    config_dir: &Path,
    project_id: &str,
    url: Option<&str>,
) -> Result<(), String> {
    let _edit = edit_lock();
    let mut nodes = load(config_dir)?;
    let node = nodes
        .iter_mut()
        .find(|n| n.id == project_id && n.kind == ProjectKind::Project)
        .ok_or_else(|| "Project not found.".to_string())?;
    node.remote_url = url
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(grokhub_code::link::sanitize_remote_url);
    save(config_dir, &nodes)
}

/// After a bind, copy link metadata onto the matching project row.
/// When `force` is false, only fills blank fields (so Open folder keeps a
/// user override). Remote link passes `force: true`.
pub fn apply_link(
    config_dir: &Path,
    folder: &str,
    remote_url: Option<&str>,
    preferred_branch: Option<&str>,
    force: bool,
) -> Result<(), String> {
    let home = home();
    let _edit = edit_lock();
    let mut nodes = load(config_dir)?;
    let Some(node) = nodes
        .iter_mut()
        .find(|n| n.kind == ProjectKind::Project && same_folder(&n.path, folder, home.as_deref()))
    else {
        return Ok(());
    };
    if let Some(url) = remote_url.map(str::trim).filter(|s| !s.is_empty()) {
        if force || node.remote_url.is_none() {
            node.remote_url = Some(grokhub_code::link::sanitize_remote_url(url));
        }
    }
    if let Some(b) = preferred_branch.map(str::trim).filter(|s| !s.is_empty()) {
        if force || node.preferred_branch.is_none() {
            node.preferred_branch = Some(b.to_string());
        }
    }
    save(config_dir, &nodes)
}

/// Makes a project under the work root and names its folder, so the Code tab
/// can open it at once.
pub fn create(config_dir: &Path, name: &str) -> Result<String, String> {
    create_in(config_dir, name, &work_root())
}

fn create_in(config_dir: &Path, name: &str, work_root: &str) -> Result<String, String> {
    let _edit = edit_lock();
    let mut nodes = load(config_dir)?;
    let id = grokhub_core::uid("proj");
    let i = grokhub_core::create_project(&mut nodes, &id, name, None, work_root)
        .map_err(str::to_string)?;
    let dir = nodes[i].path.clone();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    save(config_dir, &nodes)?;
    Ok(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "grokhub-shell-projects-{name}-{}-{}",
            std::process::id(),
            grokhub_core::now_ms()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn go(config: &Path, bound: &str, op: Op, root: &Path) -> Result<Done, String> {
        run_in(config, bound, op, &root.to_string_lossy(), None)
    }

    #[test]
    fn bind_writes_the_row_the_cabin_reads_and_names_the_folder() {
        let (config, root) = (temp_dir("bind-cfg"), temp_dir("bind-root"));
        let folder = root.join("harbor");
        let raw = folder.to_string_lossy().into_owned();
        let done = go(&config, "", Op::Bind(raw), &root).expect("bind");
        let bound = done.workspace.clone().expect("a new folder");
        assert_eq!(done.text, format!("Bound {bound}"));
        assert!(folder.is_dir(), "the folder is made");
        let saved: Vec<ProjectNode> = serde_json::from_str(
            &std::fs::read_to_string(config.join("projects.json")).expect("projects.json"),
        )
        .expect("the cabin's row shape");
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].kind, ProjectKind::Project);
        assert_eq!(saved[0].path, bound);
        assert_eq!(saved[0].parent, None);
        // Binding it again adds no second row.
        go(&config, &bound, Op::Bind(bound.clone()), &root).expect("again");
        assert_eq!(load(&config).expect("load").len(), 1);
    }

    #[test]
    fn a_room_binds_its_folder_under_the_work_root() {
        let (config, root) = (temp_dir("room-cfg"), temp_dir("room-root"));
        let done = go(&config, "", Op::Room("Harbor Lab".into()), &root).expect("room");
        let folder = root.join("harbor-lab");
        let p = folder.to_string_lossy().into_owned();
        assert_eq!(done.text, format!("Room harbor-lab → {p}"));
        assert_eq!(done.workspace.as_deref(), Some(p.as_str()));
        assert!(folder.is_dir(), "the folder is made");
        let saved = load(&config).expect("load");
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].kind, ProjectKind::Project);
        assert_eq!(saved[0].path, p);
        // A docs room shares one docs folder, as in the cabin.
        let docs = go(&config, &p, Op::Room("stage a docs desk".into()), &root).expect("docs");
        let docs_dir = root.join("docs").to_string_lossy().into_owned();
        assert_eq!(docs.text, format!("Room docs-desk → {docs_dir}"));
        assert_eq!(docs.workspace.as_deref(), Some(docs_dir.as_str()));
        assert_eq!(load(&config).expect("load").len(), 2);
    }

    #[test]
    fn show_and_clear_name_the_bound_folder() {
        let (config, root) = (temp_dir("show-cfg"), temp_dir("show-root"));
        assert_eq!(
            go(&config, "", Op::Show, &root).expect("show").text,
            "No bound project"
        );
        assert_eq!(
            go(&config, "/work/harbor", Op::Show, &root)
                .expect("show")
                .text,
            "Project /work/harbor"
        );
        let cleared = go(&config, "/work/harbor", Op::Clear, &root).expect("clear");
        assert_eq!(cleared.text, "Unbound — full desktop");
        assert_eq!(cleared.workspace, Some(String::new()));
        assert!(!config.join("projects.json").exists(), "clear edits no row");
    }

    #[test]
    fn a_row_the_cabin_wrote_is_found_renamed_moved_and_removed() {
        let (config, root) = (temp_dir("row-cfg"), temp_dir("row-root"));
        let path = root.join("harbor").to_string_lossy().into_owned();
        let cabin_row = format!(
            r#"[{{"id":"proj-1","name":"Harbor","kind":"project","path":{},"parent":null,"open":false}}]"#,
            serde_json::to_string(&path).expect("json")
        );
        std::fs::write(config.join("projects.json"), cabin_row).expect("seed");
        assert_eq!(
            go(&config, &path, Op::Rename("Dawn".into()), &root)
                .expect("rename")
                .text,
            "Renamed Dawn"
        );
        assert_eq!(load(&config).expect("load")[0].name, "Dawn");
        assert_eq!(
            go(&config, &path, Op::Folder("Docks".into()), &root)
                .expect("folder")
                .text,
            "Folder Docks"
        );
        assert_eq!(
            go(&config, &path, Op::Move("docks".into()), &root)
                .expect("move")
                .text,
            "Moved to docks"
        );
        let rows = load(&config).expect("load");
        let folder = rows
            .iter()
            .find(|n| n.kind == ProjectKind::Folder)
            .expect("folder");
        let project = rows
            .iter()
            .find(|n| n.kind == ProjectKind::Project)
            .expect("project");
        assert_eq!(project.parent.as_deref(), Some(folder.id.as_str()));
        assert_eq!(
            go(&config, &path, Op::Move("nowhere".into()), &root),
            Err("No folder named nowhere".into())
        );
        assert_eq!(
            go(&config, &path, Op::Move("root".into()), &root)
                .expect("root")
                .text,
            "Moved to Projects"
        );
        assert_eq!(load(&config).expect("load")[0].parent, None);

        // Delete asks first, with the name and the folder, and changes nothing.
        let asked = go(&config, &path, Op::Delete { yes: false }, &root).expect("asks");
        assert_eq!(
            asked.ask,
            Some(format!(
                "Remove project Dawn ({path}) from your list? Its files stay on disk."
            ))
        );
        assert_eq!(load(&config).expect("load").len(), 2);
        let done = go(&config, &path, Op::Delete { yes: true }, &root).expect("delete");
        assert_eq!(done.text, "Removed project Dawn");
        assert_eq!(done.workspace, Some(String::new()), "it was the bound one");
        assert_eq!(
            load(&config).expect("load").len(),
            1,
            "only the folder is left"
        );
    }

    #[test]
    fn rename_move_and_delete_need_a_bound_project() {
        let (config, root) = (temp_dir("none-cfg"), temp_dir("none-root"));
        for op in [
            Op::Rename("x".into()),
            Op::Move("root".into()),
            Op::Delete { yes: true },
        ] {
            assert_eq!(
                go(&config, "", op, &root),
                Err("Select a project first: /project bind <folder>.".into())
            );
        }
    }

    #[test]
    fn new_makes_a_project_folder_under_the_work_root() {
        let (config, root) = (temp_dir("new-cfg"), temp_dir("new-root"));
        let done = go(&config, "", Op::New("Night watch".into()), &root).expect("new");
        assert!(done.text.starts_with("Project "), "{}", done.text);
        let rows = load(&config).expect("load");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "Night watch");
        assert!(
            Path::new(&rows[0].path).starts_with(&root),
            "{}",
            rows[0].path
        );
        assert!(Path::new(&rows[0].path).is_dir());
        assert_eq!(
            go(&config, "", Op::New("  ".into()), &root),
            Err("need a project name".into())
        );
        assert_eq!(
            go(&config, "", Op::Folder("".into()), &root),
            Err("need a folder name".into())
        );
    }

    #[test]
    fn eight_projects_made_at_once_keep_all_eight_rows() {
        let (config, root) = (temp_dir("race-cfg"), temp_dir("race-root"));
        let threads: Vec<_> = (0..8)
            .map(|i| {
                let (config, root) = (config.clone(), root.clone());
                std::thread::spawn(move || go(&config, "", Op::New(format!("p{i}")), &root))
            })
            .collect();
        let errors: Vec<String> = threads
            .into_iter()
            .filter_map(|t| t.join().unwrap().err())
            .collect();
        let mut names: Vec<String> = load(&config)
            .expect("load")
            .into_iter()
            .map(|n| n.name)
            .collect();
        names.sort();
        let _ = std::fs::remove_dir_all(&config);
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(errors, Vec::<String>::new());
        assert_eq!(names, ["p0", "p1", "p2", "p3", "p4", "p5", "p6", "p7"]);
    }

    #[test]
    fn two_writers_round_after_round_keep_both_rows_and_a_readable_list() {
        let (mut failed, mut lost, mut unreadable) = (0, 0, 0);
        for round in 0..50 {
            let config = temp_dir(&format!("pair-cfg-{round}"));
            let root = temp_dir(&format!("pair-root-{round}"));
            let writer = |name: &str| {
                let (config, root, name) = (config.clone(), root.clone(), name.to_string());
                std::thread::spawn(move || go(&config, "", Op::New(name), &root))
            };
            let (a, b) = (writer("alpha"), writer("beta"));
            let made = [a.join().unwrap(), b.join().unwrap()]
                .iter()
                .filter(|r| r.is_ok())
                .count();
            failed += usize::from(made < 2);
            match load(&config) {
                Ok(rows) => lost += usize::from(rows.len() < made),
                Err(_) => unreadable += 1,
            }
            let _ = std::fs::remove_dir_all(&config);
            let _ = std::fs::remove_dir_all(&root);
        }
        assert_eq!((failed, lost, unreadable), (0, 0, 0));
    }

    /// Origin's URL can carry a token (set by hand, or by an `insteadOf`
    /// rule); the row the page gets never does.
    #[test]
    fn the_open_project_row_never_carries_credentials() {
        let (config, root) = (temp_dir("cred-cfg"), temp_dir("cred-root"));
        let repo = root.join("app");
        std::fs::create_dir_all(&repo).unwrap();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(&repo)
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?}");
        };
        git(&["init", "-q"]);
        git(&[
            "remote",
            "add",
            "origin",
            "https://user:sk-abcdefghijklmnopqrstuv@github.com/org/app.git?private_token=sk-abcdefghijklmnopqrstuv",
        ]);
        let bound = repo.to_string_lossy().into_owned();
        // `/project bind <folder>` from chat writes the row with no remote.
        go(&config, "", Op::Bind(bound.clone()), &root).expect("bind");
        let open = listing_in(&config, &bound, None).expect("listing");
        // A row saved with a token in it by an older version.
        let mut rows = load(&config).expect("load");
        rows[0].remote_url =
            Some("ssh://git:sk-abcdefghijklmnopqrstuv@example.com/org/app.git".into());
        save(&config, &rows).expect("save");
        let saved = listing_in(&config, &bound, None).expect("listing");
        let _ = std::fs::remove_dir_all(&config);
        let _ = std::fs::remove_dir_all(&root);
        assert_eq!(
            open.projects[0].remote_url.as_deref(),
            Some("https://github.com/org/app.git")
        );
        assert_eq!(
            saved.projects[0].remote_url.as_deref(),
            Some("ssh://git@example.com/org/app.git")
        );
    }

    #[test]
    fn a_broken_list_is_left_alone() {
        let (config, root) = (temp_dir("bad-cfg"), temp_dir("bad-root"));
        std::fs::write(config.join("projects.json"), "{not json").expect("seed");
        assert_eq!(
            go(&config, "", Op::Folder("Docks".into()), &root),
            Err("The project list couldn't be read, so nothing was changed.".into())
        );
        assert_eq!(
            std::fs::read_to_string(config.join("projects.json")).expect("read"),
            "{not json"
        );
    }

    #[test]
    fn the_code_tab_lists_projects_and_marks_the_open_and_the_missing_one() {
        let (config, root) = (temp_dir("list-cfg"), temp_dir("list-root"));
        let here = root.join("harbor");
        std::fs::create_dir_all(&here).expect("folder");
        let (here, gone) = (
            here.to_string_lossy().into_owned(),
            root.join("gone").to_string_lossy().into_owned(),
        );
        save(
            &config,
            &[
                ProjectNode {
                    id: "f1".into(),
                    name: "Docks".into(),
                    kind: ProjectKind::Folder,
                    path: String::new(),
                    parent: None,
                    open: true,
                    remote_url: None,
                    preferred_branch: None,
                },
                ProjectNode {
                    id: "p1".into(),
                    name: "Harbor".into(),
                    kind: ProjectKind::Project,
                    path: here.clone(),
                    parent: Some("f1".into()),
                    open: false,
                    remote_url: None,
                    preferred_branch: None,
                },
                ProjectNode {
                    id: "p2".into(),
                    name: "Dawn".into(),
                    kind: ProjectKind::Project,
                    path: gone.clone(),
                    parent: None,
                    open: false,
                    remote_url: None,
                    preferred_branch: None,
                },
            ],
        )
        .expect("save");
        let list = listing_in(&config, &format!("{here}/"), None).expect("listing");
        assert_eq!(list.bound, format!("{here}/"));
        assert_eq!(list.projects.len(), 2);
        assert_eq!(list.projects[0].id, "p1");
        assert_eq!(list.projects[0].name, "Harbor");
        assert_eq!(list.projects[0].path, here);
        assert!(list.projects[0].current);
        assert!(!list.projects[0].missing);
        assert_eq!(list.projects[1].id, "p2");
        assert_eq!(list.projects[1].name, "Dawn");
        assert_eq!(list.projects[1].path, gone);
        assert!(!list.projects[1].current);
        assert!(list.projects[1].missing);
        let none = listing_in(&config, "", None).expect("unbound");
        assert_eq!(none.bound, "");
        assert!(none.projects.iter().all(|p| !p.current));
    }

    #[test]
    fn create_makes_the_row_and_the_folder_and_names_it() {
        let (config, root) = (temp_dir("make-cfg"), temp_dir("make-root"));
        let dir = create_in(&config, "Night watch", &root.to_string_lossy()).expect("create");
        assert!(Path::new(&dir).starts_with(&root), "{dir}");
        assert!(Path::new(&dir).is_dir());
        let rows = load(&config).expect("load");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "Night watch");
        assert_eq!(rows[0].path, dir);
        assert_eq!(
            create_in(&config, "  ", &root.to_string_lossy()),
            Err("need a project name".into())
        );
        assert_eq!(load(&config).expect("load").len(), 1);
    }
}
