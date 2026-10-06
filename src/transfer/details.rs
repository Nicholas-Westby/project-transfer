//! A project's details without its files: folders, description and
//! commands. A push used to be the only way the other computer heard of a
//! new folder, so it couldn't say where the folder would land until files
//! were on their way.

use super::home::{hints_for, resolve};
use super::link::{self, Link};
use super::prepare::{local_project, unexpected};
use super::projects::{adopt, exchange_commands, update_projects, wire_commands, wire_project};
use super::validate_name;
use crate::model::{Folder, FolderId, Project, ProjectId};
use crate::net::{Connection, Shared};
use crate::protocol::{RemoteProject, Request, Response};
use anyhow::{Context, anyhow, bail};
use std::collections::HashMap;
use tracing::info;

/// What a sync did here.
#[derive(Debug)]
pub struct Synced {
    /// The project's id once it ran: the other computer's, if `link` is set.
    pub project: ProjectId,
    /// Set when the project took the other computer's ids, as a transfer
    /// would have.
    pub link: Option<Link>,
}

/// Sends `project`'s details to the other computer, then takes in what it
/// has that this one lacks. Never creates a folder or copies a file.
pub async fn sync_details(
    conn: &mut Connection,
    shared: &Shared,
    mut project: ProjectId,
) -> anyhow::Result<Synced> {
    let name = conn.peer_name().to_string();
    let theirs = match conn.request(&Request::Status).await? {
        Response::Status {
            allows, projects, ..
        } if allows.may_push_to_me => projects,
        Response::Status { .. } => bail!(
            "{name} doesn't let this computer push to it, and syncing details decides where \
             pushed files land there. Allow pushing for it on {name}, then try again."
        ),
        other => return Err(unexpected(conn, "the status request", other)),
    };
    let all = shared.projects.read().await.clone();
    let link = link::plan(conn, &all, project, &theirs).await?;
    if let Some(l) = &link {
        l.apply(shared).await?;
        project = l.to;
    }
    let local = local_project(shared, project)
        .await
        .context("This project is not on this computer.")?;
    let pf = shared.settings.read().await.projects_folder.clone();
    let home = shared.home.as_deref();
    let ask = Request::SyncProject {
        project: wire_project(&local),
        home_hints: hints_for(&local, &pf, home),
    };
    let info = match conn.request(&ask).await? {
        Response::ProjectInfo(Some(info)) => info,
        other => return Err(unexpected(conn, "the project details", other)),
    };
    let hints: HashMap<FolderId, _> = info
        .folders
        .iter()
        .filter_map(|f| Some((f.id, resolve(home, f.home_hint.as_deref()?)?)))
        .collect();
    let incoming = from_remote(project, info);
    // Folder names become folder names here.
    for f in &incoming.folders {
        validate_name(&f.name).map_err(|e| anyhow!(e))?;
    }
    update_projects(shared, |all| {
        adopt(all, &incoming, &pf, &HashMap::new(), &hints)
    })
    .await?;
    // The other computer merged ours already; this brings its list back.
    let mine = local_project(shared, project).await.map(|p| p.commands);
    let ex = Request::ExchangeCommands {
        project,
        commands: wire_commands(&mine.unwrap_or_default()),
    };
    match conn.request(&ex).await? {
        Response::Commands(theirs) => {
            exchange_commands(shared, project, &theirs).await?;
        }
        other => return Err(unexpected(conn, "the command exchange", other)),
    }
    conn.close().await;
    info!("synced details of {} with {name}", local.name);
    Ok(Synced { project, link })
}

/// Asks the other computer to keep `folder` at `path`, as typed there.
pub async fn set_peer_folder_path(
    conn: &mut Connection,
    project: ProjectId,
    folder: FolderId,
    path: &str,
) -> anyhow::Result<()> {
    let ask = Request::SetFolderPath {
        project,
        folder,
        path: path.to_string(),
    };
    let reply = conn.request(&ask).await?;
    conn.close().await;
    match reply {
        Response::Ok => Ok(()),
        other => Err(unexpected(conn, "the new folder path", other)),
    }
}

fn from_remote(id: ProjectId, info: RemoteProject) -> Project {
    Project {
        id,
        name: info.name,
        folders: info
            .folders
            .into_iter()
            .map(|f| Folder {
                id: f.id,
                name: f.name,
                local_path: None,
            })
            .collect(),
        primary: info.primary,
        commands: Vec::new(),
        last_transfer: None,
        description: info.description,
    }
}
