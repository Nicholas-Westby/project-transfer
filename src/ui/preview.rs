//! The preview: what a push or pull will change, before anything is written.

use super::widgets::plural;
use crate::manifest::Change;
use crate::model::Direction;
use crate::transfer::Preview;

/// Paths under these folders collapse to one line with a count.
const NOISE: &[&str] = &[".git"];

#[derive(Debug, Default, PartialEq)]
pub struct Groups {
    pub added: Vec<String>,
    pub changed: Vec<String>,
    pub timestamp_only: Vec<String>,
    pub removed: Vec<String>,
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

fn noise_root(rel: &str) -> Option<&'static str> {
    NOISE
        .iter()
        .find(|n| rel == **n || rel.strip_prefix(**n).is_some_and(|r| r.starts_with('/')))
        .copied()
}

/// Sorts every change into its list. With more than one folder each path
/// starts with its folder's name.
pub fn group(p: &Preview, peer: &str) -> Groups {
    let mut g = Groups::default();
    let newer_where = match p.request.direction {
        Direction::Push => format!("newer on {peer}"),
        Direction::Pull => "newer on this computer".to_string(),
    };
    let multi = p.folders.len() > 1;
    for f in &p.folders {
        let name = |rel: &str| {
            if multi {
                format!("{}/{rel}", f.name)
            } else {
                rel.to_string()
            }
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
            let (list, rel) = match c {
                Change::Add(e) => (0, e.rel.as_str()),
                Change::Update { entry, .. } | Change::Replace { entry } => (1, entry.rel.as_str()),
                Change::TimestampOnly(e) => (2, e.rel.as_str()),
                Change::RemoveFile(r) | Change::RemoveDir { rel: r, .. } => (3, r.as_str()),
            };
            if let Some(root) = noise_root(rel) {
                bump(list, root);
                continue;
            }
            let line = match c {
                Change::Update {
                    dest_newer: true, ..
                } => format!("{} ({newer_where})", name(rel)),
                Change::Replace { .. } => {
                    format!("{} (replaces a different kind of entry)", name(rel))
                }
                Change::RemoveDir { files, ignored, .. } => {
                    let total = files + ignored;
                    let ignored = match ignored {
                        0 => String::new(),
                        n => format!(", {n} of them ignored"),
                    };
                    format!(
                        "Remove folder {} ({}{ignored})",
                        name(rel),
                        plural(total, "file", "files")
                    )
                }
                _ => name(rel),
            };
            g.list(list).push(line);
        }
        for (list, root, n) in noise {
            let line = format!("{} contents ({})", name(root), plural(n, "file", "files"));
            g.list(list).push(line);
        }
        // Counted as removed files, so they are listed with the removals.
        for r in &f.replaced {
            g.removed.push(if r.was_folder {
                format!(
                    "Replace folder {} with {} ({})",
                    name(&r.rel),
                    r.by,
                    plural(r.files, "file", "files")
                )
            } else {
                format!("Replace file {} with {}", name(&r.rel), r.by)
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
    fn list(&mut self, n: u8) -> &mut Vec<String> {
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
mod tests {
    use super::*;
    use crate::manifest::{Entry, Kind, Plan};
    use crate::transfer::{FolderPreview, TransferRequest};

    fn file(rel: &str) -> Entry {
        Entry {
            rel: rel.into(),
            kind: Kind::File {
                size: 1,
                mtime_ms: 0,
                exec: false,
            },
        }
    }

    fn preview(direction: Direction, changes: Vec<Change>) -> Preview {
        Preview {
            request: TransferRequest {
                peer: uuid::Uuid::nil(),
                project: uuid::Uuid::nil(),
                direction,
                send_everything: false,
            },
            folders: vec![FolderPreview {
                folder: uuid::Uuid::nil(),
                name: "app".into(),
                source_path: "/a".into(),
                dest_path: "/b".into(),
                dest_will_be_created: false,
                plan: Plan {
                    changes,
                    needs_hash: vec![],
                },
                skipped: vec![("aux.txt".into(), "`aux.txt` is reserved on Windows.".into())],
                replaced: vec![],
            }],
            warnings: vec![],
            link: None,
            description: false,
        }
    }

    #[test]
    fn a_pull_says_the_newer_file_is_on_this_computer() {
        let changes = vec![Change::Update {
            entry: file("README.md"),
            dest_newer: true,
        }];
        let g = group(&preview(Direction::Pull, changes.clone()), "Desk");
        assert_eq!(g.changed, ["README.md (newer on this computer)"]);
        let g = group(&preview(Direction::Push, changes), "Desk");
        assert_eq!(g.changed, ["README.md (newer on Desk)"]);
    }

    #[test]
    fn the_button_counts_files_like_the_counts_row() {
        let p = preview(
            Direction::Push,
            vec![
                Change::Add(file("a")),
                Change::RemoveDir {
                    rel: "old".into(),
                    files: 2,
                    ignored: 3,
                },
            ],
        );
        assert_eq!(file_count(&p), 6);
        assert_eq!(confirm_label(&p, "Desk"), "Push 6 file changes to Desk");
        let one = preview(Direction::Pull, vec![Change::Add(file("a"))]);
        assert_eq!(confirm_label(&one, "Desk"), "Pull 1 file change from Desk");
    }

    #[test]
    fn a_skipped_name_is_listed_once_and_summed_up_on_top() {
        let p = preview(Direction::Push, vec![]);
        assert_eq!(skipped(&p), ["aux.txt is reserved on Windows."]);
        let w = warnings(&p, "Desk");
        assert_eq!(w, ["1 file can't be copied to Desk and will be skipped."]);
    }

    #[test]
    fn noise_roots_match_whole_names_only() {
        assert_eq!(noise_root(".git/HEAD"), Some(".git"));
        assert_eq!(noise_root(".git"), Some(".git"));
        assert_eq!(noise_root(".github/workflows"), None);
    }
}
