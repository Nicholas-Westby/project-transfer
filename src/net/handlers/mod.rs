//! Data requests (manifests, hashes, files, pushes, commands). Every call here
//! has already passed the permission gate in `gate`, and every path from the
//! network is validated before it touches the disk or the ignore rules.

use super::Shared;
use crate::ignore_rules::Matcher;
use crate::manifest::compose_for;
use crate::model::{FolderId, Os, Peer, ProjectId};
use crate::protocol::{FolderScan, Request, Response, write_msg};
use crate::transfer::commands::{exchange_commands, wire_commands};
use crate::transfer::home::resolve;
use crate::transfer::ignored;
use crate::transfer::projects::default_path;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Instant;
use tokio::io::{AsyncRead, AsyncWrite};
use tracing::info;

mod details;
mod peek;
mod pull;
mod push;
mod scanned;

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
    /// Temp files those scans found an unfinished transfer left, which a
    /// push into the folder clears without walking it again. A push on a
    /// connection that never scanned the folder clears none.
    leftovers: HashMap<(ProjectId, FolderId), Vec<PathBuf>>,
    /// What the asking computer runs, as it said when it asked for a folder
    /// scan; older versions don't say.
    peer_os: Option<Os>,
    /// When this computer first scanned a folder for this connection.
    scanned_ms: Option<i64>,
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
            home_hint,
            name_ignored_dirs,
        } => {
            let names = Names {
                project: &project_name,
                folder: &folder_name,
                multi: multi_folder,
                hint: home_hint.as_deref(),
            };
            let listing = Listing {
                ignore,
                from_os,
                name_ignored_dirs,
            };
            manifest(ctx, state, project, folder, names, listing).await
        }
        Request::Hashes {
            project,
            folder,
            paths,
        } => scanned::hashes(ctx, state, project, folder, paths).await,
        Request::CountFiles {
            project,
            folder,
            dirs,
        } => scanned::count_files(ctx, state, project, folder, dirs).await,
        Request::ExchangeCommands {
            project,
            commands,
            shared,
        } => {
            let theirs = wire_commands(&commands);
            match exchange_commands(ctx.shared, project, &theirs, shared.as_deref()).await {
                // An asker that sent none can't read them back.
                Ok((commands, library)) if shared.is_some() => Response::CommandsWithShared {
                    commands,
                    shared: library,
                },
                Ok((commands, _)) => Response::Commands(commands),
                Err(e) => refused(format!("Could not save the commands: {e:#}")),
            }
        }
        Request::GetFile {
            project,
            folder,
            rel,
        } => return pull::get_file(ctx, stream, project, folder, &rel).await,
        Request::ReadFile {
            project,
            folder,
            rel,
            max,
        } => return peek::read_file(ctx, stream, project, folder, &rel, max).await,
        Request::EndPull { project, files } => {
            pull::end_pull(ctx, project, files, state.scanned_ms).await
        }
        Request::SyncProject {
            project,
            home_hints,
        } => details::sync(ctx, project, home_hints).await,
        Request::SetFolderPath {
            project,
            folder,
            path,
        } => details::set_path(ctx, project, folder, &path).await,
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
    hint: Option<&'a str>,
}

/// How the asking computer wants its folder listed.
struct Listing {
    ignore: crate::ignore_rules::IgnoreSpec,
    from_os: Option<Os>,
    name_ignored_dirs: bool,
}

async fn manifest(
    ctx: &Ctx<'_>,
    state: &mut SessionState,
    project: ProjectId,
    folder: FolderId,
    names: Names<'_>,
    listing: Listing,
) -> Response {
    let started = Instant::now();
    // Before the scan: a file made after it never travels with this transfer.
    state
        .scanned_ms
        .get_or_insert_with(crate::transfer::projects::now_ms);
    let (root, set_up) = match stored_root(ctx.shared, project, folder).await {
        Some(p) => (p, true),
        None => {
            let pf = ctx.shared.settings.read().await.projects_folder.clone();
            let projects = ctx.shared.projects.read().await;
            let (pn, fname) = (names.project, names.folder);
            let hint = names
                .hint
                .and_then(|h| resolve(ctx.shared.home.as_deref(), h));
            let hint = hint.as_deref();
            match default_path(
                &pf,
                &projects,
                project,
                pn,
                folder,
                fname,
                names.multi,
                hint,
            ) {
                Ok(p) => (p, false),
                Err(reason) => return refused(reason),
            }
        }
    };
    let matcher = match Matcher::new(&listing.ignore) {
        Ok(m) => m,
        Err(e) => return refused(format!("The ignore list is not valid: {e:#}")),
    };
    let scan_root = root.clone();
    let compose = listing.from_os.is_some_and(compose_for);
    let named = listing.name_ignored_dirs;
    let scanned = tokio::task::spawn_blocking(move || {
        let mut m = ignored::scan(&scan_root, &matcher, compose)?;
        // An older version's preview counts on every ignored file in these.
        if !named {
            ignored::count_all(&scan_root, &mut m)?;
        }
        anyhow::Ok(m)
    })
    .await;
    let mut manifest = match scanned {
        Ok(Ok(m)) => m,
        Ok(Err(e)) => return refused(format!("Could not read {}: {e:#}", root.display())),
        Err(e) => return refused(format!("Scanning {} stopped: {e}", root.display())),
    };
    // A preview the other computer shows is built from this; say what it saw
    // and how long that took.
    info!(
        "{} looked at {} for a preview: {} entries in {:.1} s{}",
        ctx.peer.name,
        root.display(),
        manifest.entries.len(),
        started.elapsed().as_secs_f64(),
        if set_up { "" } else { ", not set up here yet" }
    );
    state.roots.insert((project, folder), root.clone());
    let leftovers = std::mem::take(&mut manifest.leftovers);
    state.leftovers.insert((project, folder), leftovers);
    state.peer_os = listing.from_os;
    Response::Manifest(FolderScan {
        manifest,
        path: root.display().to_string(),
        exists: root.is_dir(),
        set_up,
        os: Os::current(),
    })
}
