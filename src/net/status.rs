//! What a paired computer may learn about this one: its status, with the
//! paired computers this one can pass a connection along to, and its
//! projects.

use super::Shared;
use crate::model::{InstanceId, Permissions, ProjectId};
use crate::protocol::{ProjectSummary, Reachable, RemoteFolder, RemoteProject, Response};

/// `reachable` leaves out the caller: it asked to reach the others.
pub(super) async fn status(shared: &Shared, allows: Permissions, caller: InstanceId) -> Response {
    let projects = shared
        .projects
        .read()
        .await
        .iter()
        .map(|p| ProjectSummary {
            id: p.id,
            name: p.name.clone(),
        })
        .collect();
    let seen: Vec<InstanceId> = shared.found.read().await.keys().copied().collect();
    let reachable = shared
        .peers
        .read()
        .await
        .iter()
        .filter(|p| p.id != caller && seen.contains(&p.id))
        .map(|p| Reachable {
            id: p.id,
            name: p.name.clone(),
        })
        .collect();
    Response::Status {
        allows,
        projects,
        reachable,
    }
}

pub(super) async fn project_info(shared: &Shared, project: ProjectId) -> Response {
    let projects = shared.projects.read().await;
    let info = projects
        .iter()
        .find(|p| p.id == project)
        .map(|p| RemoteProject {
            name: p.name.clone(),
            primary: p.primary,
            description: p.description.clone(),
            folders: p
                .folders
                .iter()
                .map(|f| RemoteFolder {
                    id: f.id,
                    name: f.name.clone(),
                    path: f.local_path.as_ref().map(|l| l.display().to_string()),
                })
                .collect(),
        });
    Response::ProjectInfo(info)
}
