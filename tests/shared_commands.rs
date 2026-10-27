//! Shared commands between two computers: every command exchange swaps the
//! list, the links to it and deletions from it.

mod support;

use project_transfer::model::{Command, Os, Project, SharedCommand};
use project_transfer::protocol::{Request, Response};
use project_transfer::transfer::commands::update_commands;
use support::*;

fn release(at: i64) -> SharedCommand {
    SharedCommand {
        id: uuid::Uuid::from_u128(77),
        label: "Release".into(),
        line: "./release.sh".into(),
        created_on: Os::MacOs,
        updated_at_ms: at,
        deleted: false,
    }
}

/// A command that follows `s`, carrying its version as linked ones do.
fn linked(s: &SharedCommand) -> Command {
    Command {
        id: uuid::Uuid::new_v4(),
        label: s.label.clone(),
        line: s.line.clone(),
        created_on: s.created_on,
        updated_at_ms: s.updated_at_ms,
        deleted: false,
        last_run_hash: None,
        linked_to: Some(s.id),
    }
}

/// Garden on `on`, with a command linked to Release, and Release in the list.
async fn garden_with_release(on: &Instance) -> Project {
    let src = on.project_dir("garden");
    write(&src, "plan.txt", "x");
    let mut p = on.add_project("Garden", &[("garden", &src)]).await;
    p.commands = vec![linked(&release(5))];
    on.put_project(p.clone()).await;
    on.put_shared_commands(vec![release(5)]).await;
    p
}

#[tokio::test]
async fn a_push_brings_shared_commands_and_links_along() {
    let (a, b) = pair_full().await;
    let p = garden_with_release(&a).await;
    push(&a, &b, p.id, false).await;
    assert_eq!(b.shared_commands().await, vec![release(5)]);
    let there = b.project(p.id).await.unwrap().commands;
    assert_eq!(there[0].linked_to, Some(release(5).id));
    assert_eq!(
        b.shared.store.load_shared_commands().unwrap(),
        vec![release(5)]
    );
}

#[tokio::test]
async fn an_edit_there_reaches_every_command_linked_here() {
    let (a, b) = pair_full().await;
    let p = garden_with_release(&a).await;
    // Tides follows Release too, but never travels.
    let src = a.project_dir("tides");
    let mut tides = a.add_project("Tides", &[("tides", &src)]).await;
    tides.commands = vec![linked(&release(5))];
    a.put_project(tides.clone()).await;
    push(&a, &b, p.id, false).await;

    update_commands(&b.shared, |_, library| {
        library[0].line = "./release.sh --notes".into();
        library[0].updated_at_ms = 50;
        Ok(())
    })
    .await
    .unwrap();
    pull(&a, &b, p.id).await;

    assert_eq!(a.shared_commands().await[0].line, "./release.sh --notes");
    for id in [p.id, tides.id] {
        let c = &a.project(id).await.unwrap().commands[0];
        assert_eq!(c.line, "./release.sh --notes");
        assert_eq!(c.linked_to, Some(release(5).id));
    }
}

#[tokio::test]
async fn a_deletion_here_unlinks_there_and_the_text_stays() {
    let (a, b) = pair_full().await;
    let p = garden_with_release(&a).await;
    push(&a, &b, p.id, false).await;
    update_commands(&a.shared, |_, library| {
        library[0].deleted = true;
        library[0].updated_at_ms = 60;
        Ok(())
    })
    .await
    .unwrap();
    push(&a, &b, p.id, false).await;

    assert!(b.shared_commands().await[0].deleted);
    let c = &b.project(p.id).await.unwrap().commands[0];
    assert_eq!((c.line.as_str(), c.linked_to), ("./release.sh", None));
}

#[tokio::test]
async fn sync_details_swaps_shared_commands_both_ways() {
    let (a, b) = pair_full().await;
    let p = garden_with_release(&a).await;
    let deploy = SharedCommand {
        id: uuid::Uuid::from_u128(78),
        label: "Deploy".into(),
        ..release(9)
    };
    b.put_shared_commands(vec![deploy.clone()]).await;

    sync(&a, &b, p.id).await.unwrap();

    for side in [&a, &b] {
        let lib = side.shared_commands().await;
        assert_eq!(lib.len(), 2, "{lib:?}");
        assert!(
            lib.contains(&release(5)) && lib.contains(&deploy),
            "{lib:?}"
        );
    }
    let there = &b.project(p.id).await.unwrap().commands[0];
    assert_eq!(there.linked_to, Some(release(5).id));
}

#[tokio::test]
async fn an_asker_that_sends_shared_commands_gets_them_back() {
    let (a, b) = pair_full().await;
    let p = garden_with_release(&a).await;
    push(&a, &b, p.id, false).await;
    let mut conn = a.open(&b).await;
    let ex = Request::ExchangeCommands {
        project: p.id,
        commands: vec![],
        shared: Some(vec![]),
    };
    match conn.request(&ex).await.unwrap() {
        Response::CommandsWithShared { commands, shared } => {
            assert_eq!(commands.len(), 1);
            assert_eq!(shared, vec![release(5)]);
        }
        other => panic!("{other:?}"),
    }
}
