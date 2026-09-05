//! Freedesktop desktop-entry rewrite so a user-prefix install does not depend
//! on the GUI session inheriting `~/.local/bin` on PATH.

/// `{prefix}/bin/grokhub` with no trailing slash on prefix.
pub fn desktop_bin_path(prefix: &str) -> String {
    let prefix = prefix.trim_end_matches('/');
    format!("{prefix}/bin/grokhub")
}

/// Rewrite `Exec=` / `TryExec=` from the in-tree template to the prefix binary.
/// PATH is unused on purpose: the menu launcher must not need `~/.local/bin`.
pub fn rewrite_desktop_entry(template: &str, prefix: &str) -> String {
    let bin = desktop_bin_path(prefix);
    let mut out = String::with_capacity(template.len() + bin.len());
    for line in template.lines() {
        if line == "Exec=grokhub" {
            out.push_str("Exec=");
            out.push_str(&bin);
        } else if line == "TryExec=grokhub" {
            out.push_str("TryExec=");
            out.push_str(&bin);
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out
}

/// True when Exec/TryExec name the prefix binary, not a PATH-relative `grokhub`.
pub fn desktop_entry_uses_prefix_bin(entry: &str, prefix: &str) -> bool {
    let bin = desktop_bin_path(prefix);
    let exec = format!("Exec={bin}");
    let try_exec = format!("TryExec={bin}");
    entry.lines().any(|l| l == exec) && entry.lines().any(|l| l == try_exec)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEMPLATE: &str = include_str!("../../../packaging/grokhub.desktop");

    #[test]
    fn user_prefix_exec_does_not_need_local_bin_on_path() {
        assert!(
            TEMPLATE.lines().any(|l| l == "Exec=grokhub"),
            "in-tree template stays PATH-relative; install rewrites it: {TEMPLATE}"
        );
        let prefix = "/home/cabin/.local";
        let entry = rewrite_desktop_entry(TEMPLATE, prefix);
        let bin = desktop_bin_path(prefix);
        assert_eq!(bin, "/home/cabin/.local/bin/grokhub");
        assert!(
            desktop_entry_uses_prefix_bin(&entry, prefix),
            "menu Exec must be the prefix binary: {entry}"
        );
        assert!(
            !entry.lines().any(|l| l == "Exec=grokhub" || l == "TryExec=grokhub"),
            "PATH-relative grokhub must not remain after rewrite: {entry}"
        );
        // A session PATH that lacks ~/.local/bin still has an absolute Exec.
        let path_without_prefix = "/usr/bin:/bin";
        assert!(
            !path_without_prefix.split(':').any(|p| p == "/home/cabin/.local/bin"),
            "fixture PATH must omit the prefix bin dir"
        );
        assert!(
            entry.contains(&format!("Exec={bin}")),
            "Exec stays absolute when PATH is {path_without_prefix}: {entry}"
        );
    }

    #[test]
    fn install_sh_rewrites_exec_and_refreshes_the_desktop_database() {
        let sh = include_str!("../../../scripts/install.sh");
        assert!(
            sh.contains("rewrite_desktop_entry")
                || (sh.contains("Exec=") && sh.contains("$PREFIX/bin/grokhub")),
            "install.sh must write Exec=$PREFIX/bin/grokhub: {sh}"
        );
        assert!(
            sh.contains("update-desktop-database"),
            "install.sh must refresh the desktop database when the tool exists: {sh}"
        );
        assert!(
            !sh.contains("pgrep grokhub"),
            "overlay must never pgrep grokhub (self-match): {sh}"
        );
        let sync = include_str!("../../../scripts/sync-user-integration.sh");
        assert!(
            sync.contains("update-desktop-database"),
            "sync-user-integration.sh already refreshed the database: {sync}"
        );
        assert!(
            sync.contains("$PREFIX/bin/grokhub") && (sync.contains("Exec=") || sync.contains("rewrite")),
            "sync-user-integration.sh must also write a prefix Exec: {sync}"
        );
    }
}
