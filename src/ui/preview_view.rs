//! Drawing the preview body: folders, warnings, counts and the lists.

use super::preview::{group, skipped, warnings};
use super::theme::Palette;
use super::widgets::{self, plural};
use crate::transfer::Preview;
use egui::text::LayoutJob;
use egui::{CollapsingHeader, Color32, FontId, RichText, TextFormat, Ui};

/// Draws the body of a ready preview: folders, warnings, counts, lists.
pub fn show(ui: &mut Ui, p: &Preview, peer: &str) {
    let pal = Palette::of(ui.ctx());
    for f in &p.folders {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(&f.name).strong().color(pal.ink));
            ui.label(widgets::mono(&f.source_path).color(pal.muted()));
            ui.label(RichText::new("→").color(pal.muted()));
            ui.label(widgets::mono(&f.dest_path).color(pal.muted()));
        });
        if f.dest_will_be_created {
            widgets::small_muted(ui, format!("{} will be created.", f.dest_path));
        }
    }
    let warnings = warnings(p, peer);
    if !warnings.is_empty() {
        ui.add_space(8.0);
        for w in &warnings {
            ui.add(egui::Label::new(ticks(w, pal.changed)).wrap());
        }
    }

    let c = p.counts();
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 20.0;
        count(ui, c.added, "added", pal.added);
        count(ui, c.changed, "changed", pal.changed);
        count(ui, c.timestamp_only, "timestamp only", pal.muted());
        count(ui, c.removed_files, "removed", pal.removed);
    });
    ui.add_space(8.0);

    let g = group(p, peer);
    let skipped = skipped(p);
    egui::ScrollArea::vertical()
        .max_height(ui.ctx().content_rect().height() * 0.42)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            // Deletions and rewrites open by default: they are what can lose work.
            list(
                ui,
                "Removed",
                c.removed_files,
                &g.removed,
                pal.removed,
                true,
            );
            list(ui, "Changed", c.changed, &g.changed, pal.changed, true);
            list(ui, "Added", c.added, &g.added, pal.added, false);
            let ts = &g.timestamp_only;
            list(
                ui,
                "Timestamp only",
                c.timestamp_only,
                ts,
                pal.muted(),
                false,
            );
            let n = skipped.len() as u64;
            list(ui, "Skipped", n, &skipped, pal.changed, true);
        });
}

fn count(ui: &mut Ui, n: u64, what: &str, color: Color32) {
    let color = if n == 0 {
        Palette::of(ui.ctx()).muted()
    } else {
        color
    };
    ui.label(RichText::new(format!("{n} {what}")).strong().color(color));
}

/// One collapsible list; its header counts files, as the counts row does.
fn list(ui: &mut Ui, title: &str, files: u64, items: &[String], color: Color32, open: bool) {
    if items.is_empty() {
        return;
    }
    let pal = Palette::of(ui.ctx());
    let header = format!("{title} ({})", plural(files, "file", "files"));
    CollapsingHeader::new(RichText::new(header).color(pal.ink))
        .id_salt(title)
        .default_open(open)
        .show(ui, |ui| {
            for item in items {
                ui.label(path_line(item, color, pal.ink));
            }
        });
}

/// A list line with its marker in the signal color and the text in mono.
fn path_line(text: &str, marker: Color32, ink: Color32) -> LayoutJob {
    let mut job = LayoutJob::default();
    let mono = FontId::monospace(13.0);
    let mut dot = TextFormat::simple(FontId::proportional(8.0), marker);
    dot.valign = egui::Align::Center;
    job.append("●  ", 0.0, dot);
    job.append(text, 0.0, TextFormat::simple(mono, ink));
    job
}

/// Body text with `backticked` names set in mono, as the core writes them.
pub fn ticks(text: &str, color: Color32) -> LayoutJob {
    let mut job = LayoutJob::default();
    for (i, part) in text.split('`').enumerate() {
        let font = if i % 2 == 1 {
            FontId::monospace(13.0)
        } else {
            FontId::proportional(14.0)
        };
        let mut f = TextFormat::simple(font, color);
        f.valign = egui::Align::Center;
        job.append(part, 0.0, f);
    }
    job
}
