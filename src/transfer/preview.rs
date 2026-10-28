//! What a transfer will do, and the pure checks that shape it.

use super::sync_count::SyncChoice;
use crate::manifest::{Change, Plan};
use crate::model::{Direction, FolderId, InstanceId, ProjectId};

#[derive(Clone, Debug, PartialEq)]
pub struct TransferRequest {
    pub peer: InstanceId,
    pub project: ProjectId,
    pub direction: Direction,
    pub send_everything: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Preview {
    pub request: TransferRequest,
    pub folders: Vec<FolderPreview>,
    /// Folders of the project this transfer skips, so the sheet can still
    /// name every folder and say why.
    pub left_out: Vec<LeftOut>,
    pub warnings: Vec<String>,
    /// The other computer's ids this computer's project takes on when the
    /// transfer runs; `request.project` is already the new id.
    pub link: Option<super::link::Link>,
    /// The transfer changes the description on the receiving computer.
    pub description: bool,
    /// Why a sync chose this direction; None for a push or pull asked for
    /// as such.
    pub sync: Option<SyncChoice>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LeftOut {
    pub name: String,
    /// A full sentence: what is wrong and what to do.
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FolderPreview {
    pub folder: FolderId,
    pub name: String,
    pub source_path: String,
    pub dest_path: String,
    pub dest_will_be_created: bool,
    pub plan: Plan,
    /// (rel, reason) for names the destination cannot hold.
    pub skipped: Vec<(String, String)>,
    /// (rel, reason) for destination names left in place rather than
    /// removed, such as those a copy on Windows can't hold.
    pub left_in_place: Vec<(String, String)>,
    /// Destination entries that writing the source removes without a
    /// removal line of their own, such as a folder replaced by a file.
    pub replaced: Vec<Replaced>,
    /// Temp files an unfinished transfer left in this computer's copy, for
    /// a pull to clear before it writes. Empty for a push, whose receiver
    /// clears its own.
    pub leftovers: Vec<std::path::PathBuf>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Replaced {
    pub rel: String,
    /// Files removed, ignored ones included.
    pub files: u64,
    pub ignored: u64,
    /// What takes its place: "a file", "a link" or "a folder".
    pub by: String,
    /// False when a single file is replaced by a folder.
    pub was_folder: bool,
}

impl Replaced {
    pub fn warning(&self) -> String {
        if !self.was_folder {
            return format!("Replacing file `{}` with {}.", self.rel, self.by);
        }
        let files = match self.files {
            1 => "1 file".to_string(),
            n => format!("{n} files"),
        };
        let ignored = match self.ignored {
            0 => String::new(),
            n => format!(" ({n} of them ignored)"),
        };
        format!(
            "Replacing folder `{}` with {} removes {files} inside it{ignored}.",
            self.rel, self.by
        )
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub added: u64,
    pub changed: u64,
    pub timestamp_only: u64,
    /// Files that will be deleted, including those inside removed folders.
    pub removed_files: u64,
    pub dest_newer: u64,
}

impl Preview {
    pub fn counts(&self) -> Counts {
        let mut c = Counts::default();
        for change in self.folders.iter().flat_map(|f| &f.plan.changes) {
            match change {
                Change::Add(_) => c.added += 1,
                Change::Update { dest_newer, .. } => {
                    c.changed += 1;
                    c.dest_newer += u64::from(*dest_newer);
                }
                Change::Replace { .. } => c.changed += 1,
                Change::TimestampOnly(_) => c.timestamp_only += 1,
                Change::RemoveFile(_) => c.removed_files += 1,
                Change::RemoveDir { files, ignored, .. } => c.removed_files += files + ignored,
            }
        }
        c.removed_files += self
            .folders
            .iter()
            .flat_map(|f| &f.replaced)
            .map(|r| r.files)
            .sum::<u64>();
        c
    }

    /// Nothing at all to do: no file changes, and the description matches.
    pub fn is_empty(&self) -> bool {
        self.files_match() && !self.description
    }

    pub fn files_match(&self) -> bool {
        self.folders.iter().all(|f| f.plan.changes.is_empty())
    }
}

#[cfg(test)]
#[path = "preview_tests.rs"]
mod tests;
