//! Running a project's commands here and collecting their output for the UI.
//! Output goes only to the UI, never to the log.

use super::projects::live_command;
use super::state::{CommandRun, OutputLine, RunStatus};
use super::{Core, lock};
use crate::commands::{self, CommandOutput};
use crate::model::{CommandId, ProjectId};
use crate::transfer::projects::update_projects;
use anyhow::{Context, bail};

impl Core {
    pub(super) async fn run_command(
        &self,
        id: ProjectId,
        cmd: CommandId,
        confirmed: Option<String>,
    ) -> anyhow::Result<()> {
        if lock(&self.runs).contains_key(&cmd) {
            bail!("That command is already running. Stop it first to run it again.");
        }
        let (command, cwd, project) = {
            let all = self.shared.projects.read().await;
            let p = all
                .iter()
                .find(|p| p.id == id)
                .context("That project is no longer on this computer.")?;
            let c = p
                .commands
                .iter()
                .find(|c| c.id == cmd && !c.deleted)
                .context("That command no longer exists.")?;
            let primary = p.folders.iter().find(|f| f.id == p.primary);
            let Some(cwd) = primary.and_then(|f| f.local_path.clone()) else {
                bail!(
                    "Choose where the primary folder of `{}` is on this computer, then run \
                     the command again.",
                    p.name
                );
            };
            (c.clone(), cwd, p.name.clone())
        };
        // A transfer can change the text between the dialog and this point.
        match &confirmed {
            Some(h) if *h != commands::command_hash(&command) => bail!(
                "`{}` changed after you confirmed it, so it did not run. Run it again to check \
                 the new text.",
                command.label
            ),
            None if commands::must_confirm(&command) => bail!(
                "`{}` is new or changed on this computer, so it did not run. Run it again to \
                 check its text first.",
                command.label
            ),
            _ => {}
        }
        // In place before the first line of output can arrive.
        self.ui.update(|s| {
            s.command_runs.insert(
                cmd,
                CommandRun {
                    lines: Vec::new(),
                    status: RunStatus::Running,
                },
            );
        });
        let (tx, rx) = std::sync::mpsc::channel();
        let running = match commands::run(&command.line, &cwd, tx) {
            Ok(r) => r,
            Err(e) => {
                let why = format!("Could not run `{}`: {e:#}", command.label);
                self.ui.update(|s| {
                    if let Some(r) = s.command_runs.get_mut(&cmd) {
                        r.status = RunStatus::Failed(why.clone());
                    }
                });
                bail!(why);
            }
        };
        lock(&self.runs).insert(cmd, running);
        let hash = commands::command_hash(&command);
        let saved = update_projects(&self.shared, |all| {
            let p = all
                .iter_mut()
                .find(|p| p.id == id)
                .ok_or("The project was removed.")?;
            live_command(p, cmd)?.last_run_hash = Some(hash);
            Ok(())
        })
        .await;
        if let Err(e) = saved {
            self.ui.warn(format!(
                "Could not record that `{}` ran ({e:#}); it asks for confirmation again next time.",
                command.label
            ));
        }
        self.sync_projects().await;
        self.ui
            .info(format!("Running `{}` in `{project}`.", command.label));
        let (ui, runs, label) = (self.ui.clone(), self.runs.clone(), command.label);
        std::thread::spawn(move || {
            for out in rx {
                let status = match out {
                    CommandOutput::Line { stderr, text } => {
                        ui.update(|s| {
                            if let Some(r) = s.command_runs.get_mut(&cmd) {
                                r.push(OutputLine { stderr, text });
                            }
                        });
                        continue;
                    }
                    CommandOutput::Exited(code) => RunStatus::Exited(code),
                    CommandOutput::Failed(e) => RunStatus::Failed(e),
                };
                lock(&runs).remove(&cmd);
                match &status {
                    RunStatus::Exited(Some(code)) => {
                        ui.info(format!("`{label}` finished with exit code {code}."))
                    }
                    RunStatus::Exited(None) => ui.info(format!("`{label}` was stopped.")),
                    RunStatus::Failed(e) => ui.error(format!("`{label}` failed: {e}")),
                    RunStatus::Running => {}
                }
                ui.update(|s| {
                    if let Some(r) = s.command_runs.get_mut(&cmd) {
                        r.status = status;
                    }
                });
                break;
            }
        });
        Ok(())
    }

    pub(super) fn stop_command(&self, cmd: CommandId) {
        if let Some(r) = lock(&self.runs).get(&cmd) {
            r.stop();
        }
    }
}
