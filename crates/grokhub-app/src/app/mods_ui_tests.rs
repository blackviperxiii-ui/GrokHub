use super::*;
use grokhub_core::mods::{Download, ModFile};

fn game(app_id: u32, name: &str, last_played: u64, playtime_min: u64) -> SteamGame {
    SteamGame {
        app_id,
        name: name.into(),
        last_played,
        playtime_min,
    }
}

fn bnet(product: &str, install: &str) -> BnetGame {
    BnetGame {
        product: product.into(),
        install: PathBuf::from(install),
        prefix: None,
    }
}

fn texts(cabin: &mut Cabin, draw: impl Fn(&mut Cabin, &mut egui::Ui)) -> Vec<String> {
    let ctx = egui::Context::default();
    crate::theme::install_fonts_on(&ctx);
    let mut out = Vec::new();
    for frame in 0..2 {
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1400.0, 900.0),
            )),
            time: Some(f64::from(frame) / 60.0),
            ..Default::default()
        };
        let full = crate::theme::test_pass(&ctx, raw, |ui| draw(cabin, ui));
        out.clear();
        for clipped in &full.shapes {
            collect(&clipped.shape, &mut out);
        }
    }
    out
}

fn collect(shape: &egui::Shape, out: &mut Vec<String>) {
    match shape {
        egui::Shape::Text(t) => out.push(t.galley.job.text.clone()),
        egui::Shape::Vec(v) => v.iter().for_each(|s| collect(s, out)),
        _ => {}
    }
}

#[test]
fn the_rail_lists_moddable_games_by_play_and_the_rest_under_cant_mod() {
    let steam = [
        game(
            489830,
            "The Elder Scrolls V: Skyrim Special Edition",
            100,
            50,
        ),
        game(413150, "Stardew Valley", 900, 10),
        game(294100, "RimWorld", 100, 400),
        game(1172470, "Apex Legends", 2_000, 9_000),
        game(4_000_000, "Some Puzzle Game", 50, 5),
    ];
    let bnet_games = [
        bnet("wow", "/games/wow"),
        bnet("wow_classic", "/games/wowc"),
    ];
    let rail = mod_rail(&steam, &bnet_games, true);
    let ids: Vec<&str> = rail.games.iter().map(|g| g.entry.id).collect();
    assert_eq!(
        ids,
        ["stardew", "rimworld", "skyrimse", "minecraft", "wow"],
        "recent first, then playtime, then name"
    );
    let wow = rail.games.iter().find(|g| g.entry.id == "wow").unwrap();
    assert_eq!(wow.installs.len(), 2, "both WoW installs, one row");
    let cant: Vec<(&str, &str)> = rail
        .cant
        .iter()
        .map(|c| (c.name.as_str(), c.reason.as_str()))
        .collect();
    assert_eq!(cant.len(), 2);
    assert_eq!(cant[0].0, "Apex Legends");
    assert!(cant[0].1.contains("Easy Anti-Cheat"), "{}", cant[0].1);
    assert_eq!(cant[1], ("Some Puzzle Game", UNKNOWN_REASON));
    assert!(mod_rail(&[], &[], false).games.is_empty());
}

#[test]
fn a_blocked_game_shows_its_reason_and_no_search_box() {
    let _g = crate::config::hold_test_config();
    let mut cabin = Cabin::quiet_for_test();
    let rail = mod_rail(&[game(1172470, "Apex Legends", 1, 1)], &[], false);
    let mut view = ModsView::default();
    view.pick("apex".into());
    let shown = texts(&mut cabin, |c, ui| {
        c.mods_pane(ui, &rail, &mut view.clone())
    });
    assert!(shown.iter().any(|t| t == "Apex Legends"), "{shown:?}");
    assert!(
        shown.iter().any(|t| t.contains("Easy Anti-Cheat")),
        "{shown:?}"
    );
    assert!(
        !shown.iter().any(|t| t.contains("Search mods")),
        "{shown:?}"
    );
    assert!(!shown.iter().any(|t| t == "POPULAR"), "{shown:?}");
}

#[test]
fn offline_only_greys_search_and_keeps_installed() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("mods-offline");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);
    let _off = grokhub_core::offline::force_for_thread(true);
    let mut cabin = Cabin::quiet_for_test();
    let rail = mod_rail(&[game(413150, "Stardew Valley", 1, 1)], &[], false);
    let mut view = ModsView::default();
    view.pick("stardew".into());
    let shown = texts(&mut cabin, |c, ui| {
        c.mods_pane(ui, &rail, &mut view.clone())
    });
    assert!(
        shown.iter().any(|t| t == "Offline only is on"),
        "search hint: {shown:?}"
    );
    assert!(
        shown
            .iter()
            .any(|t| t.contains("Installed mods still show")),
        "{shown:?}"
    );
    assert!(shown.iter().any(|t| t == "INSTALLED"), "{shown:?}");
    assert!(
        shown.iter().any(|t| t.contains("Singleplayer")),
        "mode note: {shown:?}"
    );
    std::env::remove_var("GROKHUB_CONFIG");
}

#[test]
fn adult_results_hide_unless_18_plus_is_on() {
    let page = Page {
        mods: vec![
            ModSummary {
                id: "1".into(),
                name: "Plain".into(),
                ..Default::default()
            },
            ModSummary {
                id: "2".into(),
                name: "Adult".into(),
                adult: true,
                ..Default::default()
            },
        ],
        total: Some(2),
    };
    let names = |ok| {
        visible_mods(&page, ok)
            .iter()
            .map(|m| m.name.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(names(false), ["Plain"]);
    assert_eq!(names(true), ["Plain", "Adult"]);
    let mut cabin = Cabin::quiet_for_test();
    cabin.cfg.explicit_personalities = true;
    assert!(!cabin.adult_ok(), "18+ also needs the age confirmed");
    cabin.cfg.adult_confirmed = true;
    assert!(cabin.adult_ok());
}

#[test]
fn mods_has_its_own_page_id() {
    let mut cabin = Cabin::quiet_for_test();
    cabin.set_nav_id("mods");
    assert_eq!(cabin.nav, Nav::Mods);
    assert_eq!(cabin.nav_id(), "mods");
    assert_eq!(Cabin::nav_from_id("mods"), Nav::Mods);
    cabin.nav = Nav::Chat;
    cabin.select_main_nav("mods");
    assert_eq!(cabin.nav, Nav::Mods);
    assert!(crate::theme::LOADOUT_NAV.contains(&("mods", "Mods")));
    let ctx = egui::Context::default();
    cabin.nav = Nav::Library;
    cabin.show_in_mods(&ctx, 1172470);
    assert_eq!(cabin.nav, Nav::Mods);
    let view: ModsView = ctx
        .data(|d| d.get_temp(egui::Id::new(VIEW_ID)))
        .expect("view");
    assert_eq!(view.picked.as_deref(), Some("apex"));
    assert!(view.show_cant, "a blocked pick opens Can't mod");
}

#[test]
fn the_query_waits_for_typing_to_pause() {
    let mut view = ModsView {
        query: "sodium".into(),
        typed_at: now_ms(),
        ..Default::default()
    };
    assert_eq!(view.query(), Query::browse(Browse::Popular));
    view.searched = "sodium".into();
    view.tab = Tab::Updated;
    view.page = 2;
    let q = view.query();
    assert_eq!(q.text(), Some("sodium"));
    assert_eq!(q.browse, Browse::Updated);
    assert_eq!(q.page, 2);
}

#[test]
fn installed_reads_workshop_items_and_wow_addons() {
    let root = std::env::temp_dir().join(format!("mods-installed-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let steam = root.join("steam");
    let ws = steam.join("steamapps/workshop");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::write(
        ws.join("appworkshop_294100.acf"),
        r#""AppWorkshop" { "WorkshopItemsInstalled" { "818773962" { "size" "2500000" "timeupdated" "5" } } }"#,
    )
    .unwrap();
    let rim = mod_rail(&[game(294100, "RimWorld", 1, 1)], &[], false).games[0].clone();
    let rows = installed_mods(&rim, Some(&steam));
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].name, "Workshop item 818773962");
    assert_eq!(rows[0].detail, "2.5 MB, from Steam Workshop");
    assert_eq!(
        rows[0].link.as_deref(),
        Some("steam://url/CommunityFilePage/818773962")
    );
    let wow_dir = root.join("wow");
    let addons = wow_dir.join("_retail_/Interface/AddOns");
    for name in ["Details", "Blizzard_Debug", "WeakAuras"] {
        std::fs::create_dir_all(addons.join(name)).unwrap();
    }
    let wow = mod_rail(&[], &[bnet("wow", wow_dir.to_str().unwrap())], false).games[0].clone();
    let rows = installed_mods(&wow, None);
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["Details", "WeakAuras"]);
    assert_eq!(rows[0].detail, "Addon, Retail");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn counts_sizes_and_files_read_short() {
    assert_eq!(short_count(980), "980");
    assert_eq!(short_count(34_500), "34K");
    assert_eq!(short_count(1_250_000), "1.2M");
    assert_eq!(drawer::size_label(880_000), "880 KB");
    assert_eq!(drawer::size_label(4_200_000), "4.2 MB");
    let f = ModFile {
        id: "1".into(),
        name: "Main".into(),
        file_name: "main.zip".into(),
        version: "1.2".into(),
        size: 4_200_000,
        hash: None,
        download: Download::Browser("https://www.nexusmods.com/x".into()),
        game_versions: vec![],
        deps: vec![],
        primary: true,
        uploaded: 0,
    };
    assert_eq!(drawer::file_line(&f), "1.2 · 4.2 MB · download on the site");
    assert!(open_link("file:///etc/passwd").is_err());
}
