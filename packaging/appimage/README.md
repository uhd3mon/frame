# Frame AppImage

`packaging/appimage/build.sh` builds `dist/Frame-x86_64.AppImage`, one file
people download, `chmod +x` and run. `dist/` is ignored by Git.

```sh
packaging/appimage/build.sh            # Docker build (glibc 2.35), then pack
packaging/appimage/build.sh --host     # local cargo (needs the host's glibc!)
packaging/appimage/build.sh --no-build # pack the last binary again
```

Needs: Docker, curl, objdump. The first run builds the image
`frame-appimage-build:<rust>` (~1 min). Cargo's cache and output go to
`target/appimage/`.

## Inputs (pinned in `versions.env`)

| What | Version | sha256 |
|---|---|---|
| appimagetool | 1.9.1 | `ed4ce84f…cb384eb0` |
| type2 runtime (static) | 20251108 | `2fca8b44…47ec260d` |
| build image | ubuntu:22.04 by digest | `076e7bf8…04fb5e11` |
| rustup-init | 1.29.2 | `5be191e8…22ec24f4` |
| Rust | 1.99.0 | (via rustup) |

The sha256 values match GitHub's asset digests. The runtime's GPG
signature checks out with the release's `signing-pubkey.asc` (key
570C77AC…6490F695). build.sh checks each sha256 on every run. apt's
`gcc`/`libc6-dev` in the image are not pinned, but the image digest is.

## What is inside

```
AppRun                 sets FRAME_BIN=$APPIMAGE, execs usr/bin/frame
frame.desktop          (no X-KDE-DBUS-Restricted-Interfaces: see KDE)
frame.png, .DirIcon    256 px icon
usr/bin/frame          the release binary
usr/share/icons/...    hicolor svg + png sizes
usr/share/licenses/frame/LICENSE
```

No libraries are bundled. `ldd` shows only libc, libm and libgcc_s.
Everything else is loaded with dlopen at runtime: libEGL/libGL, libwayland-client
and -egl, libxkbcommon(-x11), libX11, libX11-xcb, libxcb, libXcursor,
libXi and libXrender. Those must come from the host: GL/EGL belong to the
GPU driver, and every desktop session already has the rest. D-Bus is pure Rust (zbus), so
there is no libdbus. The minimum is **glibc 2.35** (Ubuntu 22.04+,
Debian 12+, Fedora 36+). On the host (glibc 2.44) the binary needed 2.43.

The static runtime needs `fusermount`/`fusermount3` on the user's PC, not
libfuse2. Without FUSE (containers, some locked-down PCs):
`./Frame-x86_64.AppImage --appimage-extract-and-run ...`.

## Why FRAME_BIN

The binary runs from `/tmp/.mount_Frame*`, and that path is gone when it
exits. Shortcuts, `_toast`/`_clip` children and the manual-bind hints
must run the `.AppImage` file instead. `Ctx::process` already prefers
`FRAME_BIN`. The cost is about 10 ms of mounting per run (measured: 0.010 s vs 0.001 s
for `--version`).

Autostart (fixed 2026-10-29): `settings.rs` `write_autostart` now uses
`install::stable_exe`: the installed frame if there is one, else
`$APPIMAGE`, never the temporary mount path.

## KDE: KWin ScreenShot2 authorization

KWin up to 6.7 (`src/utils/serviceutils.h`, read at tag v6.7.5) checks the
caller like this: D-Bus sender → pid → `/proc/<pid>/exe` symlink target.
That target must equal the canonical path of the first word of `Exec=` in
some installed .desktop entry that has
`X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2`. The exe of
an AppImage process is `/tmp/.mount_XXXX/usr/bin/frame`, which is random
and different from the `.AppImage` file. So no .desktop entry can match it,
and even Exec=$APPIMAGE does not.

KWin 6.8.2 (Kevin's version) does not run that check: `checkPermissions` is still
declared in the header but no capture method calls it. This was observed on the live session: a
CaptureArea(0,0,0,0) call from python3 got `InvalidArea`, not
`NoAuthorized`, and no pixels were captured. Don't count on this staying the same.

Done (2026-10-29): `frame install` (src/install.rs) and an "Install Frame"
notice in the settings window when run from the AppImage and not installed.
The original recommendation: **an "Install Frame" step** for the AppImage on first
run, through the settings window and `frame install`. It would be a Rust port of install.sh's
per-user path: copy the binary to `~/.local/bin/frame`, write
`frame.desktop` (absolute Exec + the restricted interface), icons and
autostart, then rebind so the shortcuts run `~/.local/bin/frame`. This fixes
KWin ≤ 6.7, gives a stable path for shortcuts and autostart, and adds a
start-menu entry. The xdg-desktop-portal Screenshot API (ashpd is already
in Cargo.lock via rfd) is the better fallback for GNOME, not for KDE.

## External helpers (all replaced, 2026-10-29)

| Helper | Used for | Now |
|---|---|---|
| grim | Hyprland/Sway capture | `src/wl.rs`: ext-image-copy-capture, else wlr-screencopy |
| slurp | Wayland region selection | `src/select.rs`: own layer-shell / fullscreen-window overlay, wl_shm drawing |
| ffmpeg | jpg/webp | `image` 0.25.10 with `jpeg` + `webp` (adds only image-webp 0.2.4) |
| xclip | X11 clipboard | `src/x11.rs`: own CLIPBOARD owner (INCR for big pictures) |
| maim/import, xdotool, spectacle (X11) | X11 capture/window | `src/x11.rs`: GetImage, RandR, `_NET_ACTIVE_WINDOW` + frame extents |
| gnome-screenshot | GNOME capture | `src/portal.rs`: portal Screenshot over zbus (may ask permission once) |
| wl-copy | GNOME clipboard (no data-control) | `src/wlclip.rs`: 1x1 focused window + core wl_data_device |
| hyprctl/swaymsg (capture) | focused window/monitor | `src/ipc.rs`: the IPC sockets directly |
| hyprctl/swaymsg/gsettings (binds) | binds | Kept: shipped by those desktops themselves |
| xdg-open | toast buttons | Portal OpenFile/OpenDirectory first; xdg-open only as a last resort |
