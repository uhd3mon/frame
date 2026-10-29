# Frame

Screenshots for Linux, driven by global shortcuts. `frame` (one Bash script) does the work; `frame-gui`
is the tray icon and settings window. One config file.
Works on KDE Plasma, GNOME, Hyprland, Sway and X11.

Frame used to be called Clipped. The first time it runs it copies
`~/.config/clipped/config` to `~/.config/frame/config` (if there is no
Frame config yet) and says so; the old file stays where it was. Nothing
else is moved: your old captures stay in `<save_dir>/clipped/`, new
screenshots go to `<save_dir>/frame/images`. `./install.sh` removes an
old Clipped install from the same `PREFIX` (below).

## Quick start

```sh
./install.sh    # builds frame-gui (needs cargo), installs to ~/.local
frame-gui       # start Frame now; from then on it starts when you log in
```

The tray binds the shortcuts below when it starts (`frame bind`). On
KDE and GNOME they stay bound after Quit and across logins; Hyprland and
Sway forget them when the session ends, so there keep the tray starting
at login, or run `frame bind` from your compositor's autostart.

Frame 0.4 is pictures only: replay, recording and `frame.service` are
gone. Old video keys in your config are ignored without a warning, and
`frame bind` / `frame unbind` remove the replay / record shortcuts an
older Frame bound.

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
real install (no `DESTDIR`) it first stops the tray (waits up to 12 s),
runs `frame unbind`, and removes the start-at-login entry if it starts
this frame-gui.

Upgrading from Frame 0.3 (video): a real install or uninstall turns off
and removes `~/.config/systemd/user/frame.service` and removes the
`frame-replay` / `frame-record` shortcut entries in
`~/.local/share/applications`. Your videos stay where they are.

Upgrading from Clipped: a real install (no `DESTDIR`) that finds
`share/clipped/installed` in the same `PREFIX` first stops `clipped-gui`,
runs the old `clipped service off` and `clipped unbind`, removes the
files that list names (and the list) and
`~/.config/autostart/clipped-gui.desktop` if it starts that clipped-gui.
`~/.config/clipped` and your captures are not touched.

## Tray and settings (frame-gui)

The tray runs `frame bind` once when it starts, then waits for you.
Menu: Region, Window, Monitor, All monitors, Open folder, Settings…,
Quit. Clicking the icon opens the settings. Its pid is in
`$XDG_RUNTIME_DIR/frame/tray.pid` (`frame status` shows it).

The settings window edits the config file: save folder, image format,
clipboard and notification switches, the four shortcuts (type them, or
press **Record** and then the keys) and **Start Frame at login**. Save
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
| `frame bind` / `frame unbind` | add / remove the shortcuts |
| `frame status` | config, save folder, bound shortcuts, tray |
| `frame config` | create the config if missing, print path |
| `frame check` | validate the config: `ok`, or the error and exit 2 |
| `frame --detect` | show the backends frame would use |

Each screenshot is saved, copied to the clipboard as image data and
announced with a notification that previews it and has **Open** and
**Show in folder** buttons. On KDE the preview can be dragged out like
Spectacle's. Errors from a shortcut (no screenshot tool, KWin refused…)
show a critical notification.

## Config

One file: `~/.config/frame/config` (or `$XDG_CONFIG_HOME/frame/config`;
`FRAME_CONFIG` overrides it). Plain `key = value` lines. `#` starts a
comment at the start of a line or after a space (`a#b` is a value).
Values may be wrapped in one pair of quotes. Only `~` and `~/` are
expanded, `$VARS` are not; `save_dir` must end up absolute. The file is
parsed, never executed; bad values stop frame with exit code 2.

| Key | Default | Notes |
|---|---|---|
| `save_dir` | `~/Documents` | screenshots go to `frame/images` |
| `image_format` | `png` | `png`, `jpg`, `webp` (jpg/webp via ffmpeg) |
| `clipboard` | `true` | |
| `notify` | `true` | notify-send toast with preview |
| `bind.region` | `Meta+Alt+R` | |
| `bind.window` | `Meta+Alt+W` | |
| `bind.monitor` | `Meta+Alt+M` | |
| `bind.all` | `Meta+Alt+A` | |

Shortcuts use KDE-style names: modifiers `Meta`, `Ctrl`, `Alt`, `Shift`
plus exactly one key (a letter, digit, `F1`–`F35`, `Print`, `Space`,
`Home`, `PgUp`…). Anything else is rejected. An empty value means no
shortcut. Keys Frame no longer uses (the video settings of Frame 0.3:
`video_*`, `replay*`, `audio`, `bind.replay`, `bind.record`) are
ignored; any other unknown key gets a warning.
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

Clipboard: wl-copy on Wayland, xclip on X11. Notifications: notify-send.
ffmpeg only for `jpg` / `webp` (screenshots are taken as PNG).

gnome-screenshot has no per-monitor mode, so on GNOME `frame monitor`
captures all monitors.

frame never takes a shortcut that is already in use: KDE reports taken
combos (frame warns and skips them), on Hyprland it checks
`hyprctl binds -j`, on Sway it reads your config (`swaymsg -t
get_config`; best effort: `set $var` is expanded, mode blocks count as
taken). What frame bound is recorded in `~/.local/state/frame/binds`, and
`frame unbind` removes exactly that, not whatever the config says now.
If GNOME's shortcut list can't be read, frame changes nothing.

Hyprland and Sway shortcuts only last for the running session; the
tray applies them when it starts (see Quick start).

## Test

```sh
bash tests/smoke.sh
```

Uses `FRAME_DRY_RUN=1`, a temp config and temp XDG dirs, with fake
frame-gui / gsettings / notify-send / systemctl first on `PATH`: no
screenshots, clipboard, notifications or shortcut changes. If cargo is
installed it also runs the GUI's unit tests (`cargo test` in `gui/`),
and if a release build exists it installs into a temp `DESTDIR` and a
temp `PREFIX` (with a stand-in frame and tray) and uninstalls again.

## Licence

MIT. See [LICENSE](LICENSE).
