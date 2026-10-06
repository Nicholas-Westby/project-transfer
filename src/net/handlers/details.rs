//! The receiving side of "Sync details" and of changing where a folder
//! lives here from the other computer. Neither touches files: a push later
//! creates whatever folder they point at.

use super::super::status::project_info;
use super::{Ctx, refused};
use crate::merge::merge_commands;
use crate::model::{FolderId, Project, ProjectId};
use crate::net::NetEvent;
use crate::protocol::Response;
use crate::transfer::home::{check_overlap, expand, resolve_all, too_broad};
use crate::transfer::projects::{adopt, update_projects, wire_commands};
use crate::transfer::validate_name;
use std::collections::HashMap;
use std::path::{Component, Path};
use tracing::{info, warn};

pub async fn sync(ctx: &Ctx<'_>, project: Project, hints: HashMap<FolderId, String>) -> Response {
    // Folder names become folder names here; the project's name is a label.
    for f in &project.folders {
        if let Err(reason) = validate_name(&f.name) {
            return refused(reason);
        }
    }
    if project.folders.is_empty() {
        return refused("The project has no folders. Add one, then sync again.");
    }
    let pf = ctx.shared.settings.read().await.projects_folder.clone();
    let hints = resolve_all(&hints, ctx.shared.home.as_deref());
    let commands = wire_commands(&project.commands);
    let added = update_projects(ctx.shared, |all| {
        let before = all
            .iter()
            .find(|p| p.id == project.id)
            .map_or(0, |p| p.folders.len());
        adopt(all, &project, &pf, &HashMap::new(), &hints)?;
        let p = all
            .iter_mut()
            .find(|p| p.id == project.id)
            .expect("just adopted");
        p.commands = merge_commands(&p.commands, &commands);
        Ok(p.folders.len() - before)
    })
    .await;
    let peer = &ctx.peer.name;
    match added {
        Ok(added) => {
            let name = local_name(ctx, project.id).await;
            info!("{peer} synced details of {name}: {added} folders new here");
            let text = format!("{peer} synced details of `{name}`.");
            let _ = ctx.shared.events.send(NetEvent::ProjectsChanged(text));
            project_info(ctx.shared, project.id).await
        }
        Err(e) => {
            warn!(
                "could not take in details of {} from {peer}: {e:#}",
                project.name
            );
            refused(format!("Could not save the project details: {e:#}"))
        }
    }
}

pub async fn set_path(
    ctx: &Ctx<'_>,
    project: ProjectId,
    folder: FolderId,
    typed: &str,
) -> Response {
    let me = ctx.shared.settings.read().await.name.clone();
    let typed = typed.trim();
    let path = expand(typed, ctx.shared.home.as_deref());
    if let Err(why) = check_path(&path, typed, &me, ctx.shared.home.as_deref()) {
        return refused(why);
    }
    let done = update_projects(ctx.shared, |all| {
        let not_here = |what: &str| {
            format!("{me} doesn't have this {what} yet. Sync details with {me}, then try again.")
        };
        let p = all
            .iter()
            .find(|p| p.id == project)
            .ok_or_else(|| not_here("project"))?;
        if !p.folders.iter().any(|f| f.id == folder) {
            return Err(not_here("folder"));
        }
        check_overlap(all, &path, Some(folder))?;
        let p = all.iter_mut().find(|p| p.id == project).expect("found");
        let f = p
            .folders
            .iter_mut()
            .find(|f| f.id == folder)
            .expect("found");
        f.local_path = Some(path.clone());
        Ok((p.name.clone(), f.name.clone()))
    })
    .await;
    let peer = &ctx.peer.name;
    match done {
        Ok((project, folder)) => {
            let text = format!(
                "{peer} set where `{folder}` of `{project}` lives on this computer: {}.",
                path.display()
            );
            let _ = ctx.shared.events.send(NetEvent::ProjectsChanged(text));
            Response::Ok
        }
        Err(e) => {
            info!(
                "did not let {peer} move a folder to {}: {e:#}",
                path.display()
            );
            refused(format!("{e:#}"))
        }
    }
}

/// The checks a folder chosen here gets, except that it need not exist
/// yet: the next push creates it.
fn check_path(path: &Path, typed: &str, me: &str, home: Option<&Path>) -> Result<(), String> {
    if !path.is_absolute() {
        return Err(format!(
            "`{typed}` is not a full path on {me}. Enter one from the top of the disk, or \
             start it with ~ for the home folder there."
        ));
    }
    // Overlap checks compare paths as written, which `..` would get round.
    if path
        .components()
        .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
    {
        return Err(format!(
            "`{typed}` has a `.` or `..` part. Enter the path without them."
        ));
    }
    if path.exists() && !path.is_dir() {
        return Err(format!(
            "{} is a file on {me}. Enter the path of a folder.",
            path.display()
        ));
    }
    match too_broad(path, home) {
        Some(why) => Err(why),
        None => Ok(()),
    }
}

/// The name this computer shows for the project, which may differ in case.
async fn local_name(ctx: &Ctx<'_>, id: ProjectId) -> String {
    ctx.shared
        .projects
        .read()
        .await
        .iter()
        .find(|p| p.id == id)
        .map(|p| p.name.clone())
        .unwrap_or_default()
}
