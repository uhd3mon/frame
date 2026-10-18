//! frame-gui: tray icon and settings window for frame. bin/frame does the
//! work; this only shows its state, runs its commands and edits its config.
//! The one exception is --grab: KWin hands screenshots only to a compiled
//! binary named in a .desktop file, so bin/frame calls us for that on KDE.
//!
//!   frame-gui             tray (one per session; if it runs, open settings)
//!   frame-gui --open      tray if not running, and the settings window
//!   frame-gui --settings  the settings window only (one at a time)
//!   frame-gui --grab MODE [X Y W H] OUT.png   KDE screenshot (used by frame)

mod config;
mod daemon;
mod grab;
mod icons;
mod settings;
mod state;
mod theme;
mod tray;

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let code = match args.first().map(String::as_str) {
        None => run_tray(false),
        Some("--open") => run_tray(true),
        Some("--settings") => run_settings(),
        Some("--grab") => grab::main(&args[1..]),
        Some("--version") => {
            println!("frame-gui {}", env!("CARGO_PKG_VERSION"));
            0
        }
        Some("-h" | "--help") => {
            println!("Usage: frame-gui [--open | --settings | --version | --grab MODE [X Y W H] OUT.png]");
            0
        }
        Some(a) => {
            eprintln!("frame-gui: unknown option {a}");
            2
        }
    };
    std::process::exit(code);
}

/// Hold an exclusive flock on RUNDIR/NAME for the life of the process,
/// or None if another process has it. Rust opens files close-on-exec, so
/// the lock never leaks into frame or the recorder.
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
            eprintln!("frame-gui: {e}");
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
            eprintln!("frame-gui: {e}");
            1
        }
    }
}

pub fn spawn_settings() {
    match std::env::current_exe() {
        Ok(exe) => spawn_detached(Command::new(exe).arg("--settings")),
        Err(e) => eprintln!("frame-gui: {e}"),
    }
}

/// Start a command in its own process group, without waiting for it; a
/// thread reaps it so no zombie is left.
pub fn spawn_detached(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).process_group(0);
    match cmd.spawn() {
        Ok(mut child) => {
            std::thread::spawn(move || child.wait());
        }
        Err(e) => eprintln!("frame-gui: cannot run {:?}: {e}", cmd.get_program()),
    }
}

/// bin/frame: $FRAME_BIN, then `frame` next to this binary, then the repo
/// layout (gui/target/<profile>/frame-gui -> bin/frame), then PATH.
pub fn clip_bin() -> PathBuf {
    if let Some(p) = std::env::var_os("FRAME_BIN").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for c in [dir.join("frame"), dir.join("../../../bin/frame")] {
                if is_exec(&c) {
                    return c.canonicalize().unwrap_or(c);
                }
            }
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for d in std::env::split_paths(&path) {
            let c = d.join("frame");
            if is_exec(&c) {
                return c;
            }
        }
    }
    PathBuf::from("frame")
}

fn is_exec(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.metadata().map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
}

/// `frame ARGS…` detached (screenshots, record, replay, service).
pub fn clip_detached(args: &[&str]) {
    spawn_detached(Command::new(clip_bin()).args(args));
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
            "frame-gui-test-{}-{tag}-{}",
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
