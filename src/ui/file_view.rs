//! The viewer over a transfer's preview: one file, as each computer has
//! it.

use super::dialogs;
use super::file_panes::{side_by_side, single};
use super::theme::Palette;
use super::widgets::{self, ago};
use crate::model::Direction;
use crate::transfer::projects::now_ms;
use crate::units::size;
use crate::viewer::text::{Body, Endings, Text};
use crate::viewer::{Alike, Copies, FileView, Pane, Shown};
use egui::{Color32, Id, RichText, Ui, Vec2};

/// Room the sheet keeps around the panes for its title, notes, headers and
/// button, so the button stays in the window.
const AROUND_PANES: f32 = 330.0;

/// Which computer's copy a pane shows.
#[derive(Clone, Copy, PartialEq)]
enum Side {
    Here,
    There,
}

/// Draws the viewer for `view` over a preview going `direction`; `folder`
/// names the file's folder when the project has several. True once it asks
/// to close: Close, Escape, or a click beside it.
pub fn sheet(
    ui: &mut Ui,
    view: &FileView,
    direction: Direction,
    peer: &str,
    folder: Option<&str>,
) -> bool {
    let wide = (ui.ctx().content_rect().width() - 96.0).max(400.0);
    let title = match folder {
        Some(folder) => format!("{} in {folder}", view.ask.rel),
        None => view.ask.rel.clone(),
    };
    let r = dialogs::sheet(ui, "file_view", wide, true, |ui| {
        dialogs::sheet_title(ui, &title);
        match &view.shown {
            None => loading(ui, view.ask.copies, peer),
            Some(shown) => body(ui, view, shown, direction, peer),
        }
        ui.add_space(12.0);
        dialogs::right_row(ui, |ui| widgets::primary(ui, "Close").clicked()).inner
    });
    r.inner || r.dismissed
}

fn loading(ui: &mut Ui, copies: Copies, peer: &str) {
    let what = match (copies.here(), copies.there()) {
        (true, true) => format!("Reading the file here and on {peer}…"),
        (false, _) => format!("Reading the file on {peer}…"),
        (true, false) => "Reading the file here…".to_string(),
    };
    ui.horizontal(|ui| {
        ui.spinner();
        widgets::muted(ui, what);
    });
}

fn body(ui: &mut Ui, view: &FileView, shown: &Shown, direction: Direction, peer: &str) {
    let copies = view.ask.copies;
    let endings = endings_note(shown, peer);
    let alike = alike_note(shown, endings.is_some()).map(String::from);
    for line in [note(copies, direction, peer), endings, alike]
        .into_iter()
        .flatten()
    {
        ui.add(egui::Label::new(RichText::new(line).color(Palette::of(ui.ctx()).muted())).wrap());
    }
    ui.add_space(6.0);
    let height = (ui.ctx().content_rect().height() - AROUND_PANES).max(160.0);
    // Each opening starts at the top, even of a file opened before.
    let id = Id::new(("file_view_scroll", view.opened));
    let offset = ui
        .ctx()
        .data(|d| d.get_temp::<Vec2>(id))
        .unwrap_or_default();
    let next = match (&shown.here, &shown.there) {
        (Some(here), Some(there)) => {
            ui.columns(2, |cols| {
                header(&mut cols[0], Side::Here, here, copies, direction, peer);
                header(&mut cols[1], Side::There, there, copies, direction, peer);
            });
            match (text_of(here), text_of(there)) {
                (Some(a), Some(b)) => side_by_side(ui, a, b, &shown.rows, offset, height),
                _ => {
                    let mut next = offset;
                    ui.columns(2, |cols| {
                        for (ui, pane) in cols.iter_mut().zip([here, there]) {
                            next = content(ui, pane, offset, height).unwrap_or(next);
                        }
                    });
                    next
                }
            }
        }
        (Some(here), None) if shown.waiting => {
            let mut next = offset;
            ui.columns(2, |cols| {
                header(&mut cols[0], Side::Here, here, copies, direction, peer);
                next = content(&mut cols[0], here, offset, height).unwrap_or(offset);
                cols[1].horizontal(|ui| {
                    ui.spinner();
                    widgets::muted(ui, format!("Reading the copy on {peer}…"));
                });
            });
            next
        }
        (Some(only), None) | (None, Some(only)) => {
            let side = if shown.here.is_some() {
                Side::Here
            } else {
                Side::There
            };
            header(ui, side, only, copies, direction, peer);
            content(ui, only, offset, height).unwrap_or(offset)
        }
        (None, None) => offset,
    };
    if next != offset {
        ui.ctx().data_mut(|d| d.insert_temp(id, next));
        // The other pane catches up on the next frame.
        ui.ctx().request_repaint();
    }
}

/// One copy's name, size, age and what the transfer does to it.
fn header(
    ui: &mut Ui,
    side: Side,
    pane: &Result<Pane, String>,
    copies: Copies,
    direction: Direction,
    peer: &str,
) {
    let pal = Palette::of(ui.ctx());
    let name = match side {
        Side::Here => "Here".to_string(),
        Side::There => format!("On {peer}"),
    };
    let (role, color) = role(copies, side, direction, &pal);
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(name).strong().color(pal.ink));
        if let Ok(p) = pane {
            let when = ago(now_ms(), p.mtime_ms);
            widgets::small_muted(ui, format!("{}, modified {when}", size(p.size)));
        }
        ui.label(RichText::new(role).small().strong().color(color));
    });
    if let Ok(p) = pane
        && p.cut
    {
        let first = size(p.shown);
        widgets::small_muted(
            ui,
            format!("Showing the first {first} of {}.", size(p.size)),
        );
    }
}

/// What the transfer does to the copy on `side`.
fn role(
    copies: Copies,
    side: Side,
    direction: Direction,
    pal: &Palette,
) -> (&'static str, Color32) {
    let source = matches!(
        (side, direction),
        (Side::Here, Direction::Push) | (Side::There, Direction::Pull)
    );
    match copies {
        Copies::SameContent => ("Same content", pal.muted()),
        _ if source => ("Sent", pal.route),
        Copies::Both => ("Replaced", pal.changed),
        Copies::Here | Copies::There => ("Removed", pal.removed),
    }
}

/// A pane's lines, or what stands in for them. Returns the scroll offset a
/// pane of lines moved to.
fn content(ui: &mut Ui, pane: &Result<Pane, String>, offset: Vec2, height: f32) -> Option<Vec2> {
    let pal = Palette::of(ui.ctx());
    match pane {
        Err(why) => {
            widgets::error_text(ui, why);
            None
        }
        Ok(Pane {
            body: Body::Binary,
            size: n,
            ..
        }) => {
            let why = format!(
                "This copy isn't text, so it can't be shown. It is {}.",
                size(*n)
            );
            ui.add(egui::Label::new(RichText::new(why).color(pal.muted())).wrap());
            None
        }
        Ok(Pane {
            body: Body::Text(t),
            ..
        }) if t.is_empty() => {
            widgets::muted(ui, "This copy is empty.");
            None
        }
        Ok(Pane {
            body: Body::Text(t),
            ..
        }) => Some(single(ui, t, offset, height)),
    }
}

fn text_of(pane: &Result<Pane, String>) -> Option<&Text> {
    match pane {
        Ok(Pane {
            body: Body::Text(t),
            ..
        }) => Some(t),
        _ => None,
    }
}

/// What a single pane, or the same content, means for this transfer.
fn note(copies: Copies, direction: Direction, peer: &str) -> Option<String> {
    Some(match (copies, direction) {
        (Copies::Both, _) => return None,
        (Copies::SameContent, _) => {
            format!("Only the modified time differs. The content is the same here and on {peer}.")
        }
        (Copies::Here, Direction::Push) => {
            format!("Only this computer has this file. The push copies it to {peer}.")
        }
        (Copies::Here, Direction::Pull) => {
            "Only this computer has this file. The pull removes it here.".to_string()
        }
        (Copies::There, Direction::Push) => {
            format!("Only {peer} has this file. The push removes it there.")
        }
        (Copies::There, Direction::Pull) => {
            format!("Only {peer} has this file. The pull copies it here.")
        }
    })
}

/// Said when the copies end their lines differently, which lining them up
/// leaves out.
fn endings_note(shown: &Shown, peer: &str) -> Option<String> {
    let (Some(here), Some(there)) = (&shown.here, &shown.there) else {
        return None;
    };
    let (a, b) = (text_of(here)?.endings, text_of(there)?.endings);
    if a == b || a == Endings::None || b == Endings::None {
        return None;
    }
    Some(format!(
        "Line endings differ: {} here, {} on {peer}. Lines are lined up without them.",
        a.name(),
        b.name()
    ))
}

/// Said when every line matches, so two panes that look the same still
/// say why the file is listed.
fn alike_note(shown: &Shown, endings_said: bool) -> Option<&'static str> {
    let cut = [&shown.here, &shown.there]
        .iter()
        .any(|p| matches!(p, Some(Ok(p)) if p.cut));
    Some(match shown.alike? {
        _ if cut => {
            "The part shown reads the same in both copies. The difference is further on in \
             the file, or in something the viewer doesn't show."
        }
        Alike::Lines if endings_said => return None,
        Alike::Lines => {
            "The lines are the same in both copies. They differ only in line endings, a \
             byte-order mark or the line break at the end."
        }
        Alike::Bytes => {
            "Both copies have the same content. The transfer rewrites this file for another \
             reason, most likely whether it can be run as a program."
        }
    })
}

#[cfg(test)]
#[path = "file_view_tests.rs"]
mod tests;
