use super::*;

fn mtime(p: &Path) -> i64 {
    let t = filetime::FileTime::from_last_modification_time(&std::fs::symlink_metadata(p).unwrap());
    t.unix_seconds() * 1000 + (t.nanoseconds() / 1_000_000) as i64
}

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

#[test]
fn finished_file_has_content_mtime_and_no_temp() {
    let t = tempfile::tempdir().unwrap();
    let a = Applier::new(t.path().to_path_buf());
    let mut p = a.begin_file("deep/er/f.txt").unwrap();
    p.write(b"hello ").unwrap();
    p.write(b"world").unwrap();
    p.finish(1_600_000_000_123, false).unwrap();
    let f = t.path().join("deep/er/f.txt");
    assert_eq!(std::fs::read(&f).unwrap(), b"hello world");
    assert_eq!(mtime(&f), 1_600_000_000_123);
    assert_eq!(names(&t.path().join("deep/er")), ["f.txt"]);
}

#[cfg(unix)]
#[test]
fn exec_flag_is_set_and_cleared() {
    use std::os::unix::fs::PermissionsExt;
    let t = tempfile::tempdir().unwrap();
    let a = Applier::new(t.path().to_path_buf());
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode();
    a.begin_file("run.sh").unwrap().finish(1000, true).unwrap();
    assert_ne!(mode(&t.path().join("run.sh")) & 0o100, 0);
    a.begin_file("run.sh").unwrap().finish(1000, false).unwrap();
    assert_eq!(mode(&t.path().join("run.sh")) & 0o111, 0);
}

#[test]
fn dropped_file_leaves_old_content_and_no_temp() {
    let t = tempfile::tempdir().unwrap();
    std::fs::write(t.path().join("f"), "old").unwrap();
    let a = Applier::new(t.path().to_path_buf());
    let mut p = a.begin_file("f").unwrap();
    p.write(b"new and half").unwrap();
    assert_eq!(names(t.path()).len(), 2, "temp file sits beside the target");
    drop(p);
    assert_eq!(std::fs::read(t.path().join("f")).unwrap(), b"old");
    assert_eq!(names(t.path()), ["f"]);
}

#[test]
fn temp_name_is_beside_target_and_ignored_suffix() {
    let t = tempfile::tempdir().unwrap();
    let a = Applier::new(t.path().to_path_buf());
    let long = "x".repeat(240);
    let _p = a.begin_file(&long).unwrap();
    let n = names(t.path());
    assert_eq!(n.len(), 1);
    assert!(n[0].ends_with(".pt-tmp"), "{}", n[0]);
    assert!(n[0].len() <= 255);
}

#[test]
fn finish_replaces_a_directory_in_the_way() {
    let t = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(t.path().join("x/sub")).unwrap();
    std::fs::write(t.path().join("x/sub/a"), "a").unwrap();
    let a = Applier::new(t.path().to_path_buf());
    let mut p = a.begin_file("x").unwrap();
    p.write(b"file").unwrap();
    p.finish(1000, false).unwrap();
    assert_eq!(std::fs::read(t.path().join("x")).unwrap(), b"file");
}

#[test]
fn make_dir_replaces_a_file_and_is_idempotent() {
    let t = tempfile::tempdir().unwrap();
    std::fs::write(t.path().join("x"), "f").unwrap();
    let a = Applier::new(t.path().to_path_buf());
    a.make_dir("x").unwrap();
    a.make_dir("x").unwrap();
    a.make_dir("x/y/z").unwrap();
    assert!(t.path().join("x/y/z").is_dir());
}

#[test]
fn set_mtime_changes_only_the_time() {
    let t = tempfile::tempdir().unwrap();
    std::fs::write(t.path().join("f"), "same").unwrap();
    let a = Applier::new(t.path().to_path_buf());
    a.set_mtime("f", 1_500_000_000_000).unwrap();
    assert_eq!(mtime(&t.path().join("f")), 1_500_000_000_000);
    assert_eq!(std::fs::read(t.path().join("f")).unwrap(), b"same");
    assert!(a.set_mtime("missing", 1).is_err());
}

#[test]
fn a_read_only_file_still_gets_its_time_fixed() {
    // Git makes its object files read-only, and a push often only fixes
    // their time.
    let t = tempfile::tempdir().unwrap();
    let f = t.path().join("object");
    std::fs::write(&f, "same").unwrap();
    let mut p = std::fs::metadata(&f).unwrap().permissions();
    p.set_readonly(true);
    std::fs::set_permissions(&f, p).unwrap();
    let a = Applier::new(t.path().to_path_buf());
    a.set_mtime("object", 1_500_000_000_000).unwrap();
    assert_eq!(mtime(&f), 1_500_000_000_000);
    assert_eq!(std::fs::read(&f).unwrap(), b"same");
}

#[test]
fn remove_handles_files_dirs_and_missing() {
    let t = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(t.path().join("d/node_modules/x")).unwrap();
    std::fs::write(t.path().join("d/node_modules/x/i.js"), "i").unwrap();
    std::fs::write(t.path().join("f"), "f").unwrap();
    let a = Applier::new(t.path().to_path_buf());
    a.remove("d", true).unwrap();
    a.remove("f", false).unwrap();
    a.remove("never", false).unwrap();
    a.remove("never/deeper", true).unwrap();
    assert!(names(t.path()).is_empty());
}

#[test]
fn removing_a_file_not_written_here_still_removes_it() {
    let t = tempfile::tempdir().unwrap();
    std::fs::write(t.path().join("old.txt"), "x").unwrap();
    let a = Applier::new(t.path().to_path_buf());
    a.remove("old.txt", false).unwrap();
    assert!(!t.path().join("old.txt").exists());
}

// A Mac treats the two spellings of "ú" as one name, so a plan that writes one
// and removes the other would delete what it just wrote.
#[cfg(target_os = "macos")]
#[test]
fn removing_another_spelling_of_a_file_just_written_keeps_it() {
    let t = tempfile::tempdir().unwrap();
    let a = Applier::new(t.path().to_path_buf());
    std::fs::write(t.path().join("soldu\u{301}.jpg"), "old").unwrap();
    let mut p = a.begin_file("sold\u{fa}.jpg").unwrap();
    p.write(b"new").unwrap();
    p.finish(0, false).unwrap();
    a.remove("soldu\u{301}.jpg", false).unwrap();
    assert_eq!(names(t.path()).len(), 1, "{:?}", names(t.path()));
    let kept = std::fs::read_to_string(t.path().join("sold\u{fa}.jpg"));
    assert_eq!(kept.unwrap(), "new");
}

#[cfg(target_os = "macos")]
#[test]
fn removing_another_spelling_of_a_folder_just_made_keeps_its_files() {
    let t = tempfile::tempdir().unwrap();
    let a = Applier::new(t.path().to_path_buf());
    std::fs::create_dir(t.path().join("Cafe\u{301}")).unwrap();
    a.make_dir("Caf\u{e9}").unwrap();
    let mut p = a.begin_file("Caf\u{e9}/menu.md").unwrap();
    p.write(b"soup").unwrap();
    p.finish(0, false).unwrap();
    a.remove("Cafe\u{301}", true).unwrap();
    let kept = std::fs::read_to_string(t.path().join("Caf\u{e9}/menu.md"));
    assert_eq!(kept.unwrap(), "soup");
}

#[cfg(target_os = "macos")]
#[test]
fn removing_another_spelling_of_a_folder_that_replaced_a_file_keeps_it() {
    let t = tempfile::tempdir().unwrap();
    let a = Applier::new(t.path().to_path_buf());
    std::fs::write(t.path().join("Cafe\u{301}"), "old").unwrap();
    a.make_dir("Caf\u{e9}").unwrap();
    a.remove("Cafe\u{301}", false).unwrap();
    assert!(t.path().join("Caf\u{e9}").is_dir());
}

#[cfg(target_os = "macos")]
#[test]
fn removing_another_spelling_of_a_link_just_made_keeps_it() {
    let t = tempfile::tempdir().unwrap();
    let a = Applier::new(t.path().to_path_buf());
    std::fs::write(t.path().join("lieu\u{301}"), "old").unwrap();
    a.make_symlink("lie\u{fa}", "elsewhere").unwrap();
    a.remove("lieu\u{301}", false).unwrap();
    let kept = std::fs::read_link(t.path().join("lie\u{fa}"));
    assert_eq!(kept.unwrap(), PathBuf::from("elsewhere"));
}

#[test]
fn sweep_deletes_stray_temps_anywhere() {
    let t = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(t.path().join("a/b")).unwrap();
    std::fs::write(t.path().join("a/b/f.1234.pt-tmp"), "x").unwrap();
    std::fs::write(t.path().join("g.9.pt-tmp"), "x").unwrap();
    std::fs::write(t.path().join("keep.txt"), "x").unwrap();
    let a = Applier::new(t.path().to_path_buf());
    assert_eq!(a.sweep_temp().unwrap(), 2);
    assert_eq!(names(t.path()), ["a", "keep.txt"]);
    assert!(names(&t.path().join("a/b")).is_empty());
    let missing = Applier::new(t.path().join("nope"));
    assert_eq!(missing.sweep_temp().unwrap(), 0);
}

#[test]
fn invalid_rel_is_refused_by_every_operation() {
    let t = tempfile::tempdir().unwrap();
    let inner = t.path().join("root");
    std::fs::create_dir(&inner).unwrap();
    let a = Applier::new(inner);
    assert!(a.begin_file("../escape").is_err());
    assert!(a.make_dir("../escape").is_err());
    assert!(a.make_symlink("../escape", "x").is_err());
    assert!(a.set_mtime("../escape", 1).is_err());
    assert!(a.remove("..", true).is_err());
    assert!(!t.path().join("escape").exists());
}

#[cfg(unix)]
mod links {
    use super::*;

    #[test]
    fn paths_through_a_symlink_are_refused() {
        let t = tempfile::tempdir().unwrap();
        let outside = t.path().join("outside");
        let root = t.path().join("root");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(outside.join("victim"), "keep").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("evil")).unwrap();
        let a = Applier::new(root.clone());
        assert!(a.begin_file("evil/new").is_err());
        assert!(a.make_dir("evil/d").is_err());
        assert!(a.remove("evil/victim", false).is_err());
        assert!(a.set_mtime("evil/victim", 1).is_err());
        assert_eq!(names(&outside), ["victim"]);
        // The link itself may be removed; its target stays.
        a.remove("evil", true).unwrap();
        assert_eq!(names(&outside), ["victim"]);
        assert!(names(&root).is_empty());
    }

    #[test]
    fn make_dir_and_symlink_replace_links_without_following() {
        let t = tempfile::tempdir().unwrap();
        let outside = t.path().join("outside");
        let root = t.path().join("root");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(outside.join("victim"), "keep").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("l")).unwrap();
        let a = Applier::new(root.clone());
        a.make_dir("l").unwrap();
        assert!(
            !std::fs::symlink_metadata(root.join("l"))
                .unwrap()
                .is_symlink()
        );
        assert_eq!(names(&outside), ["victim"]);

        std::fs::write(root.join("f"), "f").unwrap();
        a.make_symlink("f", "target/x").unwrap();
        assert_eq!(
            std::fs::read_link(root.join("f")).unwrap(),
            PathBuf::from("target/x")
        );
        a.make_symlink("l", "elsewhere").unwrap();
        assert_eq!(
            std::fs::read_link(root.join("l")).unwrap(),
            PathBuf::from("elsewhere")
        );
        assert_eq!(names(&root), ["f", "l"]);
    }
}

#[cfg(windows)]
#[test]
fn read_only_files_are_replaced_and_removed_on_windows() {
    let dir = tempfile::tempdir().unwrap();
    let a = Applier::new(dir.path().to_path_buf());
    for name in ["kept.txt", "gone.txt"] {
        let path = dir.path().join(name);
        std::fs::write(&path, "old").unwrap();
        let mut p = std::fs::metadata(&path).unwrap().permissions();
        p.set_readonly(true);
        std::fs::set_permissions(&path, p).unwrap();
    }
    let mut f = a.begin_file("kept.txt").unwrap();
    f.write(b"new").unwrap();
    f.finish(0, false).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("kept.txt")).unwrap(),
        "new"
    );
    a.remove("gone.txt", false).unwrap();
    assert!(!dir.path().join("gone.txt").exists());
}
