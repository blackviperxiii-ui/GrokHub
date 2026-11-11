use super::*;
use std::cell::RefCell;
use std::collections::BTreeMap;

#[derive(Default)]
struct Mem(RefCell<BTreeMap<String, String>>);

impl Vault for Mem {
    fn get(&self, account: &str) -> Result<Option<String>, String> {
        Ok(self.0.borrow().get(account).cloned())
    }
    fn set(&self, account: &str, value: &str) -> Result<(), String> {
        self.0.borrow_mut().insert(account.into(), value.into());
        Ok(())
    }
    fn delete(&self, account: &str) -> Result<(), String> {
        self.0.borrow_mut().remove(account);
        Ok(())
    }
}

struct Dirs {
    root: PathBuf,
    config: PathBuf,
    ws: PathBuf,
}

impl Drop for Dirs {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn dirs(tag: &str) -> Dirs {
    let root = std::env::temp_dir().join(format!("cabin-secrets-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let config = root.join("config");
    let ws = root.join("project");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::create_dir_all(&ws).unwrap();
    Dirs { root, config, ws }
}

const VALUE: &str = "s3cr3t-value-41";

#[test]
fn add_empties_the_value_at_once_and_lists_the_name_only() {
    let d = dirs("add");
    let vault = Mem::default();
    let mut p = SecretsPanel {
        names: Some(Vec::new()),
        ..SecretsPanel::default()
    };
    p.name = " DEPLOY_TOKEN ".into();
    p.value = VALUE.into();
    assert!(p.can_save());
    let job = p.save().expect("a save job");
    assert_eq!(p.value, "");
    assert!(p.busy);
    assert_eq!(format!("{job:?}"), r#"Set { name: "DEPLOY_TOKEN" }"#);
    let out = job.run(&d.config, &d.ws, &vault);
    assert!(!format!("{out:?}").contains(VALUE));
    p.apply(out);
    assert_eq!(p.names, Some(vec!["DEPLOY_TOKEN".to_string()]));
    assert_eq!(p.note, None);
    assert_eq!(p.name, "");
    // The value went to the vault under the shell's own account name.
    let account = secrets::account(&repo_key_of(&d.ws), "DEPLOY_TOKEN");
    assert!(account.starts_with("code-secret:"));
    assert_eq!(vault.get(&account).unwrap().as_deref(), Some(VALUE));
    // Only the name is on disk, in the shared names file.
    let names_file = std::fs::read_to_string(d.config.join(secrets::FILE)).unwrap();
    assert!(names_file.contains("DEPLOY_TOKEN"));
    assert!(!names_file.contains(VALUE));
    // The activity log names the secret and never holds the value.
    let log = std::fs::read_to_string(d.config.join(activity_file())).unwrap();
    assert!(log.contains("Added secret DEPLOY_TOKEN."));
    assert!(!log.contains(VALUE));
}

fn activity_file() -> &'static str {
    grokhub_code::activity::FILE
}

#[test]
fn replace_says_so_and_keeps_one_name() {
    let d = dirs("replace");
    let vault = Mem::default();
    let mut p = SecretsPanel::default();
    p.apply(
        SecretJob::Set {
            name: "DEPLOY_TOKEN".into(),
            value: "first-value".into(),
        }
        .run(&d.config, &d.ws, &vault),
    );
    p.replace("DEPLOY_TOKEN");
    assert_eq!(p.name, "DEPLOY_TOKEN");
    assert!(p.replacing());
    p.value = "second-value".into();
    let out = p.save().unwrap().run(&d.config, &d.ws, &vault);
    p.apply(out);
    assert_eq!(p.note.as_deref(), Some("Replaced DEPLOY_TOKEN's value."));
    assert_eq!(p.names, Some(vec!["DEPLOY_TOKEN".to_string()]));
    let account = secrets::account(&repo_key_of(&d.ws), "DEPLOY_TOKEN");
    assert_eq!(
        vault.get(&account).unwrap().as_deref(),
        Some("second-value")
    );
}

#[test]
fn delete_asks_first_and_cancel_keeps_the_secret() {
    let d = dirs("delete");
    let vault = Mem::default();
    let mut p = SecretsPanel::default();
    p.apply(
        SecretJob::Set {
            name: "DEPLOY_TOKEN".into(),
            value: VALUE.into(),
        }
        .run(&d.config, &d.ws, &vault),
    );
    // The row's Delete only opens the confirm.
    p.ask_delete("DEPLOY_TOKEN");
    assert_eq!(p.deleting.as_deref(), Some("DEPLOY_TOKEN"));
    p.cancel_delete();
    assert_eq!(p.confirm_delete().map(|j| format!("{j:?}")), None);
    let account = secrets::account(&repo_key_of(&d.ws), "DEPLOY_TOKEN");
    assert!(vault.get(&account).unwrap().is_some());
    // Confirmed: gone from the keychain and the list.
    p.ask_delete("DEPLOY_TOKEN");
    let job = p.confirm_delete().expect("a delete job");
    p.apply(job.run(&d.config, &d.ws, &vault));
    assert_eq!(p.names, Some(Vec::new()));
    assert_eq!(vault.get(&account).unwrap(), None);
    // The shared store refuses a delete with no confirm.
    assert_eq!(
        secrets::delete(
            &d.config,
            &vault,
            &repo_key_of(&d.ws),
            "DEPLOY_TOKEN",
            false
        ),
        Err(secrets::NEEDS_CONFIRM.to_string())
    );
}

#[test]
fn a_refused_name_or_value_shows_why_and_the_value_still_empties() {
    let d = dirs("refused");
    let vault = Mem::default();
    let mut p = SecretsPanel {
        name: "PATH".into(),
        value: VALUE.into(),
        ..SecretsPanel::default()
    };
    let out = p.save().unwrap().run(&d.config, &d.ws, &vault);
    p.apply(out);
    assert_eq!(
        p.note.as_deref(),
        Some("PATH changes how programs run, so it can't be a deploy secret.")
    );
    assert_eq!(p.value, "");
    assert_eq!(p.name, "PATH");
    p.name = "DEPLOY_TOKEN".into();
    p.value = "abc".into();
    let out = p.save().unwrap().run(&d.config, &d.ws, &vault);
    p.apply(out);
    assert_eq!(
        p.note.as_deref(),
        Some("A secret needs at least 4 characters so it can be hidden in output.")
    );
    assert!(vault.0.borrow().is_empty());
}

#[test]
fn nothing_saves_without_a_name_and_a_value() {
    let mut p = SecretsPanel::default();
    assert!(!p.can_save());
    p.name = "DEPLOY_TOKEN".into();
    assert!(p.save().is_none());
    p.value = "   ".into();
    assert!(p.save().is_none());
    p.value = VALUE.into();
    p.busy = true;
    assert!(p.save().is_none());
    assert_eq!(p.value, VALUE);
}

#[test]
fn a_name_set_in_the_other_app_lists_here() {
    let d = dirs("shared");
    let vault = Mem::default();
    // The desktop app's own path: the same store and key.
    secrets::set(&d.config, &vault, &repo_key_of(&d.ws), "API_KEY", VALUE).unwrap();
    let out = SecretJob::List.run(&d.config, &d.ws, &vault);
    assert_eq!(out, SecretOut::Listed(vec!["API_KEY".to_string()]));
    assert_eq!(secrets::list_for(&d.config, &d.ws).names, vec!["API_KEY"]);
}

// ---- a value that left the panel leaves the field's undo history too ----

/// One painted frame of the panel at time `t`, with `events` and `mods`.
fn secrets_frame(
    ctx: &egui::Context,
    p: &mut SecretsPanel,
    d: &Dirs,
    t: f64,
    mut events: Vec<egui::Event>,
    mods: egui::Modifiers,
) {
    events.insert(0, egui::Event::ModifiersChanged(mods));
    let raw = egui::RawInput {
        time: Some(t),
        events,
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(800.0, 600.0),
        )),
        ..Default::default()
    };
    let _ = crate::theme::test_pass(ctx, raw, |ui| p.ui(ui, &d.config, &d.ws));
}

/// Type `VALUE` into the focused Value field and leave it there for 1.5 s,
/// long enough for egui to keep it as an undo point.
fn type_a_value(ctx: &egui::Context, p: &mut SecretsPanel, d: &Dirs) -> f64 {
    let none = egui::Modifiers::NONE;
    let mut t = 0.0;
    p.focus_value = true;
    secrets_frame(ctx, p, d, t, Vec::new(), none);
    t += 0.1;
    secrets_frame(ctx, p, d, t, Vec::new(), none);
    t += 0.1;
    secrets_frame(ctx, p, d, t, vec![egui::Event::Text(VALUE.into())], none);
    assert_eq!(p.value, VALUE);
    for _ in 0..15 {
        t += 0.1;
        secrets_frame(ctx, p, d, t, Vec::new(), none);
    }
    t
}

/// Click into the Value field again and press Ctrl+Z.
fn focus_and_undo(ctx: &egui::Context, p: &mut SecretsPanel, d: &Dirs, mut t: f64) {
    let none = egui::Modifiers::NONE;
    p.focus_value = true;
    for _ in 0..2 {
        t += 0.1;
        secrets_frame(ctx, p, d, t, Vec::new(), none);
    }
    let undo = egui::Event::Key {
        key: egui::Key::Z,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::COMMAND,
    };
    t += 0.1;
    secrets_frame(ctx, p, d, t, vec![undo], egui::Modifiers::COMMAND);
}

#[test]
fn undo_cannot_bring_a_saved_value_back_into_the_field() {
    let d = dirs("undo");
    let ctx = egui::Context::default();
    crate::theme::install_fonts_on(&ctx);
    // Replacing TOKEN's value: the list does not change, so the Value field
    // sits in the same place before and after the save.
    let mut p = SecretsPanel {
        names: Some(vec!["TOKEN".into()]),
        name: "TOKEN".into(),
        ws: Some(d.ws.clone()),
        ..SecretsPanel::default()
    };
    let mut t = type_a_value(&ctx, &mut p, &d);

    // Replace is pressed: the value leaves the panel (the job is dropped
    // here; nothing in this test reaches a keychain).
    let job = p.save().expect("a save job");
    drop(job);
    assert_eq!(p.value, "");
    for _ in 0..3 {
        t += 0.1;
        secrets_frame(&ctx, &mut p, &d, t, Vec::new(), egui::Modifiers::NONE);
    }
    // The save ends.
    p.busy = false;
    t += 0.1;
    secrets_frame(&ctx, &mut p, &d, t, Vec::new(), egui::Modifiers::NONE);

    focus_and_undo(&ctx, &mut p, &d, t);
    assert_eq!(p.value, "", "Ctrl+Z put the saved value back");
    // Nothing egui still holds for the old field can undo to it either.
    let old = egui::TextEdit::load_state(&ctx, value_id(0)).expect("the old field's state");
    let empty = (egui::text::CCursorRange::default(), String::new());
    assert!(!old.undoer().has_undo(&empty));
}

#[test]
fn undo_cannot_bring_a_value_typed_for_another_project_into_this_one() {
    let d = dirs("undo-project");
    let other = d.root.join("other-project");
    std::fs::create_dir_all(&other).unwrap();
    let ctx = egui::Context::default();
    crate::theme::install_fonts_on(&ctx);
    let mut p = SecretsPanel {
        names: Some(Vec::new()),
        name: "TOKEN".into(),
        ws: Some(d.ws.clone()),
        ..SecretsPanel::default()
    };
    let t = type_a_value(&ctx, &mut p, &d);

    // The project changes with the value still typed: the panel starts over.
    let there = Dirs {
        root: d.root.join("unused"),
        config: d.config.clone(),
        ws: other,
    };
    secrets_frame(
        &ctx,
        &mut p,
        &there,
        t + 0.1,
        Vec::new(),
        egui::Modifiers::NONE,
    );
    assert_eq!(p.value, "");
    // Its list arrives (read off the UI thread) and the row is usable again.
    let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut t = t + 0.1;
    while p.busy && std::time::Instant::now() < until {
        t += 0.1;
        secrets_frame(&ctx, &mut p, &there, t, Vec::new(), egui::Modifiers::NONE);
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(!p.busy);
    focus_and_undo(&ctx, &mut p, &there, t);
    assert_eq!(p.value, "", "Ctrl+Z put the other project's value back");
}
