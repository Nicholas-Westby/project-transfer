//! Drawing the preview body: folders, warnings, counts and the lists.

use super::ignore_sheet::Draft;
use super::preview::{Groups, Line, group, skipped, warnings};
use super::theme::{CONTROL_RADIUS, Palette};
use super::widgets::{self, plural};
use crate::transfer::Preview;
use egui::text::LayoutJob;
use egui::{
    Align, Align2, CollapsingHeader, Color32, CursorIcon, FontId, Id, Layout, Rect, RichText,
    Sense, TextFormat, Ui, WidgetInfo, WidgetType, vec2,
};

/// Room kept at the end of every row that can be ignored, so its text
/// never reflows when the button shows.
const IGNORE_ROOM: f32 = 64.0;

/// Lines each list of matches shows: drawing more would slow every frame
/// while a pattern is typed.
const MATCHES_SHOWN: usize = 200;

/// Draws the body of a ready preview: folders, warnings, counts, lists.
/// Returns the dialog to open when something was picked to ignore.
pub fn show(ui: &mut Ui, p: &Preview, peer: &str) -> Option<Draft> {
    let pal = Palette::of(ui.ctx());
    folder_lines(ui, p);
    let warnings = warnings(p, peer);
    if !warnings.is_empty() {
        ui.add_space(8.0);
        for w in &warnings {
            ui.add(egui::Label::new(ticks(w, pal.changed)).wrap());
        }
    }

    let c = p.counts();
    let mut picked = None;
    ui.add_space(12.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 20.0;
        count(ui, c.added, "added", pal.added);
        count(ui, c.changed, "changed", pal.changed);
        count(ui, c.timestamp_only, "timestamp only", pal.muted());
        count(ui, c.removed_files, "removed", pal.removed);
        // Sending everything skips the list, so adding to it changes nothing.
        if !p.request.send_everything {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if widgets::quiet(ui, "Ignore files…", Some(pal.route)).clicked() {
                    picked = Some(Draft::blank());
                }
            });
        }
    });
    ui.add_space(8.0);

    egui::ScrollArea::vertical()
        .max_height(ui.ctx().content_rect().height() * 0.42)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            if let Some(d) = lists(ui, p, &group(p, peer), false) {
                picked = Some(d);
            }
        });
    picked
}

/// The preview's lists. Deletions and rewrites open by default: they are
/// what can lose work. With `matches` they list what a new pattern would
/// leave out instead: all open, and nothing to ignore or skip.
pub fn lists(ui: &mut Ui, p: &Preview, g: &Groups, matches: bool) -> Option<Draft> {
    let pal = Palette::of(ui.ctx());
    let c = p.counts();
    let shown = if matches { MATCHES_SHOWN } else { usize::MAX };
    let ignorable = !matches && !p.request.send_everything;
    let mut picked = None;
    for (title, files, items, color, open) in [
        ("Removed", c.removed_files, &g.removed, pal.removed, true),
        ("Changed", c.changed, &g.changed, pal.changed, true),
        ("Added", c.added, &g.added, pal.added, matches),
        (
            "Timestamp only",
            c.timestamp_only,
            &g.timestamp_only,
            pal.muted(),
            matches,
        ),
    ] {
        if items.is_empty() {
            continue;
        }
        list(ui, title, files, open, |ui| {
            if let Some(line) = lines(ui, p, items, shown, color, ignorable) {
                picked = Some(Draft::for_path(&line.rel, line.is_dir));
            }
        });
    }
    let skipped = skipped(p);
    if !matches && !skipped.is_empty() {
        list(ui, "Skipped", skipped.len() as u64, true, |ui| {
            for s in &skipped {
                ui.label(path_line(s, pal.changed, pal.ink));
            }
        });
    }
    picked
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
fn list(ui: &mut Ui, title: &str, files: u64, open: bool, add: impl FnOnce(&mut Ui)) {
    let pal = Palette::of(ui.ctx());
    let header = format!("{title} ({})", plural(files, "file", "files"));
    CollapsingHeader::new(RichText::new(header).color(pal.ink))
        .id_salt(title)
        .default_open(open)
        .show(ui, add);
}

/// A list's first `shown` lines, under their folder's name when the
/// project has more than one: the paths start inside the folder, as
/// patterns do. Returns the line whose Ignore button was clicked.
fn lines<'a>(
    ui: &mut Ui,
    p: &Preview,
    items: &'a [Line],
    shown: usize,
    color: Color32,
    ignorable: bool,
) -> Option<&'a Line> {
    let pal = Palette::of(ui.ctx());
    let multi = p.folders.len() > 1;
    let mut folder = None;
    let mut picked = None;
    for line in items.iter().take(shown) {
        if multi && folder != Some(line.folder) {
            folder = Some(line.folder);
            let name = &p.folders[line.folder].name;
            ui.label(RichText::new(name).small().strong().color(pal.muted()));
        }
        let id = (ignorable && line.ignorable).then(|| ui.id().with((line.folder, &line.rel)));
        if row(ui, path_line(&line.text, color, pal.ink), id) {
            picked = Some(line);
        }
    }
    if items.len() > shown {
        widgets::small_muted(ui, format!("And {} more", items.len() - shown));
    }
    picked
}

/// One line. Given an id, an "Ignore…" button shows at its end while the
/// pointer is on the row; true once it is clicked. Drawn by hand in room
/// kept for it, so showing it never moves the text or the rows below.
fn row(ui: &mut Ui, job: LayoutJob, ignorable: Option<Id>) -> bool {
    let Some(id) = ignorable else {
        ui.label(job);
        return false;
    };
    let width = ui.available_width();
    let backdrop = ui.painter().add(egui::Shape::Noop);
    let text = ui
        .allocate_ui(vec2((width - IGNORE_ROOM).max(0.0), 0.0), |ui| {
            ui.label(job)
        })
        .response
        .rect;
    let rect = Rect::from_min_size(text.min, vec2(width, text.height()));
    if !ui.rect_contains_pointer(rect) {
        return false;
    }
    let pal = Palette::of(ui.ctx());
    let fill = ui.visuals().widgets.inactive.weak_bg_fill;
    let shape = egui::epaint::RectShape::filled(rect.expand2(vec2(4.0, 1.0)), CONTROL_RADIUS, fill);
    ui.painter().set(backdrop, shape);
    let room = Rect::from_min_max(egui::pos2(rect.right() - IGNORE_ROOM, rect.top()), rect.max);
    let r = ui
        .interact(room, id, Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand);
    let color = if r.hovered() { pal.ink } else { pal.route };
    let font = FontId::proportional(13.0);
    ui.painter().text(
        room.right_center(),
        Align2::RIGHT_CENTER,
        "Ignore…",
        font,
        color,
    );
    r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "Ignore…"));
    r.clicked()
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

/// Which folders a preview compares, each as name, source and destination,
/// so even "nothing to do" says which copies matched.
pub fn folder_lines(ui: &mut Ui, p: &Preview) {
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
    for l in &p.left_out {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(&l.name).strong().color(pal.ink));
            ui.label(RichText::new("Left out.").color(pal.changed));
            ui.add(egui::Label::new(ticks(&l.reason, pal.muted())).wrap());
        });
    }
}
