//! Showing one file of a transfer's preview: each computer's copy read
//! as text, lined up side by side when both have it.

pub mod rows;
pub mod text;

use crate::manifest::{Change, Entry, Kind, mtime_ms};
use crate::model::{Direction, FolderId, InstanceId, ProjectId};
use crate::net::FilePart;
use crate::protocol::MAX_PART;
use crate::transfer::{Preview, safe_join};
use rows::{Row, align};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use text::{Body, Text, decode};

/// How long lining two copies up may take before it settles for a rougher
/// match.
const ALIGN_BUDGET: Duration = Duration::from_secs(2);

/// Which copies of a file a line of the preview is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Copies {
    /// Only this computer has it.
    Here,
    /// Only the other computer has it.
    There,
    /// Both have it, with different content.
    Both,
    /// Both have it with the same content; only the modified time differs.
    SameContent,
}

impl Copies {
    /// Which copies the preview change `c` is about, or None when it is not
    /// about one file on each side: a folder, a link, or an entry another
    /// kind of entry replaces.
    pub fn of(c: &Change, direction: Direction) -> Option<Copies> {
        let (source, dest) = match direction {
            Direction::Push => (Copies::Here, Copies::There),
            Direction::Pull => (Copies::There, Copies::Here),
        };
        let file = |e: &Entry| matches!(e.kind, Kind::File { .. });
        match c {
            Change::Add(e) if file(e) => Some(source),
            Change::Update { entry, .. } if file(entry) => Some(Copies::Both),
            Change::TimestampOnly(e) if file(e) => Some(Copies::SameContent),
            Change::RemoveFile(_) => Some(dest),
            _ => None,
        }
    }

    /// Whether this computer's copy is read.
    pub fn here(self) -> bool {
        self != Copies::There
    }

    /// Whether the other computer's copy is read. Same content needs only
    /// the one here.
    pub fn there(self) -> bool {
        matches!(self, Copies::There | Copies::Both)
    }
}

/// One file of the open preview, to show.
#[derive(Clone, Debug, PartialEq)]
pub struct FileAsk {
    pub peer: InstanceId,
    pub project: ProjectId,
    pub folder: FolderId,
    /// The folder on this computer, as the preview names it.
    pub local_root: PathBuf,
    /// The file's path inside the folder, '/' separated.
    pub rel: String,
    pub copies: Copies,
}

impl FileAsk {
    /// The file at `rel` in folder `folder`, an index into `p.folders`.
    pub fn new(p: &Preview, folder: usize, rel: &str, copies: Copies) -> FileAsk {
        let f = &p.folders[folder];
        let here = match p.request.direction {
            Direction::Push => &f.source_path,
            Direction::Pull => &f.dest_path,
        };
        FileAsk {
            peer: p.request.peer,
            project: p.request.project,
            folder: f.folder,
            local_root: PathBuf::from(here),
            rel: rel.to_string(),
            copies,
        }
    }
}

/// A file of the open preview, as the viewer shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct FileView {
    pub ask: FileAsk,
    /// Which opening this is; a read fills only the viewer it was for.
    pub opened: u64,
    /// None while the copies are read. Shared, so the window's copy of
    /// the state each frame doesn't copy the file.
    pub shown: Option<Arc<Shown>>,
}

/// What two copies whose lines all match still differ in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alike {
    /// Nothing that was read: the same bytes.
    Bytes,
    /// Only what the viewer doesn't show: line endings, a byte-order mark
    /// or the line break at the end.
    Lines,
}

/// One copy, ready to draw.
#[derive(Clone, Debug, PartialEq)]
pub struct Pane {
    pub size: u64,
    pub mtime_ms: i64,
    /// Only the start of the file was read.
    pub cut: bool,
    /// Bytes read from the start.
    pub shown: u64,
    pub body: Body,
}

/// Both copies of a file, ready to draw.
#[derive(Clone, Debug, PartialEq)]
pub struct Shown {
    /// This computer's copy, or why it can't be shown; None when the file
    /// is only on the other computer.
    pub here: Option<Result<Pane, String>>,
    pub there: Option<Result<Pane, String>>,
    /// The two copies lined up; empty unless both are text.
    pub rows: Vec<Row>,
    /// Set when both copies are text and every row matches.
    pub alike: Option<Alike>,
    /// Only the copy here so far; the other computer's is still read.
    pub waiting: bool,
}

impl Shown {
    /// Reads what was fetched as text and lines the copies up, which can
    /// take a moment for a large file: call it off the UI thread.
    pub fn build(
        here: Option<Result<FilePart, String>>,
        there: Option<Result<FilePart, String>>,
    ) -> Shown {
        // What matching lines would still differ in, while the bytes are
        // at hand.
        let kind = match (&here, &there) {
            (Some(Ok(a)), Some(Ok(b))) if a.bytes == b.bytes => Alike::Bytes,
            _ => Alike::Lines,
        };
        let here = here.map(|r| r.map(pane));
        let there = there.map(|r| r.map(pane));
        let rows = match (&here, &there) {
            (Some(Ok(a)), Some(Ok(b))) => match (&a.body, &b.body) {
                (Body::Text(ta), Body::Text(tb)) => {
                    let mut rows = align(&ta.lines(), &tb.lines(), ALIGN_BUDGET);
                    settle_cut_end(&mut rows, (ta, a.cut), (tb, b.cut));
                    Some(rows)
                }
                _ => None,
            },
            _ => None,
        };
        let alike = rows
            .as_ref()
            .filter(|rows| !rows.iter().any(|r| r.differs))
            .map(|_| kind);
        Shown {
            here,
            there,
            rows: rows.unwrap_or_default(),
            alike,
            waiting: false,
        }
    }
}

/// Past where a cut copy stops, rows at the end aren't known to differ: a
/// line of the other copy may sit in the part not read, and the cut copy's
/// last line may go on into the line it faces. A row that already differs
/// in what was read stays marked. `left` and `right` are (lines, cut).
fn settle_cut_end(rows: &mut [Row], left: (&Text, bool), right: (&Text, bool)) {
    for r in rows.iter_mut().rev() {
        let unknown = match (r.left, r.right) {
            (Some(_), None) => right.1,
            (None, Some(_)) => left.1,
            (Some(a), Some(b)) => {
                goes_on_into(left, a, right.0.line(b)) || goes_on_into(right, b, left.0.line(a))
            }
            (None, None) => false,
        };
        if !unknown {
            break;
        }
        r.differs = false;
    }
}

/// Whether line `n` is a cut copy's last and, as far as it was read, the
/// start of `other`. A `\r` the read ended on may be half a line break.
fn goes_on_into((text, cut): (&Text, bool), n: usize, other: &str) -> bool {
    let line = text.line(n);
    cut && n + 1 == text.len() && other.starts_with(line.strip_suffix('\r').unwrap_or(line))
}

fn pane(f: FilePart) -> Pane {
    Pane {
        size: f.size,
        mtime_ms: f.mtime_ms,
        cut: f.size > f.bytes.len() as u64,
        shown: f.bytes.len() as u64,
        body: decode(&f.bytes, f.size),
    }
}

/// Reads the start of this computer's copy of `rel` in the folder at
/// `root`, as the other computer reads its own: never through a link and
/// never outside the folder. Blocks; call it off the UI thread.
pub fn read_local(root: &Path, rel: &str) -> Result<FilePart, String> {
    let gone = || {
        format!(
            "\"{rel}\" is no longer a file here. Cancel the preview and start it again to see \
             what changed."
        )
    };
    let path = safe_join(root, rel).map_err(|e| e.to_string())?;
    let meta = std::fs::symlink_metadata(&path).map_err(|_| gone())?;
    if meta.file_type().is_symlink() {
        return Err(format!(
            "\"{rel}\" is a link here, not a file, so there is nothing to show."
        ));
    }
    if !meta.is_file() {
        return Err(gone());
    }
    let unread = |e: std::io::Error| {
        format!("Could not read \"{rel}\" here: {e}. Close this and try again in a moment.")
    };
    let file = std::fs::File::open(&path).map_err(unread)?;
    let meta = file.metadata().map_err(unread)?;
    let mut bytes = Vec::new();
    file.take(MAX_PART)
        .read_to_end(&mut bytes)
        .map_err(unread)?;
    Ok(FilePart {
        size: meta.len(),
        mtime_ms: mtime_ms(&meta),
        bytes,
    })
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
