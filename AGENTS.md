# AGENTS.md — clip

## Read first

Before any work, read `README.md`, this file and `HANDOFF.md`.
After lasting work, update `HANDOFF.md` (state, next steps, decisions,
lessons, checks you actually ran and what they printed).

## Goal

A Hyprshot-style screenshot tool (region / window / output) that saves a
PNG and copies it to the clipboard on KDE, GNOME, Hyprland, Sway and X11.
Reference: https://github.com/Gustash/hyprshot (Bash, GPL-3.0).

## Constraints

- Plain Bash, one script (`bin/clip`). Call existing tools; do not ship a
  compiled binary unless the team agrees a portal client is needed.
- Pick backends at runtime by desktop/session; never assume Hyprland.
- grim does **not** work on KWin or Mutter ("compositor doesn't support the
  screen capture protocol"). KDE uses `spectacle -b -n`; GNOME uses
  gnome-screenshot or the xdg-desktop-portal.
- Hyprshot is GPL-3.0. Copying its code makes clip GPL-3.0 too. Licence is
  not chosen yet; ask Kevin before adding a LICENSE or copying code.
- Tests must not take real screenshots or touch the clipboard by default.
- Local repo only. Do not add a remote or push without Kevin's OK.
- No credentials or personal notes in the repo.

## Layout

```
bin/clip         the tool (executable Bash)
tests/smoke.sh   smoke tests, no screenshots taken
README.md        user overview
AGENTS.md        rules for agents (CLAUDE.md points here)
HANDOFF.md       current state and history
```

## Commands (verified 2026-10-17)

```sh
bash -n bin/clip       # syntax check
bash tests/smoke.sh    # 7 checks, exits 0 on pass
bin/clip --detect      # shows chosen backends
```

shellcheck is not installed on the dev machine; run it if available.
