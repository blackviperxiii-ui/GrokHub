use super::*;

use std::sync::Mutex;

use grokhub_core::mods::install::State as InstallState;
use grokhub_core::mods::{Fetch, Keys, Reply, Request, SourceError};

use crate::mods_install::{requested_view, Offer};

const NOW: u64 = 1_800_000_000;

fn fixture(name: &str) -> String {
    std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../crates/grokhub-core/tests/fixtures/mods")
            .join(name),
    )
    .unwrap()
}

/// Answers Modrinth and Nexus from the fixtures and logs every URL asked.
struct Fake(Arc<Mutex<Vec<String>>>);

impl Fetch for Fake {
    fn send(&self, req: &Request) -> Result<Reply, SourceError> {
        self.0.lock().unwrap().push(req.url.clone());
        let body = if req.url.contains("/v2/search") {
            fixture("modrinth_search.json")
        } else if req.url.ends_with("/v2/project/AANobbMI/version") {
            fixture("modrinth_versions.json")
        } else if req.url.ends_with("/v2/project/AANobbMI") {
            fixture("modrinth_project.json")
        } else if req.url.contains("graphql") {
            fixture("nexus_search.json")
        } else if req.url.contains("/files.json") {
            fixture("nexus_files.json")
        } else if req.url.contains("/mods/") {
            fixture("nexus_detail.json")
        } else {
            return Ok(Reply {
                status: 404,
                ..Reply::default()
            });
        };
        Ok(Reply::ok(&body))
    }
}

fn temp(name: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("loadout-mods-agent-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn client(dir: &Path) -> (ModClient, Arc<Mutex<Vec<String>>>) {
    let log = Arc::new(Mutex::new(Vec::new()));
    let keys = Keys {
        nexus: Some("sk-abcdefghijklmnopqrstuv".into()),
        ..Keys::default()
    };
    (ModClient::new(Box::new(Fake(log.clone())), dir, keys), log)
}

fn minecraft() -> ModGame {
    ModGame {
        entry: allowlist::by_id("minecraft").unwrap(),
        last_played: 0,
        playtime_min: 0,
        installs: Vec::new(),
    }
}

fn file(id: &str, primary: bool) -> ModFile {
    ModFile {
        id: id.into(),
        name: "Main".into(),
        file_name: format!("{id}.zip"),
        version: "1".into(),
        size: 1_000,
        hash: None,
        download: Download::Direct("https://cdn.modrinth.com/x".into()),
        game_versions: vec![],
        deps: vec![],
        primary,
        uploaded: 0,
    }
}

#[test]
fn a_request_names_its_file_or_takes_the_main_one() {
    let files = [file("a", false), file("b", true)];
    assert_eq!(pick_file(&files, None).unwrap().id, "b");
    assert_eq!(pick_file(&files, Some("a")).unwrap().id, "a");
    assert_eq!(
        pick_file(&files, Some("z")).unwrap_err(),
        "This mod has no file z."
    );
    assert_eq!(
        pick_file(&[], None).unwrap_err(),
        "This mod has no files to install."
    );
    let m = ModSummary {
        id: "1".into(),
        name: "Sodium".into(),
        author: "jellysquid".into(),
        downloads: 34_500,
        summary: "  Fast\n rendering ".into(),
        ..Default::default()
    };
    assert_eq!(
        result_line(&m),
        "- Sodium (id 1) by jellysquid: 34K downloads. Fast rendering"
    );
    assert!(entry("apex").is_ok(), "the tools check the allowlist first");
    assert_eq!(entry("nope").unwrap_err(), allowlist::UNKNOWN_REASON);
}

#[test]
fn a_search_lists_the_sources_mods_for_the_model() {
    let dir = temp("search");
    let (c, log) = client(&dir);
    let mc = allowlist::by_id("minecraft").unwrap();
    let text = search_text(&c, mc, Source::Modrinth, "sodium", false, NOW).unwrap();
    assert_eq!(
        text,
        "On Modrinth:\n\
         - Sodium (id AANobbMI) by jellysquid3: 90.0M downloads. The fastest rendering optimization mod.\n\
         - Fabric API (id P7dR8mSH) by modmuss50: 120.0M downloads. Lightweight and modular API."
    );
    let asked = log.lock().unwrap().clone();
    assert_eq!(asked.len(), 1);
    assert!(asked[0].starts_with("https://api.modrinth.com/v2/search?query=sodium"));
}

#[test]
fn an_adult_mod_stays_out_unless_adults_only_is_on() {
    let dir = temp("adult");
    let (c, _) = client(&dir);
    let stardew = allowlist::by_id("stardew").unwrap();
    let off = search_text(&c, stardew, Source::Nexus, "", false, NOW).unwrap();
    let on = search_text(&c, stardew, Source::Nexus, "", true, NOW).unwrap();
    assert_eq!(
        off,
        "On Nexus Mods:\n- SkyUI (id 12604) by SkyUI Team: 20.0M downloads. Elegant, PC-friendly interface mod"
    );
    assert!(
        on.ends_with("\n- Adult Example (id 5000) by someone: 10 downloads. Flagged"),
        "{on}"
    );
}

#[test]
fn details_list_the_files_and_what_the_main_one_needs() {
    let dir = temp("detail");
    let (c, _) = client(&dir);
    let mc = allowlist::by_id("minecraft").unwrap();
    let text = detail_text(&c, mc, Source::Modrinth, "AANobbMI", false, NOW).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0], "Sodium on Modrinth");
    assert_eq!(lines[1], "Page: https://modrinth.com/mod/sodium");
    assert!(
        lines.contains(
            &"- file x: Sodium 0.6.0 mc1.21.3-0.6.0-fabric, 1.0 MB; Install downloads it after the user's click"
        ),
        "{text}"
    );
    assert_eq!(*lines.last().unwrap(), "Needs: P7dR8mSH");
}

#[test]
fn setting_up_a_mod_makes_a_card_and_downloads_nothing() {
    let dir = temp("prepare");
    let (c, log) = client(&dir);
    let (text, r) = prepare_request(
        &c,
        minecraft(),
        Source::Modrinth,
        ("AANobbMI", None),
        false,
        None,
        NOW,
    )
    .unwrap();
    assert_eq!(
        text,
        format!(
            "Install Sodium mc1.21.3-0.6.0-fabric for Minecraft: Java Edition? File sodium-fabric-0.6.0.jar, 1.0 MB, from Modrinth; Install downloads it after the user's click. {}",
            minecraft().entry.risk
        )
    );
    assert_eq!(r.key(), "modrinth:AANobbMI");
    assert_eq!(r.f.file_name, "sodium-fabric-0.6.0.jar");
    // Only the source's API was asked; the file's CDN never was.
    assert!(log
        .lock()
        .unwrap()
        .iter()
        .all(|u| u.starts_with("https://api.modrinth.com/")));

    let blocked = prepare_request(
        &c,
        minecraft(),
        Source::Modrinth,
        ("AANobbMI", None),
        false,
        Some("Close Minecraft first.".into()),
        NOW,
    )
    .unwrap()
    .0;
    assert!(
        blocked.contains(". The card says instead: Close Minecraft first. "),
        "{blocked}"
    );
    let missing = prepare_request(
        &c,
        minecraft(),
        Source::Modrinth,
        ("AANobbMI", Some("nope")),
        false,
        None,
        NOW,
    );
    assert_eq!(missing.unwrap_err(), "This mod has no file nope.");
}

#[test]
fn installed_lists_ours_then_what_else_the_game_has() {
    let mc = allowlist::by_id("minecraft").unwrap();
    assert_eq!(
        installed_text(mc, &[], &[]),
        "No mods found for Minecraft: Java Edition."
    );
    let ours = Manifest {
        game: "minecraft".into(),
        mod_key: "modrinth:AANobbMI".into(),
        name: "Sodium".into(),
        version: "0.6.0".into(),
        state: InstallState::Installed,
        root: PathBuf::from("/home/user/.minecraft"),
        files: Vec::new(),
        dirs: Vec::new(),
        build: None,
        at: 0,
        offline_only: Some("Offline play only.".into()),
    };
    let other = Installed {
        name: "OptiFine".into(),
        detail: "in mods".into(),
        link: None,
    };
    let app = grokhub_core::brand::product_name();
    assert_eq!(
        installed_text(mc, &[ours], &[other]),
        format!("- Sodium 0.6.0 (installed by {app}). Offline play only.\n- OptiFine (in mods)")
    );
}

/// The slot is global: one test owns it from start to end.
#[test]
fn the_card_waits_for_its_own_click_and_a_newer_ask_replaces_it() {
    let dir = temp("card");
    let (c, _) = client(&dir);
    let (_, r) = prepare_request(
        &c,
        minecraft(),
        Source::Modrinth,
        ("AANobbMI", None),
        false,
        None,
        NOW,
    )
    .unwrap();
    let v = requested_view((4, &r), &[], None, None);
    assert_eq!(v.ask, 4);
    assert_eq!(v.title, "Install Sodium for Minecraft: Java Edition?");
    assert_eq!(
        v.line,
        "sodium-fabric-0.6.0.jar · mc1.21.3-0.6.0-fabric · 1.0 MB · from Modrinth · asked for in chat"
    );
    assert_eq!(v.file.offer, Offer::Install);
    assert_eq!((v.game.as_str(), v.source), ("minecraft", "modrinth"));
    let shut = requested_view((4, &r), &[], Some("Close Minecraft first.".into()), None);
    assert_eq!(shut.file.offer, Offer::Blocked);
    assert_eq!(shut.blocked.as_deref(), Some("Close Minecraft first."));

    let first = mods_install::request(r.clone());
    let mut other_mod = r.clone();
    other_mod.m.id = "P7dR8mSH".into();
    let second = mods_install::request(other_mod);
    assert_eq!(second, first + 1);
    assert_eq!(
        mods_install::requested().map(|(_, r)| r.key()).as_deref(),
        Some("modrinth:P7dR8mSH")
    );
    // Dismiss on the card the page showed leaves a newer one alone, even a
    // newer ask for the same mod with another file.
    mods_install::drop_request(first);
    assert_eq!(mods_install::requested().map(|(n, _)| n), Some(second));
    mods_install::drop_request(second);
    assert_eq!(mods_install::requested(), None);
    let shown = mods_install::request(r.clone());
    let mut other_file = r.clone();
    other_file.f.id = "y".into();
    let newer = mods_install::request(other_file);
    mods_install::drop_request(shown);
    assert_eq!(
        mods_install::requested().map(|(n, r)| (n, r.f.id)),
        Some((newer, "y".to_string()))
    );
    mods_install::drop_request(newer);

    // Starting that mod's install answers the ask; another mod's doesn't.
    mods_install::request(r);
    mods_install::begin_plan("modrinth:P7dR8mSH").unwrap();
    mods_install::end_plan();
    assert!(mods_install::requested().is_some());
    mods_install::begin_plan("modrinth:AANobbMI").unwrap();
    assert_eq!(
        mods_install::begin_plan("modrinth:AANobbMI").unwrap_err(),
        "Another install is getting ready. Wait for it first."
    );
    mods_install::end_plan();
    assert_eq!(mods_install::requested(), None);
}

#[test]
fn the_host_counts_as_the_chats_mod_tools() {
    let told = Arc::new(Mutex::new(0));
    let t = told.clone();
    let host = Host::new(
        temp("host"),
        Box::new(|| false),
        Box::new(move || *t.lock().unwrap() += 1),
    );
    // A game the allowlist doesn't know is refused before any request.
    assert_eq!(
        host.prepare("nope", Source::Modrinth, "AANobbMI", None)
            .unwrap_err(),
        allowlist::UNKNOWN_REASON
    );
    assert_eq!(*told.lock().unwrap(), 0);
}
