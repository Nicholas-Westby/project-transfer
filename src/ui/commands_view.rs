//! A project's commands: run, stop, read the output, add, edit, unlink,
//! delete.

use super::theme::{CONTROL_RADIUS, Palette};
use super::widgets::{self, mono};
use super::{App, Confirm, Sheet, add_command, command_editor, run_sheet};
use crate::core::{Action, CommandRun, RunStatus, UiState};
use crate::model::{Command, CommandId, Os, Project, ProjectId};
use egui::{Align, Frame, Layout, Margin, RichText, ScrollArea, Sides, Ui};

impl App {
    pub(super) fn commands_section(&mut self, ui: &mut Ui, s: &UiState, p: &Project) {
        ui.horizontal(|ui| {
            widgets::section(ui, "Commands");
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Add command").clicked() {
                    self.view.sheet = Sheet::AddCommand(add_command::Draft::new(p.id));
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
        let linked = c.linked_to.is_some();
        // The buttons on the right go first; the command line gives way.
        let (start, (delete, edit, unlink, stop)) = Sides::new().shrink_left().truncate().show(
            ui,
            |ui| {
                let r = ui.add_enabled(!running, egui::Button::new(format!("▶ {}", c.label)));
                if linked {
                    widgets::badge(ui, "Shared").on_hover_text(
                        "Linked to a shared command: editing it changes every project that uses \
                         it.",
                    );
                }
                let note = (c.created_on != Os::current()).then(|| {
                    let text = format!("Made on {}", os_name(c.created_on));
                    RichText::new(text).small().color(pal.muted())
                });
                let room = note.as_ref().map_or(0.0, |n| {
                    widgets::text_width(ui, n.clone()) + ui.spacing().item_spacing.x
                });
                ui.scope(|ui| {
                    ui.set_max_width((ui.available_width() - room).max(0.0));
                    ui.label(mono(&c.line).color(pal.muted()));
                });
                if let Some(n) = note {
                    ui.label(n);
                }
                r.clicked()
            },
            |ui| {
                let delete = widgets::quiet(ui, "Delete", Some(pal.removed)).clicked();
                let edit = widgets::quiet(ui, "Edit", None).clicked();
                let unlink = linked && widgets::quiet(ui, "Unlink", None).clicked();
                let stop = running && widgets::quiet(ui, "Stop", Some(pal.removed)).clicked();
                (delete, edit, unlink, stop)
            },
        );
        if start {
            self.request_run(p.id, c);
        }
        if delete {
            self.view.confirm = Some(Confirm::DeleteCommand(p.id, c.id));
        }
        if edit {
            self.view.sheet = Sheet::EditCommand(command_editor::Draft::of(p.id, c));
        }
        if unlink {
            self.act(Action::UnlinkCommand(p.id, c.id));
        }
        if stop {
            self.act(Action::StopCommand(c.id));
        }
        if let Some(run) = run
            && !self.view.hidden_output.contains(&c.id)
        {
            self.output_panel(ui, c.id, run);
        }
    }

    /// Runs at once, or opens the run sheet when the command is new here or
    /// changed, or asks for values.
    fn request_run(&mut self, project: ProjectId, c: &Command) {
        if run_sheet::needed(c) {
            self.view.sheet = Sheet::RunCommand(run_sheet::Draft::of(project, c));
        } else {
            self.run_now(project, c.id, None, Vec::new());
        }
    }

    /// `shown` is the hash of the text the run sheet showed, if it opened;
    /// `values` what was typed for each input, by variable.
    pub(super) fn run_now(
        &mut self,
        project: ProjectId,
        id: CommandId,
        shown: Option<String>,
        values: Vec<(String, String)>,
    ) {
        self.view.hidden_output.remove(&id);
        self.act(Action::RunCommand(project, id, shown, values));
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
                let (text, color) = match &run.status {
                    RunStatus::Running => ("Running…".to_string(), pal.muted()),
                    RunStatus::Exited(Some(0)) => ("Finished with exit code 0".into(), pal.added),
                    RunStatus::Exited(Some(n)) => {
                        (format!("Finished with exit code {n}"), pal.removed)
                    }
                    RunStatus::Exited(None) => ("Stopped".into(), pal.muted()),
                    RunStatus::Failed(e) => (e.clone(), pal.removed),
                };
                // A long failure wraps beside the button rather than past it.
                let (_, hide) = Sides::new().shrink_left().wrap().show(
                    ui,
                    |ui| ui.label(RichText::new(text).small().color(color)),
                    |ui| {
                        run.status != RunStatus::Running
                            && widgets::quiet(ui, "Hide output", None).clicked()
                    },
                );
                if hide {
                    self.view.hidden_output.insert(id);
                }
            });
    }
}

pub fn os_name(os: Os) -> &'static str {
    match os {
        Os::MacOs => "macOS",
        Os::Windows => "Windows",
    }
}
