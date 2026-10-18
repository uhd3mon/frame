# clip

Screenshots, instant-replay clips and recordings for Linux, driven by
global shortcuts: a small background daemon and one config file, plus an
optional tray icon and settings window (`clip-gui`).
Works on KDE Plasma, GNOME, Hyprland, Sway and X11.

## Quick start

```sh
bin/clip config        # write the default config, print its path
bin/clip service on    # start the daemon now and at every login
```

The daemon sets the shortcuts below and keeps a replay buffer running.
Run `clip service off` to stop it and `clip unbind` to drop the shortcuts.

## Install

```sh
./install.sh             # builds clip-gui (needs cargo), installs to ~/.local
./install.sh --uninstall
```

It installs `bin/clip` and `clip-gui` to `$PREFIX/bin` (default
`~/.local`), the `clip` icon to `share/icons/hicolor` and a **Clip**
start-menu entry (`share/applications/clip.desktop`, runs
`clip-gui --open`). `PREFIX=/usr/local` and `DESTDIR=/tmp/pkg` work as
usual; `--no-build` installs an existing `gui/target/release/clip-gui`.
The list of installed files goes to `share/clip/installed`. Install
refuses to replace a `bin/clip` or `bin/clip-gui` it didn't put there
(`--force` does). `--uninstall` removes only the listed files; for a
real install (no `DESTDIR`) it first stops the tray, runs
`clip service off` and `clip unbind`, and removes the "Start tray at
login" entry if it starts this clip-gui.

## Tray and settings (clip-gui)

`clip-gui` is a tray icon that shows what clip is doing: grey = idle,
teal = replay buffer running, red = recording. The tooltip says the
same (“clip: recording to …”, “clip: replay buffer 60 s”, “clip: not
running”). Its menu: Start/Stop recording, Save replay, Screenshot
(region / window / monitor / all), Open clip folder, Background service
(`clip service on|off`), Settings…, Quit. Clicking the icon opens the
settings.

The settings window edits the config file: save folder and formats,
video and replay options, the six shortcuts (type them, or press
**Record** and then the keys; egui can't see the Meta key on Linux, so
an *add Meta* box adds it) and **Start tray at login** (writes
`~/.config/autostart/clip-gui.desktop`). Save keeps your comments and
line order and only changes the values you edited. It checks the new
file with `clip check` first; if clip rejects it, the real file is left
alone and the error is shown. Changed shortcuts are re-applied right
away when the daemon runs (or clip has bound them before); changed video
settings offer **Restart service**.

| Command | What it does |
|---|---|
| `clip-gui` | tray (only one runs; a second start opens settings) |
| `clip-gui --open` | tray if not running, and the settings window |
| `clip-gui --settings` | only the settings window (one at a time) |

The tray needs a StatusNotifierItem host (KDE, most panels; GNOME needs
the AppIndicator extension). clip-gui finds clip via `$CLIP_BIN`, then
`clip` next to itself, then the repo's `bin/clip`, then `PATH`.

## Commands

| Command | What it does |
|---|---|
| `clip region` | pick an area |
| `clip window` | the active window |
| `clip monitor` | the current monitor |
| `clip all` | all monitors |
| `clip replay` | save the last N seconds of the replay buffer |
| `clip record` | start a recording; run again to stop |
| `clip daemon` | foreground: shortcuts + replay buffer |
| `clip bind` / `clip unbind` | add / remove the shortcuts |
| `clip service on\|off` | systemd user unit `clip.service` |
| `clip status` | daemon, replay, recording, service |
| `clip config` | create the config if missing, print path |
| `clip check` | validate the config: `ok`, or the error and exit 2 |
| `clip --detect` | show the backends clip would use |

Each capture is saved, copied to the clipboard (images as image data,
videos as a file link you can paste into apps) and announced with a
notification that previews it and has **Open** and **Show in folder**
buttons. On KDE the preview can be dragged out like Spectacle's; videos
get a frame grabbed with ffmpeg (cached in `~/.cache/clip/thumbs`).
Errors from a shortcut (no recorder, replay not running…) show a
critical notification.

## Config

One file: `~/.config/clip/config` (or `$XDG_CONFIG_HOME/clip/config`;
`CLIP_CONFIG` overrides it). Plain `key = value` lines. `#` starts a
comment at the start of a line or after a space (`a#b` is a value).
Values may be wrapped in one pair of quotes. Only `~` and `~/` are
expanded, `$VARS` are not; `save_dir` must end up absolute. The file is
parsed, never executed; bad values stop clip with exit code 2.

| Key | Default | Notes |
|---|---|---|
| `save_dir` | `~/Documents` | files go to `clip/images`, `clip/videos` |
| `image_format` | `png` | `png`, `jpg`, `webp` |
| `video_format` | `mp4` | `mp4`, `webm`, `mkv` |
| `clipboard` | `true` | |
| `notify` | `true` | notify-send toast with preview |
| `replay` | `true` | replay buffer in the daemon |
| `replay_seconds` | `auto` | 5–1800; auto = 60 with ≥ 8 GiB free RAM, else 30 |
| `video_source` | `auto` | focused/first monitor, or `DP-1`, `portal`… |
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
| KDE Plasma | spectacle | KGlobalAccel (via .desktop files) |
| Hyprland | grim + slurp + hyprctl | `hyprctl keyword bind` |
| Sway | grim + slurp + swaymsg | `swaymsg bindsym` |
| GNOME | gnome-screenshot (`monitor` = `all`) | gsettings custom keybindings |
| X11 (other) | maim + slop + xdotool, or import | printed for you to add |

Clipboard: wl-copy on Wayland, xclip on X11. Video: gpu-screen-recorder
(replay buffer in RAM, saved with SIGUSR1). Without it, `clip record`
falls back to ffmpeg x11grab on X11 (x264 + AAC for mp4/mkv, VP9 + Opus
for webm); replay needs gpu-screen-recorder.

gnome-screenshot has no per-monitor mode, so on GNOME `clip monitor`
captures all monitors.

clip never takes a shortcut that is already in use: KDE reports taken
combos (clip warns and skips them), on Hyprland it checks
`hyprctl binds -j`, on Sway it reads your config (`swaymsg -t
get_config`; best effort: `set $var` is expanded, mode blocks count as
taken). What clip bound is recorded in `~/.local/state/clip/binds`, and
`clip unbind` removes exactly that, not whatever the config says now.
If GNOME's shortcut list can't be read, clip changes nothing.

Hyprland and Sway shortcuts only last for the running session, so keep
the daemon running (it applies them on start). For `clip service on`
there, the compositor must export its environment to systemd
(`systemctl --user import-environment`); `clip service on` imports the
display variables of the shell you run it from.

## Test

```sh
bash tests/smoke.sh
```

Uses `CLIP_DRY_RUN=1`, a temp config and temp XDG dirs, with fake
recorder / gsettings / notify-send first on `PATH`: no screenshots,
recordings, clipboard, notifications or shortcut changes. If cargo is
installed it also runs the GUI's unit tests (`cargo test` in `gui/`),
and if a release build exists it installs into a temp `DESTDIR` and a
temp `PREFIX` (with a stand-in clip and tray) and uninstalls again.

## Licence

MIT. See [LICENSE](LICENSE).
