//! Writes a mirror to disk without ever leaving a half-written file.

use super::paths::{invalid, safe_join};
use std::collections::HashSet;
use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use tracing::info;

const TEMP_SUFFIX: &str = ".pt-tmp";
/// Keeps "<name>.<rand>.pt-tmp" under the usual 255-byte name limit.
const TEMP_NAME_KEEP: usize = 200;

fn ms_to_filetime(ms: i64) -> filetime::FileTime {
    filetime::FileTime::from_unix_time(
        ms.div_euclid(1000),
        (ms.rem_euclid(1000) * 1_000_000) as u32,
    )
}

/// Windows refuses to delete a read-only file, rename over one or open one
/// for writing (which setting its time does), and Git makes its object files
/// read-only; the mirror must change them like any other. Elsewhere the
/// file's own mode stops none of these.
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

/// What the disk calls a file or folder, whatever spelling reached it.
#[cfg(unix)]
fn identity(m: &std::fs::Metadata) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    Some((m.dev(), m.ino()))
}

/// Windows tells spellings apart except by case, and the preview already
/// keeps back removals that differ only by case.
#[cfg(not(unix))]
fn identity(_: &std::fs::Metadata) -> Option<(u64, u64)> {
    None
}

/// Looks at the entry itself and never at what a link points to, since a
/// removal takes the link away, not its target.
fn identity_at(path: &Path) -> Option<(u64, u64)> {
    identity(&std::fs::symlink_metadata(path).ok()?)
}

type Written = Arc<Mutex<HashSet<(u64, u64)>>>;

/// A lock poisoned by a panic elsewhere must not stop a removal.
fn lock(written: &Written) -> MutexGuard<'_, HashSet<(u64, u64)>> {
    written.lock().unwrap_or_else(|p| p.into_inner())
}

/// Notes what is at `path` as written, once it is in place.
fn note(written: &Written, path: &Path) {
    if let Some(id) = identity_at(path) {
        lock(written).insert(id);
    }
}

pub struct Applier {
    root: PathBuf,
    /// Files and folders this transfer wrote or made, by what the disk calls
    /// them. Another spelling of a name (case, or composed against decomposed
    /// letters on a Mac) can reach the same file, and removing that spelling
    /// must not delete what was just written.
    written: Written,
}

/// A file being received. Dropping it without `finish` deletes the temp file,
/// so the destination keeps its old content.
pub struct PendingFile {
    file: Option<File>,
    tmp: PathBuf,
    dest: PathBuf,
    written: Written,
}

impl Applier {
    pub fn new(root: PathBuf) -> Applier {
        Applier {
            root,
            written: Written::default(),
        }
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
            written: self.written.clone(),
        })
    }

    pub fn make_dir(&self, rel: &str) -> io::Result<()> {
        let path = safe_join(&self.root, rel)?;
        match std::fs::symlink_metadata(&path) {
            Ok(m) if m.is_dir() => {
                note(&self.written, &path);
                return Ok(());
            }
            Ok(_) => {
                make_writable(&path);
                std::fs::remove_file(&path)?
            }
            Err(_) => {}
        }
        std::fs::create_dir_all(&path)?;
        note(&self.written, &path);
        Ok(())
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
            })?;
            note(&self.written, &path);
            Ok(())
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
        make_writable(&path);
        filetime::set_file_mtime(&path, ms_to_filetime(mtime_ms))
    }

    /// Removes a file, link or whole folder; `is_dir` is the sender's view
    /// and the disk decides, since the mirror wants it gone either way.
    /// Whatever this transfer wrote stays, under any spelling.
    pub fn remove(&self, rel: &str, is_dir: bool) -> io::Result<()> {
        let _ = is_dir;
        let path = safe_join(&self.root, rel)?;
        if identity_at(&path).is_some_and(|id| lock(&self.written).contains(&id)) {
            info!(
                "kept {rel:?} in {}: this transfer just wrote it under another spelling",
                self.root.display()
            );
            return Ok(());
        }
        clear(&path)
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
        std::fs::rename(&self.tmp, &self.dest)?;
        note(&self.written, &self.dest);
        Ok(())
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
