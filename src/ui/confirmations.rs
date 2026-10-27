//! Confirmations for every risky action except a transfer, whose preview
//! is its own confirmation, and a command's run, which the run sheet
//! confirms. Each is worded from the current state and sent only on confirm.

use super::{App, dialogs};
use crate::core::{Action, UiState};
use crate::model::{CommandId, FolderId, InstanceId, Permissions, ProjectId, SharedCommandId};
use egui::Ui;

#[derive(Clone, Debug, PartialEq)]
pub enum Confirm {
    DeleteProject(ProjectId),
    RemoveFolder(ProjectId, FolderId),
    Unpair(InstanceId),
    DeleteCommand(ProjectId, CommandId),
    DeleteSharedCommand(SharedCommandId),
    /// The peer and what it may do once confirmed.
    Revoke(InstanceId, Permissions),
    SendEverything,
}

/// What the peer loses, in words, between the current and the new allows.
pub fn revoked_text(name: &str, old: Permissions, new: Permissions) -> String {
    let push = old.may_push_to_me && !new.may_push_to_me;
    let pull = old.may_pull_from_me && !new.may_pull_from_me;
    match (push, pull) {
        (true, true) => {
            format!("{name} will no longer be able to push to this computer or pull from it.")
        }
        (true, false) => format!(
            "{name} will no longer be able to push to this computer and overwrite its files."
        ),
        _ => format!("{name} will no longer be able to pull files from this computer."),
    }
}

impl App {
    pub(super) fn confirmations(&mut self, ui: &mut Ui, s: &UiState) {
        let Some(c) = self.view.confirm.clone() else {
            return;
        };
        let answer = match &c {
            Confirm::DeleteProject(id) => {
                let Some(p) = s.project(*id) else {
                    self.view.confirm = None;
                    return;
                };
                dialogs::confirm(
                    ui,
                    "confirm",
                    &format!("Delete {}?", p.name),
                    "Project Transfer forgets this project on this computer. Its files stay on \
                     disk, and other computers keep their copy.",
                    "Delete project",
                    true,
                )
            }
            Confirm::RemoveFolder(pid, fid) => {
                let Some(f) = s
                    .project(*pid)
                    .and_then(|p| p.folders.iter().find(|f| f.id == *fid))
                else {
                    self.view.confirm = None;
                    return;
                };
                dialogs::confirm(
                    ui,
                    "confirm",
                    &format!("Remove {} from this project?", f.name),
                    "Its files stay on disk. Only the setup goes: the folder stops moving with \
                     this project.",
                    "Remove folder",
                    true,
                )
            }
            Confirm::Unpair(id) => {
                let Some(name) = s.peer(*id).map(|p| p.peer.name.clone()) else {
                    self.view.confirm = None;
                    return;
                };
                dialogs::confirm(
                    ui,
                    "confirm",
                    &format!("Unpair {name}?"),
                    &format!(
                        "Neither computer can push or pull until you pair them again, which \
                         needs both screens and a new code. Files on {name} are not touched."
                    ),
                    "Unpair",
                    true,
                )
            }
            Confirm::DeleteCommand(pid, cid) => {
                let Some(c) = command(s, *pid, *cid) else {
                    self.view.confirm = None;
                    return;
                };
                let stays = match c.linked_to {
                    Some(_) => " The shared command stays, so you can add it again.",
                    None => "",
                };
                dialogs::confirm(
                    ui,
                    "confirm",
                    &format!("Delete {}?", c.label),
                    &format!("The deletion reaches other computers on the next transfer.{stays}"),
                    "Delete command",
                    true,
                )
            }
            Confirm::DeleteSharedCommand(id) => {
                let Some(c) = s.shared_commands.iter().find(|c| c.id == *id && !c.deleted) else {
                    self.view.confirm = None;
                    return;
                };
                let linked = s
                    .projects
                    .iter()
                    .flat_map(|p| &p.commands)
                    .filter(|c| !c.deleted && c.linked_to == Some(*id))
                    .count();
                let kept = match linked {
                    0 => String::new(),
                    1 => "The command linked to it keeps its text and becomes a separate \
                          command. "
                        .into(),
                    n => format!(
                        "The {n} commands linked to it keep their text and become separate \
                         commands. "
                    ),
                };
                dialogs::confirm(
                    ui,
                    "confirm",
                    &format!("Delete shared command {}?", c.label),
                    &format!(
                        "{kept}Paired computers drop it from their shared commands after the \
                         next transfer."
                    ),
                    "Delete shared command",
                    true,
                )
            }
            Confirm::Revoke(id, new) => {
                let Some(p) = s.peer(*id) else {
                    self.view.confirm = None;
                    return;
                };
                dialogs::confirm(
                    ui,
                    "confirm",
                    "Take back this permission?",
                    &revoked_text(&p.peer.name, p.peer.allows, *new),
                    "Take back permission",
                    true,
                )
            }
            Confirm::SendEverything => dialogs::confirm(
                ui,
                "confirm",
                "Send everything next time?",
                "The next push or pull skips the ignore list, so ignored folders such as \
                 node_modules, target and bin copy too. This can be large and slow.",
                "Send everything",
                false,
            ),
        };
        match answer {
            None => {}
            Some(false) => self.view.confirm = None,
            Some(true) => {
                self.view.confirm = None;
                self.confirmed(c);
            }
        }
    }

    fn confirmed(&mut self, c: Confirm) {
        match c {
            Confirm::DeleteProject(id) => self.act(Action::DeleteProject(id)),
            Confirm::RemoveFolder(p, f) => self.act(Action::RemoveFolder(p, f)),
            Confirm::Unpair(id) => self.act(Action::Unpair(id)),
            Confirm::DeleteCommand(p, c) => self.act(Action::DeleteCommand(p, c)),
            Confirm::DeleteSharedCommand(id) => self.act(Action::DeleteSharedCommand(id)),
            Confirm::Revoke(id, allows) => self.act(Action::SetAllows(id, allows)),
            Confirm::SendEverything => self.view.send_everything = true,
        }
    }
}

fn command(s: &UiState, p: ProjectId, c: CommandId) -> Option<&crate::model::Command> {
    s.project(p)?.commands.iter().find(|x| x.id == c)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: Permissions = Permissions {
        may_push_to_me: true,
        may_pull_from_me: true,
    };

    #[test]
    fn revoking_names_what_is_lost() {
        let no_push = Permissions {
            may_push_to_me: false,
            ..ALL
        };
        assert!(revoked_text("Desktop", ALL, no_push).contains("push to this computer"));
        let none = Permissions::default();
        assert!(revoked_text("Desktop", ALL, none).contains("push to this computer or pull"));
    }
}
