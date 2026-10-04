//! Project bookkeeping both sides of a transfer share: where a folder lands,
//! taking in a project from the other computer, commands and records.

use super::apply::validate_name;
use crate::merge::merge_commands;
use crate::model::{
    Command, Direction, Folder, FolderId, InstanceId, Project, ProjectId, TransferRecord,
};
use crate::net::Shared;
use anyhow::anyhow;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

/// Fails with a sentence for the preview when the computer that receives the
/// files could not hold a folder's name. The project's own name is a label;
/// `default_path` makes a folder name from it when it needs one.
pub fn check_names_for(
    folders: &[&str],
    dest: crate::model::Os,
    computer: &str,
) -> anyhow::Result<()> {
    let unusable = |kind: &str, name: &str| match dest {
        crate::model::Os::Windows => crate::naming::name_problem(kind, name),
        _ if validate_name(name).is_err() => {
            crate::naming::name_problem(kind, name).or_else(|| {
                Some(format!(
                    "{kind} name `{name}` can't be used as a folder name."
                ))
            })
        }
        _ => None,
    };
    for f in folders {
        if let Some(why) = unusable("Folder", f) {
            anyhow::bail!(
                "{why} {computer} can't hold it. Remove the folder from the project or rename \
                 it on disk, then preview again."
            );
        }
    }
    Ok(())
}

/// How many "<name> N" siblings to try before giving up.
const MAX_SUFFIX: u32 = 100;

/// Where a folder this computer has no path for lands:
/// `<projects>/<project>/<folder>` when the project has several folders,
/// otherwise `<projects>/<folder>`. Local names win over incoming ones.
///
/// The part directly under the Projects folder gets a " 2", " 3"... suffix
/// while the path equals, contains or lies inside another project's folder,
/// so a new folder never mirrors over someone else's files. Every side uses
/// this one function, so the preview shows the path the transfer uses.
pub fn default_path(
    projects_folder: &Path,
    all: &[Project],
    project: ProjectId,
    incoming_name: &str,
    folder: FolderId,
    folder_name: &str,
    incoming_multi: bool,
) -> Result<PathBuf, String> {
    let local = all.iter().find(|p| p.id == project);
    let project_name = local.map_or(incoming_name, |p| p.name.as_str());
    let folder_name = local
        .and_then(|p| p.folders.iter().find(|f| f.id == folder))
        .map_or(folder_name, |f| f.name.as_str());
    let mut ids: HashSet<FolderId> = local
        .map(|p| p.folders.iter().map(|f| f.id).collect())
        .unwrap_or_default();
    ids.insert(folder);
    validate_name(folder_name)?;
    let multi = incoming_multi || ids.len() > 1;
    let holder = crate::naming::project_folder_name(project_name);
    let (top, rest) = if multi {
        (holder.as_str(), Some(folder_name))
    } else {
        (folder_name, None)
    };
    let taken: Vec<(&str, &Path)> = all
        .iter()
        .filter(|p| p.id != project)
        .flat_map(|p| {
            p.folders
                .iter()
                .filter_map(move |f| Some((p.name.as_str(), f.local_path.as_deref()?)))
        })
        .collect();
    if let Some((name, path)) = taken.iter().find(|(_, t)| projects_folder.starts_with(t)) {
        return Err(format!(
            "The Projects folder {} is inside {}, a folder of project `{name}`. Choose another \
             Projects folder in Settings, then try again.",
            projects_folder.display(),
            path.display()
        ));
    }
    for n in 1..=MAX_SUFFIX {
        let mut path = match n {
            1 => projects_folder.join(top),
            n => projects_folder.join(format!("{top} {n}")),
        };
        if let Some(r) = rest {
            path.push(r);
        }
        if !taken
            .iter()
            .any(|(_, t)| path.starts_with(t) || t.starts_with(&path))
        {
            return Ok(path);
        }
    }
    Err(format!(
        "Could not find a free folder name for `{top}` in {}. Set this folder's path on this \
         computer, then try again.",
        projects_folder.display()
    ))
}

/// Takes in `incoming` from the other computer: creates it if unknown, adds
/// folders it lacks, and gives each folder without a path `chosen[id]` or the
/// default. Never changes an existing path or a local name.
pub fn adopt(
    projects: &mut Vec<Project>,
    incoming: &Project,
    projects_folder: &Path,
    chosen: &HashMap<FolderId, PathBuf>,
) -> Result<(), String> {
    let multi = incoming.folders.len() > 1;
    let local = projects.iter().find(|p| p.id == incoming.id).cloned();
    let mut next = local.clone().unwrap_or_else(|| Project {
        id: incoming.id,
        name: incoming.name.clone(),
        folders: Vec::new(),
        primary: incoming.primary,
        commands: Vec::new(),
        last_transfer: None,
    });
    for f in &incoming.folders {
        if !next.folders.iter().any(|x| x.id == f.id) {
            next.folders.push(Folder {
                id: f.id,
                name: f.name.clone(),
                local_path: None,
            });
        }
    }
    // Defaults are worked out against the project before this change, so a
    // preview of the same transfer shows the same paths.
    for f in next.folders.iter_mut().filter(|f| f.local_path.is_none()) {
        let path = match chosen.get(&f.id) {
            Some(p) => p.clone(),
            None => default_path(
                projects_folder,
                projects,
                incoming.id,
                &incoming.name,
                f.id,
                &f.name,
                multi,
            )?,
        };
        f.local_path = Some(path);
    }
    if !next.folders.iter().any(|f| f.id == next.primary) {
        next.primary = next.folders[0].id;
    }
    match projects.iter_mut().find(|p| p.id == next.id) {
        Some(p) => *p = next,
        None => projects.push(next),
    }
    Ok(())
}

/// Changes the project list, saving it before memory changes so memory never
/// claims what the disk does not have.
pub async fn update_projects<T>(
    shared: &Shared,
    f: impl FnOnce(&mut Vec<Project>) -> Result<T, String>,
) -> anyhow::Result<T> {
    let mut guard = shared.projects.write().await;
    let mut next = guard.clone();
    let out = f(&mut next).map_err(|e| anyhow!(e))?;
    shared.store.save_projects(&next)?;
    *guard = next;
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

/// The project as sent to the other computer: no paths, no run hashes.
pub fn wire_project(p: &Project) -> Project {
    Project {
        folders: p
            .folders
            .iter()
            .map(|f| Folder {
                local_path: None,
                ..f.clone()
            })
            .collect(),
        commands: wire_commands(&p.commands),
        ..p.clone()
    }
}

/// Merges `remote` into the project if this computer has it, and returns the
/// list to send back.
pub async fn exchange_commands(
    shared: &Shared,
    project: ProjectId,
    remote: &[Command],
) -> anyhow::Result<Vec<Command>> {
    update_projects(shared, |all| {
        Ok(match all.iter_mut().find(|p| p.id == project) {
            Some(p) => {
                p.commands = merge_commands(&p.commands, remote);
                wire_commands(&p.commands)
            }
            None => wire_commands(remote),
        })
    })
    .await
}

/// Which computer started a transfer, so each side describes it from where
/// it sits.
pub enum StartedBy {
    Here,
    Peer,
}

pub async fn record_transfer(
    shared: &Shared,
    project: ProjectId,
    peer: InstanceId,
    direction: Direction,
    files: u64,
    started: StartedBy,
) -> anyhow::Result<()> {
    update_projects(shared, |all| {
        let p = all
            .iter_mut()
            .find(|p| p.id == project)
            .ok_or("The project is no longer on this computer.")?;
        p.last_transfer = Some(TransferRecord {
            at_ms: now_ms(),
            peer,
            direction,
            files,
            by_peer: matches!(started, StartedBy::Peer),
        });
        Ok(())
    })
    .await
}

#[cfg(test)]
#[path = "projects_tests.rs"]
mod tests;
