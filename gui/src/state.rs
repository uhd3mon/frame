//! Paths shared with bin/clipped and the live state read from its pid files.

use std::path::{Path, PathBuf};

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from)
}

pub fn home() -> PathBuf {
    env_path("HOME").unwrap_or_else(|| PathBuf::from("/"))
}

pub fn uid() -> u32 {
    // SAFETY: getuid has no preconditions and cannot fail.
    unsafe { libc::getuid() }
}

/// bin/clipped line 9: ${XDG_RUNTIME_DIR:-${TMPDIR:-/tmp}/clipped-$UID}/clipped
pub fn rundir() -> PathBuf {
    match env_path("XDG_RUNTIME_DIR") {
        Some(d) => d.join("clipped"),
        None => env_path("TMPDIR")
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join(format!("clipped-{}", uid()))
            .join("clipped"),
    }
}

/// Create the runtime dir. Without XDG_RUNTIME_DIR it is under /tmp, so
/// (like ensure_rundir in bin/clipped) refuse a base we don't own.
pub fn ensure_rundir() -> Result<PathBuf, String> {
    use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
    let dir = rundir();
    if env_path("XDG_RUNTIME_DIR").is_none() {
        let base = dir.parent().unwrap_or(Path::new("/"));
        let _ = std::fs::DirBuilder::new().mode(0o700).create(base);
        let m = std::fs::symlink_metadata(base).map_err(|e| format!("{}: {e}", base.display()))?;
        if !m.is_dir() || m.uid() != uid() {
            return Err(format!("{} is not a directory owned by you; set XDG_RUNTIME_DIR", base.display()));
        }
        std::fs::set_permissions(base, std::fs::Permissions::from_mode(0o700)).ok();
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(dir)
}

pub fn config_path() -> PathBuf {
    if let Some(p) = env_path("CLIPPED_CONFIG") {
        return p;
    }
    config_home().join("clipped/config")
}

pub fn config_home() -> PathBuf {
    env_path("XDG_CONFIG_HOME").unwrap_or_else(|| home().join(".config"))
}

pub fn binds_state() -> PathBuf {
    env_path("XDG_STATE_HOME").unwrap_or_else(|| home().join(".local/state")).join("clipped/binds")
}

pub fn autostart_file() -> PathBuf {
    config_home().join("autostart/clipped-gui.desktop")
}

/// save_dir with ~ and ~/ expanded (only those, like expand_dir).
pub fn expand_dir(d: &str) -> PathBuf {
    if d == "~" {
        home()
    } else if let Some(rest) = d.strip_prefix("~/") {
        home().join(rest)
    } else {
        PathBuf::from(d)
    }
}

/// Field 22 (start time) of <proc>/PID/stat. The command name may hold
/// spaces and parentheses, so fields are counted after the last ") ".
pub fn proc_start(proc_root: &Path, pid: u32) -> Option<String> {
    let s = std::fs::read_to_string(proc_root.join(pid.to_string()).join("stat")).ok()?;
    let rest = &s[s.rfind(") ")? + 2..];
    rest.split_whitespace().nth(19).map(str::to_string)
}

/// A pid file: "PID STARTTIME" then extra lines (output file, seconds…).
#[derive(Debug, Clone, PartialEq)]
pub struct PidFile {
    pub pid: u32,
    pub extra: Vec<String>,
}

/// The pid file's process, if that same process still runs (live_pid).
pub fn live_pid(proc_root: &Path, file: &Path) -> Option<PidFile> {
    let text = std::fs::read_to_string(file).ok()?;
    let mut lines = text.lines();
    let mut first = lines.next()?.split_whitespace();
    let pid_s = first.next()?;
    let start = first.next()?;
    if !pid_s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let pid: u32 = pid_s.parse().ok()?;
    if proc_start(proc_root, pid)? != start {
        return None;
    }
    Some(PidFile { pid, extra: lines.map(str::to_string).collect() })
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Status {
    /// Output file of a running `clipped record`.
    pub recording: Option<String>,
    /// Replay buffer length in seconds, when the buffer runs.
    pub replay: Option<String>,
    pub daemon: bool,
}

impl Status {
    pub fn read(proc_root: &Path, rundir: &Path) -> Status {
        let rec = live_pid(proc_root, &rundir.join("record.pid"));
        let daemon = live_pid(proc_root, &rundir.join("daemon.pid")).is_some();
        let replay = if daemon { live_pid(proc_root, &rundir.join("replay.pid")) } else { None };
        Status {
            recording: rec.map(|p| p.extra.first().cloned().unwrap_or_default()),
            replay: replay.map(|p| p.extra.first().cloned().unwrap_or_default()),
            daemon,
        }
    }

    pub fn tooltip(&self) -> String {
        if let Some(f) = &self.recording {
            let name = Path::new(f).file_name().map(|n| n.to_string_lossy().into_owned());
            return format!("clipped: recording to {}", name.unwrap_or_else(|| f.clone()));
        }
        match (&self.replay, self.daemon) {
            (Some(s), _) if !s.is_empty() => format!("clipped: replay buffer {s} s"),
            (Some(_), _) => "clipped: replay buffer on".into(),
            (None, true) => "clipped: running, replay buffer off".into(),
            (None, false) => "clipped: not running".into(),
        }
    }
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
    fn pid_files_and_status() {
        let t = tempdir("pids");
        let (p, run) = (t.join("proc"), t.join("run"));
        std::fs::create_dir_all(&run).unwrap();
        fake_proc(&p, 100, "gpu-screen-recorder", "500");
        fake_proc(&p, 200, "bash", "600");
        fake_proc(&p, 201, "gpu screen) rec", "601");

        assert_eq!(Status::read(&p, &run).tooltip(), "clipped: not running");

        std::fs::write(run.join("daemon.pid"), "200 600\n").unwrap();
        std::fs::write(run.join("replay.pid"), "201 601\n60\n").unwrap();
        let s = Status::read(&p, &run);
        assert_eq!(s, Status { recording: None, replay: Some("60".into()), daemon: true });
        assert_eq!(s.tooltip(), "clipped: replay buffer 60 s");

        // Stale: same pid, other start time -> not live.
        std::fs::write(run.join("record.pid"), "100 499\n/v/a.mp4\ngsr\n").unwrap();
        assert_eq!(Status::read(&p, &run).recording, None);
        std::fs::write(run.join("record.pid"), "100 500\n/v/2026_record.mp4\ngsr\n").unwrap();
        let s = Status::read(&p, &run);
        assert_eq!(s.recording.as_deref(), Some("/v/2026_record.mp4"));
        assert_eq!(s.tooltip(), "clipped: recording to 2026_record.mp4");

        // Replay without a live daemon doesn't count.
        std::fs::remove_file(run.join("daemon.pid")).unwrap();
        assert_eq!(Status::read(&p, &run).replay, None);

        for bad in ["", "abc 500\n", "100\n", "-100 500\n", "100 500x\n"] {
            std::fs::write(run.join("record.pid"), bad).unwrap();
            assert_eq!(live_pid(&p, &run.join("record.pid")), None, "{bad:?}");
        }
    }
}
