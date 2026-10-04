//! Project tooling: `cargo xtask install`, `icon`, `bump-version` and `hooks`.

mod icon;
mod mac;
mod version;
mod windows;

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;

const USAGE: &str = "usage: cargo xtask <command>\n\n  install [--dest <folder>]   build the app and install it\n  icon                        render assets/icon.png (and icon.ico)\n  bump-version                count the version up and stage it (the pre-commit hook runs this)\n  hooks                       turn on the pre-commit hook for this clone";

#[derive(Debug, PartialEq)]
enum Task {
    Install { dest: Option<PathBuf> },
    Icon,
    BumpVersion,
    Hooks,
}

fn parse_args(args: &[String]) -> Result<Task> {
    match args.first().map(String::as_str) {
        Some("icon") if args.len() == 1 => Ok(Task::Icon),
        Some("bump-version") if args.len() == 1 => Ok(Task::BumpVersion),
        Some("hooks") if args.len() == 1 => Ok(Task::Hooks),
        Some("install") => {
            let mut dest = None;
            let mut rest = args[1..].iter();
            while let Some(arg) = rest.next() {
                match arg.as_str() {
                    "--dest" => {
                        let value = rest.next().context("--dest needs a folder")?;
                        dest = Some(PathBuf::from(value));
                    }
                    other => bail!("unknown option {other}\n{USAGE}"),
                }
            }
            Ok(Task::Install { dest })
        }
        _ => bail!("{USAGE}"),
    }
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match parse_args(&args)? {
        Task::Icon => icon::render_files(&repo_root()),
        Task::BumpVersion => {
            let v = version::bump(&repo_root())?;
            println!("Version {v}");
            Ok(())
        }
        Task::Hooks => {
            version::install_hooks(&repo_root())?;
            println!("Every commit in this clone now counts the version up.");
            Ok(())
        }
        Task::Install { dest } => {
            // Resolve now: the installer changes nothing about the working
            // directory, but a relative path should mean the caller's.
            let dest = dest.map(|d| std::path::absolute(&d)).transpose()?;
            if cfg!(target_os = "macos") {
                mac::install(&repo_root(), dest)
            } else if cfg!(windows) {
                windows::install(&repo_root(), dest)
            } else {
                bail!("the installer supports macOS and Windows only")
            }
        }
    }
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives inside the workspace")
        .to_path_buf()
}

/// Builds the release binary and returns its path.
fn build_release(root: &Path) -> Result<PathBuf> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    run(Command::new(cargo).current_dir(root).args([
        "build",
        "--release",
        "--package",
        "project-transfer",
    ]))?;
    let exe = if cfg!(windows) {
        "project-transfer.exe"
    } else {
        "project-transfer"
    };
    let path = target_dir(root)?.join("release").join(exe);
    if !path.is_file() {
        bail!("the build finished but {} is missing", path.display());
    }
    Ok(path)
}

fn target_dir(root: &Path) -> Result<PathBuf> {
    let meta = metadata(root)?;
    Ok(PathBuf::from(
        meta["target_directory"]
            .as_str()
            .context("cargo metadata has no target_directory")?,
    ))
}

fn metadata(root: &Path) -> Result<serde_json::Value> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let out = Command::new(cargo)
        .current_dir(root)
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .output()
        .context("could not run cargo metadata")?;
    if !out.status.success() {
        bail!("cargo metadata failed");
    }
    Ok(serde_json::from_slice(&out.stdout)?)
}

/// The app crate's version from Cargo.toml.
fn app_version(root: &Path) -> Result<String> {
    let meta = metadata(root)?;
    meta["packages"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|p| p["name"] == "project-transfer")
        .and_then(|p| p["version"].as_str())
        .map(str::to_owned)
        .context("project-transfer is not in the workspace")
}

/// Runs a command, failing with its name when it exits non-zero.
fn run(cmd: &mut Command) -> Result<()> {
    let name = format!("{:?}", cmd.get_program());
    let status = cmd
        .status()
        .with_context(|| format!("could not start {name}"))?;
    if !status.success() {
        bail!("{name} failed ({status})");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_commands() {
        assert_eq!(parse_args(&args(&["icon"])).unwrap(), Task::Icon);
        assert_eq!(
            parse_args(&args(&["bump-version"])).unwrap(),
            Task::BumpVersion
        );
        assert_eq!(parse_args(&args(&["hooks"])).unwrap(), Task::Hooks);
        assert_eq!(
            parse_args(&args(&["install"])).unwrap(),
            Task::Install { dest: None }
        );
        assert_eq!(
            parse_args(&args(&["install", "--dest", "/x"])).unwrap(),
            Task::Install {
                dest: Some("/x".into())
            }
        );
    }

    #[test]
    fn rejects_bad_input() {
        assert!(parse_args(&args(&[])).is_err());
        assert!(parse_args(&args(&["install", "--dest"])).is_err());
        assert!(parse_args(&args(&["install", "--bogus"])).is_err());
        assert!(parse_args(&args(&["nope"])).is_err());
    }
}
