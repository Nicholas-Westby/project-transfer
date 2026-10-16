//! Requests about the folder a connection scanned: the hashes of files in
//! it, and the files in the ignored folders its scan only named (see
//! `transfer::ignored`).

use super::{Ctx, SessionState, refused, stored_root};
use crate::manifest::hash_file;
use crate::model::{FolderId, ProjectId};
use crate::protocol::Response;
use crate::transfer::{ignored, safe_join, validate_rel};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;
use tracing::info;

/// The folder `Hashes` and `CountFiles` read: this computer's own, or the
/// one this connection scanned for a folder not set up here.
async fn read_root(
    ctx: &Ctx<'_>,
    state: &SessionState,
    project: ProjectId,
    folder: FolderId,
) -> Option<PathBuf> {
    match stored_root(ctx.shared, project, folder).await {
        Some(r) => Some(r),
        None => state.roots.get(&(project, folder)).cloned(),
    }
}

pub async fn hashes(
    ctx: &Ctx<'_>,
    state: &SessionState,
    project: ProjectId,
    folder: FolderId,
    paths: Vec<String>,
) -> Response {
    let Some(root) = read_root(ctx, state, project, folder).await else {
        return Response::Hashes(HashMap::new());
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

pub async fn count_files(
    ctx: &Ctx<'_>,
    state: &SessionState,
    project: ProjectId,
    folder: FolderId,
    dirs: Vec<String>,
) -> Response {
    if let Some(reason) = dirs.iter().find_map(|d| validate_rel(d).err()) {
        return refused(reason);
    }
    // "This computer" would read as the asker, which shows the refusal.
    let me = ctx.shared.settings.read().await.name.clone();
    let Some(root) = read_root(ctx, state, project, folder).await else {
        return refused(format!(
            "{me} hasn't looked at that folder yet. Preview again."
        ));
    };
    let (started, asked) = (Instant::now(), dirs.len());
    let shown = root.display().to_string();
    match tokio::task::spawn_blocking(move || ignored::count(&root, &dirs)).await {
        Ok(Ok(counts)) => {
            if asked > 0 {
                info!(
                    "{} looked inside {} ignored folders of {} for a preview: {} files in {:.1} s",
                    ctx.peer.name,
                    asked,
                    shown,
                    counts.values().sum::<u64>(),
                    started.elapsed().as_secs_f64()
                );
            }
            Response::FileCounts(counts)
        }
        Ok(Err(e)) => refused(ignored::count_failed(&e, &me)),
        Err(e) => refused(format!("Counting the files in {shown} stopped: {e}")),
    }
}
