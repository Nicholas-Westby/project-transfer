//! What building a preview reads from both computers: folder scans and
//! the hashes of files whose times differ. A sync reads them first, so its
//! preview takes them from here instead of reading them again.

use super::prepare::unexpected;
use super::preview::{FolderPreview, LeftOut, Replaced, drop_unholdable, replaced_folders};
use super::safe_join;
use crate::ignore_rules::{IgnoreSpec, Matcher};
use crate::manifest::{Kind, Manifest, compare, compose_for, hash_file, resolve_hashes, scan};
use crate::model::{FolderId, Os, ProjectId};
use crate::net::Connection;
use crate::protocol::{FolderScan, Request, Response};
use anyhow::Context;
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::info;

/// One folder's hashes of the files whose sizes match but whose times
/// differ, by path: (this computer's, the other computer's).
pub(super) type Hashes = (HashMap<String, String>, HashMap<String, String>);

/// Scans and hashes already read over this connection, each used once in
/// place of reading it again.
#[derive(Default)]
pub(super) struct Scans {
    pub(super) remote: HashMap<FolderId, FolderScan>,
    /// By the folder here and whether its names were composed.
    pub(super) local: HashMap<(PathBuf, bool), Manifest>,
    pub(super) hashes: HashMap<FolderId, Hashes>,
}

pub(super) struct Ctx<'a> {
    pub(super) conn: &'a mut Connection,
    pub(super) spec: IgnoreSpec,
    pub(super) project: ProjectId,
    pub(super) warnings: Vec<String>,
    pub(super) left_out: Vec<LeftOut>,
    pub(super) scans: Scans,
}

/// What the other computer needs to scan a folder, or to say where it
/// would land there.
pub(super) struct Ask<'a> {
    pub(super) folder: FolderId,
    pub(super) folder_name: &'a str,
    pub(super) project_name: &'a str,
    pub(super) multi: bool,
    pub(super) hint: Option<String>,
}

/// One folder's two sides: (manifest, display path) each.
pub(super) struct Sides {
    pub(super) folder: FolderId,
    pub(super) name: String,
    pub(super) src: (Manifest, String),
    pub(super) dst: (Manifest, String),
    /// This computer's side, which is hashed here.
    pub(super) local_root: PathBuf,
    pub(super) local_is_src: bool,
    pub(super) dest_exists: bool,
    pub(super) dest_os: Os,
}

impl Ctx<'_> {
    pub(super) fn leave_out(&mut self, name: &str, reason: String) {
        self.left_out.push(LeftOut {
            name: name.to_string(),
            reason,
        });
    }

    pub(super) async fn remote_scan(&mut self, ask: Ask<'_>) -> anyhow::Result<FolderScan> {
        if let Some(scan) = self.scans.remote.remove(&ask.folder) {
            return Ok(scan);
        }
        self.conn.mark_scanned();
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
            Response::Manifest(mut s) => {
                exec_between(&mut s.manifest, Os::current(), s.os);
                Ok(s)
            }
            other => Err(unexpected(self.conn, "the folder scan", other)),
        }
    }

    /// This computer's copy of a folder, to compare with the other
    /// computer's, which runs `other`.
    pub(super) async fn local_scan(
        &mut self,
        root: PathBuf,
        other: Os,
    ) -> anyhow::Result<Manifest> {
        let compose = compose_for(other);
        if let Some(m) = self.scans.local.remove(&(root.clone(), compose)) {
            return Ok(m);
        }
        let spec = self.spec.clone();
        let shown = root.display().to_string();
        let mut m =
            tokio::task::spawn_blocking(move || scan(&root, &Matcher::new(&spec)?, compose))
                .await?
                .with_context(|| format!("Could not read {shown}"))?;
        exec_between(&mut m, Os::current(), other);
        Ok(m)
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

    /// Both computers' hashes of `paths` in one folder, whose copy here is
    /// at `local_root`.
    pub(super) async fn hashes(
        &mut self,
        folder: FolderId,
        local_root: PathBuf,
        paths: &[String],
    ) -> anyhow::Result<Hashes> {
        if let Some(h) = self.scans.hashes.remove(&folder) {
            return Ok(h);
        }
        let remote = self.remote_hashes(folder, paths).await?;
        let local = local_hashes(local_root, paths.to_vec()).await;
        Ok((local, remote))
    }

    pub(super) async fn plan(&mut self, s: Sides) -> anyhow::Result<FolderPreview> {
        let (src, dst) = (&s.src.0, &s.dst.0);
        let mut plan = compare(src, dst);
        if !plan.needs_hash.is_empty() {
            let (local, remote) = self
                .hashes(s.folder, s.local_root.clone(), &plan.needs_hash)
                .await?;
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

/// Windows keeps no executable bit, so its scans say no file runs. Next to
/// one, every script on a Mac would look changed on every transfer and
/// count for Sync, so when either computer runs Windows `m` loses the bit.
fn exec_between(m: &mut Manifest, a: Os, b: Os) {
    if a == Os::Windows || b == Os::Windows {
        for e in &mut m.entries {
            if let Kind::File { exec, .. } = &mut e.kind {
                *exec = false;
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::Entry;

    fn script(exec: bool) -> Manifest {
        let kind = Kind::File {
            size: 4,
            mtime_ms: 7,
            exec,
        };
        Manifest {
            entries: vec![Entry {
                rel: "install".into(),
                kind,
            }],
            ignored_in_dir: Default::default(),
        }
    }

    #[test]
    fn a_script_is_unchanged_next_to_a_copy_on_windows() {
        let (mut mac, mut pc) = (script(true), script(false));
        exec_between(&mut mac, Os::MacOs, Os::MacOs);
        assert_eq!(compare(&mac, &pc).changes.len(), 1, "two Macs keep the bit");
        for m in [&mut mac, &mut pc] {
            exec_between(m, Os::MacOs, Os::Windows);
        }
        assert!(compare(&mac, &pc).changes.is_empty());
    }
}
