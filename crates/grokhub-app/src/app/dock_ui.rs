//! U7f: optional egui_dock main-panel tabs (Settings → Dockable panels; off by default).
//! Layout tree persists in `dock.json`. Fixed sidebar + page CentralPanel path stays when off.

use eframe::egui;
use egui_dock::{DockArea, DockState, Style, TabViewer};
use serde::{Deserialize, Serialize};

use super::{Cabin, Nav};

const DOCK_FILE: &str = "dock.json";

/// One dockable main-panel tab (GROK_NAV destinations).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(super) enum DockTab {
    Chat,
    Imagine,
    Automations,
    Skills,
    Workboards,
    Pulse,
}

impl DockTab {
    fn all() -> [Self; 6] {
        [
            Self::Chat,
            Self::Imagine,
            Self::Automations,
            Self::Skills,
            Self::Workboards,
            Self::Pulse,
        ]
    }

    fn title(self) -> &'static str {
        match self {
            Self::Chat => "Chat",
            Self::Imagine => "Imagine",
            Self::Automations => "Automations",
            Self::Skills => "Skills",
            Self::Workboards => "Workboards",
            Self::Pulse => "Pulse",
        }
    }

    fn nav(self) -> Nav {
        match self {
            Self::Chat => Nav::Chat,
            Self::Imagine => Nav::Imagine,
            Self::Automations => Nav::Night,
            Self::Skills => Nav::Skills,
            Self::Workboards => Nav::Workboard,
            Self::Pulse => Nav::Pulse,
        }
    }

    fn nav_id(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Imagine => "imagine",
            Self::Automations => "automations",
            Self::Skills => "skills",
            Self::Workboards => "workboard",
            Self::Pulse => "pulse",
        }
    }

    fn from_nav(nav: Nav) -> Option<Self> {
        match nav {
            Nav::Chat | Nav::Settings => Some(Self::Chat),
            Nav::Imagine => Some(Self::Imagine),
            Nav::Night => Some(Self::Automations),
            Nav::Skills => Some(Self::Skills),
            Nav::Workboard => Some(Self::Workboards),
            Nav::Pulse => Some(Self::Pulse),
            _ => None,
        }
    }
}

/// Paint a page shell: CentralPanel when fixed, or fill the dock leaf Ui when docked.
/// Returns the content rect (for overlays that anchor to the page).
pub(super) fn page_panel(
    ui: &mut egui::Ui,
    docked: bool,
    frame: egui::Frame,
    add_contents: impl FnOnce(&mut egui::Ui),
) -> egui::Rect {
    if docked {
        let rect = ui.available_rect_before_wrap();
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
            ui.set_clip_rect(rect);
            ui.set_min_size(rect.size());
            frame.show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                add_contents(ui);
            });
        });
        rect
    } else {
        egui::CentralPanel::default()
            .frame(frame)
            .show(ui, add_contents)
            .response
            .rect
    }
}

fn default_dock_state() -> DockState<DockTab> {
    DockState::new(DockTab::all().to_vec())
}

pub(super) fn load_dock_state() -> DockState<DockTab> {
    let path = crate::config::config_dir().join(DOCK_FILE);
    let state =
        crate::config::load_json_or(&path, crate::config::JSON_STORE_CAP, default_dock_state);
    if state.main_surface().is_empty() {
        default_dock_state()
    } else {
        state
    }
}

fn save_dock_state(state: &DockState<DockTab>) {
    let path = crate::config::config_dir().join(DOCK_FILE);
    if let Ok(s) = serde_json::to_string_pretty(state) {
        let _ = crate::config::atomic_write(&path, s.as_bytes());
    }
}

/// Which main-nav id (if any) to apply after a dock paint.
///
/// Split leaves all paint via `TabViewer::ui`; only the focused leaf's active tab
/// may move page nav. `cabin_nav` is the raw `Cabin.nav` (Settings overlay); `page_nav`
/// is what the sidebar shows behind Settings (`page_nav()`).
fn nav_id_from_focused_leaf(
    cabin_nav: Nav,
    page_nav: Nav,
    focused: Option<DockTab>,
) -> Option<&'static str> {
    // Settings paints as an overlay; page_nav() is settings_back, not Settings.
    if cabin_nav == Nav::Settings {
        return None;
    }
    let tab = focused?;
    if page_nav == tab.nav() {
        return None;
    }
    // Only sync when the cabin is already on a dockable page (sidebar / tab).
    DockTab::from_nav(page_nav)?;
    Some(tab.nav_id())
}

struct CabinTabViewer<'a> {
    cabin: &'a mut Cabin,
}

impl TabViewer for CabinTabViewer<'_> {
    type Tab = DockTab;

    fn id(&mut self, tab: &mut Self::Tab) -> egui::Id {
        egui::Id::new(("u7f-dock-tab", *tab))
    }

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        tab.title().into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        // Paint only. Do not touch page_nav here — egui_dock paints every visible
        // split leaf each frame, so last-leaf-wins would steal sidebar / Settings.
        match *tab {
            DockTab::Chat => {
                self.cabin.paint_native_badge(ui);
                self.cabin.ui_chat(ui);
            }
            DockTab::Imagine => self.cabin.ui_imagine(ui),
            DockTab::Automations => self.cabin.ui_night(ui),
            DockTab::Skills => self.cabin.ui_skills(ui),
            DockTab::Workboards => self.cabin.ui_board(ui),
            DockTab::Pulse => self.cabin.ui_pulse(ui),
        }
    }
}

impl Cabin {
    /// Whether the current page is a dockable GROK_NAV destination.
    pub(super) fn dock_covers_page(&self) -> bool {
        DockTab::from_nav(self.page_nav()).is_some()
    }

    /// DockArea for main panels (sidebar stays; pages become tabs).
    pub(super) fn ui_docked_main(&mut self, ui: &mut egui::Ui) {
        let on_settings = self.nav == Nav::Settings;
        // Sidebar → dock: focus the matching tab. Skip while Settings overlays.
        if !on_settings {
            if let Some(want) = DockTab::from_nav(self.page_nav()) {
                ensure_tab_focused(&mut self.dock_state, want);
            }
        }
        let before = serde_json::to_vec(&self.dock_state).ok();
        let mut dock_state = std::mem::replace(&mut self.dock_state, default_dock_state());
        {
            let mut viewer = CabinTabViewer { cabin: self };
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE.fill(crate::theme::bg()))
                .show(ui, |ui| {
                    DockArea::new(&mut dock_state)
                        .style(Style::from_egui(ui.style().as_ref()))
                        .show_close_buttons(false)
                        .show_add_buttons(false)
                        .show_inside(ui, &mut viewer);
                });
        }
        // Dock tab / leaf click → page_nav from the focused leaf only.
        let focused = dock_state.find_active_focused().map(|(_, tab)| *tab);
        self.dock_state = dock_state;
        if let Some(id) = nav_id_from_focused_leaf(self.nav, self.page_nav(), focused) {
            self.select_main_nav(id);
        }
        let after = serde_json::to_vec(&self.dock_state).ok();
        if before != after {
            // Serialize + write off the UI thread under persist_io (same mutex as
            // PersistService / window geom) so a dock drag does not hitch paint.
            let state = self.dock_state.clone();
            let io = self.persist_io.clone();
            std::thread::spawn(move || {
                if let Ok(_g) = io.lock() {
                    save_dock_state(&state);
                }
            });
        }
    }
}

fn ensure_tab_focused(state: &mut DockState<DockTab>, want: DockTab) {
    if let Some(path) = state.find_tab(&want) {
        let _ = state.set_active_tab(path);
    } else {
        state.push_to_focused_leaf(want);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focused_leaf_moves_page_nav_between_dock_tabs() {
        assert_eq!(
            nav_id_from_focused_leaf(Nav::Chat, Nav::Chat, Some(DockTab::Imagine)),
            Some("imagine")
        );
        assert_eq!(
            nav_id_from_focused_leaf(Nav::Pulse, Nav::Pulse, Some(DockTab::Chat)),
            Some("chat")
        );
    }

    #[test]
    fn focused_leaf_does_not_overwrite_matching_page() {
        assert_eq!(
            nav_id_from_focused_leaf(Nav::Chat, Nav::Chat, Some(DockTab::Chat)),
            None
        );
    }

    #[test]
    fn focused_leaf_does_not_steal_settings_overlay() {
        // page_nav() while Settings is open is settings_back (often Chat), not Settings.
        // Split paint must not select_main_nav and close the overlay.
        assert_eq!(
            nav_id_from_focused_leaf(Nav::Settings, Nav::Chat, Some(DockTab::Chat)),
            None
        );
        assert_eq!(
            nav_id_from_focused_leaf(Nav::Settings, Nav::Chat, Some(DockTab::Imagine)),
            None
        );
        assert_eq!(
            nav_id_from_focused_leaf(Nav::Settings, Nav::Imagine, Some(DockTab::Pulse)),
            None
        );
    }

    #[test]
    fn focused_leaf_ignores_non_dock_pages() {
        assert_eq!(
            nav_id_from_focused_leaf(Nav::History, Nav::History, Some(DockTab::Chat)),
            None
        );
    }

    #[test]
    fn no_focused_leaf_means_no_nav_change() {
        assert_eq!(nav_id_from_focused_leaf(Nav::Chat, Nav::Chat, None), None);
    }

    #[test]
    fn tab_viewer_ui_source_does_not_select_main_nav() {
        // Guard the Bugbot regression: every painted split leaf used to call
        // select_main_nav (last-leaf-wins). Keep nav updates out of TabViewer::ui.
        let src = include_str!("dock_ui.rs");
        let ui_fn = src
            .split("fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab)")
            .nth(1)
            .and_then(|s| s.split("fn ui_docked_main").next())
            .expect("TabViewer::ui body");
        assert!(
            !ui_fn.contains("select_main_nav"),
            "TabViewer::ui must not call select_main_nav (split leaves all paint)"
        );
        assert!(
            !ui_fn.contains("self.focus"),
            "TabViewer::ui must not track last-painted leaf as focus"
        );
    }
}
