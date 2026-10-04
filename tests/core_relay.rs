//! Three app cores on 127.0.0.1, driven only through actions, as the UI
//! would: W and V are each paired with M, which sees both and passes
//! connections between them. W is never told where V is.

mod core_support;

use core_support::*;
use project_transfer::core::{Action, ActivityKind, AppCore, PromptState, TransferState};
use project_transfer::discovery::Discovered;
use project_transfer::model::{Direction, InstanceId, Peer};
use project_transfer::transfer::TransferRequest;
use std::time::Duration;
use tempfile::TempDir;

struct Trio {
    w: AppCore,
    m: AppCore,
    v: AppCore,
    // Dropped after the cores, which still write there while they run.
    dirs: [TempDir; 3],
}

fn id(c: &AppCore) -> InstanceId {
    c.state().me.id
}

fn name(c: &AppCore) -> String {
    c.state().me.name.clone()
}

/// What `on` stored for the paired computer `id`.
fn stored(on: &AppCore, id: InstanceId) -> Peer {
    on.state().peer(id).expect("paired").peer.clone()
}

/// What `on` lists for the computer `id`.
fn listed(on: &AppCore, id: InstanceId) -> Discovered {
    let s = on.state();
    let found = s.discovered.iter().find(|d| d.id == id);
    found.cloned().expect("listed")
}

/// `from` adds `to` by address, so it sees it there.
fn add(from: &AppCore, to: &AppCore) {
    let to_id = id(to);
    from.act(Action::AddByAddress(format!("127.0.0.1:{}", to.port())));
    wait(from, "the other computer to be found", |s| {
        s.discovered
            .iter()
            .any(|d| d.id == to_id && d.via.is_none())
    });
}

/// `from` pairs with `to`, listed as `target`; both allow everything.
fn pair(from: &AppCore, to: &AppCore, target: Discovered) {
    let (from_id, to_id) = (id(from), id(to));
    from.act(Action::Pair {
        target,
        requested: ALL,
        offered: ALL,
    });
    wait(to, "the pairing prompt", |s| {
        s.pair_prompt
            .as_ref()
            .is_some_and(|p| p.from_id == from_id && p.state == PromptState::Asking)
    });
    to.act(Action::AnswerPair(Some(ALL)));
    from.act(Action::ConfirmPairCode(true));
    wait(from, "the pairing to be saved", |s| s.peer(to_id).is_some());
    wait(to, "the pairing to be saved there", |s| {
        s.peer(from_id).is_some()
    });
}

/// W and V, each paired with M directly, and M seeing both.
fn trio() -> Trio {
    let dirs = [(); 3].map(|_| tempfile::tempdir().unwrap());
    let [w, m, v] = [0, 1, 2].map(|i| start(dirs[i].path()));
    for c in [&w, &v] {
        add(c, &m);
        pair(c, &m, listed(c, id(&m)));
    }
    for c in [&w, &v] {
        add(&m, c);
    }
    Trio { w, m, v, dirs }
}

/// W asks M for its status and learns of V there.
fn listed_through_m(t: &Trio) -> Discovered {
    let (m_id, v_id) = (id(&t.m), id(&t.v));
    t.w.act(Action::SelectPeer(m_id));
    wait(&t.w, "V to be listed through M", |s| {
        s.discovered
            .iter()
            .any(|d| d.id == v_id && d.via == Some(m_id))
    });
    listed(&t.w, v_id)
}

/// W pairs with V, which it found through M.
fn pair_through_m(t: &Trio) {
    let target = listed_through_m(t);
    pair(&t.w, &t.v, target);
}

#[test]
fn a_computer_a_paired_one_sees_is_listed_through_it() {
    let t = trio();
    let v = listed_through_m(&t);
    assert_eq!(v.name, name(&t.v));
    // M passes connections along; it never says where V is.
    assert!(v.addrs.is_empty(), "{v:?}");
}

#[test]
fn a_computer_pairs_with_another_through_one_both_are_paired_with() {
    let t = trio();
    pair_through_m(&t);
    let (w_id, m_id, v_id) = (id(&t.w), id(&t.m), id(&t.v));
    let on_w = stored(&t.w, v_id);
    assert_eq!(on_w.via, Some(m_id));
    // W called M's address, which would never reach V.
    assert_eq!(on_w.last_address, None);
    assert_eq!(stored(&t.v, w_id).via, Some(m_id));
}

#[test]
fn a_push_through_the_computer_in_the_middle_arrives() {
    let t = trio();
    pair_through_m(&t);
    let v_id = id(&t.v);
    let src = t.dirs[0].path().join("work/garden");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("plan.txt"), "rows").unwrap();
    t.w.act(Action::CreateProject {
        name: "garden-planner".into(),
        folder: src,
    });
    t.w.settle();
    let project = t.w.state().projects[0].id;
    t.w.act(Action::Prepare(TransferRequest {
        peer: v_id,
        project,
        direction: Direction::Push,
        send_everything: false,
    }));
    wait(&t.w, "the preview", |s| {
        matches!(s.transfer, TransferState::Ready(_))
    });
    t.w.act(Action::Execute);
    wait(&t.w, "the push to finish", |s| {
        matches!(s.transfer, TransferState::Finished(_))
    });
    let arrived = t.dirs[2].path().join("Dev/garden/plan.txt");
    assert_eq!(std::fs::read_to_string(arrived).unwrap(), "rows");
    // Reached through M each time, so W still knows no address for V.
    assert_eq!(stored(&t.w, v_id).last_address, None);
}

#[test]
fn the_other_computer_reaches_back_through_the_one_in_the_middle() {
    let t = trio();
    pair_through_m(&t);
    let w_id = id(&t.w);
    t.v.act(Action::SelectPeer(w_id));
    wait(&t.v, "W to be online on V", |s| {
        s.peer(w_id).is_some_and(|p| p.online)
    });
    // Through M: no address of W's was learned on the way.
    let on_v = stored(&t.v, w_id);
    assert_eq!(on_v.last_address, None);
    assert_eq!(on_v.via, Some(id(&t.m)));
}

#[test]
fn when_the_computer_in_the_middle_stops_the_other_goes_offline_once() {
    let t = trio();
    pair_through_m(&t);
    let Trio { w, m, v, dirs } = t;
    let (v_id, m_name) = (id(&v), name(&m));
    w.act(Action::SelectPeer(v_id));
    wait(&w, "V to be online", |s| {
        s.peer(v_id).is_some_and(|p| p.online)
    });
    let before = w.state().activity.len();
    drop(m);
    w.act(Action::SelectPeer(v_id));
    wait(&w, "V to be offline", |s| {
        s.peer(v_id).is_some_and(|p| !p.online)
    });
    // Each selection polls again; none of them says it again.
    for _ in 0..3 {
        w.act(Action::SelectPeer(v_id));
        std::thread::sleep(Duration::from_millis(300));
    }
    let s = w.state();
    let warned: Vec<&str> = s.activity[before..]
        .iter()
        .filter(|l| l.kind == ActivityKind::Warn)
        .map(|l| l.text.as_str())
        .collect();
    assert_eq!(warned.len(), 1, "{warned:#?}");
    // The reason names the computer that stopped, not just V.
    assert!(warned[0].contains(&m_name), "{}", warned[0]);
    drop(s);
    drop((w, v, dirs));
}
