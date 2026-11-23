use super::*;

struct Dir(PathBuf);

impl std::ops::Deref for Dir {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn dir(label: &str) -> Dir {
    let root = std::env::temp_dir().join(format!(
        "grokhub-storage-{label}-{}-{}",
        std::process::id(),
        grokhub_core::now_ms()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("config")).expect("config");
    std::fs::create_dir_all(root.join("home")).expect("home");
    Dir(root)
}

#[test]
fn the_view_names_each_folder_and_the_models() {
    let d = dir("view");
    let (config, home) = (d.join("config"), d.join("home"));
    let picked = d.join("files");
    assert_eq!(
        change(&config, &picked),
        format!(
            "New files go to {}. Files already saved stay where they are.",
            picked.display()
        )
    );
    let v = view(&config, Some(&home), None);
    assert_eq!(
        v.files_line,
        format!(
            "Your files: {}. Exports, recordings, images and reports go here.",
            picked.display()
        )
    );
    assert!(v.can_reset);
    let lines: Vec<&str> = v.folders.iter().map(|f| f.line.as_str()).collect();
    assert_eq!(
        lines,
        [
            "Chats: Exported chats",
            "Diagnostics: Diagnostics for support",
            "Recordings: Screen recordings",
            "Images: Saved images",
            "Reports: Internet log and week recap images",
        ]
    );
    assert_eq!(
        v.models.line,
        format!(
            "Local models: {}",
            home.join("GrokHub").join("models").display()
        )
    );
    assert_eq!(v.waiting, None);
    assert_eq!(v.moves, Vec::<String>::new());
}

#[test]
fn a_change_refuses_a_relative_path_the_app_folder_and_a_read_only_one() {
    let d = dir("refuse");
    let config = d.join("config");
    assert_eq!(
        change(&config, Path::new("notes")),
        "Can't use that folder: pick a full folder path."
    );
    assert_eq!(
        change(&config, &config.join("inside")),
        "Can't use that folder: that folder is inside the app's own data folder."
    );
    let locked = d.join("locked");
    std::fs::create_dir_all(&locked).expect("locked");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).expect("chmod");
        // An administrator can write anywhere; the check only holds for a person.
        if std::fs::write(locked.join("probe"), b"x").is_err() {
            assert!(
                change(&config, &locked)
                    .starts_with("Can't use that folder: can't save files there ("),
                "{}",
                change(&config, &locked)
            );
        }
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }
    assert!(!view(&config, None, None).can_reset);
}

#[test]
fn reset_goes_back_to_the_default() {
    let d = dir("reset");
    let config = d.join("config");
    change(&config, &d.join("files"));
    let default = user_files::default_files_root(&config);
    assert_eq!(
        reset(&config),
        format!("New files go to {} again.", default.display())
    );
    assert!(!view(&config, None, None).can_reset);
}

#[test]
fn older_files_are_counted_named_and_brought_over() {
    let d = dir("bring");
    let (config, home) = (d.join("config"), d.join("home"));
    change(&config, &d.join("files"));
    let imagine = home.join("GrokHub-Work").join("imagine");
    std::fs::create_dir_all(&imagine).expect("imagine");
    std::fs::write(imagine.join("a-cat.png"), b"png").expect("cat");
    std::fs::write(imagine.join("a-dog.png"), b"png").expect("dog");
    let v = view(&config, Some(&home), None);
    assert_eq!(
        v.waiting.as_deref(),
        Some("2 older items are still in the folders earlier versions used. Bringing them over moves them on the same drive, or copies them and keeps the originals on another drive.")
    );
    assert_eq!(
        v.moves,
        [format!(
            "{} → {}",
            imagine.display(),
            d.join("files").join("Images").display()
        )]
    );
    let done = bring_all_over(&config, &older_spots(Some(&home), None));
    assert_eq!(done.summary(), "Moved 2 items.");
    assert!(d.join("files/Images/a-cat.png").is_file());
    assert_eq!(view(&config, Some(&home), None).waiting, None);
}

#[test]
fn a_change_adds_the_old_folder_to_the_spots() {
    let old = Path::new("/old/files");
    let spots = older_spots(None, Some(old));
    let names: Vec<String> = spots.iter().map(|(p, _)| p.display().to_string()).collect();
    // Built with join so the separator is the platform's own (`\` on Windows).
    let expected = ["Chats", "Diagnostics", "Recordings", "Images", "Reports"]
        .map(|n| old.join(n).display().to_string());
    assert_eq!(names, expected);
}

#[test]
fn open_knows_each_folder_and_refuses_others() {
    let d = dir("open");
    let (config, home) = (d.join("config"), d.join("home"));
    change(&config, &d.join("files"));
    assert_eq!(
        folder_for(&config, Some(&home), "Reports").expect("reports"),
        d.join("files/Reports")
    );
    assert!(d.join("files/Reports").is_dir());
    assert_eq!(
        folder_for(&config, Some(&home), "models").expect("models"),
        home.join("GrokHub/models")
    );
    assert_eq!(
        folder_for(&config, Some(&home), "Secrets").unwrap_err(),
        "No folder called Secrets."
    );
}
