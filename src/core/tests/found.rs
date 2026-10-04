use super::*;
use crate::discovery::{Discovered, DiscoveryEvent};
use crate::protocol::{PROTOCOL_VERSION, Reachable};

/// Where this core's server would dial to pass a connection along.
fn found(core: &AppCore) -> HashMap<InstanceId, Vec<SocketAddr>> {
    core.core.shared.found.blocking_read().clone()
}

/// Adding by address finishes in the background.
fn wait_for_found(core: &AppCore, id: InstanceId) -> Vec<SocketAddr> {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(addrs) = found(core).get(&id) {
            return addrs.clone();
        }
        assert!(Instant::now() < end, "{id} was never found");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn a_computer_added_by_address_is_found_at_that_address() {
    let (a, b) = (Fixture::new(), Fixture::new());
    let b_id = b.core.state().me.id;
    let at: SocketAddr = format!("127.0.0.1:{}", b.core.port()).parse().unwrap();
    a.core.act(Action::AddByAddress(at.to_string()));
    assert_eq!(wait_for_found(&a.core, b_id), vec![at]);
}

#[test]
fn discovery_keeps_the_found_addresses_in_step() {
    let f = Fixture::new();
    let id = uuid::Uuid::new_v4();
    let seen_at = |ip: &str| Discovered {
        id,
        name: "Mini Brisk Lynx".into(),
        addrs: vec![format!("{ip}:47820").parse().unwrap()],
        version: PROTOCOL_VERSION,
        via: None,
    };
    let (first, moved) = (seen_at("192.168.1.40"), seen_at("192.168.1.41"));
    f.core
        .core
        .discovery_event(DiscoveryEvent::Found(first.clone()));
    assert_eq!(found(&f.core), HashMap::from([(id, first.addrs)]));
    // A later announcement replaces the addresses rather than adding to them.
    f.core
        .core
        .discovery_event(DiscoveryEvent::Found(moved.clone()));
    assert_eq!(found(&f.core), HashMap::from([(id, moved.addrs.clone())]));
    assert_eq!(f.core.state().discovered, vec![moved]);
    f.core.core.discovery_event(DiscoveryEvent::Lost(id));
    assert!(found(&f.core).is_empty());
    assert!(f.core.state().discovered.is_empty());
}

#[test]
fn what_a_paired_computer_can_reach_is_listed_through_it() {
    let f = Fixture::new();
    let me = f.core.state().me.id;
    let [relay, other_relay, seen, far, farther] = [(); 5].map(|_| uuid::Uuid::new_v4());
    let named = |id, name: &str| Reachable {
        id,
        name: name.into(),
    };
    let through = |id, name: &str, via| Discovered {
        id,
        name: name.into(),
        addrs: vec![],
        version: PROTOCOL_VERSION,
        via: Some(via),
    };
    let here = Discovered {
        id: seen,
        name: "Mini Brisk Lynx".into(),
        addrs: vec!["192.168.1.40:47820".parse().unwrap()],
        version: PROTOCOL_VERSION,
        via: None,
    };
    f.core
        .core
        .discovery_event(DiscoveryEvent::Found(here.clone()));
    // This computer, the relay itself, and one this computer sees directly
    // are listed already, or never.
    let report = vec![
        named(far, "Tower Calm Wren"),
        named(seen, "Mini Brisk Lynx"),
        named(me, "Studio Amber Otter"),
        named(relay, "Desktop Swift Heron"),
    ];
    f.core.core.reachable_through(relay, report);
    let wren = through(far, "Tower Calm Wren", relay);
    assert_eq!(f.core.state().discovered, vec![here.clone(), wren]);
    // Its next answer replaces what it said before.
    f.core
        .core
        .reachable_through(relay, vec![named(farther, "Laptop Quiet Finch")]);
    let finch = through(farther, "Laptop Quiet Finch", relay);
    assert_eq!(f.core.state().discovered, vec![here.clone(), finch]);
    // One computer is listed once, through whichever relay said so last.
    f.core
        .core
        .reachable_through(other_relay, vec![named(farther, "Laptop Quiet Finch")]);
    let finch = through(farther, "Laptop Quiet Finch", other_relay);
    assert_eq!(f.core.state().discovered, vec![here.clone(), finch]);
    // This computer can't reach any of them itself, so it never offers to
    // pass a connection to them along.
    assert_eq!(found(&f.core), HashMap::from([(seen, here.addrs)]));
}

#[test]
fn a_computer_found_through_a_relay_is_logged_once_by_the_relays_name() {
    let f = Fixture::new();
    let desktop = crate::model::Peer {
        id: uuid::Uuid::new_v4(),
        name: "Desktop Swift Heron".into(),
        fingerprint: "ab".repeat(32),
        allows: Default::default(),
        granted: Default::default(),
        last_address: None,
        via: None,
    };
    let relay = desktop.id;
    f.core
        .core
        .ui
        .update(|s| s.peers.push(peers::offline_view(&desktop)));
    let far = uuid::Uuid::new_v4();
    let text = logged(|| {
        // Each poll reports it again.
        for _ in 0..3 {
            let report = vec![Reachable {
                id: far,
                name: "Tower Calm Wren".into(),
            }];
            f.core.core.reachable_through(relay, report);
        }
    });
    let lines: Vec<&str> = text.lines().filter(|l| l.contains("found")).collect();
    assert_eq!(lines.len(), 1, "{text}");
    assert!(
        lines[0].contains("found Tower Calm Wren")
            && lines[0].ends_with("through Desktop Swift Heron"),
        "{text}"
    );
}
