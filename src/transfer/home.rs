//! Folders under the home folder but outside the Projects folder, such as
//! `~/.microsoft/usersecrets/<id>`. Tools look for those at one place under
//! home, so on the other computer they land at the same place under its home
//! rather than in its Projects folder. Also the checks every folder path
//! must pass, wherever it was chosen.

use super::validate_rel;
use crate::model::{FolderId, Project};
use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

/// This user's home folder, if the system says where it is.
pub fn real_home() -> Option<PathBuf> {
    directories::BaseDirs::new()
        .map(|b| b.home_dir().to_path_buf())
        .filter(|h| !h.as_os_str().is_empty())
}

/// `path` relative to `home`, '/' separated, when it lies inside the home
/// folder but not inside the Projects folder and is not the home itself.
/// Folders in the Projects folder follow the other computer's own layout.
pub fn home_hint(path: &Path, projects_folder: &Path, home: &Path) -> Option<String> {
    if path.starts_with(projects_folder) {
        return None;
    }
    let rest = path.strip_prefix(home).ok()?;
    let parts = rest
        .components()
        .map(|c| match c {
            Component::Normal(s) => s.to_str(),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;
    let rel = parts.join("/");
    check_hint(&rel)?;
    Some(rel)
}

/// The wire form both ends hold a hint to: '/' separated parts that are
/// each one plain name. A colon is refused too, since Windows reads it as a
/// drive or a stream.
fn check_hint(rel: &str) -> Option<()> {
    validate_rel(rel).ok()?;
    (!rel.contains(':')).then_some(())
}

/// The hints for every folder of `p` this computer has a path for.
pub fn hints_for(
    p: &Project,
    projects_folder: &Path,
    home: Option<&Path>,
) -> HashMap<FolderId, String> {
    let Some(home) = home else {
        return HashMap::new();
    };
    p.folders
        .iter()
        .filter_map(|f| {
            let hint = home_hint(f.local_path.as_deref()?, projects_folder, home)?;
            Some((f.id, hint))
        })
        .collect()
}

/// Where a hint from the other computer points on this one. A hint that
/// fails the checks for a path from the network is ignored, so the folder
/// gets the usual default.
pub fn resolve(home: Option<&Path>, hint: &str) -> Option<PathBuf> {
    let home = home?;
    check_hint(hint)?;
    // Part by part, so the path is built with this system's separator and
    // compares equal to the same path built anywhere else here.
    Some(
        hint.split('/')
            .fold(home.to_path_buf(), |p, part| p.join(part)),
    )
}

pub fn resolve_all(
    hints: &HashMap<FolderId, String>,
    home: Option<&Path>,
) -> HashMap<FolderId, PathBuf> {
    hints
        .iter()
        .filter_map(|(id, h)| Some((*id, resolve(home, h)?)))
        .collect()
}

/// `path` as people read it: under the home folder it starts with `~`.
pub fn shown(path: &Path, home: Option<&Path>) -> String {
    if let Some(home) = home
        && let Ok(rest) = path.strip_prefix(home)
    {
        if rest.as_os_str().is_empty() {
            return "~".into();
        }
        return format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display());
    }
    path.display().to_string()
}

/// A typed path, with a leading `~` standing for the home folder.
pub fn expand(text: &str, home: Option<&Path>) -> PathBuf {
    let rest = match text.strip_prefix('~') {
        Some(r) if r.is_empty() || r.starts_with(['/', '\\']) => r.trim_start_matches(['/', '\\']),
        _ => return PathBuf::from(text),
    };
    match home {
        Some(h) if rest.is_empty() => h.to_path_buf(),
        Some(h) => h.join(rest),
        None => PathBuf::from(text),
    }
}

/// A project folder is mirrored, deletions included, so the home folder or a
/// whole disk is refused: one wrong push would rewrite everything on it.
pub fn too_broad(path: &Path, home: Option<&Path>) -> Option<String> {
    let real = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let home = home.map(|h| h.canonicalize().unwrap_or_else(|_| h.to_path_buf()));
    if home.as_deref() == Some(real.as_path()) {
        return Some(format!(
            "{} is the home folder. Choose the project's own folder inside it.",
            path.display()
        ));
    }
    if real.parent().is_none() || is_mount_point(&real) {
        return Some(format!(
            "{} is the top of a disk. Choose the project's own folder on it.",
            path.display()
        ));
    }
    None
}

/// Fails when `path` is, contains or sits inside a folder already in a
/// project, other than `except`: a transfer to one would rewrite the other.
pub fn check_overlap(all: &[Project], path: &Path, except: Option<FolderId>) -> Result<(), String> {
    for p in all {
        for f in p.folders.iter().filter(|f| Some(f.id) != except) {
            let Some(other) = &f.local_path else { continue };
            if path.starts_with(other) || other.starts_with(path) {
                return Err(format!(
                    "{} overlaps {}, folder `{}` of project `{}`. Choose a folder that is not \
                     part of another project.",
                    path.display(),
                    other.display(),
                    f.name,
                    p.name
                ));
            }
        }
    }
    Ok(())
}

/// A folder on another device than its parent is where a disk is mounted,
/// such as `/Volumes/USB` on a Mac.
#[cfg(unix)]
fn is_mount_point(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    let (Some(parent), Ok(me)) = (path.parent(), std::fs::metadata(path)) else {
        return false;
    };
    std::fs::metadata(parent).is_ok_and(|p| p.dev() != me.dev())
}

/// Drive roots and shares have no parent, which `too_broad` checks already.
#[cfg(not(unix))]
fn is_mount_point(_path: &Path) -> bool {
    false
}

#[cfg(test)]
#[path = "home_tests.rs"]
mod tests;
