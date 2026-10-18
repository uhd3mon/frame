//! clip-gui: tray icon and settings window for clip. bin/clip does the
//! work; this only shows its state, runs its commands and edits its config.
//!
//!   clip-gui             tray (one per session; if it runs, open settings)
//!   clip-gui --open      tray if not running, and the settings window
//!   clip-gui --settings  the settings window only (one at a time)

mod config;
mod icons;
mod settings;
mod state;
mod tray;

use std::fs::File;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn main() {
    let arg = std::env::args().nth(1);
    let code = match arg.as_deref() {
        None => run_tray(false),
        Some("--open") => run_tray(true),
        Some("--settings") => run_settings(),
        Some("--version") => {
            println!("clip-gui {}", env!("CARGO_PKG_VERSION"));
            0
        }
        Some("-h" | "--help") => {
            println!("Usage: clip-gui [--open | --settings | --version]");
            0
        }
        Some(a) => {
            eprintln!("clip-gui: unknown option {a}");
            2
        }
    };
    std::process::exit(code);
}

/// Hold an exclusive flock on RUNDIR/NAME for the life of the process,
/// or None if another process has it. Rust opens files close-on-exec, so
/// the lock never leaks into clip or the recorder.
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
            eprintln!("clip-gui: {e}");
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
            eprintln!("clip-gui: {e}");
            1
        }
    }
}

pub fn spawn_settings() {
    match std::env::current_exe() {
        Ok(exe) => spawn_detached(Command::new(exe).arg("--settings")),
        Err(e) => eprintln!("clip-gui: {e}"),
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
        Err(e) => eprintln!("clip-gui: cannot run {:?}: {e}", cmd.get_program()),
    }
}

/// bin/clip: $CLIP_BIN, then `clip` next to this binary, then the repo
/// layout (gui/target/<profile>/clip-gui -> bin/clip), then PATH.
pub fn clip_bin() -> PathBuf {
    if let Some(p) = std::env::var_os("CLIP_BIN").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            for c in [dir.join("clip"), dir.join("../../../bin/clip")] {
                if is_exec(&c) {
                    return c.canonicalize().unwrap_or(c);
                }
            }
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for d in std::env::split_paths(&path) {
            let c = d.join("clip");
            if is_exec(&c) {
                return c;
            }
        }
    }
    PathBuf::from("clip")
}

fn is_exec(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.metadata().map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
}

/// `clip ARGS…` detached (screenshots, record, replay, service).
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
            "clip-gui-test-{}-{tag}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&d).unwrap();
        TempDir(d)
    }

    /// Tests that set process-wide env vars (XDG_*, CLIP_*) hold this.
    pub fn env_lock() -> MutexGuard<'static, ()> {
        static L: Mutex<()> = Mutex::new(());
        L.lock().unwrap_or_else(|e| e.into_inner())
    }
}
