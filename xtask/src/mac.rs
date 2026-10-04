//! macOS installer: builds Project Transfer.app and puts it in Applications.

use crate::{app_version, build_number, build_release, icon, run};
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

const APP_NAME: &str = "Project Transfer.app";
const EXECUTABLE: &str = "project-transfer";
const LSREGISTER: &str = "/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister";

/// Where the app goes: `--dest`, else /Applications when writable, else
/// ~/Applications.
pub fn choose_destination(dest: Option<PathBuf>, system_writable: bool, home: &Path) -> PathBuf {
    match dest {
        Some(d) => d,
        None if system_writable => PathBuf::from("/Applications"),
        None => home.join("Applications"),
    }
}

/// The iconset file names and pixel sizes iconutil expects.
pub fn iconset_entries() -> Vec<(String, u32)> {
    let mut entries = Vec::new();
    for base in [16u32, 32, 128, 256, 512] {
        entries.push((format!("icon_{base}x{base}.png"), base));
        entries.push((format!("icon_{base}x{base}@2x.png"), base * 2));
    }
    entries
}

pub fn info_plist(version: &str, build: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>CFBundleIdentifier</key>
	<string>local.project-transfer</string>
	<key>CFBundleExecutable</key>
	<string>{EXECUTABLE}</string>
	<key>CFBundleName</key>
	<string>Project Transfer</string>
	<key>CFBundleDisplayName</key>
	<string>Project Transfer</string>
	<key>CFBundlePackageType</key>
	<string>APPL</string>
	<key>CFBundleIconFile</key>
	<string>AppIcon</string>
	<key>CFBundleShortVersionString</key>
	<string>{version}</string>
	<key>CFBundleVersion</key>
	<string>{build}</string>
	<key>LSMinimumSystemVersion</key>
	<string>12.0</string>
	<key>NSPrincipalClass</key>
	<string>NSApplication</string>
	<key>NSHighResolutionCapable</key>
	<true/>
	<key>NSLocalNetworkUsageDescription</key>
	<string>Project Transfer finds and connects to its copies on other computers on your local network.</string>
	<key>NSBonjourServices</key>
	<array>
		<string>_projtransfer._tcp</string>
	</array>
</dict>
</plist>
"#
    )
}

/// Escapes a path for use as an extended regex (pkill -f).
pub fn regex_escape(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars() {
        if "\\.^$|?*+()[]{}".contains(c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn current_uid() -> Result<u32> {
    let out = Command::new("id").arg("-u").output()?;
    String::from_utf8(out.stdout)?
        .trim()
        .parse()
        .context("could not read the user id")
}

fn refuse_root(uid: u32) -> Result<()> {
    if uid == 0 {
        bail!(
            "do not run the installer as root; an app installed by root cannot be replaced later without sudo. Run it as your own user"
        );
    }
    Ok(())
}

fn writable_dir(dir: &Path) -> bool {
    let probe = dir.join(format!(".write-test-{}", std::process::id()));
    let ok = std::fs::create_dir(&probe).is_ok();
    let _ = std::fs::remove_dir(&probe);
    ok
}

/// A scratch directory removed on drop.
struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn install(root: &Path, dest: Option<PathBuf>) -> Result<()> {
    refuse_root(current_uid()?)?;
    let home = PathBuf::from(std::env::var("HOME").context("HOME is not set")?);
    let dest = choose_destination(dest, writable_dir(Path::new("/Applications")), &home);
    std::fs::create_dir_all(&dest)
        .with_context(|| format!("could not create {}", dest.display()))?;
    let dest = dest.canonicalize()?;
    if !writable_dir(&dest) {
        bail!("{} is not writable; pass --dest <folder>", dest.display());
    }

    let version = app_version(root)?;
    let exe = build_release(root)?;

    let scratch = Scratch(std::env::temp_dir().join(format!("pt-install-{}", std::process::id())));
    let _ = std::fs::remove_dir_all(&scratch.0);
    std::fs::create_dir_all(&scratch.0)?;
    let staged = scratch.0.join(APP_NAME);
    make_bundle(root, &staged, &exe, &version, &build_number(root))?;

    // Copy into a hidden folder inside the destination and verify there, so
    // the old copy is untouched until the new one is known to be whole.
    let hidden = make_hidden_dir(&dest)?;
    let new_app = hidden.join(APP_NAME);
    let copied = copy_and_verify(&staged, &new_app);
    if let Err(e) = copied {
        let _ = std::fs::remove_dir_all(&hidden);
        return Err(e);
    }

    let final_app = dest.join(APP_NAME);
    quit_running(&final_app)?;
    swap_in(&new_app, &final_app, &hidden.join("previous.app"))?;
    let _ = std::fs::remove_dir_all(&hidden);

    // Registration is a convenience; a failure must not undo the install.
    let _ = Command::new(LSREGISTER).arg("-f").arg(&final_app).status();
    println!("{}", final_app.display());
    Ok(())
}

fn make_hidden_dir(dest: &Path) -> Result<PathBuf> {
    for n in 0..100 {
        let dir = dest.join(format!(".install-{}-{n}", std::process::id()));
        if std::fs::create_dir(&dir).is_ok() {
            return Ok(dir);
        }
    }
    bail!("could not create a temporary folder in {}", dest.display())
}

fn make_bundle(root: &Path, app: &Path, exe: &Path, version: &str, build: &str) -> Result<()> {
    let contents = app.join("Contents");
    let (macos, resources) = (contents.join("MacOS"), contents.join("Resources"));
    std::fs::create_dir_all(&macos)?;
    std::fs::create_dir_all(&resources)?;
    std::fs::write(contents.join("Info.plist"), info_plist(version, build))?;
    std::fs::write(contents.join("PkgInfo"), "APPL????")?;
    run(Command::new("plutil")
        .args(["-lint", "-s"])
        .arg(contents.join("Info.plist")))?;
    run(Command::new("ditto")
        .args(["--noextattr", "--norsrc"])
        .arg(exe)
        .arg(macos.join(EXECUTABLE)))?;
    make_icns(root, &resources.join("AppIcon.icns"))?;
    run(Command::new("codesign")
        .args(["--force", "--sign", "-"])
        .arg(app))?;
    verify(app)
}

fn make_icns(root: &Path, out: &Path) -> Result<()> {
    let source = image::open(root.join("assets/icon.png"))
        .context("assets/icon.png is missing; run `cargo xtask icon`")?
        .to_rgba8();
    let set = out.with_file_name("AppIcon.iconset");
    std::fs::create_dir_all(&set)?;
    for (name, px) in iconset_entries() {
        icon::resized(&source, px).save(set.join(name))?;
    }
    run(Command::new("iconutil")
        .args(["-c", "icns", "-o"])
        .arg(out)
        .arg(&set))?;
    // The iconset is input only; codesign rejects stray content.
    std::fs::remove_dir_all(&set)?;
    Ok(())
}

fn verify(app: &Path) -> Result<()> {
    run(Command::new("codesign")
        .args(["--verify", "--strict", "--deep"])
        .arg(app))
}

fn copy_and_verify(from: &Path, to: &Path) -> Result<()> {
    run(Command::new("ditto")
        .args(["--noextattr", "--norsrc"])
        .arg(from)
        .arg(to))?;
    verify(to)
}

fn is_running(pattern: &str) -> bool {
    Command::new("pgrep")
        .args(["-f", pattern])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Matches the app's process by its full command line. The end anchor keeps
/// `/A/Project Transfer` from also matching `/A/Project Transfer Beta`.
pub fn running_pattern(app: &Path) -> String {
    let exe = app.join("Contents/MacOS").join(EXECUTABLE);
    format!("^{}( |$)", regex_escape(&exe.to_string_lossy()))
}

/// Asks a running copy to quit, then kills it after ten seconds.
fn quit_running(app: &Path) -> Result<()> {
    let pattern = running_pattern(app);
    if !is_running(&pattern) {
        return Ok(());
    }
    println!("Quitting the running Project Transfer");
    let _ = Command::new("pkill")
        .args(["-TERM", "-f", &pattern])
        .status();
    let deadline = Instant::now() + Duration::from_secs(10);
    while is_running(&pattern) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(200));
    }
    if is_running(&pattern) {
        let _ = Command::new("pkill")
            .args(["-KILL", "-f", &pattern])
            .status();
        std::thread::sleep(Duration::from_millis(500));
    }
    Ok(())
}

/// Old copy aside, new copy in, and the old one is only deleted by the
/// caller removing the hidden folder. On failure the old copy is restored.
fn swap_in(new_app: &Path, final_app: &Path, aside: &Path) -> Result<()> {
    let had_old = final_app.exists();
    if had_old {
        std::fs::rename(final_app, aside).context("could not move the old copy aside")?;
    }
    if let Err(e) = std::fs::rename(new_app, final_app) {
        if had_old && let Err(restore) = std::fs::rename(aside, final_app) {
            bail!(
                "could not install the new copy ({e}) and could not put the old one back ({restore}); it is kept at {}",
                aside.display()
            );
        }
        bail!("could not move the new copy into place: {e}; the old copy was restored");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn destination_prefers_flag_then_system_then_home() {
        let home = Path::new("/Users/me");
        assert_eq!(
            choose_destination(Some("/x".into()), true, home),
            PathBuf::from("/x")
        );
        assert_eq!(
            choose_destination(None, true, home),
            PathBuf::from("/Applications")
        );
        assert_eq!(
            choose_destination(None, false, home),
            PathBuf::from("/Users/me/Applications")
        );
    }

    #[test]
    fn iconset_covers_all_sizes() {
        let entries = iconset_entries();
        let mut sizes: Vec<u32> = entries.iter().map(|e| e.1).collect();
        sizes.sort();
        sizes.dedup();
        assert_eq!(sizes, vec![16, 32, 64, 128, 256, 512, 1024]);
        assert!(entries.contains(&("icon_512x512@2x.png".to_string(), 1024)));
        assert!(entries.contains(&("icon_16x16.png".to_string(), 16)));
    }

    #[test]
    fn plist_has_required_keys() {
        let p = info_plist("1.2.3", "42");
        for needle in [
            "<string>local.project-transfer</string>",
            "<key>CFBundleShortVersionString</key>\n\t<string>1.2.3</string>",
            "<key>CFBundleVersion</key>\n\t<string>42</string>",
            "<key>LSMinimumSystemVersion</key>\n\t<string>12.0</string>",
            "<key>NSLocalNetworkUsageDescription</key>\n\t<string>Project Transfer finds and connects to its copies on other computers on your local network.</string>",
            "<array>\n\t\t<string>_projtransfer._tcp</string>",
            "<key>CFBundleIconFile</key>\n\t<string>AppIcon</string>",
        ] {
            assert!(p.contains(needle), "missing {needle}");
        }
    }

    #[test]
    fn root_is_refused() {
        assert!(refuse_root(0).is_err());
        assert!(refuse_root(501).is_ok());
    }

    #[test]
    fn running_pattern_is_anchored_at_both_ends() {
        let p = running_pattern(Path::new("/Apps/Project Transfer.app"));
        assert!(
            p.starts_with("^/Apps/Project Transfer\\.app/Contents/MacOS/"),
            "{p}"
        );
        assert!(p.ends_with("( |$)"), "{p}");
    }

    #[test]
    fn regex_escape_handles_paths() {
        assert_eq!(regex_escape("/a b/P.app"), "/a b/P\\.app");
    }

    #[test]
    fn swap_replaces_and_restores() {
        let dir = tempfile::tempdir().unwrap();
        let (new, fin, aside) = (
            dir.path().join("new"),
            dir.path().join("final"),
            dir.path().join("aside"),
        );
        std::fs::create_dir(&new).unwrap();
        std::fs::write(new.join("v"), "new").unwrap();
        std::fs::create_dir(&fin).unwrap();
        std::fs::write(fin.join("v"), "old").unwrap();
        swap_in(&new, &fin, &aside).unwrap();
        assert_eq!(std::fs::read_to_string(fin.join("v")).unwrap(), "new");
        assert_eq!(std::fs::read_to_string(aside.join("v")).unwrap(), "old");

        // A missing new copy must put the old one back.
        let gone = dir.path().join("gone");
        let aside2 = dir.path().join("aside2");
        assert!(swap_in(&gone, &fin, &aside2).is_err());
        assert_eq!(std::fs::read_to_string(fin.join("v")).unwrap(), "new");
    }
}
