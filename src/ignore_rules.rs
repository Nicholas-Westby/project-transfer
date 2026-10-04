//! Gitignore-style rules deciding which paths take part in a transfer.

use crate::model::InstanceSettings;
use anyhow::Context;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use serde::{Deserialize, Serialize};

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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defaults() -> IgnoreSpec {
        IgnoreSpec {
            patterns: DEFAULT_IGNORES.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn nested_default_is_ignored() {
        let m = Matcher::new(&defaults()).unwrap();
        assert!(m.is_ignored("a/node_modules/x", false));
        assert!(m.is_ignored("a/node_modules", true));
        assert!(m.is_ignored("sub/.DS_Store", false));
        assert!(!m.is_ignored("src/main.rs", false));
    }

    #[test]
    fn always_include_beats_default() {
        let mut spec = defaults();
        spec.always_include = vec!["dist/".into()];
        let m = Matcher::new(&spec).unwrap();
        assert!(!m.is_ignored("dist/app.js", false));
        assert!(m.is_ignored("node_modules/x", false));
    }

    #[test]
    fn send_everything_only_ignores_temp_files() {
        let mut spec = defaults();
        spec.send_everything = true;
        let m = Matcher::new(&spec).unwrap();
        assert!(!m.is_ignored("node_modules/x", false));
        assert!(m.is_ignored("a/file.bin.pt-tmp", false));
    }

    #[test]
    fn invalid_pattern_names_itself() {
        let spec = IgnoreSpec {
            patterns: vec!["{a,b".into()],
            ..Default::default()
        };
        let err = Matcher::new(&spec).err().unwrap();
        assert!(format!("{err:#}").contains("{a,b"));
    }

    #[test]
    fn spec_from_settings_applies_removals_and_extras() {
        let s = InstanceSettings {
            id: uuid::Uuid::new_v4(),
            name: "x".into(),
            projects_folder: "/p".into(),
            extra_ignores: vec!["*.log".into()],
            always_include: vec!["keep/".into()],
            removed_default_ignores: vec!["bin/".into()],
            last_peer: None,
            theme: Default::default(),
            port: 0,
        };
        let spec = IgnoreSpec::from_settings(&s);
        assert!(!spec.patterns.contains(&"bin/".to_string()));
        assert!(spec.patterns.contains(&"obj/".to_string()));
        assert_eq!(spec.patterns.last().unwrap(), "*.log");
        assert_eq!(spec.always_include, vec!["keep/".to_string()]);
        assert!(!spec.send_everything);
    }
}
