# HANDOFF — Clipped (repo folder still `clip`)

## Goal

Background screenshot / replay / recording tool driven by global
shortcuts, one config file; `bin/clipped` is the engine, `clipped-gui`
(Rust) an optional tray + settings window. Saves to
`<save_dir>/clipped/images` and
`/videos`, copies to the clipboard, works on KDE, GNOME, Hyprland, Sway
and X11. MIT, all our own code (no Hyprshot code).

## Rename to Clipped (2026-10-17, Kevin asked; Atlas did it, Sora checked)

Kevin renamed the app "clip" -> "Clipped" to avoid clashing with other
apps. Everything user-facing moved: `bin/clipped`, crate/binary
`clipped-gui`, `~/.config/clipped/config`, `<save_dir>/clipped/…`,
XDG state/cache/runtime dirs `clipped`, `clipped.service`,
`clipped-<action>.desktop`, kglobalaccel "clipped region" etc.,
`notify-send -a clipped`, `clipped.desktop` (Name=Clipped),
`clipped-gui.desktop` autostart, manifest `share/clipped/installed`,
icons `gui/icons/clipped*`, env vars `CLIPPED_*` (was `CLIP_*`). No
migration code: nothing of the old name existed in Kevin's home (checked
~/.config, ~/Documents, state, cache, applications, systemd user dir,
kglobalshortcutsrc). Kept: the repo folder name `clip` (Workspace
project path), internal Rust names (`ClipTray`, `run_clip`), and the
replay toast "Clip saved (N s)" (a video clip). Older sections below
use the old names; read them as clipped / CLIPPED_.

Sora's check: `git grep -nIiw clip` outside HANDOFF = only the toast
line and its smoke assertion; `git grep CLIP_` = none. `bash -n
bin/clipped` ok; `bash tests/smoke.sh` "95 checks, all passed"; `cargo
test` "29 passed"; dry `--detect` shows images=~/Documents/clipped/images
and did not create it. Atlas also ran clippy (0 warnings), release
build (6,601,768 bytes) and DESTDIR install (13 files incl. manifest) +
uninstall (left: 0). Stale `gui/target/release/clip-gui` is ignored
build output.

GPU encoding (Kevin asked): recording/replay use gpu-screen-recorder
6.1.4 without `-encoder`, so it encodes on the GPU (on AMD/Intel via
VAAPI, NVENC on NVIDIA — from gsr docs). Capture is GPU-side too.
`gpu-screen-recorder --info` here: AMD RX 9070 (card1), codecs h264,
hevc, av1 (+ _hdr/_10bit, _vulkan variants, h264_software); auto picks
av1. Only the X11 ffmpeg fallback (used when gsr is missing) is CPU
(libx264 / libvpx-vp9); a VAAPI ffmpeg path is a possible follow-up.

## Current state (2026-10-17, 0.2.0 = round 3: GUI + Nova's review fixes, committed after Sora's review)

Round 2 is committed (274cd9e). Round 3 adds, not committed yet, the
GUI below plus fixes for Nova's round-3 review (next section).

### Round 3 review fixes (Nova, items 1–10 + desktop entry)

1. Config read errors: `read_config` returns None only for NotFound
   (incl. a dangling symlink). Any other error is shown, Save is off,
   Revert becomes Reload. Save reads the file again with the same rule.
   Parser and writer work on bytes, so a non-UTF-8 config (Latin-1
   comment, odd value) is edited in place, byte for byte; a non-UTF-8
   value is shown lossily and only written if edited.
2. `load_config` and `parse_shortcut` run with `local LC_ALL=C`, so
   `[[:space:]]` and `${x,,}` are ASCII only. Rust follows: ASCII
   `is_sp`, `to_ascii_lowercase` (Kelvin sign K is not k). U+3000
   before # is part of the value for both; `unrepresentable` no longer
   refuses it.
3. install.sh writes `$PREFIX/share/clip/installed` and `--uninstall`
   removes only the files it lists (plain relative paths; `..` or odd
   lines skipped with a warning); no manifest = nothing removed, exit 1.
   Install refuses to replace a bin/clip or bin/clip-gui not in the
   manifest unless `--force` (Sora asked for "warn"; a non-interactive
   script can't ask, so it stops and says how to go on). Real uninstall
   (no DESTDIR): TERM every process whose /proc/PID/exe is our clip-gui
   (waits 3 s, never KILLs), then `clip service off` and `clip unbind`
   (failures only warn), then removes the autostart entry only if its
   Exec= is our clip-gui (PREFIX path or its readlink -f).
4. GUI rebind runs only `clip bind` (it unbinds the saved state
   itself); `rebinding` disables Save and shows a spinner until it
   reports. bash: `with_bind_lock` flocks `$RUNDIR/bind.lock` around
   `bind_now` / `unbind_now` (wait `CLIP_BIND_WAIT`, default 30 s) and
   closes the fd afterwards, so the daemon doesn't hold it.
5. `resolve_link` follows symlinks with read_link (relative to the
   link's dir); a dangling link's target is created, the link stays.
6. Temp file: `create_new` + mode (config's mode, else 0644 and umask),
   explicit chmod for the umask case, `sync_all`, rename, dir fsync. A
   planted temp name is skipped (tries -0 … -99).
7. Edited lines keep a trailing `\r`.
8. save() collects errors: a failed autostart write after the config
   was written is reported, but reload and rebind still happen;
   `Msg::Rebind` replaces "Updating shortcuts…" in the status (or
   appends) instead of replacing the whole status.
9. `config::valid_value` mirrors validate_config per key; combos,
   checkboxes and numbers show "config has 'fast'" in red when the
   file's value is refused (see `shots/gui-settings-invalid.png`, its
   own frame). save_dir, video_source, audio and bind.* are trimmed.
10. GUI tests hold `testutil::env_lock()` and call `test_env`
    (CLIP_DRY_RUN=1, temp XDG dirs, XDG_CURRENT_DESKTOP=clip-test, so
    `clip bind` is the "manual" backend). `TempDir` removes itself on
    drop, also after a panic or chmod 000. The 3 stale
    /tmp/clip-gui-test-* dirs were deleted.
- clip.desktop: `Categories=Utility;` (Graphics dropped);
  desktop-file-validate now prints nothing. Exec= stays the absolute
  quoted path.

- `bin/clip check`: load_config (which validates) then prints `ok`; a
  bad config exits 2 with the message. 3 smoke checks.
- `gui/` crate `clip-gui` 0.1.0 (MIT, edition 2021): eframe 0.33.3
  (glow, wayland+x11, no persistence), ksni 0.3.6 (tokio + blocking),
  png 0.18, rfd 0.15 (xdg-portal + tokio), libc. ~1700 lines incl. tests.
  `Cargo.lock` is generated; commit it with the crate.
  - `main.rs`: modes, flock locks (`File::try_lock`, close-on-exec so
    clip/gsr never inherit them) on RUNDIR/gui.lock and settings.lock,
    `clip_bin()` lookup ($CLIP_BIN, sibling, ../../../bin/clip, PATH),
    detached spawns (own process group, a thread reaps).
  - `state.rs`: RUNDIR/config/state paths (same rules as bin/clip),
    `proc_start` (field 22 after the last ") "), `live_pid`, Status +
    tooltip text.
  - `config.rs`: DEFAULTS, bash-exact parser, `apply()` writer (keeps
    comments/order/unknown keys, keeps a trailing comment at its column,
    rewrites every duplicate of a key, appends missing keys in default
    order, quotes only when needed), `unrepresentable()` (control chars,
    whitespace followed by #), `valid_shortcut()` = parse_shortcut.
  - `tray.rs`: SNI tray; polls pid files every 1 s, `systemctl --user
    is-active` every 5 s; menu as specified; left click = settings.
  - `settings.rs`: one-page egui window. Save = local checks → base text
    (file, or `clip config` run on a temp CLIP_CONFIG) → apply → temp
    file → `clip check` → rename (symlinked config: writes the target).
    Binds changed → `clip bind` in a thread if the daemon
    runs or the binds state file exists, else "Shortcuts apply when clip
    starts". Daemon running + video/save keys changed → "Restart
    service" button (only if clip.service is active). Autostart file
    written/removed on Save. `CLIP_GUI_SHOT=f.png` saves the window's
    own frame and quits.
  - `icons.rs`: Strata's PNGs from gui/icons/png via include_bytes:
    clip-{idle,replay,recording}-{16,22,24,32,48,64}.png (tray) and
    clip-64.png (window icon).
- `install.sh` [--no-build | --force | --uninstall], PREFIX (default
  ~/.local), DESTDIR. Installs 12 files + manifest: bin/clip, bin/clip-gui,
  share/applications/clip.desktop, hicolor scalable clip.svg and
  clip.png at 16–256. Refreshes desktop/icon caches only without
  DESTDIR.
- smoke.sh: +3 `clip check`, +`cargo test` (if cargo), +install into
  DESTDIR and uninstall (if a release build exists) = 89; review fixes
  add U+3000/Kelvin, 2 bind.lock, 3 install (live uninstall ×2,
  foreign bin/clip) = 95.
- `.gitignore`: gui/target/; Strata had already re-included
  gui/icons/png/*.png (preview.png stays ignored).

### Round 2 (committed 274cd9e)

- `bin/clip` (~1000 lines Bash): region/window/monitor/all screenshots,
  replay buffer (gpu-screen-recorder in the daemon, SIGUSR1 to save),
  record toggle, daemon, bind/unbind for KDE/GNOME/Hyprland/Sway,
  systemd user service, status, `--detect`.
- Round 2 fixed everything in Nova's v0.1 review (items 1–20):
  - GNOME never writes the shortcut list when `gsettings get` fails or
    isn't a list.
  - pid files are `PID STARTTIME` (from /proc), so a stale file can't
    signal a reused PID.
  - `flock` on record.lock and daemon.lock.
  - Every `bind.*` is validated as modifiers plus one known key
    (closes the Hyprland/Sway exec injection).
  - Paths with spaces are quoted for KDE Exec=, systemd, GNOME,
    Hyprland and Sway.
  - Config: quotes, `#` only after a space, only `~` expanded, absolute
    save_dir, ranges checked.
  - file:// URIs are encoded per byte.
  - Bind state lives in `$XDG_STATE_HOME/clip/binds`; unbind uses it.
    clip skips combos already taken on Hyprland, Sway and KDE.
  - Record stop: INT, then TERM, then keep the pid file and exit 1.
  - ffmpeg webm path is VP9 + Opus.
  - The /tmp RUNDIR fallback is checked.
  - Temp captures go in `mktemp -d` with a cleanup trap; the daemon's
    TERM handler also kills its sleep.
- New: toast notifications (`toast`, `_toast`).
  - Title "Screenshot saved" / "Clip saved (N s)" / "Recording saved";
    body is the file name plus "Copied to clipboard".
  - KDE gets `x-kde-urls`; others get `image-path`. Videos get an
    ffmpeg frame in `~/.cache/clip/thumbs` (pruned after 7 days), also
    on KDE, because Plasma has no video thumbnailer without ffmpegthumbs.
  - "Open" / "Show in folder" buttons (FileManager1.ShowItems, else
    xdg-open of the folder). It runs detached (`setsid -f clip _toast`)
    under `timeout 30m`.
  - Errors go through `fail()`, which sends a critical notification.

## Next steps

0. GUI (round 3), not verified yet: pressing keys in **Record** (needs a
   person at the keyboard), the Browse portal dialog, Save on the real
   config with a live daemon (rebind / Restart service), tray menu
   clicks, the start-menu entry after a real `./install.sh`, and how
   the icons look in the Plasma panel. Nothing was installed into the
   real ~/.local; Kevin decides when. A real `./install.sh --uninstall`
   (it runs service off / unbind / stops the tray) was only run against
   a temp PREFIX with a stand-in clip and tray.
   Settled by Sora: Categories=Utility only; Exec= keeps the absolute
   path (~/.local/bin is often not on PATH for launchers). egui can't
   see Meta on Linux, so Record has an "add Meta" box (on by default).
1. Sora/Kevin: look at the test toast; decide whether KDE images also
   want `image-path` (now only `x-kde-urls`, like Spectacle).
2. Real-check on KDE (not done in round 2): `clip bind` with a taken
   combo (setShortcut reply check), `clip record` start/stop with real
   gsr, replay save via the daemon, toast buttons.
3. Test on GNOME, Hyprland, Sway, X11 (only dry-run so far).
4. Committed locally after Sora's review (no remote).
5. Kevin: `./install.sh` (puts clip + clip-gui in ~/.local/bin, Clip in
   the start menu), open Clip, tick "Start tray at login", then
   `clip service on`; check a toast with Do Not Disturb off. Script is ~1000 lines; trim later if
   wanted.

## Decisions

- GUI = separate Rust binary; bin/clip stays the engine (Kevin,
  2026-10-17). Tray and settings window are separate processes (no
  winit/tray main-thread fights); the autostart entry is written by the
  GUI checkbox, not by install.sh.
- Empty values are written as `key =` (no trailing space).

- Bash, single script, runtime backend choice; never source the config.
- Shortcut state file in XDG_STATE_HOME (KDE/GNOME binds survive
  reboot). Hyprland/Sway unbind leaves a combo alone if it now belongs
  to someone else.
- `systemd_arg` does not double `$`: systemd doesn't expand variables in
  the program path (`systemd-analyze verify` showed `$$` kept literally).
- `CLIP_STOP_WAIT` (seconds, default 10) exists for tests.

## Lessons

- `set -e` exits when a `for` loop's last body command is a failing
  `[[ … ]] && x` (the loop returns 1); use `if` in loops of set -e
  scripts.
- A temp file that copies the config's mode copies 000 too, so in the
  EACCES case `clip check` fails and hides the template-overwrite bug;
  the data loss shows only for a readable file the reader rejects
  (non-UTF-8 → `read_to_string` InvalidData). Test that case.
- `kill -0` succeeds on a zombie: a test's fake tray must not be the
  test shell's child (start it in a `( … & )` subshell).
- Rust tests share one process: tests that set env vars need a lock.

- egui-winit 0.33 drops Super/Meta from `Modifiers` on Linux (only
  macOS gets it as mac_cmd), so a Meta+… combo can't be captured.
- egui's default font has no `▾`; `⏷` (emoji font) renders.
- jq `//` treats `false` as missing: `.enabled.data // true` printed
  true for a disabled dbusmenu item. Test with `== null`.
- A temp `XDG_RUNTIME_DIR` breaks `WAYLAND_DISPLAY=wayland-0` and
  `systemctl --user`; use an absolute WAYLAND_DISPLAY, and the tray
  silences systemctl's stderr.

- On KDE Wayland, `grim` fails ("compositor doesn't support the screen
  capture protocol"); use `spectacle -b -n`.
- Bash background jobs start with SIGINT ignored, and a bash script
  can't re-trap it. The fake recorder in tests is Python for that reason.
  Real gsr/ffmpeg install their own handlers.
- Bash 5.2+ `patsub_replacement`: quote replacement strings that hold
  `&` or `\` (`${s//&/"&amp;"}`).
- `notify-send … | grep -q` under pipefail can fail (SIGPIPE); grep a
  here-string instead.
- GLib refuses a desktop entry whose program path contains `%%` (it
  checks the binary before expanding field codes); space and `$` work.
- Plasma 6.7.5 notification server: actions, x-kde-urls, body-images,
  persistence. `notify-send --wait` did not return when the popup
  timed out (see verification).
- Dev machine has grim, slurp, spectacle, wl-copy, notify-send 0.8.8,
  jq, ffmpeg, flock, gpu-screen-recorder, desktop-file-validate,
  python3; it lacks hyprctl, xclip, shellcheck, bats, man, ffmpegthumbs.

## Verification observed, Sora's check (2026-10-17 ~20:00, KDE Wayland)

- `bash -n bin/clip install.sh` ok; `bash tests/smoke.sh` → "95 checks,
  all passed"; `cargo test` → "29 passed; 0 failed"; release binary
  6,601,592 bytes.
- Live tray on temp XDG dirs: registered as
  `org.kde.StatusNotifierItem-<pid>-1`; ToolTip "clip: not running" →
  with a fake live record.pid "clip: recording to x_record.mp4" → after
  killing that pid "clip: not running". Tray stopped (TERM, rc 143).
- Real ~/.local/bin/clip*, ~/.config/clip/config and autostart entry
  still absent (nothing installed for real).

## Verification observed, round 3 review fixes (2026-10-17, KDE Plasma 6.7.5 Wayland)

- Before the fix, U+3000 before #: `LC_ALL=en_US.UTF-8` bash read
  `audio = x\u3000#c` as `[x]`, `LC_ALL=C` as `[x　#c]` (Nova was right).
- `cd gui && cargo test`: "test result: ok. 29 passed; 0 failed".
  `cargo clippy --all-targets`: no warnings. `cargo build --release`:
  ok (22.9 s incremental), 6,601,592 bytes.
- Mutation checks for #1 (each in a scratch edit, then restored and
  29/29 again):
  - save() re-read back to "any error → template":
    `unreadable_config_is_never_replaced` fails ("Not saved: … line 120:
    …tmp: Permission denied" instead of "Can't read": the copied 000
    mode stopped the overwrite, see Lessons).
  - `read_config` maps every error to "no file":
    `unreadable_config_is_never_replaced` fails at the load check.
  - round-3 reader (`read_to_string`, any error = no file):
    `non_utf8_config_is_edited_in_place` fails with left = "# clip
    config: key = value. …" (the template) vs right = "# caf\u{fffd}\r\n
    video_fps = 30 …": the user's file was replaced (the reported loss).
- Extra bash mutants (scratch copies): without `local LC_ALL=C` the
  U+3000 check fails; with cmd_bind not locking, the bind.lock check
  fails.
- `bash -n bin/clip`, `bash -n install.sh` ok. `bash tests/smoke.sh`:
  "95 checks, all passed" (~14 s).
- `clip-gui --settings` with a temp config holding `video_fps = fast`,
  `notify = maybe`, `image_format = PNG`: rc 0, config md5 unchanged
  (b828b4d6…), frame in `shots/gui-settings-invalid.png` shows the three
  red "config has '…'" notes.
- `ls -d /tmp/clip-gui-test-*` after the runs: 0.
- Not run: anything against Kevin's real config, shortcuts, service,
  tray or ~/.local.

## Verification observed, round 3 (2026-10-17, KDE Plasma 6.7.5 Wayland)

- `cargo build --release --offline`: ok (~35 s clean, ~23 s
  incremental). Binary 6,590,448 bytes, stripped; links only libc,
  libm, libgcc_s (wayland/x11/GL are dlopened).
- `cargo test`: "test result: ok. 19 passed". `cargo clippy`: 0
  warnings. Mutation: with quoting turned off in `format_value`,
  `tricky_values_round_trip` and `bash_reads_what_we_write` fail (bash
  read back " lead" wrong).
- `bash -n bin/clip` ok. `bash tests/smoke.sh`: "89 checks, all passed"
  (~10 s).
- `DESTDIR=$(mktemp -d) ./install.sh --no-build`: 12 files;
  desktop-file-validate passes with one hint (two main categories).
  `--uninstall` removed 12, 0 left. PREFIX "/opt/my clip" gives
  `Exec="/opt/my clip/bin/clip-gui" --open`, validates. Real
  ~/.local/bin/clip-gui and ~/.local/share/applications/clip.desktop:
  not present (untouched).
- Live tray (temp XDG_RUNTIME_DIR/CONFIG/STATE, real session bus):
  registered as `org.kde.StatusNotifierItem-<pid>-1` in
  RegisteredStatusNotifierItems; Category ApplicationStatus; pixmaps
  16,22,24,32,48,64. With fake pid files naming a live `sleep`:
  tooltip "clip: not running" → "clip: recording to
  2026-10-17_20-00-00_record.mp4" → "clip: replay buffer 45 s" →
  "clip: not running" for a daemon.pid with the wrong start time. Centre
  pixel of the 64 px icon: grey (158,163,168) / red (237,28,36) / teal
  (31,191,159). dbusmenu: Save replay `enabled=false` when idle, enabled
  with replay; label flips Start/Stop recording; Background service
  toggle-state 0. A second `clip-gui` / `clip-gui --open` exits 0 and
  no second item registers. After kill the item is gone.
- Settings window (`--settings`, temp config with comments + unknown
  key): opens on Wayland via glow, exit 0, config md5 unchanged.
  `shots/gui-settings.png` (untracked) is its own frame.
- Not run: anything against Kevin's real config, shortcuts, service or
  ~/.local.

## Verification observed, round 2 (2026-10-17, KDE Plasma 6.7.5 Wayland)

- `bash -n bin/clip`: ok. `bash tests/smoke.sh`: 84 checks, all passed
  (~10 s).
- Mutation check: with the v0.1 GNOME read, pid check or bind validation
  put back in a temp copy, the GNOME, stale-pid and injection tests fail.
- `gio launch` of a temp desktop entry written by `desktop_arg` for
  `…/my $dir/clip` ran it with argv `…/my $dir/clip | region`.
  `desktop-file-validate` ok. `systemd-analyze --user verify` ok for
  `…/my $dir/clip` and `…/my $dir%x/clip`.
- One real toast at 18:49:32 with a /tmp test-pattern PNG through
  `toast` (args `-a clip -i camera-photo -h string:x-kde-urls:file://…
  -A open=Open -A "folder=Show in folder" "Screenshot saved" "<name>\n
  Copied to clipboard"`). notify-send accepted it and blocked. It printed
  nothing and hadn't returned when my 120 s `timeout` killed it (rc
  124). So on Plasma, `--wait` outlives the popup: each toast keeps a
  detached waiter alive until a click, dismissal or the 30 min cap.
  How it looked is still to be checked by Sora/Kevin.
- Sora re-check (19:00): `bash tests/smoke.sh` → 84 passed. Real
  `CLIP_CONFIG=/tmp/clipcheck/config bin/clip monitor` → rc 0, PNG in
  /tmp/clipcheck/clip/images, `wl-paste -l` → image/png, toast process
  started. The popup did NOT show: Plasma Do Not Disturb was on
  (Notifications `Inhibited` = true), so the look is still unchecked.
- Default keys Meta+Alt+R/W/M/A/C/V: no matches in kglobalshortcutsrc.
  Not bound for real yet; the daemon/replay buffer was not started
  (waits for Kevin). Sora's clip-probe test shortcut was removed.

## Team memory

<!-- workspace:memory:start -->
- clip layout (2026-10-17): /home/kevin/Workspace/clip, local git (main, b564813). bin/clip = single Bash script (skeleton: --help/--version/--detect; capture not built), tests/smoke.sh (7 checks, no real screenshots), README/AGENTS/CLAUDE(@AGENTS.md)/HANDOFF. Checks: bash -n bin/clip; bash tests/smoke.sh. grim fails on KDE Wayland, so KDE uses spectacle -b -n. Licence undecided (GPL-3.0 if Hyprshot code reused) - ask Kevin. (Sora, 2026-10-17)
- clip v0.1 built 2026-10-17 by Atlas (uncommitted, for Sora's review): bin/clip ~700 lines Bash, MIT, own code. CLIP_DRY_RUN=1 + temp CLIP_CONFIG/XDG dirs for tests (31 checks). Real-checked on KDE: spectacle all/monitor, webp via ffmpeg, gsr record (av1 2560x1440@60), daemon replay + -sc hook rename, TERM exit, KDE bind/unbind via kglobalaccel. Dry-run used to mkdir the real ~/Documents/clip, so tests must set save_dir to temp. (Atlas, 2026-10-17)
- clip round 2 (2026-10-17, Atlas, uncommitted): all 20 Nova review items fixed + toast notifications (KDE x-kde-urls, others image-path, ffmpeg video thumbs in ~/.cache/clip/thumbs, Open/Show-in-folder actions via detached `clip _toast`). Bind state in $XDG_STATE_HOME/clip/binds; unbind uses it. smoke.sh = 84 checks with fake gsr(python)/gsettings/hyprctl/swaymsg/notify-send on PATH; real notify/clipboard calls are tripwired. systemd ExecStart program path must not double $ (systemd doesn't expand there). (Atlas, 2026-10-17)
- clip 0.1.0 committed 2026-10-17 (274cd9e, local). Kevin's decisions: MIT, own code only (no Hyprshot code), no UI, one config ~/.config/clip/config, saves to <save_dir>/clip/{images,videos} with save_dir default ~/Documents, png/mp4 default. Video = gpu-screen-recorder replay buffer in RAM (60 s if >=8 GiB free, else 30). KDE binds via kglobalaccel D-Bus + ~/.local/share/applications/clip-<action>.desktop. Toasts with preview (Kevin asked). Default keys Meta+Alt+R/W/M/A/C/V not yet bound for real; daemon not started. (Sora, 2026-10-17)
- clip round 3 (2026-10-17, Atlas, uncommitted for Sora's review): Rust GUI in gui/ (clip-gui 0.1.0: eframe 0.33.3 glow + ksni 0.3.6 tray + png + rfd), tray and settings are separate processes with flock gui.lock/settings.lock in RUNDIR. bin/clip got `clip check`. install.sh (PREFIX, DESTDIR, --no-build, --uninstall; 12 files). smoke.sh = 89 checks, cargo test = 19. egui can't see Meta on Linux, so shortcut Record has an "add Meta" box. CLIP_GUI_SHOT=f.png makes the settings window save its own frame. Nothing installed into real ~/.local yet. (Atlas, 2026-10-17)
- clip round 3 review fixes (2026-10-17, Atlas, uncommitted): all of Nova's 10 items are fixed. The GUI config parser/writer now works on bytes (lossless for non-UTF-8). Only NotFound counts as "no config"; any other read error turns Save off. bash load_config/parse_shortcut use local LC_ALL=C. bind/unbind are flocked on $RUNDIR/bind.lock. install.sh writes a manifest at $PREFIX/share/clip/installed; uninstall removes only what it lists and, without DESTDIR, stops clip-gui, runs service off + unbind and removes our autostart entry. Install refuses a foreign bin/clip unless --force. smoke = 95 checks, cargo test = 29. GUI tests use testutil::env_lock + test_env (dry run). Never run a real-mode install.sh --uninstall in tests without a fake bin/clip. (Atlas, 2026-10-17)
- clip 0.2.0 committed 2026-10-17 (884e8d6, local): Rust clip-gui in gui/ (eframe settings window + ksni tray showing idle/replay/recording from RUNDIR pid files), install.sh (PREFIX/DESTDIR, manifest, --uninstall, --force), Clip start-menu entry (Exec=clip-gui --open), "Start tray at login" checkbox writes autostart. GUI validates via `clip check` before atomic rename. Checks: smoke 95, cargo test 29. Not installed for real yet; Kevin runs ./install.sh. (Sora, 2026-10-18)
- clip renamed to clipped (2026-10-17, Atlas, uncommitted for Sora's review): bin/clipped, clipped-gui crate/binary, gui/icons/clipped*.svg/png, config ~/.config/clipped/config, <save_dir>/clipped/{images,videos}, XDG dirs .../clipped, clipped.service, KDE clipped-<action>.desktop, notify -a clipped, Name=Clipped, manifest share/clipped/installed, env CLIPPED_* (DRY_RUN, CONFIG, BIN, GUI_SHOT, BIND_WAIT, STOP_WAIT). Repo folder stays /home/kevin/Workspace/clip. Window/tray titles "Clipped"; "Clip saved" toast (video clip) kept. Checks after rename: smoke 95 pass, cargo test 29 pass, clippy clean. (Atlas, 2026-10-18)
<!-- workspace:memory:end -->
