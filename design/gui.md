# Frame GUI design

Target look: `design/settings-mockup.png`. That image is a real egui frame
drawn with `gui/src/theme.rs` (from a scratch crate, not the app), so every
widget in it already exists in theme.rs.

Icons: `gui/icons/frame*.svg` → `gui/icons/png/` via `render.sh`;
`gui/icons/preview.png` shows every size on Breeze dark and light.

## Settings window layout

Width about 600 px, minimum 520. Three regions:

1. **Header** (top panel, `theme::page_frame()`):
   app icon 36 px · "Frame" (19 px, strong) over the muted line "Screenshots,
   replay and recording" · right-aligned `status_pill`:
   - `Replay on · 60 s` – ACCENT dot (same blue as the tray's replay dot)
   - `Recording · 0:42` – RED dot
   - `Replay off` – TEXT_FAINT dot
   The pill is read from the same pid files the tray uses.
2. **Body** (central panel, `page_frame()` + vertical `ScrollArea`):
   four `theme::card`s, 12 px apart, in this order:
   - **Saving** – Save folder (text field + Browse…), Image format,
     Video format, "After capture": toggles *Copy to clipboard* and
     *Notification with preview* (second toggle on a row with an empty label).
   - **Video & replay** – Replay buffer toggle *Always on while Frame
     runs*; Replay length (DragValue in s, disabled when *Auto (by free
     RAM)* is on); Source; Frame rate; Quality; Codec; Audio. The
     "Restart service" offer appears as a hint + secondary button at the
     bottom of this card.
   - **Shortcuts** – one row per action: `keycaps` (180 px wide, "Not set"
     when empty) · Record · Clear. While recording, the Record button
     becomes the accent `primary_button("Stop")` and the keycaps show the
     live keys. Hint at the bottom. "Turn on shortcuts" (when never
     bound) is a `primary_button` at the top of this card.
   - **Startup** – *Start Frame at login (tray + replay)*, hint "Frame
     comes back with your saved settings and replay buffer."
   Config path and validation errors: a `hint` under the card the bad
   key belongs to (error text in RED), not a line at the top of the window.
3. **Bottom bar** (bottom panel, `theme::bottom_bar_frame()`, always
   visible): left: AMBER dot + "Unsaved changes" (hidden when clean);
   right: `primary_button("Save")` then `secondary_button("Revert")`.
   Both disabled when there is nothing to save (Save turns grey).

Rules:
- Every row is `theme::row(ui, label, …)`: 132 px label column
  (`LABEL_WIDTH`), so controls line up across all cards.
- On/off settings use `theme::toggle_label`, never checkboxes.
- Combo boxes 200 px wide. Show friendly text ("Very high", "MP4",
  "Desktop audio", "Auto (AV1)") while writing the raw config values.
- Card titles are `section_header` (upper case, 11.5 px, TEXT_MUTED).
- No separators; cards and spacing do the grouping.

## As built (phase 2, settings.rs)

- Bottom bar order is Revert then Save (Save rightmost); status text
  (red error, green "Saved.", muted otherwise) wraps above the buttons.
- Record on a shortcut row opens the shortcut editor dialog (themed
  `egui::Modal`, CARD fill, BORDER_STRONG edge, 20 px padding) already
  listening; it shows the combo as keycaps in a FIELD box (ACCENT edge
  while listening), Record/Stop, Meta/Ctrl/Alt/Shift chips + key combo,
  then Cancel / accent Set. No "Restart service" button (the tray
  restarts the daemon itself).
- Status pill also has "Starting…" (AMBER: tray up, daemon not yet) and
  "Not running" (TEXT_FAINT). Real shots: `shots/frame-settings*.png`,
  `shots/frame-shortcut-editor.png`.

## Colours (dark theme)

| Token | Hex | Use |
|---|---|---|
| BG | `#15171C` | window / page background |
| FIELD | `#111317` | text fields, sunken areas |
| CARD | `#1D2027` | cards, bottom bar, popups |
| CONTROL | `#272B34` | buttons, combos, keycaps, toggle off |
| CONTROL_HOVER | `#303540` | hovered control |
| CONTROL_ACTIVE | `#383E4A` | pressed / open control |
| BORDER | `#2B2F38` | card + control outlines |
| BORDER_STRONG | `#3A404C` | hovered outline, keycap edge |
| TEXT | `#E6E9EF` | body text |
| TEXT_MUTED | `#9AA2B1` | card titles, secondary text |
| TEXT_FAINT | `#6B7380` | hints, "Not set" |
| ACCENT | `#3D8BFF` | Save, toggles on, focus, replay status |
| ACCENT_HOVER | `#5A9CFF` | hovered accent, links |
| ACCENT_PRESSED | `#2F74E0` | pressed accent |
| RED | `#F0414B` | recording, errors |
| GREEN | `#34C77B` | success ("Saved") |
| AMBER | `#F5B544` | unsaved changes, warnings |

App icon gradient `#5A9CFF` → `#1F5BD8` (top-left to bottom-right),
white marks. Tray: idle `#A8ADB4` marks; replay/recording `#FCFCFC` marks
with a `#3D8BFF` / `#F0414B` dot; all with a `#232629` outline so they
read on Breeze dark and light panels.

Type: egui default font. Heading 20, Body/Button 14, Small 12,
Monospace 13. Radius: controls 8, cards 12, pills fully round.
Spacing: item 8×8, button padding 12×6, controls 30 px tall,
card padding 16 (12 top).

## theme.rs API

`apply(ctx)` once at start-up (it sets the dark theme style).
Widgets: `card`, `section_header`, `row`, `hint`, `primary_button`,
`secondary_button`, `toggle`, `toggle_label`, `status_pill`, `keycaps`,
`bottom_bar_frame`, `page_frame`; colour and size constants as above.
