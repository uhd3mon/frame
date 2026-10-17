# clip

A quick screenshot tool for Linux. It saves to a file and copies to the
clipboard, and it works on all major desktops (KDE, GNOME, Hyprland, Sway,
X11). It is modelled on [Hyprshot](https://github.com/Gustash/hyprshot), which
only supports Hyprland.

**Status:** skeleton. `clip --detect` works. Capture is not built yet.

## Usage (planned)

```sh
clip -m region            # pick an area
clip -m window            # active / picked window
clip -m output            # whole monitor
clip -m region --clipboard-only
clip --detect             # show the backends clip would use
```

Files go to `$CLIP_DIR`, or your XDG Pictures folder.

## Backends

| Desktop       | Capture           | Clipboard     |
|---------------|-------------------|---------------|
| Hyprland/Sway | grim + slurp      | wl-copy       |
| KDE Plasma    | spectacle         | wl-copy/xclip |
| GNOME         | gnome-screenshot  | wl-copy/xclip |
| X11 (other)   | maim / import     | xclip/xsel    |

## Test

```sh
bash tests/smoke.sh
```
