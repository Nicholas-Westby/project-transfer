//! The preview: what a push or pull will change, before anything is written.

use super::widgets::plural;
use crate::manifest::{Change, Kind};
use crate::model::Direction;
use crate::transfer::Preview;

/// Paths under these folders collapse to one line with a count.
const NOISE: &[&str] = &[".git"];

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Groups {
    pub added: Vec<Line>,
    pub changed: Vec<Line>,
    pub timestamp_only: Vec<Line>,
    pub removed: Vec<Line>,
}

/// One line of a list, and the path it is about, so it can be ignored.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    /// Its folder's index in `Preview::folders`.
    pub folder: usize,
    /// The path inside that folder; for a collapsed folder, the folder.
    pub rel: String,
    pub is_dir: bool,
    pub text: String,
    /// False for an entry another computer has where this one writes
    /// something of another kind: leaving out only that entry would let
    /// what takes its place through, unseen.
    pub ignorable: bool,
}

/// How many files the transfer touches, in the same unit as the counts row:
/// files inside a removed folder count one by one.
pub fn file_count(p: &Preview) -> u64 {
    let c = p.counts();
    c.added + c.changed + c.timestamp_only + c.removed_files
}

pub fn confirm_label(p: &Preview, peer: &str) -> String {
    let changes = plural(file_count(p), "file change", "file changes");
    match p.request.direction {
        Direction::Push => format!("Push {changes} to {peer}"),
        Direction::Pull => format!("Pull {changes} from {peer}"),
    }
}

/// The path a change is about, and whether it is a folder.
pub fn path_of(c: &Change) -> (&str, bool) {
    match c {
        Change::Add(e) | Change::TimestampOnly(e) => (&e.rel, e.kind == Kind::Dir),
        Change::Update { entry, .. } | Change::Replace { entry } => {
            (&entry.rel, entry.kind == Kind::Dir)
        }
        Change::RemoveFile(r) => (r, false),
        Change::RemoveDir { rel, .. } => (rel, true),
    }
}

fn noise_root(rel: &str) -> Option<&'static str> {
    NOISE
        .iter()
        .find(|n| rel == **n || rel.strip_prefix(**n).is_some_and(|r| r.starts_with('/')))
        .copied()
}

/// Sorts every change into its list. Each line keeps its folder and path,
/// and its text leaves the folder's name out: patterns see paths that way.
pub fn group(p: &Preview, peer: &str) -> Groups {
    let mut g = Groups::default();
    let newer_where = match p.request.direction {
        Direction::Push => format!("newer on {peer}"),
        Direction::Pull => "newer on this computer".to_string(),
    };
    for (folder, f) in p.folders.iter().enumerate() {
        let line = |rel: &str, is_dir: bool, text: String| Line {
            folder,
            rel: rel.to_string(),
            is_dir,
            text,
            ignorable: true,
        };
        // (list, root) -> count, so noise folders become one line per list.
        let mut noise: Vec<(u8, &str, u64)> = Vec::new();
        let mut bump = |list: u8, root: &'static str| match noise
            .iter_mut()
            .find(|(l, r, _)| *l == list && *r == root)
        {
            Some(e) => e.2 += 1,
            None => noise.push((list, root, 1)),
        };
        for c in &f.plan.changes {
            let list = match c {
                Change::Add(_) => 0,
                Change::Update { .. } | Change::Replace { .. } => 1,
                Change::TimestampOnly(_) => 2,
                Change::RemoveFile(_) | Change::RemoveDir { .. } => 3,
            };
            let (rel, is_dir) = path_of(c);
            if let Some(root) = noise_root(rel) {
                bump(list, root);
                continue;
            }
            let text = match c {
                Change::Update {
                    dest_newer: true, ..
                } => format!("{rel} ({newer_where})"),
                Change::Replace { .. } => format!("{rel} (replaces a different kind of entry)"),
                Change::RemoveDir { files, ignored, .. } => {
                    let total = files + ignored;
                    let ignored = match ignored {
                        0 => String::new(),
                        n => format!(", {n} of them ignored"),
                    };
                    format!(
                        "Remove folder {rel} ({}{ignored})",
                        plural(total, "file", "files")
                    )
                }
                _ => rel.to_string(),
            };
            g.list(list).push(line(rel, is_dir, text));
        }
        for (list, root, n) in noise {
            let text = format!("{root} contents ({})", plural(n, "file", "files"));
            g.list(list).push(line(root, true, text));
        }
        // Counted as removed files, so they are listed with the removals.
        for r in &f.replaced {
            let text = if r.was_folder {
                format!(
                    "Replace folder {} with {} ({})",
                    r.rel,
                    r.by,
                    plural(r.files, "file", "files")
                )
            } else {
                format!("Replace file {} with {}", r.rel, r.by)
            };
            g.removed.push(Line {
                ignorable: false,
                ..line(&r.rel, r.was_folder, text)
            });
        }
    }
    g
}

/// Names the destination cannot hold, each with its reason.
pub fn skipped(p: &Preview) -> Vec<String> {
    p.folders
        .iter()
        .flat_map(|f| f.skipped.iter())
        .map(|(rel, why)| {
            let why = why.replace('`', "");
            if why.contains(rel.as_str()) {
                why
            } else {
                format!("{rel}: {why}")
            }
        })
        .collect()
}

impl Groups {
    fn list(&mut self, n: u8) -> &mut Vec<Line> {
        match n {
            0 => &mut self.added,
            1 => &mut self.changed,
            2 => &mut self.timestamp_only,
            _ => &mut self.removed,
        }
    }
}

/// Warnings for the top of the preview, most serious first.
pub fn warnings(p: &Preview, peer: &str) -> Vec<String> {
    let c = p.counts();
    let mut w = Vec::new();
    if c.dest_newer > 0 {
        let whose = match p.request.direction {
            Direction::Push => format!("on {peer}"),
            Direction::Pull => "on this computer".into(),
        };
        let files = plural(c.dest_newer, "file", "files");
        w.push(format!(
            "{files} {whose} {} newer than the copy replacing {}. Check before you continue.",
            if c.dest_newer == 1 { "is" } else { "are" },
            if c.dest_newer == 1 { "it" } else { "them" },
        ));
    }
    w.extend(
        p.folders
            .iter()
            .flat_map(|f| &f.replaced)
            .map(|r| r.warning()),
    );
    w.extend(p.warnings.iter().cloned());
    // Each skipped name is listed under Skipped; the top only says how many.
    let n: u64 = p.folders.iter().map(|f| f.skipped.len() as u64).sum();
    if n > 0 {
        let dest = match p.request.direction {
            Direction::Push => peer.to_string(),
            Direction::Pull => "this computer".to_string(),
        };
        w.push(format!(
            "{} can't be copied to {dest} and will be skipped.",
            plural(n, "file", "files")
        ));
    }
    w
}

#[cfg(test)]
#[path = "preview_tests.rs"]
mod tests;
