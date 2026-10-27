//! The sheet a command opens before it runs, when it has to: to show text
//! that is new or changed on this computer, and to ask for the values of
//! its inputs. It always shows the text as it is now and says so when that
//! changed while it was open; the run carries the hash of what it showed,
//! so text that changes after the click is refused rather than run unseen.

use super::command_editor::named;
use super::commands_view::os_name;
use super::theme::{CONTROL_RADIUS, Palette};
use super::widgets::{self, mono};
use super::{App, Sheet, dialogs};
use crate::command_inputs::{self, Input};
use crate::commands::{command_hash, must_confirm};
use crate::core::UiState;
use crate::model::{Command, CommandId, Os, ProjectId};
use egui::{Frame, Label, Margin, RichText, TextEdit, Ui};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub struct Draft {
    pub project: ProjectId,
    pub id: CommandId,
    /// The hash of the text when the sheet opened.
    pub opened: String,
    /// What has been typed, by variable.
    pub values: BTreeMap<String, String>,
    /// The first box has had the keyboard once, so typing can start at once.
    pub focused: bool,
}

impl Draft {
    pub fn of(project: ProjectId, c: &Command) -> Draft {
        Draft {
            project,
            id: c.id,
            opened: command_hash(c),
            values: BTreeMap::new(),
            focused: false,
        }
    }
}

/// Whether running `c` goes through the sheet: its text needs a check, or
/// it asks for values.
pub fn needed(c: &Command) -> bool {
    must_confirm(c) || !command_inputs::inputs(&c.line).is_empty()
}

impl App {
    pub(super) fn run_command_sheet(&mut self, ui: &mut Ui, s: &UiState, mut d: Draft) {
        // Deleted meanwhile, here or by a transfer: nothing is left to run.
        let Some(c) = s
            .project(d.project)
            .and_then(|p| p.commands.iter().find(|c| c.id == d.id && !c.deleted))
            .cloned()
        else {
            self.view.sheet = Sheet::None;
            return;
        };
        // Drawn from the text as it is now, which is the text that runs.
        let asked = command_inputs::inputs(&c.line);
        let check = must_confirm(&c);
        // Typed values aren't lost to a stray click outside or Escape.
        let blank = d.values.values().all(|v| v.trim().is_empty());
        let r = dialogs::sheet(ui, "run_command", 520.0, blank, |ui| {
            let title = match check {
                true => format!("Run {}?", c.label),
                false => format!("Run {}", c.label),
            };
            dialogs::sheet_title(ui, &title);
            if command_hash(&c) != d.opened {
                let changed = Palette::of(ui.ctx()).changed;
                ui.label(
                    RichText::new(
                        "The command changed while this was open, so check it again before \
                         you run it.",
                    )
                    .color(changed),
                );
                ui.add_space(6.0);
            }
            match check {
                true => text_to_check(ui, &c),
                false => {
                    let muted = Palette::of(ui.ctx()).muted();
                    ui.add(Label::new(mono(&c.line).color(muted)).wrap());
                }
            }
            for (n, input) in asked.iter().enumerate() {
                let value = d.values.entry(input.var.clone()).or_default();
                let first = n == 0 && !d.focused;
                value_box(ui, input, value, first);
            }
            d.focused = true;
            dialogs::button_row(ui, &format!("Run {}", c.label), false, true)
        });
        match (r.dismissed, r.inner) {
            (true, _) | (_, Some(false)) => self.view.sheet = Sheet::None,
            (_, Some(true)) => {
                let values = asked
                    .iter()
                    .map(|i| {
                        let v = d.values.get(&i.var).map_or("", |v| v.trim_end());
                        (i.var.clone(), v.to_string())
                    })
                    .collect();
                self.view.sheet = Sheet::None;
                self.run_now(d.project, d.id, Some(command_hash(&c)), values);
            }
            _ => self.view.sheet = Sheet::RunCommand(d),
        }
    }
}

/// Why the text is shown, the text in full, and a warning when it was
/// written for another system.
fn text_to_check(ui: &mut Ui, c: &Command) {
    let pal = Palette::of(ui.ctx());
    ui.label(
        RichText::new(
            "This command is new on this computer or changed since it last ran here. Check \
             that you trust it before it runs:",
        )
        .color(pal.ink),
    );
    ui.add_space(6.0);
    Frame::new()
        .fill(pal.ground)
        .corner_radius(CONTROL_RADIUS)
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.add(Label::new(mono(&c.line).color(pal.ink)).wrap());
        });
    if c.created_on != Os::current() {
        ui.add_space(6.0);
        let made = os_name(c.created_on);
        ui.label(
            RichText::new(format!(
                "This command was created on {made}. Commands from {made} rarely work on {}.",
                os_name(Os::current())
            ))
            .color(pal.changed),
        );
    }
}

/// One input's box, a few lines high, for text such as release notes.
fn value_box(ui: &mut Ui, input: &Input, value: &mut String, focus: bool) {
    ui.add_space(8.0);
    widgets::muted(ui, &input.label);
    let r = ui.add(
        TextEdit::multiline(value)
            // Named by its variable, not its place, so a notice appearing
            // above it doesn't take away the keyboard mid-sentence.
            .id_salt(("run input", &input.var))
            .desired_rows(2)
            .desired_width(f32::INFINITY),
    );
    named(&r, &input.label, value);
    if focus {
        r.request_focus();
    }
}
