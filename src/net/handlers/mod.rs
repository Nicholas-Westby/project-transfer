//! Data requests (manifests, hashes, files, pushes, commands). Every call here
//! has already passed the permission gate in `gate`, and every path from the
//! network is validated before it touches the disk or the ignore rules.

use super::Shared;
use crate::ignore_rules::Matcher;
use crate::manifest::{compose_for, hash_file, scan};
use crate::model::{FolderId, Os, Peer, ProjectId};
use crate::protocol::{FolderScan, Request, Response, write_msg};
use crate::transfer::projects::{default_path, exchange_commands, wire_commands};
use crate::transfer::safe_join;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use tokio::io::{AsyncRead, AsyncWrite};
use tracing::info;

mod pull;
mod push;

pub struct Ctx<'a> {
    pub shared: &'a Shared,
    /// Re-read from the stored peers for this request.
    pub peer: &'a Peer,
    #[allow(dead_code)]
    pub remote: SocketAddr,
}

/// Per-connection state.
#[derive(Default)]
pub struct SessionState {
    /// Folders this connection scanned, so `Hashes` reads the same place
    /// even for a folder this computer has not set up yet.
    roots: HashMap<(ProjectId, FolderId), PathBuf>,
    push: Option<push::PushState>,
}

/// Writes its own reply, so a handler can follow a header with raw bytes.
pub async fn handle<S: AsyncRead + AsyncWrite + Unpin + Send>(
    ctx: &Ctx<'_>,
    state: &mut SessionState,
    stream: &mut S,
    req: Request,
) -> anyhow::Result<()> {
    let reply = match req {
        Request::Manifest {
            project,
            folder,
            ignore,
            folder_name,
            project_name,
            multi_folder,
            from_os,
        } => {
            let names = Names {
                project: &project_name,
                folder: &folder_name,
                multi: multi_folder,
            };
            manifest(ctx, state, project, folder, names, ignore, from_os).await
        }
        Request::Hashes {
            project,
            folder,
            paths,
        } => hashes(ctx, state, project, folder, paths).await,
        Request::ExchangeCommands { project, commands } => {
            match exchange_commands(ctx.shared, project, &wire_commands(&commands)).await {
                Ok(merged) => Response::Commands(merged),
                Err(e) => refused(format!("Could not save the commands: {e:#}")),
            }
        }
        Request::GetFile {
            project,
            folder,
            rel,
        } => return pull::get_file(ctx, stream, project, folder, &rel).await,
        Request::EndPull { project, files } => pull::end_pull(ctx, project, files).await,
        Request::PutFile {
            rel,
            size,
            mtime_ms,
            exec,
        } => {
            let file = push::Incoming {
                rel: &rel,
                size,
                mtime_ms,
                exec,
            };
            push::put_file(ctx, state, stream, file).await?
        }
        other => push::handle(ctx, state, other).await,
    };
    write_msg(stream, &reply).await
}

pub(super) fn refused(reason: impl Into<String>) -> Response {
    Response::Refused {
        reason: reason.into(),
    }
}

/// The stored path of a folder this computer has set up.
pub(super) async fn stored_root(
    shared: &Shared,
    project: ProjectId,
    folder: FolderId,
) -> Option<PathBuf> {
    shared
        .projects
        .read()
        .await
        .iter()
        .find(|p| p.id == project)?
        .folders
        .iter()
        .find(|f| f.id == folder)?
        .local_path
        .clone()
}

struct Names<'a> {
    project: &'a str,
    folder: &'a str,
    multi: bool,
}

async fn manifest(
    ctx: &Ctx<'_>,
    state: &mut SessionState,
    project: ProjectId,
    folder: FolderId,
    names: Names<'_>,
    ignore: crate::ignore_rules::IgnoreSpec,
    from_os: Option<Os>,
) -> Response {
    let (root, set_up) = match stored_root(ctx.shared, project, folder).await {
        Some(p) => (p, true),
        None => {
            let pf = ctx.shared.settings.read().await.projects_folder.clone();
            let projects = ctx.shared.projects.read().await;
            let (pn, fname) = (names.project, names.folder);
            match default_path(&pf, &projects, project, pn, folder, fname, names.multi) {
                Ok(p) => (p, false),
                Err(reason) => return refused(reason),
            }
        }
    };
    let matcher = match Matcher::new(&ignore) {
        Ok(m) => m,
        Err(e) => return refused(format!("The ignore list is not valid: {e:#}")),
    };
    let scan_root = root.clone();
    let compose = from_os.is_some_and(compose_for);
    let scanned = tokio::task::spawn_blocking(move || scan(&scan_root, &matcher, compose)).await;
    let manifest = match scanned {
        Ok(Ok(m)) => m,
        Ok(Err(e)) => return refused(format!("Could not read {}: {e:#}", root.display())),
        Err(e) => return refused(format!("Scanning {} stopped: {e}", root.display())),
    };
    // A preview the other computer shows is built from this; say what it saw.
    info!(
        "{} looked at {} for a preview: {} entries{}",
        ctx.peer.name,
        root.display(),
        manifest.entries.len(),
        if set_up { "" } else { ", not set up here yet" }
    );
    state.roots.insert((project, folder), root.clone());
    Response::Manifest(FolderScan {
        manifest,
        path: root.display().to_string(),
        exists: root.is_dir(),
        set_up,
        os: Os::current(),
    })
}

async fn hashes(
    ctx: &Ctx<'_>,
    state: &SessionState,
    project: ProjectId,
    folder: FolderId,
    paths: Vec<String>,
) -> Response {
    let root = match stored_root(ctx.shared, project, folder).await {
        Some(r) => r,
        None => match state.roots.get(&(project, folder)) {
            Some(r) => r.clone(),
            None => return Response::Hashes(HashMap::new()),
        },
    };
    let done = tokio::task::spawn_blocking(move || {
        // A path that is not allowed or not a plain file gets no hash, which
        // keeps it a full update.
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
    .await;
    Response::Hashes(done.unwrap_or_default())
}
