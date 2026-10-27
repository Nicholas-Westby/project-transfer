//! Projects' commands and the shared commands: changing both together, so
//! linked commands never fall out of step, and swapping both with the
//! other computer at the end of every transfer and details sync.

use super::prepare::{local_project, unexpected};
use crate::merge::merge_commands;
use crate::model::{Command, Project, ProjectId, SharedCommand};
use crate::net::{Connection, Shared};
use crate::protocol::{Request, Response};
use crate::shared_commands::{merge_shared, reconcile};
use anyhow::anyhow;

/// Applies `f` to copies of both lists, puts linked commands in step, and
/// saves what changed before memory changes, as `update_projects` does.
pub async fn update_commands<T>(
    shared: &Shared,
    f: impl FnOnce(&mut Vec<Project>, &mut Vec<SharedCommand>) -> Result<T, String>,
) -> anyhow::Result<T> {
    let mut projects = shared.projects.write().await;
    let mut library = shared.shared_commands.write().await;
    let (mut next, mut next_library) = (projects.clone(), library.clone());
    let out = f(&mut next, &mut next_library).map_err(|e| anyhow!(e))?;
    reconcile(&mut next, &mut next_library);
    if next_library != *library {
        shared.store.save_shared_commands(&next_library)?;
    }
    if next != *projects {
        shared.store.save_projects(&next)?;
    }
    *projects = next;
    *library = next_library;
    Ok(out)
}

/// The run hash is this computer's own record, so it never leaves it.
pub fn wire_commands(commands: &[Command]) -> Vec<Command> {
    commands
        .iter()
        .map(|c| Command {
            last_run_hash: None,
            ..c.clone()
        })
        .collect()
}

/// Takes in the other computer's commands for `project`, if this computer
/// has it, and its shared commands, if it sent them. Returns the project's
/// commands (the other computer's own when this one lacks the project) and
/// every shared command, as they go back.
pub async fn exchange_commands(
    shared: &Shared,
    project: ProjectId,
    remote: &[Command],
    remote_shared: Option<&[SharedCommand]>,
) -> anyhow::Result<(Vec<Command>, Vec<SharedCommand>)> {
    update_commands(shared, |all, library| {
        if let Some(theirs) = remote_shared {
            *library = merge_shared(library, theirs);
        }
        if let Some(p) = all.iter_mut().find(|p| p.id == project) {
            p.commands = merge_commands(&p.commands, remote);
        }
        Ok(())
    })
    .await?;
    // Read after the merge, so the reply carries what reconciling made.
    let commands = match local_project(shared, project).await {
        Some(p) => wire_commands(&p.commands),
        None => wire_commands(remote),
    };
    let library = shared.shared_commands.read().await.clone();
    Ok((commands, library))
}

/// Sends `project`'s commands and the shared commands to the other
/// computer, which merges them first, and takes in what it sends back.
pub async fn swap_commands(
    conn: &mut Connection,
    shared: &Shared,
    project: ProjectId,
) -> anyhow::Result<()> {
    let mine = local_project(shared, project).await.map(|p| p.commands);
    let library = shared.shared_commands.read().await.clone();
    let ask = Request::ExchangeCommands {
        project,
        commands: wire_commands(&mine.unwrap_or_default()),
        shared: Some(library),
    };
    match exchanged(conn.request(&ask).await?) {
        Ok((theirs, their_shared)) => {
            exchange_commands(shared, project, &theirs, their_shared.as_deref()).await?;
            Ok(())
        }
        Err(other) => Err(unexpected(conn, "the command exchange", *other)),
    }
}

type Exchanged = (Vec<Command>, Option<Vec<SharedCommand>>);

/// The commands in an answer to `ExchangeCommands`, with the shared
/// commands when the answering version has them; anything else, as it came.
fn exchanged(r: Response) -> Result<Exchanged, Box<Response>> {
    match r {
        Response::Commands(commands) => Ok((commands, None)),
        Response::CommandsWithShared { commands, shared } => Ok((commands, Some(shared))),
        other => Err(Box::new(other)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_answer_from_a_version_without_shared_commands_still_counts() {
        assert_eq!(exchanged(Response::Commands(vec![])), Ok((vec![], None)));
        let both = Response::CommandsWithShared {
            commands: vec![],
            shared: vec![],
        };
        assert_eq!(exchanged(both), Ok((vec![], Some(vec![]))));
        assert_eq!(exchanged(Response::Ok), Err(Box::new(Response::Ok)));
    }
}
