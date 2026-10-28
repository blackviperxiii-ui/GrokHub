//! Pure slot layout for Home Mission Control (grid v2).
//!
//! `(content rect, HomeLayout-ish slots) → placed rects` at three breakpoints.
//! No egui — unit-tested with fixed rects.

use super::config::HomeLayout;

/// Gap between columns and stacked slots (SPEC: 12 px).
pub const GAP: f32 = 12.0;
/// Wide breakpoint: three equal-height columns.
pub const WIDE_MIN_W: f32 = 1500.0;
/// Medium breakpoint: L|M on top, smalls in a row under.
/// 1000 so a 1100×720 cabin (content ≈1012 after rail + pad) still hits Medium
/// and keeps slide-09 modules visible — not Narrow with an empty lower half.
pub const MEDIUM_MIN_W: f32 = 1000.0;
/// Below this, Large paints at Medium look (caller); layout still reserves L height.
pub const NARROW_L_AS_M: f32 = 640.0;

/// Column ratio L : M : S on wide (and L : M when S collapses).
const L_WEIGHT: f32 = 1.5;
const M_WEIGHT: f32 = 1.85;
const S_WEIGHT: f32 = 1.0;

/// Minimum body height so columns read as one box.
const MIN_BODY_H: f32 = 440.0;
const DEFAULT_BODY_H: f32 = 470.0;
/// Stacked medium height target on wide.
const MEDIUM_SLOT_H: f32 = 229.0;
/// Stacked / row small height target.
const SMALL_SLOT_H: f32 = 107.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModuleSize {
    Large,
    Medium,
    Small,
}

impl ModuleSize {
    pub fn label(self) -> &'static str {
        match self {
            ModuleSize::Large => "Large",
            ModuleSize::Medium => "Medium",
            ModuleSize::Small => "Small",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SlotId {
    Large,
    Medium1,
    Medium2,
    Small1,
    Small2,
    Small3,
    Small4,
}

impl SlotId {
    pub fn size(self) -> ModuleSize {
        match self {
            SlotId::Large => ModuleSize::Large,
            SlotId::Medium1 | SlotId::Medium2 => ModuleSize::Medium,
            SlotId::Small1 | SlotId::Small2 | SlotId::Small3 | SlotId::Small4 => ModuleSize::Small,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SlotId::Large => "Large",
            SlotId::Medium1 => "Medium 1",
            SlotId::Medium2 => "Medium 2",
            SlotId::Small1 => "Small 1",
            SlotId::Small2 => "Small 2",
            SlotId::Small3 => "Small 3",
            SlotId::Small4 => "Small 4",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Breakpoint {
    Wide,
    Medium,
    Narrow,
}

impl Breakpoint {
    pub fn for_width(w: f32) -> Self {
        if w >= WIDE_MIN_W {
            Breakpoint::Wide
        } else if w >= MEDIUM_MIN_W {
            Breakpoint::Medium
        } else {
            Breakpoint::Narrow
        }
    }
}

/// One placed slot: where to paint, which size look, and which module id (or None = empty).
#[derive(Debug, Clone, PartialEq)]
pub struct PlacedSlot {
    pub slot: SlotId,
    pub size: ModuleSize,
    /// Module id assigned to this slot; None when hidden / empty.
    pub module_id: Option<String>,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Visible medium slots in order (hidden skipped).
fn visible_medium(layout: &HomeLayout) -> Vec<(SlotId, Option<&str>)> {
    let ids = [
        (SlotId::Medium1, layout.medium.first().map(|s| s.as_str())),
        (SlotId::Medium2, layout.medium.get(1).map(|s| s.as_str())),
    ];
    ids.into_iter()
        .filter(|(_, id)| id.map(|s| !s.is_empty()).unwrap_or(false))
        .collect()
}

/// Visible small slots in order; `None` entries in `layout.small` are hidden.
fn visible_small(layout: &HomeLayout) -> Vec<(SlotId, &str)> {
    const SLOTS: [SlotId; 4] = [
        SlotId::Small1,
        SlotId::Small2,
        SlotId::Small3,
        SlotId::Small4,
    ];
    layout
        .small
        .iter()
        .zip(SLOTS)
        .filter_map(|(id, slot)| {
            id.as_ref()
                .filter(|s| !s.is_empty())
                .map(|s| (slot, s.as_str()))
        })
        .collect()
}

fn col_widths(total: f32, with_small: bool) -> (f32, f32, f32) {
    let (lw, mw, sw) = if with_small {
        (L_WEIGHT, M_WEIGHT, S_WEIGHT)
    } else {
        (L_WEIGHT, M_WEIGHT, 0.0)
    };
    let sum = lw + mw + sw;
    let gaps = if with_small { 2.0 } else { 1.0 };
    let inner = (total - GAP * gaps).max(0.0);
    (
        inner * (lw / sum),
        inner * (mw / sum),
        if with_small { inner * (sw / sum) } else { 0.0 },
    )
}

fn body_height(avail_h: f32) -> f32 {
    if avail_h <= 0.0 {
        DEFAULT_BODY_H
    } else {
        avail_h.max(MIN_BODY_H)
    }
}

/// Place visible slots inside `rect` (`x,y,w,h`) per the active breakpoint.
/// Hidden slots are omitted. Large is always placed when `layout.large` is set.
pub fn place_slots(rect: (f32, f32, f32, f32), layout: &HomeLayout) -> Vec<PlacedSlot> {
    let (ox, oy, w, h) = rect;
    let bp = Breakpoint::for_width(w);
    let mediums = visible_medium(layout);
    let smalls = visible_small(layout);
    let large_trim = layout.large.trim();
    let large_id = (!large_trim.is_empty()).then_some(large_trim);

    match bp {
        Breakpoint::Wide => place_wide(ox, oy, w, h, large_id, &mediums, &smalls),
        Breakpoint::Medium => place_medium(ox, oy, w, h, large_id, &mediums, &smalls),
        Breakpoint::Narrow => place_narrow(ox, oy, w, h, large_id, &mediums, &smalls),
    }
}

fn push_large(
    out: &mut Vec<PlacedSlot>,
    id: Option<&str>,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    as_medium: bool,
) {
    let Some(id) = id.filter(|s| !s.is_empty()) else {
        return;
    };
    out.push(PlacedSlot {
        slot: SlotId::Large,
        size: if as_medium {
            ModuleSize::Medium
        } else {
            ModuleSize::Large
        },
        module_id: Some(id.to_string()),
        x,
        y,
        w,
        h,
    });
}

fn place_wide(
    ox: f32,
    oy: f32,
    w: f32,
    h: f32,
    large: Option<&str>,
    mediums: &[(SlotId, Option<&str>)],
    smalls: &[(SlotId, &str)],
) -> Vec<PlacedSlot> {
    let body_h = body_height(h);
    let with_s = !smalls.is_empty();
    let (lw, mw, sw) = col_widths(w, with_s);
    let mut out = Vec::with_capacity(7);
    let mut x = ox;
    push_large(&mut out, large, x, oy, lw, body_h, false);
    x += lw + GAP;

    // Medium column: visible Ms share the height.
    if !mediums.is_empty() {
        let n = mediums.len() as f32;
        let slot_h = ((body_h - GAP * (n - 1.0)) / n).max(1.0);
        let mut y = oy;
        for (slot, id) in mediums {
            if let Some(id) = id.filter(|s| !s.is_empty()) {
                out.push(PlacedSlot {
                    slot: *slot,
                    size: ModuleSize::Medium,
                    module_id: Some(id.to_string()),
                    x,
                    y,
                    w: mw,
                    h: slot_h,
                });
            }
            y += slot_h + GAP;
        }
        x += mw + GAP;
    }

    if with_s {
        let n = smalls.len() as f32;
        let slot_h = ((body_h - GAP * (n - 1.0).max(0.0)) / n).max(1.0);
        let mut y = oy;
        for (slot, id) in smalls {
            out.push(PlacedSlot {
                slot: *slot,
                size: ModuleSize::Small,
                module_id: Some((*id).to_string()),
                x,
                y,
                w: sw,
                h: slot_h,
            });
            y += slot_h + GAP;
        }
    }
    out
}

fn place_medium(
    ox: f32,
    oy: f32,
    w: f32,
    h: f32,
    large: Option<&str>,
    mediums: &[(SlotId, Option<&str>)],
    smalls: &[(SlotId, &str)],
) -> Vec<PlacedSlot> {
    let with_s = !smalls.is_empty();
    let small_row_h = if with_s { SMALL_SLOT_H } else { 0.0 };
    let top_h = if with_s {
        (h - GAP - small_row_h).max(MEDIUM_SLOT_H)
    } else {
        body_height(h)
    };
    let (lw, mw, _) = col_widths(w, false);
    let mut out = Vec::with_capacity(7);
    push_large(&mut out, large, ox, oy, lw, top_h, false);

    if !mediums.is_empty() {
        let n = mediums.len() as f32;
        let slot_h = ((top_h - GAP * (n - 1.0)) / n).max(1.0);
        let mx = ox + lw + GAP;
        let mut y = oy;
        for (slot, id) in mediums {
            if let Some(id) = id.filter(|s| !s.is_empty()) {
                out.push(PlacedSlot {
                    slot: *slot,
                    size: ModuleSize::Medium,
                    module_id: Some(id.to_string()),
                    x: mx,
                    y,
                    w: mw,
                    h: slot_h,
                });
            }
            y += slot_h + GAP;
        }
    }

    if with_s {
        let n = smalls.len() as f32;
        let gaps = GAP * (n - 1.0).max(0.0);
        let tile_w = ((w - gaps) / n).max(1.0);
        let y = oy + top_h + GAP;
        let mut x = ox;
        for (slot, id) in smalls {
            out.push(PlacedSlot {
                slot: *slot,
                size: ModuleSize::Small,
                module_id: Some((*id).to_string()),
                x,
                y,
                w: tile_w,
                h: small_row_h,
            });
            x += tile_w + GAP;
        }
    }
    out
}

fn place_narrow(
    ox: f32,
    oy: f32,
    w: f32,
    h: f32,
    large: Option<&str>,
    mediums: &[(SlotId, Option<&str>)],
    smalls: &[(SlotId, &str)],
) -> Vec<PlacedSlot> {
    let as_m = w < NARROW_L_AS_M;
    let n_m = mediums
        .iter()
        .filter(|(_, id)| id.filter(|s| !s.is_empty()).is_some())
        .count() as f32;
    let s_rows = if smalls.is_empty() {
        0.0
    } else {
        ((smalls.len() + 1) / 2) as f32
    };
    // Keep Medium + smalls in the first viewport (slide 09 900×700) instead of
    // a 440 px Large that leaves an empty lower half with siblings off-screen.
    let sibling_h = n_m * (MEDIUM_SLOT_H + GAP) + s_rows * (SMALL_SLOT_H + GAP);
    let avail = if h > 0.0 { h } else { DEFAULT_BODY_H };
    let mut out = Vec::with_capacity(7);
    let mut y = oy;
    let l_h = if as_m {
        MEDIUM_SLOT_H
    } else if sibling_h > 0.0 {
        // Prefer fitting L+M+S in `avail` (900×700 body); floor only so Large stays readable.
        (avail - sibling_h).clamp(120.0, MIN_BODY_H)
    } else {
        avail.max(MIN_BODY_H).min(440.0)
    };
    push_large(&mut out, large, ox, y, w, l_h, as_m);
    if large.filter(|s| !s.is_empty()).is_some() {
        y += l_h + GAP;
    }

    for (slot, id) in mediums {
        if let Some(id) = id.filter(|s| !s.is_empty()) {
            out.push(PlacedSlot {
                slot: *slot,
                size: ModuleSize::Medium,
                module_id: Some(id.to_string()),
                x: ox,
                y,
                w,
                h: MEDIUM_SLOT_H,
            });
            y += MEDIUM_SLOT_H + GAP;
        }
    }

    // Smalls as 2×2 (one small → full-width row).
    if !smalls.is_empty() {
        let cols = 2.0_f32.min(smalls.len() as f32);
        let tile_w = ((w - GAP * (cols - 1.0)) / cols).max(1.0);
        for (i, (slot, id)) in smalls.iter().enumerate() {
            let col = (i % 2) as f32;
            let row = (i / 2) as f32;
            let x = ox + col * (tile_w + GAP);
            let sy = y + row * (SMALL_SLOT_H + GAP);
            out.push(PlacedSlot {
                slot: *slot,
                size: ModuleSize::Small,
                module_id: Some((*id).to_string()),
                x,
                y: sy,
                w: tile_w,
                h: SMALL_SLOT_H,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::config::{HomeLayout, HomePreset};
    use super::*;

    fn default_layout() -> HomeLayout {
        HomeLayout::default()
    }

    #[test]
    fn breakpoint_edges() {
        assert_eq!(Breakpoint::for_width(1500.0), Breakpoint::Wide);
        assert_eq!(Breakpoint::for_width(1499.0), Breakpoint::Medium);
        assert_eq!(Breakpoint::for_width(1000.0), Breakpoint::Medium);
        assert_eq!(Breakpoint::for_width(999.0), Breakpoint::Narrow);
        assert_eq!(Breakpoint::for_width(1100.0), Breakpoint::Medium);
        assert_eq!(SlotId::Large.size(), ModuleSize::Large);
        assert_eq!(SlotId::Small3.label(), "Small 3");
        assert_eq!(ModuleSize::Medium.label(), "Medium");
    }

    #[test]
    fn wide_places_slide09_three_columns() {
        // Mission Control: Objectives | PC Loadout | Up Next (one each).
        let layout = default_layout();
        let placed = place_slots((0.0, 0.0, 1600.0, 470.0), &layout);
        assert_eq!(placed.len(), 3);
        assert_eq!(placed[0].slot, SlotId::Large);
        assert_eq!(placed[0].module_id.as_deref(), Some("objectives"));
        assert_eq!(placed[0].size, ModuleSize::Large);
        let body_h = 470.0;
        assert!((placed[0].h - body_h).abs() < 0.01);
        let m1 = placed.iter().find(|p| p.slot == SlotId::Medium1).unwrap();
        assert_eq!(m1.module_id.as_deref(), Some("pc_loadout"));
        assert!((m1.h - body_h).abs() < 0.01);
        assert!(placed.iter().all(|p| p.slot != SlotId::Medium2));
        let s1 = placed.iter().find(|p| p.slot == SlotId::Small1).unwrap();
        assert_eq!(s1.module_id.as_deref(), Some("up_next"));
        assert!((s1.h - body_h).abs() < 0.01);
        assert!(placed[0].w < m1.w);
        let (lw, mw, sw) = col_widths(1600.0, true);
        assert!((placed[0].w - lw).abs() < 0.01);
        assert!((m1.w - mw).abs() < 0.01);
        assert!((s1.w - sw).abs() < 0.01);
    }

    #[test]
    fn medium_puts_smalls_in_a_row_under() {
        let layout = default_layout();
        let placed = place_slots((0.0, 0.0, 1200.0, 500.0), &layout);
        assert_eq!(placed.len(), 3);
        let smalls: Vec<_> = placed
            .iter()
            .filter(|p| p.size == ModuleSize::Small)
            .collect();
        assert_eq!(smalls.len(), 1);
        assert_eq!(smalls[0].module_id.as_deref(), Some("up_next"));
        let large = placed.iter().find(|p| p.slot == SlotId::Large).unwrap();
        let m1 = placed.iter().find(|p| p.slot == SlotId::Medium1).unwrap();
        assert_eq!(m1.module_id.as_deref(), Some("pc_loadout"));
        assert!(smalls[0].y > large.y + large.h - 0.01);
        assert!((smalls[0].y - (m1.y + m1.h + GAP)).abs() < 0.5);
    }

    #[test]
    fn narrow_stacks_slide09_modules_in_viewport() {
        let layout = default_layout();
        let placed = place_slots((10.0, 20.0, 800.0, 500.0), &layout);
        assert_eq!(placed.len(), 3);
        let large = &placed[0];
        assert_eq!(large.x, 10.0);
        assert_eq!(large.size, ModuleSize::Large);
        assert_eq!(large.module_id.as_deref(), Some("objectives"));
        // Large shrinks so PC Loadout + Up Next stay in the first viewport.
        assert!(large.h <= MIN_BODY_H + 0.01);
        assert!(large.h >= 120.0 - 0.01);
        let m1 = placed.iter().find(|p| p.slot == SlotId::Medium1).unwrap();
        assert_eq!(m1.module_id.as_deref(), Some("pc_loadout"));
        assert!(m1.y > large.y);
        let s1 = placed.iter().find(|p| p.slot == SlotId::Small1).unwrap();
        assert_eq!(s1.module_id.as_deref(), Some("up_next"));
        assert!(s1.y > m1.y);
        assert!((s1.w - 800.0).abs() < 0.01, "one small spans full width");
        let bottom = s1.y + s1.h - 20.0;
        assert!(
            bottom <= 500.0 + 0.5,
            "slide 09 modules must fit the narrow body height, got bottom {bottom}"
        );
    }

    #[test]
    fn narrow_four_smalls_two_by_two() {
        let layout = HomeLayout::preset(HomePreset::Performance);
        let placed = place_slots((10.0, 20.0, 800.0, 900.0), &layout);
        let s1 = placed.iter().find(|p| p.slot == SlotId::Small1).unwrap();
        let s2 = placed.iter().find(|p| p.slot == SlotId::Small2).unwrap();
        let s3 = placed.iter().find(|p| p.slot == SlotId::Small3).unwrap();
        assert!((s1.y - s2.y).abs() < 0.01);
        assert!(s3.y > s1.y);
        assert!(s2.x > s1.x);
    }

    #[test]
    fn narrow_below_640_uses_medium_look_for_large() {
        let layout = default_layout();
        let placed = place_slots((0.0, 0.0, 600.0, 800.0), &layout);
        let large = placed.iter().find(|p| p.slot == SlotId::Large).unwrap();
        assert_eq!(large.size, ModuleSize::Medium);
    }

    #[test]
    fn hiding_all_smalls_collapses_s_column_on_wide() {
        let mut layout = default_layout();
        layout.small = [None, None, None, None];
        let placed = place_slots((0.0, 0.0, 1600.0, 470.0), &layout);
        assert!(placed.iter().all(|p| p.size != ModuleSize::Small));
        assert_eq!(placed.len(), 2); // L + 1 M (Mission Control)
        let large = &placed[0];
        let m1 = placed.iter().find(|p| p.slot == SlotId::Medium1).unwrap();
        let (lw, mw, sw) = col_widths(1600.0, false);
        assert_eq!(sw, 0.0);
        assert!((large.w - lw).abs() < 0.01);
        assert!((m1.w - mw).abs() < 0.01);
        assert!(large.w + GAP + m1.w <= 1600.0 + 0.01);
    }

    #[test]
    fn hiding_one_medium_lets_the_other_fill() {
        let mut layout = default_layout();
        layout.medium = vec!["up_next".into()]; // only M1
        layout.small = [None, None, None, None];
        let placed = place_slots((0.0, 0.0, 1600.0, 470.0), &layout);
        let mediums: Vec<_> = placed
            .iter()
            .filter(|p| p.size == ModuleSize::Medium)
            .collect();
        assert_eq!(mediums.len(), 1);
        assert!((mediums[0].h - 470.0).abs() < 0.01);
    }

    #[test]
    fn streamer_preset_places_clips_as_large() {
        let layout = HomeLayout::preset(HomePreset::Streamer);
        let placed = place_slots((0.0, 0.0, 1600.0, 470.0), &layout);
        assert_eq!(placed[0].module_id.as_deref(), Some("clips"));
        assert_eq!(
            placed
                .iter()
                .find(|p| p.slot == SlotId::Medium1)
                .and_then(|p| p.module_id.as_deref()),
            Some("squad")
        );
    }
}
