//! Frame's look: a dark theme with a blue accent (matches the app icon),
//! plus a few widgets the settings window is built from. Colours are
//! listed in design/gui.md; keep the two in step.

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontFamily, FontId, Layout, Margin, Pos2, Rect,
    Response, RichText, Sense, Shadow, Stroke, StrokeKind, TextStyle, Ui, UiBuilder, Vec2,
    WidgetInfo, WidgetType,
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

/// Small caps sub-heading (inside the shortcut editor).
pub fn section_header(ui: &mut Ui, title: &str) {
    ui.label(
        RichText::new(title.to_uppercase())
            .size(11.5)
            .strong()
            .color(TEXT_MUTED),
    );
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

/// Coloured dot + text, for the header status ("Running").
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

/// Frame for the sticky bottom bar (Unsaved changes · Revert · Save). No
/// box stroke: call `top_hairline` inside it for the one line on top.
pub fn bottom_bar_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(CARD)
        .inner_margin(Margin {
            left: 16,
            right: 16,
            top: 10,
            bottom: 10,
        })
}

/// The 1 px BORDER line along the top edge of `bottom_bar_frame`.
pub fn top_hairline(ui: &mut Ui) {
    let r = ui.max_rect().expand2(Vec2::new(16.0, 10.0));
    ui.painter().hline(r.x_range(), r.top() + 0.5, line(1.0, BORDER));
}

/// Frame for the central scroll area (page margins around the cards).
pub fn page_frame() -> egui::Frame {
    egui::Frame::new().fill(BG).inner_margin(Margin {
        left: 16,
        right: 16,
        top: 8,
        bottom: 12,
    })
}

/// Height of a one-line list row and of a row with a subtitle.
pub const ROW_H: f32 = 44.0;
pub const ROW_H_TALL: f32 = 54.0;
/// Left/right padding inside list rows.
pub const ROW_PAD_X: f32 = 14.0;
/// Icon tile on shortcut rows.
const TILE: f32 = 28.0;

/// Which glyph a shortcut row shows.
#[derive(Clone, Copy)]
pub enum Glyph {
    Region,
    Window,
    Monitor,
    AllMonitors,
}

/// Group title above a list card ("Shortcuts").
pub fn group_title(ui: &mut Ui, text: &str) {
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        ui.label(RichText::new(text).size(13.0).strong().color(TEXT_MUTED));
    });
    ui.add_space(-2.0);
}

/// A card holding list rows edge to edge (no title inside, no padding;
/// rows bring their own). Rows are separated by hairlines, not gaps.
pub fn list_card<R>(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(CARD)
        .stroke(Stroke::new(1.0_f32, BORDER))
        .corner_radius(CornerRadius::same(CARD_RADIUS))
        .inner_margin(Margin::same(0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            add_contents(ui)
        })
        .inner
}

/// One list row: optional icon tile, title (+ faint subtitle) on the left,
/// `right` laid out right to left on the right. `divider` draws the
/// hairline above it (false for the first row). A `clickable` row
/// highlights on hover, shows a chevron and its response `clicked()`.
pub fn list_row<R>(
    ui: &mut Ui,
    divider: bool,
    glyph: Option<Glyph>,
    title: &str,
    subtitle: Option<&str>,
    clickable: bool,
    right: impl FnOnce(&mut Ui) -> R,
) -> (Response, R) {
    let h = if subtitle.is_some() { ROW_H_TALL } else { ROW_H };
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), Sense::hover());
    let sense = if clickable { Sense::click() } else { Sense::hover() };
    let resp = ui.interact(rect, ui.id().with(("list_row", title)), sense);
    let enabled = ui.is_enabled();
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, enabled, title));

    let p = ui.painter().clone();
    if clickable && (resp.hovered() || resp.has_focus()) {
        p.rect_filled(rect.shrink(4.0), CornerRadius::same(RADIUS), CONTROL.gamma_multiply(0.7));
    }
    if resp.has_focus() {
        p.rect_stroke(rect.shrink(4.0), CornerRadius::same(RADIUS), Stroke::new(1.5_f32, ACCENT_HOVER), StrokeKind::Inside);
    }
    let mut x = rect.left() + ROW_PAD_X;
    if divider {
        let dx = if glyph.is_some() { x + TILE + 12.0 } else { x };
        p.hline(dx..=rect.right(), rect.top(), Stroke::new(1.0_f32, BORDER));
    }
    if let Some(g) = glyph {
        let tile = Rect::from_center_size(Pos2::new(x + TILE / 2.0, rect.center().y), Vec2::splat(TILE));
        p.rect_filled(tile, CornerRadius::same(7), ACCENT.gamma_multiply(0.16));
        paint_glyph(&p, tile.shrink(6.0), g, ACCENT_HOVER);
        x += TILE + 12.0;
    }
    let mut right_rect = rect.shrink2(Vec2::new(ROW_PAD_X, 0.0));
    if clickable {
        let c = Pos2::new(right_rect.right() - 3.0, rect.center().y);
        let s = Stroke::new(1.6_f32, if resp.hovered() { TEXT_MUTED } else { TEXT_FAINT });
        p.line_segment([c + Vec2::new(-3.0, -5.0), c], s);
        p.line_segment([c + Vec2::new(-3.0, 5.0), c], s);
        right_rect.max.x -= 18.0;
    }
    let right = ui.scope_builder(
        UiBuilder::new().max_rect(right_rect).layout(Layout::right_to_left(Align::Center)),
        |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            right(ui)
        },
    );
    // Text gets what the right side left over, cut with "…" if longer.
    let used = right.response.rect;
    let text_w = if used.width() > 0.0 { used.left() - 12.0 - x } else { right_rect.right() - x };
    let line = |text: &str, size: f32, color: Color32| {
        let mut job = egui::text::LayoutJob::single_section(
            text.to_owned(),
            egui::TextFormat::simple(FontId::proportional(size), color),
        );
        job.wrap = egui::text::TextWrapping::truncate_at_width(text_w.max(24.0));
        p.layout_job(job)
    };
    match subtitle {
        None => {
            let g = line(title, 14.0, TEXT);
            p.galley(Pos2::new(x, rect.center().y - g.size().y / 2.0), g, TEXT);
        }
        Some(sub) => {
            let g = line(title, 14.0, TEXT);
            p.galley(Pos2::new(x, rect.center().y - 1.0 - g.size().y), g, TEXT);
            let g = line(sub, 12.0, TEXT_FAINT);
            p.galley(Pos2::new(x, rect.center().y + 2.0), g, TEXT_FAINT);
        }
    }
    (resp, right.inner)
}

/// 16 px line icons for the four capture actions, painted (no font glyphs).
pub fn paint_glyph(p: &egui::Painter, r: Rect, g: Glyph, color: Color32) {
    let s = Stroke::new(1.6_f32, color);
    let (l, t, rt, b) = (r.left(), r.top(), r.right(), r.bottom());
    match g {
        Glyph::Region => {
            let k = 4.5;
            for (c, dx, dy) in [
                (Pos2::new(l, t), 1.0, 1.0),
                (Pos2::new(rt, t), -1.0, 1.0),
                (Pos2::new(rt, b), -1.0, -1.0),
                (Pos2::new(l, b), 1.0, -1.0),
            ] {
                p.line_segment([c, c + Vec2::new(k * dx, 0.0)], s);
                p.line_segment([c, c + Vec2::new(0.0, k * dy)], s);
            }
            p.circle_filled(r.center(), 2.0, color);
        }
        Glyph::Window => {
            let w = Rect::from_min_max(Pos2::new(l, t + 1.5), Pos2::new(rt, b - 1.5));
            p.rect_stroke(w, CornerRadius::same(2), s, StrokeKind::Middle);
            p.hline(l..=rt, t + 5.0, s);
        }
        Glyph::Monitor => {
            let m = Rect::from_min_max(Pos2::new(l, t + 1.0), Pos2::new(rt, b - 4.0));
            p.rect_stroke(m, CornerRadius::same(2), s, StrokeKind::Middle);
            let cx = r.center().x;
            p.line_segment([Pos2::new(cx, b - 4.0), Pos2::new(cx, b)], s);
            p.hline(cx - 3.5..=cx + 3.5, b, s);
        }
        Glyph::AllMonitors => {
            let a = Rect::from_min_max(Pos2::new(l, t + 2.0), Pos2::new(l + 7.5, b - 4.0));
            let c = Rect::from_min_max(Pos2::new(rt - 7.5, t + 2.0), Pos2::new(rt, b - 4.0));
            p.rect_stroke(a, CornerRadius::same(2), s, StrokeKind::Middle);
            p.rect_stroke(c, CornerRadius::same(2), s, StrokeKind::Middle);
            let cx = r.center().x;
            p.line_segment([Pos2::new(cx, b - 4.0), Pos2::new(cx, b)], s);
            p.hline(cx - 4.0..=cx + 4.0, b, s);
        }
    }
}

/// Keycaps sized to their text, for right-aligned shortcut rows. Empty
/// combo → faint "Not set". Painted, so it works in any layout direction.
pub fn keycaps_compact(ui: &mut Ui, combo: &str) -> Response {
    let font = FontId::proportional(12.5);
    let keys: Vec<&str> = combo.split('+').map(str::trim).filter(|k| !k.is_empty()).collect();
    if keys.is_empty() {
        return ui.label(RichText::new("Not set").italics().color(TEXT_FAINT));
    }
    let galleys: Vec<_> = keys
        .iter()
        .map(|k| ui.painter().layout_no_wrap((*k).to_owned(), font.clone(), TEXT))
        .collect();
    let (cap_h, pad, gap, min_w) = (24.0, 8.0, 4.0, 24.0);
    let widths: Vec<f32> = galleys.iter().map(|g| (g.size().x + 2.0 * pad).max(min_w)).collect();
    let total = widths.iter().sum::<f32>() + gap * (widths.len() - 1) as f32;
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(total, cap_h + 2.0), Sense::hover());
    if ui.is_rect_visible(rect) {
        let p = ui.painter();
        let mut x = rect.left();
        for (g, w) in galleys.into_iter().zip(widths) {
            let cap = Rect::from_min_size(Pos2::new(x, rect.top()), Vec2::new(w, cap_h));
            let r = CornerRadius::same(6);
            // Key "depth": a darker lip 2 px under the cap.
            p.rect_filled(cap.translate(Vec2::new(0.0, 2.0)), r, FIELD);
            p.rect_filled(cap, r, CONTROL);
            p.rect_stroke(cap, r, Stroke::new(1.0_f32, BORDER_STRONG), StrokeKind::Inside);
            let pos = cap.center() - g.size() / 2.0;
            p.galley(pos, g, TEXT);
            x += w + gap;
        }
    }
    resp
}

/// Segmented control writing the raw value of the picked option
/// (e.g. "png") while showing its label ("PNG").
pub fn segmented(ui: &mut Ui, value: &mut String, options: &[(&str, &str)]) -> Response {
    let font = FontId::proportional(13.0);
    let (h, seg_w, inset) = (30.0, 58.0, 3.0);
    let size = Vec2::new(seg_w * options.len() as f32 + 2.0 * inset, h);
    let (rect, mut resp) = ui.allocate_exact_size(size, Sense::hover());
    let p = ui.painter().clone();
    p.rect_filled(rect, CornerRadius::same(RADIUS), FIELD);
    p.rect_stroke(rect, CornerRadius::same(RADIUS), Stroke::new(1.0_f32, BORDER), StrokeKind::Inside);
    for (i, (raw, label)) in options.iter().enumerate() {
        let seg = Rect::from_min_size(
            Pos2::new(rect.left() + inset + seg_w * i as f32, rect.top() + inset),
            Vec2::new(seg_w, h - 2.0 * inset),
        );
        let r = ui.interact(seg, resp.id.with(i), Sense::click());
        let on = value == raw;
        r.widget_info(|| WidgetInfo::selected(WidgetType::RadioButton, true, on, *label));
        if r.clicked() && !on {
            *value = (*raw).to_owned();
            resp.mark_changed();
        }
        let rr = CornerRadius::same(RADIUS - 2);
        if on {
            p.rect_filled(seg, rr, CONTROL_ACTIVE);
            p.rect_stroke(seg, rr, Stroke::new(1.0_f32, BORDER_STRONG), StrokeKind::Inside);
        } else if r.hovered() {
            p.rect_filled(seg, rr, CONTROL);
        }
        if r.has_focus() {
            p.rect_stroke(seg, rr, Stroke::new(1.5_f32, ACCENT_HOVER), StrokeKind::Outside);
        }
        let color = if on { TEXT } else { TEXT_MUTED };
        let font = if on { FontId::proportional(13.0) } else { font.clone() };
        p.text(seg.center(), Align2::CENTER_CENTER, *label, font, color);
        resp |= r;
    }
    resp
}

/// Smaller secondary button (28 px) for use inside list rows.
pub fn compact_button(ui: &mut Ui, text: &str) -> Response {
    ui.scope(|ui| {
        ui.spacing_mut().button_padding = Vec2::new(10.0, 4.0);
        ui.add(egui::Button::new(RichText::new(text).size(13.0)).min_size(Vec2::new(0.0, 28.0)))
    })
    .inner
}

/// Text-only button that turns red on hover (Clear in the shortcut editor).
pub fn danger_link(ui: &mut Ui, text: &str) -> Response {
    ui.scope(|ui| {
        let w = &mut ui.visuals_mut().widgets;
        w.hovered.fg_stroke.color = RED;
        w.active.fg_stroke.color = RED;
        ui.add(
            egui::Button::new(RichText::new(text).color(TEXT_MUTED))
                .frame(false)
                .min_size(Vec2::new(0.0, 32.0)),
        )
    })
    .inner
}

/// Amber notice strip with an action, for "Shortcuts aren't on yet".
pub fn notice<R>(ui: &mut Ui, text: &str, action: impl FnOnce(&mut Ui) -> R) -> R {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_H + 4.0), Sense::hover());
    let p = ui.painter();
    let inner = rect.shrink(4.0);
    p.rect_filled(inner, CornerRadius::same(RADIUS), AMBER.gamma_multiply(0.12));
    p.circle_filled(Pos2::new(inner.left() + 16.0, inner.center().y), 4.0, AMBER);
    p.text(Pos2::new(inner.left() + 28.0, inner.center().y), Align2::LEFT_CENTER, text, FontId::proportional(13.0), TEXT);
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(inner.shrink2(Vec2::new(8.0, 0.0)))
            .layout(Layout::right_to_left(Align::Center)),
        action,
    )
    .inner
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
                primary_button(ui, "Save");
                status_pill(ui, GREEN, "Running");
            });
        });
        assert!(!on);
    }
}
