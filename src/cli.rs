//! The command line: screenshots, shortcuts, config and status.

use crate::config;
use crate::ctx::Ctx;
use crate::detect;
use crate::shot::{self, Stop};
use std::path::Path;

pub const USAGE: &str = "\
Usage: frame [command]

Screenshots:  region | window | monitor | all
Shortcuts:    bind | unbind   add / remove the global shortcuts
App:          (no command)    tray icon and the settings window
              tray            tray icon (if it runs already: settings)
              settings        settings window only
              install         copy frame, its menu entry and icons to ~/.local
                              (PREFIX); --force, --no-autostart, --from FILE
              uninstall       stop frame, remove its shortcuts and those files
Other:        status | config | --detect | --help | --version
              check [FILE]    validate the config (or FILE): prints ok, or exits 2

Config: ${XDG_CONFIG_HOME:-~/.config}/frame/config (env FRAME_CONFIG overrides).
FRAME_DRY_RUN=1 prints capture, clipboard, notification and bind commands
instead of running them.
";

/// Frame was called Clipped: the first time, copy ~/.config/clipped/config
/// (only that file, unchanged) to the new place. Only for the default path.
pub fn migrate_config(ctx: &Ctx) {
    let cfg = ctx.config_path();
    let old = ctx.config_home().join("clipped/config");
    if ctx.var_os("FRAME_CONFIG").is_some() || cfg.symlink_metadata().is_ok() || !old.is_file() {
        return;
    }
    let r = cfg.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::copy(&old, &cfg));
    match r {
        Ok(_) => ctx.warn(&format!("note: copied your Clipped config {} to {}", old.display(), cfg.display())),
        Err(e) => ctx.warn(&format!("could not copy {} to {}: {e}", old.display(), cfg.display())),
    }
}

/// `frame check FILE` for the settings window: Err = the message.
pub fn check_file(ctx: &Ctx, file: &Path) -> Result<(), String> {
    config::load(file, &ctx.home()).map(|_| ())
}

/// Run a CLI command; returns the exit code.
pub fn run(ctx: &Ctx, args: &[String]) -> i32 {
    let cmd = args.first().map(String::as_str).unwrap_or("");
    match cmd {
        "-h" | "--help" | "help" => {
            ctx.say(USAGE.trim_end());
            return 0;
        }
        "-v" | "--version" => {
            ctx.say(&format!("frame {}", env!("CARGO_PKG_VERSION")));
            return 0;
        }
        "install" | "uninstall" => return crate::install::run(ctx, args),
        "_toast" => {
            let (Some(f), Some(t)) = (args.get(1), args.get(2)) else { return 2 };
            crate::notify::toast(ctx, Path::new(f), t, args.get(3).is_some_and(|c| c == "1"));
            return 0;
        }
        _ => {}
    }
    if !matches!(cmd, "--detect" | "region" | "window" | "monitor" | "all" | "bind" | "unbind" | "status" | "config" | "check") {
        ctx.warn(&format!("unknown command: {cmd}"));
        ctx.err_raw(USAGE);
        return 2;
    }
    migrate_config(ctx);
    let path = match (cmd, args.get(1)) {
        ("check", Some(f)) => f.into(),
        _ => ctx.config_path(),
    };
    let cfg = match config::load(&path, &ctx.home()) {
        Ok((vals, warnings)) => {
            for w in warnings {
                ctx.warn(&w);
            }
            vals
        }
        Err(e) => {
            ctx.warn(&e);
            return 2;
        }
    };
    match cmd {
        "--detect" => {
            ctx.say(&format!("session={}", detect::session(ctx)));
            ctx.say(&format!("desktop={}", detect::desktop_name(ctx)));
            ctx.say(&format!("capture={}", detect::capture(ctx).name()));
            ctx.say(&format!("clipboard={}", detect::clipboard(ctx).name()));
            ctx.say(&format!("binds={}", detect::binds_name(ctx)));
            ctx.say(&format!("config={}", ctx.config_path().display()));
            ctx.say(&format!("images={}", images_dir(ctx, &cfg).display()));
            0
        }
        "region" | "window" | "monitor" | "all" => match shot::shot(ctx, &cfg, cmd) {
            Ok(()) | Err(Stop::Busy) => 0, // busy: another shot's selector is open
            Err(Stop::Cancelled) => {
                ctx.warn("cancelled");
                1
            }
            Err(Stop::Failed(e)) => {
                crate::notify::error(ctx, config::yes(&cfg["notify"]), &e);
                ctx.warn(&e);
                1
            }
        },
        "bind" | "unbind" => {
            let r = if cmd == "bind" { crate::binds::bind(ctx, &cfg) } else { crate::binds::unbind(ctx) };
            match r {
                Ok(()) => 0,
                Err(e) => {
                    ctx.warn(&e);
                    1
                }
            }
        }
        "status" => {
            let p = ctx.config_path();
            let note = if p.is_file() { "" } else { " (defaults; run frame config)" };
            ctx.say(&format!("config: {}{note}", p.display()));
            ctx.say(&format!("images: {}", images_dir(ctx, &cfg).display()));
            let bound: Vec<String> = crate::binds::state_get(ctx, detect::binds_name(ctx))
                .into_iter()
                .map(|(a, _)| a)
                .filter(|a| config::ACTIONS.contains(&a.as_str()))
                .collect();
            ctx.say(&format!("shortcuts: {}", bound.join(" ")));
            match crate::state::live_pid(Path::new("/proc"), &ctx.rundir().join("tray.pid")) {
                Some(pid) => ctx.say(&format!("tray: running (pid {pid})")),
                None => ctx.say("tray: not running"),
            }
            0
        }
        "config" => {
            let p = ctx.config_path();
            if !p.exists() {
                let r = p.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::write(&p, config::DEFAULT_TEXT));
                if let Err(e) = r {
                    ctx.warn(&format!("{}: {e}", p.display()));
                    return 1;
                }
            }
            ctx.say(&p.to_string_lossy());
            0
        }
        _ => {
            ctx.say("ok"); // check: load above already validated it
            0
        }
    }
}

fn images_dir(ctx: &Ctx, cfg: &std::collections::BTreeMap<String, String>) -> std::path::PathBuf {
    ctx.expand_dir(&cfg["save_dir"]).join("frame/images")
}

#[cfg(test)]
mod tests {
    use crate::ctx::testenv::T;

    #[test]
    fn help_version_unknown() {
        let t = T::new("help");
        let (c, out, _) = t.frame(&["--help"]);
        assert_eq!(c, 0);
        assert!(out.starts_with("Usage: frame"));
        for bad in ["replay", "record", "daemon", "service", "video"] {
            assert!(!out.to_lowercase().contains(bad), "{bad}");
        }
        assert_eq!(t.frame(&["--version"]).1, format!("frame {}\n", env!("CARGO_PKG_VERSION")));
        assert_eq!(env!("CARGO_PKG_VERSION"), "0.5.0");
        for c in ["nope", "replay", "record", "daemon", "service", "monitors", "_saved"] {
            let (code, out, err) = t.frame(&[c]);
            assert_eq!(code, 2, "{c}");
            assert!(out.is_empty(), "{out}");
            assert_eq!(err, format!("frame: unknown command: {c}\n{}", super::USAGE));
        }
    }

    #[test]
    fn config_and_check() {
        let t = T::new("cfgcmd");
        let cfg = t.dir.join("config");
        let (c, out, _) = t.frame(&["config"]);
        assert_eq!((c, out.trim()), (0, cfg.to_str().unwrap()));
        assert_eq!(std::fs::read_to_string(&cfg).unwrap(), crate::config::DEFAULT_TEXT);
        let (c, out, _) = t.frame(&["--detect"]);
        assert_eq!(c, 0);
        assert!(out.contains(&format!("images={}/home/Documents/frame/images\n", t.dir.display())), "{out}");
        let keys: Vec<&str> = out.lines().map(|l| l.split('=').next().unwrap()).collect();
        assert_eq!(keys, ["session", "desktop", "capture", "clipboard", "binds", "config", "images"]);
        assert_eq!(t.frame(&["check"]).1, "ok\n");

        std::fs::write(&cfg, "notify = maybe\n").unwrap();
        let (c, out, err) = t.frame(&["check"]);
        assert_eq!((c, out.as_str(), err.as_str()), (2, "", "frame: notify must be true or false\n"));

        // A 0.3 config: video keys skipped silently; other unknown keys warned.
        std::fs::write(&cfg, "replay = true\nvideo_fps = 9000\ncolour = red\nbind.record = Meta+V\nimage_format = jpg\n").unwrap();
        let (c, out, err) = t.frame(&["check"]);
        assert_eq!((c, out.as_str()), (0, "ok\n"));
        assert_eq!(err, format!("frame: {}:3: unknown key 'colour'\n", cfg.display()));

        // check FILE checks that file; a missing file = defaults, not created.
        let other = t.dir.join("other");
        std::fs::write(&other, "image_format = gif\n").unwrap();
        assert_eq!(t.frame(&["check", other.to_str().unwrap()]).0, 2);
        let none = t.dir.join("none");
        assert_eq!(t.frame(&["check", none.to_str().unwrap()]).1, "ok\n");
        assert!(!none.exists());
    }

    #[test]
    fn config_never_executed_and_tilde() {
        let t = T::new("pwn");
        let pwned = t.dir.join("pwned");
        std::fs::write(t.dir.join("config"), format!("save_dir = $(touch {0})\nimage_format = `touch {0}`\n", pwned.display())).unwrap();
        assert_eq!(t.frame(&["--detect"]).0, 2);
        assert!(!pwned.exists());
        std::fs::write(t.dir.join("config"), "save_dir = ~/x\n").unwrap();
        assert!(t.frame(&["--detect"]).1.contains(&format!("images={}/home/x/frame/images", t.dir.display())));
        std::fs::write(t.dir.join("config"), "save_dir = \"/my#docs\"  # c\nimage_format = 'webp'\n").unwrap();
        let (c, out, err) = t.frame(&["--detect"]);
        assert_eq!(c, 0);
        assert!(out.contains("images=/my#docs/frame/images") && err.is_empty(), "{out}{err}");
    }

    #[test]
    fn migrates_clipped_config_once() {
        let mut t = T::new("migrate");
        t.unset("FRAME_CONFIG");
        let old = t.dir.join("cfg/clipped/config");
        std::fs::create_dir_all(old.parent().unwrap()).unwrap();
        std::fs::write(&old, "image_format = jpg\n").unwrap();
        let (c, _, err) = t.frame(&["check"]);
        assert_eq!(c, 0);
        assert!(err.contains("note: copied your Clipped config"), "{err}");
        let new = t.dir.join("cfg/frame/config");
        assert_eq!(std::fs::read_to_string(&new).unwrap(), "image_format = jpg\n");
        std::fs::write(&new, "image_format = png\n").unwrap();
        let (_, _, err) = t.frame(&["check"]);
        assert!(err.is_empty());
        assert_eq!(std::fs::read_to_string(&new).unwrap(), "image_format = png\n");
    }

    #[test]
    fn status() {
        let t = T::new("status");
        std::fs::create_dir_all(t.dir.join("state/frame")).unwrap();
        std::fs::write(t.dir.join("state/frame/binds"), "kde region Meta+Alt+R\nkde replay Meta+Alt+C\nsway all Meta+A\n").unwrap();
        let (c, out, _) = t.frame(&["status"]);
        assert_eq!(c, 0);
        assert!(out.contains("(defaults; run frame config)"), "{out}");
        assert!(out.contains("shortcuts: region\n") && out.contains("tray: not running"), "{out}");
    }
}
