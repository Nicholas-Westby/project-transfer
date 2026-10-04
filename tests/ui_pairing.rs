//! Pairing and the Computers sheet, over a hand-built state.

mod ui_support;

use egui_kittest::kittest::Queryable;
use project_transfer::core::{Action, OutgoingPairView, PairState, PromptState};
use ui_support::{FakeBackend, ui_harness};

#[test]
fn unpairing_asks_first() {
    let fake = FakeBackend::seeded();
    let mut h = ui_harness(fake.clone());
    ui_support::open_computers(&mut h);
    h.get_all_by_label("Unpair").next().unwrap().click();
    h.run();
    h.get_by_label("Unpair Desktop Swift Heron?");
    assert!(fake.actions().is_empty());
    h.get_all_by_label("Unpair").last().unwrap().click();
    h.run();
    assert!(matches!(fake.actions()[..], [Action::Unpair(_)]));
}

#[test]
fn the_pair_prompt_shows_the_code_and_answers() {
    let fake = FakeBackend::seeded();
    fake.update(ui_support::with_pair_prompt);
    let mut h = ui_harness(fake.clone());
    h.get_by_label("481 205");
    h.get_by_label("Check that both screens show the same code.");
    h.get_by_label("Mini Brisk Lynx asks to be allowed to push and pull.");
    h.get_by_label("Pair").click();
    h.run();
    // Push starts unticked even though it was asked for; pull follows the request.
    match &fake.actions()[..] {
        [Action::AnswerPair(Some(allows))] => {
            assert!(!allows.may_push_to_me && allows.may_pull_from_me);
        }
        other => panic!("expected one AnswerPair, got {other:?}"),
    }
}

#[test]
fn the_starting_computer_confirms_or_cancels_the_code() {
    for (button, yes) in [("Codes match", true), ("Cancel", false)] {
        let fake = FakeBackend::seeded();
        fake.update(|s| {
            s.pairing = Some(OutgoingPairView {
                target_name: "Mini Brisk Lynx".into(),
                code: Some("481 205".into()),
                state: PairState::Confirm,
                other_accepted: true,
            })
        });
        let mut h = ui_harness(fake.clone());
        h.get_by_label("Does Mini Brisk Lynx show this code?");
        h.get_by_label("481 205");
        h.get_by_label_contains("accepted");
        h.get_by_label(button).click();
        h.run();
        assert_eq!(fake.actions(), vec![Action::ConfirmPairCode(yes)]);
    }
}

#[test]
fn an_accepted_prompt_waits_and_can_be_cancelled() {
    let fake = FakeBackend::seeded();
    fake.update(|s| {
        ui_support::with_pair_prompt(s);
        s.pair_prompt.as_mut().unwrap().state = PromptState::Waiting;
    });
    let mut h = ui_harness(fake.clone());
    h.get_by_label("Waiting for Mini Brisk Lynx to confirm the code.");
    h.get_by_label("Cancel pairing").click();
    // The spinner keeps asking for frames, so step instead of running to rest.
    h.run_steps(2);
    assert_eq!(fake.actions(), vec![Action::DismissPairPrompt]);
}

#[test]
fn a_computer_reached_through_a_paired_one_says_which() {
    let fake = FakeBackend::seeded();
    fake.update(ui_support::with_relay);
    let mut h = ui_harness(fake);
    ui_support::open_computers(&mut h);
    // Where the address would be, and under the paired computer's name.
    h.get_by_label("through Desktop Swift Heron");
    h.get_by_label("Reached through Desktop Swift Heron.");
}

#[test]
fn a_relay_no_longer_paired_is_called_another_computer() {
    let fake = FakeBackend::seeded();
    fake.update(|s| {
        ui_support::with_relay(s);
        let gone = Some(uuid::Uuid::from_u128(999));
        s.discovered.last_mut().unwrap().via = gone;
        s.peers[1].peer.via = gone;
    });
    let mut h = ui_harness(fake);
    ui_support::open_computers(&mut h);
    h.get_by_label("through another computer");
    h.get_by_label("Reached through another computer.");
}

#[test]
fn pairing_asks_before_letting_the_other_computer_overwrite_files() {
    let fake = FakeBackend::seeded();
    let mut h = ui_harness(fake.clone());
    ui_support::open_computers(&mut h);
    h.get_by_label("Pair").click();
    h.run();
    h.get_by_label("Pair").click();
    h.run();
    match &fake.actions()[..] {
        [
            Action::Pair {
                offered, requested, ..
            },
        ] => {
            assert!(!offered.may_push_to_me && offered.may_pull_from_me);
            assert!(!requested.may_push_to_me && requested.may_pull_from_me);
        }
        other => panic!("expected one Pair, got {other:?}"),
    }
}
