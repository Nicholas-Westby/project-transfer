//! Macs and Windows PCs usually ignore case in names, and a name keeps the
//! case it was first written in: writing `README.md` over `readme.md`
//! changes the file but not its name. A name changed only in case on the
//! other computer would never arrive, and every transfer would show it again.

use std::path::Path;
use tracing::{info, warn};

/// Renames what is at `path` to the case `path` spells it in, when the disk
/// holds it under a name differing only by case. `path` must not be a link,
/// which finding the name on disk would follow.
pub(super) fn match_case(path: &Path) {
    let (Some(asked), Ok(real)) = (path.file_name(), std::fs::canonicalize(path)) else {
        return;
    };
    let Some(held) = real.file_name() else {
        return;
    };
    // Only case: a Mac also opens a name by another Unicode spelling, and
    // the spelling it holds is kept on purpose.
    let same_letters =
        held.to_string_lossy().to_lowercase() == asked.to_string_lossy().to_lowercase();
    if held == asked || !same_letters {
        return;
    }
    let from = path.with_file_name(held);
    match std::fs::rename(&from, path) {
        Ok(()) => info!(
            "renamed {} to {asked:?}, as the copy sent spells it",
            from.display()
        ),
        // The content arrived; only the case is off, so the transfer goes on.
        Err(e) => warn!("could not rename {} to {asked:?}: {e}", from.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_differing_only_by_case_takes_the_case_asked_for() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("readme.md"), "r").unwrap();
        let asked = dir.path().join("README.md");
        if !asked.exists() {
            return; // This disk tells case apart.
        }
        match_case(&asked);
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["README.md"]);
    }

    #[test]
    fn another_spelling_of_the_same_letters_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let decomposed = "soldu\u{301}.jpg";
        std::fs::write(dir.path().join(decomposed), "s").unwrap();
        let composed = dir.path().join("sold\u{fa}.jpg");
        match_case(&composed);
        let names: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, [decomposed]);
    }
}
