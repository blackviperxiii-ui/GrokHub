use super::*;
use std::fs;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::attach::{Kind, Staged};

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "gh-shot-offer-{tag}-{}-{}",
        std::process::id(),
        grokhub_core::now_ms()
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A picture in `dir` stamped `age_ms` before `now_ms`.
fn shot(dir: &Path, name: &str, now_ms: u64, age_ms: u64) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, b"\x89PNG\r\n\x1a\n").unwrap();
    let when = UNIX_EPOCH + Duration::from_millis(now_ms - age_ms);
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(when)
        .unwrap();
    path
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

fn staged(name: &str) -> Outcome {
    Outcome {
        staged: Some(Staged {
            path: format!("/chat/attach/{name}"),
            name: name.into(),
            kind: Kind::Image,
            label: format!("Picture {name}"),
            status: None,
        }),
        error: None,
    }
}

fn explain_offer(name: &str) -> Offer {
    Offer {
        name: name.into(),
        label: "Explain this?".into(),
        dismiss: "Not now".into(),
        prompt: "Explain what you see in this screenshot. Name the app or screen, the important text, and what the user might want to do next.".into(),
    }
}

#[test]
fn a_new_screenshot_in_a_watched_folder_offers_explain() {
    let cfg = scratch("new-cfg");
    let pics = scratch("new-pics");
    let t = now();
    set_enabled(&cfg, true, t).unwrap();
    shot(&pics, "Screenshot 1.png", t, 5_000);
    let view = check(&cfg, std::slice::from_ref(&pics), t);
    assert_eq!(
        view,
        View {
            enabled: true,
            offer: Some(explain_offer("Screenshot 1.png")),
        }
    );
    // The cabin reads the same file and sees the same offer.
    let ledger = load_ledger(&cfg);
    assert_eq!(
        ledger.pending.map(|p| p.path),
        Some(pics.join("Screenshot 1.png").display().to_string())
    );
    let _ = fs::remove_dir_all(&cfg);
    let _ = fs::remove_dir_all(&pics);
}

#[test]
fn an_old_screenshot_offers_nothing() {
    let cfg = scratch("old-cfg");
    let pics = scratch("old-pics");
    let t = now();
    set_enabled(&cfg, true, t).unwrap();
    shot(&pics, "last-week.png", t, 10 * 60 * 1000);
    assert_eq!(
        check(&cfg, std::slice::from_ref(&pics), t),
        View {
            enabled: true,
            offer: None,
        }
    );
    let _ = fs::remove_dir_all(&cfg);
    let _ = fs::remove_dir_all(&pics);
}

#[test]
fn with_the_switch_off_nothing_is_offered() {
    let cfg = scratch("off-cfg");
    let pics = scratch("off-pics");
    let t = now();
    shot(&pics, "fresh.png", t, 1_000);
    // Off by default: no app.json at all.
    assert!(!enabled(&cfg));
    assert_eq!(
        check(&cfg, std::slice::from_ref(&pics), t),
        View {
            enabled: false,
            offer: None,
        }
    );
    assert!(load_ledger(&cfg).pending.is_none());
    let _ = fs::remove_dir_all(&cfg);
    let _ = fs::remove_dir_all(&pics);
}

#[test]
fn turning_it_off_takes_the_offer_down_and_keeps_other_keys() {
    let cfg = scratch("toggle-cfg");
    let pics = scratch("toggle-pics");
    fs::write(cfg.join("app.json"), r#"{"focusHelp":true}"#).unwrap();
    let t = now();
    set_enabled(&cfg, true, t).unwrap();
    shot(&pics, "a.png", t, 1_000);
    assert!(check(&cfg, std::slice::from_ref(&pics), t).offer.is_some());

    let view = set_enabled(&cfg, false, t).unwrap();
    assert_eq!(
        view,
        View {
            enabled: false,
            offer: None,
        }
    );
    assert!(load_ledger(&cfg).pending.is_none());
    let app: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(cfg.join("app.json")).unwrap()).unwrap();
    assert_eq!(
        app,
        serde_json::json!({ "focusHelp": true, "screenshotExplain": { "enabled": false } })
    );
    let _ = fs::remove_dir_all(&cfg);
    let _ = fs::remove_dir_all(&pics);
}

#[test]
fn an_offer_the_cabin_saved_shows_here() {
    let cfg = scratch("cabin-cfg");
    let t = now();
    fs::write(
        cfg.join("app.json"),
        r#"{"screenshotExplain":{"enabled":true}}"#,
    )
    .unwrap();
    fs::write(
        cfg.join("screenshot-offer.json"),
        format!(
            r#"{{"lastSeenModifiedMs":{},"pending":{{"path":"/pics/from-cabin.png","offeredAtMs":{}}}}}"#,
            t - 2_000,
            t - 1_000
        ),
    )
    .unwrap();
    assert_eq!(
        check(&cfg, &[], t),
        View {
            enabled: true,
            offer: Some(explain_offer("from-cabin.png")),
        }
    );
    let _ = fs::remove_dir_all(&cfg);
}

#[test]
fn explain_attaches_the_file_and_then_the_offer_comes_down() {
    let cfg = scratch("explain-cfg");
    let pics = scratch("explain-pics");
    let t = now();
    set_enabled(&cfg, true, t).unwrap();
    let file = shot(&pics, "b.png", t, 1_000);
    check(&cfg, std::slice::from_ref(&pics), t);

    let mut asked = None;
    let outcome = explain(&cfg, t, |p| {
        asked = Some(p.to_path_buf());
        staged("b.png")
    })
    .unwrap();
    assert_eq!(asked, Some(file));
    assert_eq!(outcome, staged("b.png"));
    assert!(load_ledger(&cfg).pending.is_none());
    assert_eq!(check(&cfg, std::slice::from_ref(&pics), t).offer, None);
    let _ = fs::remove_dir_all(&cfg);
    let _ = fs::remove_dir_all(&pics);
}

#[test]
fn a_full_tray_keeps_the_offer_for_another_try() {
    let cfg = scratch("full-cfg");
    let pics = scratch("full-pics");
    let t = now();
    set_enabled(&cfg, true, t).unwrap();
    shot(&pics, "c.png", t, 1_000);
    check(&cfg, std::slice::from_ref(&pics), t);
    let refused = Outcome {
        staged: None,
        error: Some("c.png: too many attachments".into()),
    };
    assert_eq!(explain(&cfg, t, |_| refused.clone()).unwrap(), refused);
    assert_eq!(
        check(&cfg, std::slice::from_ref(&pics), t).offer,
        Some(explain_offer("c.png"))
    );
    let _ = fs::remove_dir_all(&cfg);
    let _ = fs::remove_dir_all(&pics);
}

#[test]
fn explain_on_a_deleted_file_says_so_and_never_stages() {
    let cfg = scratch("gone-cfg");
    let pics = scratch("gone-pics");
    let t = now();
    set_enabled(&cfg, true, t).unwrap();
    let file = shot(&pics, "d.png", t, 1_000);
    check(&cfg, std::slice::from_ref(&pics), t);
    fs::remove_file(&file).unwrap();
    let result = explain(&cfg, t, |_| panic!("nothing to stage"));
    assert_eq!(result, Err("That screenshot is gone.".to_string()));
    assert!(load_ledger(&cfg).pending.is_none());
    let _ = fs::remove_dir_all(&cfg);
    let _ = fs::remove_dir_all(&pics);
}

#[test]
fn explain_without_a_live_offer_is_refused() {
    let cfg = scratch("none-cfg");
    let t = now();
    set_enabled(&cfg, true, t).unwrap();
    let result = explain(&cfg, t, |_| panic!("nothing to stage"));
    assert_eq!(result, Err("That screenshot offer has ended.".to_string()));
    let _ = fs::remove_dir_all(&cfg);
}

#[test]
fn not_now_takes_the_offer_down_until_the_next_screenshot() {
    let cfg = scratch("dismiss-cfg");
    let pics = scratch("dismiss-pics");
    let t = now();
    set_enabled(&cfg, true, t).unwrap();
    shot(&pics, "e.png", t, 3_000);
    check(&cfg, std::slice::from_ref(&pics), t);
    assert_eq!(
        dismiss(&cfg),
        View {
            enabled: true,
            offer: None,
        }
    );
    // The same file never offers again.
    assert_eq!(check(&cfg, std::slice::from_ref(&pics), t).offer, None);
    // A newer one does.
    shot(&pics, "f.png", t, 1_000);
    assert_eq!(
        check(&cfg, std::slice::from_ref(&pics), t).offer,
        Some(explain_offer("f.png"))
    );
    let _ = fs::remove_dir_all(&cfg);
    let _ = fs::remove_dir_all(&pics);
}
