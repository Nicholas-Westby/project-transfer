//! Building the preview: scan both sides, compare, hash only what is unclear.

use super::preview::{
    FolderPreview, LeftOut, Preview, Replaced, TransferRequest, drop_unholdable, replaced_folders,
};
use super::projects::{check_names_for, default_path};
use super::{home, link, safe_join};
use crate::ignore_rules::{IgnoreSpec, Matcher};
use crate::manifest::{Manifest, compare, compose_for, hash_file, resolve_hashes, scan};
use crate::model::{Direction, FolderId, Os, Project, ProjectId};
use crate::net::{Connection, Shared};
use crate::protocol::{FolderScan, ProjectSummary, Request, Response};
use anyhow::{Context, bail};
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::info;

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
/// Returns the other computer's projects.
async fn check_allowed(
    conn: &mut Connection,
    direction: Direction,
) -> anyhow::Result<Vec<ProjectSummary>> {
    let name = conn.peer_name().to_string();
    let (allows, projects) = match conn.request(&Request::Status).await? {
        Response::Status {
            allows, projects, ..
        } => (allows, projects),
        other => return Err(unexpected(conn, "the status request", other)),
    };
    match direction {
        Direction::Push if !allows.may_push_to_me => bail!(
            "{name} doesn't let this computer push to it. Allow pushing for it on {name}, then try again."
        ),
        Direction::Pull if !allows.may_pull_from_me => bail!(
            "{name} doesn't let this computer pull from it. Allow pulling for it on {name}, then try again."
        ),
        _ => Ok(projects),
    }
}

pub async fn prepare(
    conn: &mut Connection,
    shared: &Shared,
    mut req: TransferRequest,
) -> anyhow::Result<Preview> {
    check_peer(conn, &req)?;
    let theirs = check_allowed(conn, req.direction).await?;
    let peer = conn.peer_name().to_string();
    // The preview works on the projects as they will be once a match with
    // the other computer's project is saved, which only running it does.
    let mut projects = shared.projects.read().await.clone();
    let mut warnings = Vec::new();
    let link = link::plan(conn, &projects, req.project, &theirs).await?;
    if let Some(l) = &link {
        link::rekey(&mut projects, l.from, l.to, &l.folders).map_err(|e| anyhow::anyhow!(e))?;
        req.project = l.to;
        warnings.push(l.warning(&peer));
    }
    let settings = shared.settings.read().await.clone();
    let mut spec = IgnoreSpec::from_settings(&settings);
    spec.send_everything = req.send_everything;
    Matcher::new(&spec)?;
    let local = projects.iter().find(|p| p.id == req.project).cloned();
    let mut ctx = Ctx {
        conn,
        spec,
        project: req.project,
        warnings,
        left_out: Vec::new(),
    };
    let mut folders = Vec::new();
    let description;
    match req.direction {
        Direction::Push => {
            let p = local.context("This project is not on this computer.")?;
            let ask = Request::ProjectInfo {
                project: req.project,
            };
            let theirs = match ctx.conn.request(&ask).await? {
                Response::ProjectInfo(info) => info.map(|i| i.description).unwrap_or_default(),
                other => return Err(unexpected(ctx.conn, "the project request", other)),
            };
            description = p.description.replaces(&theirs);
            let multi = p.folders.len() > 1;
            let home = shared.home.as_deref();
            for f in &p.folders {
                let Some(root) = f.local_path.clone().filter(|r| r.is_dir()) else {
                    ctx.leave_out(
                        &f.name,
                        format!(
                            "It is not on this computer, and pushing it would empty the copy \
                             on {peer}."
                        ),
                    );
                    continue;
                };
                let hint = home.and_then(|h| home::home_hint(&root, &settings.projects_folder, h));
                let ask = Ask {
                    folder: f.id,
                    folder_name: &f.name,
                    project_name: &p.name,
                    multi,
                    hint,
                };
                let remote = ctx.remote_scan(ask).await?;
                let names: Vec<&str> = p.folders.iter().map(|f| f.name.as_str()).collect();
                check_names_for(&names, remote.os, &peer)?;
                if !remote.set_up && remote.exists && !remote.manifest.entries.is_empty() {
                    ctx.warnings.push(unclaimed(&remote.path, &peer));
                }
                let mine = ctx.local_scan(root.clone(), compose_for(remote.os)).await?;
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
            check_names_for(&names, Os::current(), "This computer")?;
            let mine = local.as_ref().map(|p| p.description.clone());
            description = info.description.replaces(&mine.unwrap_or_default());
            let multi = info.folders.len() > 1;
            for rf in &info.folders {
                let ask = Ask {
                    folder: rf.id,
                    folder_name: &rf.name,
                    project_name: &info.name,
                    multi,
                    hint: None,
                };
                let remote = ctx.remote_scan(ask).await?;
                if !remote.set_up || !remote.exists {
                    ctx.leave_out(
                        &rf.name,
                        format!("It is not on {peer}, and pulling it would empty the copy here."),
                    );
                    continue;
                }
                let known = local
                    .as_ref()
                    .and_then(|p| p.folders.iter().find(|f| f.id == rf.id));
                let dest = match known.and_then(|f| f.local_path.clone()) {
                    Some(p) => p,
                    None => match default_path(
                        &settings.projects_folder,
                        &projects,
                        req.project,
                        &info.name,
                        rf.id,
                        &rf.name,
                        multi,
                        rf.home_hint
                            .as_deref()
                            .and_then(|h| home::resolve(shared.home.as_deref(), h))
                            .as_deref(),
                    ) {
                        Ok(p) => p,
                        Err(reason) => {
                            ctx.leave_out(&rf.name, reason);
                            continue;
                        }
                    },
                };
                let mine = ctx.local_scan(dest.clone(), compose_for(remote.os)).await?;
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
            // Folders only this computer has: pull never visits them, so say so
            // rather than leave them out without a word.
            let theirs: Vec<FolderId> = info.folders.iter().map(|f| f.id).collect();
            for f in local.iter().flat_map(|p| &p.folders) {
                if !theirs.contains(&f.id) {
                    ctx.leave_out(
                        &f.name,
                        format!(
                            "{peer} doesn't have this folder yet. Sync details with {peer} \
                             to add it there, then pull again."
                        ),
                    );
                }
            }
        }
    }
    Ok(Preview {
        request: req,
        folders,
        left_out: ctx.left_out,
        warnings: ctx.warnings,
        link,
        description,
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
    left_out: Vec<LeftOut>,
}

/// What the other computer needs to scan a folder, or to say where it
/// would land there.
struct Ask<'a> {
    folder: FolderId,
    folder_name: &'a str,
    project_name: &'a str,
    multi: bool,
    hint: Option<String>,
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
    fn leave_out(&mut self, name: &str, reason: String) {
        self.left_out.push(LeftOut {
            name: name.to_string(),
            reason,
        });
    }

    async fn remote_scan(&mut self, ask: Ask<'_>) -> anyhow::Result<FolderScan> {
        let req = Request::Manifest {
            project: self.project,
            folder: ask.folder,
            ignore: self.spec.clone(),
            folder_name: ask.folder_name.into(),
            project_name: ask.project_name.into(),
            multi_folder: ask.multi,
            from_os: Some(Os::current()),
            home_hint: ask.hint,
        };
        match self.conn.request(&req).await? {
            Response::Manifest(s) => Ok(s),
            other => Err(unexpected(self.conn, "the folder scan", other)),
        }
    }

    async fn local_scan(&self, root: PathBuf, compose: bool) -> anyhow::Result<Manifest> {
        let spec = self.spec.clone();
        let shown = root.display().to_string();
        tokio::task::spawn_blocking(move || scan(&root, &Matcher::new(&spec)?, compose))
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
        info!(
            "preview of {}: {} ({} entries) against {} ({} entries): {} changes",
            s.name,
            s.src.1,
            src.entries.len(),
            s.dst.1,
            dst.entries.len(),
            plan.changes.len()
        );
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
