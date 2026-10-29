# Frame GUI design

Frame is a screenshot tool only (video removed 2026-10-28). Target look:
`design/settings-v2.png` (normal state) and `design/settings-v2-states.png`
(left: not running + shortcuts never turned on; right: shortcut editor).
Both are real egui 0.33.3 frames drawn with `gui/src/theme.rs` plus the
proposed widgets in `design/theme-v2.rs`, rendered at 2× (960×1400 px for
a 480×700 pt window). The v1 look (`settings-mockup.png`) is superseded.

Icons: `gui/icons/frame.svg` (app) and `gui/icons/frame-tray.svg` (tray,
the only tray state) → `gui/icons/png/` via `render.sh`;
`gui/icons/preview.png` shows both at 16/22/32/64 on Breeze dark and light.

## Settings window (v2)

Window **480 × 700** (`with_inner_size([480.0, 700.0])`), minimum
**440 × 520**; below 700 tall the body scrolls, header and bottom bar stay.
No card titles inside cards: group titles sit above list cards, and
rows are separated by hairlines. Three regions:

1. **Header** (top panel, BG fill, margin 20 left/right, 16 top, 4 bottom,
   no separator): app icon 34 pt (`frame-128.png` texture) · 4 pt ·
   "Frame" (18, strong, TEXT) over "Screenshots on a shortcut" (12,
   TEXT_FAINT) · right-aligned `status_pill`:
   - `Running` – GREEN dot (tray process up: `tray.pid` live)
   - `Not running` – TEXT_FAINT dot
2. **Body** (central panel, BG, margin 16 left/right, 8 top, 12 bottom,
   vertical `ScrollArea`, item spacing y 6). Groups, each a
   `group_title` then a `list_card`, 6 pt extra between groups:
   - **Shortcuts** – four clickable rows with glyph tiles:
     Region, Window, Monitor, All monitors. Right side:
     `keycaps_compact` (or faint italic "Not set") then a chevron.
     Clicking anywhere on the row (or Enter/Space when focused) opens the
     shortcut editor already listening. No per-row Record/Clear buttons.
     When shortcuts were never bound, a `notice` strip is the card's first
     row: AMBER dot + "Shortcuts aren't turned on yet" + `primary_button`
     "Turn on" (runs `frame bind`).
   - **Saving** – row "Save to" with subtitle `<save_dir>/frame/images`
     (`~` for $HOME), right: `compact_button` "Open" (xdg-open the images
     folder, creating it first) and "Change…" (rfd folder picker writes
     `save_dir`). Row "Format": `segmented` PNG | JPG | WebP writing
     `png|jpg|webp`.
   - **Behaviour** – toggle rows (toggle on the right):
     "Copy to clipboard"; "Show a notification" / "With a preview of the
     picture"; "Start Frame at login" / "Lives in the tray, keeps
     shortcuts on" (autostart entry).
   Validation errors: a red `hint` directly under the card that owns the
   bad key; config path/"no config yet" goes in the bottom bar text.
3. **Bottom bar** (bottom panel, CARD fill, margin 16×10, **no box
   stroke, a 1 px BORDER hairline on top only**): left: AMBER 8 pt dot +
   "Unsaved changes" (13, TEXT_MUTED), or "All changes saved"
   (TEXT_FAINT), or a status/error line (GREEN "Saved.", RED errors).
   Right: `primary_button("Save")`, then `secondary_button("Revert")`
   to its left. Both disabled when clean.

### Shortcut editor (modal)

Same dialog as v1 (`settings.rs editor_dialog`) with three changes:
width 380 (fits the 480 window), the combo shown with `keycaps_compact`,
and a **"Clear shortcut"** `danger_link` bottom-left (writes an empty
binding, closes) opposite Cancel / Set. Everything else (Record/Stop,
Esc cancels, Meta/Ctrl/Alt/Shift chips + key combo, hint) stays.

### Row metrics

| Item | Size |
|---|---|
| One-line row (`ROW_H`) | 44 pt |
| Row with subtitle (`ROW_H_TALL`) | 54 pt |
| Row padding left/right (`ROW_PAD_X`) | 14 pt |
| Title / subtitle | 14 TEXT / 12 TEXT_FAINT, 3 pt apart |
| Glyph tile | 28 pt, radius 7, ACCENT at 16 %; 16 pt glyph, 1.6 stroke, ACCENT_HOVER |
| Hairline | 1 px BORDER, from the text column (after the tile) to the card's right edge |
| Row hover / focus | inset 4, radius 8, CONTROL at 70 %; focus adds a 1.5 ACCENT_HOVER ring |
| Chevron | 3×10 pt, 1.6 stroke, TEXT_FAINT (TEXT_MUTED on hover), 18 pt reserved |
| Keycap | 24 tall, min 24 wide, 8 side padding, radius 6, CONTROL + BORDER_STRONG edge, 2 pt FIELD lip, 12.5 text, 4 apart |
| Segmented | 30 tall, 58 per segment, 3 inset; track FIELD + BORDER; selected CONTROL_ACTIVE + BORDER_STRONG, TEXT; others TEXT_MUTED, hover CONTROL |
| Compact button | 28 tall, padding 10×4, 13 text |
| Group title | 13 strong TEXT_MUTED, sentence case, 4 pt indent |

Rules:
- On/off settings are `toggle`s on the right of a row; never checkboxes.
- Labels are sentence case and short; explanations go in the subtitle,
  not in hints under the card.
- Hairlines only inside list cards; spacing groups the cards.
- Show friendly text (PNG, WebP) while writing raw config values.

## Colours (dark theme)

| Token | Hex | Use |
|---|---|---|
| BG | `#15171C` | window / page background |
| FIELD | `#111317` | text fields, segmented track, keycap lip |
| CARD | `#1D2027` | cards, bottom bar, popups |
| CONTROL | `#272B34` | buttons, combos, keycaps, toggle off |
| CONTROL_HOVER | `#303540` | hovered control |
| CONTROL_ACTIVE | `#383E4A` | pressed / open control, selected segment |
| BORDER | `#2B2F38` | card outlines, hairlines |
| BORDER_STRONG | `#3A404C` | hovered outline, keycap edge |
| TEXT | `#E6E9EF` | body text |
| TEXT_MUTED | `#9AA2B1` | group titles, secondary text |
| TEXT_FAINT | `#6B7380` | subtitles, hints, "Not set" |
| ACCENT | `#3D8BFF` | Save, toggles on, focus, glyph tiles |
| ACCENT_HOVER | `#5A9CFF` | hovered accent, glyphs, focus rings |
| ACCENT_PRESSED | `#2F74E0` | pressed accent |
| RED | `#F0414B` | errors, Clear hover |
| GREEN | `#34C77B` | "Running", "Saved." |
| AMBER | `#F5B544` | unsaved changes, "shortcuts off" notice |

App icon gradient `#5A9CFF` → `#1F5BD8` (top-left to bottom-right),
white marks. Tray (`frame-tray.svg`): viewfinder corners (2.75 stroke on a
24 grid) + centre dot r 3.5, all `#FCFCFC`, with a thin `#232629` halo
(4.25 under-stroke, 0.75 dot edge). Built for dark panels (Kevin's KDE);
on light panels it still reads from 22 px up but looks outlined, and is
weak at 16 px.

Type: egui default font. Heading 20, Body/Button 14, Small 12,
Monospace 13. Radius: controls 8, cards 12, keycaps 6, pills round.
Spacing: item 8×8 (body y 6), button padding 12×6, controls 30 tall.

## theme.rs API

v2 is implemented (2026-10-29) in `gui/src/settings.rs` on these:
`apply(ctx)`, `section_header` (editor only), `hint`, `primary_button`,
`secondary_button`, `compact_button`, `danger_link`, `toggle`,
`status_pill`, `group_title`, `list_card`, `list_row` (+ `Glyph`,
`paint_glyph`, `ROW_H`, `ROW_H_TALL`, `ROW_PAD_X`), `keycaps_compact`,
`segmented`, `notice`, `bottom_bar_frame` (no stroke) + `top_hairline`,
`page_frame`. The v1 `card`, `row`, `toggle_label`, `keycaps` and
`LABEL_WIDTH` are gone. `list_row` lays out its right side first and
cuts the title/subtitle with "…" to the space left (long save folders).

Real frames of the built window: `shots/frame-settings-v2.png`,
`shots/frame-settings-v2-off.png`, `shots/frame-settings-v2-invalid.png`,
`shots/frame-shortcut-editor-v2.png` (1× scale; the mockups are 2×).
