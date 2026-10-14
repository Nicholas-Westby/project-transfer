use super::*;
use crate::viewer::rows::align;
use crate::viewer::text::{Body, decode};
use egui::{Event, Modifiers, MouseWheelUnit, RawInput, Rect, TouchPhase, pos2};
use std::time::Duration;

fn text(lines: usize, word: &str) -> Text {
    let body: String = (0..lines).map(|i| format!("{word} {i}\n")).collect();
    match decode(body.as_bytes(), body.len() as u64) {
        Body::Text(t) => t,
        Body::Binary => unreachable!(),
    }
}

/// Runs one frame of `draw` in an 800 by 600 window with `events`.
fn frame(ctx: &egui::Context, events: Vec<Event>, draw: impl FnMut(&mut Ui)) {
    let input = RawInput {
        screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0))),
        events,
        ..Default::default()
    };
    let mut out = ctx.run_ui(input, draw);
    // Nothing is painted, so the font textures are never needed.
    out.textures_delta.clear();
}

/// The wheel turned with the pointer at `x`, halfway down.
fn wheel(x: f32) -> Vec<Event> {
    vec![
        Event::PointerMoved(pos2(x, 300.0)),
        Event::MouseWheel {
            unit: MouseWheelUnit::Point,
            delta: vec2(0.0, -300.0),
            phase: TouchPhase::Move,
            modifiers: Modifiers::NONE,
        },
    ]
}

#[test]
fn turning_the_wheel_over_either_pane_scrolls_both() {
    let (left, right) = (text(1_000, "here"), text(1_000, "there"));
    let rows = align(&left.lines(), &right.lines(), Duration::from_secs(1));
    let ctx = egui::Context::default();
    let mut offset = Vec2::ZERO;
    let draw = |events: Vec<Event>, offset: &mut Vec2| {
        frame(&ctx, events, |ui| {
            *offset = side_by_side(ui, &left, &right, &rows, *offset, 500.0);
        });
    };
    draw(vec![], &mut offset);
    draw(wheel(200.0), &mut offset);
    let after_left = offset.y;
    assert!(after_left > 0.0, "{offset:?}");
    draw(wheel(600.0), &mut offset);
    assert!(offset.y > after_left, "{offset:?}");
}

#[test]
fn one_copy_scrolls_on_its_own() {
    let only = text(1_000, "only");
    let ctx = egui::Context::default();
    let mut offset = Vec2::ZERO;
    for events in [vec![], wheel(300.0)] {
        frame(&ctx, events, |ui| {
            offset = single(ui, &only, offset, 500.0);
        });
    }
    assert!(offset.y > 0.0, "{offset:?}");
}

#[test]
fn a_very_long_line_is_cut_where_it_is_drawn() {
    let long = "x".repeat(LONGEST + 50);
    let shown = cut(Cow::Borrowed(&long));
    assert_eq!(shown.chars().count(), LONGEST + 1);
    assert!(shown.ends_with('…'));
    assert!(matches!(cut(Cow::Borrowed("short")), Cow::Borrowed(_)));
}

#[test]
fn a_long_line_with_tabs_is_cut_the_same_way() {
    let tabs = "\t".repeat(3_000);
    let shown = shown_line(&tabs);
    assert_eq!(shown.chars().count(), LONGEST + 1);
    assert!(shown.ends_with('…'));
    assert_eq!(shown_line("a\tb"), "a   b");
}
