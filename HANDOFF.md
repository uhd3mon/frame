# HANDOFF — clip

## Goal

Background screenshot / replay / recording tool driven by global
shortcuts, one config file, no UI. Saves to `<save_dir>/clip/images` and
`/videos`, copies to the clipboard, works on KDE, GNOME, Hyprland, Sway
and X11. MIT, all our own code (no Hyprshot code).

## Current state (2026-10-17, round 2, uncommitted)

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

1. Sora/Kevin: look at the test toast; decide whether KDE images also
   want `image-path` (now only `x-kde-urls`, like Spectacle).
2. Real-check on KDE (not done in round 2): `clip bind` with a taken
   combo (setShortcut reply check), `clip record` start/stop with real
   gsr, replay save via the daemon, toast buttons.
3. Test on GNOME, Hyprland, Sway, X11 (only dry-run so far).
4. Committed locally after Sora's review (no remote).
5. Kevin: run `clip config`, `clip bind`, `clip service on`, then check a
   toast with Do Not Disturb off. Script is ~1000 lines; trim later if
   wanted.

## Decisions

- Bash, single script, runtime backend choice; never source the config.
- Shortcut state file in XDG_STATE_HOME (KDE/GNOME binds survive
  reboot). Hyprland/Sway unbind leaves a combo alone if it now belongs
  to someone else.
- `systemd_arg` does not double `$`: systemd doesn't expand variables in
  the program path (`systemd-analyze verify` showed `$$` kept literally).
- `CLIP_STOP_WAIT` (seconds, default 10) exists for tests.

## Lessons

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

## Verification observed (2026-10-17, KDE Plasma 6.7.5 Wayland)

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
<!-- workspace:memory:end -->
