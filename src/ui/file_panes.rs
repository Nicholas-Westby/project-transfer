//! The viewer's text panes: only the rows in sight are drawn, lines never
//! wrap, and both panes scroll as one.

use super::theme::{Palette, mix};
use crate::viewer::rows::Row;
use crate::viewer::text::{Text, expand_tabs};
use egui::{
    Align, Color32, FontId, Label, Layout, RichText, ScrollArea, Sense, Ui, UiBuilder, Vec2, vec2,
};
use std::borrow::Cow;
use std::fmt::Debug;
use std::hash::Hash;

/// The height of one line.
pub const ROW: f32 = 18.0;
/// Characters of a line drawn at most; a longer line ends in "…".
const LONGEST: usize = 2_000;
const FONT: f32 = 13.0;
/// Room left of each line.
const PAD: f32 = 6.0;

/// Draws two copies side by side, `height` tall, lined up by `rows` and
/// scrolled to `offset`. Returns the offset for the next frame: wherever
/// the user moved either pane, so both follow.
pub fn side_by_side(
    ui: &mut Ui,
    left: &Text,
    right: &Text,
    rows: &[Row],
    offset: Vec2,
    height: f32,
) -> Vec2 {
    let pal = Palette::of(ui.ctx());
    // Both as wide as the wider, so they scroll the same way sideways too.
    let width = content_width(ui, left.widest.max(right.widest));
    let mut seen = [offset; 2];
    ui.columns(2, |cols| {
        for (i, ui) in cols.iter_mut().enumerate() {
            let text = if i == 0 { left } else { right };
            seen[i] = pane(
                ui,
                ("file_pane", i),
                offset,
                height,
                rows.len(),
                width,
                |ui, r| {
                    let row = rows[r];
                    let line = if i == 0 { row.left } else { row.right };
                    let fill = tint(&pal, row.differs, line.is_some());
                    draw_line(ui, line.map(|n| text.line(n)), width, fill);
                },
            );
        }
    });
    if seen[0] != offset { seen[0] } else { seen[1] }
}

/// Draws one copy's lines, `height` tall, scrolled to `offset`; returns the
/// offset for the next frame.
pub fn single(ui: &mut Ui, text: &Text, offset: Vec2, height: f32) -> Vec2 {
    let width = content_width(ui, text.widest);
    pane(
        ui,
        "file_pane_single",
        offset,
        height,
        text.len(),
        width,
        |ui, r| {
            draw_line(ui, Some(text.line(r)), width, None);
        },
    )
}

/// One scrolling pane that lays out only the rows in sight.
fn pane(
    ui: &mut Ui,
    id: impl Hash + Debug,
    offset: Vec2,
    height: f32,
    rows: usize,
    width: f32,
    mut line: impl FnMut(&mut Ui, usize),
) -> Vec2 {
    ui.scope(|ui| {
        // Rows touch, so a run of tinted lines reads as one block.
        ui.spacing_mut().item_spacing.y = 0.0;
        ScrollArea::both()
            .id_salt(id)
            .auto_shrink([false, false])
            .max_height(height)
            .scroll_offset(offset)
            .show_rows(ui, ROW, rows, |ui, range| {
                ui.set_min_width(width);
                for r in range {
                    line(ui, r);
                }
            })
            .state
            .offset
    })
    .inner
}

/// How wide a pane's content is for lines `columns` wide.
fn content_width(ui: &Ui, columns: usize) -> f32 {
    let font = FontId::monospace(FONT);
    let glyph = ui.ctx().fonts_mut(|f| f.glyph_width(&font, '0'));
    columns.min(LONGEST + 1) as f32 * glyph + 2.0 * PAD
}

/// A changed line, or one only this side has, in the change color; the gap
/// that faces a line only the other side has in a neutral one.
fn tint(pal: &Palette, differs: bool, has_line: bool) -> Option<Color32> {
    match (differs, has_line) {
        (false, _) => None,
        (true, true) => Some(mix(pal.surface, pal.changed, 0.22)),
        (true, false) => Some(mix(pal.surface, pal.ink, 0.06)),
    }
}

fn draw_line(ui: &mut Ui, text: Option<&str>, width: f32, fill: Option<Color32>) {
    let (rect, _) = ui.allocate_exact_size(vec2(width, ROW), Sense::hover());
    if let Some(fill) = fill {
        ui.painter().rect_filled(rect, 0.0, fill);
    }
    let Some(text) = text else {
        return;
    };
    let ink = Palette::of(ui.ctx()).ink;
    let shown = shown_line(text);
    let inner = UiBuilder::new()
        .max_rect(rect.shrink2(vec2(PAD, 0.0)))
        .layout(Layout::left_to_right(Align::Center));
    ui.scope_builder(inner, |ui| {
        let text = RichText::new(shown)
            .font(FontId::monospace(FONT))
            .color(ink);
        ui.add(Label::new(text).extend());
    });
}

/// `text` as drawn: tabs as spaces, cut to `LONGEST` characters. Only its
/// start is expanded, since a minified line can be a megabyte.
fn shown_line(text: &str) -> Cow<'_, str> {
    let head = text
        .char_indices()
        .nth(LONGEST + 1)
        .map_or(text, |(i, _)| &text[..i]);
    cut(expand_tabs(head))
}

/// `line`, cut to `LONGEST` characters for drawing.
fn cut(line: Cow<'_, str>) -> Cow<'_, str> {
    match line.char_indices().nth(LONGEST) {
        Some((i, _)) => Cow::Owned(format!("{}…", &line[..i])),
        None => line,
    }
}

#[cfg(test)]
#[path = "file_panes_tests.rs"]
mod tests;
