use super::*;
use crate::manifest::{Entry, Kind, Manifest, compare};
use crate::transfer::replaced_folders;

fn f(rel: &str) -> Entry {
    Entry {
        rel: rel.into(),
        kind: Kind::File {
            size: 1,
            mtime_ms: 1,
            exec: false,
        },
    }
}

fn d(rel: &str) -> Entry {
    Entry {
        rel: rel.into(),
        kind: Kind::Dir,
    }
}

fn link(rel: &str) -> Entry {
    Entry {
        rel: rel.into(),
        kind: Kind::Symlink { target: "x".into() },
    }
}

fn man(mut entries: Vec<Entry>, ignored: &[(&str, u64)]) -> Manifest {
    entries.sort_by(|a, b| a.rel.cmp(&b.rel));
    Manifest {
        entries,
        ignored_in_dir: ignored.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
        ..Default::default()
    }
}

fn preview(plans: Vec<Plan>) -> Preview {
    Preview {
        request: TransferRequest {
            peer: uuid::Uuid::nil(),
            project: uuid::Uuid::nil(),
            direction: Direction::Push,
            send_everything: false,
        },
        folders: plans
            .into_iter()
            .map(|plan| FolderPreview {
                folder: uuid::Uuid::nil(),
                name: "f".into(),
                source_path: "/a".into(),
                dest_path: "/b".into(),
                dest_will_be_created: false,
                plan,
                skipped: vec![],
                left_in_place: vec![],
                replaced: vec![],
                leftovers: vec![],
            })
            .collect(),
        left_out: Vec::new(),
        warnings: vec![],
        link: None,
        description: false,
        sync: None,
    }
}

#[test]
fn counts_add_up_across_folders() {
    let p1 = Plan {
        changes: vec![
            Change::Add(f("a")),
            Change::Add(d("dir")),
            Change::Update {
                entry: f("u"),
                dest_newer: true,
            },
            Change::Replace { entry: f("r") },
            Change::TimestampOnly(f("t")),
        ],
        needs_hash: vec![],
    };
    let p2 = Plan {
        changes: vec![
            Change::RemoveFile("x".into()),
            Change::RemoveDir {
                rel: "old".into(),
                files: 3,
                ignored: 4,
            },
        ],
        needs_hash: vec![],
    };
    let p = preview(vec![p1, p2]);
    assert_eq!(
        p.counts(),
        Counts {
            added: 2,
            changed: 2,
            timestamp_only: 1,
            removed_files: 8,
            dest_newer: 1,
        }
    );
    assert!(!p.is_empty());
    assert!(preview(vec![Plan::default()]).is_empty());
    // Removing an empty folder is still a change.
    let empty_dir = Plan {
        changes: vec![Change::RemoveDir {
            rel: "e".into(),
            files: 0,
            ignored: 0,
        }],
        needs_hash: vec![],
    };
    assert!(!preview(vec![empty_dir]).is_empty());
}

#[test]
fn replacing_a_folder_counts_every_file_inside_it() {
    let src = man(vec![f("x"), d("y"), f("z"), link("w")], &[]);
    let dst = man(
        vec![
            d("x"),
            f("x/a"),
            f("y"),
            d("z"),
            d("w"),
            d("w/s"),
            f("w/s/1"),
            f("w/2"),
        ],
        &[("", 5), ("x", 5)],
    );
    let plan = compare(&src, &dst);
    let r = replaced_folders(&plan, &dst);
    assert_eq!(
        r,
        vec![
            Replaced {
                rel: "w".into(),
                files: 2,
                ignored: 0,
                by: "a link".into(),
                was_folder: true,
            },
            Replaced {
                rel: "x".into(),
                files: 6,
                ignored: 5,
                by: "a file".into(),
                was_folder: true,
            },
        ]
    );
    assert_eq!(
        r[0].warning(),
        "Replacing folder `w` with a link removes 2 files inside it."
    );
    assert_eq!(
        r[1].warning(),
        "Replacing folder `x` with a file removes 6 files inside it (5 of them ignored)."
    );
    let one = Replaced {
        rel: "q".into(),
        files: 1,
        ignored: 0,
        by: "a file".into(),
        was_folder: true,
    };
    assert_eq!(
        one.warning(),
        "Replacing folder `q` with a file removes 1 file inside it."
    );

    let mut p = preview(vec![plan]);
    p.folders[0].replaced = r;
    assert_eq!(p.counts().removed_files, 8);
}
