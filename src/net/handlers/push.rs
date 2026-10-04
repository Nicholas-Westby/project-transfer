//! The receiving side of a push.

use super::{Ctx, SessionState, refused};
use crate::model::{Direction, Project, ProjectId};
use crate::net::NetEvent;
use crate::protocol::{Request, Response};
use crate::transfer::projects::{StartedBy, adopt, record_transfer, update_projects};
use crate::transfer::{Applier, validate_name, validate_rel};
use std::collections::HashMap;
use tokio::io::{AsyncRead, AsyncReadExt};
use tracing::{info, warn};

const CHUNK: usize = 256 * 1024;
const NO_PUSH: &str = "No push is in progress on this connection. Start the push again.";

pub struct PushState {
    project: ProjectId,
    applier: Applier,
    /// Files written in this push, across its folders.
    files: u64,
}

pub struct Incoming<'a> {
    pub rel: &'a str,
    pub size: u64,
    pub mtime_ms: i64,
    pub exec: bool,
}

pub async fn handle(ctx: &Ctx<'_>, state: &mut SessionState, req: Request) -> Response {
    match req {
        Request::BeginPush {
            project,
            folder,
            expected_path,
        } => begin(ctx, state, project, folder, &expected_path).await,
        Request::EndPush => end(ctx, state).await,
        Request::MakeDir { rel } => apply(state, &rel, |a| a.make_dir(&rel)),
        Request::MakeSymlink { rel, target } => {
            apply(state, &rel, |a| a.make_symlink(&rel, &target))
        }
        Request::SetMtime { rel, mtime_ms } => apply(state, &rel, |a| a.set_mtime(&rel, mtime_ms)),
        Request::Remove { rel, is_dir } => apply(state, &rel, |a| a.remove(&rel, is_dir)),
        other => refused(format!("Unexpected request {other:?}.")),
    }
}

fn apply(
    state: &mut SessionState,
    rel: &str,
    op: impl FnOnce(&Applier) -> std::io::Result<()>,
) -> Response {
    if let Err(reason) = validate_rel(rel) {
        return refused(reason);
    }
    let Some(push) = &state.push else {
        return refused(NO_PUSH);
    };
    match op(&push.applier) {
        Ok(()) => Response::Ok,
        Err(e) => refused(format!("\"{rel}\": {e}")),
    }
}

async fn begin(
    ctx: &Ctx<'_>,
    state: &mut SessionState,
    project: Project,
    folder: crate::model::FolderId,
    expected_path: &str,
) -> Response {
    let names = std::iter::once(project.name.as_str())
        .chain(project.folders.iter().map(|f| f.name.as_str()));
    for name in names {
        if let Err(reason) = validate_name(name) {
            return refused(reason);
        }
    }
    if !project.folders.iter().any(|f| f.id == folder) {
        return refused("The pushed folder is not part of the pushed project.");
    }
    let (pf, me) = {
        let s = ctx.shared.settings.read().await;
        (s.projects_folder.clone(), s.name.clone())
    };
    let adopted = update_projects(ctx.shared, |all| {
        adopt(all, &project, &pf, &HashMap::new())?;
        let p = all
            .iter()
            .find(|p| p.id == project.id)
            .expect("just adopted");
        let f = p
            .folders
            .iter()
            .find(|f| f.id == folder)
            .expect("just adopted");
        let root = f
            .local_path
            .clone()
            .expect("adopt gives every folder a path");
        // Returning an error here saves nothing, so a refused push leaves
        // this computer's projects as they were.
        if root.display().to_string() != expected_path {
            return Err(moved(&me));
        }
        Ok(root)
    })
    .await;
    let root = match adopted {
        Ok(r) => r,
        Err(e) if e.to_string() == moved(&me) => return refused(moved(&me)),
        Err(e) => return refused(format!("Could not set up the project here: {e:#}")),
    };
    if let Err(e) = std::fs::create_dir_all(&root) {
        return refused(format!("Could not create {}: {e}", root.display()));
    }
    let applier = Applier::new(root.clone());
    let swept = {
        let sweeper = Applier::new(root.clone());
        tokio::task::spawn_blocking(move || sweeper.sweep_temp()).await
    };
    match swept {
        Ok(Ok(0)) => {}
        Ok(Ok(n)) => info!("removed {n} unfinished files left in {}", root.display()),
        Ok(Err(e)) => warn!(
            "could not clear unfinished files in {}: {e}",
            root.display()
        ),
        Err(e) => warn!(
            "clearing unfinished files in {} stopped: {e}",
            root.display()
        ),
    }
    let files = match &state.push {
        Some(p) if p.project == project.id => p.files,
        _ => 0,
    };
    info!("{} began pushing to {}", ctx.peer.name, root.display());
    state.push = Some(PushState {
        project: project.id,
        applier,
        files,
    });
    Response::Ok
}

fn moved(me: &str) -> String {
    format!("The folder on {me} changed since the preview. Preview again.")
}

async fn end(ctx: &Ctx<'_>, state: &mut SessionState) -> Response {
    let Some(push) = state.push.take() else {
        return refused(NO_PUSH);
    };
    let peer = ctx.peer.id;
    let (project, files) = (push.project, push.files);
    if let Err(e) = record_transfer(
        ctx.shared,
        project,
        peer,
        Direction::Push,
        files,
        StartedBy::Peer,
    )
    .await
    {
        warn!("could not record the push: {e:#}");
    }
    info!("{} pushed {} files", ctx.peer.name, push.files);
    let _ = ctx.shared.events.send(NetEvent::Received {
        project: push.project,
        peer,
        files: push.files,
    });
    Response::Ok
}

/// Always reads all `size` bytes so the connection stays in step, even when
/// the file is refused. A connection that ends early drops the temp file.
pub async fn put_file<S: AsyncRead + Unpin>(
    state: &mut SessionState,
    stream: &mut S,
    file: Incoming<'_>,
) -> anyhow::Result<Response> {
    let mut problem: Option<String> = validate_rel(file.rel).err();
    let mut pending = None;
    if problem.is_none() {
        match &state.push {
            None => problem = Some(NO_PUSH.into()),
            Some(p) => match p.applier.begin_file(file.rel) {
                Ok(f) => pending = Some(f),
                Err(e) => problem = Some(format!("\"{}\": {e}", file.rel)),
            },
        }
    }
    let mut buf = vec![0u8; CHUNK];
    let mut left = file.size;
    while left > 0 {
        let n = left.min(CHUNK as u64) as usize;
        stream.read_exact(&mut buf[..n]).await?;
        left -= n as u64;
        if let Some(p) = &mut pending
            && let Err(e) = p.write(&buf[..n])
        {
            problem = Some(format!("\"{}\": {e}", file.rel));
            pending = None;
        }
    }
    if let Some(p) = pending {
        match p.finish(file.mtime_ms, file.exec) {
            Ok(()) => {
                if let Some(push) = &mut state.push {
                    push.files += 1;
                }
                return Ok(Response::Ok);
            }
            Err(e) => problem = Some(format!("\"{}\": {e}", file.rel)),
        }
    }
    Ok(refused(problem.unwrap_or_else(|| NO_PUSH.into())))
}
