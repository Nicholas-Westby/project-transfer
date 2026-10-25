//! `./install` offers to install Rust when cargo is missing. These tests run
//! it with an empty home folder and stand-ins for curl, rustup and cargo, so
//! nothing is downloaded or built.

// macOS keeps cargo out of /usr/bin and /bin, the only real folders on the
// tests' PATH, so each test decides whether there is a cargo to find.
#![cfg(target_os = "macos")]

use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// Serves the stand-in rustup, but only when asked for rustup's script the
/// way rustup.rs says to fetch it.
const CURL: &str = r#"[ "$*" = "--proto =https --tlsv1.2 -sSf https://sh.rustup.rs" ] || exit 22
cat "$(dirname "$0")/../fakes/rustup-init.sh""#;

const OFFLINE_CURL: &str = r#"echo "curl: (6) Could not resolve host: sh.rustup.rs" >&2
exit 6"#;

/// Notes how it ran and puts the stand-in cargo where rustup puts cargo.
const RUSTUP: &str = r#"printf '%s\n' "$*" > "$HOME/rustup-ran-with"
mkdir -p "$HOME/.cargo/bin"
cp "$HOME/../fakes/cargo" "$HOME/.cargo/bin/""#;

/// Notes how it ran.
const CARGO: &str = r#"printf '%s\n' "$*" > "$HOME/cargo-ran-with""#;

/// What an interrupted rustup leaves behind: its cargo, with no Rust for it
/// to run.
const HALF_INSTALLED_CARGO: &str = r#"echo "error: rustup could not choose a version of cargo to run" >&2
exit 1"#;

/// A home folder without Rust, a `bin` folder first on the PATH, and the
/// stand-ins.
struct Sandbox(tempfile::TempDir);

impl Sandbox {
    fn new(curl: &str) -> Self {
        let sandbox = Sandbox(tempfile::tempdir().unwrap());
        std::fs::create_dir(sandbox.path("home")).unwrap();
        sandbox.add_script("bin/curl", curl);
        sandbox.add_script("fakes/rustup-init.sh", RUSTUP);
        sandbox.add_script("fakes/cargo", CARGO);
        sandbox
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.0.path().join(relative)
    }

    fn add_script(&self, relative: &str, body: &str) {
        let path = self.path(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// Runs `./install` with `args`, typing `answer` if it asks.
    fn install(&self, answer: &str, args: &[&str]) -> Output {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let path = format!("{}:/usr/bin:/bin", self.path("bin").display());
        let mut child = Command::new(repo.join("install"))
            .args(args)
            .env_clear()
            .env("HOME", self.path("home"))
            .env("PATH", path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        // A script that doesn't ask can be gone before the answer is written.
        let _ = child.stdin.take().unwrap().write_all(answer.as_bytes());
        child.wait_with_output().unwrap()
    }

    /// What the stand-in `tool` was run with, if it ran.
    fn ran_with(&self, tool: &str) -> Option<String> {
        std::fs::read_to_string(self.path(&format!("home/{tool}-ran-with"))).ok()
    }
}

/// The last line on stderr, which is what the user is left reading.
fn last_words(out: &Output) -> String {
    let stderr = String::from_utf8_lossy(&out.stderr);
    let last = stderr.trim_end().lines().last();
    last.unwrap_or_default().to_string()
}

#[test]
fn yes_installs_rust_and_then_the_app() {
    // Pressing Enter takes the suggested answer.
    for answer in ["y\n", "yes\n", "\n"] {
        let sandbox = Sandbox::new(CURL);
        let out = sandbox.install(answer, &["--dest", "/apps"]);
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{answer:?}: {stderr}");
        // -y keeps rustup from asking a second time.
        assert_eq!(sandbox.ran_with("rustup").as_deref(), Some("-y\n"));
        assert_eq!(
            sandbox.ran_with("cargo").as_deref(),
            Some("xtask install --dest /apps\n")
        );
    }
}

#[test]
fn no_leaves_rust_alone_and_gives_the_command_to_install_it() {
    let sandbox = Sandbox::new(CURL);
    let out = sandbox.install("n\n", &[]);
    assert!(!out.status.success());
    assert_eq!(sandbox.ran_with("rustup"), None);
    let last = last_words(&out);
    assert!(last.ends_with("https://sh.rustup.rs | sh"), "{last}");
}

#[test]
fn no_answer_counts_as_no() {
    let sandbox = Sandbox::new(CURL);
    let out = sandbox.install("", &[]);
    assert!(!out.status.success());
    assert_eq!(sandbox.ran_with("rustup"), None);
    let last = last_words(&out);
    assert!(last.ends_with("https://sh.rustup.rs | sh"), "{last}");
    // Nothing typed ended the question's line, so the script has to.
    let stderr = String::from_utf8_lossy(&out.stderr);
    let ends_question = |line: &str| line.trim_end().ends_with("[Y/n]");
    assert!(stderr.lines().any(ends_question), "{stderr}");
}

#[test]
fn uses_rust_installed_after_the_terminal_opened() {
    let sandbox = Sandbox::new(CURL);
    // Where rustup puts cargo, which isn't on the PATH yet.
    sandbox.add_script("home/.cargo/bin/cargo", CARGO);
    let out = sandbox.install("", &[]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    assert_eq!(sandbox.ran_with("rustup"), None);
    let cargo = sandbox.ran_with("cargo");
    assert_eq!(cargo.as_deref(), Some("xtask install\n"));
}

#[test]
fn an_interrupted_rust_install_is_offered_again() {
    let sandbox = Sandbox::new(CURL);
    sandbox.add_script("home/.cargo/bin/cargo", HALF_INSTALLED_CARGO);
    let out = sandbox.install("y\n", &[]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    assert_eq!(sandbox.ran_with("rustup").as_deref(), Some("-y\n"));
    let cargo = sandbox.ran_with("cargo");
    assert_eq!(cargo.as_deref(), Some("xtask install\n"));
}

#[test]
fn a_failed_download_says_rust_didnt_install() {
    let sandbox = Sandbox::new(OFFLINE_CURL);
    let out = sandbox.install("y\n", &[]);
    assert!(!out.status.success());
    let last = last_words(&out);
    assert!(last.starts_with("Rust didn't install"), "{last}");
}
