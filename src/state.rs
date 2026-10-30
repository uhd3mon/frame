//! Paths for the GUI (from the process environment), and the tray's pid file.

use std::path::{Path, PathBuf};

use crate::ctx::Ctx;

pub fn home() -> PathBuf {
    Ctx::process().home()
}

pub fn rundir() -> PathBuf {
    Ctx::process().rundir()
}

pub fn ensure_rundir() -> Result<PathBuf, String> {
    Ctx::process().ensure_rundir()
}

pub fn config_path() -> PathBuf {
    Ctx::process().config_path()
}

pub fn binds_state() -> PathBuf {
    Ctx::process().binds_state()
}

pub fn autostart_file() -> PathBuf {
    Ctx::process().autostart_file()
}

/// RUNDIR/tray.pid: "PID STARTTIME" of the running tray.
pub fn tray_pid_file(rundir: &Path) -> PathBuf {
    rundir.join("tray.pid")
}

/// Write FILE as "PID STARTTIME" for our own process (`frame status`
/// and install.sh read it).
pub fn write_own_pid(file: &Path) -> std::io::Result<()> {
    let pid = std::process::id();
    let start = proc_start(Path::new("/proc"), pid).unwrap_or_default();
    std::fs::write(file, format!("{pid} {start}\n"))
}

/// Is the tray running?
pub fn tray_running(proc_root: &Path, rundir: &Path) -> bool {
    live_pid(proc_root, &tray_pid_file(rundir)).is_some()
}

/// save_dir with ~ and ~/ expanded (only those).
pub fn expand_dir(d: &str) -> PathBuf {
    Ctx::process().expand_dir(d)
}

/// Field 22 (start time) of <proc>/PID/stat. The command name may hold
/// spaces and parentheses, so fields are counted after the last ") ".
pub fn proc_start(proc_root: &Path, pid: u32) -> Option<String> {
    let s = std::fs::read_to_string(proc_root.join(pid.to_string()).join("stat")).ok()?;
    let rest = &s[s.rfind(") ")? + 2..];
    rest.split_whitespace().nth(19).map(str::to_string)
}

/// The PID in a pid file ("PID STARTTIME"), if that same process still
/// runs; a stale file never matches a reused PID.
pub fn live_pid(proc_root: &Path, file: &Path) -> Option<u32> {
    let text = std::fs::read_to_string(file).ok()?;
    let mut first = text.lines().next()?.split_whitespace();
    let pid_s = first.next()?;
    let start = first.next()?;
    if !pid_s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let pid: u32 = pid_s.parse().ok()?;
    if proc_start(proc_root, pid)? != start {
        return None;
    }
    Some(pid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::tempdir;

    fn fake_proc(root: &Path, pid: u32, comm: &str, start: &str) {
        let d = root.join(pid.to_string());
        std::fs::create_dir_all(&d).unwrap();
        // fields 3..21, then 22 = start time
        let mid: Vec<String> = (3..22).map(|i| format!("{i}")).collect();
        std::fs::write(d.join("stat"), format!("{pid} ({comm}) {} {start} 23 24\n", mid.join(" ")))
            .unwrap();
    }

    #[test]
    fn start_time_after_last_paren() {
        let t = tempdir("proc");
        fake_proc(&t, 42, "evil) 1 2 3 (x", "987654");
        assert_eq!(proc_start(&t, 42).as_deref(), Some("987654"));
        assert_eq!(proc_start(&t, 43), None);
    }

    #[test]
    fn pid_files() {
        let t = tempdir("pids");
        let (p, run) = (t.join("proc"), t.join("run"));
        std::fs::create_dir_all(&run).unwrap();
        fake_proc(&p, 200, "frame-gui", "600");
        assert!(!tray_running(&p, &run));
        std::fs::write(tray_pid_file(&run), "200 600\n").unwrap();
        assert!(tray_running(&p, &run));
        assert_eq!(live_pid(&p, &tray_pid_file(&run)), Some(200));

        // Stale: same pid, other start time -> not live.
        std::fs::write(tray_pid_file(&run), "200 599\n").unwrap();
        assert!(!tray_running(&p, &run));

        for bad in ["", "abc 600\n", "200\n", "-200 600\n", "200 600x\n"] {
            std::fs::write(tray_pid_file(&run), bad).unwrap();
            assert_eq!(live_pid(&p, &tray_pid_file(&run)), None, "{bad:?}");
        }
    }
}
