use super::*;
use crate::manifest::{Entry, Kind};

fn dir(rel: &str) -> Entry {
    Entry {
        rel: rel.into(),
        kind: Kind::Dir,
    }
}

fn file(rel: &str) -> Entry {
    Entry {
        rel: rel.into(),
        kind: Kind::File {
            size: 1,
            mtime_ms: 1,
            exec: false,
        },
    }
}

fn with(entries: Vec<Entry>, ignored_dirs: &[&str]) -> Manifest {
    Manifest {
        entries,
        ignored_dirs: ignored_dirs.iter().map(|d| d.to_string()).collect(),
        ..Default::default()
    }
}

fn touch(root: &Path, rel: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, "x").unwrap();
}

#[test]
fn only_ignored_folders_inside_a_folder_the_source_lacks_are_needed() {
    let src = with(
        vec![
            dir("app"),
            file("build"),
            dir("lib"),
            file("old"),
            dir("web"),
        ],
        &[],
    );
    let dst = with(
        vec![
            dir("Build"),
            dir("app"),
            dir("gone"),
            dir("gone/deep"),
            dir("old"),
            dir("web"),
            dir("web/old"),
        ],
        &[
            "Build/node_modules",
            "node_modules",
            "app/node_modules",
            "gone/node_modules",
            "gone/deep/target",
            "old/bin",
            "web/old/node_modules",
        ],
    );
    // A top-level ignored folder and one in a folder that stays are never
    // removed; one in a folder that goes or becomes a file is, even when
    // that folder sits in one that stays, as `web/old` sits in `web`. A file
    // whose name differs only by case takes the folder's place on a disk
    // that ignores case, so `Build` goes too.
    assert_eq!(
        needed(&src, &dst),
        [
            "Build/node_modules",
            "gone/node_modules",
            "gone/deep/target",
            "old/bin",
            "web/old/node_modules",
        ]
    );
}

#[test]
fn counted_files_join_every_folder_around_them() {
    // A reply's counts, then the ignored files by folder and the folders
    // still named once they are added.
    let rows = [
        (
            &[("a/b/node_modules", 5)][..],
            &[("", 6), ("a", 6), ("a/b", 5)][..],
            &["a/target"][..],
        ),
        // A count for a folder not named, such as one counted already,
        // changes nothing.
        (
            &[("c/vendor", 7)][..],
            &[("", 1), ("a", 1)][..],
            &["a/b/node_modules", "a/target"][..],
        ),
    ];
    for (counts, want, named) in rows {
        let mut m = with(vec![], &["a/b/node_modules", "a/target"]);
        m.ignored_in_dir.insert("".into(), 1);
        m.ignored_in_dir.insert("a".into(), 1);
        let counts = counts.iter().map(|(d, n)| (d.to_string(), *n)).collect();
        add_counts(&mut m, &counts);
        let got: Vec<(&str, u64)> = m
            .ignored_in_dir
            .iter()
            .map(|(k, v)| (k.as_str(), *v))
            .collect();
        assert_eq!(got, want, "{counts:?}");
        assert_eq!(m.ignored_dirs, named, "{counts:?}");
    }
}

#[test]
fn counting_finds_every_file_at_any_depth() {
    let t = tempfile::tempdir().unwrap();
    for rel in ["nm/a.js", "nm/x/b.js", "nm/x/y/c.js", "other/d.js"] {
        touch(t.path(), rel);
    }
    std::fs::create_dir_all(t.path().join("nm/empty")).unwrap();
    let got = count(t.path(), &["nm".to_string(), "missing".to_string()]).unwrap();
    assert_eq!(
        got,
        [("nm".to_string(), 3), ("missing".to_string(), 0)].into()
    );
}

/// A build can clear `target` while it is counted. A folder listed but
/// gone by the time it is read holds nothing, so the preview goes on.
#[test]
fn a_folder_gone_while_counted_holds_no_files() {
    let t = tempfile::tempdir().unwrap();
    assert_eq!(count_files(&t.path().join("target/debug")).unwrap(), 0);
}

#[test]
fn counting_every_folder_gives_what_older_versions_expect() {
    let t = tempfile::tempdir().unwrap();
    for rel in ["a/keep.txt", "a/node_modules/x/1.js", "a/node_modules/2.js"] {
        touch(t.path(), rel);
    }
    let mut m = with(vec![dir("a"), file("a/keep.txt")], &["a/node_modules"]);
    count_all(t.path(), &mut m).unwrap();
    assert_eq!(m.ignored_in_dir.get("a"), Some(&2));
    assert_eq!(m.ignored_in_dir.get(""), Some(&2));
    assert!(m.ignored_dirs.is_empty());
}

/// A request can't carry a name with a backslash, so such a folder is
/// counted as it is scanned; any other stays named until a removal needs it.
#[cfg(unix)]
#[test]
fn a_scan_counts_only_the_ignored_folders_no_request_can_name() {
    use crate::ignore_rules::{DEFAULT_IGNORES, IgnoreSpec};
    let t = tempfile::tempdir().unwrap();
    for rel in [
        "odd\\name/node_modules/x/1.js",
        "odd\\name/node_modules/2.js",
    ] {
        touch(t.path(), rel);
    }
    touch(t.path(), "app/node_modules/3.js");
    let spec = IgnoreSpec {
        patterns: DEFAULT_IGNORES.iter().map(|s| s.to_string()).collect(),
        ..Default::default()
    };
    let m = scan(t.path(), &Matcher::new(&spec).unwrap(), false).unwrap();
    assert_eq!(m.ignored_dirs, ["app/node_modules"]);
    assert_eq!(m.ignored_in_dir.get("odd\\name"), Some(&2));
    assert_eq!(m.ignored_in_dir.get(""), Some(&2));
    assert!(!m.ignored_in_dir.contains_key("app"));
}
