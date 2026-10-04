//! Building the preview: scan both sides, compare, hash only what is unclear.

use super::preview::{
    FolderPreview, Preview, Replaced, TransferRequest, drop_unholdable, replaced_folders,
};
use super::projects::{check_names_for, default_path};
use super::safe_join;
use crate::ignore_rules::{IgnoreSpec, Matcher};
use crate::manifest::{Manifest, compare, hash_file, resolve_hashes, scan};
use crate::model::{Direction, FolderId, Os, Project, ProjectId};
use crate::net::{Connection, Shared};
use crate::protocol::{FolderScan, Request, Response};
use anyhow::{Context, bail};
use std::collections::HashMap;
use std::path::PathBuf;

/// Fails unless `conn` reaches the computer the request names.
pub(super) fn check_peer(conn: &Connection, req: &TransferRequest) -> anyhow::Result<()> {
    if conn.peer_id() != req.peer {
        bail!(
            "This connection goes to {}, which is not the computer you chose. Try again.",
            conn.peer_name()
        );
    }
    Ok(())
}

pub(super) async fn local_project(shared: &Shared, id: ProjectId) -> Option<Project> {
    shared
        .projects
        .read()
        .await
        .iter()
        .find(|p| p.id == id)
        .cloned()
}

pub(super) fn unexpected(conn: &Connection, what: &str, r: Response) -> anyhow::Error {
    match r {
        Response::Refused { reason } => anyhow::anyhow!("{reason}"),
        other => anyhow::anyhow!("{} answered {what} with {other:?}", conn.peer_name()),
    }
}

/// Asks first, so a missing permission stops the preview with a clear reason.
async fn check_allowed(conn: &mut Connection, direction: Direction) -> anyhow::Result<()> {
    let name = conn.peer_name().to_string();
    let allows = match conn.request(&Request::Status).await? {
        Response::Status { allows, .. } => allows,
        other => return Err(unexpected(conn, "the status request", other)),
    };
    match direction {
        Direction::Push if !allows.may_push_to_me => bail!(
            "{name} doesn't let this computer push to it. Allow pushing for it on {name}, then try again."
        ),
        Direction::Pull if !allows.may_pull_from_me => bail!(
            "{name} doesn't let this computer pull from it. Allow pulling for it on {name}, then try again."
        ),
        _ => Ok(()),
    }
}

pub async fn prepare(
    conn: &mut Connection,
    shared: &Shared,
    req: TransferRequest,
) -> anyhow::Result<Preview> {
    check_peer(conn, &req)?;
    check_allowed(conn, req.direction).await?;
    let settings = shared.settings.read().await.clone();
    let mut spec = IgnoreSpec::from_settings(&settings);
    spec.send_everything = req.send_everything;
    Matcher::new(&spec)?;
    let local = local_project(shared, req.project).await;
    let peer = conn.peer_name().to_string();
    let mut ctx = Ctx {
        conn,
        spec,
        project: req.project,
        warnings: Vec::new(),
    };
    let mut folders = Vec::new();
    match req.direction {
        Direction::Push => {
            let p = local.context("This project is not on this computer.")?;
            let multi = p.folders.len() > 1;
            for f in &p.folders {
                let Some(root) = f.local_path.clone().filter(|r| r.is_dir()) else {
                    ctx.warnings.push(format!(
                        "Folder `{}` was left out: it is not on this computer, and pushing it \
                         would empty the copy on {peer}.",
                        f.name
                    ));
                    continue;
                };
                let remote = ctx.remote_scan(f.id, &f.name, &p.name, multi).await?;
                let names: Vec<&str> = p.folders.iter().map(|f| f.name.as_str()).collect();
                check_names_for(&p.name, &names, remote.os, &peer)?;
                if !remote.set_up && remote.exists && !remote.manifest.entries.is_empty() {
                    ctx.warnings.push(unclaimed(&remote.path, &peer));
                }
                let mine = ctx.local_scan(root.clone()).await?;
                let side = Sides {
                    folder: f.id,
                    name: f.name.clone(),
                    src: (mine, root.display().to_string()),
                    dst: (remote.manifest, remote.path),
                    local_root: root,
                    local_is_src: true,
                    dest_exists: remote.exists,
                    dest_os: remote.os,
                };
                folders.push(ctx.plan(side).await?);
            }
        }
        Direction::Pull => {
            let info = match ctx
                .conn
                .request(&Request::ProjectInfo {
                    project: req.project,
                })
                .await?
            {
                Response::ProjectInfo(Some(info)) => info,
                Response::ProjectInfo(None) => bail!("{peer} does not have this project."),
                other => return Err(unexpected(ctx.conn, "the project request", other)),
            };
            let names: Vec<&str> = info.folders.iter().map(|f| f.name.as_str()).collect();
            check_names_for(&info.name, &names, Os::current(), "This computer")?;
            let multi = info.folders.len() > 1;
            for rf in &info.folders {
                let remote = ctx.remote_scan(rf.id, &rf.name, &info.name, multi).await?;
                if !remote.set_up || !remote.exists {
                    ctx.warnings.push(format!(
                        "Folder `{}` was left out: it is not on {peer}, and pulling it would \
                         empty the copy here.",
                        rf.name
                    ));
                    continue;
                }
                let known = local
                    .as_ref()
                    .and_then(|p| p.folders.iter().find(|f| f.id == rf.id));
                let dest = match known.and_then(|f| f.local_path.clone()) {
                    Some(p) => p,
                    None => match default_path(
                        &settings.projects_folder,
                        &shared.projects.read().await,
                        req.project,
                        &info.name,
                        rf.id,
                        &rf.name,
                        multi,
                    ) {
                        Ok(p) => p,
                        Err(reason) => {
                            ctx.warnings
                                .push(format!("Folder `{}` was left out: {reason}", rf.name));
                            continue;
                        }
                    },
                };
                let mine = ctx.local_scan(dest.clone()).await?;
                let set_up = known.is_some_and(|f| f.local_path.is_some());
                if !set_up && dest.is_dir() && !mine.entries.is_empty() {
                    ctx.warnings
                        .push(unclaimed(&dest.display().to_string(), &settings.name));
                }
                let side = Sides {
                    folder: rf.id,
                    name: known.map_or(rf.name.clone(), |f| f.name.clone()),
                    src: (remote.manifest, remote.path),
                    dst: (mine, dest.display().to_string()),
                    dest_exists: dest.is_dir(),
                    local_root: dest,
                    local_is_src: false,
                    dest_os: Os::current(),
                };
                folders.push(ctx.plan(side).await?);
            }
        }
    }
    Ok(Preview {
        request: req,
        folders,
        warnings: ctx.warnings,
    })
}

/// A default path can be a folder that already exists and belongs to no
/// project; the mirror would remove what is there.
fn unclaimed(path: &str, computer: &str) -> String {
    format!(
        "`{path}` already exists on {computer} and isn't part of this project yet. Files there \
         that aren't in the source will be removed."
    )
}

struct Ctx<'a> {
    conn: &'a mut Connection,
    spec: IgnoreSpec,
    project: ProjectId,
    warnings: Vec<String>,
}

/// One folder's two sides: (manifest, display path) each.
struct Sides {
    folder: FolderId,
    name: String,
    src: (Manifest, String),
    dst: (Manifest, String),
    /// This computer's side, which is hashed here.
    local_root: PathBuf,
    local_is_src: bool,
    dest_exists: bool,
    dest_os: Os,
}

impl Ctx<'_> {
    async fn remote_scan(
        &mut self,
        folder: FolderId,
        folder_name: &str,
        project_name: &str,
        multi_folder: bool,
    ) -> anyhow::Result<FolderScan> {
        let req = Request::Manifest {
            project: self.project,
            folder,
            ignore: self.spec.clone(),
            folder_name: folder_name.into(),
            project_name: project_name.into(),
            multi_folder,
        };
        match self.conn.request(&req).await? {
            Response::Manifest(s) => Ok(s),
            other => Err(unexpected(self.conn, "the folder scan", other)),
        }
    }

    async fn local_scan(&self, root: PathBuf) -> anyhow::Result<Manifest> {
        let spec = self.spec.clone();
        let shown = root.display().to_string();
        tokio::task::spawn_blocking(move || scan(&root, &Matcher::new(&spec)?))
            .await?
            .with_context(|| format!("Could not read {shown}"))
    }

    async fn remote_hashes(
        &mut self,
        folder: FolderId,
        paths: &[String],
    ) -> anyhow::Result<HashMap<String, String>> {
        let req = Request::Hashes {
            project: self.project,
            folder,
            paths: paths.to_vec(),
        };
        match self.conn.request(&req).await? {
            Response::Hashes(h) => Ok(h),
            other => Err(unexpected(self.conn, "the hash request", other)),
        }
    }

    async fn plan(&mut self, s: Sides) -> anyhow::Result<FolderPreview> {
        let (src, dst) = (&s.src.0, &s.dst.0);
        let mut plan = compare(src, dst);
        if !plan.needs_hash.is_empty() {
            let remote = self.remote_hashes(s.folder, &plan.needs_hash).await?;
            let local = local_hashes(s.local_root.clone(), plan.needs_hash.clone()).await;
            plan = if s.local_is_src {
                resolve_hashes(plan, &local, &remote)
            } else {
                resolve_hashes(plan, &remote, &local)
            };
        }
        let (plan, skipped, case_clashes) = drop_unholdable(plan, src, dst, s.dest_os);
        let mut replaced = replaced_folders(&plan, dst);
        replaced.extend(case_clashes);
        self.warnings.extend(replaced.iter().map(Replaced::warning));
        Ok(FolderPreview {
            folder: s.folder,
            name: s.name,
            source_path: s.src.1,
            dest_path: s.dst.1,
            dest_will_be_created: !s.dest_exists,
            plan,
            skipped,
            replaced,
        })
    }
}

async fn local_hashes(root: PathBuf, paths: Vec<String>) -> HashMap<String, String> {
    tokio::task::spawn_blocking(move || {
        paths
            .into_iter()
            .filter_map(|rel| {
                let p = safe_join(&root, &rel).ok()?;
                std::fs::symlink_metadata(&p)
                    .ok()?
                    .is_file()
                    .then_some(())?;
                Some((rel, hash_file(&p).ok()?))
            })
            .collect()
    })
    .await
    .unwrap_or_default()
}
