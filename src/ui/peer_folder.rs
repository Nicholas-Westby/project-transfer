//! Changing where the other computer keeps one of this project's folders.
//! It only records the place there; the next push copies the files.

use super::{App, Sheet, dialogs, widgets};
use crate::core::{Action, UiState};
use crate::model::{Folder, FolderId, Project, ProjectId};
use egui::{TextEdit, Ui, WidgetInfo};

/// The dialog's draft, kept in `Sheet::PeerFolder`.
#[derive(Clone, Debug, PartialEq)]
pub struct PeerFolderDraft {
    pub project: ProjectId,
    pub folder: FolderId,
    pub path: String,
}

impl App {
    pub(super) fn peer_folder_sheet(&mut self, ui: &mut Ui, s: &UiState, mut d: PeerFolderDraft) {
        let peer = super::peer_name(s).unwrap_or_else(|| "the other computer".into());
        let Some(name) = s
            .project(d.project)
            .and_then(|p| p.folders.iter().find(|f| f.id == d.folder))
            .map(|f| f.name.clone())
        else {
            self.view.sheet = Sheet::None;
            return;
        };
        let r = dialogs::sheet(ui, "peer_folder", 520.0, true, |ui| {
            dialogs::sheet_title(ui, &format!("Change where {name} lives on {peer}"));
            let label = format!("Folder on {peer}");
            widgets::muted(ui, &label);
            let field = ui.add(
                TextEdit::singleline(&mut d.path)
                    .font(egui::TextStyle::Monospace)
                    .desired_width(f32::INFINITY),
            );
            field.widget_info(|| {
                let mut info = WidgetInfo::text_edit(true, &d.path, &d.path, "");
                info.label = Some(label.clone());
                info
            });
            widgets::small_muted(
                ui,
                format!(
                    "A full path on {peer}, or one starting with ~ for its home folder. Nothing \
                     moves now: the next push copies the files there."
                ),
            );
            let ok = !d.path.trim().is_empty();
            dialogs::button_row(ui, &format!("Save on {peer}"), false, ok)
        });
        match (r.dismissed, r.inner) {
            (true, _) | (_, Some(false)) => self.view.sheet = Sheet::None,
            (_, Some(true)) => {
                let path = d.path.trim().to_string();
                self.act(Action::SetPeerFolderPath(d.project, d.folder, path));
                self.view.sheet = Sheet::None;
            }
            _ => self.view.sheet = Sheet::PeerFolder(d),
        }
    }

    /// Offered once the peer is online and has the project; the peer
    /// itself checks the path.
    pub(super) fn peer_folder_entry(&mut self, ui: &mut Ui, s: &UiState, p: &Project, f: &Folder) {
        let Some(peer) = s.selected_peer.and_then(|id| s.peer(id)) else {
            return;
        };
        let Some(remote) = s.remote_projects.get(&p.id).filter(|_| peer.online) else {
            return;
        };
        let name = &peer.peer.name;
        let there = remote.folders.iter().find(|rf| rf.id == f.id);
        let why = match there {
            None => Some(format!(
                "{name} doesn't have this folder yet. Sync details first."
            )),
            Some(_) if !peer.peer.granted.may_push_to_me => Some(format!(
                "{name} doesn't allow pushes from this computer, which changing its folders needs."
            )),
            Some(_) => None,
        };
        let r = ui
            .add_enabled(
                why.is_none(),
                egui::Button::new(format!("Change folder on {name}…")),
            )
            .on_disabled_hover_text(why.unwrap_or_default());
        if r.clicked()
            && let Some(rf) = there
        {
            let path = rf
                .shown
                .clone()
                .or_else(|| rf.path.clone())
                .unwrap_or_default();
            self.view.sheet = Sheet::PeerFolder(PeerFolderDraft {
                project: p.id,
                folder: f.id,
                path,
            });
        }
    }
}
