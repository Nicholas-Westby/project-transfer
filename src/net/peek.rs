//! Reading the start of a file on a paired computer, to show it.

use super::Connection;
use super::server::is_eof;
use crate::model::{FolderId, ProjectId};
use crate::protocol::{MAX_PART, Request, Response, read_msg_limited};
use anyhow::anyhow;
use std::time::Duration;

/// How long a part may take to arrive. It is at most 1 MiB, so this only
/// runs out when the other computer stopped answering.
const PART_WAIT: Duration = Duration::from_secs(30);
/// A part's header and a refusal are short; a longer answer is refused
/// before it is read.
const ANSWER_MAX: u32 = 64 * 1024;

/// The start of a file on the other computer.
#[derive(Clone, Debug, PartialEq)]
pub struct FilePart {
    /// The whole file's size.
    pub size: u64,
    pub mtime_ms: i64,
    pub bytes: Vec<u8>,
}

/// Why the other computer's copy could not be read.
#[derive(Debug)]
pub enum PartError {
    /// It turned the request down; the reason is worded for this computer.
    Refused(String),
    /// It closed the connection without answering, as a version that
    /// doesn't know the request does.
    Unanswered,
    /// The connection failed some other way.
    Failed(anyhow::Error),
    /// It stopped answering before the part arrived.
    TimedOut,
}

impl PartError {
    /// What happened and what to do, for the viewer's pane.
    pub fn sentence(&self, peer: &str) -> String {
        match self {
            PartError::Refused(reason) => reason.clone(),
            PartError::Unanswered => format!(
                "{peer} closed the connection without sending its copy. It may run an older \
                 version of Project Transfer; update it there to see its copy here."
            ),
            PartError::TimedOut => format!(
                "{peer} stopped answering before its copy arrived. Close this and try again in \
                 a moment."
            ),
            PartError::Failed(e) => format!(
                "Could not read the copy on {peer} ({e:#}). Close this and try again in a moment."
            ),
        }
    }
}

impl Connection {
    /// Reads up to `max` bytes from the start of `rel` in the other
    /// computer's copy of a folder, and the size of the whole file.
    pub async fn read_part(
        &mut self,
        project: ProjectId,
        folder: FolderId,
        rel: &str,
        max: u64,
    ) -> Result<FilePart, PartError> {
        let read = self.read_part_now(project, folder, rel, max);
        tokio::time::timeout(PART_WAIT, read)
            .await
            .unwrap_or(Err(PartError::TimedOut))
    }

    async fn read_part_now(
        &mut self,
        project: ProjectId,
        folder: FolderId,
        rel: &str,
        max: u64,
    ) -> Result<FilePart, PartError> {
        let ask = Request::ReadFile {
            project,
            folder,
            rel: rel.into(),
            max,
        };
        let answer = async {
            self.send(&ask).await?;
            read_msg_limited(&mut self.stream, ANSWER_MAX).await
        };
        let answer = match answer.await {
            Ok(a) => a,
            Err(e) if is_eof(&e) => return Err(PartError::Unanswered),
            Err(e) => return Err(PartError::Failed(e)),
        };
        let (size, mtime_ms, len) = match answer {
            Response::FilePart {
                size,
                mtime_ms,
                len,
            } => (size, mtime_ms, len),
            Response::Refused { reason } => return Err(PartError::Refused(reason)),
            other => {
                let name = &self.peer_name;
                let what = format!("{name} answered the file request with {other:?}");
                return Err(PartError::Failed(anyhow!(what)));
            }
        };
        // Never trust a length enough to hold more than was asked for.
        if len > max.min(MAX_PART) {
            let why = format!(
                "{} sent more of the file than was asked for",
                self.peer_name
            );
            return Err(PartError::Failed(anyhow!(why)));
        }
        let mut bytes = Vec::with_capacity(len as usize);
        self.recv_raw(len, &mut bytes)
            .await
            .map_err(PartError::Failed)?;
        Ok(FilePart {
            size,
            mtime_ms,
            bytes,
        })
    }
}

#[cfg(test)]
#[path = "peek_tests.rs"]
mod tests;
