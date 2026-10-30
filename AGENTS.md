# AGENTS.md — Frame (formerly Clipped; repo folder `frame`)

## Read first

Before any work, read `README.md`, this file and `HANDOFF.md`.
After lasting work, update `HANDOFF.md` (state, next steps, decisions,
lessons, checks you actually ran and what they printed).

## Goal

A screenshot tool driven by global shortcuts (region, window, monitor,
all). Saves to `<save_dir>/frame/images`, copies to the clipboard, works
on KDE, GNOME, Hyprland, Sway and X11. One config file. Since 0.5 it is
one Rust binary, `frame` (Kevin, 2026-10-29: everything in Rust, an
AppImage later): the CLI, the tray icon and the settings window. The
tray runs `frame bind` once when it starts (Hyprland/Sway forget runtime
binds) and starts at login via `~/.config/autostart/frame.desktop`
(`frame tray`).

Pictures only (Kevin, 2026-10-29): no video, replay buffer, recording,
gpu-screen-recorder, `frame daemon` or `frame.service`. Don't add them
back. Old configs' video keys are ignored silently (`config::OLD_KEYS`)
and bind/unbind still remove 0.3's replay/record shortcuts
(`config::OLD_ACTIONS`); `frame install` removes 0.3's frame.service, those
.desktop entries, and 0.4's `bin/frame-gui` and its autostart entry.

## Constraints

- MIT licence. All code is our own: do not copy code from Hyprshot or
  other projects. No AI co-author trailers or assistant references in
  commits or in files that go public (Kevin's rule).
- One binary, no Bash engine, nothing else to install (Kevin,
  2026-10-29): capture, area selection, encoding and clipboard are our
  own code. Crates stay lean: prefer crates already in `Cargo.lock`
  (eframe/egui glow, ksni, png, rfd, zbus, winit, wayland-client,
  wayland-protocols(-wlr), x11rb, image) and pin them to the locked
  version. Crates 0.5 added to the lock: serde_json, image-webp (WebP).
  The only programs still run are the desktop's own: hyprctl / swaymsg /
  gsettings for binds, plus xdg-open, update-desktop-database and
  gtk-update-icon-cache only if present (last-resort/optional).
- Backends (`src/detect.rs`): KDE Wayland = KWin ScreenShot2 (`grab.rs`);
  other Wayland = ext-image-copy-capture or wlr-screencopy (`wl.rs`), with
  focused window/monitor from Hyprland/Sway IPC sockets (`ipc.rs`);
  GNOME Wayland (and Wayland without screencopy) = portal Screenshot
  (`portal.rs`); X11 = GetImage (`x11.rs`). Region everywhere = capture
  first, then our selector over the frozen picture (`select.rs`:
  layer-shell, else fullscreen xdg-toplevels; X11 window in `x11.rs`),
  crop with `pixels.rs`. Encoding: `pixels::save` (png, jpg q90, lossless
  webp). Clipboard: `frame _clip FILE MIME data-control|focus|x11`
  (`wlclip.rs`, X11 owner in `x11.rs`).
- Wayland event loops: flush after dispatching, before blocking
  (`Wl::wait`), or requests made in handlers (get_pointer) sit unsent.
- Output transforms (`pixels::apply_transform`) were checked on Sway for
  all 8 values with both capture protocols; keep the mapping.
- KDE Wayland screenshots go through KWin's ScreenShot2 D-Bus API in
  this process (`src/grab.rs`): KWin <= 6.7 only authorizes the binary
  named by a .desktop entry with
  `X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2` and reads
  only the first entry whose Exec matches, so `frame.desktop` (`frame
  install`) and every `frame-<action>.desktop` (`frame bind`) carry it.
  KWin 6.8 no longer enforces the allow-list; keep the key for <= 6.7.
  The AppImage's binary lives in a temp mount, hence `frame install`. Never
  authorize a shell tool (gdbus, python, busctl) that way: any script
  could then take screenshots. KWin (6.8.2) and Mutter offer neither
  screencopy protocol, so they keep their own paths.
- KDE shortcuts: kglobalaccel over zbus (`src/binds.rs`). setShortcut
  flags must be 6 (SetPresent|NoAutoloading) or the key is stored but
  inactive; retry ~10x300 ms while isGlobalShortcutAvailable is true
  (KDE needs ~1.5 s to see a new .desktop entry).
- `src/config.rs` is the only config reader/writer. The writer only
  changes edited values (comments, order, unknown keys kept). The
  settings window checks the new text (`cli::check_file`) on a temp file
  before the rename. `DEFAULT_TEXT` and `DEFAULTS` must agree (a test
  checks this).
- One config file, parsed as `key = value` text. Never execute it.
- Pick backends at runtime by desktop/session (`src/detect.rs`); never
  assume Hyprland.
- Commands take a `Ctx` (`src/ctx.rs`), never the process environment
  directly, so tests can run them in parallel on temp dirs. D-Bus too:
  `ctx.session_bus()` (address from the Ctx env; tests have none, so
  they can't reach the real bus), never `Connection::session()`.
  Output goes through `ctx.say` / `ctx.warn` / `ctx.err_raw`.
- Tests must not take real screenshots, touch the clipboard, send
  notifications, change real shortcuts or install into the real
  `~/.local`. CLI tests use `ctx::testenv::T` (dry run, temp XDG dirs,
  `PATH` = only its fake dir, output captured). Settings tests hold
  `testutil::env_lock()` and call `test_env` (it sets PREFIX to a temp
  dir). Install tests (`src/install.rs`) use T with a temp PREFIX and
  `--from` a logging stand-in; their fake running frames are copies of
  /bin/sh in the temp tree. A real-mode uninstall runs `binds::unbind`:
  never run one outside a temp HOME with no D-Bus and a fake desktop
  (smoke.sh's last check does exactly that).
- Local repo + public GitHub (github.com/uhd3mon/frame). Push only with
  Kevin's OK. No credentials or personal notes in the repo.

## Layout

```
src/main.rs      dispatch: no args/tray/settings = GUI, else cli
src/cli.rs       CLI commands (usage, check, config, status, --detect)
src/ctx.rs       Ctx: env, paths, run/dry-run, quoting; testenv for tests
src/config.rs    config reader/validator/writer, DEFAULT_TEXT, OLD_KEYS
src/detect.rs    desktop/session and backend choice
src/shot.rs      picks the backend per mode, save, clipboard
src/pixels.rs    Rect/Shot, raw -> RGBA, transforms, stitch, crop, png/jpg/webp
src/grab.rs      KWin ScreenShot2 (KDE Wayland)
src/wl.rs        Wayland outputs, shm buffers, ext/wlr screencopy
src/select.rs    area selector (Sel logic, painter, Wayland overlay)
src/ipc.rs       Hyprland / Sway IPC sockets (focused window / monitor)
src/portal.rs    portal Screenshot + OpenFile, Mutter primary monitor
src/x11.rs       X11 capture, selector window, CLIPBOARD owner
src/wlclip.rs    `frame _clip`: data-control, GNOME focus method, X11
src/notify.rs    notifications over D-Bus (`frame _toast`)
src/binds.rs     bind/unbind: KDE (zbus), Hyprland, Sway, GNOME; state
src/install.rs   frame install / uninstall (PREFIX, DESTDIR, manifest, upgrades)
src/keys.rs      shortcut names -> Qt key codes / desktop syntax
src/tray.rs, settings.rs, theme.rs, icons.rs, state.rs   tray + window
icons/           app + tray icons (SVG sources, png/ rendered by render.sh)
install.sh       cargo build --release + `frame install` (thin wrapper)
tests/smoke.sh   install.sh wrapper checks + isolated real install
README.md        user overview (GitHub front page)
docs/            images the README shows (only docs/*.png is tracked)
LICENSE          MIT
AGENTS.md        rules for contributors and agents
HANDOFF.md       current state and history
```

## Commands (verified 2026-10-29)

```sh
cargo test               # "test result: ok. 101 passed"
cargo clippy --all-targets   # no warnings
cargo build --release    # ~41 s; 7.6 MB stripped binary
bash tests/smoke.sh      # "8 checks, all passed" (last two use a built frame)
target/debug/frame --detect   # backends and resolved settings
DESTDIR=/tmp/x ./install.sh --no-build   # 11 files + manifest; then --uninstall
packaging/appimage/build.sh   # dist/Frame-x86_64.AppImage, 4.1 MB (Docker)
```

Real-mode checks without touching Kevin's session: run a private
headless sway (`WLR_BACKENDS=headless WLR_RENDERER=pixman
WLR_HEADLESS_OUTPUTS=2 sway -c CFG` with its own `XDG_RUNTIME_DIR`; give
the outputs different scales/transforms), then run frame there with temp
XDG dirs, `notify = false`, `XDG_CURRENT_DESKTOP=sway`, `SWAYSOCK` and no
`DBUS_SESSION_BUS_ADDRESS`. The headless seat has no input devices: a
throwaway client with zwlr_virtual_pointer + zwp_virtual_keyboard (one
process for a whole drag, or the seat loses the pointer) drives the
selector; `wl-paste` reads the clipboard. Test switches:
`FRAME_SCREENCOPY=wlr` (skip ext-image-copy-capture), `FRAME_SELECT=xdg`
(selector without layer-shell, as on GNOME), `FRAME_DEBUG=1` (prints the
capture protocol). X11: `Xvfb :N` plus a throwaway x11rb client (XTEST
input, a painted window as `_NET_ACTIVE_WINDOW`, a CLIPBOARD reader with
INCR). KWin: `dbus-run-session` + `kwin_wayland --virtual --socket NAME`
(private bus; `frame --grab all` shows the overlay; no input injection).

GUI checks: set temp `XDG_RUNTIME_DIR`, `XDG_CONFIG_HOME`,
`XDG_STATE_HOME`, `FRAME_CONFIG`, `XDG_CURRENT_DESKTOP=frame-test`; keep
`DBUS_SESSION_BUS_ADDRESS` and use an absolute
`WAYLAND_DISPLAY=/run/user/$UID/wayland-1` (agent shells).
`FRAME_GUI_SHOT=file.png frame settings` saves the window's own frame and
quits (no desktop screenshot); `FRAME_GUI_SHOT_VIEW=end` scrolls down
first, `=region` (any action) opens its shortcut editor. To run the
tray without an icon in the real panel, use `dbus-run-session` with temp
XDG dirs and `XDG_CURRENT_DESKTOP=none`.
