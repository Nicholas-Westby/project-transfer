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
        linked_to: None,
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
    run("echo hi; echo oops >&2", dir.path(), &[], tx).unwrap();
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
    run("exit 3", dir.path(), &[], tx).unwrap();
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
    run("pwd -P", dir.path(), &[], tx).unwrap();
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
    let running = run("sleep 30; echo unreachable", dir.path(), &[], tx).unwrap();
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
    run("printf 'bad\\377\\r\\n'; echo after", dir.path(), &[], tx).unwrap();
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
    let running = run("sleep 30 & echo started", dir.path(), &[], tx).unwrap();
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
    let running = run(line, dir.path(), &[], tx).unwrap();
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
    let err = run("echo hi", &gone, &[], tx).err().unwrap();
    assert_eq!(
        err.to_string(),
        format!(
            "The primary folder {} does not exist on this computer.",
            gone.display()
        )
    );
    assert!(rx.try_recv().is_err());
}

#[cfg(unix)]
#[test]
fn input_values_arrive_exactly_as_typed() {
    let dir = tempfile::tempdir().unwrap();
    let (tx, rx) = mpsc::channel();
    let value = "it's \"done\" `date` $HOME \\n;\nsecond line";
    let env = [("INPUT_NOTES".to_string(), value.to_string())];
    run("printf '%s\\n' \"$INPUT_NOTES\"", dir.path(), &env, tx).unwrap();
    let got = collect(&rx, 10);
    let lines: Vec<&str> = got
        .iter()
        .filter_map(|o| match o {
            CommandOutput::Line { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(lines.join("\n"), value, "{got:?}");
}

#[test]
fn powershell_gets_inputs_as_plain_variables_too() {
    let env = [("INPUT_A".to_string(), "x".to_string())];
    assert_eq!(
        powershell_line("Write-Output \"$INPUT_A\"", &env),
        "$INPUT_A = $env:INPUT_A\nWrite-Output \"$INPUT_A\""
    );
    assert_eq!(powershell_line("dir", &[]), "dir");
    let odd = [("A; Remove-Item x".to_string(), "x".to_string())];
    assert_eq!(powershell_line("dir", &odd), "dir");
}
