//! The folders table: where each folder lives here and on the peer.

use super::theme::Palette;
use super::widgets::{self, display_path};
use super::{App, Confirm, peer_name};
use crate::core::{Action, UiState};
use crate::model::{Folder, Project};
use crate::net::may_ask;
use crate::protocol::Request;
use egui::{Grid, Popup, RichText, TextWrapMode, Ui};

pub const NOT_SET_UP_HERE: &str = "Choose where this folder lives on this computer.";

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
        let here: Vec<Option<RichText>> = p
            .folders
            .iter()
            .map(|f| Some(widgets::mono(display_path(f.local_path.as_ref()?)).color(pal.ink)))
            .collect();
        // Each with the hover text it needs, if any.
        let there: Vec<(RichText, Option<String>)> = p
            .folders
            .iter()
            .map(|f| {
                let path = remote
                    .and_then(|r| r.folders.iter().find(|rf| rf.id == f.id))
                    .and_then(|rf| rf.path.clone());
                match (path, peer.is_some(), online || remote.is_some()) {
                    (Some(path), _, _) => (widgets::mono(path).color(pal.ink), None),
                    (None, false, _) => (muted("No computer selected"), None),
                    (None, true, false) => (muted("Unknown while offline"), None),
                    (None, true, true) if !shares => {
                        let name = peer.as_deref().unwrap_or_default();
                        let why = format!(
                            "{name} lets this computer neither push nor pull, so it \
                             doesn't say where its folders are."
                        );
                        (muted("Not shared with this computer"), Some(why))
                    }
                    (None, true, true) => (muted("Not set up"), None),
                }
            })
            .collect();
        let badge = widgets::text_width(ui, RichText::new("Primary").small())
            + 2.0 * f32::from(widgets::BADGE_PAD)
            + ui.spacing().item_spacing.x;
        let natural = [
            widest(
                0.0,
                p.folders.iter().map(|f| {
                    let name = widgets::text_width(ui, RichText::new(&f.name));
                    if f.id == p.primary {
                        name + badge
                    } else {
                        name
                    }
                }),
            ),
            widest(
                widgets::text_width(ui, head("Here")),
                here.iter()
                    .flatten()
                    .map(|t| widgets::text_width(ui, t.clone())),
            ),
            widest(
                widgets::text_width(ui, head(&there_head)),
                there
                    .iter()
                    .map(|(t, _)| widgets::text_width(ui, t.clone())),
            ),
        ];
        let [name_w, here_w, there_w] = column_widths(ui.available_width(), natural);
        Grid::new(("folders", p.id))
            .num_columns(4)
            .spacing([GAP, 6.0])
            // Each cell sets its own column's width below; the last column
            // is only as wide as its button.
            .min_col_width(MORE)
            .max_col_width(name_w.max(here_w).max(there_w))
            .show(ui, |ui| {
                cell(ui, name_w, |ui| widgets::section(ui, "Folders"));
                cell(ui, here_w, |ui| ui.label(head("Here")));
                cell(ui, there_w, |ui| ui.label(head(&there_head)));
                ui.label("");
                ui.end_row();

                for ((f, here), (there, why)) in p.folders.iter().zip(here).zip(there) {
                    cell(ui, name_w, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&f.name).color(pal.ink));
                            if f.id == p.primary {
                                widgets::badge(ui, "Primary")
                                    .on_hover_text("Commands run in the primary folder.");
                            }
                        })
                    });
                    cell(ui, here_w, |ui| match here {
                        Some(path) => {
                            ui.label(path);
                        }
                        None => {
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 0.0;
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(NOT_SET_UP_HERE).color(pal.changed),
                                    )
                                    .wrap(),
                                );
                                if widgets::quiet(ui, "Choose folder", Some(pal.route)).clicked() {
                                    self.pick_path(p, f);
                                }
                            });
                        }
                    });
                    cell(ui, there_w, |ui| {
                        let r = ui.label(there);
                        if let Some(why) = why {
                            r.on_hover_text(why);
                        }
                    });
                    self.folder_actions(ui, p, f);
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
    fn folder_actions(&mut self, ui: &mut Ui, p: &Project, f: &Folder) {
        let pal = Palette::of(ui.ctx());
        ui.horizontal(|ui| {
            let more = widgets::more_button(ui, &format!("More for {}", f.name));
            Popup::menu(&more).show(|ui| {
                if f.local_path.is_some() && ui.button("Change folder…").clicked() {
                    self.pick_path(p, f);
                }
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

/// The widths of the name, here and there columns. Each keeps what it
/// needs while the table fits; otherwise the paths share what is left,
/// and a short one keeps its width.
fn column_widths(available: f32, [name, here, there]: [f32; 3]) -> [f32; 3] {
    let room = (available - name - MORE - 3.0 * GAP).max(2.0 * MIN_COL);
    let half = room / 2.0;
    if here + there <= room {
        [name, here, there]
    } else if here <= half {
        [name, here, room - here]
    } else if there <= half {
        [name, room - there, there]
    } else {
        [name, half, half]
    }
}

#[cfg(test)]
mod tests {
    use super::column_widths;

    #[test]
    fn paths_give_up_room_only_when_the_table_does_not_fit() {
        // The "More" button and three gaps take 112 points.
        let cases = [
            // Everything fits: each column keeps what it needs.
            (800.0, [100.0, 300.0, 200.0], [100.0, 300.0, 200.0]),
            // The short path keeps its width; the long one gets the rest.
            (590.0, [100.0, 320.0, 120.0], [100.0, 258.0, 120.0]),
            (590.0, [100.0, 120.0, 320.0], [100.0, 120.0, 258.0]),
            // Both long: they share the room.
            (590.0, [100.0, 320.0, 300.0], [100.0, 189.0, 189.0]),
            // Never narrower than the smallest column.
            (300.0, [100.0, 320.0, 300.0], [100.0, 80.0, 80.0]),
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
