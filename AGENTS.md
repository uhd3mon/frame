# AGENTS.md — clip

## Read first

Before any work, read `README.md`, this file and `HANDOFF.md`.
After lasting work, update `HANDOFF.md` (state, next steps, decisions,
lessons, checks you actually ran and what they printed).

## Goal

A background screenshot / replay / recording tool driven by global
shortcuts (region, window, monitor, all, replay, record). Saves to
`<save_dir>/clip/images` and `/videos`, copies to the clipboard, works on
KDE, GNOME, Hyprland, Sway and X11. No UI; one config file.

## Constraints

- MIT licence. All code is our own: do not copy code from Hyprshot or
  other projects.
- Plain Bash, one script (`bin/clip`). Call existing tools; do not ship a
  compiled binary unless the team agrees one is needed.
- One config file, parsed as `key = value` text. Never `source` or eval it.
- Pick backends at runtime by desktop/session; never assume Hyprland.
- grim does **not** work on KWin or Mutter. KDE uses `spectacle -b -n`;
  GNOME uses gnome-screenshot. Video uses gpu-screen-recorder.
- Tests must not take real screenshots or recordings, touch the
  clipboard, send notifications or change real shortcuts. Use
  `CLIP_DRY_RUN=1`, a temp `CLIP_CONFIG`, temp XDG dirs (incl.
  `XDG_STATE_HOME`, `XDG_CACHE_HOME`, `TMPDIR`) and the fakes in
  `tests/smoke.sh` for real-mode runs.
- Local repo only. Do not add a remote or push without Kevin's OK.
- No credentials or personal notes in the repo.

## Layout

```
bin/clip         the tool (executable Bash)
tests/smoke.sh   smoke tests, dry-run only
README.md        user overview
LICENSE          MIT
AGENTS.md        rules for agents (CLAUDE.md points here)
HANDOFF.md       current state and history
```

## Commands (verified 2026-10-17)

```sh
bash -n bin/clip       # syntax check
bash tests/smoke.sh    # 84 checks, exits 0 on pass (~10 s)
bin/clip --detect      # shows chosen backends and resolved settings
```

Functions can be unit-tested by sourcing the script
(`source bin/clip; qt_keyint Meta+Alt+F12`); `main` only runs when it is
executed. shellcheck is not installed on the dev machine; run it if
available.
