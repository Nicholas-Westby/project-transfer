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
        left_out: Vec::new(),
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
    assert_eq!(g.changed[0].text, "README.md (newer on this computer)");
    let g = group(&preview(Direction::Push, changes), "Desk");
    assert_eq!(g.changed[0].text, "README.md (newer on Desk)");
}

#[test]
fn lines_name_their_folder_and_leave_its_name_out() {
    let mut p = preview(Direction::Push, vec![Change::Add(file("src/new.rs"))]);
    let mut second = p.folders[0].clone();
    second.name = "plant-data".into();
    second.plan.changes = vec![
        Change::Add(file("beds/north.csv")),
        Change::Add(file(".git/HEAD")),
        Change::RemoveDir {
            rel: "old".into(),
            files: 2,
            ignored: 0,
        },
    ];
    p.folders.push(second);
    let g = group(&p, "Desk");
    let added: Vec<_> = g
        .added
        .iter()
        .map(|l| (l.folder, l.rel.as_str(), l.is_dir, l.text.as_str()))
        .collect();
    assert_eq!(
        added,
        [
            (0, "src/new.rs", false, "src/new.rs"),
            (1, "beds/north.csv", false, "beds/north.csv"),
            (1, ".git", true, ".git contents (1 file)"),
        ]
    );
    assert_eq!(g.removed[0].folder, 1);
    assert!(g.removed[0].is_dir);
    assert_eq!(g.removed[0].text, "Remove folder old (2 files)");
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
