//! A project's commands: run, stop, read the output, add, edit, delete.

use super::theme::{CONTROL_RADIUS, Palette};
use super::widgets::{self, mono};
use super::{App, Confirm, Sheet, dialogs};
use crate::commands::must_confirm;
use crate::core::{Action, CommandRun, RunStatus, UiState};
use crate::model::{Command, CommandId, Os, Project, ProjectId};
use egui::{Align, Frame, Layout, Margin, RichText, ScrollArea, TextEdit, Ui};

impl App {
    pub(super) fn commands_section(&mut self, ui: &mut Ui, s: &UiState, p: &Project) {
        ui.horizontal(|ui| {
            widgets::section(ui, "Commands");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Add command").clicked() {
                    self.view.sheet = Sheet::CommandEditor {
                        project: p.id,
                        id: None,
                        label: String::new(),
                        line: String::new(),
                    };
                }
            });
        });
        let live: Vec<&Command> = p.commands.iter().filter(|c| !c.deleted).collect();
        if live.is_empty() {
            widgets::muted(
                ui,
                "Save a command you run often, such as installing packages. It runs in the \
                 primary folder.",
            );
            return;
        }
        ui.add_space(4.0);
        for c in live {
            self.command_row(ui, s, p, c);
            ui.add_space(6.0);
        }
    }

    fn command_row(&mut self, ui: &mut Ui, s: &UiState, p: &Project, c: &Command) {
        let pal = Palette::of(ui.ctx());
        let run = s.command_runs.get(&c.id);
        let running = run.is_some_and(|r| r.status == RunStatus::Running);
        ui.horizontal(|ui| {
            let r = ui.add_enabled(!running, egui::Button::new(format!("▶ {}", c.label)));
            if r.clicked() {
                self.request_run(p.id, c);
            }
            ui.label(mono(&c.line).color(pal.muted()));
            if c.created_on != Os::current() {
                widgets::small_muted(ui, format!("Made on {}", os_name(c.created_on)));
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if widgets::quiet(ui, "Delete", Some(pal.removed)).clicked() {
                    self.view.confirm = Some(Confirm::DeleteCommand(p.id, c.id));
                }
                if widgets::quiet(ui, "Edit", None).clicked() {
                    self.view.sheet = Sheet::CommandEditor {
                        project: p.id,
                        id: Some(c.id),
                        label: c.label.clone(),
                        line: c.line.clone(),
                    };
                }
                if running && widgets::quiet(ui, "Stop", Some(pal.removed)).clicked() {
                    self.act(Action::StopCommand(c.id));
                }
            });
        });
        if let Some(run) = run
            && !self.view.hidden_output.contains(&c.id)
        {
            self.output_panel(ui, c.id, run);
        }
    }

    /// Runs at once, or asks first when the command is new here or changed.
    fn request_run(&mut self, project: ProjectId, c: &Command) {
        if must_confirm(c) {
            self.view.confirm = Some(Confirm::RunCommand(project, c.id));
        } else {
            self.run_now(project, c.id, None);
        }
    }

    /// `confirmed` is the hash of the text the user confirmed, if they had to.
    pub(super) fn run_now(&mut self, project: ProjectId, id: CommandId, confirmed: Option<String>) {
        self.view.hidden_output.remove(&id);
        self.act(Action::RunCommand(project, id, confirmed));
    }

    fn output_panel(&mut self, ui: &mut Ui, id: CommandId, run: &CommandRun) {
        let pal = Palette::of(ui.ctx());
        Frame::new()
            .fill(pal.ground)
            .corner_radius(CONTROL_RADIUS)
            .inner_margin(Margin::symmetric(12, 10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ScrollArea::vertical()
                    .id_salt(("output", id))
                    .max_height(120.0)
                    .stick_to_bottom(true)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        for line in &run.lines {
                            let color = if line.stderr { pal.changed } else { pal.ink };
                            ui.label(mono(&line.text).color(color));
                        }
                    });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    let (text, color) = match &run.status {
                        RunStatus::Running => ("Running…".to_string(), pal.muted()),
                        RunStatus::Exited(Some(0)) => {
                            ("Finished with exit code 0".into(), pal.added)
                        }
                        RunStatus::Exited(Some(n)) => {
                            (format!("Finished with exit code {n}"), pal.removed)
                        }
                        RunStatus::Exited(None) => ("Stopped".into(), pal.muted()),
                        RunStatus::Failed(e) => (e.clone(), pal.removed),
                    };
                    ui.label(RichText::new(text).small().color(color));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if run.status != RunStatus::Running
                            && widgets::quiet(ui, "Hide output", None).clicked()
                        {
                            self.view.hidden_output.insert(id);
                        }
                    });
                });
            });
    }

    pub(super) fn command_editor(
        &mut self,
        ui: &mut Ui,
        project: ProjectId,
        id: Option<CommandId>,
        mut label: String,
        mut line: String,
    ) {
        let r = dialogs::sheet(ui, "command_editor", 520.0, true, |ui| {
            let title = if id.is_some() {
                "Edit command"
            } else {
                "Add command"
            };
            dialogs::sheet_title(ui, title);
            widgets::muted(ui, "Label");
            ui.add(
                TextEdit::singleline(&mut label)
                    .hint_text(widgets::hint("Fetch dependencies"))
                    .desired_width(f32::INFINITY),
            );
            ui.add_space(8.0);
            widgets::muted(ui, "Command");
            ui.add(
                TextEdit::multiline(&mut line)
                    .font(egui::TextStyle::Monospace)
                    .hint_text(widgets::hint("make deps").monospace())
                    .desired_rows(3)
                    .desired_width(f32::INFINITY),
            );
            let shell = if cfg!(windows) {
                "Runs with PowerShell in the primary folder, only on the computer where you click it."
            } else {
                "Runs with your login shell in the primary folder, only on the computer where you \
                 click it."
            };
            widgets::small_muted(ui, shell);
            let ok = !label.trim().is_empty() && !line.trim().is_empty();
            let confirm = if id.is_some() {
                "Save command"
            } else {
                "Add command"
            };
            dialogs::button_row(ui, confirm, false, ok)
        });
        match (r.dismissed, r.inner) {
            (true, _) | (_, Some(false)) => self.view.sheet = Sheet::None,
            (_, Some(true)) => {
                let (l, c) = (label.trim().to_string(), line.trim().to_string());
                self.act(match id {
                    Some(id) => Action::EditCommand(project, id, l, c),
                    None => Action::AddCommand(project, l, c),
                });
                self.view.sheet = Sheet::None;
            }
            _ => {
                self.view.sheet = Sheet::CommandEditor {
                    project,
                    id,
                    label,
                    line,
                }
            }
        }
    }
}

pub fn os_name(os: Os) -> &'static str {
    match os {
        Os::MacOs => "macOS",
        Os::Windows => "Windows",
    }
}
