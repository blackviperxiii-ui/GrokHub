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

/// The `root` a read of this project folder returns.
fn root_of(ws: &Path) -> String {
    ws.canonicalize().unwrap().to_string_lossy().into_owned()
}

#[test]
fn reads_and_saves_a_file_inside_the_workspace() {
    let ws = temp("rw");
    std::fs::write(ws.join("a.txt"), "one\n").unwrap();
    let got = read(&ws, "a.txt").unwrap();
    assert_eq!(got.text, "one\n");
    assert_eq!(got.root, root_of(&ws));
    let saved = write(&ws, "a.txt", "two\n", &got.root).unwrap();
    assert_eq!(saved.root, root_of(&ws));
    assert_eq!(std::fs::read_to_string(ws.join("a.txt")).unwrap(), "two\n");
    write(&ws, "new.rs", "fn main() {}\n", &got.root).unwrap();
    assert_eq!(
        std::fs::read_to_string(ws.join("new.rs")).unwrap(),
        "fn main() {}\n"
    );
}

#[test]
fn refuses_paths_that_leave_the_workspace() {
    let ws = temp("escape");
    assert_eq!(read(&ws, "../x").unwrap_err(), OUTSIDE);
    assert_eq!(
        write(&ws, "../x", "no", &root_of(&ws)).unwrap_err(),
        OUTSIDE
    );
    assert_eq!(read(&ws, "/etc/hosts").unwrap_err(), OUTSIDE);
    assert_eq!(
        write(&ws, "sub/../../x", "no", &root_of(&ws)).unwrap_err(),
        OUTSIDE
    );
}

#[test]
fn refuses_anything_inside_dot_git() {
    let ws = temp("gitdir");
    std::fs::create_dir_all(ws.join(".git")).unwrap();
    std::fs::write(ws.join(".git/config"), "[core]\n").unwrap();
    assert_eq!(read(&ws, ".git/config").unwrap_err(), GIT_DIR);
    assert_eq!(
        write(&ws, ".git/hooks/pre-push", "x", &root_of(&ws)).unwrap_err(),
        GIT_DIR
    );
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
    assert_eq!(
        write(&ws, "l.txt", "changed", &root_of(&ws)).unwrap_err(),
        OUTSIDE
    );
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
        write(&ws, "big2.txt", &"a".repeat(MAX_BYTES + 1), &root_of(&ws)).unwrap_err(),
        TOO_BIG
    );
    std::fs::create_dir_all(ws.join("d")).unwrap();
    assert_eq!(read(&ws, "d").unwrap_err(), NOT_FILE);
    assert_eq!(write(&ws, "d", "x", &root_of(&ws)).unwrap_err(), NOT_FILE);
}

#[cfg(unix)]
#[test]
fn refuses_a_dangling_symlink_out_and_into_dot_git() {
    let ws = temp("dangling");
    let outside = temp("dangling-outside");
    std::fs::create_dir_all(ws.join(".git")).unwrap();
    std::os::unix::fs::symlink(outside.join("new.txt"), ws.join("l.txt")).unwrap();
    assert_eq!(
        write(&ws, "l.txt", "x", &root_of(&ws)).unwrap_err(),
        OUTSIDE
    );
    assert!(!outside.join("new.txt").exists());
    std::os::unix::fs::symlink(".git/commondir", ws.join("build-dir")).unwrap();
    assert_eq!(
        write(&ws, "build-dir", "x", &root_of(&ws)).unwrap_err(),
        OUTSIDE
    );
    assert!(!ws.join(".git/commondir").exists());
    // A link to a name inside the project that isn't there yet is refused too.
    std::os::unix::fs::symlink("later.txt", ws.join("alias.txt")).unwrap();
    assert_eq!(
        write(&ws, "alias.txt", "x", &root_of(&ws)).unwrap_err(),
        OUTSIDE
    );
    assert!(!ws.join("later.txt").exists());
}

#[test]
fn a_save_is_refused_when_the_project_folder_changed() {
    let a = temp("root-a");
    let b = temp("root-b");
    let config = temp("root-config");
    std::fs::write(a.join("README.md"), "# Project A\n").unwrap();
    std::fs::write(b.join("README.md"), "# Project B\n").unwrap();
    let got = read(&a, "README.md").unwrap();
    assert_eq!(
        serde_json::to_value(&got).unwrap()["root"],
        serde_json::Value::String(root_of(&a))
    );
    assert_eq!(
        write(&b, "README.md", "A text", &got.root).unwrap_err(),
        "The project folder changed since this file was opened. Nothing was saved."
    );
    // A name the other project doesn't have isn't created there either.
    assert_eq!(
        write(&b, "only-in-a.md", "A text", &got.root).unwrap_err(),
        ROOT_CHANGED
    );
    assert_eq!(
        write_logged(&config, &b, "README.md", "A text", &got.root, 100).unwrap_err(),
        ROOT_CHANGED
    );
    assert_eq!(
        write(&b, "README.md", "A text", "").unwrap_err(),
        ROOT_CHANGED
    );
    assert_eq!(
        std::fs::read_to_string(b.join("README.md")).unwrap(),
        "# Project B\n"
    );
    assert!(!b.join("only-in-a.md").exists());
    assert!(crate::activity::list(&config, 10).is_empty());
    // The same file still saves into the project it was opened in.
    write(&a, "README.md", "A text", &got.root).unwrap();
    assert_eq!(
        std::fs::read_to_string(a.join("README.md")).unwrap(),
        "A text"
    );
}

/// Hands out `left` bytes of `a` and counts them.
struct Counted {
    left: usize,
    given: usize,
}

impl std::io::Read for Counted {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = buf.len().min(self.left);
        buf[..n].fill(b'a');
        self.left -= n;
        self.given += n;
        Ok(n)
    }
}

#[test]
fn a_read_stops_one_byte_past_the_size_limit() {
    let mut grown = Counted {
        left: 4 * MAX_BYTES,
        given: 0,
    };
    assert_eq!(read_capped(&mut grown).unwrap_err(), TOO_BIG);
    assert_eq!(grown.given, MAX_BYTES + 1);
    let mut full = Counted {
        left: MAX_BYTES,
        given: 0,
    };
    assert_eq!(read_capped(&mut full).unwrap().len(), 524_288);
    assert_eq!(read_capped(std::io::repeat(b'a')).unwrap_err(), TOO_BIG);
}

#[cfg(unix)]
#[test]
fn refuses_to_save_into_a_named_pipe() {
    let ws = temp("pipe");
    let made = std::process::Command::new("mkfifo")
        .arg(ws.join("pipe"))
        .status()
        .unwrap();
    assert!(made.success());
    let (tx, rx) = std::sync::mpsc::channel();
    let dir = ws.clone();
    std::thread::spawn(move || {
        let opened = read(&dir, "pipe").map(|f| f.text);
        let saved = write(&dir, "pipe", "x", &root_of(&dir)).map(|f| f.text);
        let _ = tx.send((opened, saved));
    });
    // Nothing reads the pipe, so a save that opens it never comes back.
    let got = rx.recv_timeout(std::time::Duration::from_secs(2));
    assert_eq!(got, Ok((Err(NOT_FILE.into()), Err(NOT_FILE.into()))));
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

#[test]
fn a_long_listing_keeps_the_first_names_in_order() {
    let ws = temp("long");
    std::fs::create_dir_all(ws.join("many/zz-dir")).unwrap();
    for n in 0..1500 {
        std::fs::write(ws.join(format!("many/f{n:04}")), "").unwrap();
    }
    let got = list(&ws, "many").unwrap();
    assert_eq!(got.entries.len(), 1000);
    assert!(got.more);
    assert_eq!(got.entries[0].name, "zz-dir");
    assert_eq!(got.entries[0].path, "many/zz-dir");
    assert!(got.entries[0].dir);
    let files: Vec<&str> = got.entries[1..].iter().map(|e| e.name.as_str()).collect();
    let want: Vec<String> = (0..999).map(|n| format!("f{n:04}")).collect();
    assert_eq!(files, want);
    assert!(got.entries[1..].iter().all(|e| !e.dir));
    // Exactly the limit is the whole folder, with nothing more to say.
    std::fs::create_dir_all(ws.join("full")).unwrap();
    for n in 0..1000 {
        std::fs::write(ws.join(format!("full/f{n:04}")), "").unwrap();
    }
    let full = list(&ws, "full").unwrap();
    assert_eq!(full.entries.len(), 1000);
    assert!(!full.more);
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
    write_logged(
        &config,
        &ws,
        "notes.md",
        "hello world\n",
        &root_of(&ws),
        100,
    )
    .unwrap();
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
        write_logged(&config, &ws, "../x", "no", &root_of(&ws), 100).unwrap_err(),
        OUTSIDE
    );
    assert_eq!(crate::activity::list(&config, 10).len(), 1);
}
