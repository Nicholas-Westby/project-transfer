//! Temp files an unfinished transfer left, which the next transfer into the
//! folder clears. The scan for its preview found them, so nothing walks the
//! folder again for them.

use crate::ignore_rules::TEMP_SUFFIX;
use std::io;
use std::path::{Path, PathBuf};

/// Removes the temp files an unfinished transfer left in `root`, as a scan
/// of it found them. A path that is no longer a temp file there is left
/// alone, and so is one below a folder that became a link since, which could
/// lead out of `root`. Every one is tried; the first error comes back.
pub fn clear_leftovers(root: &Path, found: &[PathBuf]) -> io::Result<u64> {
    let (mut n, mut first) = (0, None);
    for p in found {
        let named = p
            .file_name()
            .is_some_and(|f| f.to_string_lossy().ends_with(TEMP_SUFFIX));
        let temp = named && p.starts_with(root) && no_link_between(root, p);
        if temp && std::fs::symlink_metadata(p).is_ok_and(|m| !m.is_dir()) {
            match std::fs::remove_file(p) {
                Ok(()) => n += 1,
                Err(e) => {
                    first.get_or_insert(e);
                }
            }
        }
    }
    first.map_or(Ok(n), Err)
}

/// Whether every folder between `root` and `p` is still a folder. The disk
/// follows a link anywhere on the way, which `p`'s own metadata can't show.
fn no_link_between(root: &Path, p: &Path) -> bool {
    p.ancestors()
        .skip(1)
        .take_while(|a| *a != root)
        .all(|a| std::fs::symlink_metadata(a).is_ok_and(|m| m.is_dir()))
}

#[cfg(test)]
#[path = "leftovers_tests.rs"]
mod tests;
