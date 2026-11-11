use super::*;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "grokhub-code-files-{name}-{}-{}",
        std::process::id(),
        grokhub_core::now_ms()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn reads_and_saves_a_file_inside_the_workspace() {
    let ws = temp("rw");
    std::fs::write(ws.join("a.txt"), "one\n").unwrap();
    let got = read(&ws, "a.txt").unwrap();
    assert_eq!(got.text, "one\n");
    write(&ws, "a.txt", "two\n").unwrap();
    assert_eq!(std::fs::read_to_string(ws.join("a.txt")).unwrap(), "two\n");
    write(&ws, "new.rs", "fn main() {}\n").unwrap();
    assert_eq!(
        std::fs::read_to_string(ws.join("new.rs")).unwrap(),
        "fn main() {}\n"
    );
}

#[test]
fn refuses_paths_that_leave_the_workspace() {
    let ws = temp("escape");
    assert_eq!(read(&ws, "../x").unwrap_err(), OUTSIDE);
    assert_eq!(write(&ws, "../x", "no").unwrap_err(), OUTSIDE);
    assert_eq!(read(&ws, "/etc/hosts").unwrap_err(), OUTSIDE);
    assert_eq!(write(&ws, "sub/../../x", "no").unwrap_err(), OUTSIDE);
}

#[test]
fn refuses_anything_inside_dot_git() {
    let ws = temp("gitdir");
    std::fs::create_dir_all(ws.join(".git")).unwrap();
    std::fs::write(ws.join(".git/config"), "[core]\n").unwrap();
    assert_eq!(read(&ws, ".git/config").unwrap_err(), GIT_DIR);
    assert_eq!(write(&ws, ".git/hooks/pre-push", "x").unwrap_err(), GIT_DIR);
    assert_eq!(
        std::fs::read_to_string(ws.join(".git/config")).unwrap(),
        "[core]\n"
    );
}

#[cfg(unix)]
#[test]
fn refuses_a_symlink_that_points_outside() {
    let ws = temp("link");
    let outside = temp("link-outside");
    std::fs::write(outside.join("secret.txt"), "keep\n").unwrap();
    std::os::unix::fs::symlink(outside.join("secret.txt"), ws.join("l.txt")).unwrap();
    assert_eq!(read(&ws, "l.txt").unwrap_err(), OUTSIDE);
    assert_eq!(write(&ws, "l.txt", "changed").unwrap_err(), OUTSIDE);
    assert_eq!(
        std::fs::read_to_string(outside.join("secret.txt")).unwrap(),
        "keep\n"
    );
}

#[test]
fn refuses_binary_big_and_directory_paths() {
    let ws = temp("kinds");
    std::fs::write(ws.join("bin.dat"), [0xff, 0xfe, 0x00, 0x80]).unwrap();
    assert_eq!(read(&ws, "bin.dat").unwrap_err(), NOT_TEXT);
    std::fs::write(ws.join("big.txt"), vec![b'a'; MAX_BYTES + 1]).unwrap();
    assert_eq!(read(&ws, "big.txt").unwrap_err(), TOO_BIG);
    assert_eq!(
        write(&ws, "big2.txt", &"a".repeat(MAX_BYTES + 1)).unwrap_err(),
        TOO_BIG
    );
    std::fs::create_dir_all(ws.join("d")).unwrap();
    assert_eq!(read(&ws, "d").unwrap_err(), NOT_FILE);
    assert_eq!(write(&ws, "d", "x").unwrap_err(), NOT_FILE);
}

#[test]
fn lists_a_folder_dirs_first_without_dot_git() {
    let ws = temp("list");
    std::fs::create_dir_all(ws.join(".git")).unwrap();
    std::fs::create_dir_all(ws.join("src")).unwrap();
    std::fs::write(ws.join("b.txt"), "b").unwrap();
    std::fs::write(ws.join("a.txt"), "a").unwrap();
    std::fs::write(ws.join("src/main.rs"), "fn main() {}").unwrap();
    let top = list(&ws, "").unwrap();
    let names: Vec<(&str, &str, bool)> = top
        .entries
        .iter()
        .map(|e| (e.name.as_str(), e.path.as_str(), e.dir))
        .collect();
    assert_eq!(
        names,
        vec![
            ("src", "src", true),
            ("a.txt", "a.txt", false),
            ("b.txt", "b.txt", false)
        ]
    );
    assert!(!top.more);
    let sub = list(&ws, "src").unwrap();
    assert_eq!(sub.path, "src");
    assert_eq!(sub.entries.len(), 1);
    assert_eq!(sub.entries[0].path, "src/main.rs");
    assert_eq!(list(&ws, "../").unwrap_err(), OUTSIDE);
    assert_eq!(list(&ws, ".git").unwrap_err(), GIT_DIR);
    assert_eq!(list(&ws, "a.txt").unwrap_err(), "That path isn't a folder.");
}

#[cfg(unix)]
#[test]
fn a_listing_hides_symlinks_that_leave_the_workspace() {
    let ws = temp("list-link");
    let outside = temp("list-link-outside");
    std::fs::write(outside.join("x.txt"), "x").unwrap();
    std::fs::write(ws.join("in.txt"), "in").unwrap();
    std::os::unix::fs::symlink(outside.join("x.txt"), ws.join("out.txt")).unwrap();
    std::os::unix::fs::symlink(ws.join("in.txt"), ws.join("alias.txt")).unwrap();
    let names: Vec<String> = list(&ws, "")
        .unwrap()
        .entries
        .into_iter()
        .map(|e| e.name)
        .collect();
    assert_eq!(names, vec!["alias.txt".to_string(), "in.txt".to_string()]);
}

#[test]
fn a_save_is_logged_with_its_path_and_size_but_not_its_text() {
    let ws = temp("logged");
    let config = temp("logged-config");
    write_logged(&config, &ws, "notes.md", "hello world\n", 100).unwrap();
    assert_eq!(
        std::fs::read_to_string(ws.join("notes.md")).unwrap(),
        "hello world\n"
    );
    let rows = crate::activity::list(&config, 10);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].kind, crate::activity::Kind::Edit);
    assert_eq!(rows[0].summary, "Saved notes.md (12 bytes).");
    let raw = std::fs::read_to_string(config.join(crate::activity::FILE)).unwrap();
    assert!(
        !raw.contains("hello world"),
        "the file's text reached the log: {raw}"
    );
    assert_eq!(
        write_logged(&config, &ws, "../x", "no", 100).unwrap_err(),
        OUTSIDE
    );
    assert_eq!(crate::activity::list(&config, 10).len(), 1);
}
