use super::*;

// The command line is unix shell; the runner itself is tested per OS.
#[cfg(unix)]
#[test]
fn a_command_runs_with_the_values_typed_for_its_inputs() {
    let f = Fixture::new();
    let p = f.create("Garden", "app");
    let line = "printf '%s\\n' \"$INPUT_WORDS\"";
    f.act(Action::AddCommand(p.id, "Say".into(), line.into()));
    let c = f.project().commands[0].clone();
    let hash = Some(crate::commands::command_hash(&c));
    f.act(Action::RunCommand(p.id, c.id, hash.clone(), vec![]));
    let refused = f.last_activity();
    assert_eq!(refused.kind, ActivityKind::Error);
    assert!(
        refused.text.contains("needs a value for Words"),
        "{}",
        refused.text
    );
    assert!(f.core.state().command_runs.is_empty());
    let words = "it's \"secret\"";
    let values = vec![("INPUT_WORDS".to_string(), words.to_string())];
    f.act(Action::RunCommand(p.id, c.id, hash, values));
    wait_until(&f.core, "the command to exit", |s| {
        s.command_runs
            .get(&c.id)
            .is_some_and(|r| r.status == RunStatus::Exited(Some(0)))
    });
    let run = f.core.state().command_runs[&c.id].clone();
    let lines: Vec<&str> = run.lines.iter().map(|l| l.text.as_str()).collect();
    assert_eq!(lines, [words]);
    // The activity, and so the log, names the input but never its value.
    let activity = f.core.state().activity.clone();
    assert!(activity.iter().any(|l| l.text.contains("for Words")));
    assert!(!activity.iter().any(|l| l.text.contains("secret")));
}

#[test]
fn values_do_not_get_a_changed_command_past_its_confirmation() {
    let f = Fixture::new();
    let p = f.create("Garden", "app");
    let line = "echo \"$INPUT_WORDS\"";
    f.act(Action::AddCommand(p.id, "Say".into(), line.into()));
    let c = f.project().commands[0].clone();
    let old = crate::commands::command_hash(&c);
    let edited = format!("{line} \"$INPUT_MORE\"");
    f.act(Action::EditCommand(p.id, c.id, "Say".into(), edited));
    let values = vec![
        ("INPUT_WORDS".to_string(), "a".to_string()),
        ("INPUT_MORE".to_string(), "b".to_string()),
    ];
    f.act(Action::RunCommand(p.id, c.id, Some(old), values));
    let line = f.last_activity();
    assert!(
        line.text.contains("changed after you confirmed it"),
        "{}",
        line.text
    );
    assert!(f.core.state().command_runs.is_empty());
}
