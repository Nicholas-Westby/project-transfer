//! Projects the selected computer has and this one doesn't, under the local
//! ones in the project list, each pullable into a new local project.

use super::App;
use super::theme::{Palette, semibold};
use crate::core::{Action, TransferState, UiState};
use crate::model::{Direction, ProjectId};
use crate::transfer::TransferRequest;
use egui::{Align, FontId, Layout, RichText, Ui};

/// The selected peer's projects that are not here, sorted by name.
pub fn remote_only(s: &UiState) -> Vec<(ProjectId, String)> {
    let mut list: Vec<(ProjectId, String)> = s
        .remote_projects
        .iter()
        .filter(|(id, _)| s.project(**id).is_none())
        .map(|(id, p)| (*id, p.name.clone()))
        .collect();
    list.sort_by(|a, b| {
        a.1.to_lowercase()
            .cmp(&b.1.to_lowercase())
            .then(a.0.cmp(&b.0))
    });
    list
}

/// Why pulling a project that is not here can't start, or None when it can.
pub fn pull_blocked(s: &UiState) -> Option<String> {
    let peer = s.selected_peer.and_then(|id| s.peer(id))?;
    let name = &peer.peer.name;
    if !peer.peer.granted.may_pull_from_me {
        return Some(format!(
            "{name} doesn't allow pulls to this computer. Allow it on {name}."
        ));
    }
    if !matches!(s.transfer, TransferState::Idle) {
        return Some("Another transfer is open. Finish it first.".into());
    }
    None
}

impl App {
    pub(super) fn remote_projects(&mut self, ui: &mut Ui, s: &UiState) {
        let list = remote_only(s);
        let peer = s.selected_peer.and_then(|id| s.peer(id));
        // An offline peer's list is from its last answer and may be stale.
        let Some(peer) = peer.filter(|p| p.online && !list.is_empty()) else {
            return;
        };
        let pal = Palette::of(ui.ctx());
        let blocked = pull_blocked(s);
        ui.add_space(14.0);
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            ui.add(
                egui::Label::new(
                    RichText::new(format!("Only on {}", peer.peer.name))
                        .font(FontId::new(15.0, semibold()))
                        .color(pal.muted()),
                )
                .truncate(),
            );
        });
        if let Some(why) = &blocked {
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                ui.add(egui::Label::new(RichText::new(why).small().color(pal.muted())).wrap());
            });
        }
        ui.add_space(4.0);
        let mut pull = None;
        for (id, name) in &list {
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let button = ui.add_enabled(blocked.is_none(), egui::Button::new("Pull"));
                    if button.clicked() {
                        pull = Some(*id);
                    }
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        ui.add(egui::Label::new(RichText::new(name).color(pal.muted())).truncate());
                    });
                });
            });
        }
        if let Some(project) = pull {
            // Selected once the pull has created it here.
            self.view.select_when_added = list
                .iter()
                .find(|(id, _)| *id == project)
                .map(|(_, n)| n.clone());
            let req = TransferRequest {
                peer: peer.peer.id,
                project,
                direction: Direction::Pull,
                send_everything: false,
            };
            self.view.last_request = Some(req.clone());
            self.act(Action::Prepare(req));
        }
    }
}
