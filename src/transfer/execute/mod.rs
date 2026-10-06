//! Applying a confirmed preview: folders first, then files, times, removals.

use super::home::{hints_for, resolve};
use super::prepare::{check_peer, local_project, unexpected};
use super::preview::{FolderPreview, Preview};
use super::projects::{
    StartedBy, adopt, exchange_commands, record_transfer, update_projects, wire_commands,
    wire_project,
};
use super::{Applier, Progress, Summary};
use crate::logging::one_line;
use crate::manifest::{Change, Entry, Kind};
use crate::model::{Direction, Folder, Project};
use crate::net::{Connection, Shared};
use crate::protocol::{Request, Response};
use anyhow::{Context, bail};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;
use tokio::sync::mpsc::UnboundedSender;
use tokio_util::sync::CancellationToken;
use tracing::warn;

mod ops;
mod plan;
mod report;
mod stream;
mod window;

use super::Op;
use plan::{content, is_dir_change, totals, written_rel};
use report::log_done;
use window::{Awaiting, Counts};

const CANCELLED: &str = "The transfer was cancelled. Files already copied stay in place.";

pub async fn execute(
    conn: &mut Connection,
    shared: &Shared,
    preview: Preview,
    progress: UnboundedSender<Progress>,
    cancel: CancellationToken,
) -> anyhow::Result<Summary> {
    let started = Instant::now();
    let mut run = Run {
        conn,
        progress: &progress,
        cancel: &cancel,
        summary: Summary::default(),
        bytes_done: 0,
        last_progress: Instant::now(),
        direction: preview.request.direction,
        here: String::new(),
        in_flight: Default::default(),
    };
    let result = run.all(shared, &preview).await;
    let mut s = run.summary;
    s.took_ms = started.elapsed().as_millis() as u64;
    log_done(run.direction, run.conn.peer_name(), &s, result.is_err());
    match result {
        Ok(()) => {
            let _ = progress.send(Progress::Done(s.clone()));
            Ok(s)
        }
        Err(e) => {
            let _ = progress.send(Progress::Failed(format!("{e:#}")));
            Err(e)
        }
    }
}

struct Run<'a> {
    conn: &'a mut Connection,
    progress: &'a UnboundedSender<Progress>,
    cancel: &'a CancellationToken,
    summary: Summary,
    bytes_done: u64,
    last_progress: Instant,
    direction: Direction,
    /// The current folder as it is on this computer, for the log.
    here: String,
    /// Requests sent whose answers have not been read, oldest first.
    in_flight: std::collections::VecDeque<Awaiting>,
}

/// Where the current folder's files come from and go to.
enum Side {
    /// Read from here, write on the peer.
    Push(PathBuf),
    /// Read from the peer, write here.
    Pull(Applier, crate::model::ProjectId, crate::model::FolderId),
}

impl Run<'_> {
    async fn all(&mut self, shared: &Shared, preview: &Preview) -> anyhow::Result<()> {
        let req = &preview.request;
        check_peer(self.conn, req)?;
        if let Some(link) = &preview.link {
            link.apply(shared).await?;
        }
        let (files, bytes) = totals(preview);
        let _ = self.progress.send(Progress::Started {
            total_files: files,
            total_bytes: bytes,
        });
        self.check_cancel()?;
        let project = match req.direction {
            Direction::Push => local_project(shared, req.project)
                .await
                .context("This project is no longer on this computer.")?,
            Direction::Pull => self.take_in_project(shared, preview).await?,
        };
        let pf = shared.settings.read().await.projects_folder.clone();
        let hints = hints_for(&project, &pf, shared.home.as_deref());
        for fp in &preview.folders {
            let side = match req.direction {
                Direction::Push => {
                    let begin = Request::BeginPush {
                        project: wire_project(&project),
                        folder: fp.folder,
                        expected_path: fp.dest_path.clone(),
                        home_hints: hints.clone(),
                    };
                    self.expect_ok(&begin, "the start of the push").await?;
                    Side::Push(PathBuf::from(&fp.source_path))
                }
                Direction::Pull => {
                    let root = PathBuf::from(&fp.dest_path);
                    std::fs::create_dir_all(&root)
                        .with_context(|| format!("Could not create {}", root.display()))?;
                    let sweeper = Applier::new(root.clone());
                    // Leftovers are only clutter, never sent, so a failed
                    // sweep must not stop the transfer.
                    match tokio::task::spawn_blocking(move || sweeper.sweep_temp()).await {
                        Ok(Ok(_)) => {}
                        Ok(Err(e)) => warn!(
                            "could not clear unfinished files in {}: {e}",
                            root.display()
                        ),
                        Err(e) => warn!(
                            "clearing unfinished files in {} stopped: {e}",
                            root.display()
                        ),
                    }
                    Side::Pull(Applier::new(root), req.project, fp.folder)
                }
            };
            self.folder(&side, fp).await?;
        }
        let mine = local_project(shared, req.project).await.map(|p| p.commands);
        let ex = Request::ExchangeCommands {
            project: req.project,
            commands: wire_commands(&mine.unwrap_or_default()),
        };
        match self.conn.request(&ex).await? {
            Response::Commands(theirs) => {
                exchange_commands(shared, req.project, &theirs).await?;
            }
            other => return Err(unexpected(self.conn, "the command exchange", other)),
        }
        let end = match req.direction {
            Direction::Push => Request::EndPush,
            Direction::Pull => Request::EndPull {
                project: req.project,
                files: self.summary.files,
            },
        };
        self.expect_ok(&end, "the end of the transfer").await?;
        record_transfer(
            shared,
            req.project,
            req.peer,
            req.direction,
            self.summary.files,
            StartedBy::Here,
        )
        .await
    }

    /// Records the pulled project here before writing, keeping existing paths
    /// and giving new folders the paths the preview showed.
    async fn take_in_project(
        &mut self,
        shared: &Shared,
        preview: &Preview,
    ) -> anyhow::Result<Project> {
        let id = preview.request.project;
        let info = match self
            .conn
            .request(&Request::ProjectInfo { project: id })
            .await?
        {
            Response::ProjectInfo(Some(i)) => i,
            other => return Err(unexpected(self.conn, "the project request", other)),
        };
        let hints: HashMap<_, _> = info
            .folders
            .iter()
            .filter_map(|f| {
                Some((
                    f.id,
                    resolve(shared.home.as_deref(), f.home_hint.as_deref()?)?,
                ))
            })
            .collect();
        let incoming = Project {
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
        };
        let chosen: HashMap<_, _> = preview
            .folders
            .iter()
            .map(|f| (f.folder, PathBuf::from(&f.dest_path)))
            .collect();
        let pf = shared.settings.read().await.projects_folder.clone();
        update_projects(shared, |all| adopt(all, &incoming, &pf, &chosen, &hints)).await?;
        local_project(shared, id)
            .await
            .context("The project could not be recorded.")
    }

    async fn folder(&mut self, side: &Side, fp: &FolderPreview) -> anyhow::Result<()> {
        self.here = match side {
            Side::Push(root) => root.display().to_string(),
            Side::Pull(a, ..) => a.root().display().to_string(),
        };
        let changes = &fp.plan.changes;
        for c in changes.iter().filter(|c| is_dir_change(c)) {
            let rel = written_rel(c);
            self.simple(side, rel, Op::MakeDir, Counts::Nothing).await?;
        }
        for e in changes.iter().filter_map(content) {
            self.check_cancel()?;
            match &e.kind {
                Kind::Symlink { target } => {
                    let op = Op::Symlink(target);
                    self.simple(side, &e.rel, op, Counts::File(0)).await?
                }
                _ => self.file(side, &e.rel).await?,
            }
        }
        // Content lands before times and removals, as it always has.
        self.settle_all(side).await?;
        for c in changes {
            if let Change::TimestampOnly(Entry {
                rel,
                kind: Kind::File { mtime_ms, .. },
            }) = c
            {
                let op = Op::SetMtime(*mtime_ms);
                self.simple(side, rel, op, Counts::File(0)).await?;
            }
        }
        for c in changes {
            let (rel, is_dir) = match c {
                Change::RemoveFile(r) => (r, false),
                Change::RemoveDir { rel, .. } => (rel, true),
                _ => continue,
            };
            self.check_cancel()?;
            let op = Op::Remove(is_dir);
            self.simple(side, rel, op, Counts::Removal).await?;
        }
        // The next request needs its own answer, which comes after these.
        self.settle_all(side).await
    }

    fn check_cancel(&self) -> anyhow::Result<()> {
        if self.cancel.is_cancelled() {
            bail!(CANCELLED);
        }
        Ok(())
    }

    /// The summary that lists failures closes for good, so each one is
    /// logged here as well.
    fn fail(&mut self, rel: &str, reason: String) {
        let (here, peer) = (&self.here, self.conn.peer_name());
        // Escaped, so no file name can break the line.
        let why = one_line(&reason);
        match self.direction {
            Direction::Push => warn!("could not push {rel:?} from {here} to {peer}: {why}"),
            Direction::Pull => warn!("could not pull {rel:?} from {peer} to {here}: {why}"),
        }
        self.summary.failures.push((rel.to_string(), reason));
    }
}
