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
    Clear,
    Rename(String),
    Move(String),
    Folder(String),
    Delete { yes: bool },
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

/// The saved rows; none when the file isn't there yet.
pub fn load(config_dir: &Path) -> Result<Vec<ProjectNode>, String> {
    match std::fs::read_to_string(path(config_dir)) {
        Ok(raw) if raw.trim().is_empty() => Ok(Vec::new()),
        Ok(raw) => serde_json::from_str(&raw)
            .map_err(|_| "The project list couldn't be read, so nothing was changed.".to_string()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e.to_string()),
    }
}

/// Writes through a temp file and a rename, as the shell's settings do.
pub fn save(config_dir: &Path, nodes: &[ProjectNode]) -> Result<(), String> {
    std::fs::create_dir_all(config_dir).map_err(|e| e.to_string())?;
    let raw = serde_json::to_string(nodes).map_err(|e| e.to_string())?;
    let tmp = config_dir.join("projects.json.tmp");
    std::fs::write(&tmp, raw).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path(config_dir)).map_err(|e| e.to_string())
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
        .map(|n| {
            let dir = grokhub_core::expand_project_root(&n.path, home);
            CodeProject {
                name: grokhub_core::project_display_name(&n.name).to_string(),
                current: same_folder(&n.path, bound, home),
                missing: !Path::new(&dir).is_dir(),
                id: n.id,
                path: n.path,
            }
        })
        .collect();
    Ok(CodeProjects {
        bound: bound.trim().to_string(),
        projects,
    })
}

/// Makes a project under the work root and names its folder, so the Code tab
/// can open it at once.
pub fn create(config_dir: &Path, name: &str) -> Result<String, String> {
    create_in(config_dir, name, &work_root())
}

fn create_in(config_dir: &Path, name: &str, work_root: &str) -> Result<String, String> {
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
                },
                ProjectNode {
                    id: "p1".into(),
                    name: "Harbor".into(),
                    kind: ProjectKind::Project,
                    path: here.clone(),
                    parent: Some("f1".into()),
                    open: false,
                },
                ProjectNode {
                    id: "p2".into(),
                    name: "Dawn".into(),
                    kind: ProjectKind::Project,
                    path: gone.clone(),
                    parent: None,
                    open: false,
                },
            ],
        )
        .expect("save");
        let list = listing_in(&config, &format!("{here}/"), None).expect("listing");
        assert_eq!(list.bound, format!("{here}/"));
        assert_eq!(
            list.projects,
            vec![
                CodeProject {
                    id: "p1".into(),
                    name: "Harbor".into(),
                    path: here,
                    current: true,
                    missing: false,
                },
                CodeProject {
                    id: "p2".into(),
                    name: "Dawn".into(),
                    path: gone,
                    current: false,
                    missing: true,
                },
            ]
        );
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
