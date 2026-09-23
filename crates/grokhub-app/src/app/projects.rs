//! Bound project tree and overlays.

use super::*;

impl Cabin {

    pub(super) fn work_root(&self) -> String {
        let home = std::env::var("HOME").ok();
        let profile = std::env::var("USERPROFILE").ok();
        grokhub_core::cabin_work_root(cfg!(windows), home.as_deref(), profile.as_deref())
    }

    pub(super) fn touch_projects(&mut self) {
        self.projects_dirty = true;
    }

    pub(super) fn flush_projects(&mut self) {
        if !self.projects_dirty {
            return;
        }
        self.projects_dirty = false;
        let nodes = self.projects.clone();
        let io = self.persist_io.clone();
        std::thread::spawn(move || {
            if let Ok(_g) = io.lock() {
                let _ = crate::store::save_projects(&nodes);
            }
        });
    }

    pub(super) fn history_folder_label(&self) -> String {
        let Some(id) = self.project_sel.as_deref() else {
            return "History".into();
        };
        self.projects
            .iter()
            .find(|n| n.id == id)
            .map(|n| format!("History · {}", n.name))
            .unwrap_or_else(|| "History".into())
    }

    pub(super) fn bind_project_id(&mut self, id: &str) {
        let Some(n) = self
            .projects
            .iter()
            .find(|n| n.id == id && n.kind == ProjectKind::Project)
        else {
            return;
        };
        let path = expand_home(&n.path);
        let name = n.name.clone();
        let tree_changed = !path.trim().is_empty() && self.cfg.project_dir != path;
        if !path.trim().is_empty() {
            if !std::path::Path::new(&path).is_dir() {
                let p = path.clone();
                std::thread::spawn(move || {
                    let _ = std::fs::create_dir_all(&p);
                });
            }
            self.cfg.project_dir = path.clone();
        }
        let already = self.project_sel.as_deref() == Some(id);
        self.project_sel = Some(id.to_string());
        assert!(
            !click_project_opens_board(already),
            "project click must not open the workboard"
        );
        if self.nav == Nav::Workboard {
            self.nav = Nav::Chat;
        }
        self.status = format!("Bound {name}");
        if self.running {
            self.halt_in_flight();
        }
        self.acp = None;
        self.acp_spawn_rx = None;
        if tree_changed {
            if let Some(t) = self.threads.get_mut(self.thread_idx) {
                t.grok_cwd = None;
                t.grok_session = None;
            }
            self.persist();
        } else {
            self.persist_cfg();
        }
    }

    pub(super) fn make_project(&mut self, name: &str, parent: Option<&str>) {
        let id = uid("proj");
        let root = self.work_root();
        match create_project(&mut self.projects, &id, name, parent, &root) {
            Ok(i) => {
                let path = self.projects[i].path.clone();
                if !std::path::Path::new(&path).is_dir() {
                    let p = path.clone();
                    std::thread::spawn(move || {
                        let _ = std::fs::create_dir_all(&p);
                    });
                }
                self.touch_projects();
                self.bind_project_id(&id);
                self.status = format!("Project {}", self.projects[i].name);
            }
            Err(e) => self.status = e.into(),
        }
    }

    pub(super) fn remove_project_id(&mut self, id: &str) {
        let bound = self.cfg.project_dir.clone();
        let selected = self.project_sel.as_deref() == Some(id);
        let out = drop_selected(&mut self.projects, id, &bound);
        if !out.dropped {
            self.status = "Project not found".into();
            return;
        }
        let released = threads::release_project_chats(&mut self.threads, id);
        if out.unbound {
            self.cfg.project_dir.clear();
            if self.running {
                self.halt_in_flight();
            }
            self.acp = None;
            self.acp_spawn_rx = None;
            if let Some(t) = self.threads.get_mut(self.thread_idx) {
                t.grok_cwd = None;
                t.grok_session = None;
            }
        }
        if selected {
            self.project_sel = None;
        }
        self.touch_projects();
        if out.unbound || released > 0 {
            self.persist();
        } else {
            self.flush_projects();
        }
        self.status = if released > 0 {
            format!("Removed {} · chats back in History", out.name)
        } else if out.unbound {
            format!("Removed {} · unbound", out.name)
        } else {
            format!("Removed {}", out.name)
        };
    }

    pub(super) fn apply_project_menu(&mut self, id: String, act: ProjectMenuAct) {
        match act {
            ProjectMenuAct::Rename => {
                if let Some(n) = self.projects.iter().find(|n| n.id == id) {
                    self.begin_proj_rename(id, n.name.clone());
                }
            }
            ProjectMenuAct::AddToFolder => {
                self.proj_add_for = Some(id.clone());
                self.project_sel = Some(id);
                self.proj_ignore_close = true;
            }
            ProjectMenuAct::RemoveFromFolder => {
                if add_to_folder(&mut self.projects, &id, None).is_ok() {
                    self.status = "Moved to Projects".into();
                    self.touch_projects();
                    self.flush_projects();
                }
            }
            ProjectMenuAct::NewHere => self.stage_new_project(Some(&id)),
            ProjectMenuAct::Delete => self.remove_project_id(&id),
        }
    }

    pub(super) fn stage_new_project(&mut self, parent: Option<&str>) {
        let id = uid("proj");
        match stage_project(&mut self.projects, &id, "Project", parent) {
            Ok(_) => {
                if let Some(pid) = parent {
                    if let Some(f) = self.projects.iter_mut().find(|n| n.id == pid) {
                        f.open = true;
                    }
                }
                self.begin_proj_rename(id.clone(), String::new());
                self.proj_staged = Some(id);
                self.status = "Name this project".into();
                self.touch_projects();
                self.flush_projects();
            }
            Err(e) => self.status = e.into(),
        }
    }

    pub(super) fn make_folder(&mut self, name: &str) {
        let id = uid("fold");
        match create_folder(&mut self.projects, &id, name, None) {
            Ok(i) => {
                self.status = format!("Folder {}", self.projects[i].name);
                self.touch_projects();
                self.flush_projects();
            }
            Err(e) => self.status = e.into(),
        }
    }

    pub(super) fn stage_new_folder(&mut self) {
        let id = uid("fold");
        match create_folder(&mut self.projects, &id, "Folder", None) {
            Ok(_) => {
                self.begin_proj_rename(id.clone(), String::new());
                self.proj_staged = Some(id);
                self.status = "Name this folder".into();
                self.touch_projects();
                self.flush_projects();
            }
            Err(e) => self.status = e.into(),
        }
    }

    pub(super) fn begin_proj_rename(&mut self, id: String, buf: String) {
        self.proj_rename_lock = if buf.is_empty() {
            None
        } else {
            Some(buf.clone())
        };
        self.proj_rename_buf = buf;
        self.proj_rename = Some(id);
        self.proj_rename_focus = true;
    }

    pub(super) fn cancel_proj_rename(&mut self) {
        let id = self.proj_rename.take();
        self.proj_rename_buf.clear();
        self.proj_rename_focus = false;
        self.proj_rename_lock = None;
        if let Some(id) = id {
            if self.proj_staged.as_deref() == Some(id.as_str()) {
                drop_node(&mut self.projects, &id);
                self.touch_projects();
                self.flush_projects();
            }
        }
        self.proj_staged = None;
    }

    pub(super) fn finish_proj_rename(&mut self) {
        let Some(id) = self.proj_rename.take() else {
            return;
        };
        let staged = self.proj_staged.as_deref() == Some(id.as_str());
        match rename_node(&mut self.projects, &id, &self.proj_rename_buf) {
            Ok(()) => {
                self.status = format!("Renamed {}", self.proj_rename_buf.trim());
                self.touch_projects();
                let mut bound = false;
                if staged {
                    let root = self.work_root();
                    if let Ok(path) = settle_project_path(&mut self.projects, &id, &root) {
                        if !path.is_empty() {
                            if !std::path::Path::new(&path).is_dir() {
                                let p = path.clone();
                                std::thread::spawn(move || {
                                    let _ = std::fs::create_dir_all(&p);
                                });
                            }
                            self.bind_project_id(&id);
                            bound = true;
                        }
                    }
                }
                if !bound {
                    self.flush_projects();
                }
            }
            Err(e) => {
                if staged {
                    drop_node(&mut self.projects, &id);
                    self.touch_projects();
                    self.flush_projects();
                }
                self.status = e.into();
            }
        }
        self.proj_rename_buf.clear();
        self.proj_rename_focus = false;
        self.proj_rename_lock = None;
        self.proj_staged = None;
    }

    pub(super) fn move_sel_to_folder_name(&mut self, folder: &str) {
        let Some(pid) = self.project_sel.clone() else {
            self.status = "Select a project first".into();
            return;
        };
        if folder.eq_ignore_ascii_case("root") {
            match add_to_folder(&mut self.projects, &pid, None) {
                Ok(()) => {
                    self.status = "Moved to Projects".into();
                    self.touch_projects();
                    self.flush_projects();
                }
                Err(e) => self.status = e.into(),
            }
            return;
        }
        let fid = self
            .projects
            .iter()
            .find(|n| n.kind == ProjectKind::Folder && n.name.eq_ignore_ascii_case(folder))
            .map(|n| n.id.clone());
        let Some(fid) = fid else {
            self.status = format!("No folder {folder}");
            return;
        };
        match add_to_folder(&mut self.projects, &pid, Some(&fid)) {
            Ok(()) => {
                if let Some(f) = self.projects.iter_mut().find(|n| n.id == fid) {
                    f.open = true;
                }
                self.status = format!("Added to {folder}");
                self.touch_projects();
                self.flush_projects();
            }
            Err(e) => self.status = e.into(),
        }
    }

    pub(super) fn ui_project_overlays(&mut self, ctx: &egui::Context) {
        if self.proj_plus_open {
            let mut pick: Option<&'static str> = None;
            let mut menu_rect = egui::Rect::NOTHING;
            egui::Area::new(egui::Id::new("proj-plus"))
                .fixed_pos(self.proj_plus_pos + egui::vec2(0.0, 4.0))
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_min_width(160.0);
                        if ui.selectable_label(false, "New project").clicked() {
                            pick = Some("project");
                        }
                        if ui.selectable_label(false, "New folder").clicked() {
                            pick = Some("folder");
                        }
                        menu_rect = ui.min_rect();
                    });
                });
            if let Some(kind) = pick {
                self.proj_plus_open = false;
                match kind {
                    "project" => self.stage_new_project(None),
                    "folder" => self.stage_new_folder(),
                    _ => {}
                }
            } else if self.proj_ignore_close {
                self.proj_ignore_close = false;
            } else if ctx.input(|i| i.pointer.any_click()) {
                if let Some(pos) = ctx.pointer_interact_pos() {
                    if !menu_rect.expand(8.0).contains(pos) {
                        self.proj_plus_open = false;
                    }
                }
            }
        }
        if let Some(pid) = self.proj_add_for.clone() {
            let folders = folder_choices(&self.projects);
            let mut picked: Option<Option<String>> = None;
            let mut menu_rect = egui::Rect::NOTHING;
            egui::Area::new(egui::Id::new("proj-add"))
                .fixed_pos(self.proj_menu_pos + egui::vec2(8.0, 8.0))
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_min_width(168.0);
                        ui.label(
                            RichText::new("Add to folder")
                                .size(12.0)
                                .color(crate::theme::muted()),
                        );
                        if folders.is_empty() {
                            ui.label("Create a folder first");
                        }
                        for (fid, name) in &folders {
                            if ui.selectable_label(false, name).clicked() {
                                picked = Some(Some(fid.clone()));
                            }
                        }
                        if ui.selectable_label(false, "Projects (root)").clicked() {
                            picked = Some(None);
                        }
                        menu_rect = ui.min_rect();
                    });
                });
            if let Some(folder) = picked {
                self.proj_add_for = None;
                match add_to_folder(&mut self.projects, &pid, folder.as_deref()) {
                    Ok(()) => {
                        if let Some(fid) = folder {
                            if let Some(f) = self.projects.iter_mut().find(|n| n.id == fid) {
                                f.open = true;
                            }
                            self.status = "Added to folder".into();
                        } else {
                            self.status = "Moved to Projects".into();
                        }
                        self.touch_projects();
                        self.flush_projects();
                    }
                    Err(e) => self.status = e.into(),
                }
            } else if self.proj_ignore_close {
                self.proj_ignore_close = false;
            } else if ctx.input(|i| i.pointer.any_click()) {
                if let Some(pos) = ctx.pointer_interact_pos() {
                    if !menu_rect.expand(8.0).contains(pos) {
                        self.proj_add_for = None;
                    }
                }
            }
        }
    }
}
