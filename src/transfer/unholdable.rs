//! Names one computer's disk can't hold, and what a plan does about them.

use super::preview::Replaced;
use crate::manifest::{Change, Entry, Kind, Manifest, Plan};
use crate::model::Os;
use std::collections::HashMap;

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

/// Whether Windows allows every part of the path `rel` as a name.
pub(super) fn windows_holds(rel: &str) -> bool {
    rel.split('/')
        .all(|part| crate::naming::windows_problem(part).is_none())
}

/// Whether `rel` is `top` or lies inside it.
fn within(rel: &str, top: &str) -> bool {
    rel == top || (rel.starts_with(top) && rel.as_bytes().get(top.len()) == Some(&b'/'))
}

/// A plan with what the destination can't hold taken out.
#[derive(Debug, PartialEq)]
pub struct Holdable {
    pub plan: Plan,
    /// (rel, reason) for names the destination can't hold, which are not
    /// copied.
    pub skipped: Vec<(String, String)>,
    /// (rel, reason) for destination names the plan would remove, which
    /// stay instead.
    pub left_in_place: Vec<(String, String)>,
    /// Destination entries that writing a path differing only by case
    /// removes on a case-insensitive disk.
    pub replaced: Vec<Replaced>,
}

/// Takes out of `plan` what the destination can't hold: names with a
/// backslash always, names Windows forbids and symlinks when it is Windows,
/// and later paths differing only by case always.
///
/// Some removals are left in place: names with a backslash, which can't be
/// addressed safely, and names a copy on Windows can't hold, which it never
/// has. A removal differing only by case from a source path is kept back,
/// as on a case-insensitive disk it would delete what was just written.
/// When the two differ in kind (file against folder), writing the source
/// still removes the destination entry on such a disk, so it is returned
/// as `Replaced` for the preview to show.
pub fn drop_unholdable(
    plan: Plan,
    src: &Manifest,
    dst: &Manifest,
    src_os: Os,
    dest_os: Os,
) -> Holdable {
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
    // What each destination path is, which a removal from a copy on
    // Windows checks below.
    let dst_kinds: HashMap<&str, &Kind> = match src_os {
        Os::Windows => dst
            .entries
            .iter()
            .map(|e| (e.rel.as_str(), &e.kind))
            .collect(),
        Os::MacOs => HashMap::new(),
    };
    let mut skipped: Vec<(String, String)> = Vec::new();
    let mut left_in_place: Vec<(String, String)> = Vec::new();
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
                left_in_place.push((
                    rel.clone(),
                    "Its name contains a backslash, so it is left in place. Remove it by hand \
                     if it should go."
                        .into(),
                ));
                continue;
            }
            // Windows can't hold such a name, so the copy there never has
            // one, and that says nothing about whether this one should go.
            if let Some(why) = dst_kinds
                .get(rel.as_str())
                .and_then(|k| windows_reason(rel, k))
            {
                left_in_place.push((
                    rel.clone(),
                    format!("{why} The copy on Windows can't have it, so it is left in place."),
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
    Holdable {
        plan: Plan {
            changes: kept,
            needs_hash,
        },
        skipped,
        left_in_place,
        replaced,
    }
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
#[path = "unholdable_tests.rs"]
mod tests;
