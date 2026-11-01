//! The cabin window's size, place and maximized state: put back on launch,
//! remembered as it changes, and saved a moment after it settles.

use std::time::Duration;

use eframe::egui;

use super::Cabin;

impl Cabin {
    pub(super) fn apply_saved_geom(&mut self, ctx: &egui::Context) {
        let g = crate::window::clamp_geom(self.cfg.window);
        #[cfg(windows)]
        if g.maximized && self.window_visible {
            if crate::win_native::work_area().is_some() {
                // Clear winit's own maximized flag (the launch asked for it) this
                // frame; eframe applies viewport commands at the end of the
                // frame, so the native fill of the work area waits for the next
                // one or winit's restore would shrink the window back.
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(false));
                self.native_maximize_due = true;
                self.win_max = true;
                self.cfg.window.maximized = true;
                self.geom_applied = true;
                self.geom_apply_frames = 0;
                ctx.request_repaint();
                return;
            }
            self.win_max = true;
            return;
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(g.w, g.h)));
        if let Some([x, y]) = crate::window::launch_pos(&g) {
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(x, y)));
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(g.maximized));
        self.win_max = g.maximized;
        self.geom_applied = true;
        self.geom_apply_frames = 0;
    }

    pub(super) fn capture_window(&mut self, ctx: &egui::Context) {
        if !self.window_visible {
            return;
        }
        if !self.geom_applied {
            self.apply_saved_geom(ctx);
            return;
        }
        #[cfg(windows)]
        if self.native_maximize_due {
            self.native_maximize_due = false;
            self.native_maximize();
            self.geom_apply_frames = 0;
            return;
        }
        self.geom_apply_frames = self
            .geom_apply_frames
            .saturating_add(1)
            .min(crate::window::GEOM_SETTLE_FRAMES);
        if !crate::window::geom_can_remember(self.geom_applied, self.geom_apply_frames) {
            return;
        }
        let (outer, inner, egui_max) = ctx.input(|i| {
            (
                i.viewport().outer_rect,
                i.viewport().inner_rect,
                i.viewport().maximized,
            )
        });
        let Some(outer) = outer else {
            return;
        };
        let size = inner.map(|r| r.size()).unwrap_or(outer.size());
        let maximized = crate::helpers::maximized_now(egui_max, self.win_max);
        if let Some(g) = crate::window::remember_geom(
            self.window_visible,
            maximized,
            outer.min.x,
            outer.min.y,
            size.x,
            size.y,
            self.cfg.window,
        ) {
            if crate::window::geom_moved(g, self.cfg.window) {
                self.cfg.window = g;
                self.geom_dirty = true;
            }
            self.win_max = g.maximized;
        }
    }

    /// Undecorated egui windows ignore SW_MAXIMIZE: fill the monitor work area
    /// natively and re-apply the frame polish.
    #[cfg(windows)]
    pub(super) fn native_maximize(&mut self) {
        if let Some((x, y, w, h)) = crate::win_native::work_area() {
            let _ = crate::win_native::show_cabin(x, y, w, h, true);
            let dark = grokhub_core::resolve_dark(
                grokhub_core::parse_theme(&self.cfg.theme),
                crate::theme::desktop_prefers_dark(),
            );
            crate::win_native::polish_hwnd(dark);
            self.win_max = true;
        }
    }

    pub(super) fn flush_window(&mut self, ctx: &egui::Context) {
        match crate::window::geom_flush(
            self.geom_dirty,
            self.last_persist.elapsed().as_millis() as u64,
        ) {
            crate::window::GeomFlush::Skip => {}
            crate::window::GeomFlush::Now => {
                self.geom_dirty = false;
                let mut cfg = self.cfg.clone();
                cfg.api_key.clear();
                // PersistService worker pins dir + takes persist_io before save_in.
                let err = self.persist_err.clone();
                let dir = crate::config::config_dir();
                self.persist_svc.submit_window_cfg(dir, cfg, err);
            }
            crate::window::GeomFlush::AfterMs(ms) => {
                ctx.request_repaint_after(Duration::from_millis(ms));
            }
        }
    }
}
