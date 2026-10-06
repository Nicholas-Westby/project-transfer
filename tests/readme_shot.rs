//! Renders the README screenshot from the seeded fake state, never a real
//! install. `cargo xtask screenshot` runs it and turns the PNG into webp.

mod ui_support;

use project_transfer::core::{OutputLine, RunStatus};
use ui_support::{FakeBackend, NoPicker, SIZE, build};

#[test]
#[ignore = "run by `cargo xtask screenshot`"]
fn readme_screenshot() {
    let path = std::env::var("README_SHOT_PNG").expect("set README_SHOT_PNG to the output path");
    let fake = FakeBackend::seeded();
    fake.update(|s| {
        // A finished run has no spinner, so reruns draw the same pixels.
        for run in s.command_runs.values_mut() {
            run.status = RunStatus::Exited(Some(0));
            run.lines.retain(|l| !l.stderr);
            run.lines.push(OutputLine {
                stderr: false,
                text: "Done.".into(),
            });
        }
    });
    let mut h = build(fake, Box::new(NoPicker), SIZE, 1.0, true);
    h.run_ok();
    let img = h.render().expect("render the window");
    img.save(&path).expect("save the png");
}
