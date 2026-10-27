//! The sheet that adds a command to a project: a new one, which can be
//! shared too, or one of the shared commands, linked or as a separate copy.
//! Its list of shared commands is where they are edited and deleted too.

use super::command_editor::{
    and_list, choice_note, command_fields, filled, grouped, linked_note, projects_using, share_box,
    trimmed,
};
use super::commands_view::os_name;
use super::theme::Palette;
use super::widgets::{self, mono};
use super::{App, Confirm, Sheet, dialogs};
use crate::core::{Action, UiState};
use crate::model::{Os, Project, ProjectId, SharedCommand, SharedCommandId};
use egui::{Response, ScrollArea, Sides, Ui, WidgetInfo, WidgetType};

#[derive(Clone, Debug, PartialEq)]
pub struct Draft {
    pub project: ProjectId,
    pub label: String,
    pub line: String,
    pub share: bool,
    /// Showing the shared commands rather than a new command's boxes.
    pub from_shared: bool,
    pub picked: Option<SharedCommandId>,
    /// Add the picked one linked rather than as a separate copy.
    pub linked: bool,
    /// A shared command being edited in place of the list.
    pub editing: Option<SharedEdit>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SharedEdit {
    pub id: SharedCommandId,
    pub label: String,
    pub line: String,
}

impl Draft {
    pub fn new(project: ProjectId) -> Draft {
        Draft {
            project,
            label: String::new(),
            line: String::new(),
            share: false,
            from_shared: false,
            picked: None,
            linked: true,
            editing: None,
        }
    }
}

/// What the sheet was asked for this frame.
enum Asked {
    Add,
    Close,
    Edit(SharedCommandId),
    Delete(SharedCommandId),
    SaveEdit,
    EndEdit,
}

impl App {
    pub(super) fn add_command_sheet(&mut self, ui: &mut Ui, s: &UiState, mut d: Draft) {
        let Some(p) = s.project(d.project) else {
            self.view.sheet = Sheet::None;
            return;
        };
        let list = live_shared(s);
        // One deleted meanwhile, here or by a transfer, is no longer offered.
        d.picked = d.picked.filter(|id| list.iter().any(|c| c.id == *id));
        if let Some(e) = &d.editing
            && !list.iter().any(|c| c.id == e.id)
        {
            d.editing = None;
        }
        d.from_shared &= !list.is_empty();
        let r = dialogs::sheet(ui, "add_command", 560.0, true, |ui| {
            if let Some(e) = &mut d.editing {
                return edit_shared(ui, s, e);
            }
            dialogs::sheet_title(ui, "Add command");
            if !list.is_empty() {
                ui.horizontal(|ui| {
                    if ui.selectable_label(!d.from_shared, "New command").clicked() {
                        d.from_shared = false;
                    }
                    if ui
                        .selectable_label(d.from_shared, "Shared commands")
                        .clicked()
                    {
                        d.from_shared = true;
                    }
                });
                ui.add_space(10.0);
            }
            match d.from_shared {
                true => pick_shared(ui, s, p, &list, &mut d),
                false => new_command(ui, &mut d),
            }
        });
        let asked = if r.dismissed {
            Some(Asked::Close)
        } else {
            r.inner
        };
        match asked {
            Some(Asked::Close) => {
                self.view.sheet = Sheet::None;
                return;
            }
            Some(Asked::Add) => {
                if let Some(a) = adding(&d) {
                    self.act(a);
                }
                self.view.sheet = Sheet::None;
                return;
            }
            Some(Asked::Edit(id)) => {
                d.editing = list.iter().find(|c| c.id == id).map(|c| SharedEdit {
                    id,
                    label: c.label.clone(),
                    line: c.line.clone(),
                });
            }
            Some(Asked::Delete(id)) => self.view.confirm = Some(Confirm::DeleteSharedCommand(id)),
            Some(Asked::SaveEdit) => {
                if let Some(e) = d.editing.take() {
                    let (label, line) = trimmed(&e.label, &e.line);
                    self.act(Action::EditSharedCommand(e.id, label, line));
                }
            }
            Some(Asked::EndEdit) => d.editing = None,
            None => {}
        }
        self.view.sheet = Sheet::AddCommand(d);
    }
}

/// The action the Add button sends, if the sheet holds enough for one.
fn adding(d: &Draft) -> Option<Action> {
    if !d.from_shared {
        let (label, line) = trimmed(&d.label, &d.line);
        return Some(match d.share {
            true => Action::AddSharedCommand(d.project, label, line),
            false => Action::AddCommand(d.project, label, line),
        });
    }
    let id = d.picked?;
    Some(match d.linked {
        true => Action::LinkSharedCommand(d.project, id),
        false => Action::CopySharedCommand(d.project, id),
    })
}

fn new_command(ui: &mut Ui, d: &mut Draft) -> Option<Asked> {
    command_fields(ui, &mut d.label, &mut d.line);
    share_box(ui, &mut d.share);
    let ok = filled(&d.label, &d.line);
    dialogs::button_row(ui, "Add command", false, ok).map(added)
}

fn pick_shared(
    ui: &mut Ui,
    s: &UiState,
    p: &Project,
    list: &[&SharedCommand],
    d: &mut Draft,
) -> Option<Asked> {
    let mut asked = None;
    ScrollArea::vertical()
        .max_height(260.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            for c in list {
                if let Some(a) = shared_row(ui, s, p, c, d) {
                    asked = Some(a);
                }
                ui.add_space(8.0);
            }
        });
    let here = d.picked.is_some_and(|id| linked_in(p, id));
    ui.add_space(8.0);
    widgets::muted(ui, "Add it as");
    grouped(ui, |ui| {
        ui.add_enabled_ui(!here, |ui| ui.radio_value(&mut d.linked, true, "Linked"));
        choice_note(
            ui,
            match here {
                true => "Already linked in this project.",
                false => "Editing it changes the shared command in every project that uses it.",
            },
        );
    });
    ui.add_space(4.0);
    grouped(ui, |ui| {
        ui.radio_value(&mut d.linked, false, "Separate");
        choice_note(
            ui,
            "A copy of its text as it is now. Changes to it stay in this project.",
        );
    });
    let ok = d.picked.is_some() && !(d.linked && here);
    asked.or(dialogs::button_row(ui, "Add command", false, ok).map(added))
}

/// One shared command: picked with its radio, edited or deleted with its
/// buttons, and where it is used underneath.
fn shared_row(
    ui: &mut Ui,
    s: &UiState,
    p: &Project,
    c: &SharedCommand,
    d: &mut Draft,
) -> Option<Asked> {
    let pal = Palette::of(ui.ctx());
    let (picked, (delete, edit)) = grouped(ui, |ui| {
        let row = Sides::new().shrink_left().truncate().show(
            ui,
            |ui| {
                let r = ui.radio(d.picked == Some(c.id), &c.label);
                ui.label(mono(&c.line).color(pal.muted()));
                r.clicked()
            },
            |ui| {
                let delete = widgets::quiet(ui, "Delete", Some(pal.removed));
                let edit = widgets::quiet(ui, "Edit", None);
                (
                    named(delete, &format!("Delete shared command {}", c.label)).clicked(),
                    named(edit, &format!("Edit shared command {}", c.label)).clicked(),
                )
            },
        );
        choice_note(ui, about(s, c));
        row
    });
    if picked {
        d.picked = Some(c.id);
        // A second link would only show the same command twice.
        if linked_in(p, c.id) {
            d.linked = false;
        }
    }
    match (delete, edit) {
        (true, _) => Some(Asked::Delete(c.id)),
        (_, true) => Some(Asked::Edit(c.id)),
        _ => None,
    }
}

fn edit_shared(ui: &mut Ui, s: &UiState, e: &mut SharedEdit) -> Option<Asked> {
    dialogs::sheet_title(ui, "Edit shared command");
    linked_note(ui, s, e.id);
    command_fields(ui, &mut e.label, &mut e.line);
    let ok = filled(&e.label, &e.line);
    dialogs::button_row(ui, "Save shared command", false, ok).map(|save| match save {
        true => Asked::SaveEdit,
        false => Asked::EndEdit,
    })
}

fn added(add: bool) -> Asked {
    match add {
        true => Asked::Add,
        false => Asked::Close,
    }
}

/// The shown text says Edit or Delete; a screen reader hears which one.
fn named(r: Response, label: &str) -> Response {
    r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
    r
}

/// "Used in garden-planner · Made on Windows".
fn about(s: &UiState, c: &SharedCommand) -> String {
    let names = projects_using(s, c.id);
    let used = match names.is_empty() {
        true => "Not used in a project yet".to_string(),
        false => format!("Used in {}", and_list(&names)),
    };
    match c.created_on == Os::current() {
        true => used,
        false => format!("{used} · Made on {}", os_name(c.created_on)),
    }
}

fn linked_in(p: &Project, id: SharedCommandId) -> bool {
    p.commands
        .iter()
        .any(|c| !c.deleted && c.linked_to == Some(id))
}

/// The shared commands that aren't deleted, by label.
fn live_shared(s: &UiState) -> Vec<&SharedCommand> {
    let mut list: Vec<&SharedCommand> = s.shared_commands.iter().filter(|c| !c.deleted).collect();
    list.sort_by_key(|c| (c.label.to_lowercase(), c.id));
    list
}
