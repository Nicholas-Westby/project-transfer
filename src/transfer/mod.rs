//! Running a push or pull end to end: preview first, then apply.

pub mod apply;
mod execute;
pub mod link;
mod op;
mod paths;
mod prepare;
mod preview;
pub mod projects;

pub use apply::{Applier, PendingFile};
pub use execute::execute;
pub use op::Op;
pub use paths::{safe_join, validate_name, validate_rel};
pub use prepare::prepare;
pub use preview::{
    Counts, FolderPreview, Preview, Replaced, TransferRequest, drop_unholdable, replaced_folders,
};

#[derive(Clone, Debug, PartialEq)]
pub enum Progress {
    Started {
        total_files: u64,
        total_bytes: u64,
    },
    /// `bytes_done` counts the whole transfer so far.
    File {
        rel: String,
        bytes_done: u64,
    },
    Done(Summary),
    Failed(String),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Summary {
    /// Files and links written, plus files whose time was fixed.
    pub files: u64,
    /// File content bytes sent.
    pub bytes: u64,
    /// Removed entries (a removed folder counts once).
    pub removed: u64,
    pub took_ms: u64,
    /// (rel, reason) for entries that could not be applied.
    pub failures: Vec<(String, String)>,
}
