# AGENTS.md — Frame (formerly Clipped; repo folder `frame`)

## Read first

Before any work, read `README.md`, this file and `HANDOFF.md`.
After lasting work, update `HANDOFF.md` (state, next steps, decisions,
lessons, checks you actually ran and what they printed).

## Goal

A background screenshot / replay / recording tool driven by global
shortcuts (region, window, monitor, all, replay, record). Saves to
`<save_dir>/frame/images` and `/videos`, copies to the clipboard, works on
KDE, GNOME, Hyprland, Sway and X11. One config file. `bin/frame` is the
engine; `frame-gui` (Rust, `gui/`) is the tray icon + settings window on
top of it. "Frame is running" = the tray process: it runs `frame daemon`
(shortcuts + replay buffer) as its child, keeps it alive with backoff,
restarts it when a daemon-start setting changes and stops it on Quit /
SIGTERM (`gui/src/daemon.rs`). It starts at login via
`~/.config/autostart/frame-gui.desktop`. `frame service on|off`
(systemd) stays as the headless alternative; the tray treats a running
service daemon as running and never starts a second one.

## Constraints

- MIT licence. All code is our own: do not copy code from Hyprshot or
  other projects.
- Plain Bash, one script (`bin/frame`). Call existing tools. The only
  compiled part is `gui/` (Kevin asked for it, 2026-10-17): eframe/egui
  (glow) window, ksni tray, png, rfd, zbus. The GUI never does frame's
  work: it runs `frame …`, reads frame's pid files and edits the
  config, and it supervises `frame daemon` (above). One exception: `frame-gui --grab` (`gui/src/grab.rs`) takes
  KDE Wayland screenshots through KWin's ScreenShot2 D-Bus API, because
  KWin only authorizes a compiled binary named by a .desktop entry with
  `X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2` (Kevin wants
  no Spectacle, 2026-10-17). Never authorize a shell tool (gdbus, python,
  busctl) that way: any script could then take screenshots. Keep its
  deps lean; don't add a second binary.
- The GUI's config reader must follow bash `load_config` exactly, and
  its writer must only change edited values (comments, order, unknown
  keys kept). Every write goes through `frame check` on a temp file
  before the rename. Keys/defaults in `gui/src/config.rs` must match
  `default_config()` (a test checks this).
- One config file, parsed as `key = value` text. Never `source` or eval it.
- Pick backends at runtime by desktop/session; never assume Hyprland.
- grim does **not** work on KWin or Mutter. KDE Wayland uses
  `frame-gui --grab` (KWin ScreenShot2; slurp for region), Spectacle
  only when frame-gui is missing or on X11; GNOME uses gnome-screenshot. Video uses gpu-screen-recorder.
- Tests must not take real screenshots or recordings, touch the
  clipboard, send notifications, change real shortcuts or install into
  the real `~/.local` (use `DESTDIR`, or a temp `PREFIX` with a fake
  `bin/frame`: a real-mode `--uninstall` runs `frame service off/unbind`).
  GUI tests hold `testutil::env_lock()` and call `test_env` (dry run,
  temp XDG dirs, no shortcut backend). Use
  `FRAME_DRY_RUN=1`, a temp `FRAME_CONFIG`, temp XDG dirs (incl.
  `XDG_STATE_HOME`, `XDG_CACHE_HOME`, `TMPDIR`) and the fakes in
  `tests/smoke.sh` for real-mode runs. Supervisor tests use a fake
  `frame` script in a temp dir, never bin/frame's real daemon.
- Local repo only. Do not add a remote or push without Kevin's OK.
- No credentials or personal notes in the repo.

## Layout

```
bin/frame        the tool (executable Bash)
gui/             frame-gui crate: src/{main,tray,daemon,settings,config,state,icons,theme,grab}.rs
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
bash -n bin/frame      # syntax check
bash tests/smoke.sh    # 114 checks, "114 checks, all passed" (~15 s
                       # once gui/ is built; cargo test is one check)
bin/frame --detect     # shows chosen backends and resolved settings
cd gui && cargo test   # 55 tests, "test result: ok. 55 passed"
cd gui && cargo clippy # no warnings
cd gui && cargo build --release   # ~35 s clean; 6.6 MB stripped binary
DESTDIR=/tmp/x ./install.sh --no-build   # 12 files + manifest; then --uninstall
```

GUI checks without touching the real session state: set temp
`XDG_RUNTIME_DIR`, `XDG_CONFIG_HOME`, `XDG_STATE_HOME`, `FRAME_CONFIG`
and `FRAME_BIN=bin/frame`; keep `DBUS_SESSION_BUS_ADDRESS` and use an
absolute `WAYLAND_DISPLAY=/run/user/$UID/wayland-0` so the window still
opens. `FRAME_GUI_SHOT=file.png frame-gui --settings` saves the window's
own frame and quits (no desktop screenshot); `FRAME_GUI_SHOT_VIEW=end`
scrolls down first, `=region` (any action) opens its shortcut editor. Tray state can be read with
`busctl --user get-property <item> /StatusNotifierItem
org.kde.StatusNotifierItem ToolTip`. To run the tray without showing an
icon in the real panel, use `dbus-run-session -- bash script.sh` with
temp XDG dirs, `XDG_CURRENT_DESKTOP=none` (no shortcut backend) and
`replay = false` or a fake `FRAME_BIN`: the tray starts a real daemon.

Functions can be unit-tested by sourcing the script
(`source bin/frame; qt_keyint Meta+Alt+F12`); `main` only runs when it is
executed. shellcheck is not installed on the dev machine; run it if
available.
