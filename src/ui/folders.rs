//! The folders table: where each folder lives here and on the peer.

use super::theme::Palette;
use super::widgets::{self, display_path};
use super::{App, Confirm, peer_name};
use crate::core::{Action, UiState};
use crate::model::{Folder, Project};
use crate::net::may_ask;
use crate::protocol::Request;
use egui::{Grid, Popup, RichText, Ui};

pub const NOT_SET_UP_HERE: &str = "Choose where this folder lives on this computer.";

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
        let head = |ui: &mut Ui, t: &str| {
            ui.label(RichText::new(t).small().color(pal.muted()));
        };
        Grid::new(("folders", p.id))
            .num_columns(4)
            .spacing([28.0, 6.0])
            .min_col_width(80.0)
            .show(ui, |ui| {
                widgets::section(ui, "Folders");
                head(ui, "Here");
                match &peer {
                    Some(name) => head(ui, &format!("On {name}")),
                    None => head(ui, "On the other computer"),
                }
                ui.label("");
                ui.end_row();

                for f in &p.folders {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&f.name).color(pal.ink));
                        if f.id == p.primary {
                            widgets::badge(ui, "Primary")
                                .on_hover_text("Commands run in the primary folder.");
                        }
                    });
                    match &f.local_path {
                        Some(path) => {
                            ui.label(widgets::mono(display_path(path)).color(pal.ink));
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
                    }
                    let there = remote
                        .and_then(|r| r.folders.iter().find(|rf| rf.id == f.id))
                        .and_then(|rf| rf.path.clone());
                    match (there, peer.is_some(), online || remote.is_some()) {
                        (Some(path), _, _) => {
                            ui.label(widgets::mono(path).color(pal.ink));
                        }
                        (None, false, _) => {
                            widgets::muted(ui, "No computer selected");
                        }
                        (None, true, false) => {
                            widgets::muted(ui, "Unknown while offline");
                        }
                        (None, true, true) if !shares => {
                            let name = peer.as_deref().unwrap_or_default();
                            widgets::muted(ui, "Not shared with this computer").on_hover_text(
                                format!(
                                    "{name} lets this computer neither push nor pull, so it \
                                     doesn't say where its folders are."
                                ),
                            );
                        }
                        (None, true, true) => {
                            widgets::muted(ui, "Not set up");
                        }
                    }
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
