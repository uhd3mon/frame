//! frame: screenshots with global shortcuts on any Linux desktop. One
//! binary: the command line (screenshots, shortcuts, config), the tray
//! icon and the settings window.
//!
//!   frame                 tray if not running, and the settings window
//!   frame tray            tray (one per session; if it runs, open settings)
//!   frame settings        the settings window only (one at a time)
//!   frame region|window|monitor|all, bind, unbind, check, ...   see --help
//!   frame install / uninstall   copy this binary to ~/.local (the AppImage too)
//!   frame --grab MODE [X Y W H] OUT.png   KDE screenshot via KWin (debugging)
//!   frame _toast / _clip  internal: a notification / the clipboard,
//!                         each detached from the shortcut that started it

mod binds;
mod cli;
mod config;
mod ctx;
mod detect;
mod grab;
mod icons;
mod install;
mod ipc;
mod keys;
mod notify;
mod pixels;
mod portal;
mod select;
mod settings;
mod shot;
mod state;
mod theme;
mod tray;
mod wl;
mod wlclip;
mod x11;

use std::fs::File;
use std::process::Command;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        None => run_tray(true),
        Some("tray") => run_tray(false),
        Some("settings") => run_settings(),
        Some("--grab") => grab::main(&args[1..]),
        Some("_clip") => wlclip::main(&args[1..]),
        _ => cli::run(&ctx::Ctx::process(), &args),
    };
    std::process::exit(code);
}

/// Hold an exclusive flock on RUNDIR/NAME for the life of the process,
/// or None if another process has it. Rust opens files close-on-exec, so
/// the lock never leaks into frame.
fn take_lock(name: &str) -> Result<Option<File>, String> {
    let dir = state::ensure_rundir()?;
    let f = File::create(dir.join(name)).map_err(|e| format!("{name}: {e}"))?;
    match f.try_lock() {
        Ok(()) => Ok(Some(f)),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(e)) => Err(format!("{name}: {e}")),
    }
}

fn run_tray(open_settings: bool) -> i32 {
    let lock = match take_lock("gui.lock") {
        Ok(l) => l,
        Err(e) => {
            eprintln!("frame: {e}");
            return 1;
        }
    };
    if lock.is_none() || open_settings {
        spawn_settings();
    }
    match lock {
        None => 0, // the tray already runs
        Some(lock) => {
            let code = tray::run();
            drop(lock);
            code
        }
    }
}

fn run_settings() -> i32 {
    match take_lock("settings.lock") {
        Ok(Some(lock)) => {
            let code = settings::run();
            drop(lock);
            code
        }
        Ok(None) => 0, // a settings window is already open
        Err(e) => {
            eprintln!("frame: {e}");
            1
        }
    }
}

pub fn spawn_settings() {
    frame_detached(&["settings"]);
}

/// `frame ARGS…` (this binary) detached: screenshots, bind, settings.
pub fn frame_detached(args: &[&str]) {
    let ctx = ctx::Ctx::process();
    let mut c = Command::new(&ctx.exe);
    c.args(args);
    if let Err(e) = ctx::spawn_detached(&mut c) {
        eprintln!("frame: cannot run {}: {e}", ctx.exe.display());
    }
}

#[cfg(test)]
pub mod testutil {
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::{Mutex, MutexGuard};

    /// A temp dir that is removed when dropped, also when a test panics.
    pub struct TempDir(PathBuf);

    impl std::ops::Deref for TempDir {
        type Target = Path;
        fn deref(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            use std::os::unix::fs::PermissionsExt;
            // Tests chmod files to 000; make everything removable first.
            let mut stack = vec![self.0.clone()];
            while let Some(d) = stack.pop() {
                let _ = std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700));
                for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                    if e.file_type().is_ok_and(|t| t.is_dir()) {
                        stack.push(e.path());
                    }
                }
            }
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    pub fn tempdir(tag: &str) -> TempDir {
        static N: AtomicU32 = AtomicU32::new(0);
        let d = std::env::temp_dir().join(format!(
            "frame-test-{}-{tag}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&d).unwrap();
        TempDir(d)
    }

    /// Tests that set process-wide env vars (XDG_*, FRAME_*) hold this.
    pub fn env_lock() -> MutexGuard<'static, ()> {
        static L: Mutex<()> = Mutex::new(());
        L.lock().unwrap_or_else(|e| e.into_inner())
    }
}
