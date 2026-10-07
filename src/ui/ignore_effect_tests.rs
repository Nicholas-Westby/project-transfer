use super::*;
use crate::ignore_rules::{IgnoreSpec, Matcher};
use crate::manifest::{Change, Entry, Kind, Plan, compare, scan};
use crate::model::{Direction, Os};
use crate::transfer::{
    FolderPreview, LeftOut, Replaced, TransferRequest, drop_unholdable, replaced_folders,
};
use crate::ui::preview::{file_count, path_of};
use std::collections::BTreeSet;
use std::path::Path;

fn matcher(pattern: &str) -> Matcher {
    Matcher::new(&IgnoreSpec {
        patterns: vec![pattern.to_string()],
        ..Default::default()
    })
    .unwrap()
}

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

fn folder(name: &str, changes: Vec<Change>, replaced: Vec<Replaced>) -> FolderPreview {
    FolderPreview {
        folder: uuid::Uuid::nil(),
        name: name.into(),
        source_path: "/a".into(),
        dest_path: "/b".into(),
        dest_will_be_created: false,
        plan: Plan {
            changes,
            needs_hash: vec![],
        },
        skipped: vec![("aux.txt".into(), "reserved".into())],
        replaced,
    }
}

fn preview(folders: Vec<FolderPreview>) -> Preview {
    Preview {
        request: TransferRequest {
            peer: uuid::Uuid::nil(),
            project: uuid::Uuid::nil(),
            direction: Direction::Push,
            send_everything: false,
        },
        folders,
        left_out: vec![LeftOut {
            name: "docs".into(),
            reason: "It is not here.".into(),
        }],
        warnings: vec!["A warning.".into()],
        link: None,
        description: false,
    }
}

/// A folder of `files` files on the other side, which a file takes over.
fn replaced(rel: &str, files: u64) -> Replaced {
    Replaced {
        rel: rel.into(),
        files,
        ignored: 0,
        by: "a file".into(),
        was_folder: true,
    }
}

#[test]
fn a_preview_narrows_to_what_a_pattern_leaves_out() {
    let app = folder(
        "app",
        vec![
            Change::Add(file("src/main.rs")),
            Change::Add(file("thumbs/a.png")),
            Change::RemoveDir {
                rel: "old/thumbs".into(),
                files: 3,
                ignored: 1,
            },
        ],
        vec![],
    );
    let data = folder(
        "plant-data",
        vec![
            Change::Add(file("data/thumbs/b.png")),
            Change::Update {
                entry: file("beds.toml"),
                dest_newer: false,
            },
        ],
        vec![],
    );
    let found = left_out(&preview(vec![app, data]), &matcher("thumbs/"));
    let rels: Vec<_> = found
        .preview
        .folders
        .iter()
        .flat_map(|f| &f.plan.changes)
        .map(|c| path_of(c).0)
        .collect();
    assert_eq!(rels, ["thumbs/a.png", "old/thumbs", "data/thumbs/b.png"]);
    // One added file, a removed folder of four, and one more.
    assert_eq!(file_count(&found.preview), 1 + 4 + 1);
    assert!(found.preview.folders.iter().all(|f| f.skipped.is_empty()));
    assert!(found.preview.left_out.is_empty() && found.preview.warnings.is_empty());
    assert!(found.still_replaced.is_empty());
}

#[test]
fn a_file_meeting_a_folder_counts_only_when_both_are_left_out() {
    // `build` is a file here and a folder of three files on the other side.
    let replace = Change::Replace {
        entry: file("build"),
    };
    let p = preview(vec![folder(
        "app",
        vec![replace],
        vec![replaced("build", 3)],
    )]);
    for pattern in ["/build", "build"] {
        let found = left_out(&p, &matcher(pattern));
        assert_eq!(file_count(&found.preview), 1 + 3, "{pattern}");
        assert!(found.still_replaced.is_empty(), "{pattern}");
    }
    // Ending in / it leaves out the folder, not the file that takes its place.
    let found = left_out(&p, &matcher("/build/"));
    assert_eq!(file_count(&found.preview), 0);
    assert_eq!(found.still_replaced, [replaced("build", 3)]);
}

#[test]
fn a_folder_clashing_by_case_counts_only_when_both_spellings_are_left_out() {
    // A file `Docs` here; a folder `docs` there, on a disk that ignores case.
    let p = preview(vec![folder(
        "app",
        vec![Change::Add(file("Docs"))],
        vec![replaced("docs", 2)],
    )]);
    let found = left_out(&p, &matcher("/docs/"));
    assert_eq!(file_count(&found.preview), 0);
    assert_eq!(found.still_replaced, [replaced("docs", 2)]);
    let found = left_out(&p, &matcher("/[Dd]ocs"));
    assert_eq!(file_count(&found.preview), 1 + 2);
    assert!(found.still_replaced.is_empty());
}

fn write(root: &Path, rel: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, rel).unwrap();
}

/// The preview `prepare` builds for a push from `src` to `dst`.
fn compared(src: &Path, dst: &Path, m: &Matcher) -> Preview {
    let (s, d) = (scan(src, m, false).unwrap(), scan(dst, m, false).unwrap());
    let plan = compare(&s, &d);
    assert!(plan.needs_hash.is_empty());
    let (plan, skipped, clashes) = drop_unholdable(plan, &s, &d, Os::MacOs);
    let mut replaced = replaced_folders(&plan, &d);
    replaced.extend(clashes);
    let mut p = preview(vec![folder("app", plan.changes, replaced)]);
    p.folders[0].skipped = skipped;
    p
}

/// Everything a preview would do, for comparing two of them. A replaced
/// folder counts by its files: which of them are ignored changes nothing.
fn effects(p: &Preview) -> BTreeSet<String> {
    let f = &p.folders[0];
    let changes = f.plan.changes.iter().map(|c| format!("{c:?}"));
    let replaced = f
        .replaced
        .iter()
        .map(|r| format!("{} ({})", r.rel, r.files));
    changes.chain(replaced).collect()
}

#[test]
fn what_the_dialog_promises_is_what_comparing_again_does() {
    let (src, dst) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let here = [
        "build",
        "main.rs",
        "exports/a.svg",
        "exports/b.svg",
        "notes/todo.txt",
    ];
    for rel in here {
        write(src.path(), rel);
    }
    for rel in ["build/1.o", "build/2.o", "build/3.o", "notes/old.txt"] {
        write(dst.path(), rel);
    }
    let before = compared(
        src.path(),
        dst.path(),
        &Matcher::new(&Default::default()).unwrap(),
    );
    for pattern in [
        "/build",
        "build",
        "/exports/",
        "*.svg",
        "*.txt",
        "notes/",
        "*.o",
    ] {
        let m = matcher(pattern);
        let found = left_out(&before, &m);
        assert!(found.still_replaced.is_empty(), "{pattern}");
        let after = compared(src.path(), dst.path(), &m);
        let promised: BTreeSet<_> = effects(&before)
            .difference(&effects(&found.preview))
            .cloned()
            .collect();
        assert_eq!(effects(&after), promised, "{pattern}");
    }
    // The file would replace the folder unseen; the dialog says so instead.
    let found = left_out(&before, &matcher("/build/"));
    assert_eq!(found.still_replaced, [replaced("build", 3)]);
}
