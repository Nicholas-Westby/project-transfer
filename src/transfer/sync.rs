//! Sync: one look at both computers decides whether to push or pull, and
//! the preview for that direction is built from what that look read.

use super::home::home_hint;
use super::link;
use super::prepare::{check_peer, prepare_with, unexpected};
use super::preview::{Preview, TransferRequest};
use super::scans::{Ask, Ctx, Scans};
use super::sync_count::{SyncChoice, Tally, tally_folder};
use crate::ignore_rules::{IgnoreSpec, Matcher};
use crate::manifest::{Kind, Manifest, compare, compose_for};
use crate::model::{Direction, FolderId, InstanceSettings, Project};
use crate::net::{Connection, Shared};
use crate::protocol::{ProjectSummary, RemoteProject, Request, Response};
use anyhow::{Context, anyhow, bail};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use tracing::info;

/// Looks at both computers' copies of the project once, picks the
/// direction with more newer changes, and builds that direction's preview
/// from the same scans. Returns the preview and why it went that way.
/// `req.direction` is ignored.
pub async fn prepare_sync(
    conn: &mut Connection,
    shared: &Shared,
    mut req: TransferRequest,
) -> anyhow::Result<(Preview, SyncChoice)> {
    check_peer(conn, &req)?;
    let theirs = both_allowed(conn).await?;
    // Scanned under the ids the preview uses, as `prepare` does.
    let mut projects = shared.projects.read().await.clone();
    let mut project = req.project;
    if let Some(l) = link::plan(conn, &projects, req.project, &theirs).await? {
        link::rekey(&mut projects, l.from, l.to, &l.folders).map_err(|e| anyhow!(e))?;
        project = l.to;
    }
    let info = match conn.request(&Request::ProjectInfo { project }).await? {
        Response::ProjectInfo(info) => info,
        other => return Err(unexpected(conn, "the project request", other)),
    };
    let settings = shared.settings.read().await.clone();
    let mut spec = IgnoreSpec::from_settings(&settings);
    spec.send_everything = req.send_everything;
    Matcher::new(&spec)?;
    let local = projects.iter().find(|p| p.id == project);
    // Both sides had every file the last transfer between them read, as long
    // as neither has moved the project with another computer since. A file
    // made after the read never travelled; older records only know when the
    // transfer ended.
    let since = local
        .and_then(|p| p.last_transfer.as_ref())
        .filter(|t| t.peer == req.peer && info.as_ref().is_some_and(|i| i.last_with_you))
        .map(|t| t.scanned_ms.unwrap_or(t.at_ms));
    let mut ctx = Ctx {
        conn,
        spec,
        project,
        warnings: Vec::new(),
        left_out: Vec::new(),
        scans: Scans::default(),
    };
    let mut tally = Tally::default();
    let look = Look {
        settings: &settings,
        home: shared.home.as_deref(),
        since,
        // "Send everything" changes what moves, not which way: what the
        // usual list leaves out never travelled, so it tells nothing.
        usual: if req.send_everything {
            Some(Matcher::new(&IgnoreSpec::from_settings(&settings))?)
        } else {
            None
        },
    };
    for f in folders(local, info.as_ref()) {
        look.folder(&mut ctx, &f, &mut tally).await?;
    }
    let Ctx { scans, .. } = ctx;
    let choice = SyncChoice::from_tally(tally);
    let verb = match choice.direction {
        Direction::Push => "push",
        Direction::Pull => "pull",
    };
    // So "why did it go that way?" can be answered from the log.
    info!(
        "sync with {}: {} files newer here ({} removed here), {} there ({} removed there), so \
         it will {verb}",
        conn.peer_name(),
        tally.here,
        tally.removed_here,
        tally.there,
        tally.removed_there
    );
    req.direction = choice.direction;
    let preview = prepare_with(conn, shared, req, scans)
        .await
        .with_context(|| format!("Sync chose to {verb}, and the {verb} can't start"))?;
    Ok((preview, choice))
}

/// Asks first, so a missing permission stops the sync with a clear reason.
/// Returns the other computer's projects.
async fn both_allowed(conn: &mut Connection) -> anyhow::Result<Vec<ProjectSummary>> {
    let name = conn.peer_name().to_string();
    let (allows, projects) = match conn.request(&Request::Status).await? {
        Response::Status {
            allows, projects, ..
        } => (allows, projects),
        other => return Err(unexpected(conn, "the status request", other)),
    };
    if !allows.may_push_to_me && !allows.may_pull_from_me {
        bail!(
            "{name} doesn't let this computer push to it or pull from it, which syncing needs. \
             Allow both for it on {name}, then try again."
        );
    }
    if !allows.may_push_to_me {
        bail!(
            "{name} doesn't let this computer push to it, which syncing needs. Allow pushing for \
             it on {name}, then try again."
        );
    }
    if !allows.may_pull_from_me {
        bail!(
            "{name} doesn't let this computer pull from it, which syncing needs. Allow pulling \
             for it on {name}, then try again."
        );
    }
    Ok(projects)
}

/// One folder of the project, as either computer has it.
struct Both {
    id: FolderId,
    name: String,
    project_name: String,
    multi: bool,
    /// Where it is on this computer, when it is set up and there.
    here: Option<PathBuf>,
    /// Whether the other computer's project has it.
    there: bool,
}

/// Every folder either computer's project has: this computer's first, in
/// its order, then the other computer's.
fn folders(local: Option<&Project>, info: Option<&RemoteProject>) -> Vec<Both> {
    let mut out: Vec<Both> = Vec::new();
    let in_info = |id: FolderId| info.is_some_and(|i| i.folders.iter().any(|f| f.id == id));
    if let Some(p) = local {
        for f in &p.folders {
            out.push(Both {
                id: f.id,
                name: f.name.clone(),
                project_name: p.name.clone(),
                multi: p.folders.len() > 1,
                here: f.local_path.clone().filter(|r| r.is_dir()),
                there: in_info(f.id),
            });
        }
    }
    if let Some(i) = info {
        for f in &i.folders {
            if !out.iter().any(|b| b.id == f.id) {
                out.push(Both {
                    id: f.id,
                    name: f.name.clone(),
                    project_name: i.name.clone(),
                    multi: i.folders.len() > 1,
                    here: None,
                    there: true,
                });
            }
        }
    }
    out
}

/// What counting a folder needs besides the folder.
struct Look<'a> {
    settings: &'a InstanceSettings,
    home: Option<&'a std::path::Path>,
    since: Option<i64>,
    /// The usual ignore list when "Send everything" is ticked.
    usual: Option<Matcher>,
}

impl Look<'_> {
    /// Scans one folder on both computers, adds its files to `tally`, and
    /// keeps the scans and hashes in `ctx` for the preview.
    async fn folder(&self, ctx: &mut Ctx<'_>, f: &Both, tally: &mut Tally) -> anyhow::Result<()> {
        if f.here.is_none() && !f.there {
            return Ok(());
        }
        let hint = f
            .here
            .as_deref()
            .zip(self.home)
            .and_then(|(root, home)| home_hint(root, &self.settings.projects_folder, home));
        let ask = Ask {
            folder: f.id,
            folder_name: &f.name,
            project_name: &f.project_name,
            multi: f.multi,
            hint,
        };
        let remote = ctx.remote_scan(ask).await?;
        // A folder there that isn't set up there is missing there; its
        // default place may hold something else.
        let there = (remote.set_up && remote.exists).then_some(&remote.manifest);
        let compose = compose_for(remote.os);
        let here = match &f.here {
            Some(root) => Some(ctx.local_scan(root.clone(), compose).await?),
            None => None,
        };
        let mut same = HashSet::new();
        if let (Some(root), Some(mine), Some(theirs)) = (&f.here, &here, there) {
            let needs = compare(mine, theirs).needs_hash;
            if !needs.is_empty() {
                let (local, remote_hashes) = ctx.hashes(f.id, root.clone(), &needs).await?;
                same = needs
                    .iter()
                    .filter(|r| {
                        local
                            .get(*r)
                            .is_some_and(|h| remote_hashes.get(*r) == Some(h))
                    })
                    .cloned()
                    .collect();
                ctx.scans.hashes.insert(f.id, (local, remote_hashes));
            }
        }
        match &self.usual {
            Some(u) => {
                let mine = here.as_ref().map(|m| kept(m, u));
                let theirs = there.map(|m| kept(m, u));
                tally_folder(tally, mine.as_ref(), theirs.as_ref(), &same, self.since);
            }
            None => tally_folder(tally, here.as_ref(), there, &same, self.since),
        }
        if let (Some(root), Some(m)) = (&f.here, here) {
            ctx.scans.local.insert((root.clone(), compose), m);
        }
        ctx.scans.remote.insert(f.id, remote);
        Ok(())
    }
}

/// `m` without what `usual` leaves out.
fn kept(m: &Manifest, usual: &Matcher) -> Manifest {
    let mut seen = HashMap::new();
    Manifest {
        entries: m
            .entries
            .iter()
            .filter(|e| !usual.leaves_out_with(&e.rel, e.kind == Kind::Dir, &mut seen))
            .cloned()
            .collect(),
        ignored_in_dir: Default::default(),
    }
}
