//! Editing and deleting the shared commands themselves. Adding one to a
//! project, linked or as a separate copy, is a project edit in `commands.rs`.

use super::Core;
use super::commands::command_text;
use super::projects::Edit;
use crate::model::{Project, SharedCommand, SharedCommandId};
use crate::shared_commands::stamp_after;
use crate::transfer::projects::now_ms;

const GONE_SHARED: &str = "That shared command no longer exists.";

impl Core {
    pub(super) async fn edit_shared_command(
        &self,
        shared: SharedCommandId,
        label: String,
        line: String,
    ) -> anyhow::Result<()> {
        self.edit_commands(|all, library| {
            let (label, line) = command_text(&label, &line)?;
            edit_shared(all, library, shared, label, line)
        })
        .await
    }

    pub(super) async fn delete_shared_command(
        &self,
        shared: SharedCommandId,
    ) -> anyhow::Result<()> {
        self.edit_commands(|all, library| {
            let s = library
                .iter_mut()
                .find(|s| s.id == shared && !s.deleted)
                .ok_or(GONE_SHARED)?;
            s.deleted = true;
            s.updated_at_ms = stamp_after(s.updated_at_ms, now_ms());
            // Commands, not projects: two computers can each have linked it
            // into the same project before they synced.
            let linked = all
                .iter()
                .flat_map(|p| &p.commands)
                .filter(|c| !c.deleted && c.linked_to == Some(shared))
                .count();
            let kept = match linked {
                0 => String::new(),
                1 => " The command linked to it keeps its text.".into(),
                n => format!(" The {n} commands linked to it keep their text."),
            };
            Ok(format!(
                "Deleted shared command `{}`.{kept} Paired computers drop it after the next \
                 transfer.",
                s.label
            ))
        })
        .await
    }
}

pub(super) fn live_shared(library: &[SharedCommand], id: SharedCommandId) -> Edit<&SharedCommand> {
    library
        .iter()
        .find(|s| s.id == id && !s.deleted)
        .ok_or_else(|| GONE_SHARED.to_string())
}

/// Edits the shared command; `update_commands` passes the edit on to the
/// commands linked to it.
pub(super) fn edit_shared(
    all: &[Project],
    library: &mut [SharedCommand],
    id: SharedCommandId,
    label: String,
    line: String,
) -> Edit<String> {
    let s = library
        .iter_mut()
        .find(|s| s.id == id && !s.deleted)
        .ok_or(GONE_SHARED)?;
    s.label = label;
    s.line = line;
    s.updated_at_ms = stamp_after(s.updated_at_ms, now_ms());
    Ok(match uses(all, id) {
        0 | 1 => format!("Saved shared command `{}`.", s.label),
        n => format!("Saved shared command `{}`, used in {n} projects.", s.label),
    })
}

/// How many projects have a command linked to `id`.
fn uses(all: &[Project], id: SharedCommandId) -> usize {
    all.iter()
        .filter(|p| {
            p.commands
                .iter()
                .any(|c| !c.deleted && c.linked_to == Some(id))
        })
        .count()
}
