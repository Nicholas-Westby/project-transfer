use super::*;
use crate::ignore_rules::{DEFAULT_IGNORES, IgnoreSpec};
use crate::manifest::Kind;

fn matcher() -> Matcher {
    Matcher::new(&IgnoreSpec {
        patterns: DEFAULT_IGNORES.iter().map(|s| s.to_string()).collect(),
        ..Default::default()
    })
    .unwrap()
}

fn write(root: &Path, rel: &str, body: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, body).unwrap();
}

#[test]
fn missing_root_is_empty() {
    let t = tempfile::tempdir().unwrap();
    let m = scan(&t.path().join("nope"), &matcher(), false).unwrap();
    assert_eq!(m, Manifest::default());
}

#[test]
fn lists_files_and_empty_dirs_sorted() {
    let t = tempfile::tempdir().unwrap();
    write(t.path(), "src/main.rs", "fn main(){}");
    std::fs::create_dir(t.path().join("empty")).unwrap();
    let m = scan(t.path(), &matcher(), false).unwrap();
    let rels: Vec<_> = m.entries.iter().map(|e| e.rel.as_str()).collect();
    assert_eq!(rels, ["empty", "src", "src/main.rs"]);
    assert_eq!(m.entries[0].kind, Kind::Dir);
    match &m.entries[2].kind {
        Kind::File { size, mtime_ms, .. } => {
            assert_eq!(*size, 11);
            assert!(*mtime_ms > 0);
        }
        k => panic!("{k:?}"),
    }
}

#[test]
fn ignored_files_are_counted_and_ignored_folders_named() {
    let t = tempfile::tempdir().unwrap();
    write(t.path(), "a/keep.txt", "k");
    write(t.path(), "a/node_modules/x/1.js", "1");
    write(t.path(), "a/node_modules/2.js", "2");
    write(t.path(), "a/.DS_Store", "d");
    write(t.path(), "b/.DS_Store", "d");
    write(t.path(), "b/target/debug/app", "x");
    let m = scan(t.path(), &matcher(), false).unwrap();
    let rels: Vec<_> = m.entries.iter().map(|e| e.rel.as_str()).collect();
    assert_eq!(rels, ["a", "a/keep.txt", "b"]);
    assert_eq!(m.ignored_dirs, ["a/node_modules", "b/target"]);
    // What the named folders hold is counted only when a removal needs it.
    assert_eq!(m.ignored_in_dir.get("a"), Some(&1));
    assert_eq!(m.ignored_in_dir.get("b"), Some(&1));
    assert_eq!(m.ignored_in_dir.get(""), Some(&2));
    assert!(!m.ignored_in_dir.contains_key("a/node_modules"));
}

/// Walking an ignored folder only to count its files cost seconds per
/// preview on a large project, and one unreadable folder in it failed the
/// scan.
#[cfg(unix)]
#[test]
fn a_scan_never_reads_inside_an_ignored_folder() {
    use std::os::unix::fs::PermissionsExt;
    let t = tempfile::tempdir().unwrap();
    write(t.path(), "node_modules/locked/x.js", "x");
    write(t.path(), "src/main.rs", "fn main(){}");
    let locked = t.path().join("node_modules/locked");
    let mode = |m| std::fs::Permissions::from_mode(m);
    std::fs::set_permissions(&locked, mode(0o000)).unwrap();
    let m = scan(t.path(), &matcher(), false);
    std::fs::set_permissions(&locked, mode(0o755)).unwrap();
    assert_eq!(m.unwrap().ignored_dirs, ["node_modules"]);
}

#[test]
fn leftover_temp_files_are_noted_where_they_are_on_disk() {
    let t = tempfile::tempdir().unwrap();
    write(t.path(), "src/main.rs", "fn main(){}");
    write(t.path(), "src/main.rs.0a1b2c3d.pt-tmp", "half");
    write(t.path(), "node_modules/x.js.0a1b2c3d.pt-tmp", "half");
    let m = scan(t.path(), &matcher(), false).unwrap();
    assert_eq!(m.leftovers, [t.path().join("src/main.rs.0a1b2c3d.pt-tmp")]);
    let rels: Vec<_> = m.entries.iter().map(|e| e.rel.as_str()).collect();
    assert_eq!(rels, ["src", "src/main.rs"]);
}

#[test]
fn gitignore_files_are_not_read() {
    let t = tempfile::tempdir().unwrap();
    write(t.path(), ".gitignore", "secret.txt\n");
    write(t.path(), "secret.txt", "s");
    let m = scan(t.path(), &matcher(), false).unwrap();
    assert!(m.entries.iter().any(|e| e.rel == "secret.txt"));
}

#[cfg(unix)]
#[test]
fn symlinks_are_recorded_not_followed() {
    use std::os::unix::fs::PermissionsExt;
    let t = tempfile::tempdir().unwrap();
    write(t.path(), "real/f.txt", "x");
    std::os::unix::fs::symlink("real", t.path().join("link")).unwrap();
    write(t.path(), "run.sh", "#!/bin/sh");
    std::fs::set_permissions(
        t.path().join("run.sh"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    let m = scan(t.path(), &matcher(), false).unwrap();
    let link = m.entries.iter().find(|e| e.rel == "link").unwrap();
    assert_eq!(
        link.kind,
        Kind::Symlink {
            target: "real".into()
        }
    );
    assert!(!m.entries.iter().any(|e| e.rel == "link/f.txt"));
    let sh = m.entries.iter().find(|e| e.rel == "run.sh").unwrap();
    assert!(matches!(sh.kind, Kind::File { exec: true, .. }));
}

/// A folder and a file in it, both stored decomposed, are listed as stored
/// unless the scan composes, and then by their composed names, so the
/// file's path is built on the composed folder name.
#[cfg(target_os = "macos")]
#[test]
fn decomposed_folders_and_files_are_composed_only_when_asked() {
    let t = tempfile::tempdir().unwrap();
    write(t.path(), "Cafe\u{301}/menu\u{301}.md", "soup");
    let rels = |compose| {
        let m = scan(t.path(), &matcher(), compose).unwrap();
        m.entries.into_iter().map(|e| e.rel).collect::<Vec<_>>()
    };
    assert_eq!(rels(false), ["Cafe\u{301}", "Cafe\u{301}/menu\u{301}.md"]);
    assert_eq!(rels(true), ["Caf\u{e9}", "Caf\u{e9}/men\u{fa}.md"]);
}

#[test]
fn hash_is_blake3_hex() {
    let t = tempfile::tempdir().unwrap();
    write(t.path(), "f", "hello");
    assert_eq!(
        hash_file(&t.path().join("f")).unwrap(),
        blake3::hash(b"hello").to_hex().to_string()
    );
}
