use super::{Entry, Kind, Manifest};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum Change {
    Add(Entry),
    Update {
        entry: Entry,
        dest_newer: bool,
    },
    TimestampOnly(Entry),
    Replace {
        entry: Entry,
    },
    RemoveFile(String),
    RemoveDir {
        rel: String,
        files: u64,
        ignored: u64,
    },
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Plan {
    pub changes: Vec<Change>,
    pub needs_hash: Vec<String>,
}

/// Same kind of thing on both sides (file/file, dir/dir, symlink/symlink).
fn same_class(a: &Kind, b: &Kind) -> bool {
    std::mem::discriminant(a) == std::mem::discriminant(b)
}

fn parent(rel: &str) -> &str {
    rel.rfind('/').map_or("", |i| &rel[..i])
}

fn depth(rel: &str) -> usize {
    rel.matches('/').count()
}

pub fn compare(src: &Manifest, dst: &Manifest) -> Plan {
    let src_by: HashMap<&str, &Entry> = src.entries.iter().map(|e| (e.rel.as_str(), e)).collect();
    let dst_by: HashMap<&str, &Entry> = dst.entries.iter().map(|e| (e.rel.as_str(), e)).collect();
    let mut plan = Plan::default();

    // Sorted by rel, so a parent always precedes its children.
    for s in &src.entries {
        let Some(d) = dst_by.get(s.rel.as_str()) else {
            plan.changes.push(Change::Add(s.clone()));
            continue;
        };
        match (&s.kind, &d.kind) {
            (
                Kind::File {
                    size: ss,
                    mtime_ms: sm,
                    exec: se,
                },
                Kind::File {
                    size: ds,
                    mtime_ms: dm,
                    exec: de,
                },
            ) => {
                let update = |plan: &mut Plan| {
                    plan.changes.push(Change::Update {
                        entry: s.clone(),
                        dest_newer: dm > sm,
                    });
                };
                if ss != ds || se != de {
                    update(&mut plan);
                } else if sm != dm {
                    plan.needs_hash.push(s.rel.clone());
                    update(&mut plan);
                }
            }
            (Kind::Symlink { target: a }, Kind::Symlink { target: b }) if a != b => {
                plan.changes.push(Change::Replace { entry: s.clone() });
            }
            (a, b) if !same_class(a, b) => {
                plan.changes.push(Change::Replace { entry: s.clone() });
            }
            _ => {}
        }
    }

    // Only the topmost missing entry is reported; anything below it, or below
    // a directory that gets replaced, goes away with it.
    let mut removals: Vec<Change> = Vec::new();
    for d in &dst.entries {
        if src_by.contains_key(d.rel.as_str()) {
            continue;
        }
        let p = parent(&d.rel);
        let parent_survives =
            p.is_empty() || matches!(src_by.get(p), Some(e) if e.kind == Kind::Dir);
        if !parent_survives {
            continue;
        }
        removals.push(match d.kind {
            Kind::Dir => {
                let prefix = format!("{}/", d.rel);
                let files = dst
                    .entries
                    .iter()
                    .filter(|e| e.rel.starts_with(&prefix) && matches!(e.kind, Kind::File { .. }))
                    .count() as u64;
                Change::RemoveDir {
                    rel: d.rel.clone(),
                    files,
                    ignored: dst.ignored_in_dir.get(&d.rel).copied().unwrap_or(0),
                }
            }
            _ => Change::RemoveFile(d.rel.clone()),
        });
    }
    let key = |c: &Change| match c {
        Change::RemoveFile(r) | Change::RemoveDir { rel: r, .. } => (depth(r), r.clone()),
        _ => (0, String::new()),
    };
    removals.sort_by(|a, b| {
        let (ka, kb) = (key(a), key(b));
        kb.0.cmp(&ka.0).then(ka.1.cmp(&kb.1))
    });
    plan.changes.extend(removals);
    plan
}

/// Equal hashes downgrade a provisional `Update` to `TimestampOnly`. A missing
/// hash keeps the `Update`, which is the safe direction.
pub fn resolve_hashes(
    mut plan: Plan,
    src_hashes: &HashMap<String, String>,
    dst_hashes: &HashMap<String, String>,
) -> Plan {
    for c in &mut plan.changes {
        if let Change::Update { entry, .. } = c
            && plan.needs_hash.contains(&entry.rel)
            && let (Some(a), Some(b)) = (src_hashes.get(&entry.rel), dst_hashes.get(&entry.rel))
            && a == b
        {
            *c = Change::TimestampOnly(entry.clone());
        }
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(rel: &str, size: u64, mtime_ms: i64) -> Entry {
        Entry {
            rel: rel.into(),
            kind: Kind::File {
                size,
                mtime_ms,
                exec: false,
            },
        }
    }
    fn d(rel: &str) -> Entry {
        Entry {
            rel: rel.into(),
            kind: Kind::Dir,
        }
    }
    fn man(mut entries: Vec<Entry>, ignored: &[(&str, u64)]) -> Manifest {
        entries.sort_by(|a, b| a.rel.cmp(&b.rel));
        Manifest {
            entries,
            ignored_in_dir: ignored.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
        }
    }

    #[test]
    fn identical_is_empty_plan() {
        let m = man(vec![d("a"), f("a/x", 1, 5)], &[]);
        assert_eq!(compare(&m, &m), Plan::default());
    }

    #[test]
    fn added_updated_deleted() {
        let src = man(vec![f("new", 1, 1), f("chg", 2, 9), f("same", 1, 1)], &[]);
        let dst = man(vec![f("chg", 3, 20), f("gone", 1, 1), f("same", 1, 1)], &[]);
        let p = compare(&src, &dst);
        assert_eq!(
            p.changes,
            vec![
                Change::Update {
                    entry: f("chg", 2, 9),
                    dest_newer: true
                },
                Change::Add(f("new", 1, 1)),
                Change::RemoveFile("gone".into()),
            ]
        );
        assert!(p.needs_hash.is_empty());
    }

    #[test]
    fn dest_newer_false_when_source_newer() {
        let src = man(vec![f("a", 2, 50)], &[]);
        let dst = man(vec![f("a", 3, 10)], &[]);
        assert!(matches!(
            compare(&src, &dst).changes[0],
            Change::Update {
                dest_newer: false,
                ..
            }
        ));
    }

    #[test]
    fn same_size_different_mtime_needs_hash_then_resolves() {
        let src = man(vec![f("a", 4, 1000), f("b", 4, 1000)], &[]);
        let dst = man(vec![f("a", 4, 2000), f("b", 4, 2000)], &[]);
        let p = compare(&src, &dst);
        assert_eq!(p.needs_hash, vec!["a", "b"]);
        assert!(matches!(p.changes[0], Change::Update { .. }));
        let sh: HashMap<_, _> = [("a", "h1"), ("b", "h2")]
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .into();
        let dh: HashMap<_, _> = [("a", "h1"), ("b", "other")]
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .into();
        let r = resolve_hashes(p, &sh, &dh);
        assert_eq!(r.changes[0], Change::TimestampOnly(f("a", 4, 1000)));
        assert!(matches!(r.changes[1], Change::Update { .. }));
    }

    #[test]
    fn sub_millisecond_difference_is_unchanged() {
        // mtimes are stored in ms, so equal ms means unchanged.
        let src = man(vec![f("a", 4, 1000)], &[]);
        assert_eq!(compare(&src, &src.clone()), Plan::default());
    }

    #[test]
    fn exec_only_difference_is_update_without_hash() {
        let mut e = f("run", 3, 7);
        let src = man(vec![e.clone()], &[]);
        if let Kind::File { exec, .. } = &mut e.kind {
            *exec = true;
        }
        let dst = man(vec![f("run", 3, 7)], &[]);
        let p = compare(&man(vec![e.clone()], &[]), &dst);
        assert_eq!(
            p.changes,
            vec![Change::Update {
                entry: e,
                dest_newer: false
            }]
        );
        assert!(p.needs_hash.is_empty());
        let _ = src;
    }

    #[test]
    fn dir_to_file_swap_is_replace_without_child_removals() {
        let src = man(vec![f("x", 1, 1)], &[]);
        let dst = man(vec![d("x"), f("x/a", 1, 1), f("x/b", 1, 1)], &[]);
        assert_eq!(
            compare(&src, &dst).changes,
            vec![Change::Replace {
                entry: f("x", 1, 1)
            }]
        );
    }

    #[test]
    fn file_to_dir_swap_is_replace_then_adds() {
        let src = man(vec![d("x"), f("x/a", 1, 1)], &[]);
        let dst = man(vec![f("x", 1, 1)], &[]);
        assert_eq!(
            compare(&src, &dst).changes,
            vec![
                Change::Replace { entry: d("x") },
                Change::Add(f("x/a", 1, 1))
            ]
        );
    }

    #[test]
    fn symlink_target_change_is_replace() {
        let s = |t: &str| Entry {
            rel: "l".into(),
            kind: Kind::Symlink { target: t.into() },
        };
        let p = compare(&man(vec![s("a")], &[]), &man(vec![s("b")], &[]));
        assert_eq!(p.changes, vec![Change::Replace { entry: s("a") }]);
        assert!(
            compare(&man(vec![s("a")], &[]), &man(vec![s("a")], &[]))
                .changes
                .is_empty()
        );
    }

    #[test]
    fn removed_dir_counts_files_and_ignored() {
        let src = man(vec![d("keep")], &[]);
        let dst = man(
            vec![
                d("keep"),
                d("old"),
                d("old/sub"),
                f("old/a", 1, 1),
                f("old/sub/b", 1, 1),
            ],
            &[("", 3), ("old", 3), ("old/sub", 1)],
        );
        assert_eq!(
            compare(&src, &dst).changes,
            vec![Change::RemoveDir {
                rel: "old".into(),
                files: 2,
                ignored: 3
            }]
        );
    }

    #[test]
    fn ignored_files_in_surviving_dir_cause_no_change() {
        let src = man(vec![d("a")], &[("a", 2), ("", 2)]);
        let dst = man(vec![d("a")], &[("a", 5), ("", 5)]);
        assert!(compare(&src, &dst).changes.is_empty());
    }

    #[test]
    fn ordering_adds_parent_first_removals_last_deepest_first() {
        let src = man(vec![d("n"), d("n/m"), f("n/m/z", 1, 1)], &[]);
        let dst = man(
            vec![f("a", 1, 1), d("p"), f("p/q", 1, 1), d("r"), d("r/s")],
            &[],
        );
        let c = compare(&src, &dst).changes;
        let kinds: Vec<String> = c
            .iter()
            .map(|c| match c {
                Change::Add(e) => format!("add {}", e.rel),
                Change::RemoveFile(r) => format!("rmf {r}"),
                Change::RemoveDir { rel, .. } => format!("rmd {rel}"),
                _ => "other".into(),
            })
            .collect();
        assert_eq!(
            kinds,
            ["add n", "add n/m", "add n/m/z", "rmf a", "rmd p", "rmd r"]
        );
    }

    #[test]
    fn removals_sort_deepest_first() {
        let src = man(vec![d("a"), d("a/b")], &[]);
        let dst = man(
            vec![
                d("a"),
                d("a/b"),
                f("a/b/c", 1, 1),
                f("a/x", 1, 1),
                f("top", 1, 1),
            ],
            &[],
        );
        let c = compare(&src, &dst).changes;
        assert_eq!(
            c,
            vec![
                Change::RemoveFile("a/b/c".into()),
                Change::RemoveFile("a/x".into()),
                Change::RemoveFile("top".into()),
            ]
        );
    }
}
