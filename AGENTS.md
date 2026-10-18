# AGENTS.md — clipped

## Read first

Before any work, read `README.md`, this file and `HANDOFF.md`.
After lasting work, update `HANDOFF.md` (state, next steps, decisions,
lessons, checks you actually ran and what they printed).

## Goal

A background screenshot / replay / recording tool driven by global
shortcuts (region, window, monitor, all, replay, record). Saves to
`<save_dir>/clipped/images` and `/videos`, copies to the clipboard, works on
KDE, GNOME, Hyprland, Sway and X11. One config file. `bin/clipped` is the
engine; `clipped-gui` (Rust, `gui/`) is an optional tray icon + settings
window on top of it.

## Constraints

- MIT licence. All code is our own: do not copy code from Hyprshot or
  other projects.
- Plain Bash, one script (`bin/clipped`). Call existing tools. The only
  compiled part is `gui/` (Kevin asked for it, 2026-10-17): eframe/egui
  (glow) window, ksni tray, png, rfd. The GUI never does clipped's work: it
  runs `clipped …`, reads clipped's pid files and edits the config. Keep its
  deps lean; don't add a second binary.
- The GUI's config reader must follow bash `load_config` exactly, and
  its writer must only change edited values (comments, order, unknown
  keys kept). Every write goes through `clipped check` on a temp file
  before the rename. Keys/defaults in `gui/src/config.rs` must match
  `default_config()` (a test checks this).
- One config file, parsed as `key = value` text. Never `source` or eval it.
- Pick backends at runtime by desktop/session; never assume Hyprland.
- grim does **not** work on KWin or Mutter. KDE uses `spectacle -b -n`;
  GNOME uses gnome-screenshot. Video uses gpu-screen-recorder.
- Tests must not take real screenshots or recordings, touch the
  clipboard, send notifications, change real shortcuts or install into
  the real `~/.local` (use `DESTDIR`, or a temp `PREFIX` with a fake
  `bin/clipped`: a real-mode `--uninstall` runs `clipped service off/unbind`).
  GUI tests hold `testutil::env_lock()` and call `test_env` (dry run,
  temp XDG dirs, no shortcut backend). Use
  `CLIPPED_DRY_RUN=1`, a temp `CLIPPED_CONFIG`, temp XDG dirs (incl.
  `XDG_STATE_HOME`, `XDG_CACHE_HOME`, `TMPDIR`) and the fakes in
  `tests/smoke.sh` for real-mode runs.
- Local repo only. Do not add a remote or push without Kevin's OK.
- No credentials or personal notes in the repo.

## Layout

```
bin/clipped      the tool (executable Bash)
gui/             clipped-gui crate: src/{main,tray,settings,config,state,icons}.rs
gui/icons/       app + tray icons (SVG sources, png/ rendered by render.sh)
install.sh       install / --uninstall under PREFIX (+ DESTDIR)
tests/smoke.sh   smoke tests, dry-run only
README.md        user overview
LICENSE          MIT
AGENTS.md        rules for agents (CLAUDE.md points here)
HANDOFF.md       current state and history
```

## Commands (verified 2026-10-17)

```sh
bash -n bin/clipped    # syntax check
bash tests/smoke.sh    # 95 checks, "95 checks, all passed" (~14 s
                       # once gui/ is built; cargo test is one check)
bin/clipped --detect   # shows chosen backends and resolved settings
cd gui && cargo test   # 29 tests, "test result: ok. 29 passed"
cd gui && cargo clippy # no warnings
cd gui && cargo build --release   # ~35 s clean; 6.6 MB stripped binary
DESTDIR=/tmp/x ./install.sh --no-build   # 12 files + manifest; then --uninstall
```

GUI checks without touching the real session state: set temp
`XDG_RUNTIME_DIR`, `XDG_CONFIG_HOME`, `XDG_STATE_HOME`, `CLIPPED_CONFIG`
and `CLIPPED_BIN=bin/clipped`; keep `DBUS_SESSION_BUS_ADDRESS` and use an
absolute `WAYLAND_DISPLAY=/run/user/$UID/wayland-0` so the window still
opens. `CLIPPED_GUI_SHOT=file.png clipped-gui --settings` saves the window's
own frame and quits (no desktop screenshot). Tray state can be read with
`busctl --user get-property <item> /StatusNotifierItem
org.kde.StatusNotifierItem ToolTip`.

Functions can be unit-tested by sourcing the script
(`source bin/clipped; qt_keyint Meta+Alt+F12`); `main` only runs when it is
executed. shellcheck is not installed on the dev machine; run it if
available.
