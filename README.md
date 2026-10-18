# Frame

Screenshots, instant-replay clips and recordings for Linux, driven by
global shortcuts. `frame` (one Bash script) does the work; `frame-gui`
is the tray icon and settings window. One config file.
Works on KDE Plasma, GNOME, Hyprland, Sway and X11.

Frame used to be called Clipped. The first time it runs it copies
`~/.config/clipped/config` to `~/.config/frame/config` (if there is no
Frame config yet) and says so; the old file stays where it was. Nothing
else is moved: your old captures stay in `<save_dir>/clipped/images` and
`/videos`, new ones go to `<save_dir>/frame/`. `./install.sh` removes an
old Clipped install from the same `PREFIX` (below).

## Quick start

```sh
./install.sh    # builds frame-gui (needs cargo), installs to ~/.local
frame-gui       # start Frame now; from then on it starts when you log in
```

While Frame runs (its tray icon is there) the shortcuts below work and
the replay buffer keeps the last 30–60 s, so `Meta+Alt+C` can save it.
Quit from the tray menu stops both.

Without the tray (a server, a bare window manager):
`frame service on` runs the same daemon as the systemd user unit
`frame.service`, at every login; `frame service off` stops it. The tray
sees a running service and doesn't start a second daemon. Use one or
the other: `frame service on` refuses while the tray runs (quit it
first), and a daemon that finds another one holding the lock exits with
code 3, which the unit does not restart (`RestartPreventExitStatus=3`).

## Install

```sh
./install.sh                 # build + install to ~/.local
./install.sh --no-autostart  # same, but don't start Frame at login
./install.sh --uninstall
```

It installs `bin/frame` and `frame-gui` to `$PREFIX/bin` (default
`~/.local`), the `frame` icon to `share/icons/hicolor` and a **Frame**
start-menu entry (`share/applications/frame.desktop`, runs
`frame-gui --open`). A first install (not `DESTDIR`, not a reinstall)
also turns on **Start Frame when I log in**
(`~/.config/autostart/frame-gui.desktop`); a reinstall keeps whatever you
chose in Settings. `PREFIX=/usr/local` and `DESTDIR=/tmp/pkg` work as
usual; `--no-build` installs an existing `gui/target/release/frame-gui`.
The list of installed files goes to `share/frame/installed`. Install
refuses to replace a `bin/frame` or `bin/frame-gui` it didn't put there
(`--force` does). `--uninstall` removes only the listed files; for a
real install (no `DESTDIR`) it first stops the tray (which stops the
daemon; it waits up to 12 s), runs `frame service off` and
`frame unbind`, and removes the start-at-login entry if it starts this
frame-gui.

Upgrading from Clipped: a real install (no `DESTDIR`) that finds
`share/clipped/installed` in the same `PREFIX` first stops `clipped-gui`,
runs the old `clipped service off` and `clipped unbind`, removes the
files that list names (and the list) and
`~/.config/autostart/clipped-gui.desktop` if it starts that clipped-gui.
`~/.config/clipped` and your captures are not touched.

## Tray and settings (frame-gui)

The tray *is* Frame running. When it starts it runs `frame daemon`
(shortcuts + replay buffer) as its child and keeps it alive: if the
daemon exits it is started again after 2 s, then 4, 8 … up to 60 s. A
config `frame check` refuses is not retried until the file changes; the
menu and tooltip show the error. If a daemon already runs (for example
`frame.service`) the tray uses that one. When you save settings the
daemon reads only at start (replay on/off, length, source, format,
quality, codec, audio, save folder), the tray restarts the daemon (or
`frame.service`) by itself. Quit or SIGTERM stops the daemon (TERM; it
stops the recorder and removes its pid files), then the tray exits. Its pid is in
`$XDG_RUNTIME_DIR/frame/tray.pid`, the daemon's output in `daemon.log`
next to it.

The icon shows idle, replay buffer running, or recording; the tooltip
says the same (“Frame: recording to …”, “Frame: replay buffer 60 s”).
Menu: the replay state (“Replay buffer: on, 60 s”, “off”, “starting…”,
or the problem), Start/Stop recording, Save replay, Screenshot (region /
window / monitor / all), Open Frame folder, Settings…, Quit. Clicking the
icon opens the settings.

The settings window edits the config file: save folder and formats,
video and replay options, the six shortcuts (type them, or press
**Record** and then the keys) and **Start Frame when I log in**. Save
keeps your comments and line order and only changes the values you
edited. It checks the new file with `frame check` first; if frame
rejects it, the real file is left alone and the error is shown. Changed
shortcuts are re-applied right away (and on the first Save if Frame has
never bound them).

| Command | What it does |
|---|---|
| `frame-gui` | tray (only one runs; a second start opens settings) |
| `frame-gui --open` | tray if not running, and the settings window |
| `frame-gui --settings` | only the settings window (one at a time) |

The tray needs a StatusNotifierItem host (KDE, most panels; GNOME needs
the AppIndicator extension). frame-gui finds frame via `$FRAME_BIN`, then
`frame` next to itself, then the repo's `bin/frame`, then `PATH`.

## Commands

| Command | What it does |
|---|---|
| `frame region` | pick an area |
| `frame window` | the active window |
| `frame monitor` | the current monitor |
| `frame all` | all monitors |
| `frame replay` | save the last N seconds of the replay buffer |
| `frame record` | start a recording; run again to stop |
| `frame daemon` | foreground: shortcuts + replay buffer |
| `frame bind` / `frame unbind` | add / remove the shortcuts |
| `frame service on\|off` | systemd user unit `frame.service` (instead of the tray) |
| `frame status` | daemon, replay, recording, service |
| `frame config` | create the config if missing, print path |
| `frame check` | validate the config: `ok`, or the error and exit 2 |
| `frame monitors` | monitors you can record, `NAME\|WxH`, the one you're on marked `\|active` |
| `frame --detect` | show the backends frame would use |

Each capture is saved, copied to the clipboard (images as image data,
videos as a file link you can paste into apps) and announced with a
notification that previews it and has **Open** and **Show in folder**
buttons. On KDE the preview can be dragged out like Spectacle's; videos
get a frame grabbed with ffmpeg (cached in `~/.cache/frame/thumbs`).
Errors from a shortcut (no recorder, replay not running…) show a
critical notification.

## Config

One file: `~/.config/frame/config` (or `$XDG_CONFIG_HOME/frame/config`;
`FRAME_CONFIG` overrides it). Plain `key = value` lines. `#` starts a
comment at the start of a line or after a space (`a#b` is a value).
Values may be wrapped in one pair of quotes. Only `~` and `~/` are
expanded, `$VARS` are not; `save_dir` must end up absolute. The file is
parsed, never executed; bad values stop frame with exit code 2.

| Key | Default | Notes |
|---|---|---|
| `save_dir` | `~/Documents` | files go to `frame/images`, `frame/videos` |
| `image_format` | `png` | `png`, `jpg`, `webp` |
| `video_format` | `mp4` | `mp4`, `webm`, `mkv` |
| `clipboard` | `true` | |
| `notify` | `true` | notify-send toast with preview |
| `replay` | `true` | replay buffer in the daemon |
| `replay_seconds` | `auto` | 5–1800; auto = 60 with ≥ 8 GiB free RAM, else 30 |
| `video_source` | `auto` | monitor you're on (KDE, Hyprland, Sway; else the first), or `DP-1`, `portal`, `screen` (X11). A recording picks it when it starts; the replay buffer when the daemon starts |
| `video_fps` | `60` | 1–240 |
| `video_quality` | `very_high` | `medium`, `high`, `very_high`, `ultra` |
| `video_codec` | `auto` | best the GPU encodes: av1 > hevc > h264 (webm: av1, vp9, vp8) |
| `audio` | `default_output` | gpu-screen-recorder `-a` value, or `none` |
| `bind.region` | `Meta+Alt+R` | |
| `bind.window` | `Meta+Alt+W` | |
| `bind.monitor` | `Meta+Alt+M` | |
| `bind.all` | `Meta+Alt+A` | |
| `bind.replay` | `Meta+Alt+C` | |
| `bind.record` | `Meta+Alt+V` | |

Shortcuts use KDE-style names: modifiers `Meta`, `Ctrl`, `Alt`, `Shift`
plus exactly one key (a letter, digit, `F1`–`F35`, `Print`, `Space`,
`Home`, `PgUp`…). Anything else is rejected. An empty value means no
shortcut.
Files are named `YYYY-MM-DD_HH-MM-SS_<action>.<ext>`.

## Backends

| Desktop | Screenshots | Shortcuts |
|---|---|---|
| KDE Plasma (Wayland) | KWin itself via `frame-gui --grab` (+ slurp for region) | KGlobalAccel (via .desktop files) |
| KDE Plasma (X11), or no frame-gui | spectacle | KGlobalAccel |
| Hyprland | grim + slurp + hyprctl | `hyprctl keyword bind` |
| Sway | grim + slurp + swaymsg | `swaymsg bindsym` |
| GNOME | gnome-screenshot (`monitor` = `all`) | gsettings custom keybindings |
| X11 (other) | maim + slop + xdotool, or import | printed for you to add |

On KDE Wayland no Spectacle is needed: `frame-gui --grab
all|monitor|window|area X Y W H OUT.png` asks KWin's built-in
ScreenShot2 D-Bus API for the pixels. KWin only answers a program named
by a `.desktop` file with `X-KDE-DBUS-Restricted-Interfaces`, so this
works once `./install.sh` has written `frame.desktop` (its `Exec=` is
the absolute path of the installed frame-gui). frame looks for
frame-gui in `$FRAME_GUI`, next to itself, then on `PATH`.

Clipboard: wl-copy on Wayland, xclip on X11. Video: gpu-screen-recorder
(replay buffer in RAM, saved with SIGUSR1). Without it, `frame record`
falls back to ffmpeg x11grab on X11 (x264 + AAC for mp4/mkv, VP9 + Opus
for webm); replay needs gpu-screen-recorder.

gnome-screenshot has no per-monitor mode, so on GNOME `frame monitor`
captures all monitors.

frame never takes a shortcut that is already in use: KDE reports taken
combos (frame warns and skips them), on Hyprland it checks
`hyprctl binds -j`, on Sway it reads your config (`swaymsg -t
get_config`; best effort: `set $var` is expanded, mode blocks count as
taken). What frame bound is recorded in `~/.local/state/frame/binds`, and
`frame unbind` removes exactly that, not whatever the config says now.
If GNOME's shortcut list can't be read, frame changes nothing.

Hyprland and Sway shortcuts only last for the running session, so keep
the daemon running (it applies them on start). For `frame service on`
there, the compositor must export its environment to systemd
(`systemctl --user import-environment`); `frame service on` imports the
display variables of the shell you run it from.

## Test

```sh
bash tests/smoke.sh
```

Uses `FRAME_DRY_RUN=1`, a temp config and temp XDG dirs, with fake
recorder / gsettings / notify-send first on `PATH`: no screenshots,
recordings, clipboard, notifications or shortcut changes. If cargo is
installed it also runs the GUI's unit tests (`cargo test` in `gui/`),
and if a release build exists it installs into a temp `DESTDIR` and a
temp `PREFIX` (with a stand-in frame and tray) and uninstalls again.

## Licence

MIT. See [LICENSE](LICENSE).
