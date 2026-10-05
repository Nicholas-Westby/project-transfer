//! Where things sit in the main window, down to its smallest size.

mod ui_support;

use egui_kittest::kittest::{NodeT, Queryable};
use project_transfer::core::{ActivityKind, ActivityLine};
use ui_support::{FakeBackend, NoPicker, build};

/// The smallest window `main` allows.
const SMALLEST: [f32; 2] = [900.0, 600.0];

#[test]
fn pull_comes_before_push() {
    let h = build(
        FakeBackend::seeded(),
        Box::new(NoPicker),
        SMALLEST,
        1.0,
        false,
    );
    let left = |label: &str| h.get_by_label(label).rect().left();
    assert!(left("Pull from Desktop Swift Heron") < left("Push to Desktop Swift Heron"));
}

#[test]
fn nothing_runs_past_the_right_edge_of_the_smallest_window() {
    let fake = FakeBackend::seeded();
    fake.update(|s| {
        let p = &mut s.projects[0];
        p.name = "garden-planner-for-the-community-allotment-beds".into();
        p.commands[1].line =
            "cargo run --release -- --config ./config/production.toml --verbose".into();
        s.activity.push(ActivityLine {
            at_ms: s.activity[0].at_ms,
            text: "Windows PC stopped answering: Could not reach 192.168.12.42:47820 within \
                   10 seconds. Check that the other computer is on and running Project \
                   Transfer."
                .into(),
            kind: ActivityKind::Warn,
        })
    });
    let mut h = build(fake, Box::new(NoPicker), SMALLEST, 1.0, false);
    h.state_mut().view.activity_open = true;
    h.run_steps(6);

    // At one pixel per point the bounds are in points; some containers
    // have none.
    let past: Vec<String> = h
        .root()
        .children_recursive()
        .filter_map(|n| {
            let a = n.accesskit_node();
            let end = a.bounding_box()?.x1;
            (end > f64::from(SMALLEST[0]) + 0.5)
                .then(|| format!("{:?} {:?} ends at {end:.0}", a.role(), a.label()))
        })
        .collect();
    assert!(past.is_empty(), "{past:#?}");
    // Anything too wide makes egui widen what follows it; the buttons that
    // fill the card show it first.
    let right = |label: &str| h.get_by_label(label).rect().right();
    let edge = right("Description");
    for button in ["Push to Desktop Swift Heron", "Add command"] {
        assert!(
            right(button) <= edge + 0.5,
            "{button} ends at {}",
            right(button)
        );
    }
}
