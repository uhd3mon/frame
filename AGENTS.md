# AGENTS.md — Frame (formerly Clipped; repo folder `frame`)

## Read first

Before any work, read `README.md`, this file and `HANDOFF.md`.
After lasting work, update `HANDOFF.md` (state, next steps, decisions,
lessons, checks you actually ran and what they printed).

## Goal

A screenshot tool driven by global shortcuts (region, window, monitor,
all). Saves to `<save_dir>/frame/images`, copies to the clipboard, works
on KDE, GNOME, Hyprland, Sway and X11. One config file. `bin/frame` is
the engine; `frame-gui` (Rust, `gui/`) is the tray icon + settings
window on top of it. The tray runs `frame bind` once when it starts
(Hyprland/Sway forget runtime binds) and starts at login via
`~/.config/autostart/frame-gui.desktop`.

Pictures only (Kevin, 2026-10-29): no video, replay buffer, recording,
gpu-screen-recorder, `frame daemon` or `frame.service`. Don't add them
back. bin/frame ignores 0.3's video keys silently (`OLD_KEYS`) and
bind/unbind still remove its replay/record shortcuts (`OLD_ACTIONS`);
install.sh removes 0.3's frame.service and those .desktop entries.

## Constraints

- MIT licence. All code is our own: do not copy code from Hyprshot or
  other projects.
- Plain Bash, one script (`bin/frame`). Call existing tools. The only
  compiled part is `gui/` (Kevin asked for it, 2026-10-17): eframe/egui
  (glow) window, ksni tray, png, rfd, zbus. The GUI never does frame's
  work: it runs `frame …`, reads frame's pid files and edits the
  config. One exception: `frame-gui --grab` (`gui/src/grab.rs`) takes
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
  only when frame-gui is missing or on X11; GNOME uses gnome-screenshot.
- Tests must not take real screenshots, touch the
  clipboard, send notifications, change real shortcuts or install into
  the real `~/.local` (use `DESTDIR`, or a temp `PREFIX` with a fake
  `bin/frame`: a real-mode `--uninstall` runs `frame unbind`, and a real
  install over a 0.3 unit runs `systemctl --user`, so smoke.sh fakes
  systemctl).
  GUI tests hold `testutil::env_lock()` and call `test_env` (dry run,
  temp XDG dirs, no shortcut backend). Use
  `FRAME_DRY_RUN=1`, a temp `FRAME_CONFIG`, temp XDG dirs (incl.
  `XDG_STATE_HOME`, `XDG_CACHE_HOME`, `TMPDIR`) and the fakes in
  `tests/smoke.sh` for real-mode runs.
- Local repo only. Do not add a remote or push without Kevin's OK.
- No credentials or personal notes in the repo.

## Layout

```
bin/frame        the tool (executable Bash)
gui/             frame-gui crate: src/{main,tray,settings,config,state,icons,theme,grab}.rs
gui/icons/       app + tray icons (SVG sources, png/ rendered by render.sh)
install.sh       install / --uninstall under PREFIX (+ DESTDIR)
tests/smoke.sh   smoke tests, dry-run only
README.md        user overview
LICENSE          MIT
AGENTS.md        rules for contributors and agents
HANDOFF.md       current state and history
```

## Commands (verified 2026-10-29)

```sh
bash -n bin/frame      # syntax check
bash tests/smoke.sh    # "100 checks, all passed" (cargo test is one
                       # check; install checks need gui/target/release)
bin/frame --detect     # shows chosen backends and resolved settings
cd gui && cargo test   # "test result: ok. 46 passed"
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
temp XDG dirs, `XDG_CURRENT_DESKTOP=none` (no shortcut backend; the tray runs
`frame bind` at start).

Functions can be unit-tested by sourcing the script
(`source bin/frame; qt_keyint Meta+Alt+F12`); `main` only runs when it is
executed. shellcheck is not installed on the dev machine; run it if
available.
