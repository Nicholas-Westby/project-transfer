//! Manifest types. Scanning and comparison are added by the manifest task.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum Kind {
    File {
        size: u64,
        mtime_ms: i64,
        exec: bool,
    },
    Dir,
    Symlink {
        target: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Entry {
    /// '/' separated, no leading slash.
    pub rel: String,
    pub kind: Kind,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Manifest {
    /// Sorted by `rel`.
    pub entries: Vec<Entry>,
    /// Ignored files under each existing directory ("" is the root), so a
    /// removed directory can report how many ignored files go with it.
    pub ignored_in_dir: BTreeMap<String, u64>,
}

mod compare;
mod scan;
mod spelling;

pub use compare::{Change, Plan, compare, resolve_hashes};
pub use scan::{hash_file, scan};
pub(crate) use scan::{is_exec, mtime_ms};
pub use spelling::compose_for;
