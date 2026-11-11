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
    let mut p = SecretsPanel::default();
    p.name = "PATH".into();
    p.value = VALUE.into();
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
