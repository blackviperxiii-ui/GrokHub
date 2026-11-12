use super::*;
use grokhub_agent::harness::{use_key_store_for, MemoryKeyStore};
use std::path::PathBuf;
use std::sync::Arc;

struct Dir(PathBuf, Arc<MemoryKeyStore>);

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

fn dir(name: &str) -> Dir {
    let root = std::env::temp_dir().join(format!(
        "grokhub-scopes-{name}-{}-{}",
        std::process::id(),
        grokhub_core::now_ms()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("dir");
    let keys = Arc::new(MemoryKeyStore::new());
    use_key_store_for(&root, keys.clone());
    Dir(root, keys)
}

fn row<'a>(v: &'a ScopesView, kind: &str) -> &'a ScopeRow {
    v.rows.iter().find(|r| r.kind == kind).expect("row")
}

#[test]
fn every_scope_starts_off() {
    let config = dir("off");
    let v = view(&config, 1_000);
    let kinds: Vec<&str> = v.rows.iter().map(|r| r.kind.as_str()).collect();
    assert_eq!(
        kinds,
        [
            "files",
            "apps",
            "browser_history",
            "calendar",
            "mail",
            "system_state"
        ]
    );
    assert!(v.rows.iter().all(|r| r.granted.is_empty()));
    let apps = row(&v, "apps").add.clone().expect("allow row");
    assert_eq!(apps.title, "Installed apps");
    assert_eq!(
        apps.hint,
        "Off. Installed apps and how often you open them."
    );
    assert_eq!(v.locked, None);
}

#[test]
fn an_allowed_scope_round_trips_and_revokes() {
    let config = dir("apps");
    let home = Path::new("/home/you");
    assert_eq!(
        allow(&config, Some(home), "apps", ""),
        format!(
            "Installed apps allowed. {} learns from it on this computer. Revoke it here any time.",
            grokhub_core::brand::product_name()
        )
    );
    assert_eq!(
        allow(&config, Some(home), "apps", ""),
        "Installed apps is already allowed."
    );
    let v = view(&config, grokhub_core::now_ms());
    let apps = row(&v, "apps");
    assert_eq!(apps.granted.len(), 1);
    assert_eq!(apps.granted[0].title, "Installed apps");
    assert!(apps.granted[0]
        .hint
        .ends_with(". Installed apps and how often you open them."));
    assert_eq!(apps.add, None);
    let id = apps.granted[0].id.clone();
    assert_eq!(
        revoke(&config, &id),
        "Installed apps revoked. It's off until you allow it again."
    );
    assert_eq!(revoke(&config, &id), "That grant is already gone.");
    assert!(row(&view(&config, 1), "apps").granted.is_empty());
}

#[test]
fn a_folder_is_named_by_its_last_part_and_more_can_be_added() {
    let config = dir("files");
    let home = Path::new("/home/you");
    assert_eq!(
        allow(&config, Some(home), "files", "/srv/notes/"),
        format!(
            "Files in notes allowed. {} learns from it on this computer. Revoke it here any time.",
            grokhub_core::brand::product_name()
        )
    );
    let v = view(&config, grokhub_core::now_ms());
    let files = row(&v, "files");
    assert_eq!(files.granted[0].title, "Files in notes");
    assert_eq!(files.granted[0].detail.as_deref(), Some("/srv/notes"));
    assert!(files.granted[0].hint.ends_with(". /srv/notes"));
    assert_eq!(
        files.add.as_ref().map(|a| a.title.as_str()),
        Some("Add a folder")
    );
    assert_eq!(folder_name("C:\\Users\\you\\Notes"), "Notes");
    assert_eq!(folder_name("/"), "/");
    assert_eq!(
        scope_label("browser_history:brave"),
        "Browser history (Brave)"
    );
}

#[test]
fn the_home_folder_and_a_relative_path_are_refused() {
    let config = dir("refuse");
    let home = Path::new("/home/you");
    assert_eq!(
        allow(&config, Some(home), "files", "/home/you"),
        "Not allowed: files: one folder at a time, never all of your home folder"
    );
    assert_eq!(
        allow(&config, Some(home), "files", "notes"),
        "Not allowed: files: needs one absolute folder"
    );
    assert_eq!(
        allow(&config, Some(home), "files", "/home/you/.ssh"),
        "Not allowed: files: that folder is always excluded"
    );
    assert!(row(&view(&config, 1), "files").granted.is_empty());
}

#[test]
fn mail_has_no_reader_and_says_so() {
    let config = dir("mail");
    let v = view(&config, 1);
    assert_eq!(
        row(&v, "mail").add.as_ref().map(|a| a.hint.clone()),
        Some(format!(
            "Off. Not read yet: {} has no mail reader, so allowing it reads nothing today.",
            grokhub_core::brand::product_name()
        ))
    );
    assert_eq!(
        allow(&config, None, "mail", ""),
        "Mail allowed. Nothing reads it yet. Revoke it here any time."
    );
}

#[test]
fn a_browser_grant_names_the_browser_and_offers_another() {
    let config = dir("browser");
    assert_eq!(
        allow(&config, None, "browser_history", "brave"),
        format!(
            "Browser history (Brave) allowed. {} learns from it on this computer. Revoke it here any time.",
            grokhub_core::brand::product_name()
        )
    );
    let v = view(&config, grokhub_core::now_ms());
    let b = row(&v, "browser_history");
    assert_eq!(b.granted[0].title, "Browser history (Brave)");
    assert_eq!(
        b.add.as_ref().map(|a| a.title.as_str()),
        Some("Add a browser")
    );
    assert_eq!(
        allow(&config, None, "nope", ""),
        "Not allowed: that isn't a scope."
    );
}

#[test]
fn a_long_path_keeps_both_ends() {
    let long = "/tmp/gh-scopes-shots/home/Documents/Projects/2026/clients/hale/notes";
    assert_eq!(
        middle_ellipsis(long, 32),
        "/tmp/gh-scop…/clients/hale/notes"
    );
    assert_eq!(middle_ellipsis("/srv/notes", 32), "/srv/notes");
}

#[test]
fn a_locked_ledger_grants_nothing() {
    let config = dir("locked");
    assert!(allow(&config, None, "apps", "").starts_with("Installed apps allowed."));
    config.1.set_available(false);
    use_key_store_for(&config, config.1.clone());
    let v = view(&config, 1);
    assert_eq!(v.lock_hover.as_deref(), Some("Private data is unavailable"));
    assert!(v
        .locked
        .as_deref()
        .unwrap_or("")
        .contains("Nothing was deleted."));
    assert_eq!(allow(&config, None, "mail", ""), v.locked.clone().unwrap());
}

fn fact(id: &str, line: &str) -> grokhub_agent::indexers::IndexedFact {
    grokhub_agent::indexers::IndexedFact {
        id: id.into(),
        line: line.into(),
    }
}

#[test]
fn learned_lists_three_facts_then_and_n_more_with_forget_these() {
    let facts = [
        fact("scope-000000000001", "Notes: a.md"),
        fact("scope-000000000002", "Notes: b.md"),
        fact("scope-000000000003", "Notes: c.md"),
        fact("scope-000000000004", "Notes: d.md"),
        fact("scope-000000000005", "Notes: e.md"),
    ];
    let l = learned("files:/srv/notes", &facts);
    assert_eq!(
        l.head.as_deref(),
        Some("Learned 5 things, sealed on this computer:")
    );
    assert_eq!(l.lines, ["Notes: a.md", "Notes: b.md", "Notes: c.md"]);
    assert_eq!(l.more.as_deref(), Some("and 2 more"));
    assert_eq!(l.note, None);
    assert!(l.can_forget);
    let one = learned("apps", &facts[..1]);
    assert_eq!(
        one.head.as_deref(),
        Some("Learned 1 thing, sealed on this computer:")
    );
    assert_eq!(one.more, None);
    assert_eq!(FORGET_THESE, "Forget these");
}

#[test]
fn learned_says_why_nothing_shows() {
    let product = grokhub_core::brand::product_name();
    // No reader: the row's hint says so, nothing more under it.
    assert_eq!(learned("mail", &[]), Learned::default());
    assert_eq!(
        learned("calendar", &[fact("scope-000000000001", "x")]),
        Learned::default()
    );
    let empty = learned("apps", &[]);
    assert_eq!(
        empty.note,
        Some(format!("Nothing learned to show. {product} reads it on a heartbeat, never on battery or in quiet hours."))
    );
    assert_eq!(empty.head, None);
    assert!(!empty.can_forget);
}

#[test]
fn a_new_grant_shows_nothing_learned_and_forget_keeps_it_allowed() {
    let config = dir("forget");
    let home = Path::new("/home/you");
    allow(&config, Some(home), "apps", "");
    let v = view(&config, 2_000);
    let g = row(&v, "apps").granted[0].clone();
    assert_eq!(g.learned.lines, Vec::<String>::new());
    assert!(!g.learned.can_forget);
    assert_eq!(
        forget(&config, &g.id),
        "Forgot 0 things learned from Installed apps. It stays allowed; revoke it to stop reading."
    );
    assert_eq!(row(&view(&config, 3_000), "apps").granted.len(), 1);
    assert_eq!(forget(&config, "nope"), "That grant is already gone.");
}
