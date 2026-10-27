//! Editing a project's commands, including adding a shared command to one,
//! linked or as a separate copy. Every edit here and in `library.rs` goes
//! through `update_commands`, which keeps linked commands in step with
//! their shared command.

use super::Core;
use super::library::{edit_shared, live_shared};
use super::projects::{Edit, project, required};
use crate::model::{Command, CommandId, Os, Project, ProjectId, SharedCommand, SharedCommandId};
use crate::shared_commands::stamp_after;
use crate::transfer::commands::update_commands;
use crate::transfer::projects::now_ms;

impl Core {
    /// Applies `f`, saves, refreshes the UI and records `f`'s sentence.
    pub(super) async fn edit_commands(
        &self,
        f: impl FnOnce(&mut Vec<Project>, &mut Vec<SharedCommand>) -> Edit<String>,
    ) -> anyhow::Result<()> {
        let text = update_commands(&self.shared, f).await?;
        self.sync_projects().await;
        self.ui.info(text);
        Ok(())
    }

    pub(super) async fn add_command(
        &self,
        id: ProjectId,
        label: String,
        line: String,
    ) -> anyhow::Result<()> {
        self.edit_commands(|all, _| {
            let (label, line) = command_text(&label, &line)?;
            let p = project(all, id)?;
            p.commands
                .push(new_command(&label, &line, Os::current(), now_ms(), None));
            Ok(format!("Added command `{label}` to `{}`.", p.name))
        })
        .await
    }

    pub(super) async fn add_shared_command(
        &self,
        id: ProjectId,
        label: String,
        line: String,
    ) -> anyhow::Result<()> {
        self.edit_commands(|all, library| {
            let (label, line) = command_text(&label, &line)?;
            let p = project(all, id)?;
            let s = SharedCommand {
                id: SharedCommandId::new_v4(),
                label: label.clone(),
                line,
                created_on: Os::current(),
                updated_at_ms: now_ms(),
                deleted: false,
            };
            p.commands.push(linked_to(&s));
            library.push(s);
            Ok(format!("Added shared command `{label}` to `{}`.", p.name))
        })
        .await
    }

    /// A linked command's edit goes to its shared command, and from there to
    /// every command linked to it.
    pub(super) async fn edit_command(
        &self,
        id: ProjectId,
        cmd: CommandId,
        label: String,
        line: String,
    ) -> anyhow::Result<()> {
        self.edit_commands(|all, library| {
            let (label, line) = command_text(&label, &line)?;
            let c = live_command(project(all, id)?, cmd)?;
            let Some(shared) = c.linked_to else {
                c.label = label.clone();
                c.line = line;
                c.updated_at_ms = stamp_after(c.updated_at_ms, now_ms());
                return Ok(format!("Saved command `{label}`."));
            };
            edit_shared(all, library, shared, label, line)
        })
        .await
    }

    pub(super) async fn delete_command(&self, id: ProjectId, cmd: CommandId) -> anyhow::Result<()> {
        self.edit_commands(|all, _| {
            let c = live_command(project(all, id)?, cmd)?;
            c.deleted = true;
            c.updated_at_ms = stamp_after(c.updated_at_ms, now_ms());
            Ok(format!(
                "Deleted command `{}`. Paired computers drop it after the next transfer.",
                c.label
            ))
        })
        .await
    }

    pub(super) async fn share_command(&self, id: ProjectId, cmd: CommandId) -> anyhow::Result<()> {
        self.edit_commands(|all, library| {
            let c = live_command(project(all, id)?, cmd)?;
            if c.linked_to.is_some() {
                return Err(format!("`{}` is a shared command already.", c.label));
            }
            let s = SharedCommand {
                id: SharedCommandId::new_v4(),
                label: c.label.clone(),
                line: c.line.clone(),
                created_on: c.created_on,
                updated_at_ms: stamp_after(c.updated_at_ms, now_ms()),
                deleted: false,
            };
            c.linked_to = Some(s.id);
            c.updated_at_ms = s.updated_at_ms;
            let text = format!(
                "Shared `{}`. Add it to other projects from Add command.",
                s.label
            );
            library.push(s);
            Ok(text)
        })
        .await
    }

    pub(super) async fn unlink_command(&self, id: ProjectId, cmd: CommandId) -> anyhow::Result<()> {
        self.edit_commands(|all, _| {
            let p = project(all, id)?;
            let name = p.name.clone();
            let c = live_command(p, cmd)?;
            if c.linked_to.take().is_none() {
                return Err(format!("`{}` isn't linked to a shared command.", c.label));
            }
            c.updated_at_ms = stamp_after(c.updated_at_ms, now_ms());
            Ok(format!(
                "Unlinked `{}` in `{name}`. It keeps its text and no longer follows the shared \
                 command.",
                c.label
            ))
        })
        .await
    }

    pub(super) async fn link_shared_command(
        &self,
        id: ProjectId,
        shared: SharedCommandId,
    ) -> anyhow::Result<()> {
        self.edit_commands(|all, library| {
            let s = live_shared(library, shared)?;
            let p = project(all, id)?;
            if p.commands
                .iter()
                .any(|c| !c.deleted && c.linked_to == Some(shared))
            {
                return Err(format!("`{}` is linked in `{}` already.", s.label, p.name));
            }
            p.commands.push(linked_to(s));
            Ok(format!(
                "Added `{}` to `{}`, linked to the shared command.",
                s.label, p.name
            ))
        })
        .await
    }

    pub(super) async fn copy_shared_command(
        &self,
        id: ProjectId,
        shared: SharedCommandId,
    ) -> anyhow::Result<()> {
        self.edit_commands(|all, library| {
            let s = live_shared(library, shared)?;
            let p = project(all, id)?;
            // Made where the text was written, so another system still warns.
            p.commands
                .push(new_command(&s.label, &s.line, s.created_on, now_ms(), None));
            Ok(format!(
                "Added a separate copy of `{}` to `{}`.",
                s.label, p.name
            ))
        })
        .await
    }
}

const GONE_COMMAND: &str = "That command no longer exists.";

pub(super) fn live_command(p: &mut Project, id: CommandId) -> Edit<&mut Command> {
    p.commands
        .iter_mut()
        .find(|c| c.id == id && !c.deleted)
        .ok_or_else(|| GONE_COMMAND.to_string())
}

fn new_command(
    label: &str,
    line: &str,
    created_on: Os,
    at_ms: i64,
    linked_to: Option<SharedCommandId>,
) -> Command {
    Command {
        id: CommandId::new_v4(),
        label: label.to_string(),
        line: line.to_string(),
        created_on,
        updated_at_ms: at_ms,
        deleted: false,
        last_run_hash: None,
        linked_to,
    }
}

/// A new command following `s`, carrying its version as every linked
/// command does.
fn linked_to(s: &SharedCommand) -> Command {
    new_command(&s.label, &s.line, s.created_on, s.updated_at_ms, Some(s.id))
}

pub(super) fn command_text(label: &str, line: &str) -> Edit<(String, String)> {
    let label = required(label, "Enter a label for the command.")?;
    let line = required(line, "Enter the command to run.")?;
    Ok((label, line))
}
