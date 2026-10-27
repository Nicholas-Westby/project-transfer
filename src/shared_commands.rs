//! Shared commands: the list any project can add a command from, and keeping
//! the commands linked to one in step with it. Both computers of a command
//! exchange merge the two lists and then `reconcile`, so they agree after it.
//! `reconcile` never touches a command's run hash, so text that reaches a
//! project this way is still shown for a check before it runs.

use crate::model::{Command, Project, SharedCommand, SharedCommandId};
use std::collections::HashMap;

/// Union by id; the newer edit wins and a tie keeps the local copy, as for a
/// project's commands.
pub fn merge_shared(local: &[SharedCommand], remote: &[SharedCommand]) -> Vec<SharedCommand> {
    let mut by_id: HashMap<SharedCommandId, SharedCommand> =
        local.iter().map(|s| (s.id, s.clone())).collect();
    for r in remote {
        match by_id.get_mut(&r.id) {
            Some(mine) if r.updated_at_ms > mine.updated_at_ms => *mine = r.clone(),
            Some(_) => {}
            None => {
                by_id.insert(r.id, r.clone());
            }
        }
    }
    let mut merged: Vec<SharedCommand> = by_id.into_values().collect();
    merged.sort_by(|a, b| a.label.cmp(&b.label).then(a.id.cmp(&b.id)));
    merged
}

/// The stamp for an edit made now. It always comes after the version it
/// replaces, even one from a computer whose clock runs ahead, or the next
/// exchange would bring that version back over the edit.
pub fn stamp_after(previous_ms: i64, now_ms: i64) -> i64 {
    now_ms.max(previous_ms + 1)
}

/// Puts the shared commands and the commands linked to them in step after an
/// edit or a merge, and says whether anything changed. The newest version of
/// a shared command wins, whether it was found in the list or in a linked
/// command (a link to one this computer lacks adds it), and every linked
/// command takes that version. A link to a deleted shared command is cut;
/// the command keeps the deleted command's last text and stamp, so both
/// computers cut it the same way. Deleted commands are left as they are.
pub fn reconcile(projects: &mut [Project], library: &mut Vec<SharedCommand>) -> bool {
    let mut changed = false;
    for c in projects.iter().flat_map(|p| &p.commands) {
        let Some(id) = c.linked_to.filter(|_| !c.deleted) else {
            continue;
        };
        match library.iter_mut().find(|s| s.id == id) {
            Some(s) if c.updated_at_ms <= s.updated_at_ms => continue,
            Some(s) => *s = version_in(c, id),
            None => library.push(version_in(c, id)),
        }
        changed = true;
    }
    let by_id: HashMap<SharedCommandId, &SharedCommand> =
        library.iter().map(|s| (s.id, s)).collect();
    for c in projects.iter_mut().flat_map(|p| p.commands.iter_mut()) {
        let Some(s) = c
            .linked_to
            .filter(|_| !c.deleted)
            .and_then(|id| by_id.get(&id))
        else {
            continue;
        };
        if s.deleted {
            // The deletion's own version, which may be an edit this computer
            // never saw: the other computer cuts its link with that text.
            c.linked_to = None;
            c.label = s.label.clone();
            c.line = s.line.clone();
            c.created_on = s.created_on;
            c.updated_at_ms = c.updated_at_ms.max(s.updated_at_ms);
        } else if !follows(c, s) {
            c.label = s.label.clone();
            c.line = s.line.clone();
            c.created_on = s.created_on;
            c.updated_at_ms = s.updated_at_ms;
        } else {
            continue;
        }
        changed = true;
    }
    changed
}

/// The shared command as the linked command `c` carries it.
fn version_in(c: &Command, id: SharedCommandId) -> SharedCommand {
    SharedCommand {
        id,
        label: c.label.clone(),
        line: c.line.clone(),
        created_on: c.created_on,
        updated_at_ms: c.updated_at_ms,
        deleted: false,
    }
}

fn follows(c: &Command, s: &SharedCommand) -> bool {
    c.label == s.label
        && c.line == s.line
        && c.created_on == s.created_on
        && c.updated_at_ms == s.updated_at_ms
}

#[cfg(test)]
#[path = "shared_commands_tests.rs"]
mod tests;
