//! Pull and Push, each with the one-line reason when it cannot run.

use super::theme::Palette;
use super::widgets;
use super::{App, Confirm};
use crate::core::{Action, TransferState, UiState};
use crate::model::{Direction, Project};
use crate::transfer::TransferRequest;
use egui::{RichText, Ui};

/// Why a push or pull can't start right now, or None when it can.
pub fn blocked(s: &UiState, p: &Project, dir: Direction) -> Option<String> {
    let Some(peer) = s.selected_peer.and_then(|id| s.peer(id)) else {
        return Some(if s.peers.is_empty() {
            "Pair a computer first. Open the menu in the bar above.".into()
        } else {
            "Choose a computer in the bar above.".into()
        });
    };
    let name = &peer.peer.name;
    if !peer.online {
        return Some(format!(
            "{name} is offline. Open Project Transfer there to continue."
        ));
    }
    if !matches!(s.transfer, TransferState::Idle) {
        return Some("Another transfer is open. Finish it first.".into());
    }
    match dir {
        Direction::Push if !peer.peer.granted.may_push_to_me => Some(format!(
            "{name} doesn't allow pushes from this computer. Allow it on {name}."
        )),
        Direction::Pull if !peer.peer.granted.may_pull_from_me => Some(format!(
            "{name} doesn't allow pulls to this computer. Allow it on {name}."
        )),
        Direction::Push if p.folders.iter().all(|f| f.local_path.is_none()) => {
            Some("None of this project's folders are on this computer yet.".into())
        }
        _ => None,
    }
}

impl App {
    pub(super) fn transfer_bar(&mut self, ui: &mut Ui, s: &UiState, p: &Project) {
        let pal = Palette::of(ui.ctx());
        let peer = super::peer_name(s).unwrap_or_else(|| "the other computer".into());
        let mut start = None;
        let reasons = [
            blocked(s, p, Direction::Push),
            blocked(s, p, Direction::Pull),
        ];
        // One shared reason reads once, under both buttons.
        let shared = reasons[0].is_some() && reasons[0] == reasons[1];
        ui.columns(2, |cols| {
            for (ui, dir) in cols.iter_mut().zip([Direction::Pull, Direction::Push]) {
                let reason = reasons[dir as usize].clone();
                let label = match dir {
                    Direction::Push => format!("Push to {peer}"),
                    Direction::Pull => format!("Pull from {peer}"),
                };
                ui.vertical(|ui| {
                    let r = ui
                        .add_enabled_ui(reason.is_none(), |ui| {
                            widgets::with_fill(ui, pal.route, |ui| {
                                ui.add_sized(
                                    [ui.available_width(), 36.0],
                                    egui::Button::new(
                                        RichText::new(&label).color(pal.on_fill()).strong(),
                                    ),
                                )
                            })
                        })
                        .inner;
                    if r.clicked() {
                        start = Some(dir);
                    }
                    if let Some(why) = reason.as_ref().filter(|_| !shared) {
                        ui.add(
                            egui::Label::new(RichText::new(why).small().color(pal.muted())).wrap(),
                        );
                    }
                });
            }
        });
        if shared && let Some(why) = &reasons[0] {
            ui.label(RichText::new(why).small().color(pal.muted()));
        }
        if let Some(dir) = start
            && let Some(peer) = s.selected_peer
        {
            let req = TransferRequest {
                peer,
                project: p.id,
                direction: dir,
                send_everything: self.view.send_everything,
            };
            self.view.last_request = Some(req.clone());
            self.act(Action::Prepare(req));
            self.view.send_everything = false;
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            let mut ticked = self.view.send_everything;
            if ui
                .checkbox(&mut ticked, "Send everything")
                .on_hover_text("Skip the ignore list for the next transfer only.")
                .changed()
            {
                if ticked {
                    self.view.confirm = Some(Confirm::SendEverything);
                } else {
                    self.view.send_everything = false;
                }
            }
            widgets::small_muted(
                ui,
                "Includes ignored folders such as node_modules, next transfer only.",
            );
        });
    }
}
