# HANDOFF — Frame (formerly Clipped; repo folder `frame`, was `clip`)

## Goal

Screenshot tool driven by global shortcuts, one config file; `bin/frame`
is the engine, `frame-gui` (Rust) the tray + settings window. Saves to
`<save_dir>/frame/images`, copies to the clipboard, works on KDE, GNOME,
Hyprland, Sway and X11. MIT, all our own code (no Hyprshot code).
Pictures only since 0.4.0 (Kevin, 2026-10-29). Sections below the 0.4.0
one are history and use old names (clip, clipped) and video features.

## Frame 0.4.0: pictures only (2026-10-29, uncommitted)

Kevin asked to remove every video feature. Removed: replay buffer,
recording, gpu-screen-recorder, `frame daemon` / `service` / `monitors`
/ `replay` / `record` / `_saved`, gui/src/daemon.rs (the tray no longer
supervises anything; it runs `frame bind` once at start). Kept for
upgrades: bin/frame ignores 0.3's video keys silently (`OLD_KEYS`, other
unknown keys still warn) and bind/unbind remove 0.3's replay/record
shortcuts (`OLD_ACTIONS`); a real install/uninstall turns off and
removes 0.3's frame.service (systemctl --user) and the
frame-replay/record .desktop entries. Strata is redoing the settings
window (settings.rs, theme.rs) in parallel.

- tests/smoke.sh rewritten for pictures only (Atlas): video, daemon and
  service checks removed; new checks: removed commands exit 2, default
  config has only picture keys, old video keys ignored silently,
  unknown key still warned, bind/unbind drop 0.3 replay/record (KDE
  state + leftover .desktop, Hyprland, GNOME frame-replay path), status
  tray pid (live vs reused PID), install.sh --help, install removes
  0.3's service/shortcuts (fake systemctl; DESTDIR leaves them). A fake
  systemctl is always first on PATH.
- The 3 old install/uninstall failures were test expectations (no more
  `service off` on uninstall; autostart Comment is now "Frame tray:
  screenshot shortcuts", same as settings.rs). Real bug fixed:
  `install.sh --help` printed only lines 2-20 and cut the header; it
  now prints the whole comment block.
- README and AGENTS.md describe a screenshot-only tool.
- Checks (Atlas, 2026-10-29): `bash -n bin/frame` ok; smoke "100
  checks, all passed"; cargo test "46 passed; 0 failed"; clippy
  --all-targets 0 warnings. Mutation check: breaking OLD_KEYS,
  OLD_ACTIONS or the --help fix makes the matching smoke checks fail.
- Nova review fixes (Atlas, 2026-10-29): a live install now stops the
  running Frame before copying: frame-gui by /proc exe (also
  "PATH (deleted)", a tray left from an earlier install) and 0.3's
  `frame daemon` / recorders from $RUNDIR daemon.pid, record.pid,
  replay.pid (pid + start time must match; INT for recorders, TERM after
  10 s; TERM for the daemon). Afterwards it starts the new tray
  (`setsid -f`, no output) if tray.pid was live or our autostart entry is
  on; with no WAYLAND_DISPLAY/DISPLAY it prints a note instead. Our
  autostart entry gets today's Comment. KDE bind/unbind always unbind
  replay/record (install deletes their .desktop files first). `frame
  status` lists only picture shortcuts.
- smoke install checks now run install.sh from a copied source tree
  ($T/src) whose "built" frame-gui only logs its start, so no install in
  a test can start the real tray (and they no longer need a release
  build). Background sleeps ignore INT, so the fake recorder is python.
- Checks: bash -n ok (3 scripts); smoke "103 checks, all passed"; cargo
  test "46 passed"; clippy --all-targets 0 warnings; no test processes
  left running.
- Settings window v2 (Strata): design/theme-v2.rs moved into
  gui/src/theme.rs (v1 card/row/toggle_label/keycaps removed), settings.rs
  rebuilt to design/gui.md: 480x700, Shortcuts glyph rows open the editor
  listening, "Turn on" notice, Save to (Open/Change…, not typeable),
  PNG|JPG|WebP segmented, Behaviour toggles, editor 380 wide with Clear
  shortcut. Modal fade-in disabled (egui's ~80 ms fade made the dialog
  look see-through in FRAME_GUI_SHOT frames). Shots:
  shots/frame-settings-v2*.png, frame-shortcut-editor-v2.png (gitignored).
  FRAME_GUI_SHOT works from an agent shell with
  WAYLAND_DISPLAY=/run/user/1000/wayland-1.

### Sora's check + real install (2026-10-29 ~13:15)

- Re-ran: bash -n bin/frame + install.sh ok; smoke "103 checks, all
  passed"; cargo test "46 passed"; clippy clean.
- `./install.sh` (real): stopped the 0.3 tray (pid 2455); its daemon and
  gsr went with it (pgrep finds no frame/gpu-screen-recorder after);
  removed frame-replay/record.desktop; updated autostart. The agent shell
  has no WAYLAND_DISPLAY, so install printed the "starts at next login"
  note; Sora started the tray with WAYLAND_DISPLAY=wayland-1 setsid -f.
  `frame status`: shortcuts region window monitor all, tray running.
- kglobalaccel: frame_{region,window,monitor,all}_desktop isActive true;
  frame_replay/record objects no longer exist.
- Real grabs with the installed build into scratch (clipboard/notify
  off): monitor 2560x1440, all 2560x2880; Spectacle didn't run.
- Not tested: real key presses (shortcut fires, Record dialog), region
  drag, tray menu clicks, the settings window opened from the start menu.
- Workspace's project folder is still ~/Workspace/clip, which only holds
  a stale HANDOFF.md + tmp/; the repo is ~/Workspace/frame. Kevin should
  point the Workspace project at ~/Workspace/frame.

## Published on GitHub (2026-10-18)

- Public repo https://github.com/uhd3mon/frame (MIT, detected by
  GitHub), branch `main`, remote `origin`. The gh login is `uhd3mon`
  (LICENSE copyright says KevinEightSeven).
- Kevin's rule: no Claude contributors or commit references. History was
  rewritten before the first push to drop every `Co-Authored-By: Claude`
  trailer (all commits were affected; hashes changed, e.g. 9fc9a92 ->
  ba1c9d9). Never add AI co-author trailers to commits or PRs here.
- CLAUDE.md is gitignored and removed from history; it stays locally
  (content `@AGENTS.md`) for Workspace. Pre-rewrite backup bundle was
  in the scratch tmp/ only.
- Checks before push: bash -n ok; smoke "114 checks, all passed";
  cargo test "55 passed". After push: repo PUBLIC, 0 commit messages
  matching claude/anthropic/co-authored, only contributor uhd3mon.
- README screenshots (shots/*.png) are gitignored, so none are on GitHub.

## Frame 0.3.0: Sora's check + real install (2026-10-17 ~23:58)

Kevin asked: modern GUI, start at login with all settings, replay
buffer always on while Frame runs, rename Clipped -> Frame, blue icon,
rename the repo folder to `frame`. Atlas: rename + tray-owned daemon
(daemon.rs) + Nova's engine/install fixes. Strata: blue icons,
theme.rs, settings restyle (design/gui.md, design/settings-mockup.png)
+ Nova's settings fixes. Nova: review (no blockers; all items fixed,
see sections below). Atlas's call, approved: the tray always runs
`frame daemon` (shortcuts), replay = false just drops the buffer.

- Checks (Sora): `bash -n bin/frame` ok; smoke "114 checks, all
  passed"; cargo test "55 passed; 0 failed"; clippy 0 warnings.
- `./install.sh` (real): removed the old Clipped install (tray, unbind,
  its 12 files, autostart), installed 12 Frame files + manifest, wrote
  ~/.config/autostart/frame-gui.desktop. Installed bins cmp-equal.
  frame.desktop has absolute Exec + the X-KDE ScreenShot2 key.
- Started the tray: `frame daemon` + gsr `-w DP-2 … -r 60 -replay-storage
  ram` running; `frame status` = replay running, 60s buffer;
  ~/.config/frame/config copied from clipped (video_source = DP-2,
  Kevin's 6 binds). kglobalaccel frame_<a>_desktop isActive = true for
  all 6. Tray ToolTip "Frame: replay buffer 60 s".
- Real `frame monitor` (temp config, clipboard/notify off): rc 0,
  2560x1440 PNG, no spectacle.
- SIGTERM tray -> tray, daemon and gsr all gone within 12 s; started
  again -> daemon + 60 s replay back.
- `~/.config/frame`, `~/.local/share/frame` already held leftover files
  of a Godot-based app (Sep 16); no name clash, left alone.
- Not tested: a real login (autostart), real key presses (Record
  dialog Tab/Enter/Esc, shortcuts), a real region drag, the tray menu
  clicked in the panel. Old captures stay in ~/Documents/clipped.

## Nova's review of the Frame rename: engine/install items (2026-10-17, Atlas, uncommitted for Sora's review)

Strata did settings.rs in parallel; this round didn't touch
settings.rs or theme.rs.

- P1 install.sh `remove_clipped`: a real install (no DESTDIR) that finds
  `$ROOT/share/clipped/installed` runs after the build and before it
  copies anything. It runs `stop_gui clipped-gui` (matched by exe), then
  the old `bin/clipped service off` and `unbind` (only if the old
  manifest lists bin/clipped and it is executable). Then it removes
  `autostart/clipped-gui.desktop` if it starts that clipped-gui, and the
  listed files plus the manifest. `~/.config/clipped` and
  `<save_dir>/clipped` are never touched. `stop_gui NAME`,
  `remove_autostart FILE NAME` and `remove_listed MANIFEST` are now
  shared with uninstall.
- P2 `frame daemon`: daemon.pid and the TERM/INT trap are set right after
  the flock, before `cmd_bind` (not in dry run, as before). Lock busy now
  exits 3. The unit gets `RestartPreventExitStatus=3`. The tray's
  Supervisor already treats an exit with a live foreign daemon.pid as
  External, so it needed no change.
- P2 decision: `frame service on` **refuses** (exit 1) while tray.pid is
  live: "the Frame tray (frame-gui, pid N) already runs the daemon; quit
  it first, or keep using the tray". A warning alone would leave a
  service daemon that loses the lock (or takes over after a tray
  restart). `service off` is always allowed.
- P3 daemon.rs: `seen`/`started_with` are recorded after `frame check`
  (`remember_config`), so a config that the check's migration just
  copied doesn't trigger a restart on the first tick. The External path
  still records before returning. New test
  `config_copied_by_check_is_not_a_change` (the fake `check` copies
  MIGRATE to a missing config).
- P3 PDEATHSIG race: the parent pid is taken before spawn; after prctl,
  pre_exec `_exit(0)`s if getppid() differs. The Supervisor lives on the
  tray's main thread, so PDEATHSIG's "parent thread exits" rule doesn't
  fire early.
- P3 uninstall/stop_gui waits 12 s (120 × 0.1 s), up from 3 s.
- P4 README: old captures stay in `<save_dir>/clipped`, the config copy
  leaves the old file, the Clipped upgrade path, and service vs. tray.
- smoke.sh +5 checks: pid + trap before bind (a fake slow hyprctl sees
  daemon.pid; TERM during bind stops it before the idle loop), lock busy
  = 3 (`flock -n` holds daemon.lock), unit has RestartPreventExitStatus=3
  (fake systemctl, real write in temp XDG_CONFIG_HOME), service on
  refused with a live tray.pid, and install over a fake old Clipped
  (temp PREFIX; logger bin/clipped, sleep copy as clipped-gui; DESTDIR
  install leaves it alone).
- Lesson: fakes on the `fakesleep` PATH that call `sleep` themselves get
  logged as daemon sleeps; use /usr/bin/sleep inside fakes.

Checks run: `bash -n bin/frame` ok; `bash tests/smoke.sh` twice, "114
checks, all passed" (once before the fix above: 1 FAIL, the test's own
sleep); `cargo test` 6 runs, all "test result: ok. 55 passed" (52 + my
1 + 2 more in the tree, which Strata is also editing); `cargo clippy --all-targets`
0 warnings (after touching main.rs to force a recheck). Nothing
installed, nothing committed; Kevin's running Clipped not touched.

## Flaky supervisor test fixed (2026-10-17, Atlas, uncommitted for Sora's review)

`daemon::tests::starts_restarts_with_backoff_and_stops` failed about
1 run in 13 in parallel with `Retry { why: "cannot run …/frame: Text
file busy (os error 26)" }`. Cause: another test thread forked while
`fake_frame` still had the script open for writing. The forked child
holds that write fd until it execs, so exec of the script got ETXTBSY.
Writing to a temp name and renaming would not help, because the inode
stays the same. Fix (only in `daemon.rs` tests): `wait_execable` runs
the new script (no args, exits 0) and retries on ETXTBSY for up to
~1 s. Our own fd is already closed, so after one successful exec no
later fork can bring the error back. The 369 assert now prints the view.
The real tray never writes its `frame` binary, so it can't hit this
itself. If install.sh is replacing bin/frame at that moment, the spawn
fails and the normal 2 s backoff retries it.
Checks: 60 consecutive `cargo test` runs (20 + 40), all "52 passed";
`cargo clippy --all-targets` 0 warnings.

## Rename to Frame + tray owns the daemon (2026-10-17, Atlas, uncommitted for Sora's review)

Kevin: rename Clipped → Frame; Frame starts at login with all settings,
and the replay buffer runs whenever Frame runs. Strata did the icons
(`gui/icons/frame*.svg`, `png/frame-*.png`) and `gui/src/theme.rs`
(not wired into main.rs yet); Atlas did the rest. Version 0.3.0.

- Rename like clip→clipped: `bin/frame`, `frame-gui`, `FRAME_*` env,
  `~/.config/frame/config`, `<save_dir>/frame/…`, XDG dirs `…/frame`,
  `frame.service`, KDE `frame-<action>.desktop` / component
  `frame_<action>_desktop`, GNOME `…/custom-keybindings/frame-<a>/`,
  `notify-send -a frame`, `frame.desktop` (absolute Exec + X-KDE key
  kept), manifest `share/frame/installed`, titles "Frame". "Clip saved"
  toast kept. Internal Rust names (`clip_bin`, `ClipTray`) unchanged.
- `migrate_config` in bin/frame (runs before load_config for every
  command but --help/--version/monitors): only when `FRAME_CONFIG` is
  unset, `~/.config/frame/config` doesn't exist (not even as a link) and
  `~/.config/clipped/config` is a file: `cp -p`, stderr note "frame:
  note: copied your Clipped config A to B". Nothing else is migrated.
  The settings window runs `frame check` once at start when there is no
  config, so the copy happens before it reads.
- `gui/src/daemon.rs` (new) `Supervisor`: the tray runs `frame daemon`
  as a child (own process group, PR_SET_PDEATHSIG=TERM, output to
  `$RUNDIR/daemon.log`, reset past 1 MiB). Before each start it runs
  `frame check`: exit 2 → BadConfig (no retry until the file's bytes
  change). A live `daemon.pid` that isn't ours (frame.service, a
  terminal) → External: left alone, ours starts when it disappears; a
  daemon-key change then runs `systemctl --user restart frame.service`
  if the service is active. Unasked exit → retry after 2, 4, … 60 s
  (reset after 60 s of running), reason = last line of daemon.log. Each
  second the tray re-reads the config; if any `config::DAEMON_KEYS`
  value differs from what the daemon started with, it restarts it (so
  replay off/on in settings stops/starts the buffer). Stop = TERM, wait
  10 s, then KILL the group.
- Design choice (differs from the brief): the tray always runs the
  daemon, also with `replay = false` (the daemon then only binds and
  idles). Hyprland/Sway shortcuts last only while something re-applies
  them at session start, and it makes "replay off → on" a plain restart.
- `tray.rs`: Quit and SIGTERM/SIGINT/SIGHUP set an atomic; the main loop
  (200 ms) stops the daemon, removes `$RUNDIR/tray.pid`, shuts ksni down.
  Menu: "Replay buffer: on, 60 s" / off / starting… / off (config
  error) + a problem line, then record, save replay, screenshots, Open
  Frame folder, Settings…, Quit. "Background service" toggle removed.
  Tooltip description = the problem, if any.
- `settings.rs` (functional only): "Restart service" button and
  `restart_service` gone; after a Save that changes daemon keys the note
  is "Frame restarts the replay buffer with the new settings." when
  tray.pid is live, else a hint to restart `frame daemon` / the service.
  Startup section: "Start Frame when I log in" (autostart/frame-gui.desktop;
  Comment "Frame tray: shortcuts and replay buffer").
- `install.sh`: a first real install (no DESTDIR, no manifest before)
  writes the same autostart entry unless `--no-autostart`; reinstalls
  keep the user's choice. Options can come in any order; unknown ones
  are refused.
- Tests: smoke +2 (migration once/default path only; autostart on first
  real install only, --no-autostart, DESTDIR none, unknown option).
  cargo +5 (4 supervisor tests with a fake frame script: backoff 2→4 s,
  stop, daemon-key restart vs. other keys, bad config waits for a
  change, external daemon left alone + taken over; tray replay_lines)
  and an exact-text autostart check shared with smoke.

Checks run (see the report to Sora for the printed lines): bash -n,
smoke, cargo test, cargo clippy --all-targets, cargo build --release.
Live, private bus (`dbus-run-session`, temp XDG dirs): release tray +
fake frame: start, restart on `replay = false`, restart 2 s after
KILL of the daemon, TERM → daemon got TERM, tray rc 0, tray.pid gone.
Release tray + real bin/frame (XDG_CURRENT_DESKTOP=none, replay off):
`video_fps = 999` → no daemon, tooltip "Frame: not running" / "Config
error: video_fps must be 1-240"; fixed → `frame status` "daemon:
running"; TERM → "daemon: stopped", log ends "frame: daemon stopped".

Next: Sora migrates Kevin's machine (stop clipped tray + clipped.service,
`clipped unbind`, remove the clipped autostart/desktop entries,
`./install.sh`, start frame-gui). Not tested: a real login autostart,
the real KDE panel menu, gsr replay under the tray (only the fake).
Strata restyles settings.rs next and wires theme.rs.

## Settings window restyle, phase 2 (2026-10-17, Strata, uncommitted for Sora's review)

Only `gui/src/settings.rs` (UI part) and `gui/src/main.rs` (`mod theme;`)
changed; theme.rs, config/state/daemon/tray untouched. Look =
`design/settings-mockup.png` (spec in `design/gui.md`).

- `theme::apply` runs in the settings creator. Window 600x900, min
  520x480, resizable; body scrolls.
- Header: app icon (APP_64 texture) + "Frame" + subtitle, right-aligned
  `status_pill` from `Status::read` + `tray_running` (re-read once a
  second): "Recording · m:ss" (RED, age of record.pid), "Replay on · 60 s"
  (ACCENT), "Replay off" (daemon up, no buffer), "Starting…" (AMBER, tray
  up, no daemon), "Not running".
- Four `theme::card`s: Saving, Video & replay, Shortcuts, Startup; every
  line is `theme::row` (132 px label column); on/off = `toggle_label`.
- Friendly combo labels, raw values written: PNG/JPEG/WebP, MP4/WebM/MKV,
  Medium…Ultra, codecs ("Auto (best for the GPU)", "HEVC (H.265)"…), Frame
  rate = combo 24…240 fps (+ the configured valid value if unlisted).
  Audio = Desktop audio / Microphone / Desktop + microphone
  (`default_output|default_input`) / No audio / Custom… (text field; any
  non-preset value opens as Custom). Values frame refuses show as the file
  has them + red "config has 'x'" (combos, toggles, replay length).
- Shortcuts: keycaps + Record + Clear per action. Record opens the
  shortcut editor as a themed `egui::Modal` already listening (KeyTap,
  Recorder unchanged: up to 3 keys, Stop, Esc cancels); without KeyTap
  (tests) it opens not listening. The dialog keeps Meta/Ctrl/Alt/Shift
  (now toggle chips) + key list, Set/Cancel; a click outside cancels
  (never while listening). The per-row free-text shortcut field is gone
  (the editor covers it; invalid file values show red in the keycaps
  slot). "Turn on shortcuts" = primary button at the top of the card.
- Sticky bottom bar: status text (red error / green "Saved." / muted),
  AMBER dot "Unsaved changes", Revert (Reload on read error), accent Save
  (grey when disabled), spinner while `frame bind` runs. Save/bind/
  autostart/restart-note logic is unchanged.
- `FRAME_GUI_SHOT_VIEW` (only with FRAME_GUI_SHOT): `end` scrolls to the
  bottom, an action name (`region`…) opens its editor, for shots.
- Shots (temp XDG dirs, temp FRAME_CONFIG, FRAME_BIN=bin/frame, dry run;
  replay pill via fake pid files naming a `sleep`): shots/frame-settings.png,
  frame-settings-end.png, frame-shortcut-editor.png,
  frame-settings-invalid.png (all 600x900). Script used: /tmp/frame-shot.sh
  (not in repo).
- New tests: friendly_labels_keep_config_values, header_pill; the
  raw-value test also runs `frame check` on each audio preset.
- Checks run: `cargo test` "test result: ok. 52 passed" (but see flake
  below); `cargo clippy --all-targets` 0 warnings; `bash tests/smoke.sh`
  "109 checks, all passed"; `bash -n bin/frame` ok.
- Known flake (not from this change, for Atlas):
  `daemon::tests::starts_restarts_with_backoff_and_stops` fails at
  daemon.rs:369 (`View::Running` expected right after `start`) in ~1 of 5
  runs, also with only `cargo test daemon::` (5/25), never with
  `--test-threads=1` (0/10). Likely ETXTBSY: each test writes its fake
  `frame` script and execs it while a parallel test forks with the write
  fd open, so `check` fails → Retry.
- Not done: real key presses in the dialog (no input tool), live window
  with the real tray, light theme.

## Sora's check + real install (2026-10-17 ~21:58, uncommitted)

Kevin: no Spectacle (users shouldn't install extra apps); video monitor
pick always recorded the same screen; asked for the update installed.

- Probes: the xdg Screenshot portal (interactive=false) never answered
  in 15 s on KDE (likely a permission prompt), so it was not used. KWin
  ScreenShot2 refuses ordinary callers (NoAuthorized) and allows a
  binary whose canonical exe = absolute Exec of a .desktop entry with
  `X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2`. Never
  authorize a general interpreter (python/gdbus/busctl).
- gsr -w DP-1 vs -w DP-2: 3 s test recordings in /tmp gave different
  frames, so gsr was fine; the bug was "auto" = first monitor on KDE.
- Checks: bash -n ok; smoke "107 checks, all passed"; cargo test "43
  passed"; clippy 0 warnings. `./install.sh` → installed bin + gui
  cmp-equal; clipped.desktop has the X-KDE key and absolute Exec.
  Installed `clipped monitor|all|window` with a temp config (save_dir
  /tmp/clippedreal, notify/clipboard off): rc 0, 2560x1440 / 2560x2880
  / 2560x1440, no spectacle process. `clipped monitors` →
  DP-1|2560x1440, DP-2|2560x1440|active.
- Restarted Kevin's tray (new binary) and clipped.service (still
  -w DP-1 because his config says video_source = DP-1; left for him).
- Still needs Kevin: a real key press, a real region drag, picking
  "Monitor you're on (auto)". Region on KDE still needs slurp (not part
  of Plasma); a built-in selector in clipped-gui would remove it.
  GNOME: gnome-screenshot isn't preinstalled on GNOME 42+; the portal
  would be the no-extra-app path there (not built).

## Video source follows your monitor; Source combo (2026-10-17, Atlas, uncommitted for Sora's review)

Kevin: "the monitor selection for videos is not working, it records the
same monitor each time." Cause: `video_source = auto` on KDE/GNOME took
the first `gpu-screen-recorder --list-monitors` line (always DP-1), and
the GUI Source was a free text box with a small ⏷ menu.

- `bin/clipped`: `active_monitor` = Hyprland/Sway focused output (as
  before) or, on KDE, `kwin_active_output` (`gdbus call --session
  --timeout 2 … org.kde.KWin.activeOutputName` under `timeout 3`; reply
  must match `('NAME',)` with NAME in `[A-Za-z0-9_.:-]`). On KDE the name
  is used only if gsr lists it; else the first listed monitor; else
  `screen`. GNOME: first monitor (no simple Mutter call). A recording
  resolves it at start; the replay buffer at daemon start.
- New `clipped monitors` (runs before load_config, so a bad config
  doesn't hide it): gsr's `NAME|WxH` lines, `|active` on the one you're
  on; exit 1 "gpu-screen-recorder is not installed" without gsr.
- Config comment: "auto = monitor you're on; or DP-1, portal, screen
  (X11)" + a line pointing at `clipped monitors`.
- GUI (`settings.rs`): Source is a ComboBox: "Monitor you're on
  (auto)", each monitor "DP-2  2560x1440", "Pick each time (portal)",
  "All monitors (X11 screen)" only when XDG_SESSION_TYPE=x11, and the
  configured value if it is none of those ("DP-9 (not detected)"), so
  it is never dropped. List comes from `clipped monitors` via
  `clip_bin()` (GUI no longer runs gsr). "(looking for monitors…)" /
  "(no monitors found)" hints. `needs_daemon_restart` (video_source is
  in DAEMON_KEYS) keeps the existing "Restart service" offer. The
  CLIPPED_GUI_SHOT self-shot now waits for the monitor list (cap 300
  frames).
- smoke.sh: fake gsr prints `FAKE_MONITORS`; a fake `gdbus` lives in
  `$T/fakekwin` (only on PATH in the new checks, so kglobalaccel checks
  are untouched). 7 new checks: KWin active → DP-2, record follows the
  active monitor, unlisted/garbage/failed KWin → first, none listed →
  screen, KWin not asked off KDE, `monitors` output + works with a bad
  config, no gsr → exit 1.

Checks run: `bash -n bin/clipped` ok; `bash tests/smoke.sh` "107
checks, all passed"; `cargo test` "test result: ok. 43 passed" (3 new:
monitors_parse, source_choices, video_source_change_offers_restart);
`cargo clippy --all-targets` 0 warnings; `cargo build --release` ok,
6,641,400 bytes. Real, read-only on Kevin's KDE: `bin/clipped monitors`
→ `DP-1|2560x1440`, `DP-2|2560x1440|active`; with a temp auto config
`--detect` followed KWin (DP-1 when KWin said DP-1). Self-shots of the
settings window (temp dirs) show "DP-2  2560x1440" and "Monitor you're
on (auto)". No recording made. shellcheck not installed.

Not done: Kevin's real config still says `video_source = DP-1`, so he
must pick "Monitor you're on (auto)" (or DP-2) and restart the service;
needs reinstall into ~/.local first. Replay buffer stays on the monitor
active at daemon start (switching it live would need a gsr restart).

## KDE without Spectacle: KWin ScreenShot2 (2026-10-17, Atlas, uncommitted for Sora's review)

Kevin: "Do we have to use Spectacle? I don't want users to have to
install other applications." Sora found KWin authorizes ScreenShot2 by
caller pid → /proc/pid/exe → a .desktop whose Exec (absolute,
canonicalized) matches and has
`X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2`.

- `gui/src/grab.rs` (new): `clipped-gui --grab all|monitor|window|area
  X Y W H OUT.png` → CaptureWorkspace / CaptureActiveScreen /
  CaptureActiveWindow (decoration on, shadow off) / CaptureArea, with
  native-resolution true, include-cursor false. Pipe read on a thread
  during the call; stride handled (last row may be short); QImage
  formats 4/5/6 and 16/17/18, unknown = error. Opaque shots are written
  as RGB PNG, else RGBA; written to `.OUT.part` then renamed. Exit 0 ok,
  1 + one stderr line on failure (NoAuthorized → "not authorized: is
  clipped.desktop installed with an absolute Exec and X-KDE-…?"), 2 on
  bad args. zbus =5.15.0 is now a direct dep (tokio + blocking-api);
  Cargo.lock only gained the "zbus" line under clipped-gui, no new crate.
- `bin/clipped`: backend `kwin` when desktop=kde, session=wayland and
  `gui_bin` finds clipped-gui (`$CLIPPED_GUI` only, if set; else next to
  the script; else PATH). Region = `select_region` (same slurp dim args)
  → `--grab area X Y W H`. A failed grab now ends with a toast +
  "clipped: screenshot failed: <reason>" (rc 1) instead of "cancelled".
  `kde_region`/`kde_layout` (full grab + ffmpeg crop + kscreen-doctor)
  are gone. Spectacle stays as fallback (no clipped-gui, or KDE X11).
  Non-png formats use the same ffmpeg conversion as before.
- `install.sh`: clipped.desktop gets the X-KDE key; Exec stays the
  final `$PREFIX` path (never DESTDIR). desktop-file-validate: rc 0, no
  output.
- smoke.sh: `CLIPPED_GUI` points at a fake clipped-gui (logs args,
  writes "png"; FAKE_GUI=fail mimics NoAuthorized). New checks: kde
  wayland → kwin, no clipped-gui → not kwin, x11 → not kwin, gui_bin
  next to script, kwin monitor/all, spectacle fallback flags (fake
  spectacle), real-mode kwin with fake gui+slurp (saves png, failed grab
  message, cancelled selection never calls the gui). Region/window webp
  checks now expect `--grab`. Install check expects the X-KDE line.
- Lesson: KDE still needs **slurp** for region (separate package).
  Replacing it with our own overlay in clipped-gui would remove the last
  extra tool on KDE; not done.

Checks run: `bash -n bin/clipped` ok; `bash tests/smoke.sh` "100 checks,
all passed" (15 s); `cargo test` "test result: ok. 40 passed" (5 new in
grab); `cargo clippy --all-targets` no warnings; `cargo build --release`
ok, 6,637,288 bytes. `DESTDIR=… PREFIX=/usr/local ./install.sh
--no-build` → entry has the key, validates; `--uninstall` left 0 files.
Real KWin check (temp `~/.local/share/applications/clipped-grabtest.desktop`
→ gui/target/release/clipped-gui, deleted afterwards): all 2560x2880
(4.4 MB), monitor 2560x1440 (717 KB), window 1280x1394 (252 KB), area
100 100 640 360 → 640x360 (70 KB), all rc 0, RGB PNGs, luma 0–255
(not blank). Without the entry: rc 1, the "not authorized" line, no file.
Not done: reinstall into ~/.local (Sora), a real shortcut press, real
slurp drag → kwin area on Kevin's screens (HiDPI/scale ≠ 1 untested).
Note: until reinstall, ~/.local/bin/clipped-gui is the old binary
without --grab, but the installed bin/clipped is also old, so shortcuts
keep using Spectacle until both are replaced.

## KDE shortcuts were inactive; grim on KDE (2026-10-17, Sora, uncommitted)

Kevin: Record captures keys fine, but pressing a bound key does
nothing; wants only grim + slurp for screenshots.

- Cause: `kde_bind` called `setShortcut … 4` (NoAutoloading only).
  KDE stored the keys (kglobalshortcutsrc, globalShortcutsByKey all
  looked right), but every `/component/clipped_*_desktop`
  `Component.isActive` was `(false,)`, so KWin never grabbed them.
  The journal showed no `app-clipped\x2d<action>@` launch ever. Now it
  passes flags 6 (SetPresent|NoAutoloading), and after a successful
  set it warns "KDE stored … but left it inactive" if isActive is
  false. The smoke test now expects `6`.
- Lesson: checking for a bound KDE shortcut means checking
  `Component.isActive`, not only the stored keys.
  `invokeShortcut` works on inactive shortcuts, so it proves nothing
  about key presses.
- grim on KDE: still impossible. grim 1.5.0 on KWin 6.7.5 →
  "compositor doesn't support the screen capture protocol";
  wayland-info lists zwlr_layer_shell_v1 but neither wlr-screencopy
  nor ext-image-copy-capture. So KDE keeps `spectacle -b -n` (no UI)
  as the grabber, with slurp as the selector. grim + slurp are already
  the backend on Hyprland/Sway. Kevin asked for grim only; told him
  why it can't work on KDE. No change made.

Checks: `bash tests/smoke.sh` "95 checks, all passed";
`./install.sh --no-build`, cmp equal; real `clipped bind` rc 0, then
isActive `(true,)` for all 6. Not checked: a real key press (needs
Kevin).

## KDE bind retry, own region selector, Record capture (2026-10-17, uncommitted)

Kevin: "changed the keybinds and it's still not working"; then asked for
a Record button capturing up to 3 keys (+ Stop), and a region mode that
dims the screens instead of Spectacle's selector.

- Bind bug (Sora): binds state held only region + window. Real
  `clipped bind` warned "Meta+Alt+A/C/V taken", but
  `isGlobalShortcutAvailable` said free and globalShortcutsByKey had no
  owner. Probes (temp `clipped-probe-*.desktop`, removed) showed
  kglobalaccel returns `@ai []` for a free key right after a new
  .desktop entry is written, and accepts it on try 1–4 at 0.5 s steps.
  `kde_bind` now retries up to 10x/0.3 s while `kde_free` (new) says
  the key is free, and warns "KDE did not accept" vs "taken". After
  reinstall, real `clipped bind`: rc 0, 1.4 s, all 6 lines in binds;
  globalShortcutsByKey shows each key owned by clipped-<action>.desktop.
  kbuildsycoca6 before register was tried and did not help.
- Region on KDE Wayland (Sora): `kde_region` = `spectacle -b -n -f`
  full grab (0.64 s, 2560x2880 on Kevin's stacked DP-1/DP-2), then
  `slurp -b #00000055` (dims the *live* screens; not a frozen image),
  then ffmpeg crop scaled by `kde_layout` (kscreen-doctor -j; fallback
  1:1). Used only when session is wayland and slurp + ffmpeg exist;
  else `spectacle -r` as before. Empty selection = cancelled. Crop
  maths checked on a generated 2560x2880 image (matches plain crop).
  smoke region test now uses fake slurp/ffmpeg in `$T/fakeregion`.
  Not checked: slurp overlay on KWin with a real drag (needs Kevin).
- Record/Stop (Atlas): settings.rs wraps eframe with
  `eframe::create_native` + `KeyTap` winit handler, so Meta and Print
  are seen (egui drops them). Up to 3 keys, stops on release or Stop,
  Esc cancels; fills the Meta/Ctrl/Alt/Shift + key list editor.
  gui/Cargo.toml: `winit = "=0.30.13"` (already in the tree). Combos
  KDE already owns (incl. ours) are grabbed by KWin first; the UI hints
  to press Stop and pick from the list. blockGlobalShortcuts not used
  (global flag, would stay on if we crash).
- Nova review: no blockers; X11 slurp gate and noisy 0x0 cancel fixed.
  Open: no tests for real-mode slurp cancel / kde_bind retry (fake
  gdbus) / spectacle -r fallback; rotated screens in kde_layout; Record
  with only modifiers held gives no key; non-Latin layouts refused.

Checks: `bash tests/smoke.sh` "95 checks, all passed"; `cargo test`
"35 passed"; `cargo clippy --all-targets` no warnings; `./install.sh`
then cmp equal for both binaries. Nova: settings window opens via the
new event loop (CLIPPED_GUI_SHOT, rc 0). Not done: real key presses in
Record, a real region drag, commit. Replay key needs `clipped service
on` (daemon not running).

## Shortcut editor + bind on Save (2026-10-17, Atlas, uncommitted for Sora's review)

Kevin installed it and reported (1) Meta+Print can't be set in the
settings window, (2) shortcuts take no pictures. Sora's diagnosis:
egui 0.33 has no Print key and sees Meta only on macOS; and Save never
ran `clipped bind` (no binds state, no daemon), so nothing was ever
registered with KDE. Capture itself works. Changes, all in
`gui/src/settings.rs`:

- One-shot key capture and the "add Meta" box are gone. Record (empty
  field) / Change opens an editor under the Shortcuts grid: Meta / Ctrl
  / Alt / Shift boxes, a key list (`key_choices`: Print first, Pause …
  Return, F1–F24, A–Z, 0–9; all accepted by key_info), live preview,
  Set / Cancel. Prefill parses the field case-insensitively
  (Super/Win = Meta, PageUp = PgUp, F25–F35 kept); an empty field
  starts with Meta on. A key egui sees fills the key + Ctrl/Alt/Shift
  (Meta box untouched); Esc alone cancels. Keys are ignored while a
  text box has focus, so typing a value still works.
- Save runs `clipped bind` if any bind.* changed OR
  `$XDG_STATE_HOME/clipped/binds` doesn't exist yet. "Shortcuts apply
  when clipped starts" is gone. `clipped bind` needs no daemon
  (with_bind_lock only makes RUNDIR). When never bound and nothing is
  edited, the Shortcuts section shows "Shortcuts are not active." +
  "Turn on shortcuts" (runs the same rebind).
- Tests: `shortcut_parse_and_text`, `key_choices_match_clipped`,
  `first_save_binds` (new); `save_flow` now expects the bind. Every test
  whose Save succeeds calls `wait_rebind` before it ends, so a bind
  thread can't outlive env_lock and log into the next test's calls.

Known gap: on a desktop with no bind backend ("manual"), `clipped bind`
writes no state, so "Turn on shortcuts" stays shown, and its stdout
(the manual list) isn't shown in the GUI.

Checks run: `cargo test` "test result: ok. 32 passed"; `cargo clippy
--all-targets` no warnings; `bash tests/smoke.sh` "95 checks, all
passed". Mutation: with the old bind rule put back, `first_save_binds`
fails. `shots/gui-shortcuts.png` = settings frame (temp env, dry run)
with Change buttons + "Turn on shortcuts". No shot of the open editor:
no input tool (xdotool/ydotool/wtype) and no test hook wanted.
Not done: real bind on Kevin's session, install, commit.

Sora's check (21:00): `cargo test` "32 passed", clippy no output,
smoke "95 checks, all passed"; diff read. Diagnosis evidence: real
`CLIPPED_CONFIG=/tmp/cpk/config clipped monitor` rc 0, PNG saved; a
temporary kglobalaccel probe entry (`clipped-probe.desktop`, Meta+Alt+F12,
invoked with Component.invokeShortcut, then unregistered and deleted)
also saved a PNG. So shortcut-launched capture works on KDE Wayland
with nothing focused; XWayland is not needed (and an X11 client can't
capture the Wayland screen). Kevin had cleared all Spectacle shortcuts
(Meta+Print free) and tried `pacman -R spectacle` at 20:48; it is still
installed (plasma-meta needs it), and Clipped needs it on KDE. Then
`./install.sh` rebuilt and reinstalled into ~/.local (binary cmp equal
to gui/target/release). The tray/settings processes Kevin had open
still run the old binary until he quits and reopens them. Kevin's real
shortcuts were not bound by us; he presses Save / "Turn on shortcuts".
Lesson: egui can't be the only way to enter a shortcut on Linux.

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
- App renamed clip -> Clipped 2026-10-17 (commit 07e34ab, local): bin/clipped, clipped-gui, ~/.config/clipped/config, <save_dir>/clipped/{images,videos}, CLIPPED_* env vars, clipped.service, clipped-<action>.desktop. Repo folder stays /home/kevin/Workspace/clip. Recording = gpu-screen-recorder on GPU (VAAPI on Kevin's AMD RX 9070, auto picks av1); only the X11 ffmpeg fallback is CPU. Checks: smoke 95, cargo test 29. (Sora, 2026-10-18)
- Clipped shortcut fix (2026-10-17, Atlas, uncommitted for Sora's review): settings.rs now has a shortcut editor (Meta/Ctrl/Alt/Shift boxes + key list with Print first, Set/Cancel) instead of key capture; Save runs `clipped bind` when bind.* changed or no $XDG_STATE_HOME/clipped/binds exists; "Turn on shortcuts" button when never bound. cargo test 32 pass, clippy clean, smoke 95 pass. Known gap: manual-backend desktops never write binds state, so the button stays. (Atlas, 2026-10-18)
- Clipped 2026-10-17: Kevin's "no pictures" = shortcuts never bound (GUI Save didn't run `clipped bind`); capture via spectacle works from KDE shortcuts on Wayland (XWayland not needed). egui has no Print key and no Meta on Linux, so settings now has a Meta/Ctrl/Alt/Shift + key-list editor; Save binds when never bound. Reinstalled to ~/.local, uncommitted. Kevin cleared Spectacle's shortcuts and tried removing spectacle (plasma-meta keeps it) — Clipped needs spectacle on KDE. (Sora, 2026-10-18)
- Clipped Record capture (2026-10-17, Atlas, uncommitted): egui-winit 0.33.3 drops Super on Linux (mac_cmd only on macOS) and has no Print key, and eframe gives no winit-event hook. settings::run now uses eframe::create_native on its own winit EventLoop, wrapped in KeyTap (an ApplicationHandler that copies KeyboardInput/Focused into a queue), so Record sees Meta and Print. Added direct dep winit =0.30.13 (default-features=false), which was already in Cargo.lock, so no new crate. Recorder = pure state machine (max 3 keys, ends on release or at 3, Stop, Esc alone cancels). Checks: cargo test 35 passed, clippy clean, smoke 95 passed. Real key presses not tested: no input tool on the machine. (Atlas, 2026-10-18)
- Clipped 2026-10-17 (uncommitted, installed to ~/.local): KDE binds silently failed because kglobalaccel rejects a free key for ~1.5 s after a new clipped-<action>.desktop is written; kde_bind now retries while isGlobalShortcutAvailable is true. All 6 of Kevin's keys bound (Meta+Alt+Print region, Meta+Print window, Meta+Ctrl+Print monitor, Meta+Alt+A/C/V). KDE Wayland region = spectacle -b -n -f grab + slurp dim (#00000055) + ffmpeg crop (Kevin doesn't want Spectacle's selector). KWin has layer-shell but no screencopy, so grim can't capture on KDE. GUI Record/Stop captures up to 3 keys via winit KeyTap (winit =0.30.13). cargo test 35, smoke 95. (Sora, 2026-10-18)
- Clipped 2026-10-17: KDE shortcuts never fired because kde_bind used setShortcut flags 4; needs 6 (SetPresent) or Component.isActive stays false (keys look stored but aren't grabbed). Fixed + installed, all 6 active. Verify binds via /component/clipped_<a>_desktop isActive. grim can't work on KWin 6.7.5 (no wlr-screencopy/ext-image-copy-capture), so KDE keeps spectacle -b -n + slurp. (Sora, 2026-10-17) (Sora, 2026-10-18)
- Clipped KDE without Spectacle (2026-10-17, Atlas, uncommitted): clipped-gui --grab all|monitor|window|area X Y W H OUT.png (gui/src/grab.rs, zbus =5.15.0 blocking, no new crates) calls KWin ScreenShot2; KWin authorizes it via clipped.desktop Exec + X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2 (install.sh writes it). bin/clipped backend "kwin" on KDE Wayland when clipped-gui found ($CLIPPED_GUI, next to script, PATH); region = slurp → --grab area; spectacle only fallback; kde_region/kde_layout removed. Real KWin grabs verified (all 2560x2880, monitor 2560x1440). Region still needs slurp. Checks: smoke 100, cargo test 40, clippy clean. (Atlas, 2026-10-18)
- Clipped video source (2026-10-17, Atlas, uncommitted): video_source=auto now = monitor you're on (KDE: KWin activeOutputName via gdbus, must be in gsr --list-monitors; Hyprland/Sway focused; else first). New `clipped monitors` (NAME|WxH[|active]). GUI Source = ComboBox fed by `clipped monitors` (auto / each monitor / portal / screen on X11 / unknown value kept). Replay picks the monitor at daemon start. Checks: smoke 107, cargo test 43, clippy clean. Kevin's config still has video_source = DP-1. (Atlas, 2026-10-18)
- Clipped 2026-10-17 (uncommitted, installed to ~/.local): KDE Wayland screenshots no longer use Spectacle. `clipped-gui --grab all|monitor|window|area X Y W H OUT` calls KWin org.kde.KWin.ScreenShot2 via zbus; KWin authorizes it because clipped.desktop has absolute Exec + X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2 (never authorize python/gdbus). Spectacle = fallback only. Region still uses slurp. video_source auto on KDE = KWin activeOutputName; `clipped monitors`; GUI Source is a combo. smoke 107, cargo test 43. (Sora, 2026-10-18)
- Frame GUI phase 1 design (Strata, 2026-10-17, uncommitted): blue icons gui/icons/frame{,-idle,-replay,-recording}.svg (accent #3D8BFF, app gradient #5A9CFF→#1F5BD8), render.sh outputs png/frame-*. gui/src/theme.rs = dark theme + widgets (apply, card, row 132px label col, toggle/toggle_label, primary/secondary_button, status_pill, keycaps, bottom_bar_frame, page_frame); not wired into main.rs yet. Spec + hex table in design/gui.md; target look design/settings-mockup.png (ignored by *.png in .gitignore, needs git add -f or an exception). theme.rs checked via scratch crate /tmp/frame-themecheck (clippy clean, 2 tests). (Strata, 2026-10-18)
- Frame (renamed from Clipped, 2026-10-17, Atlas, uncommitted, v0.3.0): bin/frame, frame-gui, FRAME_* env, ~/.config/frame/config (one-time copy from ~/.config/clipped/config), <save_dir>/frame/. Tray = Frame running: gui/src/daemon.rs Supervisor runs `frame daemon` as a child (always, even with replay=false), backoff 2..60 s, BadConfig waits for a file change, leaves an external daemon (frame.service) alone, restarts it when config::DAEMON_KEYS change, stops it on Quit/SIGTERM. $RUNDIR/tray.pid, daemon.log. The first real install writes autostart/frame-gui.desktop unless --no-autostart. Checks: smoke 109, cargo test 48, clippy 0. Live-test the tray with dbus-run-session + temp XDG dirs. (Atlas, 2026-10-18)
- Frame settings restyle phase 2 (2026-10-17, Strata, uncommitted): settings.rs UI rebuilt on theme.rs (main.rs has mod theme; theme::apply in creator). Header icon + live status pill (Status::read + tray_running, 1 s), four cards, sticky bar, friendly combo labels writing raw values, Audio presets + Custom…, shortcut editor = themed egui::Modal opened by row Record (already listening). FRAME_GUI_SHOT_VIEW=end|<action> for shots. Shots in shots/frame-settings*.png, frame-shortcut-editor.png. cargo test 52, clippy 0, smoke 109. daemon::tests::starts_restarts_with_backoff_and_stops is flaky in parallel (~1/5, likely ETXTBSY on the fake frame script), not from this change. (Strata, 2026-10-18)
- Frame flaky test fixed (2026-10-17, Atlas, uncommitted): daemon backoff test hit ETXTBSY. Another test thread forked while fake_frame's write fd was open; temp+rename can't fix that (same inode). daemon.rs tests now use wait_execable (exec the script, retry on ETXTBSY). 60/60 cargo test runs passed (52 tests each), clippy 0. The real tray isn't affected. (Atlas, 2026-10-18)
- Frame settings.rs Nova fixes (2026-10-17, Strata, uncommitted): shortcut editor only captures keys in Record (egui key_name capture removed); outside Record Enter = Set (unless Cancel focused or a popup open), Esc = modal close. Codec shows red "not for WebM" (webm_codec = auto|av1|vp8|vp9, same as bin/frame) and Save refuses the pair; emptied Custom audio saves `none`. cargo test 55, clippy --all-targets clean. Shots: shots/frame-settings*.png, frame-shortcut-editor.png. (Strata, 2026-10-18)
- Frame review round (2026-10-17, Atlas, uncommitted): install.sh removes an old Clipped install (share/clipped/installed) on a real install: stop clipped-gui, clipped service off/unbind, files in its list, autostart/clipped-gui.desktop; keeps ~/.config/clipped and the captures. frame daemon: daemon.pid + TERM trap are set right after flock; lock busy = exit 3; unit has RestartPreventExitStatus=3; `frame service on` refuses while tray.pid is live. daemon.rs reads the config after `frame check` and guards PDEATHSIG with getppid. Checks: smoke 114 passed, cargo test 55 passed, clippy 0. (Atlas, 2026-10-18)
- Frame 0.3.0 (2026-10-17, commit b7bbaa1, local): Clipped renamed to Frame (bin/frame, frame-gui, ~/.config/frame/config copied once from clipped, <save_dir>/frame/{images,videos}, FRAME_* env). Repo folder renamed to /home/kevin/Workspace/frame. Tray = Frame running: it runs `frame daemon` as a child (gui/src/daemon.rs, backoff restarts, restarts on daemon-key config changes, stops on quit); frame.service is a no-tray alternative and refuses while the tray runs. install.sh removes an old Clipped install and turns autostart on by first install (--no-autostart). Blue viewfinder icons; settings restyled via gui/src/theme.rs (design/gui.md). Installed + running for Kevin; smoke 114, cargo test 55. (Sora, 2026-10-18)
<!-- workspace:memory:end -->
