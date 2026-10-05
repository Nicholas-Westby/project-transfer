//! The project's description under its title: a multi-line box that saves
//! when it loses focus. Esc leaves it as it was.

use super::App;
use super::theme::Palette;
use super::widgets;
use crate::core::Action;
use crate::model::{MAX_DESCRIPTION_CHARS, Project, ProjectId};
use egui::{Key, TextEdit, Ui};

const HINT: &str = "Add a description";

impl App {
    pub(super) fn description(&mut self, ui: &mut Ui, p: &Project) {
        // A draft left by another project lost its box when the selection
        // moved, so it never saw the focus go: save it now.
        if let Some(d) = self.view.description.take_if(|d| d.project != p.id) {
            self.save_description(d);
        }
        let pal = Palette::of(ui.ctx());
        let editing = self.view.description.is_some();
        let mut text = match &self.view.description {
            Some(d) => d.text.clone(),
            None => p.description.text.clone(),
        };
        let r = ui.add(
            TextEdit::multiline(&mut text)
                .id_salt(("description", p.id))
                .hint_text(HINT)
                .desired_width(ui.available_width())
                .desired_rows(2)
                .margin(egui::Margin::symmetric(8, 6))
                .char_limit(MAX_DESCRIPTION_CHARS)
                .text_color(if editing { pal.ink } else { pal.muted() }),
        );
        r.widget_info(|| {
            let mut info = egui::WidgetInfo::text_edit(true, &text, &text, HINT);
            info.label = Some("Description".into());
            info
        });
        if r.has_focus() || r.gained_focus() {
            let base = match self.view.description.take() {
                Some(d) => d.base,
                None => p.description.text.clone(),
            };
            self.view.description = Some(Draft {
                project: p.id,
                base,
                text,
            });
        }
        if r.lost_focus() {
            let draft = self.view.description.take();
            // Esc takes the focus away too; then the edit is dropped.
            if !ui.input(|i| i.key_pressed(Key::Escape))
                && let Some(d) = draft
            {
                self.save_description(d);
            }
        }
        if r.has_focus() {
            widgets::small_muted(ui, "Saved when you click away. Esc undoes your changes.");
        }
    }

    /// Saves only what the user typed: a draft that still reads as it
    /// began is no edit, even if a newer description arrived meanwhile.
    fn save_description(&self, d: Draft) {
        if d.text.trim_end() != d.base.trim_end() {
            self.act(Action::SetDescription(d.project, d.text));
        }
    }
}

/// The description being typed, until its box loses focus.
#[derive(Clone, Debug, PartialEq)]
pub struct Draft {
    pub project: ProjectId,
    /// The description when typing began.
    pub base: String,
    pub text: String,
}
