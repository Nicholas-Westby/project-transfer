//! Treating two computers' same-named projects as one. A project made on
//! each computer separately gets a different id on each, so without this the
//! other computer's copy looked like a new project, and a transfer between
//! them made a second folder beside the first.

use super::projects::update_projects;
use crate::model::{FolderId, Project, ProjectId};
use crate::net::{Connection, Shared};
use crate::protocol::{ProjectSummary, RemoteProject, Request, Response};
use anyhow::bail;
use std::collections::HashMap;
use tracing::info;

/// Names match the way a person reads them: case and outer spaces aside.
pub fn same_name(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

/// The id the other computer has for `project` when it differs from ours:
/// both computers must have exactly one project of that name, and the
/// other's id must be new here. Anything less certain stays two projects.
pub fn counterpart(
    local: &[Project],
    project: &Project,
    remote: &[ProjectSummary],
) -> Option<ProjectId> {
    if remote.iter().any(|r| r.id == project.id) {
        return None;
    }
    let mut named = remote.iter().filter(|r| same_name(&r.name, &project.name));
    let found = named.next()?;
    let unique_here = local
        .iter()
        .filter(|p| same_name(&p.name, &project.name))
        .count()
        == 1;
    let new_here = !local.iter().any(|p| p.id == found.id);
    (named.next().is_none() && unique_here && new_here).then_some(found.id)
}

/// Our folder id to the other computer's, for folders of the same name
/// that differ only in id. Each of theirs is matched at most once. Folders
/// named differently are never matched: a mirror between unrelated folders
/// would remove files.
pub fn folder_map(local: &Project, remote: &RemoteProject) -> HashMap<FolderId, FolderId> {
    let mut map = HashMap::new();
    let ours = |id: FolderId| local.folders.iter().any(|f| f.id == id);
    for f in &local.folders {
        if remote.folders.iter().any(|r| r.id == f.id) {
            continue;
        }
        // Two of ours that read the same leave it unclear which one is meant.
        let alike = local.folders.iter().filter(|o| same_name(&o.name, &f.name));
        if alike.count() > 1 {
            continue;
        }
        let mut named = remote.folders.iter().filter(|r| {
            same_name(&r.name, &f.name) && !ours(r.id) && !map.values().any(|v| *v == r.id)
        });
        if let (Some(r), None) = (named.next(), named.next()) {
            map.insert(f.id, r.id);
        }
    }
    map
}

/// Gives `project` the other computer's ids, keeping its paths, commands
/// and history. Both sides then name it the same way in every request.
pub fn rekey(
    all: &mut [Project],
    project: ProjectId,
    to: ProjectId,
    folders: &HashMap<FolderId, FolderId>,
) -> Result<(), String> {
    if all.iter().any(|p| p.id == to) {
        return Err("Another project here already has that id.".into());
    }
    let p = all
        .iter_mut()
        .find(|p| p.id == project)
        .ok_or("The project is no longer on this computer.")?;
    p.id = to;
    for f in &mut p.folders {
        if let Some(id) = folders.get(&f.id) {
            f.id = *id;
        }
    }
    if let Some(id) = folders.get(&p.primary) {
        p.primary = *id;
    }
    Ok(())
}

/// The other computer's projects keyed the way this one shows them: one
/// matched to a local project by name is filed under the local id, with its
/// folders under the local folder ids, so it shows as the same project.
pub fn as_seen_here(
    local: &[Project],
    list: &[ProjectSummary],
    mut infos: HashMap<ProjectId, RemoteProject>,
) -> HashMap<ProjectId, RemoteProject> {
    for p in local {
        let Some(rid) = counterpart(local, p, list) else {
            continue;
        };
        let Some(mut info) = infos.remove(&rid) else {
            continue;
        };
        let back: HashMap<FolderId, FolderId> = folder_map(p, &info)
            .into_iter()
            .map(|(l, r)| (r, l))
            .collect();
        for f in &mut info.folders {
            if let Some(id) = back.get(&f.id) {
                f.id = *id;
            }
        }
        if let Some(id) = back.get(&info.primary) {
            info.primary = *id;
        }
        infos.insert(p.id, info);
    }
    infos
}

/// A project here to be given the other computer's ids: worked out by the
/// preview, saved only when the transfer runs, so a cancelled preview
/// leaves the project as it was.
#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    pub from: ProjectId,
    pub to: ProjectId,
    pub folders: HashMap<FolderId, FolderId>,
    /// The other computer's name for it, for the preview.
    pub their_name: String,
}

impl Link {
    pub fn warning(&self, peer: &str) -> String {
        format!(
            "Matched with the project `{}` {peer} already has, so this transfer uses its \
             folders there. Check the paths below.",
            self.their_name
        )
    }

    /// Saves the new ids. A project already moved to them is left alone.
    pub(super) async fn apply(&self, shared: &Shared) -> anyhow::Result<()> {
        update_projects(shared, |all| {
            if all.iter().any(|p| p.id == self.to) && !all.iter().any(|p| p.id == self.from) {
                return Ok(());
            }
            rekey(all, self.from, self.to, &self.folders)
                .map_err(|_| "This project changed since the preview. Preview again.".to_string())
        })
        .await?;
        info!(
            "matched project {} with the other computer's",
            self.their_name
        );
        Ok(())
    }
}

/// Before a transfer: when the other computer has its own project of this
/// name, the transfer should use its ids, so it lands in the folders that
/// computer already has. The ids that change are always the ones here, so
/// the other computer needs nothing new to take part.
pub(super) async fn plan(
    conn: &mut Connection,
    local: &[Project],
    project: ProjectId,
    theirs: &[ProjectSummary],
) -> anyhow::Result<Option<Link>> {
    let Some(mine) = local.iter().find(|p| p.id == project) else {
        return Ok(None);
    };
    let Some(rid) = counterpart(local, mine, theirs) else {
        return Ok(None);
    };
    let info = match conn.request(&Request::ProjectInfo { project: rid }).await? {
        Response::ProjectInfo(Some(info)) => info,
        // Gone since it listed it: nothing to match.
        Response::ProjectInfo(None) => return Ok(None),
        Response::Refused { reason } => bail!("{reason}"),
        other => bail!(
            "{} answered the project request with {other:?}",
            conn.peer_name()
        ),
    };
    Ok(Some(Link {
        from: project,
        to: rid,
        folders: folder_map(mine, &info),
        their_name: info.name,
    }))
}

#[cfg(test)]
#[path = "link_tests.rs"]
mod tests;
