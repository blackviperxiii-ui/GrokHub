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
    let home = std::env::var("HOME").ok();
    if nodes
        .iter()
        .any(|n| bound_paths_match(&n.path, &path, home.as_deref()))
    {
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
    let home = std::env::var("HOME").ok();
    if nodes
        .iter()
        .any(|n| n.id != id && bound_paths_match(&n.path, &path, home.as_deref()))
    {
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

pub fn bound_paths_match(a: &str, b: &str, home: Option<&str>) -> bool {
    let a = expand_project_root(a, home);
    let b = expand_project_root(b, home);
    !a.is_empty() && a == b
}

pub fn drop_selected(nodes: &mut Vec<ProjectNode>, id: &str, bound_path: &str) -> DropOutcome {
    drop_selected_in(
        nodes,
        id,
        bound_path,
        std::env::var("HOME").ok().as_deref(),
    )
}

pub fn drop_selected_in(
    nodes: &mut Vec<ProjectNode>,
    id: &str,
    bound_path: &str,
    home: Option<&str>,
) -> DropOutcome {
    let node = nodes.iter().find(|n| n.id == id);
    let name = node.map(|n| n.name.clone()).unwrap_or_default();
    let path = node.map(|n| n.path.clone()).unwrap_or_default();
    let dropped = drop_node(nodes, id);
    let unbound = dropped && bound_paths_match(&path, bound_path, home);
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

/// ACP session cwd. Bound project if set, else `work_root` (`~/GrokHub-Work`).
/// Never the cabin process cwd — that is the overlay install or a cargo `target/` tree.
pub fn resolve_acp_cwd(project_dir: &str, home: Option<&str>, work_root: &str) -> String {
    let bound = expand_project_root(project_dir, home);
    if !bound.trim().is_empty() {
        return bound;
    }
    let work = expand_project_root(work_root, home);
    if !work.trim().is_empty() {
        return work;
    }
    match home.filter(|h| !h.is_empty()) {
        Some(h) => format!("{}/GrokHub-Work", h.trim_end_matches('/')),
        None => "GrokHub-Work".into(),
    }
}

/// `/project bind .` is the bound tree or work root — never the cabin process cwd.
pub fn resolve_bind_path(raw: &str, bound: &str, work_root: &str, home: Option<&str>) -> String {
    let raw = raw.trim();
    let bound_abs = expand_project_root(bound, home);
    let work_abs = resolve_acp_cwd("", home, work_root);
    let base = if !bound_abs.trim().is_empty() {
        bound_abs
    } else {
        work_abs
    };
    if raw.is_empty() || raw == "." || raw == "./" {
        return normalize_host_path(&base);
    }
    let expanded = expand_project_root(raw, home);
    if expanded.starts_with('/') {
        return normalize_host_path(&expanded);
    }
    let rest = raw.strip_prefix("./").unwrap_or(raw);
    if base.is_empty() {
        return normalize_host_path(rest);
    }
    normalize_host_path(&format!("{}/{}", base.trim_end_matches('/'), rest))
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
    upsert_bound_in(
        nodes,
        bound_path,
        std::env::var("HOME").ok().as_deref(),
    )
}

pub fn upsert_bound_in(
    nodes: &mut Vec<ProjectNode>,
    bound_path: &str,
    home: Option<&str>,
) -> Option<String> {
    let path = bound_path.trim();
    if path.is_empty() {
        return None;
    }
    if let Some(n) = nodes
        .iter()
        .find(|n| n.kind == ProjectKind::Project && bound_paths_match(&n.path, path, home))
    {
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

pub fn expand_host_path_token(tok: &str) -> Option<String> {
    expand_host_path_token_in(tok, std::env::var("HOME").ok().as_deref())
}

fn peel_host_path_token(tok: &str) -> String {
    let t = tok.trim_matches(|c| matches!(c, '"' | '\'' | '`'));
    if let Some((_, v)) = t.split_once('=') {
        let v = v.trim_matches(|c| matches!(c, '"' | '\'' | '`'));
        if v.starts_with('/') || v.starts_with("~/") || v.starts_with("$HOME") || v.contains('/') {
            return v.to_string();
        }
    }
    t.to_string()
}

pub fn expand_project_root(root: &str, home: Option<&str>) -> String {
    let root = root.trim();
    expand_host_path_token_in(root, home).unwrap_or_else(|| root.to_string())
}

pub fn expand_host_path_token_in(tok: &str, home: Option<&str>) -> Option<String> {
    let tok = peel_host_path_token(tok);
    if tok.starts_with('/') {
        return Some(tok);
    }
    if tok == "$OLDPWD" || tok.starts_with("$OLDPWD/") {
        return Some("/var/empty".into());
    }
    let home = home.filter(|h| !h.is_empty())?;
    let home = home.trim_end_matches('/');
    if let Some(rest) = tok.strip_prefix("~/") {
        return Some(format!("{home}/{rest}"));
    }
    if let Some(rest) = tok.strip_prefix("$HOME/") {
        return Some(format!("{home}/{rest}"));
    }
    if tok == "~" || tok == "$HOME" {
        return Some(home.to_string());
    }
    if let Some(rest) = tok.strip_prefix('~') {
        if !rest.is_empty() && !rest.starts_with('/') {
            return Some(format!("/home/{rest}"));
        }
    }
    None
}

pub fn normalize_host_path(p: &str) -> String {
    let slash = p.replace('\\', "/");
    let abs = slash.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for seg in slash.split('/') {
        if seg.is_empty() || seg == "." {
            continue;
        }
        if seg == ".." {
            parts.pop();
            continue;
        }
        parts.push(seg);
    }
    if abs {
        format!("/{}", parts.join("/"))
    } else {
        parts.join("/")
    }
}

fn looks_like_host_path(tok: &str) -> bool {
    tok.contains('/') || tok == ".." || tok.starts_with('.')
}

pub fn host_cmd_leaves_project(cmd: &str, project_root: &str) -> bool {
    host_cmd_leaves_project_in(cmd, project_root, std::env::var("HOME").ok().as_deref())
}

fn host_cmd_name(tok: &str) -> &str {
    tok.trim_start_matches('\\')
}

fn host_cd_argv<'a>(bits: &'a [&'a str]) -> Option<&'a [&'a str]> {
    let mut i = 0;
    while matches!(
        bits.get(i).copied().map(host_cmd_name),
        Some("builtin") | Some("command") | Some("exec") | Some("eval")
    ) {
        i += 1;
        while i < bits.len() && (bits[i] == "--" || bits[i].starts_with('-')) {
            i += 1;
        }
    }
    if bits.get(i).copied().map(host_cmd_name) != Some("cd") {
        return None;
    }
    Some(&bits[i + 1..])
}

fn host_cd_dest_leaves(cmd: &str, root: &str, home: Option<&str>) -> bool {
    for seg in cmd.split(|c: char| matches!(c, '&' | '|' | ';')) {
        let bits: Vec<&str> = seg.split_whitespace().collect();
        let Some(after_cd) = host_cd_argv(&bits) else {
            continue;
        };
        let dest = after_cd
            .iter()
            .find(|w| **w != "--" && !w.starts_with('-'))
            .copied();
        let Some(dest) = dest else {
            return true;
        };
        let peeled = peel_host_path_token(dest);
        let path = if let Some(p) = expand_host_path_token_in(dest, home) {
            normalize_host_path(&p)
        } else if !peeled.is_empty() {
            normalize_host_path(&format!("{}/{peeled}", root.trim_end_matches('/')))
        } else {
            return true;
        };
        if !is_under_project(&path, root) {
            return true;
        }
    }
    false
}

pub fn host_cmd_leaves_project_in(cmd: &str, project_root: &str, home: Option<&str>) -> bool {
    let root = expand_project_root(project_root, home);
    if root.is_empty() {
        return false;
    }
    if host_cd_dest_leaves(cmd, &root, home) {
        return true;
    }
    for tok in cmd.split_whitespace() {
        let peeled = peel_host_path_token(tok);
        let path = if let Some(p) = expand_host_path_token_in(tok, home) {
            normalize_host_path(&p)
        } else if looks_like_host_path(&peeled) {
            normalize_host_path(&format!("{}/{peeled}", root.trim_end_matches('/')))
        } else {
            continue;
        };
        if !is_under_project(&path, &root) {
            return true;
        }
    }
    false
}

pub fn host_hour_blocked(count: u32, cap: u32) -> bool {
    cap > 0 && count >= cap
}

/// Halt refunds the reserved slots so a cancelled job does not eat the hour cap.
pub fn refund_host_reserved(count: u32, reserved: u32) -> u32 {
    count.saturating_sub(reserved)
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
        assert!(host_cmd_leaves_project_in(
            "cat ~/secrets",
            "/home/j/proj",
            Some("/home/j")
        ));
        assert!(
            !host_cmd_leaves_project_in(
                "cat ~/proj/src/main.rs",
                "/home/j/proj",
                Some("/home/j")
            ),
            "tilde inside the bound tree is still in-world"
        );
        assert!(
            !host_cmd_leaves_project_in(
                "cat $HOME/proj/src/a.rs",
                "/home/j/proj",
                Some("/home/j")
            )
        );
        assert!(!host_cmd_leaves_project("ls", "/home/j/proj"));
        assert!(!host_cmd_leaves_project("cat /etc/passwd", ""));
        assert!(
            host_cmd_leaves_project_in("cat ../outside.txt", "/home/j/proj", Some("/home/j")),
            "relative traversal must leave the bound tree"
        );
        assert!(!host_cmd_leaves_project_in(
            "cat src/main.rs",
            "/home/j/proj",
            None
        ));
        assert!(host_cmd_leaves_project_in("cd ..", "/home/j/proj", None));
        assert!(
            host_cmd_leaves_project_in(
                "grep foo --file=/etc/passwd",
                "/home/j/proj",
                Some("/home/j")
            ),
            "flag-style absolute paths leave the bound tree"
        );
        assert!(
            host_cmd_leaves_project_in("cat \"/etc/passwd\"", "/home/j/proj", Some("/home/j")),
            "quoted absolute paths leave the bound tree"
        );
        assert!(
            !host_cmd_leaves_project_in(
                "cat src/main.rs",
                "~/proj",
                Some("/home/j")
            ),
            "tilde bound roots still treat in-tree paths as inside"
        );
        assert!(host_hour_blocked(40, 40));
        assert!(!host_hour_blocked(3, 40));
        assert!(!host_hour_blocked(40, 0), "cap 0 means unlimited");
        assert_eq!(refund_host_reserved(3, 3), 0);
        assert_eq!(refund_host_reserved(3, 1), 2);
        assert_eq!(refund_host_reserved(0, 2), 0);
        assert!(
            host_cmd_leaves_project_in(
                "cp -a '/home/j/proj/.' '/home/j/.config/GrokHub/rewind/rw1'",
                "/home/j/proj",
                Some("/home/j")
            ),
            "cabin rewind dest is outside the bound tree — run_cmds must exempt it"
        );
    }

    #[test]
    fn bare_cd_leaves_the_bound_tree() {
        assert!(
            host_cmd_leaves_project_in("cd", "/home/j/proj", Some("/home/j")),
            "cd with no dest goes to HOME"
        );
        assert!(
            host_cmd_leaves_project_in("cd && ls", "/home/j/proj", Some("/home/j")),
            "cd && ls lists HOME, not the bound tree"
        );
        assert!(
            !host_cmd_leaves_project_in("cd src && ls", "/home/j/proj", Some("/home/j")),
            "cd into a project subdir stays in-world"
        );
        assert!(
            !host_cmd_leaves_project_in("echo cd", "/home/j/proj", Some("/home/j")),
            "the word cd in another command is not a directory change"
        );
    }

    #[test]
    fn wrapped_cd_without_dest_leaves_the_bound_tree() {
        assert!(
            host_cmd_leaves_project_in("builtin cd", "/home/j/proj", Some("/home/j")),
            "builtin cd with no dest goes to HOME"
        );
        assert!(
            host_cmd_leaves_project_in("command cd && ls", "/home/j/proj", Some("/home/j")),
            "command cd && ls lists HOME, not the bound tree"
        );
        assert!(
            host_cmd_leaves_project_in("builtin -- cd", "/home/j/proj", Some("/home/j")),
            "builtin -- cd with no dest goes to HOME"
        );
        assert!(
            !host_cmd_leaves_project_in("builtin cd src && ls", "/home/j/proj", Some("/home/j")),
            "builtin cd into a project subdir stays in-world"
        );
        assert!(
            !host_cmd_leaves_project_in("command echo cd", "/home/j/proj", Some("/home/j")),
            "command echo cd is not a directory change"
        );
        assert!(
            host_cmd_leaves_project_in("exec cd", "/home/j/proj", Some("/home/j")),
            "exec cd with no dest goes to HOME"
        );
        assert!(
            host_cmd_leaves_project_in("\\cd", "/home/j/proj", Some("/home/j")),
            "backslash cd skips aliases and still goes to HOME"
        );
        assert!(
            !host_cmd_leaves_project_in("exec cd src", "/home/j/proj", Some("/home/j")),
            "exec cd into a project subdir stays in-world"
        );
        assert!(
            host_cmd_leaves_project_in("eval cd", "/home/j/proj", Some("/home/j")),
            "eval cd with no dest goes to HOME"
        );
        assert!(
            host_cmd_leaves_project_in("eval builtin cd && ls", "/home/j/proj", Some("/home/j")),
            "eval builtin cd && ls lists HOME, not the bound tree"
        );
        assert!(
            host_cmd_leaves_project_in("cd ~other", "/home/j/proj", Some("/home/j")),
            "cd ~other goes to that user's home, not a project subdir named ~other"
        );
        assert!(
            host_cmd_leaves_project_in("cat ~other/secrets", "/home/j/proj", Some("/home/j")),
            "cat ~other/secrets reads outside the bound tree"
        );
        assert!(
            host_cmd_leaves_project_in("cd $OLDPWD", "/home/j/proj", Some("/home/j")),
            "cd $OLDPWD goes to the previous directory, not a project subdir"
        );
        assert!(
            host_cmd_leaves_project_in("cat $OLDPWD/secrets", "/home/j/proj", Some("/home/j")),
            "cat $OLDPWD/secrets reads outside the bound tree"
        );
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
    fn create_project_does_not_reuse_tilde_tree() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/home/j".into());
        let work = format!("{home}/GrokHub-Work");
        let mut nodes = vec![ProjectNode {
            id: "old".into(),
            name: "Night watch".into(),
            kind: ProjectKind::Project,
            path: "~/GrokHub-Work/night-watch".into(),
            parent: None,
            open: false,
        }];
        create_project(&mut nodes, "p2", "Night watch", None, &work).unwrap();
        assert_eq!(nodes.len(), 2);
        assert!(
            !bound_paths_match(&nodes[0].path, &nodes[1].path, Some(&home)),
            "a second Night watch must not reuse the tilde-bound tree: {} vs {}",
            nodes[0].path,
            nodes[1].path
        );
    }

    #[test]
    fn drop_selected_unbinds_tilde_bound_path() {
        let mut nodes = vec![ProjectNode {
            id: "p1".into(),
            name: "proj".into(),
            kind: ProjectKind::Project,
            path: "~/work/proj".into(),
            parent: None,
            open: false,
        }];
        let out = drop_selected_in(&mut nodes, "p1", "/home/j/work/proj", Some("/home/j"));
        assert!(out.dropped);
        assert!(
            out.unbound,
            "tilde sidebar path must match the expanded bound dir"
        );
        let mut nodes = vec![ProjectNode {
            id: "p1".into(),
            name: "proj".into(),
            kind: ProjectKind::Project,
            path: "$HOME/work/proj".into(),
            parent: None,
            open: false,
        }];
        let out = drop_selected_in(&mut nodes, "p1", "/home/j/work/proj", Some("/home/j"));
        assert!(out.unbound, "$HOME sidebar path must match the expanded bound dir");
    }

    #[test]
    fn upsert_bound_finds_tilde_sidebar_row() {
        let mut nodes = vec![ProjectNode {
            id: "p1".into(),
            name: "proj".into(),
            kind: ProjectKind::Project,
            path: "~/work/proj".into(),
            parent: None,
            open: false,
        }];
        let id = upsert_bound_in(&mut nodes, "/home/j/work/proj", Some("/home/j"));
        assert_eq!(id.as_deref(), Some("p1"));
        assert_eq!(nodes.len(), 1, "must not add a second row for the same tree");
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

    #[test]
    fn acp_cwd_is_the_bound_tree_or_work_root() {
        assert_eq!(
            resolve_acp_cwd("/home/j/Dawn", Some("/home/j"), "/home/j/GrokHub-Work"),
            "/home/j/Dawn"
        );
        assert_eq!(
            resolve_acp_cwd("~/Dawn", Some("/home/j"), "/home/j/GrokHub-Work"),
            "/home/j/Dawn"
        );
        assert_eq!(
            resolve_acp_cwd("", Some("/home/j"), "/home/j/GrokHub-Work"),
            "/home/j/GrokHub-Work"
        );
        assert_eq!(
            resolve_acp_cwd("   ", Some("/home/j"), "~/GrokHub-Work"),
            "/home/j/GrokHub-Work"
        );
        assert_eq!(
            resolve_acp_cwd("", Some("/home/j"), ""),
            "/home/j/GrokHub-Work"
        );
        assert_ne!(
            resolve_acp_cwd("", Some("/home/j"), "/home/j/GrokHub-Work"),
            "/home/j",
            "unbound ACP must not sit in $HOME"
        );
        assert_eq!(
            resolve_bind_path(".", "", "/home/j/GrokHub-Work", Some("/home/j")),
            "/home/j/GrokHub-Work"
        );
        assert_eq!(
            resolve_bind_path(".", "/home/j/Dawn", "/home/j/GrokHub-Work", Some("/home/j")),
            "/home/j/Dawn"
        );
        assert_eq!(
            resolve_bind_path("/tmp/cabin", "", "/home/j/GrokHub-Work", Some("/home/j")),
            "/tmp/cabin"
        );
        assert_eq!(
            resolve_bind_path("~/Dawn", "", "/home/j/GrokHub-Work", Some("/home/j")),
            "/home/j/Dawn"
        );
        assert_eq!(
            resolve_bind_path("src", "/home/j/Dawn", "/home/j/GrokHub-Work", Some("/home/j")),
            "/home/j/Dawn/src"
        );
        assert_ne!(
            resolve_bind_path(".", "", "/home/j/GrokHub-Work", Some("/home/j")),
            ".",
            "/project bind . must not inherit the cabin process cwd"
        );
    }
}
