//! Where things sit in the main window, down to its smallest size.

mod ui_support;

use egui_kittest::kittest::Queryable;
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
