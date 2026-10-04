//! Writes a mirror to disk without ever leaving a half-written file.

use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const TEMP_SUFFIX: &str = ".pt-tmp";
/// Keeps "<name>.<rand>.pt-tmp" under the usual 255-byte name limit.
const TEMP_NAME_KEEP: usize = 200;

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

fn invalid(msg: String) -> io::Error {
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

fn ms_to_filetime(ms: i64) -> filetime::FileTime {
    filetime::FileTime::from_unix_time(
        ms.div_euclid(1000),
        (ms.rem_euclid(1000) * 1_000_000) as u32,
    )
}

/// Windows refuses to delete a read-only file or rename over one, while the
/// mirror must replace it like any other; elsewhere the folder's permissions
/// decide and the file's own mode doesn't matter.
#[cfg(windows)]
fn make_writable(path: &Path) {
    if let Ok(m) = std::fs::symlink_metadata(path)
        && m.is_file()
        && m.permissions().readonly()
    {
        let mut p = m.permissions();
        // Only the Windows read-only attribute changes; there is no unix mode here.
        #[allow(clippy::permissions_set_readonly_false)]
        p.set_readonly(false);
        let _ = std::fs::set_permissions(path, p);
    }
}

#[cfg(not(windows))]
fn make_writable(_path: &Path) {}

/// Removes whatever is at `path` without following a link. Missing is fine.
fn clear(path: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => {
            make_writable(path);
            std::fs::remove_file(path)
        }
        // A file where a parent folder should be also means nothing to remove.
        Err(e)
            if matches!(
                e.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
            ) =>
        {
            Ok(())
        }
        Err(e) => Err(e),
    }
}

fn temp_beside(dest: &Path) -> PathBuf {
    let name = dest
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut cut = name.len().min(TEMP_NAME_KEEP);
    while !name.is_char_boundary(cut) {
        cut -= 1;
    }
    let rand = &uuid::Uuid::new_v4().simple().to_string()[..8];
    dest.with_file_name(format!("{}.{rand}{TEMP_SUFFIX}", &name[..cut]))
}

pub struct Applier {
    root: PathBuf,
}

/// A file being received. Dropping it without `finish` deletes the temp file,
/// so the destination keeps its old content.
pub struct PendingFile {
    file: Option<File>,
    tmp: PathBuf,
    dest: PathBuf,
}

impl Applier {
    pub fn new(root: PathBuf) -> Applier {
        Applier { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn begin_file(&self, rel: &str) -> io::Result<PendingFile> {
        let dest = safe_join(&self.root, rel)?;
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = temp_beside(&dest);
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)?;
        Ok(PendingFile {
            file: Some(file),
            tmp,
            dest,
        })
    }

    pub fn make_dir(&self, rel: &str) -> io::Result<()> {
        let path = safe_join(&self.root, rel)?;
        match std::fs::symlink_metadata(&path) {
            Ok(m) if m.is_dir() => return Ok(()),
            Ok(_) => {
                make_writable(&path);
                std::fs::remove_file(&path)?
            }
            Err(_) => {}
        }
        std::fs::create_dir_all(&path)
    }

    pub fn make_symlink(&self, rel: &str, target: &str) -> io::Result<()> {
        let path = safe_join(&self.root, rel)?;
        #[cfg(unix)]
        {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            if std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_dir()) {
                std::fs::remove_dir_all(&path)?;
            }
            // Made aside and renamed in, so the old entry stays until the new one exists.
            let tmp = temp_beside(&path);
            std::os::unix::fs::symlink(target, &tmp)?;
            std::fs::rename(&tmp, &path).inspect_err(|_| {
                let _ = std::fs::remove_file(&tmp);
            })
        }
        #[cfg(not(unix))]
        {
            let _ = (path, target);
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "symbolic links are skipped on Windows",
            ))
        }
    }

    pub fn set_mtime(&self, rel: &str, mtime_ms: i64) -> io::Result<()> {
        let path = safe_join(&self.root, rel)?;
        if !std::fs::symlink_metadata(&path)?.is_file() {
            return Err(invalid(format!("\"{rel}\" is not a file")));
        }
        filetime::set_file_mtime(&path, ms_to_filetime(mtime_ms))
    }

    /// Removes a file, link or whole folder; `is_dir` is the sender's view
    /// and the disk decides, since the mirror wants it gone either way.
    pub fn remove(&self, rel: &str, is_dir: bool) -> io::Result<()> {
        let _ = is_dir;
        clear(&safe_join(&self.root, rel)?)
    }

    pub fn sweep_temp(&self) -> io::Result<u64> {
        fn walk(dir: &Path) -> io::Result<u64> {
            let mut n = 0;
            for item in std::fs::read_dir(dir)? {
                let item = item?;
                let ft = item.file_type()?;
                if ft.is_dir() {
                    n += walk(&item.path())?;
                } else if item.file_name().to_string_lossy().ends_with(TEMP_SUFFIX) {
                    std::fs::remove_file(item.path())?;
                    n += 1;
                }
            }
            Ok(n)
        }
        if !self.root.is_dir() {
            return Ok(0);
        }
        walk(&self.root)
    }
}

impl PendingFile {
    pub fn write(&mut self, b: &[u8]) -> io::Result<()> {
        match &mut self.file {
            Some(f) => f.write_all(b),
            None => Err(io::Error::other("the file was already finished")),
        }
    }

    pub fn finish(mut self, mtime_ms: i64, exec: bool) -> io::Result<()> {
        let file = self
            .file
            .take()
            .ok_or_else(|| io::Error::other("the file was already finished"))?;
        file.sync_all()?;
        set_exec(&file, exec)?;
        drop(file);
        // Set after closing so no later write can move it.
        filetime::set_file_mtime(&self.tmp, ms_to_filetime(mtime_ms))?;
        if std::fs::symlink_metadata(&self.dest).is_ok_and(|m| m.is_dir()) {
            std::fs::remove_dir_all(&self.dest)?;
        }
        make_writable(&self.dest);
        std::fs::rename(&self.tmp, &self.dest)
    }
}

#[cfg(unix)]
fn set_exec(file: &File, exec: bool) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = file.metadata()?.permissions();
    let mode = perms.mode();
    // Execute follows read, so the umask that shaped read still applies.
    let mode = if exec {
        mode | ((mode & 0o444) >> 2)
    } else {
        mode & !0o111
    };
    perms.set_mode(mode);
    file.set_permissions(perms)
}

#[cfg(not(unix))]
fn set_exec(_file: &File, _exec: bool) -> io::Result<()> {
    Ok(())
}

impl Drop for PendingFile {
    fn drop(&mut self) {
        // Still open means not finished; after a successful rename the temp
        // path no longer exists and this is a no-op.
        self.file.take();
        if std::fs::symlink_metadata(&self.tmp).is_ok() {
            let _ = std::fs::remove_file(&self.tmp);
        }
    }
}

#[cfg(test)]
#[path = "apply_tests.rs"]
mod tests;
