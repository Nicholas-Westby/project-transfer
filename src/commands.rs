//! Runs a project's commands on this computer and streams their output.
use crate::model::Command;
use anyhow::Context;
use sha2::{Digest, Sha256};
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;

#[derive(Clone, Debug, PartialEq)]
pub enum CommandOutput {
    Line {
        stderr: bool,
        text: String,
    },
    /// None when the process was ended by a signal, for example by Stop.
    Exited(Option<i32>),
    Failed(String),
}

#[cfg(windows)]
#[path = "commands_job.rs"]
mod job;

pub struct Running {
    /// Unix stops the command's process group by its id.
    #[cfg(unix)]
    pid: u32,
    /// Windows stops every process in the command's job.
    #[cfg(windows)]
    job: job::Job,
    // Guards against signalling a recycled pid after the command has exited.
    done: Arc<AtomicBool>,
}

impl Running {
    pub fn stop(&self) {
        if self.done.load(Ordering::SeqCst) {
            return;
        }
        #[cfg(unix)]
        // The command runs in its own process group, so this also reaches
        // anything the shell started (a `sleep`, a dev server).
        // SAFETY: kill(2) takes plain integers and touches no Rust memory. The
        // negative pid targets the group we created; `done` is only set once
        // every group member has closed its pipes, so the pgid cannot have
        // been recycled.
        unsafe {
            libc::kill(-(self.pid as i32), libc::SIGKILL);
        }
        #[cfg(windows)]
        // Ends the shell and everything it started, so their pipes close and
        // the reader threads finish.
        self.job.terminate();
    }
}

pub fn command_hash(cmd: &Command) -> String {
    Sha256::digest(cmd.line.as_bytes())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// True for commands that never ran here or whose text changed since they did.
pub fn needs_confirmation(cmd: &Command) -> bool {
    cmd.last_run_hash.as_deref() != Some(command_hash(cmd).as_str())
}

/// Whether the user must see and confirm the text before it runs: new or
/// changed here, or written for another system.
pub fn must_confirm(cmd: &Command) -> bool {
    needs_confirmation(cmd) || cmd.created_on != crate::model::Os::current()
}

#[cfg(not(windows))]
fn shell_command(line: &str) -> std::process::Command {
    let shell = std::env::var("SHELL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/bin/zsh".to_string());
    let mut c = std::process::Command::new(shell);
    c.arg("-lc").arg(line);
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut c, 0);
    c
}

#[cfg(windows)]
fn shell_command(line: &str) -> std::process::Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut c = std::process::Command::new("powershell");
    c.args(["-NoProfile", "-Command", line]);
    c.creation_flags(CREATE_NO_WINDOW);
    c
}

fn pump(
    r: impl Read + Send + 'static,
    stderr: bool,
    out: Sender<CommandOutput>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut r = BufReader::new(r);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            // Keep reading on bad UTF-8: dropping the pipe would give the
            // command a broken-pipe error mid-run.
            if !matches!(r.read_until(b'\n', &mut buf), Ok(n) if n > 0) {
                break;
            }
            while matches!(buf.last(), Some(b'\n' | b'\r')) {
                buf.pop();
            }
            let text = String::from_utf8_lossy(&buf).into_owned();
            if out.send(CommandOutput::Line { stderr, text }).is_err() {
                break;
            }
        }
    })
}

/// Errors (missing folder, shell cannot start) are only returned, never sent on
/// `out`; `CommandOutput::Failed` is for failures after the command started.
pub fn run(line: &str, cwd: &Path, out: Sender<CommandOutput>) -> anyhow::Result<Running> {
    if !cwd.is_dir() {
        anyhow::bail!(
            "The primary folder {} does not exist on this computer.",
            cwd.display()
        );
    }
    #[cfg(windows)]
    let job = job::Job::new().context("could not set up the command's job object")?;
    let mut child = shell_command(line)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("could not start the shell")?;
    #[cfg(windows)]
    if let Err(e) = job.assign(&child) {
        let _ = child.kill();
        return Err(e).context("could not put the command in a job object");
    }
    #[cfg(unix)]
    let pid = child.id();
    let readers = [
        child.stdout.take().map(|s| pump(s, false, out.clone())),
        child.stderr.take().map(|s| pump(s, true, out.clone())),
    ];
    let done = Arc::new(AtomicBool::new(false));
    let d = done.clone();
    std::thread::spawn(move || {
        let status = child.wait().map_err(|e| e.to_string());
        // Drain output first so the exit code is always the last event. Group
        // members (`sleep 30 &`) can hold the pipes after the shell exits, and
        // Stop must keep working until they are gone.
        for r in readers.into_iter().flatten() {
            let _ = r.join();
        }
        d.store(true, Ordering::SeqCst);
        let _ = out.send(match status {
            Ok(s) => CommandOutput::Exited(s.code()),
            Err(e) => CommandOutput::Failed(e),
        });
    });
    Ok(Running {
        #[cfg(unix)]
        pid,
        #[cfg(windows)]
        job,
        done,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Os;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    fn cmd(line: &str, last: Option<String>) -> Command {
        Command {
            id: uuid::Uuid::new_v4(),
            label: "x".into(),
            line: line.into(),
            created_on: Os::MacOs,
            updated_at_ms: 0,
            deleted: false,
            last_run_hash: last,
        }
    }

    fn collect(rx: &mpsc::Receiver<CommandOutput>, secs: u64) -> Vec<CommandOutput> {
        let end = Instant::now() + Duration::from_secs(secs);
        let mut all = vec![];
        while let Some(left) = end.checked_duration_since(Instant::now()) {
            match rx.recv_timeout(left) {
                Ok(o) => {
                    let last = matches!(o, CommandOutput::Exited(_) | CommandOutput::Failed(_));
                    all.push(o);
                    if last {
                        break;
                    }
                }
                Err(_) => break,
            }
        }
        all
    }

    #[test]
    fn hash_is_sha256_of_the_line() {
        assert_eq!(
            command_hash(&cmd("abc", None)),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn confirmation_tracks_the_line() {
        let c = cmd("make", None);
        assert!(needs_confirmation(&c));
        let ran = cmd("make", Some(command_hash(&c)));
        assert!(!needs_confirmation(&ran));
        let edited = Command {
            line: "make all".into(),
            ..ran
        };
        assert!(needs_confirmation(&edited));
    }

    #[cfg(unix)]
    #[test]
    fn streams_output_then_exit_code() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::channel();
        run("echo hi; echo oops >&2", dir.path(), tx).unwrap();
        let got = collect(&rx, 10);
        assert!(got.contains(&CommandOutput::Line {
            stderr: false,
            text: "hi".into()
        }));
        assert!(got.contains(&CommandOutput::Line {
            stderr: true,
            text: "oops".into()
        }));
        assert_eq!(got.last(), Some(&CommandOutput::Exited(Some(0))));
    }

    #[cfg(unix)]
    #[test]
    fn reports_exit_code() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::channel();
        run("exit 3", dir.path(), tx).unwrap();
        assert_eq!(
            collect(&rx, 10).last(),
            Some(&CommandOutput::Exited(Some(3)))
        );
    }

    #[cfg(unix)]
    #[test]
    fn runs_in_the_given_folder() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::channel();
        run("pwd -P", dir.path(), tx).unwrap();
        let want = dir.path().canonicalize().unwrap().display().to_string();
        let got = collect(&rx, 10);
        assert!(got.contains(&CommandOutput::Line {
            stderr: false,
            text: want
        }));
    }

    #[cfg(unix)]
    #[test]
    fn stop_kills_children_too() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::channel();
        let running = run("sleep 30; echo unreachable", dir.path(), tx).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        let t = Instant::now();
        running.stop();
        let got = collect(&rx, 2);
        assert!(
            matches!(got.last(), Some(CommandOutput::Exited(_))),
            "{got:?}"
        );
        assert!(t.elapsed() < Duration::from_secs(2));
        assert!(!got.iter().any(|o| matches!(o, CommandOutput::Line { .. })));
    }

    #[cfg(unix)]
    #[test]
    fn invalid_utf8_does_not_end_the_stream() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::channel();
        run("printf 'bad\\377\\r\\n'; echo after", dir.path(), tx).unwrap();
        let got = collect(&rx, 10);
        assert!(
            got.contains(&CommandOutput::Line {
                stderr: false,
                text: "bad\u{fffd}".into()
            }),
            "{got:?}"
        );
        assert!(got.contains(&CommandOutput::Line {
            stderr: false,
            text: "after".into()
        }));
        assert_eq!(got.last(), Some(&CommandOutput::Exited(Some(0))));
    }

    #[cfg(unix)]
    #[test]
    fn stop_works_while_a_background_child_holds_the_pipes() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::channel();
        let running = run("sleep 30 & echo started", dir.path(), tx).unwrap();
        assert!(matches!(
            rx.recv_timeout(Duration::from_secs(5)),
            Ok(CommandOutput::Line { .. })
        ));
        std::thread::sleep(Duration::from_millis(300));
        let t = Instant::now();
        running.stop();
        let got = collect(&rx, 2);
        assert!(
            matches!(got.last(), Some(CommandOutput::Exited(_))),
            "{got:?}"
        );
        assert!(t.elapsed() < Duration::from_secs(2));
    }

    #[cfg(windows)]
    #[test]
    fn stop_ends_what_the_shell_started_on_windows() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::channel();
        // ping is a child process holding the inherited stderr pipe, as a dev
        // server would; killing only PowerShell would leave it running.
        let line = "Write-Output started; ping -n 30 127.0.0.1 > $null";
        let running = run(line, dir.path(), tx).unwrap();
        assert!(matches!(
            rx.recv_timeout(Duration::from_secs(20)),
            Ok(CommandOutput::Line { .. })
        ));
        let t = Instant::now();
        running.stop();
        let got = collect(&rx, 5);
        assert!(
            matches!(got.last(), Some(CommandOutput::Exited(_))),
            "{got:?}"
        );
        assert!(t.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn missing_folder_fails_clearly() {
        let dir = tempfile::tempdir().unwrap();
        let gone = dir.path().join("gone");
        let (tx, rx) = mpsc::channel();
        let err = run("echo hi", &gone, tx).err().unwrap();
        assert_eq!(
            err.to_string(),
            format!(
                "The primary folder {} does not exist on this computer.",
                gone.display()
            )
        );
        assert!(rx.try_recv().is_err());
    }
}
