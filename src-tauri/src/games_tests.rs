use super::*;
use std::fs;
use std::time::SystemTime;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "grokhub-games-{name}-{}-{}",
        std::process::id(),
        grokhub_core::now_ms()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("temp");
    dir
}

fn wow_install(root: &Path) -> PathBuf {
    let install = root.join("World of Warcraft");
    let retail = install.join("_retail_");
    fs::create_dir_all(retail.join("Interface").join("AddOns")).expect("addons");
    fs::create_dir_all(retail.join("WTF")).expect("wtf");
    // Flavor detection needs Interface or WTF or exe.
    fs::write(retail.join("Wow.exe"), b"").expect("exe");
    install
}

#[test]
fn wow_view_lists_flavors_and_empty_saved() {
    let root = temp("wow");
    let install = wow_install(&root);
    let games = vec![BnetGame {
        product: "wow".into(),
        install: install.clone(),
        prefix: None,
    }];
    let view = wow_view_from(&games, true, None, SystemTime::UNIX_EPOCH);
    assert_eq!(view.title, "WORLD OF WARCRAFT");
    assert!(!view.running);
    assert!(view.caption.contains("Retail"));
    assert!(view.caption.contains("Battle.net"));
    assert_eq!(view.flavors.len(), 1);
    assert_eq!(
        view.flavors[0].addon_line,
        format!(
            "{} addon not installed",
            grokhub_core::brand::product_name()
        )
    );
    assert_eq!(view.flavors[0].action.as_deref(), Some("Install addon"));
    assert_eq!(
        view.flavors[0].empty_saved.as_deref(),
        Some(WOW_EMPTY_SAVED)
    );
    assert!(view.watcher_note.is_none());
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn wow_watcher_off_shows_the_cabin_note() {
    let root = temp("wow-off");
    let install = wow_install(&root);
    let games = vec![BnetGame {
        product: "wow".into(),
        install,
        prefix: None,
    }];
    let view = wow_view_from(&games, false, None, SystemTime::UNIX_EPOCH);
    assert_eq!(view.watcher_note.as_deref(), Some(WATCHER_OFF));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn wow_install_and_remove_addon_updates_the_row() {
    let root = temp("wow-addon");
    let install = wow_install(&root);
    let games = vec![BnetGame {
        product: "wow".into(),
        install: install.clone(),
        prefix: None,
    }];
    let before = wow_view_from(&games, true, None, SystemTime::UNIX_EPOCH);
    assert_eq!(before.flavors[0].action.as_deref(), Some("Install addon"));

    wow_addon::install(&install, &WowFlavor::Retail.dir(&install)).expect("install");
    let mid = wow_view_from(&games, true, None, SystemTime::UNIX_EPOCH);
    assert!(mid.flavors[0]
        .addon_line
        .starts_with(&format!("{} addon ", grokhub_core::brand::product_name())));
    assert_eq!(mid.flavors[0].action.as_deref(), Some("Remove addon"));

    wow_addon::remove(&WowFlavor::Retail.dir(&install)).expect("remove");
    let after = wow_view_from(&games, true, None, SystemTime::UNIX_EPOCH);
    assert_eq!(
        after.flavors[0].addon_line,
        format!(
            "{} addon not installed",
            grokhub_core::brand::product_name()
        )
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn gw2_status_and_offline_row() {
    let dir = temp("gw2");
    let empty = gw2_view(&dir, 0);
    assert!(empty.status.contains("add a read-only API key"));
    assert!(!empty.has_key);

    *TEST_GW2_KEY.lock().unwrap() =
        Some("ABCDEF01-2345-6789-ABCD-EF0123456789ABCDEF01-2345-6789-ABCD-EF0123456789".into());
    let keyed = gw2_view(&dir, 0);
    assert_eq!(keyed.status, "Guild Wars 2: key saved, nothing read yet.");
    assert!(keyed.has_key);

    let offline = gw2_refresh_offline_row(&dir, 0);
    assert_eq!(offline.note, GW2_OFFLINE);

    let acct = Gw2Account {
        fractal_level: Some(5),
        fetched_ms: 0,
        ..Default::default()
    };
    gw2::save_cache(&dir, &acct).expect("cache");
    let snap = gw2_view(&dir, 4 * 60_000);
    assert_eq!(
        snap.status,
        "Guild Wars 2: Fractal level 5. Updated 4 minutes ago."
    );
    assert!(!serde_json::to_string(&snap).unwrap().contains("ABCDEF"));

    *TEST_GW2_KEY.lock().unwrap() = None;
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn gw2_credentials_card_save_and_deny() {
    let dir = temp("gw2-card");
    let _pin = grokhub_agent::perm::ConfigGuard::set(dir.clone());
    *TEST_GW2_KEY.lock().unwrap() = None;

    let bad = gw2_start_key(&dir, "hunter2".into());
    assert!(bad.unwrap_err().contains("doesn't look like"));

    let key = "ABCDEF01-2345-6789-ABCD-EF0123456789ABCDEF01-2345-6789-ABCD-EF0123456789";
    let card = gw2_start_key(&dir, key.into()).expect("card");
    assert_eq!(card.action, "Save the Guild Wars 2 API key");
    let denied = gw2_answer_key(&dir, &card.id, false, 0).expect("deny");
    assert!(denied.note.contains("denied") || denied.note.contains("Nothing saved"));
    assert!(!denied.has_key);

    let card = gw2_start_key(&dir, key.into()).expect("card2");
    let saved = gw2_answer_key(&dir, &card.id, true, 0).expect("save");
    assert!(saved.has_key);
    assert!(TEST_GW2_KEY.lock().unwrap().is_some());

    let removed = gw2_remove_key(&dir, 0).expect("remove");
    assert!(!removed.has_key);
    assert_eq!(removed.note, "Key removed.");
    *TEST_GW2_KEY.lock().unwrap() = None;
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn eso_install_remove_and_status() {
    let root = temp("eso");
    let server = root.join("SavedVariables");
    // ESO server dirs are live/na or live/eu under Documents; use a fixture
    // folder that state() can read by writing through install.
    fs::create_dir_all(&server).expect("server");
    // Install expects the server dir itself; state reads AddOns/Loadout.
    // Use eso::install on a made-up server dir that has AddOns parent.
    let server = root.join("live").join("AddOns");
    // find_server_dirs looks for specific paths; drive eso_view_from directly.
    let server_dir = root.join("live");
    fs::create_dir_all(server_dir.join("AddOns")).expect("addons");
    fs::create_dir_all(server_dir.join("SavedVariables")).expect("sv");

    let empty = eso_view_from(std::slice::from_ref(&server_dir));
    assert_eq!(empty.servers[0].status, "ESO addon: not installed.");
    assert_eq!(
        empty.servers[0].action.as_deref(),
        Some("Install ESO addon")
    );

    eso::install(&server_dir).expect("install");
    let installed = eso_view_from(std::slice::from_ref(&server_dir));
    assert!(installed.servers[0].status.contains("installed"));
    assert_eq!(
        installed.servers[0].action.as_deref(),
        Some("Remove ESO addon")
    );

    eso::remove(&server_dir).expect("remove");
    let removed = eso_view_from(&[server_dir]);
    assert_eq!(removed.servers[0].status, "ESO addon: not installed.");
    let _ = fs::remove_dir_all(&root);
    let _ = server; // silence
}

#[test]
fn osrs_save_forget_and_offline_row() {
    let dir = temp("osrs");
    let empty = osrs_view(&dir, 0);
    assert!(empty.status.contains("add your display name"));
    assert!(!empty.has_player);

    let saved = osrs_save(&dir, "Zezima", "Ironman", 0).expect("save");
    assert!(saved.has_player);
    assert_eq!(
        saved.status,
        "Old School RuneScape (Zezima, ironman): nothing read yet."
    );

    let lite = r#"{"skills":[{"id":0,"name":"Overall","rank":10,"level":500,"xp":100000},{"id":1,"name":"Attack","rank":5,"level":40,"xp":40000}],"activities":[]}"#;
    let mut store = osrs::load(&dir).expect("load");
    store.push(osrs::parse(lite, 0).unwrap());
    osrs::save(&dir, &store).expect("write");
    let with = osrs_view(&dir, 20 * 60_000);
    assert!(with.status.contains("Zezima, ironman"));
    assert!(with.status.contains("Updated 20 minutes ago"));

    let offline = osrs_refresh_offline_row(&dir, 0);
    assert_eq!(offline.note, OSRS_OFFLINE);

    let forgotten = osrs_forget(&dir, 0).expect("forget");
    assert!(!forgotten.has_player);
    assert_eq!(forgotten.note, "Player forgotten.");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn bad_osrs_name_is_refused() {
    let dir = temp("osrs-bad");
    let err = osrs_save(&dir, "bad/name", "Normal", 0).unwrap_err();
    assert!(err.contains("display name"));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn destiny_status_key_name_and_offline_row() {
    let dir = temp("d2");
    *TEST_D2_KEY.lock().unwrap() = None;
    let empty = destiny_view(&dir, 0);
    assert!(empty.status.contains("your own Bungie.net API key"));
    assert!(!empty.has_key);
    assert!(!empty.has_name);
    assert!(empty.how_to.contains(grokhub_core::brand::product_name()));

    *TEST_D2_KEY.lock().unwrap() = Some("0123456789abcdef0123456789abcdef".into());
    destiny_store_name(&dir, Some("Tester#0042")).expect("name");
    let keyed = destiny_view(&dir, 0);
    assert_eq!(
        keyed.status,
        "Destiny 2: key and name saved, nothing read yet."
    );
    assert!(keyed.has_key && keyed.has_name);

    let p = DestinyProfile {
        bungie_name: "Tester#0042".into(),
        triumph_score: Some(5),
        fetched_ms: 0,
        ..Default::default()
    };
    d2::save_cache(&dir, &p).expect("cache");
    let snap = destiny_view(&dir, 3 * 60_000);
    assert_eq!(
        snap.status,
        "Destiny 2 (Tester#0042): Triumph score 5. Updated 3 minutes ago."
    );
    assert!(!serde_json::to_string(&snap).unwrap().contains("0123456789"));

    let offline = destiny_refresh_offline_row(&dir, 0);
    assert_eq!(offline.note, D2_OFFLINE);

    *TEST_D2_KEY.lock().unwrap() = None;
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn destiny_credentials_card_and_name() {
    let dir = temp("d2-card");
    let _pin = grokhub_agent::perm::ConfigGuard::set(dir.clone());
    *TEST_D2_KEY.lock().unwrap() = None;

    let bad = destiny_start_key(&dir, "hunter2".into());
    assert!(bad.unwrap_err().contains("doesn't look like"));

    let key = "0123456789abcdef0123456789abcdef";
    let card = destiny_start_key(&dir, key.into()).expect("card");
    assert_eq!(card.action, "Save the Bungie.net API key");
    let denied = destiny_answer_key(&dir, &card.id, false, 0).expect("deny");
    assert!(denied.note.contains("denied") || denied.note.contains("Nothing saved"));
    assert!(!denied.has_key);

    let card = destiny_start_key(&dir, key.into()).expect("card2");
    let saved = destiny_answer_key(&dir, &card.id, true, 0).expect("save");
    assert!(saved.has_key);
    assert!(TEST_D2_KEY.lock().unwrap().is_some());

    let named = destiny_save_name(&dir, "Tester#42", 0);
    assert!(named.unwrap_err().contains("four-digit"));
    let named = destiny_save_name(&dir, "Tester#0042", 0).expect("name");
    assert!(named.has_name);
    assert_eq!(named.note, "Name saved.");

    let removed = destiny_remove(&dir, 0).expect("remove");
    assert!(!removed.has_key && !removed.has_name);
    assert_eq!(removed.note, "Key and name removed.");
    *TEST_D2_KEY.lock().unwrap() = None;
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn poe_and_elite_find_installs_and_show_empty_when_idle() {
    let root = temp("poe-elite");
    let steam = root.join("steam");
    // Watchers start at EOF, so a pre-written log is idle until new lines arrive.
    let logs = steam.join("steamapps/common/Path of Exile/logs");
    fs::create_dir_all(&logs).unwrap();
    fs::write(logs.join("Client.txt"), "").unwrap();
    let poe = poe_view_from(Some(&steam), 238960);
    assert_eq!(poe.title, "Path of Exile");
    assert!(poe.status.is_empty());
    assert!(poe.empty.as_deref().unwrap().contains("Client.txt"));

    let missing = poe_view_from(Some(&steam), 730);
    assert!(missing
        .empty
        .as_deref()
        .unwrap()
        .contains("isn't Path of Exile"));

    let journal = steam.join(
        "steamapps/compatdata/359320/pfx/drive_c/users/steamuser/Saved Games/Frontier Developments/Elite Dangerous",
    );
    fs::create_dir_all(&journal).unwrap();
    fs::write(journal.join("Journal.2026-10-31T211403.01.log"), "").unwrap();
    let elite = elite_view_from(Some(&steam), Some(&journal));
    assert_eq!(elite.title, "Elite Dangerous");
    assert!(elite.empty.as_deref().unwrap().contains("Player Journal"));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn factorio_mod_install_and_status() {
    let dir = temp("factorio");
    fs::create_dir_all(&dir).unwrap();
    let empty = factorio_view_from(Some(&dir));
    assert_eq!(empty.status, "Factorio mod: not installed.");
    assert_eq!(empty.action.as_deref(), Some("Install Factorio mod"));
    assert!(empty.warning.is_some());

    let installed = factorio_mod_action(&dir, true).expect("install");
    assert!(installed.status.contains("installed"));
    assert!(installed.note.starts_with("Installed."));
    assert_eq!(installed.action.as_deref(), Some("Remove Factorio mod"));

    let removed = factorio_mod_action(&dir, false).expect("remove");
    assert_eq!(removed.status, "Factorio mod: not installed.");
    assert_eq!(removed.note, "Removed.");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn stardew_reads_farm_and_minecraft_idle_shows_empty() {
    let root = temp("stardew-mc");
    let steam = root.join("steam");
    let farm = steam.join(
        "steamapps/compatdata/413150/pfx/drive_c/users/steamuser/AppData/Roaming/StardewValley/Saves/Testfarmer_1",
    );
    fs::create_dir_all(&farm).unwrap();
    fs::write(
        farm.join("SaveGameInfo"),
        r#"<?xml version="1.0" encoding="utf-8"?><Farmer><name>Testfarmer</name><farmName>Pickle</farmName><money>500</money><totalMoneyEarned>1000</totalMoneyEarned><millisecondsPlayed>5000</millisecondsPlayed><dayOfMonthForSaveGame>5</dayOfMonthForSaveGame><seasonForSaveGame>0</seasonForSaveGame><yearForSaveGame>1</yearForSaveGame></Farmer>"#,
    )
    .unwrap();
    let s = stardew_view_from(Some(&steam));
    assert!(s
        .status
        .starts_with("Stardew Valley: Testfarmer of Pickle Farm"));

    // Log tail starts at EOF; an idle companion shows the empty how-to.
    let home = root.join("home");
    let logs = home.join(".minecraft/logs");
    fs::create_dir_all(&logs).unwrap();
    fs::write(logs.join("latest.log"), "").unwrap();
    let m = minecraft_view_from(Some(&home));
    assert_eq!(m.title, "Minecraft: Java Edition");
    assert!(m.empty.as_deref().unwrap().contains("latest.log"));
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn rocket_league_how_to_when_port_closed() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    let view = rocket_league_view_from(Some(port));
    assert!(view.how_to);
    assert_eq!(view.ini_line, "PacketSendRate=30");
    assert!(view.status.contains("PacketSendRate=30"));
    assert!(view.status.contains(grokhub_core::brand::product_name()));
}
