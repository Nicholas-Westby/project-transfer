use super::*;
use crate::manifest::compare;

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

/// What a transfer from `src`, on `from`, to `dst`, on `to`, makes of the plan.
fn holdable(src: &Manifest, dst: &Manifest, from: Os, to: Os) -> Holdable {
    drop_unholdable(compare(src, dst), src, dst, from, to)
}

fn names(list: &[(String, String)]) -> Vec<&str> {
    list.iter().map(|(r, _)| r.as_str()).collect()
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
    let none = Manifest::default();
    let h = holdable(&src, &none, Os::MacOs, Os::MacOs);
    assert_eq!(h.plan, compare(&src, &none));
    assert!(h.skipped.is_empty());

    let h = holdable(&src, &none, Os::MacOs, Os::Windows);
    assert_eq!(rels(&h.plan), ["ok.txt"]);
    assert_eq!(names(&h.skipped), ["aux", "aux/in.txt", "l", "what?.md"]);
}

#[test]
fn skip_reasons_say_why() {
    let src = man(vec![f("CON.txt"), f("a?"), f("dot."), link("l")], &[]);
    let h = holdable(&src, &Manifest::default(), Os::MacOs, Os::Windows);
    let reason = |r: &str| h.skipped.iter().find(|(x, _)| x == r).unwrap().1.clone();
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
fn names_windows_cant_hold_stay_when_a_copy_on_windows_is_the_source() {
    // The copy on Windows never had them, which says nothing about whether
    // they should go.
    let src = man(vec![f("ok.txt")], &[]);
    let dst = man(
        vec![
            f("ok.txt"),
            f("gone"),
            f("CON.txt"),
            f("notes:old.txt"),
            f("trailing."),
            link("l"),
            d("aux"),
            f("aux/in.txt"),
        ],
        &[],
    );
    let h = holdable(&src, &dst, Os::Windows, Os::MacOs);
    assert_eq!(rels(&h.plan), ["-gone"]);
    assert_eq!(
        names(&h.left_in_place),
        ["CON.txt", "aux", "l", "notes:old.txt", "trailing."]
    );
    assert_eq!(
        h.left_in_place[0].1,
        "`CON.txt` is a reserved name on Windows. The copy on Windows can't have it, so it \
         is left in place."
    );
    assert!(h.skipped.is_empty());

    // A copy on a Mac could have held them, so there they were removed.
    let h = holdable(&src, &dst, Os::MacOs, Os::MacOs);
    assert_eq!(h.plan.changes.len(), 6);
    assert!(h.left_in_place.is_empty());
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
    let h = holdable(&src, &Manifest::default(), Os::MacOs, Os::MacOs);
    assert_eq!(rels(&h.plan), ["Docs", "README.md"]);
    assert_eq!(names(&h.skipped), ["Readme.md", "docs", "docs/a"]);
}

#[test]
fn case_only_rename_does_not_remove_the_new_name() {
    let src = man(vec![f("README.md"), d("src"), f("src/a")], &[]);
    let dst = man(vec![f("readme.md"), d("Src"), f("Src/a"), f("gone")], &[]);
    let h = holdable(&src, &dst, Os::MacOs, Os::MacOs);
    assert_eq!(rels(&h.plan), ["README.md", "src", "src/a", "-gone"]);
    assert!(
        h.replaced.is_empty(),
        "same kinds: nothing hidden is removed"
    );
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
    let h = holdable(&src, &dst, Os::MacOs, Os::MacOs);
    assert_eq!(rels(&h.plan), ["Baz", "Foo", "bar"]);
    assert_eq!(
        h.replaced,
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
    assert_eq!(
        h.replaced[0].warning(),
        "Replacing file `BAR` with a folder."
    );
}

#[test]
fn backslash_names_are_skipped() {
    let src = man(vec![f("a\\b"), f("ok")], &[]);
    for os in [Os::MacOs, Os::Windows] {
        let h = holdable(&src, &Manifest::default(), Os::MacOs, os);
        assert_eq!(rels(&h.plan), ["ok"]);
        assert!(h.skipped[0].1.contains("backslash"), "{:?}", h.skipped);
    }
}

#[test]
fn removing_a_destination_name_with_a_backslash_leaves_it_in_place() {
    let src = man(vec![f("ok")], &[]);
    let dst = man(vec![f("ok"), f("a\\b"), d("c\\d"), f("c\\d/e")], &[]);
    let h = holdable(&src, &dst, Os::MacOs, Os::MacOs);
    assert!(h.plan.changes.is_empty(), "{:?}", rels(&h.plan));
    assert_eq!(names(&h.left_in_place), ["a\\b", "c\\d"]);
    assert!(
        h.left_in_place[0].1.contains("left in place"),
        "{:?}",
        h.left_in_place
    );
    assert!(h.skipped.is_empty());
}
