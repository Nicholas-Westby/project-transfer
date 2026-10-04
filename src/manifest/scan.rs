use super::{Entry, Kind, Manifest};
use crate::ignore_rules::Matcher;
use std::io::Read;
use std::path::Path;

/// Walks `root` without following symlinks. Only `matcher` decides what is
/// ignored; `.gitignore` files are deliberately not read.
pub fn scan(root: &Path, matcher: &Matcher) -> anyhow::Result<Manifest> {
    let mut m = Manifest::default();
    if std::fs::symlink_metadata(root).is_err() {
        return Ok(m);
    }
    // Every existing ancestor directory of a file gets its ignored count.
    let mut chain = vec![String::new()];
    walk(root, "", matcher, &mut m, &mut chain)?;
    m.entries.sort_by(|a, b| a.rel.cmp(&b.rel));
    Ok(m)
}

fn walk(
    dir: &Path,
    rel: &str,
    matcher: &Matcher,
    m: &mut Manifest,
    chain: &mut Vec<String>,
) -> anyhow::Result<()> {
    for item in std::fs::read_dir(dir)? {
        let item = item?;
        let name = item.file_name().to_string_lossy().into_owned();
        let child = if rel.is_empty() {
            name
        } else {
            format!("{rel}/{name}")
        };
        let meta = std::fs::symlink_metadata(item.path())?;
        let ft = meta.file_type();
        if ft.is_dir() {
            if matcher.is_ignored(&child, true) {
                let n = count_files(&item.path())?;
                bump(m, chain, n);
                continue;
            }
            m.entries.push(Entry {
                rel: child.clone(),
                kind: Kind::Dir,
            });
            chain.push(child.clone());
            walk(&item.path(), &child, matcher, m, chain)?;
            chain.pop();
        } else if matcher.is_ignored(&child, false) {
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

fn count_files(dir: &Path) -> anyhow::Result<u64> {
    let mut n = 0;
    for item in std::fs::read_dir(dir)? {
        let item = item?;
        if item.file_type()?.is_dir() {
            n += count_files(&item.path())?;
        } else {
            n += 1;
        }
    }
    Ok(n)
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
mod tests {
    use super::*;
    use crate::ignore_rules::{DEFAULT_IGNORES, IgnoreSpec};
    use crate::manifest::Kind;

    fn matcher() -> Matcher {
        Matcher::new(&IgnoreSpec {
            patterns: DEFAULT_IGNORES.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        })
        .unwrap()
    }

    fn write(root: &Path, rel: &str, body: &str) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, body).unwrap();
    }

    #[test]
    fn missing_root_is_empty() {
        let t = tempfile::tempdir().unwrap();
        let m = scan(&t.path().join("nope"), &matcher()).unwrap();
        assert_eq!(m, Manifest::default());
    }

    #[test]
    fn lists_files_and_empty_dirs_sorted() {
        let t = tempfile::tempdir().unwrap();
        write(t.path(), "src/main.rs", "fn main(){}");
        std::fs::create_dir(t.path().join("empty")).unwrap();
        let m = scan(t.path(), &matcher()).unwrap();
        let rels: Vec<_> = m.entries.iter().map(|e| e.rel.as_str()).collect();
        assert_eq!(rels, ["empty", "src", "src/main.rs"]);
        assert_eq!(m.entries[0].kind, Kind::Dir);
        match &m.entries[2].kind {
            Kind::File { size, mtime_ms, .. } => {
                assert_eq!(*size, 11);
                assert!(*mtime_ms > 0);
            }
            k => panic!("{k:?}"),
        }
    }

    #[test]
    fn ignored_files_are_counted_not_listed() {
        let t = tempfile::tempdir().unwrap();
        write(t.path(), "a/keep.txt", "k");
        write(t.path(), "a/node_modules/x/1.js", "1");
        write(t.path(), "a/node_modules/2.js", "2");
        write(t.path(), "a/.DS_Store", "d");
        write(t.path(), "b/.DS_Store", "d");
        let m = scan(t.path(), &matcher()).unwrap();
        let rels: Vec<_> = m.entries.iter().map(|e| e.rel.as_str()).collect();
        assert_eq!(rels, ["a", "a/keep.txt", "b"]);
        assert_eq!(m.ignored_in_dir.get("a"), Some(&3));
        assert_eq!(m.ignored_in_dir.get("b"), Some(&1));
        assert_eq!(m.ignored_in_dir.get(""), Some(&4));
        assert!(!m.ignored_in_dir.contains_key("a/node_modules"));
    }

    #[test]
    fn gitignore_files_are_not_read() {
        let t = tempfile::tempdir().unwrap();
        write(t.path(), ".gitignore", "secret.txt\n");
        write(t.path(), "secret.txt", "s");
        let m = scan(t.path(), &matcher()).unwrap();
        assert!(m.entries.iter().any(|e| e.rel == "secret.txt"));
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_recorded_not_followed() {
        use std::os::unix::fs::PermissionsExt;
        let t = tempfile::tempdir().unwrap();
        write(t.path(), "real/f.txt", "x");
        std::os::unix::fs::symlink("real", t.path().join("link")).unwrap();
        write(t.path(), "run.sh", "#!/bin/sh");
        std::fs::set_permissions(
            t.path().join("run.sh"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        let m = scan(t.path(), &matcher()).unwrap();
        let link = m.entries.iter().find(|e| e.rel == "link").unwrap();
        assert_eq!(
            link.kind,
            Kind::Symlink {
                target: "real".into()
            }
        );
        assert!(!m.entries.iter().any(|e| e.rel == "link/f.txt"));
        let sh = m.entries.iter().find(|e| e.rel == "run.sh").unwrap();
        assert!(matches!(sh.kind, Kind::File { exec: true, .. }));
    }

    #[test]
    fn hash_is_blake3_hex() {
        let t = tempfile::tempdir().unwrap();
        write(t.path(), "f", "hello");
        assert_eq!(
            hash_file(&t.path().join("f")).unwrap(),
            blake3::hash(b"hello").to_hex().to_string()
        );
    }
}
