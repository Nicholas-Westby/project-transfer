//! What a transfer will do, and the pure checks that shape it.

use crate::manifest::{Change, Entry, Kind, Manifest, Plan};
use crate::model::{Direction, FolderId, InstanceId, Os, ProjectId};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub struct TransferRequest {
    pub peer: InstanceId,
    pub project: ProjectId,
    pub direction: Direction,
    pub send_everything: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Preview {
    pub request: TransferRequest,
    pub folders: Vec<FolderPreview>,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FolderPreview {
    pub folder: FolderId,
    pub name: String,
    pub source_path: String,
    pub dest_path: String,
    pub dest_will_be_created: bool,
    pub plan: Plan,
    /// (rel, reason) for names the destination cannot hold.
    pub skipped: Vec<(String, String)>,
    /// Destination entries that writing the source removes without a
    /// removal line of their own, such as a folder replaced by a file.
    pub replaced: Vec<Replaced>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Replaced {
    pub rel: String,
    /// Files removed, ignored ones included.
    pub files: u64,
    pub ignored: u64,
    /// What takes its place: "a file", "a link" or "a folder".
    pub by: String,
    /// False when a single file is replaced by a folder.
    pub was_folder: bool,
}

impl Replaced {
    pub fn warning(&self) -> String {
        if !self.was_folder {
            return format!("Replacing file `{}` with {}.", self.rel, self.by);
        }
        let files = match self.files {
            1 => "1 file".to_string(),
            n => format!("{n} files"),
        };
        let ignored = match self.ignored {
            0 => String::new(),
            n => format!(" ({n} of them ignored)"),
        };
        format!(
            "Replacing folder `{}` with {} removes {files} inside it{ignored}.",
            self.rel, self.by
        )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub added: u64,
    pub changed: u64,
    pub timestamp_only: u64,
    /// Files that will be deleted, including those inside removed folders.
    pub removed_files: u64,
    pub dest_newer: u64,
}

impl Preview {
    pub fn counts(&self) -> Counts {
        let mut c = Counts::default();
        for change in self.folders.iter().flat_map(|f| &f.plan.changes) {
            match change {
                Change::Add(_) => c.added += 1,
                Change::Update { dest_newer, .. } => {
                    c.changed += 1;
                    c.dest_newer += u64::from(*dest_newer);
                }
                Change::Replace { .. } => c.changed += 1,
                Change::TimestampOnly(_) => c.timestamp_only += 1,
                Change::RemoveFile(_) => c.removed_files += 1,
                Change::RemoveDir { files, ignored, .. } => c.removed_files += files + ignored,
            }
        }
        c.removed_files += self
            .folders
            .iter()
            .flat_map(|f| &f.replaced)
            .map(|r| r.files)
            .sum::<u64>();
        c
    }

    pub fn is_empty(&self) -> bool {
        self.folders.iter().all(|f| f.plan.changes.is_empty())
    }
}

/// The path a change creates or rewrites; None for removals.
fn written(c: &Change) -> Option<&Entry> {
    match c {
        Change::Add(e) | Change::TimestampOnly(e) => Some(e),
        Change::Update { entry, .. } | Change::Replace { entry } => Some(entry),
        Change::RemoveFile(_) | Change::RemoveDir { .. } => None,
    }
}

fn windows_reason(rel: &str, kind: &Kind) -> Option<String> {
    use crate::naming::{NameProblem, windows_problem};
    for part in rel.split('/') {
        if let Some(p) = windows_problem(part) {
            return Some(match p {
                NameProblem::ReservedOnWindows => {
                    format!("`{part}` is a reserved name on Windows.")
                }
                NameProblem::BadCharOnWindows(c) => {
                    format!("`{part}` contains `{c}`, which Windows does not allow in names.")
                }
                NameProblem::TrailingDotOrSpace => {
                    format!("`{part}` ends with a dot or space, which Windows does not allow.")
                }
                NameProblem::CaseCollision(_) => continue,
            });
        }
    }
    matches!(kind, Kind::Symlink { .. }).then(|| {
        "Symbolic links are skipped on Windows, where creating them needs special rights.".into()
    })
}

/// Whether `rel` is `top` or lies inside it.
fn within(rel: &str, top: &str) -> bool {
    rel == top || (rel.starts_with(top) && rel.as_bytes().get(top.len()) == Some(&b'/'))
}

/// Drops changes the destination cannot hold into `skipped`: names with a
/// backslash always (removals of them too), names Windows forbids and symlinks when it is Windows,
/// and later paths differing only by case always.
///
/// Also keeps back a removal that differs only by case from a source path,
/// which on a case-insensitive disk would delete what was just written. When
/// the two differ in kind (file against folder), writing the source still
/// removes the destination entry on such a disk, so it is returned as
/// `Replaced` for the preview to show.
pub fn drop_unholdable(
    plan: Plan,
    src: &Manifest,
    dst: &Manifest,
    dest_os: Os,
) -> (Plan, Vec<(String, String)>, Vec<Replaced>) {
    let collisions: HashMap<String, String> =
        crate::naming::case_collisions(src.entries.iter().map(|e| e.rel.as_str()))
            .into_iter()
            .map(|(kept, skipped)| (skipped, kept))
            .collect();
    let src_lower: HashMap<String, &Entry> = src
        .entries
        .iter()
        .map(|e| (e.rel.to_lowercase(), e))
        .collect();
    let mut skipped: Vec<(String, String)> = Vec::new();
    let mut replaced = Vec::new();
    // Changes come parent first, so a skipped folder is seen before its contents.
    let mut skipped_tops: Vec<String> = Vec::new();
    let mut kept = Vec::with_capacity(plan.changes.len());
    for change in plan.changes {
        let Some(entry) = written(&change) else {
            let (rel, is_dir) = match &change {
                Change::RemoveFile(r) => (r, false),
                Change::RemoveDir { rel, .. } => (rel, true),
                _ => unreachable!("written() covers the rest"),
            };
            // Such a name can't be addressed safely (a backslash separates
            // on Windows), and failing would stop every later mirror.
            if rel.contains('\\') {
                skipped.push((
                    rel.clone(),
                    "Its name contains a backslash, so it is left in place. Remove it by hand \
                     if it should go."
                        .into(),
                ));
                continue;
            }
            match src_lower.get(&rel.to_lowercase()) {
                None => kept.push(change),
                Some(s) if (s.kind == Kind::Dir) != is_dir => {
                    let r = if is_dir {
                        folder_replaced(rel, dst, by(&s.kind))
                    } else {
                        Replaced {
                            rel: rel.clone(),
                            files: 1,
                            ignored: 0,
                            by: by(&s.kind),
                            was_folder: false,
                        }
                    };
                    // An empty folder going away removes nothing worth a warning.
                    if r.files > 0 {
                        replaced.push(r);
                    }
                }
                Some(_) => {}
            }
            continue;
        };
        let rel = entry.rel.clone();
        let reason = if let Some(top) = skipped_tops.iter().find(|t| within(&rel, t)) {
            Some(format!("It is inside `{top}`, which was skipped."))
        } else if rel.contains('\\') {
            Some("Its name contains a backslash, which Windows reads as a folder separator.".into())
        } else if let Some(other) = collisions.get(&rel) {
            Some(format!(
                "It differs only by case from `{other}`, which is copied instead."
            ))
        } else if dest_os == Os::Windows {
            windows_reason(&rel, &entry.kind)
        } else {
            None
        };
        match reason {
            Some(why) => {
                skipped_tops.push(rel.clone());
                skipped.push((rel, why));
            }
            None => kept.push(change),
        }
    }
    let needs_hash = plan
        .needs_hash
        .into_iter()
        .filter(|r| !skipped.iter().any(|(s, _)| s == r))
        .collect();
    let plan = Plan {
        changes: kept,
        needs_hash,
    };
    (plan, skipped, replaced)
}

fn by(kind: &Kind) -> String {
    match kind {
        Kind::Dir => "a folder",
        Kind::Symlink { .. } => "a link",
        Kind::File { .. } => "a file",
    }
    .into()
}

/// Everything under the destination folder `rel`, ignored files included.
fn folder_replaced(rel: &str, dst: &Manifest, by: String) -> Replaced {
    let prefix = format!("{rel}/");
    let tracked = dst
        .entries
        .iter()
        .filter(|e| e.rel.starts_with(&prefix) && e.kind != Kind::Dir)
        .count() as u64;
    let ignored = dst.ignored_in_dir.get(rel).copied().unwrap_or(0);
    Replaced {
        rel: rel.to_string(),
        files: tracked + ignored,
        ignored,
        by,
        was_folder: true,
    }
}

/// Destination folders a `Replace` turns into a file or link, which removes
/// everything inside them without a removal line of their own.
pub fn replaced_folders(plan: &Plan, dst: &Manifest) -> Vec<Replaced> {
    let dst_dirs: std::collections::HashSet<&str> = dst
        .entries
        .iter()
        .filter(|e| e.kind == Kind::Dir)
        .map(|e| e.rel.as_str())
        .collect();
    plan.changes
        .iter()
        .filter_map(|c| match c {
            Change::Replace { entry }
                if entry.kind != Kind::Dir && dst_dirs.contains(entry.rel.as_str()) =>
            {
                Some(folder_replaced(&entry.rel, dst, by(&entry.kind)))
            }
            _ => None,
        })
        .filter(|r| r.files > 0)
        .collect()
}

#[cfg(test)]
#[path = "preview_tests.rs"]
mod tests;
