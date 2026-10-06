//! Checks on paths that came from another computer, before any of them
//! touches the disk.

use std::io;
use std::path::{Path, PathBuf};

/// Checks a '/' separated path that came from another computer before it is
/// joined onto a folder. Returns a sentence saying what is wrong.
pub fn validate_rel(rel: &str) -> Result<(), String> {
    let bad = |why: &str| Err(format!("The path \"{rel}\" is not allowed: {why}."));
    if rel.is_empty() {
        return bad("it is empty");
    }
    if rel.contains('\0') {
        return bad("it contains a NUL character");
    }
    if rel.starts_with('/') || rel.starts_with('\\') {
        return bad("it is absolute");
    }
    let b = rel.as_bytes();
    if b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':' {
        return bad("it starts with a drive letter");
    }
    // Backslash separates on Windows, so it could hide a ".." or a link
    // from the checks below; such a name cannot exist there anyway.
    if rel.contains('\\') {
        return bad("it contains a backslash");
    }
    for part in rel.split('/') {
        match part {
            "" => return bad("it has an empty part"),
            "." | ".." => return bad("it has a \".\" or \"..\" part"),
            _ => {}
        }
        #[cfg(windows)]
        if crate::naming::windows_problem(part).is_some() {
            return bad("Windows cannot hold one of its names");
        }
    }
    Ok(())
}

/// Checks a project or folder name used as one path component.
pub fn validate_name(name: &str) -> Result<(), String> {
    validate_rel(name)?;
    if name.contains(['/', '\\']) {
        return Err(format!(
            "The name \"{name}\" is not allowed: it contains a slash."
        ));
    }
    Ok(())
}

pub(super) fn invalid(msg: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, msg)
}

/// Joins a validated `rel` onto `root` component by component, refusing a
/// path that passes through a symbolic link, which could lead outside `root`.
pub fn safe_join(root: &Path, rel: &str) -> io::Result<PathBuf> {
    validate_rel(rel).map_err(invalid)?;
    let parts: Vec<&str> = rel.split('/').collect();
    let mut path = root.to_path_buf();
    let mut checking = true;
    for (i, part) in parts.iter().enumerate() {
        path.push(part);
        if !checking || i + 1 == parts.len() {
            continue;
        }
        match std::fs::symlink_metadata(&path) {
            Ok(m) if m.file_type().is_symlink() => {
                return Err(invalid(format!(
                    "The path \"{rel}\" is not allowed: it passes through a symbolic link."
                )));
            }
            Ok(_) => {}
            // Nothing below a missing component can be a link.
            Err(_) => checking = false,
        }
    }
    Ok(path)
}

#[cfg(test)]
#[path = "paths_tests.rs"]
mod tests;
