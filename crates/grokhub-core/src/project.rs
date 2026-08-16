//! Bound project = world. Unbound stays the full desktop.
//! Sidebar folders are org only — they do not move the bound tree.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProjectKind {
    Project,
    Folder,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectNode {
    pub id: String,
    pub name: String,
    pub kind: ProjectKind,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub open: bool,
}

pub fn clean_project_name(name: &str) -> Option<String> {
    let t: String = name.trim().chars().take(80).collect();
    if t.is_empty() {
        None
    } else {
        Some(t)
    }
}

pub fn project_slug(name: &str) -> String {
    let s: String = name
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() {
        "project".into()
    } else {
        s
    }
}

pub fn project_work_path(work_root: &str, name: &str) -> String {
    let root = work_root.replace('\\', "/").trim_end_matches('/').to_string();
    format!("{root}/{}", project_slug(name))
}

pub fn seed_from_bound(bound_path: &str) -> Vec<ProjectNode> {
    let path = bound_path.trim();
    if path.is_empty() {
        return Vec::new();
    }
    vec![ProjectNode {
        id: "bound".into(),
        name: project_name_from_path(path),
        kind: ProjectKind::Project,
        path: path.to_string(),
        parent: None,
        open: false,
    }]
}

fn parent_is_folder(nodes: &[ProjectNode], parent: Option<&str>) -> Result<Option<String>, &'static str> {
    let Some(pid) = parent else {
        return Ok(None);
    };
    match nodes.iter().find(|n| n.id == pid) {
        Some(n) if n.kind == ProjectKind::Folder => Ok(Some(pid.to_string())),
        Some(_) => Err("parent must be a folder"),
        None => Err("folder not found"),
    }
}

pub fn create_project(
    nodes: &mut Vec<ProjectNode>,
    id: &str,
    name: &str,
    parent: Option<&str>,
    work_root: &str,
) -> Result<usize, &'static str> {
    let name = clean_project_name(name).ok_or("need a project name")?;
    let parent = parent_is_folder(nodes, parent)?;
    if nodes.iter().any(|n| n.id == id) {
        return Err("id taken");
    }
    let mut path = project_work_path(work_root, &name);
    if nodes.iter().any(|n| n.path == path) {
        path = format!("{path}-{}", nodes.len());
    }
    nodes.push(ProjectNode {
        id: id.to_string(),
        name: name.clone(),
        kind: ProjectKind::Project,
        path,
        parent,
        open: false,
    });
    Ok(nodes.len() - 1)
}

pub fn stage_project(
    nodes: &mut Vec<ProjectNode>,
    id: &str,
    name: &str,
    parent: Option<&str>,
) -> Result<usize, &'static str> {
    let name = clean_project_name(name).ok_or("need a project name")?;
    let parent = parent_is_folder(nodes, parent)?;
    if nodes.iter().any(|n| n.id == id) {
        return Err("id taken");
    }
    nodes.push(ProjectNode {
        id: id.to_string(),
        name,
        kind: ProjectKind::Project,
        path: String::new(),
        parent,
        open: false,
    });
    Ok(nodes.len() - 1)
}

pub fn settle_project_path(
    nodes: &mut [ProjectNode],
    id: &str,
    work_root: &str,
) -> Result<String, &'static str> {
    let node = nodes.iter().find(|n| n.id == id).ok_or("not found")?;
    if node.kind != ProjectKind::Project {
        return Err("not a project");
    }
    if !node.path.trim().is_empty() {
        return Ok(node.path.clone());
    }
    let name = node.name.clone();
    let mut path = project_work_path(work_root, &name);
    if nodes.iter().any(|n| n.id != id && n.path == path) {
        path = format!("{path}-{}", nodes.len());
    }
    let node = nodes.iter_mut().find(|n| n.id == id).ok_or("not found")?;
    node.path = path.clone();
    Ok(path)
}

pub fn drop_node(nodes: &mut Vec<ProjectNode>, id: &str) -> bool {
    let Some(idx) = nodes.iter().position(|n| n.id == id) else {
        return false;
    };
    if nodes[idx].kind == ProjectKind::Folder {
        for n in nodes.iter_mut() {
            if n.parent.as_deref() == Some(id) {
                n.parent = None;
            }
        }
    }
    nodes.retain(|n| n.id != id);
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectMenuAct {
    Rename,
    AddToFolder,
    RemoveFromFolder,
    NewHere,
    Delete,
}

pub fn project_menu_acts(kind: ProjectKind) -> &'static [ProjectMenuAct] {
    match kind {
        ProjectKind::Project => &[
            ProjectMenuAct::Rename,
            ProjectMenuAct::AddToFolder,
            ProjectMenuAct::RemoveFromFolder,
            ProjectMenuAct::Delete,
        ],
        ProjectKind::Folder => &[
            ProjectMenuAct::Rename,
            ProjectMenuAct::NewHere,
            ProjectMenuAct::Delete,
        ],
    }
}

pub fn project_menu_label(act: ProjectMenuAct) -> &'static str {
    match act {
        ProjectMenuAct::Rename => "Rename",
        ProjectMenuAct::AddToFolder => "Add to folder",
        ProjectMenuAct::RemoveFromFolder => "Remove from folder",
        ProjectMenuAct::NewHere => "New project here",
        ProjectMenuAct::Delete => "Delete",
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DropOutcome {
    pub dropped: bool,
    pub unbound: bool,
    pub name: String,
}

pub fn drop_selected(nodes: &mut Vec<ProjectNode>, id: &str, bound_path: &str) -> DropOutcome {
    let node = nodes.iter().find(|n| n.id == id);
    let name = node.map(|n| n.name.clone()).unwrap_or_default();
    let path = node.map(|n| n.path.clone()).unwrap_or_default();
    let dropped = drop_node(nodes, id);
    let unbound = dropped && !bound_path.trim().is_empty() && path == bound_path;
    DropOutcome {
        dropped,
        unbound,
        name,
    }
}

pub fn should_seed_sidebar(file_present: bool, loaded: &[ProjectNode]) -> bool {
    loaded.is_empty() && !file_present
}

pub fn restore_bound_path(saved: &str, work_root: &str, sidebar_file_present: bool) -> String {
    if !saved.trim().is_empty() {
        return saved.to_string();
    }
    if sidebar_file_present {
        String::new()
    } else {
        work_root.to_string()
    }
}

pub fn create_folder(
    nodes: &mut Vec<ProjectNode>,
    id: &str,
    name: &str,
    parent: Option<&str>,
) -> Result<usize, &'static str> {
    if parent.is_some() {
        return Err("folders stay at the root");
    }
    let name = clean_project_name(name).ok_or("need a folder name")?;
    if nodes.iter().any(|n| n.id == id) {
        return Err("id taken");
    }
    nodes.push(ProjectNode {
        id: id.to_string(),
        name,
        kind: ProjectKind::Folder,
        path: String::new(),
        parent: None,
        open: true,
    });
    Ok(nodes.len() - 1)
}

pub fn rename_node(nodes: &mut [ProjectNode], id: &str, name: &str) -> Result<(), &'static str> {
    let name = clean_project_name(name).ok_or("need a name")?;
    let node = nodes.iter_mut().find(|n| n.id == id).ok_or("not found")?;
    node.name = name;
    Ok(())
}

pub fn add_to_folder(
    nodes: &mut [ProjectNode],
    id: &str,
    folder_id: Option<&str>,
) -> Result<(), &'static str> {
    let parent = match folder_id {
        None => None,
        Some(fid) => {
            let folder = nodes.iter().find(|n| n.id == fid).ok_or("folder not found")?;
            if folder.kind != ProjectKind::Folder {
                return Err("target is not a folder");
            }
            if folder.id == id {
                return Err("cannot nest a folder in itself");
            }
            Some(fid.to_string())
        }
    };
    let node = nodes.iter_mut().find(|n| n.id == id).ok_or("not found")?;
    if node.kind != ProjectKind::Project {
        return Err("only projects go in folders");
    }
    node.parent = parent;
    Ok(())
}

pub fn toggle_folder(nodes: &mut [ProjectNode], id: &str) -> bool {
    if let Some(n) = nodes.iter_mut().find(|n| n.id == id && n.kind == ProjectKind::Folder) {
        n.open = !n.open;
        true
    } else {
        false
    }
}

pub fn visible_tree(nodes: &[ProjectNode]) -> Vec<(u8, usize)> {
    let mut out = Vec::with_capacity(nodes.len());
    let mut shown = vec![false; nodes.len()];
    for (i, n) in nodes.iter().enumerate() {
        if n.kind == ProjectKind::Folder {
            out.push((0, i));
            shown[i] = true;
            if n.open {
                for (j, c) in nodes.iter().enumerate() {
                    if c.kind == ProjectKind::Project && c.parent.as_deref() == Some(n.id.as_str()) {
                        out.push((1, j));
                        shown[j] = true;
                    }
                }
            }
        }
    }
    for (i, n) in nodes.iter().enumerate() {
        if shown[i] || n.kind != ProjectKind::Project {
            continue;
        }
        let hidden = n.parent.as_ref().is_some_and(|pid| {
            nodes
                .iter()
                .any(|f| f.id == *pid && f.kind == ProjectKind::Folder && !f.open)
        });
        if !hidden {
            out.push((0, i));
        }
    }
    out
}

pub fn folder_choices(nodes: &[ProjectNode]) -> Vec<(String, String)> {
    nodes
        .iter()
        .filter(|n| n.kind == ProjectKind::Folder)
        .map(|n| (n.id.clone(), n.name.clone()))
        .collect()
}

pub fn upsert_bound(nodes: &mut Vec<ProjectNode>, bound_path: &str) -> Option<String> {
    let path = bound_path.trim();
    if path.is_empty() {
        return None;
    }
    if let Some(n) = nodes.iter().find(|n| n.kind == ProjectKind::Project && n.path == path) {
        return Some(n.id.clone());
    }
    let id = format!("bound-{}", nodes.len());
    nodes.push(ProjectNode {
        id: id.clone(),
        name: project_name_from_path(path),
        kind: ProjectKind::Project,
        path: path.to_string(),
        parent: None,
        open: false,
    });
    Some(id)
}

pub fn is_under_project(abs_path: &str, project_root: &str) -> bool {
    let a = abs_path.replace('\\', "/").trim_end_matches('/').to_string();
    let r = project_root.replace('\\', "/").trim_end_matches('/').to_string();
    if a.is_empty() || r.is_empty() {
        return false;
    }
    a == r || a.starts_with(&format!("{r}/"))
}

pub fn project_name_from_path(p: &str) -> String {
    p.replace('\\', "/")
        .split('/')
        .filter(|s| !s.is_empty())
        .next_back()
        .unwrap_or(p)
        .to_string()
}

pub fn host_cmd_leaves_project(cmd: &str, project_root: &str) -> bool {
    let root = project_root.trim();
    if root.is_empty() {
        return false;
    }
    for tok in cmd.split_whitespace() {
        if tok.starts_with('/') || tok.starts_with("~/") || tok.starts_with("$HOME") {
            let expanded = tok.replace('~', "").replace("$HOME", "");
            if tok.starts_with('/') && !is_under_project(tok, root) {
                return true;
            }
            if (tok.starts_with("~/") || tok.starts_with("$HOME")) && !expanded.is_empty() {
                return true;
            }
        }
    }
    false
}

pub fn host_hour_blocked(count: u32, cap: u32) -> bool {
    cap > 0 && count >= cap
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bound_tree_and_cap() {
        assert!(is_under_project("/home/j/proj/src", "/home/j/proj"));
        assert!(!is_under_project("/etc/passwd", "/home/j/proj"));
        assert_eq!(project_name_from_path("/home/j/GrokHub-Work"), "GrokHub-Work");
        assert!(host_cmd_leaves_project("cat /etc/passwd", "/home/j/proj"));
        assert!(!host_cmd_leaves_project("cat src/main.rs", "/home/j/proj"));
        assert!(!host_cmd_leaves_project("cat /home/j/proj/src/a.rs", "/home/j/proj"));
        assert!(host_cmd_leaves_project("cat ~/secrets", "/home/j/proj"));
        assert!(!host_cmd_leaves_project("ls", "/home/j/proj"));
        assert!(!host_cmd_leaves_project("cat /etc/passwd", ""));
        assert!(host_hour_blocked(40, 40));
        assert!(!host_hour_blocked(3, 40));
        assert!(!host_hour_blocked(40, 0), "cap 0 means unlimited");
    }

    #[test]
    fn create_rename_folder_and_add() {
        let mut nodes = Vec::new();
        assert_eq!(create_project(&mut nodes, "p1", "Night watch", None, "/home/j/GrokHub-Work").unwrap(), 0);
        assert_eq!(nodes[0].name, "Night watch");
        assert_eq!(nodes[0].kind, ProjectKind::Project);
        assert_eq!(nodes[0].path, "/home/j/GrokHub-Work/night-watch");
        assert!(nodes[0].parent.is_none());
        rename_node(&mut nodes, "p1", "Dawn").unwrap();
        assert_eq!(nodes[0].name, "Dawn");
        assert_eq!(create_folder(&mut nodes, "f1", "Cabin", None).unwrap(), 1);
        assert_eq!(nodes[1].kind, ProjectKind::Folder);
        add_to_folder(&mut nodes, "p1", Some("f1")).unwrap();
        assert_eq!(nodes[0].parent.as_deref(), Some("f1"));
        nodes[1].open = true;
        assert_eq!(visible_tree(&nodes), vec![(0, 1), (1, 0)]);
        add_to_folder(&mut nodes, "p1", None).unwrap();
        assert!(nodes[0].parent.is_none());
        assert!(create_folder(&mut nodes, "f2", "Nested", Some("f1")).is_err());
        assert!(add_to_folder(&mut nodes, "f1", Some("p1")).is_err());
        assert!(rename_node(&mut nodes, "p1", "   ").is_err());
        assert!(create_project(&mut nodes, "p2", "", None, "/home/j/GrokHub-Work").is_err());
        let folders = folder_choices(&nodes);
        assert_eq!(folders, vec![("f1".into(), "Cabin".into())]);
        assert!(toggle_folder(&mut nodes, "f1"));
        assert!(!nodes[1].open);
    }

    #[test]
    fn seed_and_upsert_bound() {
        assert!(seed_from_bound("").is_empty());
        let seeded = seed_from_bound("/home/j/GrokHub-Work");
        assert_eq!(seeded.len(), 1);
        assert_eq!(seeded[0].name, "GrokHub-Work");
        assert_eq!(seeded[0].path, "/home/j/GrokHub-Work");
        let mut nodes = seeded;
        let id = upsert_bound(&mut nodes, "/home/j/GrokHub-Work").unwrap();
        assert_eq!(nodes.len(), 1);
        assert_eq!(id, nodes[0].id);
        upsert_bound(&mut nodes, "/home/j/other").unwrap();
        assert_eq!(nodes.len(), 2);
        assert_eq!(project_slug("Night watch"), "night-watch");
        assert_eq!(clean_project_name("  Dawn  ").as_deref(), Some("Dawn"));
        assert!(clean_project_name("   ").is_none());
    }

    #[test]
    fn stage_rename_settles_path() {
        let mut nodes = Vec::new();
        assert_eq!(stage_project(&mut nodes, "p1", "Project", None).unwrap(), 0);
        assert_eq!(nodes[0].name, "Project");
        assert_eq!(nodes[0].path, "");
        rename_node(&mut nodes, "p1", "Night watch").unwrap();
        let path = settle_project_path(&mut nodes, "p1", "/home/j/GrokHub-Work").unwrap();
        assert_eq!(path, "/home/j/GrokHub-Work/night-watch");
        assert_eq!(nodes[0].path, path);
        assert_eq!(nodes[0].name, "Night watch");
        rename_node(&mut nodes, "p1", "Dawn").unwrap();
        let again = settle_project_path(&mut nodes, "p1", "/home/j/GrokHub-Work").unwrap();
        assert_eq!(again, "/home/j/GrokHub-Work/night-watch");
        assert_eq!(nodes[0].name, "Dawn");
        assert!(drop_node(&mut nodes, "p1"));
        assert!(nodes.is_empty());
    }

    #[test]
    fn orphans_still_show_and_folder_drop_unparents() {
        let mut nodes = Vec::new();
        create_folder(&mut nodes, "f1", "Cabin", None).unwrap();
        create_project(&mut nodes, "p1", "Night watch", Some("f1"), "/home/j/GrokHub-Work").unwrap();
        nodes[1].parent = Some("gone".into());
        assert_eq!(visible_tree(&nodes), vec![(0, 0), (0, 1)]);
        nodes[1].parent = Some("f1".into());
        assert!(drop_node(&mut nodes, "f1"));
        assert_eq!(nodes.len(), 1);
        assert!(nodes[0].parent.is_none());
        assert_eq!(visible_tree(&nodes), vec![(0, 0)]);
    }

    #[test]
    fn visible_tree_keeps_folder_then_root_order() {
        let mut nodes = Vec::new();
        create_folder(&mut nodes, "f1", "Cabin", None).unwrap();
        create_folder(&mut nodes, "f2", "Dawn", None).unwrap();
        create_project(&mut nodes, "p1", "Night", Some("f1"), "/w").unwrap();
        create_project(&mut nodes, "p2", "Root", None, "/w").unwrap();
        create_project(&mut nodes, "p3", "Late", Some("f2"), "/w").unwrap();
        nodes[0].open = true;
        nodes[1].open = false;
        assert_eq!(
            visible_tree(&nodes),
            vec![(0, 0), (1, 2), (0, 1), (0, 3)]
        );
        nodes[1].open = true;
        assert_eq!(
            visible_tree(&nodes),
            vec![(0, 0), (1, 2), (0, 1), (1, 4), (0, 3)]
        );
    }

    #[test]
    fn project_menu_can_rename_and_delete() {
        let proj = project_menu_acts(ProjectKind::Project);
        assert!(proj.contains(&ProjectMenuAct::Rename));
        assert!(proj.contains(&ProjectMenuAct::Delete));
        assert!(proj.contains(&ProjectMenuAct::AddToFolder));
        assert_eq!(project_menu_label(ProjectMenuAct::Delete), "Delete");
        let fold = project_menu_acts(ProjectKind::Folder);
        assert!(fold.contains(&ProjectMenuAct::Rename));
        assert!(fold.contains(&ProjectMenuAct::Delete));
        assert!(fold.contains(&ProjectMenuAct::NewHere));
        assert!(!fold.contains(&ProjectMenuAct::AddToFolder));
    }

    #[test]
    fn drop_selected_unbinds_the_bound_path() {
        let mut nodes = Vec::new();
        create_project(&mut nodes, "p1", "Night", None, "/w").unwrap();
        create_project(&mut nodes, "p2", "Keep", None, "/w").unwrap();
        let path = nodes[0].path.clone();
        let out = drop_selected(&mut nodes, "p1", &path);
        assert!(out.dropped);
        assert!(out.unbound);
        assert_eq!(out.name, "Night");
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].id, "p2");
        let out = drop_selected(&mut nodes, "p2", "/other");
        assert!(out.dropped);
        assert!(!out.unbound);
        assert!(nodes.is_empty());
    }

    #[test]
    fn empty_saved_sidebar_is_not_reseeded() {
        assert!(should_seed_sidebar(false, &[]));
        assert!(!should_seed_sidebar(true, &[]));
        let seeded = seed_from_bound("/home/j/GrokHub-Work");
        assert!(!should_seed_sidebar(false, &seeded));
        assert_eq!(
            restore_bound_path("/home/j/Dawn", "/home/j/GrokHub-Work", true),
            "/home/j/Dawn"
        );
        assert!(restore_bound_path("", "/home/j/GrokHub-Work", true).is_empty());
        assert_eq!(
            restore_bound_path("", "/home/j/GrokHub-Work", false),
            "/home/j/GrokHub-Work"
        );
    }
}
