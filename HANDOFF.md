# HANDOFF — clip

## Goal

Hyprshot-style screenshots (region / window / output) that save to a file
and copy to the clipboard on every major Linux desktop, KDE first.

## Current state (2026-10-17)

- Project set up: README, AGENTS, CLAUDE, HANDOFF, .gitignore, local git
  repo on `main`.
- `bin/clip`: argument parsing, `--help`, `--version`, `--detect`
  (session, desktop, capture and clipboard backend, save dir).
- Capture is **not implemented**; `clip -m <mode>` exits 1 with a notice.

## Next steps

1. KDE capture via Spectacle: region `-b -n -r -o FILE`, window `-a`,
   output `-f` (or `-m` current monitor), then `wl-copy < FILE`.
2. wlroots path (grim + slurp, hyprctl for windows on Hyprland).
3. GNOME / portal path, then X11 (maim + slop or import).
4. `--clipboard-only`, notifications (notify-send), file naming
   like Hyprshot (`YYYY-MM-DD-HHMMSS_clip.png`).
5. Decide the licence with Kevin (GPL-3.0 if Hyprshot code is reused).
6. Optional: KDE global shortcut docs / .desktop file.

## Decisions

- Bash, single script, runtime backend choice (matches Hyprshot's style).
- Env var `CLIP_DIR` for the save folder (like `HYPRSHOT_DIR`).

## Lessons

- On KDE Wayland, `grim` fails: "compositor doesn't support the screen
  capture protocol". Spectacle has a headless mode (`-b`, `-n`, `-o`, `-c`).
- Dev machine has grim, slurp, spectacle, wl-copy, notify-send, jq; it
  lacks hyprctl, xclip, shellcheck, bats.

## Verification observed (2026-10-17, KDE Wayland)

- `bash tests/smoke.sh` → 7 × ok, exit 0.
- `bin/clip --detect` → session=wayland, desktop=kde, capture=spectacle,
  clipboard=wl-copy, save_dir=/home/kevin/Pictures.

## Team memory

<!-- workspace:memory:start -->
- clip layout (2026-10-17): /home/kevin/Workspace/clip, local git (main, b564813). bin/clip = single Bash script (skeleton: --help/--version/--detect; capture not built), tests/smoke.sh (7 checks, no real screenshots), README/AGENTS/CLAUDE(@AGENTS.md)/HANDOFF. Checks: bash -n bin/clip; bash tests/smoke.sh. grim fails on KDE Wayland, so KDE uses spectacle -b -n. Licence undecided (GPL-3.0 if Hyprshot code reused) - ask Kevin. (Sora, 2026-10-17)
<!-- workspace:memory:end -->
