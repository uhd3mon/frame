# clip

Screenshots, instant-replay clips and recordings for Linux, driven by
global shortcuts. No UI: a small background daemon and one config file.
Works on KDE Plasma, GNOME, Hyprland, Sway and X11.

## Quick start

```sh
bin/clip config        # write the default config, print its path
bin/clip service on    # start the daemon now and at every login
```

The daemon sets the shortcuts below and keeps a replay buffer running.
Run `clip service off` to stop it and `clip unbind` to drop the shortcuts.

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
recordings, clipboard, notifications or shortcut changes.

## Licence

MIT. See [LICENSE](LICENSE).
