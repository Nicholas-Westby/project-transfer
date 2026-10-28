//! How a sync picks its direction: count the files each side changed more
//! recently, and go from the side with more.

use super::unholdable::windows_holds;
use crate::manifest::{Kind, Manifest};
use crate::model::Direction;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

/// Files with newer changes on each side, and when each side's newest
/// change was made.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tally {
    pub here: u64,
    pub there: u64,
    /// The modified time of the newest change counted for each side. A
    /// removal counts without one: when it happened is not known.
    pub newest_here: Option<i64>,
    pub newest_there: Option<i64>,
    /// Of those, files counted because that side removed them since the
    /// last transfer.
    pub removed_here: u64,
    pub removed_there: u64,
}

/// Which way a sync went, and the counts that decided it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyncChoice {
    pub direction: Direction,
    /// Boxed: every preview carries a choice, and a larger one would make
    /// each transfer state as large as a preview.
    pub tally: Box<Tally>,
}

impl Tally {
    fn here_changed(&mut self, at: Option<i64>) {
        self.here += 1;
        self.newest_here = self.newest_here.max(at);
    }

    fn there_changed(&mut self, at: Option<i64>) {
        self.there += 1;
        self.newest_there = self.newest_there.max(at);
    }
}

/// Each file's size, modified time and executable bit, by path.
type Files<'a> = HashMap<&'a str, (u64, i64, bool)>;

/// Each file of `m` by path, as (the project's files, the repository's
/// own files under a `.git` folder). Folders and links are not counted, nor
/// with `windows` the names Windows can't hold.
fn files(m: &Manifest, windows: bool) -> (Files<'_>, Files<'_>) {
    m.entries
        .iter()
        .filter(|e| !windows || windows_holds(&e.rel))
        .filter_map(|e| match e.kind {
            Kind::File {
                size,
                mtime_ms,
                exec,
            } => Some((e.rel.as_str(), (size, mtime_ms, exec))),
            _ => None,
        })
        .partition(|(rel, _)| !rel.split('/').any(|part| part == ".git"))
}

/// Adds one folder's files to `t`. `here` and `there` are the folder on
/// each computer, None where it is missing; `same` holds the paths whose
/// content matches although their times differ; `since` is when the last
/// transfer between these two computers read the folders, if the project
/// last moved between them; `windows` says either computer runs Windows.
pub fn tally_folder(
    t: &mut Tally,
    here: Option<&Manifest>,
    there: Option<&Manifest>,
    same: &HashSet<String>,
    since: Option<i64>,
    windows: bool,
) {
    let empty = Manifest::default();
    // A name Windows can't hold moves neither way: a push to it skips the
    // file and a pull from it leaves the file in place.
    let (mine, my_repo) = files(here.unwrap_or(&empty), windows);
    let (theirs, their_repo) = files(there.unwrap_or(&empty), windows);
    // A file one side lacks was removed there only if that side has files
    // outside .git in the folder and the file is no newer than `since`,
    // when both sides had it. A copy with no files is far likelier new than
    // emptied on purpose; reading it as removals would empty the other copy.
    let both = !mine.is_empty() && !theirs.is_empty();
    let removed = |at: i64| both && since.is_some_and(|s| at <= s);
    count(t, &mine, &theirs, same, removed);
    // One commit rewrites a dozen files in .git, which the preview shows as
    // one line; outvoting the project's own files with them would surprise.
    let mut repo = Tally::default();
    count(&mut repo, &my_repo, &their_repo, same, removed);
    if repo.here > 0 {
        t.here_changed(repo.newest_here);
    }
    if repo.there > 0 {
        t.there_changed(repo.newest_there);
    }
}

/// Counts each file for the side whose change is newer, or for the side
/// that removed it when `removed` says one side lacks it on purpose.
fn count(
    t: &mut Tally,
    mine: &Files,
    theirs: &Files,
    same: &HashSet<String>,
    removed: impl Fn(i64) -> bool,
) {
    for (rel, &(size, at, exec)) in mine {
        match theirs.get(rel) {
            Some(&(their_size, their_at, their_exec)) => {
                let same_content = size == their_size && exec == their_exec && same.contains(*rel);
                // Equal times leave nothing to tell the newer by.
                if same_content || at == their_at {
                    continue;
                }
                if at > their_at {
                    t.here_changed(Some(at));
                } else {
                    t.there_changed(Some(their_at));
                }
            }
            None if removed(at) => {
                t.there_changed(None);
                t.removed_there += 1;
            }
            None => t.here_changed(Some(at)),
        }
    }
    for (rel, &(_, at, _)) in theirs {
        if mine.contains_key(rel) {
            continue;
        }
        if removed(at) {
            t.here_changed(None);
            t.removed_here += 1;
        } else {
            t.there_changed(Some(at));
        }
    }
}

impl SyncChoice {
    /// More newer changes wins. A tie goes to the side with the newest
    /// change, and anything still tied pushes.
    pub fn from_tally(tally: Tally) -> SyncChoice {
        let direction = match tally.here.cmp(&tally.there) {
            Ordering::Greater => Direction::Push,
            Ordering::Less => Direction::Pull,
            Ordering::Equal if tally.newest_there > tally.newest_here => Direction::Pull,
            Ordering::Equal => Direction::Push,
        };
        SyncChoice {
            direction,
            tally: Box::new(tally),
        }
    }

    /// One plain sentence for the preview: which way, and why.
    pub fn reason(&self, peer: &str) -> String {
        let t = &self.tally;
        let verb = match self.direction {
            Direction::Push => "push",
            Direction::Pull => "pull",
        };
        let why = match (t.here.cmp(&t.there), self.direction) {
            (Ordering::Greater, _) => {
                format!(
                    "{} newer changes here, {} on {peer}",
                    have(t.here),
                    none_or(t.there)
                )
            }
            (Ordering::Less, _) => {
                format!(
                    "{} newer changes on {peer}, {} here",
                    have(t.there),
                    none_or(t.here)
                )
            }
            (Ordering::Equal, _) if t.here == 0 => {
                "neither computer has files with newer changes".to_string()
            }
            (Ordering::Equal, Direction::Pull) => format!(
                "{} newer changes on each computer, and the newest change is on {peer}",
                have(t.here)
            ),
            (Ordering::Equal, Direction::Push) if t.newest_here > t.newest_there => format!(
                "{} newer changes on each computer, and the newest change is here",
                have(t.here)
            ),
            (Ordering::Equal, Direction::Push) => format!(
                "{} newer changes on each computer, and on a tie it pushes",
                have(t.here)
            ),
        };
        let mut said = format!("Sync chose to {verb}: {why}.");
        // A removal is only inferred from the files' times, so the user gets
        // to check it.
        if t.removed_here + t.removed_there > 0 {
            said.push_str(&format!(
                " Removed since the last transfer: {} here, {} on {peer}.",
                none_or(t.removed_here),
                none_or(t.removed_there)
            ));
        }
        said
    }

    /// Said when comparing a sync again sends it the other way.
    pub fn turned_around(&self) -> String {
        let (now, was) = match self.direction {
            Direction::Push => ("pushes", "pulling"),
            Direction::Pull => ("pulls", "pushing"),
        };
        format!(
            "Sync now {now} instead of {was}: the new ignore list changed which side has more \
             files with newer changes. Check the lists again before you confirm."
        )
    }
}

/// "1 file has", "7 files have".
fn have(n: u64) -> String {
    match n {
        1 => "1 file has".to_string(),
        n => format!("{n} files have"),
    }
}

fn none_or(n: u64) -> String {
    match n {
        0 => "none".to_string(),
        n => n.to_string(),
    }
}

#[cfg(test)]
#[path = "sync_count_tests.rs"]
mod tests;
