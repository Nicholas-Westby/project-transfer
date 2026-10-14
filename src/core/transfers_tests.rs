use super::*;

fn preview(tag: u128) -> Preview {
    Preview {
        request: TransferRequest {
            peer: uuid::Uuid::from_u128(tag),
            project: uuid::Uuid::from_u128(tag),
            direction: Direction::Push,
            send_everything: false,
        },
        folders: Vec::new(),
        left_out: Vec::new(),
        warnings: Vec::new(),
        link: None,
        description: false,
        sync: None,
    }
}

#[test]
fn a_stale_preview_is_dropped_and_never_paired_with_a_newer_connection() {
    let mut slot: Slot<&str> = Slot::default();
    let first = slot.reset();
    // Dismissed, then a second prepare starts before the first finishes.
    let second = slot.reset();
    assert!(slot.offer(second, "conn B", preview(2)));
    assert!(!slot.offer(first, "conn A", preview(1)));
    let (conn, p) = slot.take_ready().unwrap();
    assert_eq!((conn, p), ("conn B", preview(2)));
    assert!(slot.take_ready().is_none(), "a preview runs at most once");
}

#[test]
fn dismissing_forgets_the_ready_preview() {
    let mut slot: Slot<&str> = Slot::default();
    let g = slot.reset();
    assert!(slot.offer(g, "conn", preview(1)));
    slot.reset();
    assert!(slot.take_ready().is_none());
    assert!(!slot.offer(g, "late", preview(1)));
}

#[test]
fn a_confirmed_sync_is_remembered_as_a_sync_whichever_way_it_goes() {
    let mut p = preview(3);
    assert_eq!(last_started(&p).kind, TransferKind::Push);
    p.request.direction = Direction::Pull;
    assert_eq!(last_started(&p).kind, TransferKind::Pull);
    p.sync = Some(crate::transfer::sync_count::SyncChoice::from_tally(
        Default::default(),
    ));
    let started = last_started(&p);
    assert_eq!(started.kind, TransferKind::Sync);
    assert_eq!(
        (started.project, started.peer),
        (uuid::Uuid::from_u128(3), uuid::Uuid::from_u128(3))
    );
}
