//! The patterns offered for a path picked in a preview, and the fix for a
//! pattern that starts with a project folder's name.

use crate::ignore_rules::escape;

#[derive(Clone, Debug, PartialEq)]
pub struct Suggestion {
    pub pattern: String,
    /// What it covers, with `backticked` names.
    pub label: String,
}

/// Folders above a path offered as their own choice, nearest first. More
/// would push the rest of the dialog out of reach; typing covers the rest.
const FOLDERS: usize = 4;

/// Patterns for `rel`, a path inside a project folder, narrowest first: the
/// path itself, the folders above it, anything of the same name, then every
/// file of the same type. Paths start with `/`, so they match from the top
/// of each project folder and nowhere else.
pub fn suggestions(rel: &str, is_dir: bool) -> Vec<Suggestion> {
    let parts: Vec<&str> = rel.split('/').collect();
    let path = |n: usize| {
        let escaped: Vec<String> = parts[..n].iter().map(|p| escape(p)).collect();
        format!("/{}", escaped.join("/"))
    };
    let offer = |pattern: String, label: String| Suggestion { pattern, label };
    let name = parts[parts.len() - 1];
    let mut out = vec![if is_dir {
        offer(
            format!("{}/", path(parts.len())),
            format!("Everything in `{rel}`"),
        )
    } else {
        offer(path(parts.len()), "This file only".into())
    }];
    for n in (1..parts.len()).rev().take(FOLDERS) {
        let shown = parts[..n].join("/");
        out.push(offer(
            format!("{}/", path(n)),
            format!("Everything in `{shown}`"),
        ));
    }
    let folder = if is_dir {
        Some(name)
    } else {
        parts.len().checked_sub(2).map(|i| parts[i])
    };
    if let Some(f) = folder {
        out.push(offer(
            format!("{}/", escape(f)),
            format!("Any folder named `{f}`"),
        ));
    }
    if !is_dir {
        out.push(offer(escape(name), format!("Anything named `{name}`")));
        if let Some(ext) = extension(name) {
            out.push(offer(
                format!("*.{}", escape(ext)),
                format!("Every `.{ext}` file"),
            ));
        }
    }
    out
}

/// The part after the last dot, for names with something before it.
fn extension(name: &str) -> Option<&str> {
    let (stem, ext) = name.rsplit_once('.')?;
    (!stem.is_empty() && !ext.is_empty()).then_some(ext)
}

/// For a pattern that starts with the name of one of `folders`: that name,
/// and the pattern without it. Patterns are matched inside each project
/// folder, so a leading folder name only matches a folder of that name
/// inside it. What is left keeps a leading `/`, so it still means a path
/// from the top.
pub fn without_folder_name(pattern: &str, folders: &[&str]) -> Option<(String, String)> {
    let (bang, rest) = match pattern.strip_prefix('!') {
        Some(rest) => ("!", rest),
        None => ("", pattern),
    };
    let rest = rest.strip_prefix('/').unwrap_or(rest);
    let (first, after) = rest.split_once('/')?;
    // Without the name, nothing but wildcards would leave out everything.
    let only_wildcards = after.chars().all(|c| matches!(c, '*' | '/'));
    if only_wildcards || !folders.contains(&first) {
        return None;
    }
    Some((first.to_string(), format!("{bang}/{after}")))
}

#[cfg(test)]
#[path = "ignoring_tests.rs"]
mod tests;
