//! The dialog over a preview that adds a pattern to the ignore list, after
//! showing what it would leave out of this transfer.

use super::ignore_effect::{Effect, left_out};
use super::ignoring::{Suggestion, suggestions};
use super::preview::{Groups, file_count, group};
use super::preview_view::{self, ticks};
use super::theme::Palette;
use super::widgets::{self, plural};
use super::{App, dialogs, patterns};
use crate::core::{Action, UiState};
use crate::ignore_rules::{IgnoreSpec, Matcher, check, trim};
use crate::model::InstanceSettings;
use crate::transfer::Preview;
use egui::{Label, RichText, ScrollArea, TextEdit, Ui, WidgetInfo};

const WIDTH: f32 = 560.0;
const HINT: &str = "/exports/";

/// What a pattern would change in the preview, with its lines grouped for
/// drawing, or why it can't be used.
type Found = Result<(Effect, Groups), String>;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Draft {
    /// The path it was opened from, inside its folder.
    picked: Option<String>,
    /// Patterns for that path, narrowest first.
    pub options: Vec<Suggestion>,
    pub pattern: String,
    /// Puts the cursor in the pattern field when the dialog opens blank.
    focus: bool,
    /// The pattern last worked out and what it leaves out, kept until the
    /// pattern changes: narrowing a large preview takes a moment.
    found: Option<(String, Found)>,
}

impl Draft {
    /// Opened from a line: its patterns, the narrowest one chosen.
    pub fn for_path(rel: &str, is_dir: bool) -> Draft {
        let options = suggestions(rel, is_dir);
        let pattern = options[0].pattern.clone();
        Draft {
            picked: Some(rel.to_string()),
            options,
            pattern,
            ..Default::default()
        }
    }

    /// Opened from "Ignore files…": nothing chosen yet.
    pub fn blank() -> Draft {
        Draft {
            focus: true,
            ..Default::default()
        }
    }

    /// What the pattern leaves out of `p` alongside this computer's list;
    /// None while the field is empty.
    fn found(&mut self, me: &InstanceSettings, p: &Preview, peer: &str) -> Option<&Found> {
        let pattern = trim(&self.pattern);
        if pattern.is_empty() {
            return None;
        }
        if self.found.as_ref().is_none_or(|(was, _)| was != pattern) {
            let found = narrow(me, p, pattern).map(|e| {
                let lines = group(&e.preview, peer);
                (e, lines)
            });
            self.found = Some((pattern.to_string(), found));
        }
        self.found.as_ref().map(|(_, found)| found)
    }
}

/// What this computer's list, with `pattern` added at its end, would
/// change in `p`.
fn narrow(me: &InstanceSettings, p: &Preview, pattern: &str) -> Result<Effect, String> {
    check(pattern)?;
    let mut spec = IgnoreSpec::from_settings(me);
    spec.patterns.push(pattern.to_string());
    let m = Matcher::new(&spec).map_err(|e| format!("{e:#}"))?;
    Ok(left_out(p, &m))
}

impl App {
    /// The dialog, when one was opened over the preview `p`.
    pub(super) fn ignore_sheet(&mut self, ui: &mut Ui, s: &UiState, p: &Preview, peer: &str) {
        let Some(mut d) = self.view.ignore.take() else {
            return;
        };
        let r = dialogs::sheet(ui, "ignore", WIDTH, true, |ui| {
            dialogs::sheet_title(ui, "Ignore files");
            // However many choices and matches there are, the buttons stay in
            // the window and the rest scrolls.
            let room = (ui.ctx().content_rect().height() - 240.0).max(160.0);
            let (ok, enter) = ScrollArea::vertical()
                .max_height(room)
                .auto_shrink([false, true])
                .show(ui, |ui| body(ui, s, p, peer, &mut d))
                .inner;
            ui.add_space(6.0);
            widgets::small_muted(ui, "The ignore list in Settings applies to every project.");
            let answer = dialogs::button_row(ui, "Add to ignore list", false, ok);
            if enter && ok { Some(true) } else { answer }
        });
        match (r.dismissed, r.inner) {
            (true, _) | (_, Some(false)) => {}
            (_, Some(true)) => self.act(Action::AddIgnore(trim(&d.pattern).to_string())),
            _ => self.view.ignore = Some(d),
        }
    }
}

/// The choices, the pattern and what it leaves out. Returns whether the
/// pattern can be added, and whether Enter was pressed in its field.
fn body(ui: &mut Ui, s: &UiState, p: &Preview, peer: &str, d: &mut Draft) -> (bool, bool) {
    let pal = Palette::of(ui.ctx());
    if d.options.is_empty() {
        widgets::muted(
            ui,
            "Type a pattern to see which files of this transfer it leaves out.",
        );
        ui.add_space(4.0);
        patterns::help(ui);
    } else {
        if let Some(path) = &d.picked {
            ui.label(widgets::mono(path).color(pal.muted()));
            ui.add_space(4.0);
        }
        for o in &d.options {
            if ui
                .radio(d.pattern == o.pattern, ticks(&o.label, pal.ink))
                .clicked()
            {
                d.pattern = o.pattern.clone();
            }
        }
    }
    ui.add_space(8.0);
    widgets::muted(ui, "Pattern");
    let r = ui.add(
        TextEdit::singleline(&mut d.pattern)
            .font(egui::TextStyle::Monospace)
            .hint_text(widgets::hint(HINT).monospace())
            .desired_width(f32::INFINITY),
    );
    let typed = d.pattern.clone();
    r.widget_info(|| {
        let mut info = WidgetInfo::text_edit(true, &typed, &typed, HINT);
        info.label = Some("Pattern".into());
        info
    });
    if std::mem::take(&mut d.focus) {
        r.request_focus();
    }
    let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
    ui.add_space(8.0);
    let ok = match d.found(&s.me, p, peer) {
        None => false,
        Some(Err(why)) => {
            widgets::error_text(ui, &format!("This pattern can't be used: {why}."));
            false
        }
        Some(Ok((effect, lines))) => {
            for r in &effect.still_replaced {
                let what = if r.was_folder { "folder" } else { "file" };
                let by = r.by.strip_prefix("a ").unwrap_or(&r.by);
                let warning = format!(
                    "`{}` would still be replaced: this pattern leaves out the {what} but not \
                     the {by} that takes its place. Choose one that matches both.",
                    r.rel
                );
                ui.add(Label::new(ticks(&warning, pal.removed)).wrap());
            }
            match file_count(&effect.preview) {
                // The warning above says why; the hint below would mislead.
                0 if !effect.still_replaced.is_empty() => {}
                0 => {
                    let none = "Nothing in this transfer matches. Paths start inside each \
                                project folder, and capital letters count.";
                    ui.add(Label::new(RichText::new(none).color(pal.muted())).wrap());
                }
                n => {
                    let files = plural(n, "file", "files");
                    let says = format!("Leaves out {files} of this transfer:");
                    ui.label(RichText::new(says).color(pal.ink));
                    preview_view::lists(ui, &effect.preview, lines, true);
                }
            }
            true
        }
    };
    (ok, enter)
}
