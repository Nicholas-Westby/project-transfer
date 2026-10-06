//! The app's version: every commit carries the next one. The pre-commit hook
//! in `.githooks` runs `cargo xtask bump-version`; `cargo xtask hooks` turns
//! that hook on for a clone.

use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::Command;

const PACKAGE: &str = "project-transfer";

/// "0.1.4" gives "0.1.5". Only the last part moves on its own; the others
/// are changed by hand when a release deserves it.
pub fn next_patch(version: &str) -> Result<String> {
    let parts: Vec<&str> = version.split('.').collect();
    let [major, minor, patch] = parts.as_slice() else {
        bail!("the version {version} is not three numbers, such as 0.1.4");
    };
    let number = |s: &str| {
        s.parse::<u64>()
            .with_context(|| format!("the version {version} is not three numbers, such as 0.1.4"))
    };
    let (major, minor, patch) = (number(major)?, number(minor)?, number(patch)?);
    Ok(format!("{major}.{minor}.{}", patch + 1))
}

/// The version in the `[package]` section of a Cargo.toml.
pub fn package_version(toml: &str) -> Result<String> {
    let lines: Vec<&str> = toml.split('\n').collect();
    let at = package_version_line(&lines)?;
    Ok(quoted(lines[at]).to_string())
}

/// Cargo.toml with its `[package]` version set to `new`.
pub fn with_package_version(toml: &str, new: &str) -> Result<String> {
    let mut lines: Vec<String> = toml.split('\n').map(str::to_owned).collect();
    let at = package_version_line(&lines.iter().map(String::as_str).collect::<Vec<_>>())?;
    lines[at] = requoted(&lines[at], new);
    Ok(lines.join("\n"))
}

/// Cargo.lock with the version recorded for `package` set to `new`, so a
/// build after the bump doesn't change the lock file again.
pub fn with_lock_version(lock: &str, package: &str, new: &str) -> Result<String> {
    let mut lines: Vec<String> = lock.split('\n').map(str::to_owned).collect();
    let name = format!("name = \"{package}\"");
    let at = lines
        .windows(2)
        .position(|w| w[0] == name && w[1].starts_with("version = \""))
        .with_context(|| format!("Cargo.lock has no entry for {package}"))?;
    lines[at + 1] = requoted(&lines[at + 1], new);
    Ok(lines.join("\n"))
}

/// The index of the `version = "..."` line in the `[package]` table.
fn package_version_line(lines: &[&str]) -> Result<usize> {
    let mut in_package = false;
    for (i, line) in lines.iter().enumerate() {
        let t = line.trim();
        if t.starts_with('[') {
            in_package = t == "[package]";
        } else if in_package && t.starts_with("version") && t.contains('"') {
            return Ok(i);
        }
    }
    bail!("Cargo.toml has no version in its [package] section")
}

/// The text between the first and last double quote of a line.
fn quoted(line: &str) -> &str {
    let start = line.find('"').map_or(0, |i| i + 1);
    let end = line.rfind('"').unwrap_or(line.len()).max(start);
    &line[start..end]
}

fn requoted(line: &str, value: &str) -> String {
    let start = line.find('"').map_or(0, |i| i + 1);
    let end = line.rfind('"').unwrap_or(line.len()).max(start);
    format!("{}{value}{}", &line[..start], &line[end..])
}

/// Bumps the version in Cargo.toml and Cargo.lock and stages both.
pub fn bump(root: &Path) -> Result<String> {
    let unstaged = git(
        root,
        &["diff", "--name-only", "--", "Cargo.toml", "Cargo.lock"],
    )?;
    if !unstaged.trim().is_empty() {
        bail!(
            "Cargo.toml or Cargo.lock has changes that aren't staged. Stage or stash them, \
             so the version bump doesn't carry them into this commit."
        );
    }
    let (toml_path, lock_path) = (root.join("Cargo.toml"), root.join("Cargo.lock"));
    let toml = std::fs::read_to_string(&toml_path).context("could not read Cargo.toml")?;
    let next = next_patch(&package_version(&toml)?)?;
    let lock = std::fs::read_to_string(&lock_path).context("could not read Cargo.lock")?;
    std::fs::write(&toml_path, with_package_version(&toml, &next)?)?;
    std::fs::write(&lock_path, with_lock_version(&lock, PACKAGE, &next)?)?;
    git(root, &["add", "Cargo.toml", "Cargo.lock"])?;
    Ok(next)
}

/// Points this clone's git at the hooks tracked in `.githooks`.
pub fn install_hooks(root: &Path) -> Result<()> {
    git(root, &["config", "core.hooksPath", ".githooks"]).map(|_| ())
}

pub fn git(root: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .context("could not run git")?;
    if !out.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    const TOML: &str = "[workspace]\nmembers = [\".\"]\n\n[package]\nname = \"project-transfer\"\n\
        version = \"0.1.4\"\nedition = \"2024\"\n\n[dependencies]\nfoo = { version = \"1.2.3\" }\n";

    const LOCK: &str = "[[package]]\nname = \"foo\"\nversion = \"1.2.3\"\n\n[[package]]\n\
        name = \"project-transfer\"\nversion = \"0.1.4\"\ndependencies = [\n \"foo\",\n]\n";

    #[test]
    fn the_last_part_counts_up() {
        assert_eq!(next_patch("0.1.4").unwrap(), "0.1.5");
        assert_eq!(next_patch("1.0.9").unwrap(), "1.0.10");
        assert!(next_patch("0.1").is_err());
        assert!(next_patch("0.1.x").is_err());
    }

    #[test]
    fn only_the_package_version_changes_in_cargo_toml() {
        assert_eq!(package_version(TOML).unwrap(), "0.1.4");
        let bumped = with_package_version(TOML, "0.1.5").unwrap();
        assert_eq!(package_version(&bumped).unwrap(), "0.1.5");
        assert!(bumped.contains("foo = { version = \"1.2.3\" }"));
        assert_eq!(bumped.len(), TOML.len());
        assert!(package_version("[workspace]\n").is_err());
    }

    #[test]
    fn only_this_package_changes_in_cargo_lock() {
        let bumped = with_lock_version(LOCK, PACKAGE, "0.1.5").unwrap();
        assert!(bumped.contains("name = \"project-transfer\"\nversion = \"0.1.5\"\n"));
        assert!(bumped.contains("name = \"foo\"\nversion = \"1.2.3\"\n"));
        assert!(with_lock_version(LOCK, "missing", "0.1.5").is_err());
    }
}
