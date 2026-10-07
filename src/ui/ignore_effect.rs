//! What adding an ignore pattern would do to an open preview: what it
//! leaves out, and what it would leave exposed.

use super::preview::path_of;
use crate::ignore_rules::Matcher;
use crate::manifest::Change;
use crate::transfer::{Preview, Replaced};
use std::collections::HashMap;

/// What adding a pattern would do to an open preview.
#[derive(Clone, Debug, PartialEq)]
pub struct Effect {
    /// The preview narrowed to the changes the pattern leaves out, so they
    /// can be listed and counted the way the preview does.
    pub preview: Preview,
    /// Entries on the receiving side the pattern leaves out while what
    /// replaces them is still written: they would still be replaced, and
    /// the next preview could no longer warn about it.
    pub still_replaced: Vec<Replaced>,
}

/// What a scan with `m` added to the list would change in `p`.
pub fn left_out(p: &Preview, m: &Matcher) -> Effect {
    let mut seen = HashMap::new();
    let mut out = p.clone();
    let mut still_replaced = Vec::new();
    for f in &mut out.folders {
        let gone: Vec<bool> = f
            .plan
            .changes
            .iter()
            .map(|c| change_left_out(m, c, &mut seen))
            .collect();
        let replaced = std::mem::take(&mut f.replaced);
        for r in replaced {
            // What takes its place is written at the same path, or at one
            // differing only by case on a disk that ignores case.
            let writer = f.plan.changes.iter().position(|c| {
                matches!(c, Change::Add(_) | Change::Replace { .. })
                    && path_of(c).0.to_lowercase() == r.rel.to_lowercase()
            });
            let there_gone = m.leaves_out_with(&r.rel, r.was_folder, &mut seen);
            match (there_gone, writer.is_none_or(|i| gone[i])) {
                (true, true) => f.replaced.push(r),
                (true, false) => still_replaced.push(r),
                (false, _) => {}
            }
        }
        let mut gone = gone.into_iter();
        f.plan.changes.retain(|_| gone.next() == Some(true));
        f.skipped.clear();
    }
    out.left_out.clear();
    out.warnings.clear();
    Effect {
        preview: out,
        still_replaced,
    }
}

/// Whether a scan with `m` leaves out what `c` is about. A Replace is a
/// file on one side and a folder on the other, so both must be left out.
fn change_left_out(m: &Matcher, c: &Change, seen: &mut HashMap<String, bool>) -> bool {
    let (rel, is_dir) = path_of(c);
    let gone = m.leaves_out_with(rel, is_dir, seen);
    match c {
        Change::Replace { .. } => gone && m.leaves_out_with(rel, !is_dir, seen),
        _ => gone,
    }
}

#[cfg(test)]
#[path = "ignore_effect_tests.rs"]
mod tests;
