//! The selected project on a Surface panel: title, folders, push and pull,
//! commands, and when it last moved.

use super::theme::{Palette, SHEET_RADIUS, title_style};
use super::widgets::{self, ago, plural};
use super::{App, Confirm, peer_name};
use crate::core::{Action, UiState};
use crate::model::{Direction, Project};
use crate::transfer::projects::now_ms;
use egui::{
    Align, CentralPanel, Frame, Key, Layout, Margin, RichText, ScrollArea, Sense, TextEdit, Ui,
};

pub const NO_PROJECTS: &str = "Add a project to start. Pick its folder on this computer first.";

impl App {
    pub(super) fn project_view(&mut self, ui: &mut Ui, s: &UiState) {
        let pal = Palette::of(ui.ctx());
        CentralPanel::default()
            .frame(Frame::new().fill(pal.ground).inner_margin(Margin {
                left: 0,
                right: 16,
                top: 16,
                bottom: 16,
            }))
            .show(ui, |ui| {
                Frame::new()
                    .fill(pal.surface)
                    .corner_radius(SHEET_RADIUS)
                    .inner_margin(Margin::symmetric(28, 20))
                    .show(ui, |ui| {
                        ui.set_min_size(ui.available_size());
                        let project = self.view.selected.and_then(|id| s.project(id)).cloned();
                        match project {
                            Some(p) => {
                                ScrollArea::vertical()
                                    .auto_shrink([false, false])
                                    .show(ui, |ui| self.project_body(ui, s, &p));
                            }
                            None => self.no_projects(ui),
                        }
                    });
            });
    }

    fn no_projects(&mut self, ui: &mut Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(ui.available_height() * 0.32);
            widgets::heading(ui, "No projects yet");
            ui.add_space(4.0);
            widgets::muted(ui, NO_PROJECTS);
            ui.add_space(14.0);
            if widgets::primary(ui, "Add a project").clicked() {
                self.start_new_project();
            }
        });
    }

    fn project_body(&mut self, ui: &mut Ui, s: &UiState, p: &Project) {
        self.title_row(ui, p);
        ui.add_space(12.0);
        self.folders(ui, s, p);
        ui.add_space(16.0);
        self.transfer_bar(ui, s, p);
        ui.add_space(20.0);
        self.commands_section(ui, s, p);
        ui.add_space(16.0);
        last_transfer(ui, s, p);
    }

    fn title_row(&mut self, ui: &mut Ui, p: &Project) {
        let pal = Palette::of(ui.ctx());
        ui.horizontal(|ui| {
            let editing = self
                .view
                .rename_project
                .take()
                .filter(|(id, _)| *id == p.id);
            if let Some((id, mut draft)) = editing {
                let r = ui.add(
                    TextEdit::singleline(&mut draft)
                        .font(title_style())
                        .desired_width(360.0),
                );
                r.request_focus();
                if ui.input(|i| i.key_pressed(Key::Escape)) {
                    return;
                }
                let problem = crate::naming::name_problem("Project", draft.trim())
                    .filter(|_| !draft.trim().is_empty());
                if let Some(why) = &problem {
                    widgets::error_text(ui, &format!("{why} Choose another name."));
                }
                let done = ui.input(|i| i.key_pressed(Key::Enter)) || r.lost_focus();
                // A name other computers can't use keeps the field open.
                if done && problem.is_none() {
                    let name = draft.trim();
                    if !name.is_empty() && name != p.name {
                        self.act(Action::RenameProject(id, name.to_string()));
                    }
                    return;
                }
                self.view.rename_project = Some((id, draft));
            } else {
                let r = ui
                    .add(
                        egui::Label::new(
                            RichText::new(&p.name)
                                .text_style(title_style())
                                .color(pal.ink),
                        )
                        .sense(Sense::click()),
                    )
                    .on_hover_cursor(egui::CursorIcon::Text)
                    .on_hover_text("Click to rename");
                if r.clicked() {
                    self.view.rename_project = Some((p.id, p.name.clone()));
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if widgets::quiet(ui, "Delete project", Some(pal.removed)).clicked() {
                    self.view.confirm = Some(Confirm::DeleteProject(p.id));
                }
            });
        });
    }
}

fn last_transfer(ui: &mut Ui, s: &UiState, p: &Project) {
    let Some(t) = &p.last_transfer else {
        widgets::small_muted(ui, "Not transferred yet.");
        return;
    };
    let who = s
        .peer(t.peer)
        .map(|v| v.peer.name.clone())
        .or_else(|| peer_name(s))
        .unwrap_or_else(|| "a computer that is no longer paired".into());
    let verb = match (t.by_peer, t.direction) {
        (false, Direction::Push) => format!("Last pushed to {who}"),
        (false, Direction::Pull) => format!("Last pulled from {who}"),
        (true, Direction::Push) => format!("{who} last pushed here"),
        (true, Direction::Pull) => format!("{who} last pulled from here"),
    };
    widgets::small_muted(
        ui,
        format!(
            "{verb} {}, {}.",
            ago(now_ms(), t.at_ms),
            plural(t.files, "file", "files")
        ),
    );
}
