//! Windows installer: copies the binary and adds a Start menu shortcut.

use crate::{build_release, run};
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

const EXE_NAME: &str = "Project Transfer.exe";

pub fn default_destination(local_app_data: &Path) -> PathBuf {
    local_app_data.join("Programs").join("Project Transfer")
}

/// PowerShell that creates the shortcut through the WScript.Shell COM object.
pub fn shortcut_script(lnk: &Path, target: &Path) -> String {
    let q = |p: &Path| ps_quote(&p.to_string_lossy());
    format!(
        "$s = (New-Object -ComObject WScript.Shell).CreateShortcut('{}'); $s.TargetPath = '{}'; $s.WorkingDirectory = '{}'; $s.Save()",
        q(lnk),
        q(target),
        q(target.parent().unwrap_or(Path::new(".")))
    )
}

/// Escapes text for a single-quoted PowerShell string. PowerShell also ends
/// such a string at the typographic single quotes, so those are doubled too.
fn ps_quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '\'' | '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}') {
            out.push(c);
        }
        out.push(c);
    }
    out
}

fn is_running() -> bool {
    Command::new("tasklist")
        .args(["/FI", &format!("IMAGENAME eq {EXE_NAME}"), "/NH"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains(EXE_NAME))
        .unwrap_or(false)
}

/// Asks a running copy to close, then force-stops it after ten seconds.
fn stop_running() {
    if !is_running() {
        return;
    }
    println!("Quitting the running Project Transfer");
    let _ = Command::new("taskkill").args(["/IM", EXE_NAME]).status();
    let deadline = Instant::now() + Duration::from_secs(10);
    while is_running() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(200));
    }
    if is_running() {
        let _ = Command::new("taskkill")
            .args(["/F", "/IM", EXE_NAME])
            .status();
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// Old copy aside, new copy in, old copy deleted. On failure the old copy is
/// put back.
fn swap_in(staged: &Path, target: &Path, old: &Path) -> Result<()> {
    if old.exists() {
        if target.exists() {
            let _ = std::fs::remove_file(old);
        } else {
            // A run that died between the two renames left `.old` as the
            // only copy of the app.
            std::fs::rename(old, target).context("could not restore the previous copy")?;
        }
    }
    let had_old = target.exists();
    if had_old {
        std::fs::rename(target, old).context("could not move the old copy aside")?;
    }
    if let Err(e) = std::fs::rename(staged, target) {
        if had_old {
            let _ = std::fs::rename(old, target);
        }
        bail!("could not move the new copy into place: {e}");
    }
    let _ = std::fs::remove_file(old);
    Ok(())
}

pub fn install(root: &Path, dest: Option<PathBuf>) -> Result<()> {
    let dest = match dest {
        Some(d) => d,
        None => default_destination(&PathBuf::from(
            std::env::var("LOCALAPPDATA").context("LOCALAPPDATA is not set")?,
        )),
    };
    let exe = build_release(root)?;
    std::fs::create_dir_all(&dest)?;

    // Stage beside the target so the final rename stays on one volume.
    let staged = dest.join("Project Transfer.exe.new");
    let target = dest.join(EXE_NAME);
    let old = dest.join("Project Transfer.exe.old");
    std::fs::copy(&exe, &staged).context("could not copy the new build")?;

    stop_running();
    swap_in(&staged, &target, &old)?;

    let appdata = std::env::var("APPDATA").context("APPDATA is not set")?;
    let programs = PathBuf::from(appdata).join(r"Microsoft\Windows\Start Menu\Programs");
    let lnk = programs.join("Project Transfer.lnk");
    run(Command::new("powershell").args([
        "-NoProfile",
        "-NonInteractive",
        "-Command",
        &shortcut_script(&lnk, &target),
    ]))?;
    println!("{}", target.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destination_is_under_programs() {
        let d = default_destination(Path::new("C:/Users/me/AppData/Local"));
        assert!(d.ends_with("Programs/Project Transfer"));
    }

    fn file(dir: &Path, name: &str, body: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, body).unwrap();
        p
    }

    #[test]
    fn swap_replaces_and_clears_a_stale_old_copy() {
        let dir = tempfile::tempdir().unwrap();
        let staged = file(dir.path(), "a.new", "new");
        let target = file(dir.path(), "a.exe", "current");
        let old = file(dir.path(), "a.old", "stale");
        swap_in(&staged, &target, &old).unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new");
        assert!(!old.exists() && !staged.exists());
    }

    #[test]
    fn swap_keeps_the_only_copy_left_by_an_interrupted_run() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("a.exe");
        let old = file(dir.path(), "a.old", "previous");
        // A missing staged file makes the final rename fail.
        let err = swap_in(&dir.path().join("a.new"), &target, &old);
        assert!(err.is_err());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "previous");
    }

    #[test]
    fn shortcut_script_quotes_paths() {
        let s = shortcut_script(
            Path::new("C:/a/It's.lnk"),
            Path::new("C:/p/Project Transfer.exe"),
        );
        assert!(s.contains("It''s.lnk"));
        assert!(s.contains("TargetPath = 'C:/p/Project Transfer.exe'"));
    }

    #[test]
    fn typographic_quotes_are_doubled_too() {
        let s = shortcut_script(
            Path::new("C:/Users/Jane\u{2019}s/a\u{2018}b\u{201A}c\u{201B}d.lnk"),
            Path::new("C:/p/x.exe"),
        );
        assert!(
            s.contains(
                "Jane\u{2019}\u{2019}s/a\u{2018}\u{2018}b\u{201A}\u{201A}c\u{201B}\u{201B}d.lnk"
            ),
            "{s}"
        );
    }
}
