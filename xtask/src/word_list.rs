//! Keeps `words.txt` (every word in the repository's text files) and
//! `word-changes.txt` (the words this commit adds or removes) up to date,
//! and likewise `phrases.txt` and `phrase-changes.txt`. The pre-commit hook
//! runs this so a reviewer can spot odd terms in a commit without reading
//! the whole diff.

use crate::words::Found;
use anyhow::{Context, Result, bail};
use std::collections::BTreeSet;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};

pub const WORDS: &str = "words.txt";
pub const WORD_CHANGES: &str = "word-changes.txt";
pub const PHRASES: &str = "phrases.txt";
pub const PHRASE_CHANGES: &str = "phrase-changes.txt";

/// Paths left out. Only files git tracks are read, so build output and
/// `.git` are out already; these are tracked but generated.
const EXCLUDED: [&str; 5] = [WORDS, WORD_CHANGES, PHRASES, PHRASE_CHANGES, "Cargo.lock"];

/// How many entries a refresh added to and removed from each list.
pub struct Counts {
    pub words: (usize, usize),
    pub phrases: (usize, usize),
}

/// Rewrites the word and phrase lists from what is staged and stages them.
pub fn refresh(root: &Path) -> Result<Counts> {
    let mut found = Found::default();
    for (path, text) in staged_text_files(root)? {
        found.read(&text, uses_escapes(&path));
    }
    let (words, phrases) = found.lists();
    Ok(Counts {
        words: update(root, WORDS, WORD_CHANGES, &words)?,
        phrases: update(root, PHRASES, PHRASE_CHANGES, &phrases)?,
    })
}

/// Rewrites one list and, when its entries differ from the last commit's,
/// its file of changes, then stages them. Returns how many entries were
/// added and removed.
fn update(
    root: &Path,
    list: &str,
    changes_file: &str,
    entries: &BTreeSet<String>,
) -> Result<(usize, usize)> {
    let before: BTreeSet<String> = committed(root, list)?.lines().map(str::to_owned).collect();
    let changes = changes(&before, entries);
    write(&root.join(list), &lines(sorted(entries)))?;
    let mut staged = vec![list];
    // A commit that changes none of the entries leaves the last changes
    // alone, so the file only shows up in commits it describes.
    if !changes.is_empty() {
        write(
            &root.join(changes_file),
            &lines(changes.iter().map(String::as_str)),
        )?;
        staged.push(changes_file);
    }
    let mut args = vec!["add", "--"];
    args.extend(staged);
    crate::version::git(root, &args)?;
    let added = changes.iter().filter(|c| c.starts_with('+')).count();
    Ok((added, changes.len() - added))
}

/// `+entry` for each new entry and `-entry` for each gone one, in list order.
pub fn changes(before: &BTreeSet<String>, after: &BTreeSet<String>) -> Vec<String> {
    let mut out: Vec<(&str, char)> = after
        .difference(before)
        .map(|w| (w.as_str(), '+'))
        .collect();
    out.extend(before.difference(after).map(|w| (w.as_str(), '-')));
    out.sort_by_cached_key(|&(w, _)| order(w));
    out.into_iter()
        .map(|(w, sign)| format!("{sign}{w}"))
        .collect()
}

fn sorted(entries: &BTreeSet<String>) -> impl Iterator<Item = &str> {
    let mut sorted: Vec<&str> = entries.iter().map(String::as_str).collect();
    sorted.sort_by_cached_key(|&entry| order(entry));
    sorted.into_iter()
}

/// Alphabetical whatever the case, so `macOS` sits among the m's rather than
/// after every capital. Entries that differ only in case put capitals first.
fn order(entry: &str) -> (String, &str) {
    (entry.to_lowercase(), entry)
}

pub fn excluded(path: &str) -> bool {
    EXCLUDED.contains(&path)
}

/// Rust and TOML strings use backslash escapes.
fn uses_escapes(path: &str) -> bool {
    path.ends_with(".rs") || path.ends_with(".toml")
}

fn lines<'a>(items: impl Iterator<Item = &'a str>) -> String {
    items.map(|w| format!("{w}\n")).collect()
}

/// Temp file then rename, so an interrupted run never leaves half a list.
fn write(path: &Path, text: &str) -> Result<()> {
    let temp = path.with_extension("txt.tmp");
    std::fs::write(&temp, text).with_context(|| format!("could not write {}", temp.display()))?;
    std::fs::rename(&temp, path).with_context(|| format!("could not replace {}", path.display()))
}

/// The file as of the last commit, or empty before it exists.
fn committed(root: &Path, path: &str) -> Result<String> {
    let has_head = crate::version::git(root, &["rev-parse", "--verify", "--quiet", "HEAD"]).is_ok();
    if !has_head {
        return Ok(String::new());
    }
    Ok(crate::version::git(root, &["show", &format!("HEAD:{path}")]).unwrap_or_default())
}

/// Each staged file that is text, read from the index rather than the working
/// tree, so a partly staged file counts only what will be committed.
fn staged_text_files(root: &Path) -> Result<Vec<(String, String)>> {
    let listed = crate::version::git(root, &["ls-files", "-z", "--cached"])?;
    let paths: Vec<String> = listed
        .split('\0')
        .filter(|p| !p.is_empty() && !excluded(p) && !p.contains('\n'))
        .map(str::to_owned)
        .collect();
    let mut child = Command::new("git")
        .current_dir(root)
        .args(["cat-file", "--batch"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .context("could not run git cat-file")?;
    let mut stdin = child.stdin.take().context("git cat-file has no input")?;
    let requests: String = paths.iter().map(|p| format!(":{p}\n")).collect();
    // Written from another thread: git answers as it reads, and a full output
    // pipe would otherwise stop both sides.
    let writer = std::thread::spawn(move || stdin.write_all(requests.as_bytes()));
    let mut out = BufReader::new(child.stdout.take().context("git cat-file has no output")?);
    let mut files = Vec::new();
    for path in paths {
        let mut header = String::new();
        out.read_line(&mut header)?;
        let fields: Vec<&str> = header.split_whitespace().collect();
        let [_, kind, size] = fields.as_slice() else {
            // "missing": a submodule or a path git can't show; nothing to read.
            continue;
        };
        let size: usize = size
            .parse()
            .with_context(|| format!("odd git header {header:?}"))?;
        let mut body = vec![0; size + 1];
        out.read_exact(&mut body)?;
        body.pop();
        if *kind == "blob"
            && let Some(text) = as_text(body)
        {
            files.push((path, text));
        }
    }
    writer.join().expect("writer thread panicked")?;
    if !child.wait()?.success() {
        bail!("git cat-file failed");
    }
    Ok(files)
}

/// Fonts, images and other binary files have a zero byte or aren't UTF-8.
fn as_text(bytes: Vec<u8>) -> Option<String> {
    if bytes.contains(&0) {
        return None;
    }
    String::from_utf8(bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(words: &[&str]) -> BTreeSet<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn lists_added_and_removed_words_in_word_order() {
        let before = set(&["apple", "kiwi", "pear"]);
        let after = set(&["banana", "kiwi", "zucchini"]);
        assert_eq!(
            changes(&before, &after),
            ["-apple", "+banana", "-pear", "+zucchini"]
        );
        assert!(changes(&after, &after).is_empty());
    }

    #[test]
    fn orders_changes_alphabetically_whatever_the_case() {
        let before = set(&["kiwi"]);
        let after = set(&["Zebra", "macOS", "MacOS"]);
        assert_eq!(
            changes(&before, &after),
            ["-kiwi", "+MacOS", "+macOS", "+Zebra"]
        );
    }

    #[test]
    fn leaves_out_generated_files_and_binaries() {
        assert!(excluded("words.txt") && excluded("word-changes.txt") && excluded("Cargo.lock"));
        assert!(excluded("phrases.txt") && excluded("phrase-changes.txt"));
        assert!(!excluded("README.md") && !excluded("docs/words.txt"));
        assert_eq!(as_text(b"\x00\x01font".to_vec()), None);
        assert_eq!(as_text(vec![0xff, 0xfe]), None);
        assert_eq!(as_text(b"plain".to_vec()).as_deref(), Some("plain"));
    }

    /// An empty git repository in a temporary folder.
    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for args in [
            &["init", "-q"][..],
            &["config", "user.email", "t@example.com"],
            &["config", "user.name", "t"],
        ] {
            crate::version::git(dir.path(), args).unwrap();
        }
        dir
    }

    #[test]
    fn reads_only_what_is_staged() {
        let dir = repo();
        let root = dir.path();
        let git = |args: &[&str]| crate::version::git(root, args).unwrap();
        std::fs::write(root.join(".gitignore"), "ignored.txt\n").unwrap();
        std::fs::write(root.join("ignored.txt"), "secretword").unwrap();
        std::fs::write(root.join("untracked.md"), "untrackedword").unwrap();
        std::fs::write(root.join("a.md"), "Hello one-time world").unwrap();
        std::fs::write(root.join("b.bin"), b"binaryword\x00").unwrap();
        git(&["add", ".gitignore", "a.md", "b.bin"]);
        // Unstaged edits are not part of the commit and must not count.
        std::fs::write(root.join("a.md"), "Hello one-time world unstagedword").unwrap();

        assert_eq!(refresh(root).unwrap().words, (5, 0));
        let words = std::fs::read_to_string(root.join(WORDS)).unwrap();
        assert_eq!(words, "hello\nignored\none-time\ntxt\nworld\n");
        let changes = std::fs::read_to_string(root.join(WORD_CHANGES)).unwrap();
        assert!(changes.starts_with("+hello\n+ignored\n"));
        git(&["commit", "-q", "-m", "first"]);

        std::fs::write(root.join("a.md"), "Hello there").unwrap();
        git(&["add", "a.md"]);
        assert_eq!(refresh(root).unwrap().words, (1, 2));
        let changes = std::fs::read_to_string(root.join(WORD_CHANGES)).unwrap();
        assert_eq!(changes, "-one-time\n+there\n-world\n");
        let staged = git(&["diff", "--cached", "--name-only"]);
        assert_eq!(staged, "a.md\nword-changes.txt\nwords.txt\n");
    }

    #[test]
    fn lists_phrases_and_the_latest_phrase_changes() {
        let dir = repo();
        let root = dir.path();
        let git = |args: &[&str]| crate::version::git(root, args).unwrap();
        let read = |file: &str| std::fs::read_to_string(root.join(file)).unwrap();
        let stage = |text: &str| {
            std::fs::write(root.join("a.md"), text).unwrap();
            git(&["add", "a.md"]);
        };
        // History from before the phrase list existed, as in this repository.
        stage("Hello");
        git(&["commit", "-q", "-m", "before phrases"]);
        stage("WorldPeeps on macOS");
        assert_eq!(refresh(root).unwrap().phrases, (2, 0));
        assert_eq!(read(PHRASES), "macOS\nWorldPeeps\n");
        assert_eq!(read(PHRASE_CHANGES), "+macOS\n+WorldPeeps\n");
        git(&["commit", "-q", "-m", "first"]);

        // New words alone leave the last phrase changes where they are.
        stage("WorldPeeps live on macOS");
        assert_eq!(refresh(root).unwrap().phrases, (0, 0));
        let staged = git(&["diff", "--cached", "--name-only"]);
        assert_eq!(staged, "a.md\nword-changes.txt\nwords.txt\n");
        git(&["commit", "-q", "-m", "second"]);

        stage("Hello");
        assert_eq!(refresh(root).unwrap().phrases, (0, 2));
        assert_eq!(read(PHRASES), "");
        assert_eq!(read(PHRASE_CHANGES), "-macOS\n-WorldPeeps\n");
    }
}
