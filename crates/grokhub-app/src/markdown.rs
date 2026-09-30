use eframe::egui::text::{LayoutJob, TextFormat};
use eframe::egui::{
    self, Align, Color32, Context, Frame, Id, Label, Layout, Margin, RichText, Sense, Stroke,
    TextStyle, TextWrapMode, Ui, Vec2,
};
use grokhub_core::{
    bubble_max_width, code_tokens, md_blocks, md_plain, md_spans, CodeTok, MdAlign, MdBlock,
    MdSpan, TEXT_FILE_CAP,
};

/// Paint/layout prefix. Stream buffers may hold `IMAGE_FILE_CAP`; laying that out freezes Chat.
pub(crate) fn display_text(text: &str) -> &str {
    let cap = TEXT_FILE_CAP;
    if text.len() <= cap {
        return text;
    }
    let mut end = cap;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

/// Cap for wrapping. Short bubbles hug via `bubble_outer_width`, they do not stretch to this.
pub fn bubble_width(available: f32) -> f32 {
    bubble_max_width(available)
}

/// Layout bubble text so a long token wraps inside `wrap`.
pub fn wrapped_job(ui: &Ui, text: &str, wrap: f32, color: Color32) -> LayoutJob {
    let text = display_text(text);
    let font = TextStyle::Body.resolve(ui.style());
    let wrap = wrap.max(1.0);
    let mut job = LayoutJob::simple(text.to_owned(), font, color, wrap);
    // Word boundaries first. A token wider than the row still breaks
    // (epaint falls through space, dash, punctuation, then any glyph).
    job.wrap.break_anywhere = false;
    job
}

pub fn measure_text(ui: &Ui, text: &str, wrap: f32) -> Vec2 {
    let text = display_text(text);
    let wrap = wrap.max(1.0);
    if text.is_empty() {
        return Vec2::new(0.0, ui.text_style_height(&TextStyle::Body));
    }
    let job = wrapped_job(ui, text, wrap, Color32::WHITE);
    ui.fonts(|f| f.layout_job(job)).size()
}

/// Bubble content width for a markdown reply. Code blocks, tables, and headings take
/// the full cap; other replies hug the text with the markers stripped.
pub fn measure_markdown_width(ui: &Ui, text: &str, wrap: f32) -> f32 {
    let text = display_text(text);
    let wrap = wrap.max(1.0);
    let blocks = md_blocks(text);
    if blocks.iter().any(|b| {
        matches!(
            b,
            MdBlock::Code { .. } | MdBlock::Table { .. } | MdBlock::Heading(..) | MdBlock::Quote(_)
        )
    }) {
        return wrap;
    }
    let plain: Vec<String> = blocks
        .iter()
        .map(|b| match b {
            MdBlock::Bullet { depth, text } | MdBlock::Task { depth, text, .. } => {
                format!("{}•  {}", "    ".repeat(*depth as usize), md_plain(text))
            }
            MdBlock::Numbered { depth, num, text } => {
                format!("{}{num}.  {}", "    ".repeat(*depth as usize), md_plain(text))
            }
            MdBlock::Para(t) => md_plain(t),
            _ => String::new(),
        })
        .collect();
    measure_text(ui, &plain.join("\n"), wrap).x.min(wrap)
}

/// Temp slot for a code block whose Copy was clicked this frame.
fn code_copy_id() -> Id {
    Id::new("cabin-md-code-copy")
}

/// The code a block's Copy asked for, once.
pub fn take_code_copy(ctx: &Context) -> Option<String> {
    ctx.data_mut(|d| d.remove_temp::<String>(code_copy_id()))
}

pub fn show(ui: &mut Ui, text: &str) {
    let text = display_text(text);
    ui.style_mut().wrap_mode = Some(TextWrapMode::Wrap);
    let wrap = ui.available_width().max(1.0);
    ui.set_max_width(wrap);
    for (bi, block) in md_blocks(text).into_iter().enumerate() {
        match block {
            MdBlock::Heading(level, rest) => {
                let rich = RichText::new(md_plain(&rest));
                let rich = match level {
                    1 => rich.heading().strong(),
                    2 => rich.heading(),
                    _ => rich.strong(),
                };
                wrapping_label(ui, rich, wrap);
            }
            MdBlock::Bullet { depth, text } => {
                list_item(ui, depth, Marker::Dot, &text, wrap);
            }
            MdBlock::Numbered { depth, num, text } => {
                list_item(ui, depth, Marker::Num(&num), &text, wrap);
            }
            MdBlock::Task { depth, done, text } => {
                list_item(ui, depth, Marker::Check(done), &text, wrap);
            }
            MdBlock::Quote(body) => quote(ui, &body, wrap),
            MdBlock::Rule => {
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);
            }
            MdBlock::Table { header, align, rows } => {
                table(ui, bi, &header, &align, &rows, wrap);
            }
            MdBlock::Code { lang, body } => code_block(ui, bi, &lang, &body, wrap),
            MdBlock::Blank => ui.add_space(6.0),
            MdBlock::Para(line) => inline(ui, &line, wrap),
        }
    }
}

fn wrapping_label(ui: &mut Ui, text: RichText, wrap: f32) {
    ui.set_max_width(wrap);
    ui.add(Label::new(text).wrap().selectable(true));
}

enum Marker<'a> {
    Dot,
    Num(&'a str),
    Check(bool),
}

const LIST_INDENT: f32 = 16.0;

fn list_item(ui: &mut Ui, depth: u8, marker: Marker, text: &str, wrap: f32) {
    let lead = LIST_INDENT * depth as f32;
    let gutter = 20.0;
    ui.horizontal_top(|ui| {
        ui.set_max_width(wrap);
        ui.spacing_mut().item_spacing.x = 0.0;
        if lead > 0.0 {
            ui.add_space(lead);
        }
        let row_h = ui.text_style_height(&TextStyle::Body);
        let (rect, _) = ui.allocate_exact_size(Vec2::new(gutter, row_h), Sense::hover());
        let ink = crate::theme::muted();
        match marker {
            Marker::Dot => {
                let r = if depth == 0 { 2.2 } else { 1.8 };
                let c = egui::pos2(rect.min.x + 6.0, rect.center().y);
                if depth == 0 {
                    ui.painter().circle_filled(c, r, ink);
                } else {
                    ui.painter().circle_stroke(c, r, Stroke::new(1.0, ink));
                }
            }
            Marker::Num(n) => {
                let font = TextStyle::Body.resolve(ui.style());
                let galley = ui.fonts(|f| f.layout_no_wrap(format!("{n}."), font, ink));
                // Two-digit numbers push the text a little instead of overlapping it.
                let w = galley.size().x;
                ui.painter().galley(
                    egui::pos2(rect.min.x, rect.center().y - galley.size().y * 0.5),
                    galley,
                    ink,
                );
                if w + 4.0 > gutter {
                    ui.add_space(w + 4.0 - gutter);
                }
            }
            Marker::Check(done) => {
                let side = 11.0;
                let b = egui::Rect::from_center_size(
                    egui::pos2(rect.min.x + side * 0.5 + 1.0, rect.center().y),
                    Vec2::splat(side),
                );
                ui.painter().rect_stroke(b, 2.5, Stroke::new(1.2, ink));
                if done {
                    let s = Stroke::new(1.6, crate::theme::live());
                    ui.painter().line_segment(
                        [
                            egui::pos2(b.min.x + 2.5, b.center().y),
                            egui::pos2(b.min.x + 4.8, b.max.y - 2.5),
                        ],
                        s,
                    );
                    ui.painter().line_segment(
                        [
                            egui::pos2(b.min.x + 4.8, b.max.y - 2.5),
                            egui::pos2(b.max.x - 2.2, b.min.y + 2.5),
                        ],
                        s,
                    );
                }
            }
        }
        ui.with_layout(Layout::top_down(Align::LEFT), |ui| {
            inline(ui, text, (wrap - lead - gutter).max(1.0));
        });
    });
}

fn quote(ui: &mut Ui, body: &str, wrap: f32) {
    let bar = 3.0;
    let pad = 10.0;
    let resp = Frame::none()
        .inner_margin(Margin {
            left: bar + pad,
            right: 0.0,
            top: 2.0,
            bottom: 2.0,
        })
        .show(ui, |ui| {
            let inner = (wrap - bar - pad).max(1.0);
            ui.set_max_width(inner);
            ui.visuals_mut().override_text_color = Some(crate::theme::muted());
            for line in body.lines() {
                if line.trim().is_empty() {
                    ui.add_space(4.0);
                } else {
                    inline(ui, line, inner);
                }
            }
        })
        .response;
    let r = resp.rect;
    ui.painter().rect_filled(
        egui::Rect::from_min_max(r.min, egui::pos2(r.min.x + bar, r.max.y)),
        1.5,
        crate::theme::border_strong(),
    );
}

fn table(
    ui: &mut Ui,
    key: usize,
    header: &[String],
    align: &[MdAlign],
    rows: &[Vec<String>],
    wrap: f32,
) {
    let spacing = Vec2::new(14.0, 6.0);
    let widths = table_col_widths(ui, header, rows, wrap, spacing.x);
    ui.add_space(2.0);
    // No stripes: the theme's faint fill is opaque and a stripe painted for row 1
    // covered the header's descenders. The header gets a hairline instead.
    egui::Grid::new(ui.id().with(("md-table", key)))
        .striped(false)
        .spacing(spacing)
        .min_col_width(0.0)
        .show(ui, |ui| {
            for (c, cell) in header.iter().enumerate() {
                cell_label(ui, cell, align.get(c).copied(), widths[c], true);
            }
            ui.end_row();
            for row in rows {
                for (c, cell) in row.iter().enumerate().take(widths.len()) {
                    cell_label(ui, cell, align.get(c).copied(), widths[c], false);
                }
                ui.end_row();
            }
        });
    ui.add_space(2.0);
}

/// Natural width per column (widest cell, markers stripped). A table wider than
/// the bubble shrinks every column by the same share so cells wrap instead of
/// running into the next column.
pub(crate) fn table_col_widths(
    ui: &Ui,
    header: &[String],
    rows: &[Vec<String>],
    wrap: f32,
    gap: f32,
) -> Vec<f32> {
    const MIN_COL: f32 = 24.0;
    let cols = header.len().max(1);
    let font = TextStyle::Body.resolve(ui.style());
    let measure = |t: &str| {
        ui.fonts(|f| {
            f.layout_no_wrap(md_plain(t), font.clone(), Color32::WHITE)
                .size()
                .x
        })
    };
    let mut widths = vec![MIN_COL; cols];
    for (c, h) in header.iter().enumerate() {
        widths[c] = widths[c].max(measure(h).ceil());
    }
    for row in rows {
        for (c, cell) in row.iter().enumerate().take(cols) {
            widths[c] = widths[c].max(measure(cell).ceil());
        }
    }
    let gaps = gap * (cols as f32 - 1.0);
    let room = (wrap - gaps).max(MIN_COL * cols as f32);
    let natural: f32 = widths.iter().sum();
    if natural > room {
        // Columns that would shrink below MIN_COL are pinned there, and their
        // share comes out of the wide columns, so the total still fits.
        let mut pinned = vec![false; cols];
        loop {
            let fixed = MIN_COL * pinned.iter().filter(|p| **p).count() as f32;
            let flex: f32 = widths
                .iter()
                .zip(&pinned)
                .filter(|(_, p)| !**p)
                .map(|(w, _)| *w)
                .sum();
            let scale = (room - fixed).max(0.0) / flex.max(1.0);
            let mut changed = false;
            for c in 0..cols {
                if !pinned[c] && widths[c] * scale < MIN_COL {
                    pinned[c] = true;
                    changed = true;
                }
            }
            if !changed {
                for c in 0..cols {
                    widths[c] = if pinned[c] {
                        MIN_COL
                    } else {
                        (widths[c] * scale).floor()
                    };
                }
                break;
            }
        }
    }
    widths
}

/// Each cell gets exactly its column width, so right and center alignment stay inside it.
fn cell_label(ui: &mut Ui, cell: &str, align: Option<MdAlign>, col_w: f32, head: bool) {
    let layout = match align.unwrap_or(MdAlign::Left) {
        MdAlign::Left => Layout::top_down(Align::LEFT),
        MdAlign::Center => Layout::top_down(Align::Center),
        MdAlign::Right => Layout::top_down(Align::RIGHT),
    };
    ui.allocate_ui_with_layout(Vec2::new(col_w, 0.0), layout, |ui| {
        ui.set_min_width(col_w);
        ui.set_max_width(col_w);
        if head {
            wrapping_label(ui, RichText::new(md_plain(cell)).strong(), col_w);
            let r = ui.min_rect();
            ui.painter().hline(
                r.left()..=r.left() + col_w,
                r.bottom() + 3.0,
                Stroke::new(1.0, crate::theme::border_strong()),
            );
        } else {
            inline(ui, cell, col_w);
        }
    });
}

/// One LayoutJob per fence so a drag selects across lines.
pub(crate) fn code_job(ui: &Ui, lang: &str, body: &str, wrap: f32) -> LayoutJob {
    let font = TextStyle::Monospace.resolve(ui.style());
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap.max(1.0);
    job.wrap.break_anywhere = true;
    let mut first = true;
    for line in body.lines() {
        if !first {
            job.append("\n", 0.0, TextFormat::simple(font.clone(), crate::theme::fg()));
        }
        first = false;
        for (tok, text) in code_tokens(lang, line) {
            let color = match tok {
                CodeTok::Plain => crate::theme::fg(),
                CodeTok::Keyword => crate::theme::code_keyword(),
                CodeTok::Str => crate::theme::code_string(),
                CodeTok::Number => crate::theme::code_number(),
                CodeTok::Comment => crate::theme::code_comment(),
            };
            let mut fmt = TextFormat::simple(font.clone(), color);
            fmt.italics = tok == CodeTok::Comment;
            job.append(&text, 0.0, fmt);
        }
    }
    job
}

fn code_block(ui: &mut Ui, key: usize, lang: &str, body: &str, wrap: f32) {
    let pad = Vec2::new(10.0, 8.0);
    ui.add_space(2.0);
    Frame::none()
        .fill(crate::theme::code_well())
        .stroke(Stroke::new(1.0, crate::theme::border()))
        .rounding(8.0)
        .inner_margin(Margin::symmetric(pad.x, pad.y))
        .show(ui, |ui| {
            let inner = (wrap - pad.x * 2.0 - 2.0).max(1.0);
            ui.set_width(inner);
            ui.set_max_width(inner);
            let head_h = 24.0;
            ui.allocate_ui_with_layout(
                Vec2::new(inner, head_h),
                Layout::left_to_right(Align::Center),
                |ui| {
                ui.set_min_height(head_h);
                let tag = if lang.is_empty() { "code" } else { lang };
                ui.label(
                    RichText::new(tag)
                        .size(crate::theme::FONT_TIP)
                        .color(crate::theme::subtle()),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.push_id(("md-code-copy", key), |ui| {
                        let copy = crate::theme::felt_label_button(
                            ui,
                            "Copy",
                            Color32::TRANSPARENT,
                            crate::theme::muted(),
                            6.0,
                            Vec2::ZERO,
                            None,
                            false,
                        )
                        .on_hover_text("Copy this code");
                        if copy.clicked() {
                            let code = body.to_string();
                            ui.ctx()
                                .data_mut(|d| d.insert_temp(code_copy_id(), code));
                        }
                    });
                });
                },
            );
            ui.add_space(2.0);
            let job = code_job(ui, lang, body, inner);
            ui.add(Label::new(job).wrap().selectable(true));
        });
    ui.add_space(2.0);
}

fn wrapping_break(ui: &mut Ui, line: &str, wrap: f32) {
    ui.set_max_width(wrap);
    let ink = ui.visuals().override_text_color.unwrap_or_else(crate::theme::fg);
    let job = wrapped_job(ui, line, wrap, ink);
    ui.add(Label::new(job).wrap().selectable(true));
}

/// One line of prose. No link keeps it a single LayoutJob so wrapping follows words.
fn inline(ui: &mut Ui, line: &str, wrap: f32) {
    let spans = md_spans(line);
    if let [MdSpan::Text(t)] = spans.as_slice() {
        wrapping_break(ui, t, wrap);
        return;
    }
    if spans.is_empty() {
        ui.add_space(ui.text_style_height(&TextStyle::Body));
        return;
    }
    let has_link = spans.iter().any(|s| matches!(s, MdSpan::Link { .. }));
    if !has_link {
        ui.set_max_width(wrap);
        let job = spans_job(ui, &spans, wrap);
        ui.add(Label::new(job).wrap().selectable(true));
        return;
    }
    ui.horizontal_wrapped(|ui| {
        ui.set_max_width(wrap);
        ui.spacing_mut().item_spacing.x = 0.0;
        ui.style_mut().wrap_mode = Some(TextWrapMode::Wrap);
        for span in &spans {
            match span {
                MdSpan::Link { text, url } => {
                    // `md_spans` only builds http(s) links (`md_link_ok`).
                    ui.hyperlink_to(RichText::new(text).color(crate::theme::link()), url)
                        .on_hover_text(url);
                }
                other => {
                    let job = spans_job(ui, std::slice::from_ref(other), wrap);
                    ui.add(Label::new(job).wrap().selectable(true));
                }
            }
        }
    });
}

fn spans_job(ui: &Ui, spans: &[MdSpan], wrap: f32) -> LayoutJob {
    let body = TextStyle::Body.resolve(ui.style());
    let mono = TextStyle::Monospace.resolve(ui.style());
    let fg = crate::theme::fg();
    let strong = ui.visuals().strong_text_color();
    let override_ink = ui.visuals().override_text_color;
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap.max(1.0);
    job.wrap.break_anywhere = false;
    for span in spans {
        let (text, fmt) = match span {
            MdSpan::Text(t) => (t.as_str(), TextFormat::simple(body.clone(), fg)),
            MdSpan::Bold(t) => (t.as_str(), TextFormat::simple(body.clone(), strong)),
            MdSpan::Italic(t) => {
                let mut f = TextFormat::simple(body.clone(), fg);
                f.italics = true;
                (t.as_str(), f)
            }
            MdSpan::Strike(t) => {
                let mut f = TextFormat::simple(body.clone(), crate::theme::muted());
                f.strikethrough = Stroke::new(1.0, crate::theme::muted());
                (t.as_str(), f)
            }
            MdSpan::Code(t) => {
                let mut f = TextFormat::simple(mono.clone(), crate::theme::subtle());
                f.background = crate::theme::code_well();
                (t.as_str(), f)
            }
            MdSpan::Link { text, .. } => {
                let mut f = TextFormat::simple(body.clone(), crate::theme::link());
                f.underline = Stroke::new(1.0, crate::theme::link());
                (text.as_str(), f)
            }
        };
        let mut fmt = fmt;
        if let Some(ink) = override_ink {
            if !matches!(span, MdSpan::Code(_) | MdSpan::Link { .. }) {
                fmt.color = ink;
            }
        }
        job.append(text, 0.0, fmt);
    }
    job
}

#[cfg(test)]
mod tests {
    use super::{bubble_width, display_text, measure_text};
    use grokhub_core::TEXT_FILE_CAP;
    use grokhub_core::{
        bubble_max_width, bubble_outer_width, bubble_wrap_width, BUBBLE_MAX_FRAC, BUBBLE_PAD_X,
    };

    #[test]
    fn splits_markers() {
        assert!("**bold** and `code`".contains("**"));
    }

    #[test]
    fn display_text_stays_inside_text_file_cap() {
        let huge = "é".repeat(TEXT_FILE_CAP + 16);
        let shown = display_text(&huge);
        assert!(shown.len() <= TEXT_FILE_CAP);
        assert!(shown.is_char_boundary(shown.len()));
        assert!(!shown.is_empty());
    }

    #[test]
    fn measure_and_show_do_not_layout_an_8mb_bubble() {
        let src = include_str!("markdown.rs");
        let measure = src
            .split("pub fn measure_text(")
            .nth(1)
            .and_then(|s| s.split("pub fn show(").next())
            .expect("measure_text");
        let show = src
            .split("pub fn show(")
            .nth(1)
            .and_then(|s| s.split("fn wrapping_label(").next())
            .expect("show");
        assert!(
            measure.contains("TEXT_FILE_CAP") || measure.contains("display_text"),
            "measure_text must not layout an 8MB stream body every paint: {measure}"
        );
        assert!(
            show.contains("TEXT_FILE_CAP") || show.contains("display_text"),
            "markdown show must not walk an 8MB stream body every paint: {show}"
        );
    }

    #[test]
    fn fenced_code_body_is_monospace() {
        let src = include_str!("markdown.rs");
        let job = src
            .split("pub(crate) fn code_job(")
            .nth(1)
            .and_then(|s| s.split("fn code_block(").next())
            .expect("code_job");
        assert!(
            job.contains("TextStyle::Monospace"),
            "fence body must stay monospace: {job}"
        );
        assert!(
            src.contains("selectable(true)"),
            "markdown in bubbles must stay selectable so copy does not vanish"
        );
    }

    #[test]
    fn code_block_paints_one_copy_and_a_monospace_body() {
        with_fonts_ui(|ui| {
            let body = "Run this:\n```sh\necho hi\n```\nand this:\n```\nls\n```";
            ui.set_max_width(600.0);
            super::show(ui, body);
            assert!(super::take_code_copy(ui.ctx()).is_none());
            let job = super::code_job(ui, "rust", "let x = 1; // one", 400.0);
            assert!(job.sections.len() >= 4, "{:?}", job.sections.len());
            assert!(job
                .sections
                .iter()
                .all(|s| s.format.font_id.family == eframe::egui::FontFamily::Monospace));
        });
    }

    #[test]
    fn markdown_blocks_render_without_panicking() {
        with_fonts_ui(|ui| {
            ui.set_max_width(500.0);
            let body = "# H1\n## H2\n#### H4\n1. one\n2. two\n   - nested\n- [x] done\n- [ ] open\n> quoted **bold**\n---\n| a | b |\n|:--|--:|\n| `x` | [link](https://x.ai) |\nSee https://github.com and *it* ~~old~~.\n```py\nprint('é')\n";
            super::show(ui, body);
        });
    }

    #[test]
    fn markdown_width_hugs_short_prose_and_fills_for_code() {
        with_fonts_ui(|ui| {
            let wrap = 600.0;
            let short = super::measure_markdown_width(ui, "**Hi** [there](https://x.ai/a/very/long/path/that/is/not/shown)", wrap);
            assert!(short < 120.0, "markers and urls must not widen the bubble: {short}");
            let code = super::measure_markdown_width(ui, "```\nls\n```", wrap);
            assert!((code - wrap).abs() < 0.5, "{code}");
            let table = super::measure_markdown_width(ui, "| a |\n|---|\n| 1 |", wrap);
            assert!((table - wrap).abs() < 0.5, "{table}");
        });
    }

    #[test]
    fn bubble_cap_is_not_the_forced_width() {
        let cap = bubble_width(800.0);
        assert!((cap - bubble_max_width(800.0)).abs() < 0.1);
        assert!(cap < 800.0);
        assert!(
            (cap - 800.0 * BUBBLE_MAX_FRAC).abs() < 0.1,
            "800px pane must wrap at ~84%, got {cap}"
        );
        assert!(bubble_width(100.0) <= 100.0);
        let hugged = bubble_outer_width(800.0, 40.0, BUBBLE_PAD_X);
        assert!(hugged < 120.0);
        assert!(hugged < cap);
    }

    #[test]
    fn measured_short_line_is_narrower_than_the_row_cap() {
        with_fonts_ui(|ui| {
            let wrap = bubble_wrap_width(800.0, BUBBLE_PAD_X);
            let sz = measure_text(ui, "Hi", wrap);
            let outer = bubble_outer_width(800.0, sz.x, BUBBLE_PAD_X);
            assert!(outer < 160.0, "short bubble {outer} content {}", sz.x);
            assert!(sz.y > 8.0);
        });
    }

    #[test]
    fn measured_long_line_wraps_and_grows_taller() {
        with_fonts_ui(|ui| {
            let wrap = bubble_wrap_width(800.0, BUBBLE_PAD_X);
            let short = measure_text(ui, "Hi", wrap);
            let long = measure_text(ui, &"word ".repeat(80), wrap);
            assert!(long.x <= wrap + 1.0);
            assert!(
                long.y > short.y * 2.0,
                "long y {} short y {}",
                long.y,
                short.y
            );
            let outer = bubble_outer_width(800.0, long.x, BUBBLE_PAD_X);
            let cap = bubble_max_width(800.0);
            assert!(outer <= cap + 1.0, "outer {outer} cap {cap}");
            assert!(outer > cap * 0.85, "wrapped bubble too skinny {outer}");
        });
    }

    #[test]
    fn long_sentence_wraps_even_when_available_width_is_huge() {
        with_fonts_ui(|ui| {
            let wrap = bubble_wrap_width(f32::INFINITY, BUBBLE_PAD_X);
            let body = "the clam gods? oh you know... ancient, briny, and extremely picky about their cream-to-broth ratio. they live in the black void between chowder pots, only emerging when someone dares to say manhattan style in their presence.";
            let sz = measure_text(ui, body, wrap);
            assert!(
                sz.x <= wrap + 1.0,
                "sentence must wrap inside {wrap}, got {}",
                sz.x
            );
            assert!(
                sz.y > 28.0,
                "one long sentence must become several lines, height {}",
                sz.y
            );
        });
    }

    #[test]
    fn table_columns_fit_their_widest_cell_and_shrink_to_the_bubble() {
        with_fonts_ui(|ui| {
            let header = vec!["Step".to_string(), "Time".to_string(), "Owner".to_string()];
            let rows = vec![vec!["Build".to_string(), "4m".to_string(), "CI".to_string()]];
            let w = super::table_col_widths(ui, &header, &rows, 600.0, 14.0);
            assert_eq!(w.len(), 3);
            assert!(w.iter().sum::<f32>() + 28.0 < 600.0, "short table hugs: {w:?}");
            assert!(w[0] >= w[2] - 20.0 && w.iter().all(|x| *x >= 24.0), "{w:?}");
            let long = vec![vec!["word ".repeat(60), "x".into(), "y".into()]];
            let w = super::table_col_widths(ui, &header, &long, 300.0, 14.0);
            assert!(w.iter().sum::<f32>() + 28.0 <= 300.0 + 1.0, "wide table shrinks to the bubble: {w:?}");
            assert!(w.iter().all(|x| *x >= 24.0), "{w:?}");
        });
    }

    fn with_fonts_ui(mut add: impl FnMut(&mut eframe::egui::Ui)) {
        let ctx = eframe::egui::Context::default();
        let _ = ctx.run(Default::default(), |ctx| {
            eframe::egui::CentralPanel::default().show(ctx, |ui| add(ui));
        });
    }
}

