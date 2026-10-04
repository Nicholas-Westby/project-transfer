//! Native folder pickers, behind a trait so tests never open a window.

use std::path::PathBuf;

pub trait FolderPicker {
    /// Blocks until a folder is chosen or the picker is cancelled.
    fn pick_folder(&self, title: &str) -> Option<PathBuf>;
}

/// The platform's own folder picker. It is modal, so blocking the UI thread
/// while it is open is what the person expects.
#[derive(Default)]
pub struct RfdPicker;

impl FolderPicker for RfdPicker {
    fn pick_folder(&self, title: &str) -> Option<PathBuf> {
        rfd::FileDialog::new().set_title(title).pick_folder()
    }
}
