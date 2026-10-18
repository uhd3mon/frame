//! Frame's look: a dark theme with a blue accent (matches the app icon),
//! plus a few widgets the settings window is built from. Colours are
//! listed in design/gui.md; keep the two in step.

use eframe::egui::{
    self, Color32, CornerRadius, FontFamily, FontId, Margin, Response, RichText, Sense, Shadow,
    Stroke, StrokeKind, TextStyle, Ui, Vec2, WidgetInfo, WidgetType,
};

// Surfaces, darkest to lightest.
pub const BG: Color32 = Color32::from_rgb(0x15, 0x17, 0x1c); // window background
pub const FIELD: Color32 = Color32::from_rgb(0x11, 0x13, 0x17); // text edits, sunken
pub const CARD: Color32 = Color32::from_rgb(0x1d, 0x20, 0x27); // cards, bottom bar
pub const CONTROL: Color32 = Color32::from_rgb(0x27, 0x2b, 0x34); // buttons, combos
pub const CONTROL_HOVER: Color32 = Color32::from_rgb(0x30, 0x35, 0x40);
pub const CONTROL_ACTIVE: Color32 = Color32::from_rgb(0x38, 0x3e, 0x4a);
pub const BORDER: Color32 = Color32::from_rgb(0x2b, 0x2f, 0x38);
pub const BORDER_STRONG: Color32 = Color32::from_rgb(0x3a, 0x40, 0x4c);

// Text.
pub const TEXT: Color32 = Color32::from_rgb(0xe6, 0xe9, 0xef);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x9a, 0xa2, 0xb1);
pub const TEXT_FAINT: Color32 = Color32::from_rgb(0x6b, 0x73, 0x80);

// Accent (the icon's blue) and status colours.
pub const ACCENT: Color32 = Color32::from_rgb(0x3d, 0x8b, 0xff);
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(0x5a, 0x9c, 0xff);
pub const ACCENT_PRESSED: Color32 = Color32::from_rgb(0x2f, 0x74, 0xe0);
pub const ACCENT_TEXT: Color32 = Color32::WHITE;
pub const RED: Color32 = Color32::from_rgb(0xf0, 0x41, 0x4b);
pub const GREEN: Color32 = Color32::from_rgb(0x34, 0xc7, 0x7b);
pub const AMBER: Color32 = Color32::from_rgb(0xf5, 0xb5, 0x44);

pub const RADIUS: u8 = 8; // controls
pub const CARD_RADIUS: u8 = 12;
/// Width of the label column inside cards, so every control lines up.
pub const LABEL_WIDTH: f32 = 132.0;

/// Install the theme. Call once, before the first frame.
pub fn apply(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    ctx.style_mut_of(egui::Theme::Dark, style);
}

fn style(s: &mut egui::Style) {
    use FontFamily::{Monospace, Proportional};
    s.text_styles = [
        (TextStyle::Heading, FontId::new(20.0, Proportional)),
        (TextStyle::Body, FontId::new(14.0, Proportional)),
        (TextStyle::Button, FontId::new(14.0, Proportional)),
        (TextStyle::Monospace, FontId::new(13.0, Monospace)),
        (TextStyle::Small, FontId::new(12.0, Proportional)),
    ]
    .into();

    let sp = &mut s.spacing;
    sp.item_spacing = Vec2::new(8.0, 8.0);
    sp.button_padding = Vec2::new(12.0, 6.0);
    sp.interact_size = Vec2::new(40.0, 30.0);
    sp.window_margin = Margin::same(16);
    sp.menu_margin = Margin::same(6);
    sp.combo_width = 180.0;
    sp.text_edit_width = 240.0;
    sp.icon_width = 16.0;
    sp.icon_width_inner = 10.0;
    sp.icon_spacing = 8.0;
    sp.scroll.bar_width = 8.0;
    sp.scroll.floating = true;

    s.interaction.selectable_labels = false;

    let v = &mut s.visuals;
    v.dark_mode = true;
    v.override_text_color = None;
    v.panel_fill = BG;
    v.window_fill = CARD;
    v.window_stroke = line(1.0, BORDER);
    v.window_corner_radius = CornerRadius::same(CARD_RADIUS);
    v.menu_corner_radius = CornerRadius::same(RADIUS);
    v.window_shadow = Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: Color32::from_black_alpha(110),
    };
    v.popup_shadow = Shadow {
        offset: [0, 4],
        blur: 14,
        spread: 0,
        color: Color32::from_black_alpha(90),
    };
    v.extreme_bg_color = FIELD;
    v.text_edit_bg_color = Some(FIELD);
    v.faint_bg_color = CARD;
    v.code_bg_color = FIELD;
    v.hyperlink_color = ACCENT_HOVER;
    v.warn_fg_color = AMBER;
    v.error_fg_color = RED;
    v.selection.bg_fill = ACCENT.gamma_multiply(0.45);
    v.selection.stroke = line(1.5, ACCENT);
    v.text_cursor.stroke = line(2.0, ACCENT);
    v.striped = false;
    v.slider_trailing_fill = true;
    v.handle_shape = egui::style::HandleShape::Circle;
    v.indent_has_left_vline = false;

    let r = CornerRadius::same(RADIUS);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = CARD;
    w.noninteractive.weak_bg_fill = CARD;
    w.noninteractive.bg_stroke = line(1.0, BORDER); // separators, frames
    w.noninteractive.fg_stroke = line(1.0, TEXT);
    w.noninteractive.corner_radius = r;

    w.inactive.bg_fill = CONTROL;
    w.inactive.weak_bg_fill = CONTROL;
    w.inactive.bg_stroke = line(1.0, BORDER);
    w.inactive.fg_stroke = line(1.0, TEXT);
    w.inactive.corner_radius = r;
    w.inactive.expansion = 0.0;

    w.hovered.bg_fill = CONTROL_HOVER;
    w.hovered.weak_bg_fill = CONTROL_HOVER;
    w.hovered.bg_stroke = line(1.0, BORDER_STRONG);
    w.hovered.fg_stroke = line(1.0, TEXT);
    w.hovered.corner_radius = r;
    w.hovered.expansion = 0.0;

    w.active.bg_fill = CONTROL_ACTIVE;
    w.active.weak_bg_fill = CONTROL_ACTIVE;
    w.active.bg_stroke = line(1.0, ACCENT);
    w.active.fg_stroke = line(1.0, TEXT);
    w.active.corner_radius = r;
    w.active.expansion = 0.0;

    w.open.bg_fill = CONTROL_ACTIVE;
    w.open.weak_bg_fill = CONTROL_ACTIVE;
    w.open.bg_stroke = line(1.0, ACCENT);
    w.open.fg_stroke = line(1.0, TEXT);
    w.open.corner_radius = r;
}

/// A rounded section card with a small caps title. Contents get the full
/// card width; use `row` inside for label + control lines.
pub fn card<R>(ui: &mut Ui, title: &str, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(CARD)
        .stroke(line(1.0, BORDER))
        .corner_radius(CORNER_CARD)
        .inner_margin(Margin {
            left: 16,
            right: 16,
            top: 12,
            bottom: 14,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            section_header(ui, title);
            ui.add_space(2.0);
            add_contents(ui)
        })
        .inner
}

const CORNER_CARD: CornerRadius = CornerRadius::same(CARD_RADIUS);

/// Card / group title: small, bold-ish, muted, letter-spaced feel via caps.
pub fn section_header(ui: &mut Ui, title: &str) {
    ui.label(
        RichText::new(title.to_uppercase())
            .size(11.5)
            .strong()
            .color(TEXT_MUTED),
    );
}

/// One settings line: fixed-width label on the left, control(s) after it.
pub fn row<R>(ui: &mut Ui, label: &str, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    ltr(ui, |ui| {
        let h = ui.spacing().interact_size.y;
        ui.allocate_ui_with_layout(
            Vec2::new(LABEL_WIDTH, h),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_width(LABEL_WIDTH);
                ui.label(RichText::new(label).color(TEXT));
            },
        );
        add_contents(ui)
    })
    .inner
}

/// Small grey explanatory text.
pub fn hint(ui: &mut Ui, text: impl Into<String>) -> Response {
    ui.label(RichText::new(text).size(12.0).color(TEXT_FAINT))
}

/// Filled accent button for the main action (Save).
pub fn primary_button(ui: &mut Ui, text: &str) -> Response {
    let enabled = ui.is_enabled();
    let fill = if enabled { ACCENT } else { CONTROL };
    let fg = if enabled { ACCENT_TEXT } else { TEXT_FAINT };
    ui.scope(|ui| {
        let w = &mut ui.visuals_mut().widgets;
        w.hovered.weak_bg_fill = ACCENT_HOVER;
        w.hovered.bg_stroke = line(1.0, ACCENT_HOVER);
        w.active.weak_bg_fill = ACCENT_PRESSED;
        w.active.bg_stroke = line(1.0, ACCENT_PRESSED);
        ui.add(
            egui::Button::new(RichText::new(text).color(fg).strong())
                .fill(fill)
                .stroke(Stroke::NONE)
                .corner_radius(CornerRadius::same(RADIUS))
                .min_size(Vec2::new(88.0, 32.0)),
        )
    })
    .inner
}

/// Plain secondary button with the same height as `primary_button`.
pub fn secondary_button(ui: &mut Ui, text: &str) -> Response {
    ui.add(egui::Button::new(text).min_size(Vec2::new(72.0, 32.0)))
}

/// iOS/GNOME-style on/off switch. Returns a response that is `changed()`
/// when clicked.
pub fn toggle(ui: &mut Ui, on: &mut bool) -> Response {
    let size = Vec2::new(36.0, 20.0);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let enabled = ui.is_enabled();
    resp.widget_info(|| WidgetInfo::selected(WidgetType::Checkbox, enabled, *on, ""));
    if ui.is_rect_visible(rect) {
        let t = ui.ctx().animate_bool_responsive(resp.id, *on);
        let track = lerp_color(CONTROL_ACTIVE, ACCENT, t);
        let track = if resp.hovered() && !*on {
            CONTROL_HOVER.lerp_to_gamma(BORDER_STRONG, 0.5)
        } else {
            track
        };
        let radius = rect.height() / 2.0;
        let p = ui.painter();
        p.rect_filled(rect, radius, track);
        if resp.has_focus() {
            p.rect_stroke(
                rect.expand(2.0),
                radius + 2.0,
                line(1.5, ACCENT_HOVER),
                StrokeKind::Outside,
            );
        }
        let knob_r = radius - 3.0;
        let x = egui::lerp((rect.left() + radius)..=(rect.right() - radius), t);
        p.circle_filled(egui::pos2(x, rect.center().y), knob_r, Color32::WHITE);
    }
    resp
}

/// `toggle` followed by a clickable label, like a checkbox.
pub fn toggle_label(ui: &mut Ui, on: &mut bool, text: &str) -> Response {
    ltr(ui, |ui| {
        let mut r = toggle(ui, on);
        let l = ui.add(egui::Label::new(RichText::new(text).color(TEXT)).sense(Sense::click()));
        if l.clicked() {
            *on = !*on;
            r.mark_changed();
        }
        r | l
    })
    .inner
}

/// Coloured dot + text, for the header status ("Replay on · 60 s").
pub fn status_pill(ui: &mut Ui, color: Color32, text: &str) -> Response {
    // Painted by hand so it sizes to its text in any parent layout.
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), FontId::proportional(12.5), TEXT);
    let (pad_l, dot, gap, pad_r) = (10.0, 8.0, 6.0, 12.0);
    let size = Vec2::new(
        pad_l + dot + gap + galley.size().x + pad_r,
        galley.size().y + 10.0,
    );
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        p.rect_filled(rect, rect.height() / 2.0, color.gamma_multiply(0.14));
        let c = egui::pos2(rect.left() + pad_l + dot / 2.0, rect.center().y);
        p.circle_filled(c, dot / 2.0, color);
        let pos = egui::pos2(
            rect.left() + pad_l + dot + gap,
            rect.center().y - galley.size().y / 2.0,
        );
        p.galley(pos, galley, TEXT);
    }
    resp
}

/// A shortcut shown as keycaps ("Meta+Alt+R" → [Meta] [Alt] [R]); an empty
/// string shows a faint "Not set". Fixed width so Record/Clear line up.
pub fn keycaps(ui: &mut Ui, combo: &str, width: f32) -> Response {
    let h = ui.spacing().interact_size.y;
    ui.allocate_ui_with_layout(
        Vec2::new(width, h),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.set_min_width(width);
            ui.spacing_mut().item_spacing.x = 4.0;
            if combo.trim().is_empty() {
                ui.label(RichText::new("Not set").italics().color(TEXT_FAINT));
                return;
            }
            for key in combo.split('+').filter(|k| !k.is_empty()) {
                egui::Frame::new()
                    .fill(CONTROL)
                    .stroke(line(1.0, BORDER_STRONG))
                    .corner_radius(CornerRadius::same(5))
                    .inner_margin(Margin::symmetric(7, 2))
                    .show(ui, |ui| {
                        ui.label(RichText::new(key).size(12.5).color(TEXT));
                    });
            }
        },
    )
    .response
}

/// Frame for the sticky bottom bar (Unsaved changes · Revert · Save).
pub fn bottom_bar_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(CARD)
        .stroke(line(1.0, BORDER))
        .inner_margin(Margin {
            left: 16,
            right: 16,
            top: 10,
            bottom: 10,
        })
}

/// Frame for the central scroll area (page margins around the cards).
pub fn page_frame() -> egui::Frame {
    egui::Frame::new().fill(BG).inner_margin(Margin {
        left: 16,
        right: 16,
        top: 12,
        bottom: 12,
    })
}

/// Like `ui.horizontal`, but always left to right (also inside a
/// right-to-left parent, e.g. a right-aligned header or bottom bar).
fn ltr<R>(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<R> {
    let size = Vec2::new(ui.available_width(), ui.spacing().interact_size.y);
    ui.allocate_ui_with_layout(
        size,
        egui::Layout::left_to_right(egui::Align::Center),
        add_contents,
    )
}

fn line(width: f32, color: Color32) -> Stroke {
    Stroke { width, color }
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    a.lerp_to_gamma(b, t.clamp(0.0, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_sets_dark_accent_theme() {
        let ctx = egui::Context::default();
        apply(&ctx);
        let s = ctx.style_of(egui::Theme::Dark);
        assert!(s.visuals.dark_mode);
        assert_eq!(s.visuals.panel_fill, BG);
        assert_eq!(s.visuals.selection.stroke.color, ACCENT);
        assert_eq!(s.text_styles[&TextStyle::Body].size, 14.0);
    }

    #[test]
    fn toggle_flips_on_click_only() {
        let ctx = egui::Context::default();
        apply(&ctx);
        let mut on = false;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let r = toggle(ui, &mut on);
                assert!(!r.changed());
                card(ui, "Saving", |ui| row(ui, "Format", |ui| hint(ui, "png")));
                primary_button(ui, "Save");
                status_pill(ui, GREEN, "Replay on · 60 s");
            });
        });
        assert!(!on);
    }
}
