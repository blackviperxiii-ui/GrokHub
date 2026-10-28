//! Projects and folders tests (rearch stage 5).
//!
//! Mechanical move from `tests.rs`. Wired via `#[path]` so
//! `app/mod.rs` stays untouched.

use super::*;

#[test]
fn click_other_project_stays_on_this_pane() {
    assert!(!super::click_project_opens_board(false));
}

#[test]
fn click_bound_project_stays_on_chat() {
    assert!(!super::click_project_opens_board(true));
}

#[test]
fn selected_project_stays_lit_while_filtering_chats() {
    assert!(super::project_row_active(true, true, super::Nav::Chat));
    assert!(super::project_row_active(true, true, super::Nav::Workboard));
    assert!(super::project_row_active(true, true, super::Nav::History));
    assert!(!super::project_row_active(true, false, super::Nav::Chat));
    assert!(!super::project_row_active(false, true, super::Nav::Chat));
}

#[test]
fn project_click_does_not_steal_chat_and_delete_releases_chats() {
    let src = cabin_src();
    let bind = src
        .split("fn bind_project_id(")
        .nth(1)
        .and_then(|s| s.split("fn make_project(").next())
        .expect("bind_project_id");
    assert!(
        !bind.contains("self.nav = Nav::Workboard"),
        "project click must not open the workboard: {bind}"
    );
    assert!(
        bind.contains("click_project_opens_board") && bind.contains("Nav::Chat"),
        "a project selected from the board returns to chat: {bind}"
    );
    let tree_at = bind.find("if tree_changed").expect("tree_changed");
    let halt_at = bind.find("halt_in_flight").expect("halt_in_flight");
    assert!(
        tree_at < halt_at,
        "restoring the same project filter must not halt a live reply: {bind}"
    );
    let drop_proj = src
        .split("fn remove_project_id(")
        .nth(1)
        .and_then(|s| s.split("fn apply_project_menu(").next())
        .expect("remove_project_id");
    assert!(
        drop_proj.contains("release_project_chats")
            && !drop_proj.contains("messages.clear")
            && !drop_proj.contains("threads.clear"),
        "delete must unassign chats without wiping transcripts: {drop_proj}"
    );
    let created = src
        .split("fn new_thread")
        .nth(1)
        .and_then(|s| s.split("fn begin_chat_rename").next())
        .expect("new_thread");
    assert!(
        created.contains("project_id"),
        "new chat while a project is selected must file into that folder: {created}"
    );
    let tabs = include_str!("chat_tabs.rs");
    assert!(
        tabs.contains("chat_section_indices") && !tabs.contains("project_sel"),
        "the chat tab strip lists cabin chats and does not read the project selection: {tabs}"
    );
    let activate = fn_src(&src, "activate_project_row");
    assert!(
        activate.contains("open_project_chat")
            && activate.contains("ProjectKind::Project")
            && activate.contains("n.open = !n.open")
            && activate.contains("ProjectKind::Folder"),
        "folder click only toggles open; project click opens the chat: {activate}"
    );
    assert!(
        tabs.contains("TabAct::Switch") && !tabs.contains("project_sel"),
        "clicking a chat tab must not change project selection: {tabs}"
    );
    let indices = include_str!("../threads.rs");
    assert!(
        indices.contains("fn project_section_chat_indices"),
        "project chats still have a section index helper"
    );
    let filed = fn_src(&src, "new_chat_under");
    assert!(
        filed.contains("new_thread")
            && !filed.contains("threads.clear")
            && !filed.contains("messages.clear"),
        "a new chat under a folder must not wipe History: {filed}"
    );
    let row = include_str!("../threads.rs");
    assert!(
        row.contains("fn project_folder_history_row") && row.contains("empty_chat_draft"),
        "project History rows must use empty_chat_draft"
    );
    let chat_fn = row
        .split("fn chat_section_indices(")
        .nth(1)
        .and_then(|s| s.split("pub fn project_section_chat_indices").next())
        .expect("chat_section_indices");
    assert!(
        !chat_fn.contains("selected"),
        "the chat section has no project filter argument: {chat_fn}"
    );
}

#[test]
fn folder_click_toggles_and_project_click_opens_the_chat() {
    let _g = crate::config::hold_test_config();
    let (root, mut cabin) = isolated_cabin("project-row");
    cabin.projects = vec![
        ProjectNode {
            id: "fold".into(),
            name: "Apps".into(),
            kind: ProjectKind::Folder,
            path: String::new(),
            parent: None,
            open: false,
        },
        ProjectNode {
            id: "proj".into(),
            name: "Lab".into(),
            kind: ProjectKind::Project,
            path: "/tmp/grokhub-proof-lab".into(),
            parent: Some("fold".into()),
            open: false,
        },
    ];
    let before = cabin.threads.len();
    cabin.activate_project_row("fold");
    assert!(cabin.projects.iter().find(|n| n.id == "fold").unwrap().open);
    assert_eq!(
        cabin.threads.len(),
        before,
        "a folder click does not open a chat"
    );

    cabin.activate_project_row("proj");
    assert!(matches!(cabin.nav, Nav::Chat));
    assert!(
        cabin
            .threads
            .iter()
            .any(|t| t.project_id.as_deref() == Some("proj")),
        "a project click opens that project's chat"
    );
    assert!(
        cabin.projects.iter().find(|n| n.id == "fold").unwrap().open,
        "opening a project chat does not collapse the folder"
    );
    release_isolated(&root, cabin);
}

#[test]
fn make_folder_saves_harbor_notes() {
    let _lock = crate::config::hold_test_config();
    let cfg = IsolatedConfig::arm("make-folder");
    let mut cabin = Cabin::quiet_for_test();
    cabin.make_folder("Harbor notes");
    let saved = wait_file_has(&crate::store::projects_path(), "Harbor notes");
    join_persist(&cabin.persist_svc);
    let folders: Vec<_> = cabin
        .projects
        .iter()
        .filter(|n| n.kind == ProjectKind::Folder && n.name == "Harbor notes")
        .collect();
    assert_eq!(folders.len(), 1, "one folder row named Harbor notes");
    assert!(
        cabin.status.starts_with("Folder "),
        "status {status}",
        status = cabin.status
    );
    assert!(
        saved.contains("Harbor notes"),
        "projects file must contain the folder name"
    );
    drop(cfg);
}

// Landed from PR #97.
#[test]
fn remove_project_stays_off_a_run() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("remove-project");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    cabin.remove_project_id("missing");
    assert_eq!(cabin.status, "Project not found");
    assert!(cabin.projects.is_empty());
    assert!(!cabin.running);

    cabin.projects = vec![
        ProjectNode {
            id: "f1".into(),
            name: "Notes".into(),
            kind: ProjectKind::Folder,
            path: String::new(),
            parent: None,
            open: true,
        },
        ProjectNode {
            id: "p1".into(),
            name: "Harbor".into(),
            kind: ProjectKind::Project,
            path: String::new(),
            parent: None,
            open: true,
        },
    ];
    cabin.project_sel = Some("p1".into());
    assert!(cabin.cfg.project_dir.is_empty());

    cabin.remove_project_id("p1");
    assert_eq!(cabin.status, "Removed Harbor");
    assert_eq!(cabin.projects.len(), 1);
    assert_eq!(cabin.projects[0].id, "f1");
    assert_eq!(cabin.projects[0].name, "Notes");
    assert!(matches!(cabin.projects[0].kind, ProjectKind::Folder));
    assert!(cabin.project_sel.is_none());
    assert!(cabin.cfg.project_dir.is_empty());
    assert!(!cabin.running);
}

// Landed from PR #98.
#[test]
fn rename_folder_and_move_project_into_folder() {
    let mut app = Cabin::quiet_for_test();
    app.make_folder("Harbor notes");
    let harbor_id = app
        .projects
        .iter()
        .find(|n| n.name == "Harbor notes")
        .expect("Harbor notes")
        .id
        .clone();
    app.begin_proj_rename(harbor_id.clone(), "Dock notes".to_string());
    app.finish_proj_rename();
    let renamed = app
        .projects
        .iter()
        .find(|n| n.id == harbor_id)
        .expect("renamed folder");
    assert_eq!(renamed.name, "Dock notes");
    assert_eq!(app.status, "Renamed Dock notes");

    app.move_sel_to_folder_name("Lab");
    assert_eq!(app.status, "Select a project first");

    app.make_folder("Lab");
    app.make_project("Pier", None);
    let lab_id = app
        .projects
        .iter()
        .find(|n| n.name == "Lab")
        .expect("Lab")
        .id
        .clone();
    let pier_id = app
        .projects
        .iter()
        .find(|n| n.name == "Pier")
        .expect("Pier")
        .id
        .clone();
    app.project_sel = Some(pier_id.clone());
    app.move_sel_to_folder_name("Lab");
    assert_eq!(app.status, "Added to Lab");
    let lab_open = app
        .projects
        .iter()
        .find(|n| n.id == lab_id)
        .expect("Lab")
        .open;
    assert!(lab_open);
    let pier_parent = app
        .projects
        .iter()
        .find(|n| n.id == pier_id)
        .expect("Pier")
        .parent
        .clone();
    assert_eq!(pier_parent.as_deref(), Some(lab_id.as_str()));
}

// Landed from PR #127.
#[test]
fn project_bind_show_and_clear() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("project-bind");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let home = root.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let home_s = home.display().to_string().trim_end_matches('/').to_string();
    let prev_home = std::env::var("HOME").ok();
    std::env::set_var("HOME", &home_s);
    let dock = format!("{}/dock", root.display().to_string().trim_end_matches('/'));
    let mut cabin = Cabin::quiet_for_test();
    cabin.run_slash_line("/project");
    assert_eq!(cabin.status, "No bound project");
    cabin.run_slash_line(&format!("/project bind {dock}"));
    assert_eq!(cabin.cfg.project_dir, dock);
    assert!(cabin.project_sel.is_some());
    assert_eq!(cabin.status, format!("Bound {dock}"));
    assert!(!cabin.running);
    let dir = std::path::PathBuf::from(&dock);
    let start = std::time::Instant::now();
    while !dir.is_dir() && start.elapsed() < std::time::Duration::from_secs(2) {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(dir.is_dir(), "bind did not create the project directory");
    cabin.run_slash_line("/project show");
    assert_eq!(cabin.status, format!("Project {dock}"));
    cabin.run_slash_line("/project clear");
    assert!(cabin.cfg.project_dir.is_empty());
    assert!(cabin.project_sel.is_none());
    assert_eq!(cabin.status, "Unbound — full desktop");
    assert!(!cabin.running);
    std::env::remove_var("GROKHUB_CONFIG");
    match prev_home {
        Some(h) => std::env::set_var("HOME", h),
        None => std::env::remove_var("HOME"),
    }
}

// Landed from PR #151.
#[test]
fn project_menu_rename_move_and_delete() {
    let _guard = crate::config::hold_test_config();
    let root = crate::config::test_config_root("proj-menu");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    assert!(!cabin.running);
    let work = root.join("work");
    create_folder(&mut cabin.projects, "fold-menu", "Notes", None).expect("folder");
    create_project(
        &mut cabin.projects,
        "proj-menu",
        "Harbor",
        Some("fold-menu"),
        &work.display().to_string(),
    )
    .expect("project");
    let name = cabin
        .projects
        .iter()
        .find(|n| n.id == "proj-menu")
        .expect("seeded project")
        .name
        .clone();
    assert_eq!(
        cabin
            .projects
            .iter()
            .find(|n| n.id == "proj-menu")
            .and_then(|n| n.parent.clone())
            .as_deref(),
        Some("fold-menu")
    );

    let io = cabin.persist_io.clone();
    let _held = io.lock().unwrap_or_else(|e| e.into_inner());

    cabin.apply_project_menu("proj-menu".into(), ProjectMenuAct::Rename);
    assert_eq!(cabin.proj_rename.as_deref(), Some("proj-menu"));
    assert_eq!(cabin.proj_rename_buf, name);
    assert!(!cabin.running);

    cabin.apply_project_menu("proj-menu".into(), ProjectMenuAct::AddToFolder);
    assert_eq!(cabin.proj_add_for.as_deref(), Some("proj-menu"));
    assert_eq!(cabin.project_sel.as_deref(), Some("proj-menu"));
    assert!(cabin.proj_ignore_close);
    assert!(!cabin.running);

    cabin.apply_project_menu("proj-menu".into(), ProjectMenuAct::RemoveFromFolder);
    assert!(cabin
        .projects
        .iter()
        .find(|n| n.id == "proj-menu")
        .and_then(|n| n.parent.as_ref())
        .is_none());
    assert_eq!(cabin.status, "Moved to Projects");
    assert!(!cabin.running);

    cabin.apply_project_menu("proj-menu".into(), ProjectMenuAct::Delete);
    assert!(cabin.projects.iter().all(|n| n.id != "proj-menu"));
    assert!(!cabin.running);

    let projects = cabin.projects.clone();
    let status = cabin.status.clone();
    let proj_rename = cabin.proj_rename.clone();
    let proj_rename_buf = cabin.proj_rename_buf.clone();
    let running = cabin.running;
    cabin.apply_project_menu("missing-proj".into(), ProjectMenuAct::Rename);
    assert_eq!(cabin.projects, projects);
    assert_eq!(cabin.status, status);
    assert_eq!(cabin.proj_rename, proj_rename);
    assert_eq!(cabin.proj_rename_buf, proj_rename_buf);
    assert_eq!(cabin.running, running);
    assert!(!cabin.running);

    std::thread::sleep(std::time::Duration::from_millis(150));
    drop(_held);
    std::thread::sleep(std::time::Duration::from_millis(150));
    let _done = io.lock().unwrap_or_else(|e| e.into_inner());
    drop(_done);
    let _ = std::fs::remove_dir_all(&root);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #187.
#[test]
fn project_show_and_clear_stay_off_a_run() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("project-show");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    assert!(cabin.cfg.project_dir.is_empty());
    assert!(!cabin.running);

    cabin.run_slash(Slash::ProjectShow);
    assert_eq!(cabin.status, "No bound project");
    assert!(!cabin.running);

    cabin.cfg.project_dir = r"D:\Work\Harbor".to_string();
    cabin.run_slash(Slash::ProjectShow);
    assert_eq!(cabin.status, r"Project D:\Work\Harbor");
    assert!(!cabin.running);

    cabin.new_thread(false);
    {
        let thread = cabin
            .threads
            .get_mut(cabin.thread_idx)
            .expect("seeded thread");
        thread.grok_session = Some("sess".into());
        thread.grok_cwd = Some(r"D:\Work\Harbor".into());
    }
    cabin.project_sel = Some("bound".into());

    cabin.run_slash(Slash::ProjectClear);
    assert!(cabin.cfg.project_dir.is_empty());
    assert_eq!(cabin.project_sel, None);
    let thread = cabin.threads.get(cabin.thread_idx).expect("seeded thread");
    assert_eq!(thread.grok_session, None);
    assert_eq!(thread.grok_cwd, None);
    assert_eq!(cabin.status, "Unbound — full desktop");
    assert!(!cabin.running);
}

// Landed from PR #190.
#[test]
fn project_acts_need_a_selection() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("project-need-sel");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = super::Cabin::quiet_for_test();
    assert!(cabin.project_sel.is_none());
    assert!(cabin.projects.is_empty());
    assert!(!cabin.running);

    cabin.run_slash(Slash::ProjectRename("Harbor".into()));
    assert_eq!(cabin.status, "Select a project first");
    assert!(cabin.project_sel.is_none());
    assert!(cabin.projects.is_empty());
    assert!(!cabin.running);

    cabin.run_slash(Slash::ProjectMove("Notes".into()));
    assert_eq!(cabin.status, "Select a project first");
    assert!(cabin.project_sel.is_none());
    assert!(cabin.projects.is_empty());
    assert!(!cabin.running);

    cabin.run_slash(Slash::ProjectDelete);
    assert_eq!(cabin.status, "Select a project first");
    assert!(cabin.project_sel.is_none());
    assert!(cabin.projects.is_empty());
    assert!(!cabin.running);

    let _ = std::fs::remove_dir_all(&root);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #192.
#[test]
fn stage_new_folder_asks_for_a_name() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("stage-folder");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    assert!(!cabin.running);
    cabin.stage_new_folder();

    assert_eq!(cabin.projects.len(), 1);
    let id = cabin.projects[0].id.clone();
    assert_eq!(cabin.projects[0].name, "Folder");
    assert!(matches!(cabin.projects[0].kind, ProjectKind::Folder));
    assert!(!cabin
        .projects
        .iter()
        .any(|n| matches!(n.kind, ProjectKind::Project)));
    assert_eq!(cabin.proj_staged.as_deref(), Some(id.as_str()));
    assert_eq!(cabin.proj_rename.as_deref(), Some(id.as_str()));
    assert_eq!(cabin.status, "Name this folder");
    assert!(!cabin.running);
}

// Landed from PR #194.
#[test]
fn cancel_staged_folder_drops_it() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("cancel-folder");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    assert!(!cabin.running);
    cabin.stage_new_folder();
    assert_eq!(cabin.projects.len(), 1);
    assert!(matches!(cabin.projects[0].kind, super::ProjectKind::Folder));
    assert_eq!(cabin.projects[0].name, "Folder");
    assert!(cabin.proj_staged.is_some());
    assert_eq!(cabin.status, "Name this folder");
    assert!(!cabin.running);

    cabin.cancel_proj_rename();
    assert!(cabin.projects.is_empty());
    assert!(cabin.proj_staged.is_none());
    assert!(cabin.proj_rename.is_none());
    assert_eq!(cabin.status, "Name this folder");
    assert!(!cabin.running);
}

// Landed from PR #196.
#[test]
fn finish_staged_folder_renames_it() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("finish-folder");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    assert!(!cabin.running);

    cabin.stage_new_folder();
    cabin.proj_rename_buf.clear();
    cabin.finish_proj_rename();
    assert_eq!(cabin.status, "need a name");
    assert!(cabin.projects.is_empty());
    assert!(cabin.proj_staged.is_none());
    assert!(!cabin.running);

    cabin.stage_new_folder();
    cabin.proj_rename_buf = "Harbor".into();
    cabin.finish_proj_rename();
    assert_eq!(cabin.status, "Renamed Harbor");
    assert!(
        matches!(
            cabin.projects.as_slice(),
            [node]
                if node.name == "Harbor"
                    && node.path.is_empty()
                    && matches!(node.kind, grokhub_core::ProjectKind::Folder)
        ),
        "status={} projects={}",
        cabin.status,
        cabin.projects.len()
    );
    assert!(cabin
        .projects
        .iter()
        .all(|node| !matches!(node.kind, grokhub_core::ProjectKind::Project)));
    assert!(cabin.proj_staged.is_none());
    assert!(cabin.proj_rename.is_none());
    assert!(!cabin.running);
}

fn settle_project_flush(cabin: &Cabin) {
    let start = std::time::Instant::now();
    loop {
        if cabin.persist_io.try_lock().is_err() {
            let _guard = cabin.persist_io.lock().unwrap_or_else(|e| e.into_inner());
            return;
        }
        if start.elapsed() > std::time::Duration::from_millis(500) {
            return;
        }
        std::thread::yield_now();
    }
}

#[test]
fn make_folder_names_a_folder() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("make-folder");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = quiet_cabin();
    assert!(!cabin.running);
    assert!(cabin.projects.is_empty());

    cabin.make_folder("");
    assert_eq!(cabin.status, "need a folder name");
    assert!(cabin.projects.is_empty());
    assert!(!cabin.running);

    cabin.make_folder("Harbor");
    assert_eq!(cabin.status, "Folder Harbor");
    assert_eq!(cabin.projects.len(), 1);
    assert!(matches!(
        cabin.projects[0].kind,
        grokhub_core::ProjectKind::Folder
    ));
    assert_eq!(cabin.projects[0].name, "Harbor");
    assert!(cabin.projects[0].path.is_empty());
    assert!(!cabin
        .projects
        .iter()
        .any(|n| matches!(n.kind, grokhub_core::ProjectKind::Project)));
    assert!(!cabin.running);

    settle_project_flush(&cabin);
    std::env::remove_var("GROKHUB_CONFIG");
}

// Landed from PR #198.
#[test]
fn new_chat_under_folder_stays_off_a_run() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("folder-chat");
    let _ = std::fs::remove_dir_all(&root);
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    cabin.projects.push(ProjectNode {
        id: "f1".into(),
        name: "Notes".into(),
        kind: ProjectKind::Folder,
        path: String::new(),
        parent: None,
        open: false,
    });
    cabin.apply_project_menu("f1".into(), ProjectMenuAct::NewChat);

    assert_eq!(cabin.status, "New chat");
    assert_eq!(cabin.projects.len(), 1);
    let folder = &cabin.projects[0];
    assert_eq!(folder.id, "f1");
    assert_eq!(folder.name, "Notes");
    assert!(folder.path.is_empty());
    assert!(folder.parent.is_none());
    assert!(folder.open);
    assert!(matches!(folder.kind, ProjectKind::Folder));
    assert!(cabin
        .projects
        .iter()
        .all(|n| matches!(n.kind, ProjectKind::Folder)));
    assert_eq!(cabin.project_sel.as_deref(), Some("f1"));
    assert!(matches!(cabin.nav, Nav::Chat));
    assert!(cabin.composer_want_focus);
    assert!(!cabin.running);
    assert!(cabin.cfg.goal_pin.is_empty());
    let thread = cabin.threads.get(cabin.thread_idx).expect("current thread");
    assert_eq!(thread.title, "Chat");
    assert!(!thread.scratch);
    assert_eq!(thread.project_id.as_deref(), Some("f1"));

    std::env::remove_var("GROKHUB_CONFIG");
    let _ = std::fs::remove_dir_all(&root);
}

// Landed from PR #199.
#[test]
fn remove_from_folder_stays_off_a_run() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("remove-folder");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    cabin.projects = vec![
        ProjectNode {
            id: "f1".into(),
            name: "Notes".into(),
            kind: ProjectKind::Folder,
            path: String::new(),
            parent: None,
            open: true,
        },
        ProjectNode {
            id: "p1".into(),
            name: "Harbor".into(),
            kind: ProjectKind::Project,
            path: String::new(),
            parent: Some("f1".into()),
            open: true,
        },
    ];

    let status = cabin.status.clone();
    cabin.apply_project_menu("f1".into(), ProjectMenuAct::RemoveFromFolder);
    assert_eq!(cabin.status, status);
    let harbor = cabin
        .projects
        .iter()
        .find(|n| n.id == "p1")
        .expect("harbor");
    assert_eq!(harbor.parent.as_deref(), Some("f1"));
    assert!(matches!(harbor.kind, ProjectKind::Project));
    assert!(!cabin.running);

    cabin.apply_project_menu("p1".into(), ProjectMenuAct::RemoveFromFolder);
    assert_eq!(cabin.status, "Moved to Projects");
    let harbor = cabin
        .projects
        .iter()
        .find(|n| n.id == "p1")
        .expect("harbor");
    assert!(harbor.parent.is_none());
    assert!(matches!(harbor.kind, ProjectKind::Project));
    assert!(harbor.path.is_empty());
    assert!(cabin
        .projects
        .iter()
        .any(|n| { n.id == "f1" && n.name == "Notes" && matches!(n.kind, ProjectKind::Folder) }));
    assert_eq!(
        cabin
            .projects
            .iter()
            .filter(|n| matches!(n.kind, ProjectKind::Project))
            .count(),
        1
    );
    assert!(!cabin.running);
}

// Landed from PR #200.
#[test]
fn add_to_folder_stays_off_a_run() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("add-folder");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.projects.push(ProjectNode {
        id: "p1".into(),
        name: "Harbor".into(),
        kind: ProjectKind::Project,
        path: String::new(),
        parent: None,
        open: true,
    });
    cabin.project_sel = Some("p1".into());

    cabin.move_sel_to_folder_name("Notes");
    assert_eq!(cabin.status, "No folder Notes");
    let harbor = cabin
        .projects
        .iter()
        .find(|n| n.id == "p1")
        .expect("harbor");
    assert!(matches!(harbor.kind, ProjectKind::Project));
    assert!(harbor.parent.is_none());
    assert!(!cabin.running);

    cabin.projects.push(ProjectNode {
        id: "f1".into(),
        name: "Notes".into(),
        kind: ProjectKind::Folder,
        path: String::new(),
        parent: None,
        open: false,
    });
    cabin.move_sel_to_folder_name("Notes");
    assert_eq!(cabin.status, "Added to Notes");
    let harbor = cabin
        .projects
        .iter()
        .find(|n| n.id == "p1")
        .expect("harbor");
    assert!(matches!(harbor.parent.as_deref(), Some("f1")));
    assert!(harbor.path.is_empty());
    let folder = cabin.projects.iter().find(|n| n.id == "f1").expect("notes");
    assert!(matches!(folder.kind, ProjectKind::Folder));
    assert!(folder.open);
    let projects = cabin
        .projects
        .iter()
        .filter(|n| matches!(n.kind, ProjectKind::Project))
        .count();
    let folders = cabin
        .projects
        .iter()
        .filter(|n| matches!(n.kind, ProjectKind::Folder))
        .count();
    assert_eq!((projects, folders), (1, 1));
    assert!(!cabin.running);
}

// Landed from PR #201.
#[test]
fn bind_empty_project_stays_off_a_run() {
    let _cfg = crate::config::hold_test_config();
    let root = crate::config::test_config_root("bind-empty");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    cabin.projects = vec![
        ProjectNode {
            id: "f1".into(),
            name: "Notes".into(),
            kind: ProjectKind::Folder,
            path: String::new(),
            parent: None,
            open: true,
        },
        ProjectNode {
            id: "p1".into(),
            name: "Harbor".into(),
            kind: ProjectKind::Project,
            path: String::new(),
            parent: None,
            open: true,
        },
    ];
    assert!(!cabin.running);

    let status = cabin.status.clone();
    let project_sel = cabin.project_sel.clone();
    let projects = cabin.projects.clone();

    cabin.bind_project_id("missing");
    assert_eq!(cabin.status, status);
    assert_eq!(cabin.project_sel, project_sel);
    assert!(cabin.projects == projects);
    assert!(!cabin.running);

    cabin.bind_project_id("f1");
    assert_eq!(cabin.status, status);
    assert_eq!(cabin.project_sel, project_sel);
    assert!(cabin.projects == projects);
    assert!(!cabin.running);
    let folder = cabin
        .projects
        .iter()
        .find(|n| n.id == "f1")
        .expect("folder");
    assert!(matches!(folder.kind, ProjectKind::Folder) && folder.open);

    let threads = cabin.threads.len();
    cabin.nav = Nav::Workboard;
    cabin.bind_project_id("p1");
    assert_eq!(cabin.status, "Bound Harbor");
    assert_eq!(cabin.project_sel.as_deref(), Some("p1"));
    assert!(cabin.cfg.project_dir.is_empty());
    assert!(matches!(cabin.nav, Nav::Chat));
    assert!(!cabin.running);
    let folder = cabin
        .projects
        .iter()
        .find(|n| n.id == "f1")
        .expect("folder");
    assert!(matches!(folder.kind, ProjectKind::Folder) && folder.open);
    assert_eq!(
        cabin
            .projects
            .iter()
            .filter(|n| matches!(n.kind, ProjectKind::Project))
            .count(),
        1
    );
    assert_eq!(cabin.threads.len(), threads);
}

// Landed from PR #202.
#[test]
fn remove_project_returns_chats() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("remove-chats");
    let prev = std::env::var("GROKHUB_CONFIG").ok();
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    assert!(cabin.cfg.project_dir.is_empty());
    assert!(!cabin.running);

    cabin.projects.push(ProjectNode {
        id: "p1".into(),
        name: "Harbor".into(),
        kind: ProjectKind::Project,
        path: String::new(),
        parent: None,
        open: true,
    });
    assert!(cabin.projects.iter().any(|n| {
        n.id == "p1"
            && n.name == "Harbor"
            && n.path.is_empty()
            && n.parent.is_none()
            && n.open
            && matches!(n.kind, ProjectKind::Project)
    }));

    if cabin.threads.is_empty() {
        cabin.new_thread(false);
    }
    let idx = cabin.thread_idx;
    let thread_id = cabin.threads[idx].id.clone();
    cabin.threads[idx].project_id = Some("p1".into());

    cabin.remove_project_id("p1");

    assert_eq!(cabin.status, "Removed Harbor · chats back in History");
    assert!(cabin.projects.is_empty());
    let thread = cabin
        .threads
        .iter()
        .find(|t| t.id == thread_id)
        .expect("current thread");
    assert!(thread.project_id.is_none());
    assert!(!cabin.running);
    assert!(cabin.cfg.project_dir.is_empty());

    match prev {
        Some(v) => std::env::set_var("GROKHUB_CONFIG", v),
        None => std::env::remove_var("GROKHUB_CONFIG"),
    }
}

// Landed from PR #204.
#[test]
fn remove_bound_project_unbinds() {
    let _guard = crate::config::hold_test_config();
    let root = crate::config::test_config_root("unbind-remove");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    let path = "/tmp/grokhub-unbound-harbor";
    cabin.projects.push(ProjectNode {
        id: "p1".into(),
        name: "Harbor".into(),
        kind: ProjectKind::Project,
        path: path.into(),
        parent: None,
        open: true,
    });
    assert!(matches!(cabin.projects[0].kind, ProjectKind::Project));
    cabin.cfg.project_dir = path.into();
    cabin.project_sel = Some("p1".into());
    assert!(cabin
        .threads
        .iter()
        .all(|t| !matches!(t.project_id.as_deref(), Some("p1"))));
    assert!(!cabin.running);

    cabin.remove_project_id("p1");

    assert_eq!(cabin.status, "Removed Harbor · unbound");
    assert!(cabin.projects.is_empty());
    assert!(cabin.project_sel.is_none());
    assert!(cabin.cfg.project_dir.is_empty());
    assert!(!cabin.running);
}

// Landed from PR #206.
#[test]
fn remove_folder_returns_its_project() {
    let _g = crate::config::hold_test_config();
    let root = crate::config::test_config_root("drop-folder");
    std::env::set_var("GROKHUB_CONFIG", &root);
    let mut cabin = Cabin::quiet_for_test();
    cabin.projects = vec![
        ProjectNode {
            id: "f1".into(),
            name: "Notes".into(),
            kind: ProjectKind::Folder,
            path: String::new(),
            parent: None,
            open: true,
        },
        ProjectNode {
            id: "p1".into(),
            name: "Harbor".into(),
            kind: ProjectKind::Project,
            path: String::new(),
            parent: Some("f1".into()),
            open: true,
        },
    ];
    assert!(cabin.cfg.project_dir.is_empty());
    assert!(cabin.project_sel.is_none());
    assert!(!cabin
        .threads
        .iter()
        .any(|t| t.project_id.as_deref() == Some("f1")));
    cabin.remove_project_id("f1");
    assert_eq!(cabin.status, "Removed Notes");
    assert!(cabin.projects.iter().all(|n| n.id != "f1"));
    assert!(!cabin
        .projects
        .iter()
        .any(|n| matches!(n.kind, ProjectKind::Folder)));
    assert_eq!(cabin.projects.len(), 1);
    let harbor = &cabin.projects[0];
    assert_eq!(harbor.id, "p1");
    assert_eq!(harbor.name, "Harbor");
    assert!(matches!(harbor.kind, ProjectKind::Project));
    assert!(harbor.path.is_empty());
    assert!(harbor.parent.is_none());
    assert!(!cabin.running);
    assert!(cabin.cfg.project_dir.is_empty());
}

// Landed from PR #227.
#[test]
fn stage_project_asks_for_a_name() {
    let mut app = Cabin::quiet_for_test();
    let before = app.projects.len();
    app.stage_new_folder();
    assert_eq!(app.status, "Name this folder");
    assert_eq!(app.projects.len(), before + 1);
    let id = app.proj_staged.clone().expect("staged");
    assert_eq!(app.proj_rename.as_deref(), Some(id.as_str()));
    assert!(app.proj_rename_buf.is_empty());
    assert!(app.proj_rename_focus);
    assert!(app.proj_rename_lock.is_none());
    let node = app.projects.iter().find(|n| n.id == id).expect("node");
    assert_eq!(node.name, "Folder");
    assert!(node.path.is_empty());
    assert_eq!(node.kind, ProjectKind::Folder);
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
}

// Landed from PR #228.
#[test]
fn blank_project_name_drops_it() {
    let mut app = Cabin::quiet_for_test();
    app.stage_new_folder();
    let id = app.proj_staged.clone().expect("staged");
    app.finish_proj_rename();
    assert_eq!(app.status, "need a name");
    assert!(app.proj_staged.is_none());
    assert!(app.proj_rename.is_none());
    assert!(!app.proj_rename_focus);
    assert!(app.projects.iter().all(|n| n.id != id));
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
}

// Landed from PR #230.
#[test]
fn cancel_project_drops_the_staged_one() {
    let mut app = Cabin::quiet_for_test();
    app.stage_new_folder();
    let id = app.proj_staged.clone().expect("staged");
    app.cancel_proj_rename();
    assert!(app.proj_staged.is_none());
    assert!(app.proj_rename.is_none());
    assert!(!app.proj_rename_focus);
    assert!(app.proj_rename_lock.is_none());
    assert!(app.proj_rename_buf.is_empty());
    assert!(app.projects.iter().all(|n| n.id != id));
    assert_eq!(app.status, "Name this folder");
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
}

/// Projects “+” paints Cancel next to Name. That control calls the same
/// `cancel_proj_rename` Esc uses, which drops the staged folder.
#[test]
fn projects_add_cancel_drops_the_staged_folder() {
    let _hold = crate::config::hold_test_config();
    let root = crate::config::test_config_root("proj-add-cancel");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("config root");
    std::env::set_var("GROKHUB_CONFIG", &root);

    let mut cabin = Cabin::quiet_for_test();
    cabin.stage_new_folder();
    let id = cabin.proj_staged.clone().expect("staged folder");
    assert_eq!(cabin.projects.len(), 1);
    assert_eq!(cabin.projects[0].name, "Folder");
    assert_eq!(cabin.projects[0].kind, ProjectKind::Folder);
    assert_eq!(cabin.proj_rename.as_deref(), Some(id.as_str()));
    assert_eq!(cabin.status, "Name this folder");

    cabin.cancel_proj_rename();
    assert!(cabin.projects.is_empty(), "Cancel drops the staged folder");
    assert!(cabin.projects.iter().all(|n| n.id != id));
    assert!(cabin.proj_staged.is_none());
    assert!(cabin.proj_rename.is_none());
    assert!(cabin.proj_rename_buf.is_empty());
    assert!(!cabin.proj_rename_focus);
    assert!(cabin.proj_rename_lock.is_none());
    assert_eq!(cabin.status, "Name this folder");
    assert!(!cabin.running);

    let side = include_str!("sidebar.rs");
    let row = side
        .split("hint_text(crate::theme::hint(\"Name\"))")
        .nth(1)
        .and_then(|s| s.split("self.finish_proj_rename()").next())
        .expect("proj rename row");
    assert!(
        row.contains("ghost_pill(ui, \"Cancel\")"),
        "the rename row paints Cancel beside Name: {row}"
    );
    assert!(
        row.contains("egui::Key::Escape"),
        "Esc still cancels the rename: {row}"
    );
    assert_eq!(
        row.matches("self.cancel_proj_rename()").count(),
        2,
        "Cancel click and Esc both call cancel_proj_rename: {row}"
    );
}

/// Name and Cancel share one centered row: pill height, filled field, white focus stroke.
#[test]
fn projects_rename_row_lines_up_with_cancel() {
    let side = include_str!("sidebar.rs");
    let row = side
        .split("if self.proj_rename.as_deref()")
        .nth(1)
        .and_then(|s| s.split("self.finish_proj_rename()").next())
        .expect("proj rename row");
    assert!(
        row.contains("Layout::left_to_right(egui::Align::Center)"),
        "rename row is vertically centered: {row}"
    );
    assert!(
        row.contains(".min_size(egui::vec2(edit_w, 30.0))"),
        "Name field is the pill height: {row}"
    );
    assert!(
        row.contains("Margin::symmetric(") && row.contains("10, 6"),
        "Name field has ~10px left padding: {row}"
    );
    assert!(
        row.contains("crate::theme::elevated()"),
        "Name field uses the filter fill: {row}"
    );
    assert!(
        row.contains("corner_radius(15.0)"),
        "Name field is full radius: {row}"
    );
    assert!(
        row.contains("crate::theme::muted()"),
        "Name hint is muted: {row}"
    );
    assert!(
        row.contains("2.0_f32") && row.contains("crate::theme::fg()"),
        "staged Name field draws a white focus stroke: {row}"
    );
    assert!(
        row.contains("text_cursor.stroke"),
        "staged Name field shows a caret: {row}"
    );
    assert!(
        row.contains("edit.request_focus()"),
        "staged Name field takes focus: {row}"
    );
    assert!(
        row.contains("Layout::right_to_left(egui::Align::Center)"),
        "Cancel's right edge meets the rail inset: {row}"
    );
    assert!(row.contains("cancel_w"), "{row}");
}

// Landed from PR #231.
#[test]
fn make_project_keeps_harbor() {
    let mut app = Cabin::quiet_for_test();
    let before = app.projects.len();
    app.make_project("Harbor", None);
    assert_eq!(app.status, "Project Harbor");
    assert_eq!(app.projects.len(), before + 1);
    let node = app
        .projects
        .iter()
        .find(|n| n.name == "Harbor")
        .expect("harbor");
    assert_eq!(node.kind, ProjectKind::Project);
    assert!(!node.path.trim().is_empty());
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
}

// Landed from PR #232.
#[test]
fn empty_project_name_is_refused() {
    let mut app = Cabin::quiet_for_test();
    let before = app.projects.len();
    app.make_project("   ", None);
    assert_eq!(app.status, "need a project name");
    assert_eq!(app.projects.len(), before);
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
}

// Folded from PR #436.
#[test]
fn name_project_keeps_harbor() {
    let mut app = Cabin::quiet_for_test();
    app.stage_new_folder();
    let id = app.proj_staged.clone().expect("staged");
    app.proj_rename_buf = "Harbor".into();
    app.finish_proj_rename();
    assert_eq!(app.status, "Renamed Harbor");
    assert!(app.proj_staged.is_none());
    assert!(app.proj_rename.is_none());
    assert!(!app.proj_rename_focus);
    assert!(app.proj_rename_buf.is_empty());
    let node = app.projects.iter().find(|n| n.id == id).expect("named");
    assert_eq!(node.name, "Harbor");
    assert_eq!(node.kind, ProjectKind::Folder);
    assert!(node.path.is_empty());
    assert!(!app.running);
    assert!(app.chat_job_thread.is_none());
}
