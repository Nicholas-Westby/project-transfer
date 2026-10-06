//! Where things sit in the main window, down to its smallest size.

mod ui_support;

use egui_kittest::kittest::{NodeT, Queryable};
use project_transfer::core::{ActivityKind, ActivityLine, TransferState};
use ui_support::{FakeBackend, NoPicker, SIZE, build};

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
        p.folders[0].name = "garden-planner-web-application-frontend-next".into();
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

    // Anything too wide makes egui widen everything after it, so inside the
    // card nothing may end past the description box, which comes first.
    let card = h.get_by_label("Description").rect();
    let top = h.get_by_label("Delete project").rect().top();
    // At one pixel per point the bounds are in points; some containers
    // have none, and scroll bars sit beside what they scroll.
    let past: Vec<String> = h
        .root()
        .children_recursive()
        .filter_map(|n| {
            let a = n.accesskit_node();
            let b = a.bounding_box()?;
            let in_card = b.x0 >= f64::from(card.left()) - 0.5 && b.y0 >= f64::from(top) - 0.5;
            let edge = if in_card { card.right() } else { SMALLEST[0] };
            (b.x1 > f64::from(edge) + 0.5 && a.role() != egui::accesskit::Role::ScrollBar)
                .then(|| format!("{:?} {:?} ends at {:.0}", a.role(), a.label(), b.x1))
        })
        .collect();
    assert!(past.is_empty(), "{past:#?}");
}

#[test]
fn a_folder_not_set_up_here_keeps_its_instruction_on_one_line_when_there_is_room() {
    let fake = FakeBackend::seeded();
    fake.update(|s| {
        for f in &mut s.projects[0].folders {
            f.local_path = None;
        }
    });
    let h = build(fake, Box::new(NoPicker), SIZE, 1.0, false);
    let lines = h
        .get_all_by_label("Choose where this folder lives on this computer.")
        .map(|n| n.rect().height())
        .collect::<Vec<_>>();
    assert_eq!(lines.len(), 3);
    for height in lines {
        assert!(height < 30.0, "wrapped to {height} points");
    }
}

/// The sheet is pinned at the top, so a taller path would move the button.
#[test]
fn a_long_path_keeps_the_transfer_sheet_the_same_height() {
    let name = "tomato_raised_bed_seed_photos_plant-data.jpg";
    let long = format!(
        "{}{name}",
        "src/garden-planner/assets/images/seed-catalog/".repeat(8)
    );
    let running = |current: &str| TransferState::Running {
        done: 5_400_000_000,
        total: 10_000_000_000,
        files: 25_731,
        current: current.into(),
    };
    // Each display scale rounds a row of text to whole pixels in its own way.
    for ppp in [1.0, 1.5, 2.25] {
        let fake = FakeBackend::seeded();
        fake.update(|s| s.transfer = running("src/a.txt"));
        let mut h = build(fake.clone(), Box::new(NoPicker), SIZE, ppp, false);
        h.run();
        let short = h.get_by_label("Cancel transfer").rect();
        fake.update(|s| s.transfer = running(&long));
        h.run();
        let cancel = h.get_by_label("Cancel transfer").rect();
        assert_eq!(cancel, short, "at {ppp} pixels per point");
        // A label keeps its text in the node's value, not its label.
        let shown = h
            .get_by_label_contains(name)
            .accesskit_node()
            .value()
            .unwrap_or_default();
        assert!(shown.contains('…') && shown.ends_with(name), "{shown}");
    }
}
