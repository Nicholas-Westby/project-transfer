//! Merging of a project's command list between two computers.

use crate::model::{Command, CommandId};
use std::collections::HashMap;

/// Union by id; the newer edit wins and a tie keeps the local copy. The run
/// hash is local state, so it always comes from the local side.
pub fn merge_commands(local: &[Command], remote: &[Command]) -> Vec<Command> {
    let mut by_id: HashMap<CommandId, Command> = local.iter().map(|c| (c.id, c.clone())).collect();
    for r in remote {
        match by_id.get_mut(&r.id) {
            Some(mine) if r.updated_at_ms > mine.updated_at_ms => {
                let hash = mine.last_run_hash.take();
                *mine = Command {
                    last_run_hash: hash,
                    ..r.clone()
                };
            }
            Some(_) => {}
            None => {
                by_id.insert(
                    r.id,
                    Command {
                        last_run_hash: None,
                        ..r.clone()
                    },
                );
            }
        }
    }
    let mut merged: Vec<Command> = by_id.into_values().collect();
    merged.sort_by(|a, b| a.label.cmp(&b.label).then(a.id.cmp(&b.id)));
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Os;
    use uuid::Uuid;

    fn cmd(id: Uuid, label: &str, at: i64) -> Command {
        Command {
            id,
            label: label.into(),
            line: format!("run {label}"),
            created_on: Os::MacOs,
            updated_at_ms: at,
            deleted: false,
            last_run_hash: None,
        }
    }

    #[test]
    fn adds_from_remote_sorted_by_label() {
        let a = cmd(Uuid::new_v4(), "b", 1);
        let b = cmd(Uuid::new_v4(), "a", 1);
        let m = merge_commands(std::slice::from_ref(&a), std::slice::from_ref(&b));
        assert_eq!(m, vec![b, a]);
    }

    #[test]
    fn delete_spreads_and_is_kept() {
        let id = Uuid::new_v4();
        let local = cmd(id, "x", 1);
        let mut remote = cmd(id, "x", 2);
        remote.deleted = true;
        let m = merge_commands(&[local], &[remote]);
        assert_eq!(m.len(), 1);
        assert!(m[0].deleted);
    }

    #[test]
    fn newer_edit_wins_both_ways() {
        let id = Uuid::new_v4();
        let old = cmd(id, "old", 1);
        let new = cmd(id, "new", 2);
        assert_eq!(
            merge_commands(std::slice::from_ref(&old), std::slice::from_ref(&new))[0].label,
            "new"
        );
        assert_eq!(merge_commands(&[new], &[old])[0].label, "new");
    }

    #[test]
    fn tie_keeps_local() {
        let id = Uuid::new_v4();
        let m = merge_commands(&[cmd(id, "mine", 5)], &[cmd(id, "theirs", 5)]);
        assert_eq!(m[0].label, "mine");
    }

    #[test]
    fn local_run_hash_is_preserved() {
        let id = Uuid::new_v4();
        let mut local = cmd(id, "x", 1);
        local.last_run_hash = Some("abc".into());
        let mut remote = cmd(id, "y", 9);
        remote.last_run_hash = Some("zzz".into());
        let m = merge_commands(&[local], &[remote]);
        assert_eq!(m[0].label, "y");
        assert_eq!(m[0].last_run_hash.as_deref(), Some("abc"));
        let m = merge_commands(&[], &[cmd(Uuid::new_v4(), "n", 1)]);
        assert_eq!(m[0].last_run_hash, None);
    }
}
