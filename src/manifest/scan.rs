use super::{Entry, Kind, Manifest};
use crate::ignore_rules::{Matcher, TEMP_SUFFIX};
use std::io::Read;
use std::path::Path;

/// Walks `root` without following symlinks or entering ignored folders.
/// Only `matcher` decides what is ignored; `.gitignore` files are
/// deliberately not read. With `compose`, a name stored decomposed is
/// listed composed (see `compose_for`).
pub fn scan(root: &Path, matcher: &Matcher, compose: bool) -> anyhow::Result<Manifest> {
    let mut m = Manifest::default();
    if std::fs::symlink_metadata(root).is_err() {
        return Ok(m);
    }
    // Every existing ancestor directory of a file gets its ignored count.
    let mut chain = vec![String::new()];
    walk(root, "", matcher, compose, &mut m, &mut chain)?;
    m.entries.sort_by(|a, b| a.rel.cmp(&b.rel));
    m.ignored_dirs.sort();
    Ok(m)
}

fn walk(
    dir: &Path,
    rel: &str,
    matcher: &Matcher,
    compose: bool,
    m: &mut Manifest,
    chain: &mut Vec<String>,
) -> anyhow::Result<()> {
    for item in std::fs::read_dir(dir)? {
        let item = item?;
        let meta = std::fs::symlink_metadata(item.path())?;
        let mut name = item.file_name().to_string_lossy().into_owned();
        if compose {
            name = super::spelling::composed(dir, name, &meta);
        }
        let child = if rel.is_empty() {
            name
        } else {
            format!("{rel}/{name}")
        };
        let ft = meta.file_type();
        if ft.is_dir() {
            if matcher.is_ignored(&child, true) {
                m.ignored_dirs.push(child);
                continue;
            }
            m.entries.push(Entry {
                rel: child.clone(),
                kind: Kind::Dir,
            });
            chain.push(child.clone());
            walk(&item.path(), &child, matcher, compose, m, chain)?;
            chain.pop();
        } else if matcher.is_ignored(&child, false) {
            if child.ends_with(TEMP_SUFFIX) {
                m.leftovers.push(item.path());
            }
            bump(m, chain, 1);
        } else if ft.is_symlink() {
            let target = std::fs::read_link(item.path())?
                .to_string_lossy()
                .replace('\\', "/");
            m.entries.push(Entry {
                rel: child,
                kind: Kind::Symlink { target },
            });
        } else {
            m.entries.push(Entry {
                rel: child,
                kind: Kind::File {
                    size: meta.len(),
                    mtime_ms: mtime_ms(&meta),
                    exec: is_exec(&meta),
                },
            });
        }
    }
    Ok(())
}

fn bump(m: &mut Manifest, chain: &[String], n: u64) {
    if n == 0 {
        return;
    }
    for d in chain {
        *m.ignored_in_dir.entry(d.clone()).or_default() += n;
    }
}

pub(crate) fn mtime_ms(meta: &std::fs::Metadata) -> i64 {
    match meta.modified() {
        Ok(t) => match t.duration_since(std::time::UNIX_EPOCH) {
            Ok(d) => d.as_millis() as i64,
            Err(e) => -(e.duration().as_millis() as i64),
        },
        Err(_) => 0,
    }
}

#[cfg(unix)]
pub(crate) fn is_exec(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
pub(crate) fn is_exec(_meta: &std::fs::Metadata) -> bool {
    false
}

pub fn hash_file(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut h = blake3::Hasher::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().to_hex().to_string())
}

#[cfg(test)]
#[path = "scan_tests.rs"]
mod tests;
