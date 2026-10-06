//! The folders table: where each folder lives here and on the peer.

use super::path_cut::path_cut;
use super::theme::Palette;
use super::widgets::{self, display_path};
use super::{App, Confirm, peer_name};
use crate::core::{Action, UiState};
use crate::model::{Folder, Project};
use crate::net::may_ask;
use crate::protocol::Request;
use egui::{Grid, Popup, RichText, TextWrapMode, Ui};

/// The gap between columns, the "More" button that ends each row, and the
/// narrowest a column gets.
const GAP: f32 = 28.0;
const MORE: f32 = 28.0;
const MIN_COL: f32 = 80.0;

impl App {
    pub(super) fn folders(&mut self, ui: &mut Ui, s: &UiState, p: &Project) {
        let pal = Palette::of(ui.ctx());
        let peer = peer_name(s);
        let remote = s.remote_projects.get(&p.id);
        let online = s
            .selected_peer
            .and_then(|id| s.peer(id))
            .is_some_and(|v| v.online);
        // The status poll only asks for what the peer will share, so a
        // missing path may be one it keeps to itself rather than one it lacks.
        let shares = s
            .selected_peer
            .and_then(|id| s.peer(id))
            .is_some_and(|v| may_ask(&Request::ProjectInfo { project: p.id }, v.peer.granted));
        let head = |t: &str| RichText::new(t).small().color(pal.muted());
        let muted = |t: &str| RichText::new(t).color(pal.muted());
        let there_head = match &peer {
            Some(name) => format!("On {name}"),
            None => "On the other computer".into(),
        };
        // Worked out before drawing, so the columns can be measured.
        let here: Vec<Option<String>> = p
            .folders
            .iter()
            .map(|f| Some(display_path(f.local_path.as_ref()?)))
            .collect();
        // A path, or a note with the hover text it needs, if any.
        let there: Vec<Result<String, (&str, Option<String>)>> = p
            .folders
            .iter()
            .map(|f| {
                let path = remote
                    .and_then(|r| r.folders.iter().find(|rf| rf.id == f.id))
                    .and_then(|rf| rf.shown.clone().or_else(|| rf.path.clone()));
                match (path, peer.is_some(), online || remote.is_some()) {
                    (Some(path), _, _) => Ok(path),
                    (None, false, _) => Err(("No computer selected", None)),
                    (None, true, false) => Err(("Unknown while offline", None)),
                    (None, true, true) if !shares => {
                        let name = peer.as_deref().unwrap_or_default();
                        let why = format!(
                            "{name} lets this computer neither push nor pull, so it \
                             doesn't say where its folders are."
                        );
                        Err(("Not shared with this computer", Some(why)))
                    }
                    (None, true, true) => Err(("Not set up", None)),
                }
            })
            .collect();
        let badge = widgets::text_width(ui, RichText::new("Primary").small())
            + 2.0 * f32::from(widgets::BADGE_PAD)
            + ui.spacing().item_spacing.x;
        let natural = [
            widest(
                widgets::text_width(ui, head("Here")),
                p.folders.iter().zip(&here).map(|(f, t)| {
                    let room = if f.id == p.primary { badge } else { 0.0 };
                    room + match t {
                        Some(path) => widgets::text_width(ui, widgets::mono(path)),
                        // It wraps, but gets one line when there is room.
                        None => widgets::text_width(ui, not_set_up_here(f)),
                    }
                }),
            ),
            widest(
                widgets::text_width(ui, head(&there_head)),
                there.iter().map(|t| match t {
                    Ok(path) => widgets::text_width(ui, widgets::mono(path)),
                    Err((note, _)) => widgets::text_width(ui, muted(note)),
                }),
            ),
        ];
        let [here_w, there_w] = column_widths(ui.available_width(), natural);
        widgets::section(ui, "Folders");
        Grid::new(("folders", p.id))
            .num_columns(3)
            .spacing([GAP, 6.0])
            // Each cell sets its own column's width below; the last column
            // is only as wide as its button.
            .min_col_width(MORE)
            .max_col_width(here_w.max(there_w))
            .show(ui, |ui| {
                cell(ui, here_w, |ui| ui.label(head("Here")));
                cell(ui, there_w, |ui| ui.label(head(&there_head)));
                ui.label("");
                ui.end_row();

                for ((f, here), there) in p.folders.iter().zip(here).zip(there) {
                    let primary = f.id == p.primary;
                    cell(ui, here_w, |ui| match here {
                        Some(path) => {
                            ui.horizontal(|ui| {
                                // The badge stays whole; a long path gives way.
                                let room = if primary { badge } else { 0.0 };
                                let width = (ui.available_width() - room).max(0.0);
                                ui.scope(|ui| {
                                    ui.set_max_width(width);
                                    path_label(ui, &path, width);
                                });
                                if primary {
                                    primary_badge(ui);
                                }
                            });
                        }
                        None => {
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 0.0;
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(not_set_up_here(f)).color(pal.changed),
                                    )
                                    .wrap(),
                                );
                                ui.horizontal(|ui| {
                                    if widgets::quiet(ui, "Choose folder", Some(pal.route))
                                        .clicked()
                                    {
                                        self.pick_path(p, f);
                                    }
                                    if primary {
                                        primary_badge(ui);
                                    }
                                });
                            });
                        }
                    });
                    cell(ui, there_w, |ui| match there {
                        Ok(path) => {
                            path_label(ui, &path, there_w);
                        }
                        Err((note, why)) => {
                            let r = ui.label(muted(note));
                            if let Some(why) = why {
                                r.on_hover_text(why);
                            }
                        }
                    });
                    self.folder_actions(ui, s, p, f);
                    ui.end_row();
                }
            });
        ui.add_space(2.0);
        if widgets::quiet(ui, "Add folder", Some(pal.route)).clicked()
            && let Some(path) = self.pick_folder("Choose a folder to add to this project")
        {
            self.act(Action::AddFolder(p.id, path));
        }
    }

    /// The one common fix stays visible; the rest sit in a small menu so
    /// the table keeps room for paths.
    fn folder_actions(&mut self, ui: &mut Ui, s: &UiState, p: &Project, f: &Folder) {
        let pal = Palette::of(ui.ctx());
        ui.horizontal(|ui| {
            let more = widgets::more_button(ui, &format!("More for {}", f.name));
            Popup::menu(&more).show(|ui| {
                if f.local_path.is_some() && ui.button("Change folder here…").clicked() {
                    self.pick_path(p, f);
                }
                self.peer_folder_entry(ui, s, p, f);
                if f.id != p.primary && ui.button("Make primary").clicked() {
                    self.act(Action::SetPrimary(p.id, f.id));
                }
                let can_remove = p.folders.len() > 1;
                let remove = ui
                    .add_enabled(
                        can_remove,
                        egui::Button::new(RichText::new("Remove from project").color(pal.removed)),
                    )
                    .on_disabled_hover_text("A project needs at least one folder.");
                if remove.clicked() {
                    self.view.confirm = Some(Confirm::RemoveFolder(p.id, f.id));
                }
            });
        });
    }

    fn pick_path(&mut self, p: &Project, f: &Folder) {
        if let Some(path) = self.pick_folder(&format!("Choose where {} lives", f.name)) {
            self.act(Action::SetFolderPath(p.id, f.id, path));
        }
    }
}

fn not_set_up_here(f: &Folder) -> String {
    format!("Choose where {} lives on this computer.", f.name)
}

fn primary_badge(ui: &mut Ui) {
    widgets::badge(ui, "Primary").on_hover_text("Commands run in the primary folder.");
}

/// A path cut down to `width` from the middle, so its last folder stays;
/// hovering shows it whole.
fn path_label(ui: &mut Ui, path: &str, width: f32) {
    let shown = path_cut(path, |t| widgets::text_width(ui, widgets::mono(t)) <= width);
    let ink = Palette::of(ui.ctx()).ink;
    let r = ui.label(widgets::mono(&shown).color(ink));
    if shown != path {
        r.on_hover_text(path);
    }
}

/// The widest of a column's texts, and never under the narrowest column.
fn widest(head: f32, rest: impl Iterator<Item = f32>) -> f32 {
    rest.fold(head.max(MIN_COL), f32::max)
}

/// A cell as wide as its column; text that doesn't fit ends in "…" and
/// shows in full on hover.
fn cell<R>(ui: &mut Ui, width: f32, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.scope(|ui| {
        ui.set_width(width);
        ui.style_mut().wrap_mode = Some(TextWrapMode::Truncate);
        add(ui)
    })
    .inner
}

/// The widths of the columns before "More". Each keeps what it needs while
/// the table fits; otherwise the widest give way first, down to one width
/// they share, so short columns keep theirs.
fn column_widths<const N: usize>(available: f32, natural: [f32; N]) -> [f32; N] {
    let n = N as f32;
    let room = (available - MORE - n * GAP).max(n * MIN_COL);
    if natural.iter().sum::<f32>() <= room {
        return natural;
    }
    let mut narrowest_first = natural;
    narrowest_first.sort_by(f32::total_cmp);
    let mut left = room;
    let mut cap = f32::INFINITY;
    for (i, width) in narrowest_first.into_iter().enumerate() {
        let share = left / (N - i) as f32;
        if width > share {
            cap = share;
            break;
        }
        left -= width;
    }
    natural.map(|w| w.min(cap))
}

#[cfg(test)]
mod tests {
    use super::column_widths;

    #[test]
    fn the_widest_columns_give_way_only_when_the_table_does_not_fit() {
        // The "More" button and three gaps take 112 points.
        let cases = [
            // Everything fits: each column keeps what it needs.
            (800.0, [100.0, 300.0, 200.0], [100.0, 300.0, 200.0]),
            // The short columns keep their width; the long one gets the rest.
            (590.0, [100.0, 320.0, 120.0], [100.0, 258.0, 120.0]),
            (590.0, [100.0, 120.0, 320.0], [100.0, 120.0, 258.0]),
            (590.0, [400.0, 100.0, 120.0], [258.0, 100.0, 120.0]),
            // Two long ones share what the short one leaves.
            (590.0, [100.0, 320.0, 300.0], [100.0, 189.0, 189.0]),
            // All long: they share alike.
            (592.0, [420.0, 320.0, 218.0], [160.0, 160.0, 160.0]),
            // Never narrower than the smallest column.
            (300.0, [100.0, 320.0, 300.0], [80.0, 80.0, 80.0]),
        ];
        for (available, natural, want) in cases {
            assert_eq!(
                column_widths(available, natural),
                want,
                "{available} {natural:?}"
            );
        }
    }
}
