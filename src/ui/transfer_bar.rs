//! Pull, Sync and Push, each with the one-line reason when it cannot run.

use super::theme::Palette;
use super::widgets;
use super::{App, Confirm};
use crate::core::{Action, TransferState, UiState};
use crate::model::{Direction, Project, ProjectId, TransferKind};
use crate::transfer::TransferRequest;
use egui::{RichText, TextWrapMode, Ui};

const SYNC_HELP: &str =
    "Sends this project's folders, description and commands. No files are copied.";
const SYNC_TRANSFER_HELP: &str = "Pushes or pulls, whichever way has more files with newer \
     changes. The preview says which before anything moves.";

/// Why a push or pull can't start right now, or None when it can.
pub fn blocked(s: &UiState, p: &Project, dir: Direction) -> Option<String> {
    peer_blocked(s, dir, "").or_else(|| {
        let none_here = p.folders.iter().all(|f| f.local_path.is_none());
        (dir == Direction::Push && none_here)
            .then(|| "None of this project's folders are on this computer yet.".into())
    })
}

/// Why Sync can't start right now. It may go either way, so it needs what
/// a push and a pull both need from the peer; a project with no folder
/// here can still sync, by pulling.
pub fn sync_transfer_blocked(s: &UiState) -> Option<String> {
    let needs = ", which syncing needs";
    let push = peer_blocked(s, Direction::Push, needs);
    let pull = peer_blocked(s, Direction::Pull, needs);
    // Every other reason reads the same both ways, so two different ones
    // are the two permissions: say both at once.
    if let (Some(a), Some(b), Some(name)) = (&push, &pull, super::peer_name(s))
        && a != b
    {
        return Some(format!(
            "{name} doesn't allow pushes from or pulls to this computer, which syncing needs. \
             Allow both on {name}."
        ));
    }
    push.or(pull)
}

/// Why the button for `kind` can't start right now, or None when it can.
pub fn blocked_kind(s: &UiState, p: &Project, kind: TransferKind) -> Option<String> {
    match kind {
        TransferKind::Push => blocked(s, p, Direction::Push),
        TransferKind::Pull => blocked(s, p, Direction::Pull),
        TransferKind::Sync => sync_transfer_blocked(s),
    }
}

/// Why "Sync details" can't run right now. It needs what a push needs from
/// the peer, but a project with no folder here still has details to send.
pub fn sync_blocked(s: &UiState) -> Option<String> {
    peer_blocked(s, Direction::Push, ", which syncing details needs")
}

/// What the selected peer, its permissions or an open transfer stand in
/// the way of; `needs` goes after a missing permission.
fn peer_blocked(s: &UiState, dir: Direction, needs: &str) -> Option<String> {
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
            "{name} doesn't allow pushes from this computer{needs}. Allow it on {name}."
        )),
        Direction::Pull if !peer.peer.granted.may_pull_from_me => Some(format!(
            "{name} doesn't allow pulls to this computer{needs}. Allow it on {name}."
        )),
        _ => None,
    }
}

impl App {
    pub(super) fn transfer_bar(&mut self, ui: &mut Ui, s: &UiState, p: &Project) {
        let pal = Palette::of(ui.ctx());
        let peer = super::peer_name(s).unwrap_or_else(|| "the other computer".into());
        let mut start = None;
        let kinds = [TransferKind::Pull, TransferKind::Sync, TransferKind::Push];
        let reasons = kinds.map(|k| blocked_kind(s, p, k));
        // One shared reason reads once, under all three buttons.
        let shared = reasons[0].is_some() && reasons.iter().all(|r| *r == reasons[0]);
        ui.columns(3, |cols| {
            for ((ui, kind), reason) in cols.iter_mut().zip(kinds).zip(&reasons) {
                let label = match kind {
                    TransferKind::Pull => format!("Pull from {peer}"),
                    TransferKind::Sync => format!("Sync with {peer}"),
                    TransferKind::Push => format!("Push to {peer}"),
                };
                ui.vertical(|ui| {
                    let r = ui
                        .add_enabled_ui(reason.is_none(), |ui| {
                            widgets::with_fill(ui, pal.route, |ui| {
                                let text = RichText::new(&label).color(pal.on_fill()).strong();
                                // Three to a row: a long name gives way.
                                let button =
                                    egui::Button::new(text).wrap_mode(TextWrapMode::Truncate);
                                ui.add_sized([ui.available_width(), 36.0], button)
                            })
                        })
                        .inner;
                    let r = match kind {
                        TransferKind::Sync => r.on_hover_text(SYNC_TRANSFER_HELP),
                        _ => r,
                    };
                    if r.clicked() {
                        start = Some(kind);
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
        if let Some(waiting) = self.dock_waiting(s) {
            widgets::small_muted(ui, waiting);
        }
        if let Some(kind) = start {
            self.start_transfer(s, p.id, kind);
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
        ui.add_space(8.0);
        self.sync_row(ui, s, p, &peer);
    }

    /// Starts what the button for `kind` starts: the preview of a push or
    /// pull, or the look at both sides a sync takes first. The Dock menu
    /// starts transfers through here too.
    pub(super) fn start_transfer(&mut self, s: &UiState, project: ProjectId, kind: TransferKind) {
        let Some(peer) = s.selected_peer else {
            return;
        };
        let direction = match kind {
            TransferKind::Pull => Direction::Pull,
            // A sync's own direction is decided by the comparison.
            TransferKind::Push | TransferKind::Sync => Direction::Push,
        };
        let req = TransferRequest {
            peer,
            project,
            direction,
            send_everything: self.view.send_everything,
        };
        self.view.last_request = Some(req.clone());
        self.act(match kind {
            TransferKind::Sync => Action::PrepareSync(req),
            TransferKind::Push | TransferKind::Pull => Action::Prepare(req),
        });
        self.view.send_everything = false;
    }

    /// A plain button below the filled ones: sending details is a smaller
    /// step than a transfer.
    fn sync_row(&mut self, ui: &mut Ui, s: &UiState, p: &Project, peer: &str) {
        let pal = Palette::of(ui.ctx());
        let reason = sync_blocked(s);
        ui.horizontal(|ui| {
            let label = format!("Sync details with {peer}");
            let r = ui
                .add_enabled(reason.is_none(), egui::Button::new(label))
                .on_hover_text(SYNC_HELP);
            if r.clicked() {
                self.act(Action::SyncDetails(p.id));
            }
            // A reason Pull and Push already give reads once, above.
            let note = reason
                .as_deref()
                .filter(|r| blocked(s, p, Direction::Push).as_deref() != Some(*r))
                .unwrap_or(SYNC_HELP);
            ui.add(egui::Label::new(RichText::new(note).small().color(pal.muted())).wrap());
        });
    }
}
