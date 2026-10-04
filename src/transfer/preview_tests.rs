use super::*;
use crate::manifest::{Entry, compare};

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
    }
}

fn rels(p: &Plan) -> Vec<String> {
    p.changes
        .iter()
        .map(|c| match c {
            Change::Add(e) | Change::TimestampOnly(e) => e.rel.clone(),
            Change::Update { entry, .. } | Change::Replace { entry } => entry.rel.clone(),
            Change::RemoveFile(r) | Change::RemoveDir { rel: r, .. } => format!("-{r}"),
        })
        .collect()
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
                replaced: vec![],
            })
            .collect(),
        warnings: vec![],
        link: None,
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
fn windows_names_are_skipped_only_for_windows() {
    let src = man(
        vec![
            f("ok.txt"),
            d("aux"),
            f("aux/in.txt"),
            f("what?.md"),
            link("l"),
        ],
        &[],
    );
    let plan = compare(&src, &Manifest::default());
    let (kept, skipped, _) = drop_unholdable(plan.clone(), &src, &Manifest::default(), Os::MacOs);
    assert_eq!(kept, plan);
    assert!(skipped.is_empty());

    let (kept, skipped, _) = drop_unholdable(plan, &src, &Manifest::default(), Os::Windows);
    assert_eq!(rels(&kept), ["ok.txt"]);
    let skipped: Vec<&str> = skipped.iter().map(|(r, _)| r.as_str()).collect();
    assert_eq!(skipped, ["aux", "aux/in.txt", "l", "what?.md"]);
}

#[test]
fn skip_reasons_say_why() {
    let src = man(vec![f("CON.txt"), f("a?"), f("dot."), link("l")], &[]);
    let (_, skipped, _) = drop_unholdable(
        compare(&src, &Manifest::default()),
        &src,
        &Manifest::default(),
        Os::Windows,
    );
    let reason = |r: &str| skipped.iter().find(|(x, _)| x == r).unwrap().1.clone();
    assert!(
        reason("CON.txt").contains("reserved"),
        "{}",
        reason("CON.txt")
    );
    assert!(reason("a?").contains('?'));
    assert!(reason("dot.").contains("dot or space"));
    assert!(reason("l").contains("link"));
}

#[test]
fn case_collisions_skip_later_paths_and_their_children() {
    let src = man(
        vec![
            f("README.md"),
            f("Readme.md"),
            d("Docs"),
            d("docs"),
            f("docs/a"),
        ],
        &[],
    );
    let (kept, skipped, _) = drop_unholdable(
        compare(&src, &Manifest::default()),
        &src,
        &Manifest::default(),
        Os::MacOs,
    );
    assert_eq!(rels(&kept), ["Docs", "README.md"]);
    let skipped: Vec<&str> = skipped.iter().map(|(r, _)| r.as_str()).collect();
    assert_eq!(skipped, ["Readme.md", "docs", "docs/a"]);
}

#[test]
fn case_only_rename_does_not_remove_the_new_name() {
    let src = man(vec![f("README.md"), d("src"), f("src/a")], &[]);
    let dst = man(vec![f("readme.md"), d("Src"), f("Src/a"), f("gone")], &[]);
    let (kept, _, replaced) = drop_unholdable(compare(&src, &dst), &src, &dst, Os::MacOs);
    assert_eq!(rels(&kept), ["README.md", "src", "src/a", "-gone"]);
    assert!(replaced.is_empty(), "same kinds: nothing hidden is removed");
}

#[test]
fn case_only_kind_clash_is_counted_as_a_replacement() {
    // `Foo` the file lands on `foo` the folder on a case-insensitive disk.
    // `Baz` lands on an empty folder `baz`, which removes no file.
    let src = man(vec![f("Foo"), d("bar"), f("Baz")], &[]);
    let dst = man(
        vec![d("foo"), f("foo/a"), f("foo/b"), f("BAR"), d("baz")],
        &[("foo", 1), ("", 1)],
    );
    let (kept, _, replaced) = drop_unholdable(compare(&src, &dst), &src, &dst, Os::MacOs);
    assert_eq!(rels(&kept), ["Baz", "Foo", "bar"]);
    assert_eq!(
        replaced,
        vec![
            Replaced {
                rel: "BAR".into(),
                files: 1,
                ignored: 0,
                by: "a folder".into(),
                was_folder: false,
            },
            Replaced {
                rel: "foo".into(),
                files: 3,
                ignored: 1,
                by: "a file".into(),
                was_folder: true,
            },
        ]
    );
    assert_eq!(replaced[0].warning(), "Replacing file `BAR` with a folder.");
}

#[test]
fn backslash_names_are_skipped() {
    let src = man(vec![f("a\\b"), f("ok")], &[]);
    for os in [Os::MacOs, Os::Windows] {
        let (kept, skipped, _) = drop_unholdable(
            compare(&src, &Manifest::default()),
            &src,
            &Manifest::default(),
            os,
        );
        assert_eq!(rels(&kept), ["ok"]);
        assert!(skipped[0].1.contains("backslash"), "{:?}", skipped);
    }
}

#[test]
fn removing_a_destination_name_with_a_backslash_is_skipped() {
    let src = man(vec![f("ok")], &[]);
    let dst = man(vec![f("ok"), f("a\\b"), d("c\\d"), f("c\\d/e")], &[]);
    let (kept, skipped, _) = drop_unholdable(compare(&src, &dst), &src, &dst, Os::MacOs);
    assert!(
        rels(&kept).iter().all(|r| !r.contains('\\')),
        "{:?}",
        rels(&kept)
    );
    let names: Vec<&str> = skipped.iter().map(|(r, _)| r.as_str()).collect();
    assert_eq!(names, ["a\\b", "c\\d"]);
    assert!(skipped[0].1.contains("left in place"), "{:?}", skipped);
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
