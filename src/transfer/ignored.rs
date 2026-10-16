//! The files inside ignored folders, which a scan names without walking:
//! counting them all cost seconds per preview on a large project, though a
//! transfer only needs the few that a removed folder takes along.

use super::{safe_join, validate_rel};
use crate::ignore_rules::Matcher;
use crate::manifest::{Kind, Manifest};
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};

/// `manifest::scan`, with each ignored folder whose path can't travel in a
/// request (see `validate_rel`) counted at once, as scans used to count all
/// of them, so another computer can ask about any folder still named.
pub fn scan(root: &Path, matcher: &Matcher, compose: bool) -> anyhow::Result<Manifest> {
    let mut m = crate::manifest::scan(root, matcher, compose)?;
    let stuck: Vec<String> = m
        .ignored_dirs
        .iter()
        .filter(|d| validate_rel(d).is_err())
        .cloned()
        .collect();
    let counts = count_own(root, &stuck)?;
    add_counts(&mut m, &counts);
    Ok(m)
}

/// The ignored folders in `dst` that a transfer from `src` removes: those
/// inside a folder `src` doesn't have as a folder, which goes with all it
/// holds. Any other ignored folder stays as it is.
pub fn needed(src: &Manifest, dst: &Manifest) -> Vec<String> {
    let dirs: HashSet<&str> = src
        .entries
        .iter()
        .filter(|e| e.kind == Kind::Dir)
        .map(|e| e.rel.as_str())
        .collect();
    dst.ignored_dirs
        .iter()
        .filter(|d| ancestors(d).any(|a| !dirs.contains(a)))
        .cloned()
        .collect()
}

/// Adds the files counted in ignored folders to the ignored count of every
/// folder around them, as a scan does for each ignored file it passes. A
/// count is used only for a folder still in `ignored_dirs`, which it then
/// leaves, so a reply naming any other folder changes nothing and counting
/// again can't add a folder twice.
pub fn add_counts(m: &mut Manifest, counts: &HashMap<String, u64>) {
    for dir in std::mem::take(&mut m.ignored_dirs) {
        match counts.get(&dir) {
            None => m.ignored_dirs.push(dir),
            Some(0) => {}
            Some(&n) => {
                for a in std::iter::once("").chain(ancestors(&dir)) {
                    *m.ignored_in_dir.entry(a.to_string()).or_default() += n;
                }
            }
        }
    }
}

/// The files in each of `dirs`, folders another computer named inside
/// `root`, at any depth. A folder gone since the scan holds none.
pub fn count(root: &Path, dirs: &[String]) -> io::Result<HashMap<String, u64>> {
    count_by(root, dirs, safe_join)
}

/// As `count`, for folders this computer's own scan named. Their names came
/// from its disk, so each is joined as it is, even one no request can carry.
pub fn count_own(root: &Path, dirs: &[String]) -> io::Result<HashMap<String, u64>> {
    count_by(root, dirs, |root, rel| Ok(own_path(root, rel)))
}

/// Counts every ignored folder `m` names, for an older version that
/// expects the counts to hold them all.
pub fn count_all(root: &Path, m: &mut Manifest) -> io::Result<()> {
    let counts = count_own(root, &m.ignored_dirs)?;
    add_counts(m, &counts);
    Ok(())
}

/// What a preview stops with when counting failed on `computer`, which may
/// not be the computer that shows it: what went wrong, then what to do.
pub fn count_failed(e: &io::Error, computer: &str) -> String {
    let what = e.to_string();
    let what = what.trim_end_matches('.');
    format!("{what}. Make it readable on {computer}, or remove it, then preview again.")
}

fn count_by(
    root: &Path,
    dirs: &[String],
    join: impl Fn(&Path, &str) -> io::Result<PathBuf>,
) -> io::Result<HashMap<String, u64>> {
    dirs.iter()
        .map(|d| {
            let n = join(root, d).and_then(|path| match std::fs::symlink_metadata(&path) {
                Ok(m) if m.is_dir() => count_files(&path),
                _ => Ok(0),
            })?;
            Ok((d.clone(), n))
        })
        .collect()
}

/// The files at any depth in `dir`. A folder gone by the time it is read,
/// as when a build clears `target`, holds none.
fn count_files(dir: &Path) -> io::Result<u64> {
    let mut inside = Vec::new();
    let listed = std::fs::read_dir(dir).and_then(|items| {
        let mut n = 0;
        for item in items {
            let item = item?;
            if item.file_type()?.is_dir() {
                inside.push(item.path());
            } else {
                n += 1;
            }
        }
        Ok(n)
    });
    let n = match listed {
        Ok(n) => n,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
        // The error alone doesn't say which folder. Name the one that failed,
        // not the ignored folder being counted: it is often a root-owned one
        // deep inside, and it is the one to make readable or remove.
        Err(e) => {
            let why = format!("Could not read {}: {e}", dir.display());
            return Err(io::Error::new(e.kind(), why));
        }
    };
    inside.iter().try_fold(n, |n, d| Ok(n + count_files(d)?))
}

/// `rel` under `root`, one name at a time. A name read from this computer's
/// disk holds no separator, though it may hold a backslash or start like a
/// drive.
fn own_path(root: &Path, rel: &str) -> PathBuf {
    rel.split('/')
        .fold(root.to_path_buf(), |p, name| p.join(name))
}

/// The folders `rel` lies in, outermost first, without the root (""):
/// `a` and `a/b` for `a/b/c`.
fn ancestors(rel: &str) -> impl Iterator<Item = &str> {
    rel.match_indices('/').map(move |(i, _)| &rel[..i])
}

#[cfg(test)]
#[path = "ignored_tests.rs"]
mod tests;
