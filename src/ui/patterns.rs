//! The ignore list's pattern fields, and the help that says how patterns
//! match.

use super::ignoring::without_folder_name;
use super::preview_view::ticks;
use super::theme::Palette;
use super::widgets;
use crate::ignore_rules::{check, trim};
use egui::{Grid, TextEdit, Ui, WidgetInfo};

/// One of each kind of pattern people need, the anchored kind last.
const EXAMPLES: &[(&str, &str)] = &[
    ("*.log", "Every file ending in .log"),
    ("exports/", "Every folder named exports, at any depth"),
    ("/notes.txt", "notes.txt at the top of each project folder"),
    (
        "/src/tmp/",
        "The folder src/tmp, from the top of each project folder",
    ),
];

/// How patterns match, and the rule people trip over: a path starts inside
/// the project folder, not with its name.
pub(super) fn help(ui: &mut Ui) {
    let ink = Palette::of(ui.ctx()).ink;
    Grid::new("pattern_help")
        .num_columns(2)
        .spacing([16.0, 2.0])
        .show(ui, |ui| {
            for (pattern, what) in EXAMPLES {
                ui.label(widgets::mono(*pattern).color(ink));
                widgets::small_muted(ui, *what);
                ui.end_row();
            }
        });
    widgets::small_muted(
        ui,
        "Paths start inside each project folder: leave out the folder's own name, and put / \
         between names, even on Windows.",
    );
}

/// A list of patterns with Remove buttons and a field to add one. A pattern
/// that can't be used says why before it can be added, and one that starts
/// with the name of one of `folders` offers to drop it.
pub(super) fn pattern_list(
    ui: &mut Ui,
    title: &str,
    list: &mut Vec<String>,
    draft: &mut String,
    field: (&str, &str),
    add_label: &str,
    folders: &[&str],
) {
    let pal = Palette::of(ui.ctx());
    let (field_label, hint) = field;
    widgets::muted(ui, title);
    let mut remove = None;
    let mut fix = None;
    for (i, pat) in list.iter().enumerate() {
        ui.horizontal(|ui| {
            ui.label(widgets::mono(pat).color(pal.ink));
            if widgets::quiet(ui, "Remove", Some(pal.removed)).clicked() {
                remove = Some(i);
            }
        });
        if let Some((name, fixed)) = without_folder_name(pat, folders) {
            ui.horizontal_wrapped(|ui| {
                let why = format!(
                    "Starts with the folder name `{name}`, but paths start inside each project \
                     folder."
                );
                ui.label(ticks(&why, pal.changed));
                let change = format!("Change to {fixed}");
                if widgets::quiet(ui, &change, Some(pal.route)).clicked() {
                    fix = Some((i, fixed));
                }
            });
        }
    }
    if let Some(i) = remove {
        list.remove(i);
    }
    if let Some((i, fixed)) = fix {
        if list.contains(&fixed) {
            list.remove(i);
        } else {
            list[i] = fixed;
        }
    }
    // Read after the field takes this frame's typing, so a pattern typed
    // and entered in one frame still counts.
    let problem = ui
        .horizontal(|ui| {
            let r = ui.add(
                TextEdit::singleline(draft)
                    .hint_text(widgets::hint(hint).monospace())
                    .font(egui::TextStyle::Monospace)
                    .desired_width(220.0),
            );
            let pattern = trim(draft).to_string();
            r.widget_info(|| {
                let mut info = WidgetInfo::text_edit(true, &pattern, &pattern, hint);
                info.label = Some(field_label.into());
                info
            });
            let problem = match pattern.is_empty() {
                true => None,
                false => check(&pattern).err(),
            };
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let ok = !pattern.is_empty() && problem.is_none() && !list.contains(&pattern);
            let clicked = ui.add_enabled(ok, egui::Button::new(add_label)).clicked();
            if ok && (clicked || enter) {
                list.push(pattern);
                draft.clear();
            }
            problem
        })
        .inner;
    if let Some(why) = problem {
        widgets::error_text(ui, &format!("This pattern can't be used: {why}."));
    }
}
