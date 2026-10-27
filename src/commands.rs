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

/// PowerShell reads an environment variable as `$env:INPUT_X`; a plain
/// variable of the same name lets `$INPUT_X` work there as it does in sh.
fn powershell_line(line: &str, env: &[(String, String)]) -> String {
    // Names become code here, so only plain ones; inputs never have others.
    let plain = |v: &&String| v.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    let mut script: String = env
        .iter()
        .map(|(var, _)| var)
        .filter(plain)
        // Lines of their own, so the command's first line still starts one,
        // as a `#Requires` must.
        .map(|var| format!("${var} = $env:{var}\n"))
        .collect();
    script.push_str(line);
    script
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
/// `env` holds the values of the command's inputs.
pub fn run(
    line: &str,
    cwd: &Path,
    env: &[(String, String)],
    out: Sender<CommandOutput>,
) -> anyhow::Result<Running> {
    if !cwd.is_dir() {
        anyhow::bail!(
            "The primary folder {} does not exist on this computer.",
            cwd.display()
        );
    }
    let script = match cfg!(windows) {
        true => powershell_line(line, env),
        false => line.to_string(),
    };
    #[cfg(windows)]
    let job = job::Job::new().context("could not set up the command's job object")?;
    let mut child = shell_command(&script)
        .envs(env.iter().map(|(var, value)| (var, value)))
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
#[path = "commands_tests.rs"]
mod tests;
