use super::*;

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

#[test]
fn clearing_leftovers_removes_only_temp_files_in_the_folder() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("root");
    std::fs::create_dir_all(root.join("a/b")).unwrap();
    std::fs::create_dir_all(root.join("d.5.pt-tmp")).unwrap();
    let outside = t.path().join("o.3.pt-tmp");
    for p in [
        root.join("a/b/f.1234.pt-tmp"),
        root.join("keep.txt"),
        outside.clone(),
    ] {
        std::fs::write(p, "x").unwrap();
    }
    let found = [
        root.join("a/b/f.1234.pt-tmp"),
        root.join("gone.9.pt-tmp"),
        root.join("keep.txt"),
        root.join("d.5.pt-tmp"),
        outside.clone(),
    ];
    assert_eq!(clear_leftovers(&root, &found).unwrap(), 1);
    assert!(names(&root.join("a/b")).is_empty());
    assert_eq!(names(&root), ["a", "d.5.pt-tmp", "keep.txt"]);
    assert!(outside.exists());
}

/// One that can't be removed doesn't keep the rest: each is tried, and the
/// first error comes back.
#[cfg(unix)]
#[test]
fn clearing_leftovers_goes_on_past_one_it_cannot_remove() {
    use std::os::unix::fs::PermissionsExt;
    let t = tempfile::tempdir().unwrap();
    let locked = t.path().join("locked");
    std::fs::create_dir_all(&locked).unwrap();
    let (stuck, free) = (locked.join("f.1.pt-tmp"), t.path().join("g.2.pt-tmp"));
    for p in [&stuck, &free] {
        std::fs::write(p, "x").unwrap();
    }
    let mode = |m| std::fs::Permissions::from_mode(m);
    std::fs::set_permissions(&locked, mode(0o555)).unwrap();
    let got = clear_leftovers(t.path(), &[stuck.clone(), free.clone()]);
    std::fs::set_permissions(&locked, mode(0o755)).unwrap();
    assert_eq!(got.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
    assert!(stuck.exists());
    assert!(!free.exists());
}

/// The scan found a temp file under `a`, which became a link to a folder
/// outside since; clearing it would delete a file there.
#[cfg(unix)]
#[test]
fn clearing_leftovers_never_follows_a_link() {
    let t = tempfile::tempdir().unwrap();
    let outside = t.path().join("outside");
    let root = t.path().join("root");
    std::fs::create_dir_all(outside.join("b")).unwrap();
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(outside.join("b/f.1234.pt-tmp"), "keep").unwrap();
    std::fs::write(root.join("g.5678.pt-tmp"), "x").unwrap();
    std::os::unix::fs::symlink(&outside, root.join("a")).unwrap();
    let found = [root.join("a/b/f.1234.pt-tmp"), root.join("g.5678.pt-tmp")];
    assert_eq!(clear_leftovers(&root, &found).unwrap(), 1);
    assert_eq!(names(&outside.join("b")), ["f.1234.pt-tmp"]);
    assert_eq!(names(&root), ["a"]);
}
