//! The file viewer from the UI's side: reading both copies of a file in
//! the background and handing the window what to draw.

use super::peers::NOT_PAIRED;
use super::{Core, lock};
use crate::net::FilePart;
use crate::net::may_ask;
use crate::protocol::{MAX_PART, Request};
use crate::viewer::{Copies, FileAsk, FileView, Shown, read_local};
use std::sync::Arc;
use std::sync::atomic::Ordering;

impl Core {
    /// Shows the viewer for `ask` at once, and its copies once read.
    pub(super) fn open_file(&self, ask: FileAsk) {
        let opened = self.opened.fetch_add(1, Ordering::Relaxed) + 1;
        let view = FileView {
            ask: ask.clone(),
            opened,
            shown: None,
        };
        self.ui.update(|s| s.file_view = Some(view));
        let core = self.clone();
        let read = tokio::spawn(async move {
            let shown = core.read_both(&ask, opened).await;
            core.show(opened, shown);
        });
        // The read before this one is for a viewer that is gone.
        if let Some(before) = lock(&self.viewer).replace(read.abort_handle()) {
            before.abort();
        }
    }

    /// Hands the window what to draw for opening number `opened`.
    pub(super) fn show(&self, opened: u64, shown: Shown) {
        self.ui.update(|s| {
            // Closed, or another file opened, while this one was read.
            if let Some(v) = s.file_view.as_mut().filter(|v| v.opened == opened) {
                v.shown = Some(Arc::new(shown));
            }
        });
    }

    /// Shows the copy here while the other computer's is on its way.
    async fn show_early(&self, opened: u64, here: Option<Result<FilePart, String>>) {
        let build = tokio::task::spawn_blocking(move || Shown::build(here, None));
        if let Ok(mut shown) = build.await {
            shown.waiting = true;
            self.show(opened, shown);
        }
    }

    pub(super) fn close_file(&self) {
        self.stop_viewer();
        self.ui.update(|s| s.file_view = None);
    }

    /// Stops the viewer's read, so a computer that stopped answering holds
    /// no connection open after the viewer is gone.
    pub(super) fn stop_viewer(&self) {
        if let Some(read) = lock(&self.viewer).take() {
            read.abort();
        }
    }

    /// Reads both copies at the same time, then lines them up off the
    /// runtime's workers, since that can take a moment.
    async fn read_both(&self, ask: &FileAsk, opened: u64) -> Shown {
        let here = async {
            if !ask.copies.here() {
                return None;
            }
            let (root, rel) = (ask.local_root.clone(), ask.rel.clone());
            let read = tokio::task::spawn_blocking(move || read_local(&root, &rel)).await;
            let read = Some(read.unwrap_or_else(|e| {
                Err(format!(
                    "Reading the copy here stopped unexpectedly ({e}). Close this and try again."
                ))
            }));
            if ask.copies == Copies::Both {
                self.show_early(opened, read.clone()).await;
            }
            read
        };
        let there = async {
            if !ask.copies.there() {
                return None;
            }
            Some(self.read_there(ask).await)
        };
        let (here, there) = tokio::join!(here, there);
        tokio::task::spawn_blocking(move || Shown::build(here, there))
            .await
            .unwrap_or_else(|e| Shown {
                here: Some(Err(format!(
                    "Lining the copies up stopped unexpectedly ({e}). Close this and try again."
                ))),
                there: None,
                rows: Vec::new(),
                alike: None,
                waiting: false,
            })
    }

    async fn read_there(&self, ask: &FileAsk) -> Result<FilePart, String> {
        let peer = self
            .shared
            .peers
            .read()
            .await
            .iter()
            .find(|p| p.id == ask.peer)
            .cloned()
            .ok_or(NOT_PAIRED)?;
        let name = &peer.name;
        let read = Request::ReadFile {
            project: ask.project,
            folder: ask.folder,
            rel: ask.rel.clone(),
            max: MAX_PART,
        };
        // Asking for what it won't share only fills its activity with
        // warnings.
        if !may_ask(&read, peer.granted) {
            return Err(format!(
                "{name} doesn't let this computer pull from it, so its copy can't be shown. \
                 Allow pulling for it on {name} to see it here."
            ));
        }
        let mut conn = self
            .connect(ask.peer)
            .await
            .map_err(|e| format!("Could not read the copy on {name}: {e:#}"))?;
        let part = conn
            .read_part(ask.project, ask.folder, &ask.rel, MAX_PART)
            .await;
        conn.close().await;
        part.map_err(|e| e.sentence(name))
    }
}
