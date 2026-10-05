//! Reading a confirmed plan: which changes make folders or carry content,
//! and the totals progress counts towards.

use crate::manifest::{Change, Entry, Kind};
use crate::transfer::Preview;

pub(super) fn is_dir_change(c: &Change) -> bool {
    matches!(c, Change::Add(e) | Change::Replace { entry: e } if e.kind == Kind::Dir)
}

pub(super) fn content(c: &Change) -> Option<&Entry> {
    match c {
        Change::Add(e) | Change::Update { entry: e, .. } | Change::Replace { entry: e }
            if e.kind != Kind::Dir =>
        {
            Some(e)
        }
        _ => None,
    }
}

pub(super) fn written_rel(c: &Change) -> &str {
    match c {
        Change::Add(e) | Change::TimestampOnly(e) => &e.rel,
        Change::Update { entry, .. } | Change::Replace { entry } => &entry.rel,
        Change::RemoveFile(r) | Change::RemoveDir { rel: r, .. } => r,
    }
}

impl Preview {
    /// Files and links the transfer writes (plus times it fixes), and the
    /// bytes of content it copies; what progress counts towards.
    pub fn totals(&self) -> (u64, u64) {
        totals(self)
    }
}

/// Files and links to write (plus times to fix), and the bytes to copy.
pub(super) fn totals(p: &Preview) -> (u64, u64) {
    let mut files = 0;
    let mut bytes = 0;
    for c in p.folders.iter().flat_map(|f| &f.plan.changes) {
        if let Some(e) = content(c) {
            files += 1;
            if let Kind::File { size, .. } = e.kind {
                bytes += size;
            }
        } else if matches!(c, Change::TimestampOnly(_)) {
            files += 1;
        }
    }
    (files, bytes)
}
