//! Applying a confirmed preview: folders first, then files, times, removals.

use super::prepare::{check_peer, local_project, unexpected};
use super::preview::{FolderPreview, Preview};
use super::projects::{
    StartedBy, adopt, exchange_commands, record_transfer, update_projects, wire_commands,
    wire_project,
};
use super::{Applier, Progress, Summary};
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
mod stream;

use super::Op;

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
    };
    match run.all(shared, &preview).await {
        Ok(()) => {
            let mut s = run.summary;
            s.took_ms = started.elapsed().as_millis() as u64;
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
}

/// Where the current folder's files come from and go to.
enum Side {
    /// Read from here, write on the peer.
    Push(PathBuf),
    /// Read from the peer, write here.
    Pull(Applier, crate::model::ProjectId, crate::model::FolderId),
}

fn is_dir_change(c: &Change) -> bool {
    matches!(c, Change::Add(e) | Change::Replace { entry: e } if e.kind == Kind::Dir)
}

fn content(c: &Change) -> Option<&Entry> {
    match c {
        Change::Add(e) | Change::Update { entry: e, .. } | Change::Replace { entry: e }
            if e.kind != Kind::Dir =>
        {
            Some(e)
        }
        _ => None,
    }
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
        for fp in &preview.folders {
            let side = match req.direction {
                Direction::Push => {
                    let begin = Request::BeginPush {
                        project: wire_project(&project),
                        folder: fp.folder,
                        expected_path: fp.dest_path.clone(),
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
        update_projects(shared, |all| adopt(all, &incoming, &pf, &chosen)).await?;
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
            self.simple(side, rel, Op::MakeDir).await?;
        }
        for e in changes.iter().filter_map(content) {
            self.check_cancel()?;
            match &e.kind {
                Kind::Symlink { target } => {
                    if self.simple(side, &e.rel, Op::Symlink(target)).await? {
                        self.summary.files += 1;
                    }
                }
                _ => self.file(side, &e.rel).await?,
            }
        }
        for c in changes {
            if let Change::TimestampOnly(Entry {
                rel,
                kind: Kind::File { mtime_ms, .. },
            }) = c
                && self.simple(side, rel, Op::SetMtime(*mtime_ms)).await?
            {
                self.summary.files += 1;
            }
        }
        for c in changes {
            let (rel, is_dir) = match c {
                Change::RemoveFile(r) => (r, false),
                Change::RemoveDir { rel, .. } => (rel, true),
                _ => continue,
            };
            self.check_cancel()?;
            if self.simple(side, rel, Op::Remove(is_dir)).await? {
                self.summary.removed += 1;
            }
        }
        Ok(())
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
        match self.direction {
            Direction::Push => warn!("could not push \"{rel}\" from {here} to {peer}: {reason}"),
            Direction::Pull => warn!("could not pull \"{rel}\" from {peer} to {here}: {reason}"),
        }
        self.summary.failures.push((rel.to_string(), reason));
    }
}

fn written_rel(c: &Change) -> &str {
    match c {
        Change::Add(e) | Change::TimestampOnly(e) => &e.rel,
        Change::Update { entry, .. } | Change::Replace { entry } => &entry.rel,
        Change::RemoveFile(r) | Change::RemoveDir { rel: r, .. } => r,
    }
}

impl Preview {
    /// Files and links the transfer writes (plus times it fixes), and the
    /// bytes of content it copies; what progress counts towards.
    pub fn totals(&self) -> (u64, u64) {
        totals(self)
    }
}

/// Files and links to write (plus times to fix), and the bytes to copy.
fn totals(p: &Preview) -> (u64, u64) {
    let mut files = 0;
    let mut bytes = 0;
    for c in p.folders.iter().flat_map(|f| &f.plan.changes) {
        if let Some(e) = content(c) {
            files += 1;
            if let Kind::File { size, .. } = e.kind {
                bytes += size;
            }
        } else if matches!(c, Change::TimestampOnly(_)) {
            files += 1;
        }
    }
    (files, bytes)
}
