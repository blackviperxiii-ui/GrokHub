//! Bound project = world. Unbound stays the full desktop.

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
        assert!(host_hour_blocked(40, 40));
        assert!(!host_hour_blocked(3, 40));
    }
}
