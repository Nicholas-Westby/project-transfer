//! Gitignore-style rules deciding which paths take part in a transfer.

use crate::model::InstanceSettings;
use anyhow::Context;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const DEFAULT_IGNORES: &[&str] = &[
    "node_modules/",
    "bin/",
    "obj/",
    "packages/",
    "dist/",
    ".build/",
    "target/",
    ".DS_Store",
    "Thumbs.db",
    ".vs/",
    ".idea/",
    "__pycache__/",
    ".pytest_cache/",
    ".gradle/",
    ".next/",
    ".nuxt/",
    ".cache/",
    "*.pt-tmp",
];

/// Suffix of the half-written files a transfer leaves behind; never sent.
const TEMP_SUFFIX: &str = ".pt-tmp";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Default)]
pub struct IgnoreSpec {
    pub patterns: Vec<String>,
    pub always_include: Vec<String>,
    pub send_everything: bool,
}

impl IgnoreSpec {
    pub fn from_settings(s: &InstanceSettings) -> IgnoreSpec {
        let mut patterns: Vec<String> = DEFAULT_IGNORES
            .iter()
            .filter(|d| !s.removed_default_ignores.iter().any(|r| r == **d))
            .map(|d| d.to_string())
            .collect();
        patterns.extend(s.extra_ignores.iter().cloned());
        IgnoreSpec {
            patterns,
            always_include: s.always_include.clone(),
            send_everything: false,
        }
    }
}

pub struct Matcher {
    ignored: Gitignore,
    included: Gitignore,
    send_everything: bool,
}

/// A literal file or folder name as a pattern: characters that patterns
/// give a meaning to match only themselves.
pub fn escape(name: &str) -> String {
    let last = name.chars().count().saturating_sub(1);
    let mut out = String::with_capacity(name.len());
    for (i, c) in name.chars().enumerate() {
        // A pattern loses the spaces around it unless they are escaped.
        let edge = (i == 0 || i == last) && c == ' ';
        let special = matches!(c, '*' | '?' | '[' | ']' | '{' | '}' | '\\')
            || (i == 0 && matches!(c, '!' | '#'));
        if special || edge {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// `pattern` without the spaces around it. A trailing space that a backslash
/// escapes is part of a name, so it stays.
pub fn trim(pattern: &str) -> &str {
    let start = pattern.trim_start();
    let end = start.trim_end();
    let slashes = end.bytes().rev().take_while(|b| *b == b'\\').count();
    if slashes % 2 == 1 && start[end.len()..].starts_with(' ') {
        &start[..end.len() + 1]
    } else {
        end
    }
}

/// Why `pattern` can't be used, in the parser's words.
pub fn check(pattern: &str) -> Result<(), String> {
    match GitignoreBuilder::new("").add_line(None, pattern) {
        Ok(_) => Ok(()),
        Err(ignore::Error::Glob { err, .. }) => Err(err),
        Err(e) => Err(e.to_string()),
    }
}

fn build(lines: &[String]) -> anyhow::Result<Gitignore> {
    let mut b = GitignoreBuilder::new("");
    for line in lines {
        b.add_line(None, line)
            .with_context(|| format!("invalid ignore pattern \"{line}\""))?;
    }
    Ok(b.build()?)
}

impl Matcher {
    pub fn new(spec: &IgnoreSpec) -> anyhow::Result<Matcher> {
        Ok(Matcher {
            ignored: build(&spec.patterns)?,
            included: build(&spec.always_include)?,
            send_everything: spec.send_everything,
        })
    }

    /// `rel` uses '/' separators and has no leading slash.
    pub fn is_ignored(&self, rel: &str, is_dir: bool) -> bool {
        if rel.ends_with(TEMP_SUFFIX) {
            return true;
        }
        if self.send_everything {
            return false;
        }
        if self
            .included
            .matched_path_or_any_parents(rel, is_dir)
            .is_ignore()
        {
            return false;
        }
        self.ignored
            .matched_path_or_any_parents(rel, is_dir)
            .is_ignore()
    }

    /// Whether a scan leaves `rel` out. A scan never looks inside a folder
    /// it leaves out, so nothing under one comes back, not even a path that
    /// always include names.
    pub fn leaves_out(&self, rel: &str, is_dir: bool) -> bool {
        self.leaves_out_with(rel, is_dir, &mut HashMap::new())
    }

    /// `leaves_out` for many paths; `seen` keeps each folder's answer.
    pub fn leaves_out_with(
        &self,
        rel: &str,
        is_dir: bool,
        seen: &mut HashMap<String, bool>,
    ) -> bool {
        let above = match rel.rfind('/') {
            Some(i) => self.folder_left_out(&rel[..i], seen),
            None => false,
        };
        above || self.is_ignored(rel, is_dir)
    }

    fn folder_left_out(&self, dir: &str, seen: &mut HashMap<String, bool>) -> bool {
        if let Some(out) = seen.get(dir) {
            return *out;
        }
        let out = self.leaves_out_with(dir, true, seen);
        seen.insert(dir.to_string(), out);
        out
    }
}

#[cfg(test)]
#[path = "ignore_rules_tests.rs"]
mod tests;
