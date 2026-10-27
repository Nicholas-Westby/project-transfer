//! The sheet that edits one of a project's commands, and what every command
//! sheet shares: the label and command boxes, the share box, and the note
//! on a shared command saying which projects an edit reaches.

use super::preview_view::small_ticks;
use super::theme::Palette;
use super::widgets;
use super::{App, Sheet, dialogs};
use crate::command_inputs::{self, PREFIX};
use crate::core::{Action, UiState};
use crate::model::{Command, CommandId, ProjectId, SharedCommandId};
use egui::{Label, Response, RichText, TextEdit, Ui, WidgetInfo};

#[derive(Clone, Debug, PartialEq)]
pub struct Draft {
    pub project: ProjectId,
    pub id: CommandId,
    pub label: String,
    pub line: String,
    /// Add it to the shared commands too; offered while it isn't linked.
    pub share: bool,
}

impl Draft {
    pub fn of(project: ProjectId, c: &Command) -> Draft {
        Draft {
            project,
            id: c.id,
            label: c.label.clone(),
            line: c.line.clone(),
            share: false,
        }
    }
}

impl App {
    pub(super) fn edit_command_sheet(&mut self, ui: &mut Ui, s: &UiState, mut d: Draft) {
        // Deleted meanwhile, here or by a transfer: nothing is left to edit.
        let Some(linked) = s
            .project(d.project)
            .and_then(|p| p.commands.iter().find(|c| c.id == d.id && !c.deleted))
            .map(|c| c.linked_to)
        else {
            self.view.sheet = Sheet::None;
            return;
        };
        let r = dialogs::sheet(ui, "edit_command", 520.0, true, |ui| {
            dialogs::sheet_title(ui, "Edit command");
            if let Some(id) = linked {
                linked_note(ui, s, id);
            }
            command_fields(ui, &mut d.label, &mut d.line);
            if linked.is_none() {
                share_box(ui, &mut d.share);
            }
            let confirm = match linked {
                Some(_) => "Save shared command",
                None => "Save command",
            };
            dialogs::button_row(ui, confirm, false, filled(&d.label, &d.line))
        });
        match (r.dismissed, r.inner) {
            (true, _) | (_, Some(false)) => self.view.sheet = Sheet::None,
            (_, Some(true)) => {
                let (label, line) = trimmed(&d.label, &d.line);
                self.act(Action::EditCommand(d.project, d.id, label, line));
                if d.share && linked.is_none() {
                    self.act(Action::ShareCommand(d.project, d.id));
                }
                self.view.sheet = Sheet::None;
            }
            _ => self.view.sheet = Sheet::EditCommand(d),
        }
    }
}

/// The label and command boxes, and where and how the command runs.
pub(super) fn command_fields(ui: &mut Ui, label: &mut String, line: &mut String) {
    widgets::muted(ui, "Label");
    let r = ui.add(
        TextEdit::singleline(label)
            .hint_text(widgets::hint("Fetch dependencies"))
            .desired_width(f32::INFINITY),
    );
    named(&r, "Label", label);
    ui.add_space(8.0);
    widgets::muted(ui, "Command");
    let r = ui.add(
        TextEdit::multiline(line)
            .font(egui::TextStyle::Monospace)
            .hint_text(widgets::hint("make deps").monospace())
            .desired_rows(3)
            .desired_width(f32::INFINITY),
    );
    named(&r, "Command", line);
    let shell = if cfg!(windows) {
        "Runs with PowerShell in the primary folder, only on the computer where you click it."
    } else {
        "Runs with your login shell in the primary folder, only on the computer where you \
         click it."
    };
    widgets::small_muted(ui, shell);
    inputs_help(ui, line);
}

/// How to make the command ask for a value when it runs, and what it asks
/// for as the text stands.
fn inputs_help(ui: &mut Ui, line: &str) {
    let muted = Palette::of(ui.ctx()).muted();
    let help = format!(
        "To ask for a value each time it runs, use a variable that starts with `{PREFIX}`, \
         such as `\"${PREFIX}RELEASE_NOTES\"`."
    );
    ui.add(Label::new(small_ticks(&help, muted)).wrap());
    let asked = command_inputs::inputs(line);
    if asked.is_empty() {
        return;
    }
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        widgets::small_muted(ui, "Asks when it runs");
        for i in &asked {
            widgets::badge(ui, &i.label);
        }
    });
}

/// Screen readers, and the tests, find a box by the caption over it.
pub(super) fn named(r: &Response, name: &str, text: &str) {
    r.widget_info(|| {
        let mut info = WidgetInfo::text_edit(true, text, text, "");
        info.label = Some(name.into());
        info
    });
}

/// The box that also adds the command to the shared commands, linked.
pub(super) fn share_box(ui: &mut Ui, share: &mut bool) {
    ui.add_space(8.0);
    grouped(ui, |ui| {
        ui.checkbox(share, "Share with other projects");
        choice_note(
            ui,
            "Other projects can then add it from Add command, linked or as a separate copy.",
        );
    });
}

/// Draws `add` with no gap between its lines, so they read as one item.
pub(super) fn grouped<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = 0.0;
        add(ui)
    })
    .inner
}

/// Small print under a checkbox or radio button, lined up with its text.
pub(super) fn choice_note(ui: &mut Ui, text: impl Into<String>) {
    let left = ui.spacing().icon_width + ui.spacing().icon_spacing;
    egui::Frame::new()
        .inner_margin(egui::Margin {
            left: left as i8,
            ..Default::default()
        })
        .show(ui, |ui| widgets::small_muted(ui, text));
}

/// What an edit of shared command `id` reaches: the projects linked to it
/// here, and paired computers later.
pub(super) fn linked_note(ui: &mut Ui, s: &UiState, id: SharedCommandId) {
    let ink = Palette::of(ui.ctx()).ink;
    let names = projects_using(s, id);
    let text = match names.len() {
        0 => "No project uses it yet.".to_string(),
        _ => format!("Saving changes it in {}.", and_list(&names)),
    };
    ui.horizontal_wrapped(|ui| {
        widgets::badge(ui, "Shared");
        ui.label(RichText::new(text).color(ink));
    });
    widgets::small_muted(
        ui,
        "Paired computers get the change with their next transfer.",
    );
    ui.add_space(8.0);
}

/// The projects with a command linked to `id`, in list order.
pub(super) fn projects_using(s: &UiState, id: SharedCommandId) -> Vec<String> {
    s.projects
        .iter()
        .filter(|p| {
            p.commands
                .iter()
                .any(|c| !c.deleted && c.linked_to == Some(id))
        })
        .map(|p| p.name.clone())
        .collect()
}

/// "a", "a and b", "a, b and c"; past three, how many.
pub(super) fn and_list(names: &[String]) -> String {
    match names {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] if names.len() <= 3 => format!("{} and {last}", rest.join(", ")),
        _ => format!("{} projects", names.len()),
    }
}

pub(super) fn filled(label: &str, line: &str) -> bool {
    !label.trim().is_empty() && !line.trim().is_empty()
}

pub(super) fn trimmed(label: &str, line: &str) -> (String, String) {
    (label.trim().to_string(), line.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_lists_read_as_a_sentence() {
        let names = |n: &[&str]| n.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(and_list(&names(&["garden"])), "garden");
        assert_eq!(and_list(&names(&["garden", "tides"])), "garden and tides");
        assert_eq!(
            and_list(&names(&["garden", "tides", "notes"])),
            "garden, tides and notes"
        );
        assert_eq!(and_list(&names(&["a", "b", "c", "d"])), "4 projects");
    }
}
