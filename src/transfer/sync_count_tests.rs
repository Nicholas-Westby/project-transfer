use super::*;
use crate::manifest::Entry;

fn file(rel: &str, size: u64, mtime_ms: i64) -> Entry {
    Entry {
        rel: rel.into(),
        kind: Kind::File {
            size,
            mtime_ms,
            exec: false,
        },
    }
}

fn folder(entries: Vec<Entry>) -> Manifest {
    Manifest {
        entries,
        ..Default::default()
    }
}

fn tally(
    here: Option<&Manifest>,
    there: Option<&Manifest>,
    same: &[&str],
    since: Option<i64>,
) -> Tally {
    let same: HashSet<String> = same.iter().map(|s| s.to_string()).collect();
    let mut t = Tally::default();
    tally_folder(&mut t, here, there, &same, since);
    t
}

#[test]
fn a_file_changed_on_both_sides_counts_for_the_later_one() {
    let here = folder(vec![file("a.txt", 3, 200), file("b.txt", 3, 100)]);
    let there = folder(vec![file("a.txt", 4, 100), file("b.txt", 4, 300)]);
    let t = tally(Some(&here), Some(&there), &[], None);
    assert_eq!((t.here, t.there), (1, 1));
    assert_eq!((t.newest_here, t.newest_there), (Some(200), Some(300)));
}

#[test]
fn a_new_time_on_the_same_content_does_not_count() {
    let here = folder(vec![file("a.txt", 3, 200)]);
    let there = folder(vec![file("a.txt", 3, 100)]);
    assert_eq!(
        tally(Some(&here), Some(&there), &["a.txt"], None),
        Tally::default()
    );
    // Without matching hashes it is a change.
    assert_eq!(tally(Some(&here), Some(&there), &[], None).here, 1);
}

#[test]
fn a_file_one_side_lacks_counts_for_the_side_that_has_it() {
    let here = folder(vec![file("new-here.txt", 1, 500)]);
    let there = folder(vec![file("new-there.txt", 1, 600)]);
    let t = tally(Some(&here), Some(&there), &[], None);
    assert_eq!((t.here, t.there), (1, 1));
    assert_eq!(t.newest_there, Some(600));
}

/// Both had kept.txt and old.txt at the last transfer, at 1000; the other
/// side removed old.txt since, and this side added fresh.txt.
fn removed_there() -> (Manifest, Manifest) {
    let here = folder(vec![
        file("kept.txt", 1, 900),
        file("old.txt", 1, 900),
        file("fresh.txt", 1, 1500),
    ]);
    (here, folder(vec![file("kept.txt", 1, 900)]))
}

#[test]
fn a_file_older_than_the_last_transfer_was_removed_on_the_other_side() {
    let (here, there) = removed_there();
    let t = tally(Some(&here), Some(&there), &[], Some(1000));
    assert_eq!((t.here, t.there), (1, 1));
    // The removal has no time of its own.
    assert_eq!((t.newest_here, t.newest_there), (Some(1500), None));
    assert_eq!((t.removed_here, t.removed_there), (0, 1));
    let t = tally(Some(&there), Some(&here), &[], Some(1000));
    assert_eq!((t.here, t.there), (1, 1));
    assert_eq!((t.removed_here, t.removed_there), (1, 0));
}

#[test]
fn a_copy_with_no_files_counts_as_missing() {
    let full = folder(vec![file("a.txt", 1, 10), file("b.txt", 1, 20)]);
    let empty = folder(vec![Entry {
        rel: "src".into(),
        kind: Kind::Dir,
    }]);
    let t = tally(Some(&empty), Some(&full), &[], Some(1000));
    assert_eq!((t.here, t.there), (0, 2));
    let t = tally(Some(&full), Some(&empty), &[], Some(1000));
    assert_eq!((t.here, t.there), (2, 0));
}

#[test]
fn a_folder_missing_on_one_side_counts_every_file_for_the_other() {
    let here = folder(vec![file("a.txt", 1, 10), file("b.txt", 1, 20)]);
    let t = tally(Some(&here), None, &[], Some(1000));
    assert_eq!((t.here, t.there), (2, 0));
    let t = tally(None, Some(&here), &[], Some(1000));
    assert_eq!((t.here, t.there), (0, 2));
}

#[test]
fn folders_and_links_are_not_counted() {
    let dir = Entry {
        rel: "src".into(),
        kind: Kind::Dir,
    };
    let link = Entry {
        rel: "latest".into(),
        kind: Kind::Symlink {
            target: "v2".into(),
        },
    };
    let t = tally(
        Some(&folder(vec![dir, link])),
        Some(&folder(vec![])),
        &[],
        None,
    );
    assert_eq!(t, Tally::default());
}

fn choice(
    here: u64,
    there: u64,
    newest_here: Option<i64>,
    newest_there: Option<i64>,
) -> SyncChoice {
    SyncChoice::from_tally(Tally {
        here,
        there,
        newest_here,
        newest_there,
        ..Default::default()
    })
}

#[test]
fn more_newer_changes_win_and_a_tie_goes_to_the_newest_then_to_push() {
    assert_eq!(choice(7, 2, Some(1), Some(9)).direction, Direction::Push);
    assert_eq!(choice(1, 3, Some(9), Some(1)).direction, Direction::Pull);
    assert_eq!(choice(2, 2, Some(5), Some(8)).direction, Direction::Pull);
    assert_eq!(choice(2, 2, Some(8), Some(5)).direction, Direction::Push);
    assert_eq!(choice(2, 2, Some(5), Some(5)).direction, Direction::Push);
    assert_eq!(choice(0, 0, None, None).direction, Direction::Push);
}

#[test]
fn the_reason_names_the_counts_on_each_side() {
    let say = |c: SyncChoice| c.reason("Desk");
    assert_eq!(
        say(choice(7, 2, Some(1), Some(9))),
        "Sync chose to push: 7 files have newer changes here, 2 on Desk."
    );
    assert_eq!(
        say(choice(0, 1, None, Some(9))),
        "Sync chose to pull: 1 file has newer changes on Desk, none here."
    );
    assert_eq!(
        say(choice(2, 2, Some(5), Some(8))),
        "Sync chose to pull: 2 files have newer changes on each computer, and the newest change \
         is on Desk."
    );
    assert_eq!(
        say(choice(1, 1, Some(8), Some(5))),
        "Sync chose to push: 1 file has newer changes on each computer, and the newest change is \
         here."
    );
    assert_eq!(
        say(choice(2, 2, Some(5), Some(5))),
        "Sync chose to push: 2 files have newer changes on each computer, and on a tie it pushes."
    );
    assert_eq!(
        say(choice(0, 0, None, None)),
        "Sync chose to push: neither computer has files with newer changes."
    );
}

#[test]
fn the_reason_says_how_many_were_removals() {
    let (here, there) = removed_there();
    let c = SyncChoice::from_tally(tally(Some(&here), Some(&there), &[], Some(1000)));
    assert_eq!(
        c.reason("Desk"),
        "Sync chose to push: 1 file has newer changes on each computer, and the newest change is \
         here. Removed since the last transfer: none here, 1 on Desk."
    );
}

#[test]
fn a_sync_that_turned_around_says_so() {
    assert_eq!(
        choice(0, 1, None, Some(9)).turned_around(),
        "Sync now pulls instead of pushing: the new ignore list changed which side has more \
         files with newer changes. Check the lists again before you confirm."
    );
    assert!(
        choice(1, 0, Some(9), None)
            .turned_around()
            .starts_with("Sync now pushes instead of pulling: ")
    );
}

#[test]
fn a_repository_counts_as_one_change_a_side() {
    // Three files edited here; one edited and committed there, which
    // rewrote the index and added four files under .git.
    let here = folder(vec![
        file("a.rs", 1, 300),
        file("b.rs", 1, 300),
        file("c.rs", 1, 300),
        file("d.rs", 1, 100),
        file(".git/index", 1, 100),
    ]);
    let there = folder(vec![
        file("a.rs", 2, 100),
        file("b.rs", 2, 100),
        file("c.rs", 2, 100),
        file("d.rs", 2, 400),
        file(".git/index", 2, 400),
        file(".git/COMMIT_EDITMSG", 1, 400),
        file(".git/logs/HEAD", 1, 400),
        file(".git/objects/ab/cdef", 1, 400),
        file(".git/objects/12/3456", 1, 400),
    ]);
    let t = tally(Some(&here), Some(&there), &[], None);
    assert_eq!((t.here, t.there), (3, 2));
    assert_eq!(t.newest_there, Some(400));
}

#[test]
fn a_copy_with_only_a_repository_counts_as_missing() {
    let fresh = folder(vec![file(".git/HEAD", 1, 5000)]);
    let full = folder(vec![file("a.txt", 1, 10), file("b.txt", 1, 20)]);
    let t = tally(Some(&fresh), Some(&full), &[], Some(1000));
    assert_eq!((t.here, t.there), (1, 2));
    assert_eq!(t.removed_here, 0);
}
