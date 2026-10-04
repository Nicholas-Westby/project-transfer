//! Small building blocks shared by the screens, so buttons and text look the
//! same everywhere.

use super::theme::{CONTROL_RADIUS, Palette, semibold};
use egui::{
    Button, Color32, CornerRadius, FontId, Response, RichText, Sense, Ui, Vec2, WidgetInfo,
    WidgetType,
};
use std::path::Path;

/// A filled button in `fill` for the main action of a view or dialog.
pub fn filled(ui: &mut Ui, enabled: bool, text: &str, fill: Color32, tall: bool) -> Response {
    let pal = Palette::of(ui.ctx());
    let mut b = Button::new(RichText::new(text).color(pal.on_fill()).strong())
        .fill(fill)
        .stroke(egui::Stroke::NONE)
        .corner_radius(CornerRadius::same(CONTROL_RADIUS));
    if tall {
        b = b.min_size(Vec2::new(0.0, 36.0));
    }
    ui.add_enabled(enabled, b)
}

pub fn primary(ui: &mut Ui, text: &str) -> Response {
    let route = Palette::of(ui.ctx()).route;
    filled(ui, true, text, route, false)
}

pub fn danger(ui: &mut Ui, text: &str) -> Response {
    let removed = Palette::of(ui.ctx()).removed;
    filled(ui, true, text, removed, false)
}

/// A frameless text button for secondary row actions.
pub fn quiet(ui: &mut Ui, text: &str, color: Option<Color32>) -> Response {
    let pal = Palette::of(ui.ctx());
    let text = RichText::new(text).color(color.unwrap_or(pal.muted()));
    ui.add(Button::new(text).frame(false))
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// An icon-only button that still reads as `label` to screen readers.
pub fn icon_button(ui: &mut Ui, icon: &str, label: &str) -> Response {
    let pal = Palette::of(ui.ctx());
    let r = ui
        .add(Button::new(RichText::new(icon).size(17.0).color(pal.ink)).frame(false))
        .on_hover_text(label);
    let label = label.to_string();
    r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &label));
    r
}

/// A quiet "more" button for a row's menu, drawn as three dots.
pub fn more_button(ui: &mut Ui, label: &str) -> Response {
    let pal = Palette::of(ui.ctx());
    let (rect, r) = ui.allocate_exact_size(Vec2::new(28.0, 22.0), Sense::click());
    if r.hovered() || r.has_focus() {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(CONTROL_RADIUS), pal.border());
    }
    let color = if r.hovered() { pal.ink } else { pal.muted() };
    for dx in [-6.0, 0.0, 6.0] {
        ui.painter()
            .circle_filled(rect.center() + Vec2::new(dx, 0.0), 1.6, color);
    }
    let label = label.to_string();
    r.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, &label));
    r.on_hover_text("More")
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

pub fn muted(ui: &mut Ui, text: impl Into<String>) -> Response {
    let c = Palette::of(ui.ctx()).muted();
    ui.label(RichText::new(text.into()).color(c))
}

pub fn small_muted(ui: &mut Ui, text: impl Into<String>) -> Response {
    let c = Palette::of(ui.ctx()).muted();
    ui.label(RichText::new(text.into()).small().color(c))
}

pub fn colored(ui: &mut Ui, text: impl Into<String>, color: Color32) -> Response {
    ui.label(RichText::new(text.into()).color(color))
}

/// An inline error under the field it is about, wrapped to the column.
pub fn error_text(ui: &mut Ui, text: &str) -> Response {
    let c = Palette::of(ui.ctx()).removed;
    ui.add(egui::Label::new(RichText::new(text).color(c)).wrap())
}

/// Placeholder text for an empty field: "e.g. …". egui draws it in the
/// faint `weak_text_color` the theme sets.
pub fn hint(text: &str) -> RichText {
    RichText::new(format!("e.g. {text}"))
}

pub fn mono(text: impl Into<String>) -> RichText {
    RichText::new(text.into()).monospace()
}

/// A section heading inside a view: body size, semibold.
pub fn section(ui: &mut Ui, text: &str) -> Response {
    ui.label(RichText::new(text).font(FontId::new(15.0, semibold())))
}

pub fn heading(ui: &mut Ui, text: impl Into<RichText>) -> Response {
    ui.heading(text)
}

/// The online dot, drawn rather than typed so it looks the same in every font.
pub fn dot(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, color);
}

/// Two opposed arrows between this computer and the peer. Drawn, because
/// the bundled fonts have no ⇄.
pub fn swap_glyph(ui: &mut Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(22.0, 16.0), Sense::hover());
    let p = ui.painter();
    let stroke = egui::Stroke::new(1.6, color);
    let (l, r) = (rect.left() + 2.0, rect.right() - 2.0);
    let (top, bottom) = (rect.center().y - 3.5, rect.center().y + 3.5);
    p.line_segment([egui::pos2(l, top), egui::pos2(r, top)], stroke);
    p.line_segment([egui::pos2(r - 4.0, top - 3.5), egui::pos2(r, top)], stroke);
    p.line_segment([egui::pos2(r - 4.0, top + 3.5), egui::pos2(r, top)], stroke);
    p.line_segment([egui::pos2(l, bottom), egui::pos2(r, bottom)], stroke);
    p.line_segment(
        [egui::pos2(l + 4.0, bottom - 3.5), egui::pos2(l, bottom)],
        stroke,
    );
    p.line_segment(
        [egui::pos2(l + 4.0, bottom + 3.5), egui::pos2(l, bottom)],
        stroke,
    );
}

/// A small filled triangle pointing down (open) or right (closed),
/// centred on `at`.
pub fn chevron(ui: &Ui, at: egui::Pos2, open: bool, color: Color32) {
    let s = 3.5;
    let pts = if open {
        vec![
            at + Vec2::new(-s, -s * 0.6),
            at + Vec2::new(s, -s * 0.6),
            at + Vec2::new(0.0, s * 0.8),
        ]
    } else {
        vec![
            at + Vec2::new(-s * 0.6, -s),
            at + Vec2::new(s * 0.8, 0.0),
            at + Vec2::new(-s * 0.6, s),
        ]
    };
    ui.painter()
        .add(egui::Shape::convex_polygon(pts, color, egui::Stroke::NONE));
}

/// A small rounded tag such as "Primary".
pub fn badge(ui: &mut Ui, text: &str) -> Response {
    let pal = Palette::of(ui.ctx());
    egui::Frame::new()
        .fill(pal.route_tint())
        .corner_radius(CornerRadius::same(CONTROL_RADIUS))
        .inner_margin(egui::Margin::symmetric(6, 1))
        .show(ui, |ui| {
            ui.label(RichText::new(text).small().color(pal.route))
        })
        .inner
}

pub fn plural(n: u64, one: &str, many: &str) -> String {
    if n == 1 {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

/// "just now", "3 min ago", "2 hours ago", "yesterday", "4 days ago".
pub fn ago(now_ms: i64, then_ms: i64) -> String {
    let secs = (now_ms - then_ms).max(0) / 1000;
    match secs {
        0..60 => "just now".into(),
        60..3600 => format!("{} min ago", secs / 60),
        3600..86_400 => match secs / 3600 {
            1 => "1 hour ago".into(),
            h => format!("{h} hours ago"),
        },
        86_400..172_800 => "yesterday".into(),
        _ => format!("{} days ago", secs / 86_400),
    }
}

/// Paths under the home folder start with `~`, as people read them.
pub fn display_path(p: &Path) -> String {
    if let Some(home) = directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf())
        && let Ok(rest) = p.strip_prefix(&home)
        && !home.as_os_str().is_empty()
    {
        if rest.as_os_str().is_empty() {
            return "~".into();
        }
        let sep = std::path::MAIN_SEPARATOR;
        return format!("~{sep}{}", rest.display());
    }
    p.display().to_string()
}

pub fn this_computer() -> &'static str {
    if cfg!(windows) { "this PC" } else { "this Mac" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ages_read_like_speech() {
        let now = 10_000_000_000;
        assert_eq!(ago(now, now - 5_000), "just now");
        assert_eq!(ago(now, now - 3 * 60_000), "3 min ago");
        assert_eq!(ago(now, now - 3_600_000), "1 hour ago");
        assert_eq!(ago(now, now - 5 * 3_600_000), "5 hours ago");
        assert_eq!(ago(now, now - 30 * 3_600_000), "yesterday");
        assert_eq!(ago(now, now - 4 * 86_400_000), "4 days ago");
    }

    #[test]
    fn plurals() {
        assert_eq!(plural(1, "file", "files"), "1 file");
        assert_eq!(plural(3, "file", "files"), "3 files");
    }
}
