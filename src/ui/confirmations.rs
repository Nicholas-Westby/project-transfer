//! Confirmations for every risky action except a transfer, whose preview
//! is its own confirmation. Each is worded from the current state and sent
//! only on confirm.

use super::commands_view::os_name;
use super::theme::{CONTROL_RADIUS, Palette};
use super::widgets::mono;
use super::{App, dialogs};
use crate::core::{Action, UiState};
use crate::model::{CommandId, FolderId, InstanceId, Os, Permissions, ProjectId};
use egui::{Frame, Margin, RichText, Ui};

#[derive(Clone, Debug, PartialEq)]
pub enum Confirm {
    DeleteProject(ProjectId),
    RemoveFolder(ProjectId, FolderId),
    Unpair(InstanceId),
    DeleteCommand(ProjectId, CommandId),
    RunCommand(ProjectId, CommandId),
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
        // The hash of the command text this frame shows, so running it means
        // running exactly what was read.
        let mut shown_hash = None;
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
                let Some(label) = command(s, *pid, *cid).map(|c| c.label.clone()) else {
                    self.view.confirm = None;
                    return;
                };
                dialogs::confirm(
                    ui,
                    "confirm",
                    &format!("Delete {label}?"),
                    "The deletion reaches other computers on the next transfer.",
                    "Delete command",
                    true,
                )
            }
            Confirm::RunCommand(pid, cid) => {
                let Some(cmd) = command(s, *pid, *cid).cloned() else {
                    self.view.confirm = None;
                    return;
                };
                shown_hash = Some(crate::commands::command_hash(&cmd));
                let pal = Palette::of(ui.ctx());
                dialogs::confirm_with(
                    ui,
                    "confirm",
                    &format!("Run {}?", cmd.label),
                    "This command is new on this computer or changed since it last ran here. \
                     Check that you trust it before it runs:",
                    &format!("Run {}", cmd.label),
                    false,
                    |ui| {
                        ui.add_space(6.0);
                        Frame::new()
                            .fill(pal.ground)
                            .corner_radius(CONTROL_RADIUS)
                            .inner_margin(Margin::same(12))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.add(egui::Label::new(mono(&cmd.line).color(pal.ink)).wrap());
                            });
                        if cmd.created_on != Os::current() {
                            ui.add_space(6.0);
                            ui.label(RichText::new(format!(
                                "This command was created on {}. Commands from {} rarely work on {}.",
                                os_name(cmd.created_on),
                                os_name(cmd.created_on),
                                os_name(Os::current())
                            ))
                            .color(pal.changed));
                        }
                    },
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
                self.confirmed(c, shown_hash);
            }
        }
    }

    fn confirmed(&mut self, c: Confirm, shown_hash: Option<String>) {
        match c {
            Confirm::DeleteProject(id) => self.act(Action::DeleteProject(id)),
            Confirm::RemoveFolder(p, f) => self.act(Action::RemoveFolder(p, f)),
            Confirm::Unpair(id) => self.act(Action::Unpair(id)),
            Confirm::DeleteCommand(p, c) => self.act(Action::DeleteCommand(p, c)),
            Confirm::RunCommand(p, c) => self.run_now(p, c, shown_hash),
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
