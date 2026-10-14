//! The Dock menu: what it offers, from the last transfer started here,
//! and starting one when it is picked.

use super::transfer_bar::blocked_kind;
use super::{App, Sheet};
use crate::core::{Action, TransferState, UiState};
use crate::dock::DockItem;
use crate::model::{InstanceId, ProjectId, TransferKind};
use egui::Ui;
use std::time::{Duration, Instant};

/// How long a pick waits for its computer to answer before it gives up and
/// leaves the reason to the buttons.
const WAIT: Duration = Duration::from_secs(10);

/// A pick from the Dock menu, started once its computer is selected and
/// answering.
#[derive(Clone, Debug, PartialEq)]
pub struct Pending {
    kind: TransferKind,
    project: ProjectId,
    peer: InstanceId,
    since: Instant,
    /// The last transfer's result was asked to close for this pick.
    dismissed: bool,
}

/// The menu's items, the last transfer started here first; none while
/// nothing is remembered or its project or computer is gone.
pub fn dock_items(s: &UiState) -> Vec<DockItem> {
    let Some(last) = s.me.last_started else {
        return Vec::new();
    };
    let (Some(project), Some(peer)) = (s.project(last.project), s.peer(last.peer)) else {
        return Vec::new();
    };
    let kinds: &[TransferKind] = match last.kind {
        TransferKind::Push => &[TransferKind::Push, TransferKind::Pull],
        TransferKind::Pull => &[TransferKind::Pull, TransferKind::Push],
        TransferKind::Sync => &[TransferKind::Sync, TransferKind::Pull, TransferKind::Push],
    };
    let (name, to) = (&project.name, &peer.peer.name);
    kinds
        .iter()
        .map(|&kind| DockItem {
            label: match kind {
                TransferKind::Push => format!("Push {name} to {to}"),
                TransferKind::Pull => format!("Pull {name} from {to}"),
                TransferKind::Sync => format!("Sync {name} with {to}"),
            },
            kind,
            project: last.project,
            peer: last.peer,
        })
        .collect()
}

/// What a waiting pick does this frame.
#[derive(Debug, PartialEq)]
enum Step {
    Wait,
    /// Close the last transfer's result first, as its Done button does.
    Dismiss,
    /// Start it, unless the usual reason stands in the way.
    Try,
    Drop,
}

/// `selected` is the project selected in the list.
fn step(p: &Pending, s: &UiState, selected: Option<ProjectId>, now: Instant) -> Step {
    // The user moved on to another project, or it is gone.
    if selected != Some(p.project) || s.project(p.project).is_none() {
        return Step::Drop;
    }
    let result = match &s.transfer {
        TransferState::Finished(_) | TransferState::Failed(_) => true,
        TransferState::Ready(r) => r.is_empty(),
        _ => false,
    };
    let mine = s.selected_peer == Some(p.peer);
    // With another computer selected, the result may be one the user
    // started by hand meanwhile; only the pick's own computer's result is
    // its to close.
    if result && mine && !p.dismissed {
        return Step::Dismiss;
    }
    let idle = matches!(s.transfer, TransferState::Idle);
    if mine && idle && s.peer(p.peer).is_some_and(|v| v.online) {
        return Step::Try;
    }
    if now.duration_since(p.since) < WAIT {
        return Step::Wait;
    }
    // Only ever with the computer the item names; then its reason shows.
    if mine { Step::Try } else { Step::Drop }
}

impl App {
    /// Keeps the Dock menu's items current, and starts what was picked in
    /// it the way its button would.
    pub(super) fn dock_menu(&mut self, ui: &Ui, s: &UiState) {
        self.dock.set_items(dock_items(s));
        if let Some(item) = self.dock.take_click()
            && !self.busy(s)
        {
            self.view.selected = Some(item.project);
            self.view.rename_project = None;
            if s.selected_peer != Some(item.peer) {
                self.act(Action::SelectPeer(item.peer));
            }
            self.view.dock_pending = Some(Pending {
                kind: item.kind,
                project: item.project,
                peer: item.peer,
                since: Instant::now(),
                dismissed: false,
            });
        }
        self.start_pending(ui, s);
    }

    /// A sheet, a question or a transfer in progress is the user's to
    /// finish; a pick leaves it, and everything under it, as it is.
    fn busy(&self, s: &UiState) -> bool {
        let working = match &s.transfer {
            TransferState::Preparing | TransferState::Running { .. } => true,
            TransferState::Ready(p) => !p.is_empty(),
            _ => false,
        };
        working || self.view.sheet != Sheet::None || self.view.confirm.is_some()
    }

    /// What a pick waiting for its computer says under the buttons.
    pub(super) fn dock_waiting(&self, s: &UiState) -> Option<String> {
        let p = self.view.dock_pending.as_ref()?;
        let peer = &s.peer(p.peer)?.peer.name;
        let what = match p.kind {
            TransferKind::Push => "push",
            TransferKind::Pull => "pull",
            TransferKind::Sync => "sync",
        };
        Some(format!("Starting the {what} once {peer} answers…"))
    }

    fn start_pending(&mut self, ui: &Ui, s: &UiState) {
        let Some(p) = self.view.dock_pending.clone() else {
            return;
        };
        match step(&p, s, self.view.selected, Instant::now()) {
            // A computer just selected answers its first status request in
            // a moment.
            Step::Wait => ui.ctx().request_repaint_after(Duration::from_millis(250)),
            Step::Drop => self.view.dock_pending = None,
            Step::Dismiss => {
                self.act(Action::DismissTransfer);
                if let Some(p) = &mut self.view.dock_pending {
                    p.dismissed = true;
                }
                ui.ctx().request_repaint_after(Duration::from_millis(250));
            }
            Step::Try => {
                self.view.dock_pending = None;
                // Whatever stands in the way shows under the buttons, as
                // after a click on one.
                if let Some(project) = s.project(p.project)
                    && blocked_kind(s, project, p.kind).is_none()
                {
                    self.start_transfer(s, p.project, p.kind);
                }
            }
        }
    }
}

#[cfg(test)]
#[path = "dock_menu_tests.rs"]
mod tests;
