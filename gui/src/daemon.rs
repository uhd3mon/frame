//! The tray owns `frame daemon` (shortcuts + replay buffer): it starts it,
//! restarts it with backoff when it exits, restarts it when a setting the
//! daemon reads only at start changes (the tray re-reads the config each
//! tick), and stops it when the tray quits. A daemon someone else runs
//! (frame.service, a terminal) is treated as running and left alone.

use crate::config;
use crate::state::live_pid;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

const FIRST_WAIT: Duration = Duration::from_secs(2);
const MAX_WAIT: Duration = Duration::from_secs(60);
/// A daemon that ran this long counts as healthy: the backoff starts over.
const HEALTHY: Duration = Duration::from_secs(60);
/// After TERM the daemon stops its recorder; this long, then KILL.
const STOP_WAIT: Duration = Duration::from_secs(10);
/// daemon.log is started over when it grows past this.
const LOG_MAX: u64 = 1 << 20;

/// What the tray shows about the daemon.
#[derive(Debug, Clone, PartialEq)]
pub enum View {
    /// Our daemon runs, started `up` ago.
    Running { up: Duration },
    /// Someone else's daemon (frame.service, a terminal).
    External,
    /// It exited or couldn't start; the next try is in `secs`.
    Retry { why: String, secs: u64 },
    /// `frame check` refused the config; tried again when the file changes.
    BadConfig(String),
    Stopped,
}

enum State {
    Running { child: Child, since: Instant },
    External,
    Retry { why: String, at: Instant },
    BadConfig(String),
    Stopped,
}

pub struct Supervisor {
    bin: PathBuf,
    rundir: PathBuf,
    config: PathBuf,
    proc_root: PathBuf,
    state: State,
    wait: Duration,
    /// DAEMON_KEYS values the daemon was started with.
    started_with: BTreeMap<String, String>,
    /// Config bytes at the last look (None: no file).
    seen: Option<Vec<u8>>,
    /// Restart an external daemon whose settings changed; true if it did.
    restart_external: Box<dyn FnMut() -> bool>,
}

impl Supervisor {
    pub fn new(bin: PathBuf, rundir: PathBuf, config: PathBuf, proc_root: PathBuf) -> Supervisor {
        Supervisor {
            bin,
            rundir,
            config,
            proc_root,
            state: State::Stopped,
            wait: FIRST_WAIT,
            started_with: BTreeMap::new(),
            seen: None,
            restart_external: Box::new(restart_service),
        }
    }

    fn values(&self) -> BTreeMap<String, String> {
        let mut m = config::parse(self.seen.as_deref().unwrap_or_default());
        m.retain(|k, _| config::DAEMON_KEYS.contains(&k.as_str()));
        m
    }

    /// The config asks for a replay buffer.
    pub fn replay_wanted(&self) -> bool {
        self.values().get("replay").is_some_and(|v| config::yes(v))
    }

    fn other_daemon(&self) -> bool {
        live_pid(&self.proc_root, &self.rundir.join("daemon.pid")).is_some()
    }

    pub fn view(&self, now: Instant) -> View {
        match &self.state {
            State::Running { since, .. } => View::Running { up: now.saturating_duration_since(*since) },
            State::External => View::External,
            State::Retry { why, at } => {
                View::Retry { why: why.clone(), secs: at.saturating_duration_since(now).as_secs_f32().ceil() as u64 }
            }
            State::BadConfig(m) => View::BadConfig(m.clone()),
            State::Stopped => View::Stopped,
        }
    }

    /// Start our daemon, unless one runs already or the config is refused.
    pub fn start(&mut self, now: Instant) {
        if self.other_daemon() {
            self.remember_config();
            self.state = State::External;
            return;
        }
        let check = Command::new(&self.bin).arg("check").stdin(Stdio::null()).stdout(Stdio::null()).output();
        // Read after `frame check`: it may have just copied the old Clipped
        // config into place, which must not look like a change next tick.
        self.remember_config();
        match check {
            Err(e) => return self.retry(now, format!("cannot run {}: {e}", self.bin.display())),
            // Exit 2 = the config is refused: retrying won't help until it changes.
            Ok(o) if o.status.code() == Some(2) => {
                self.state = State::BadConfig(last_line(&o.stderr).unwrap_or_else(|| "frame check failed".into()));
                return;
            }
            Ok(o) if !o.status.success() => {
                let why = last_line(&o.stderr).unwrap_or_else(|| describe(o.status));
                return self.retry(now, format!("frame check failed: {why}"));
            }
            Ok(_) => {}
        }
        match self.spawn() {
            Ok(child) => self.state = State::Running { child, since: now },
            Err(e) => self.retry(now, format!("cannot run {}: {e}", self.bin.display())),
        }
    }

    fn remember_config(&mut self) {
        self.seen = std::fs::read(&self.config).ok();
        self.started_with = self.values();
    }

    fn spawn(&self) -> std::io::Result<Child> {
        use std::os::unix::process::CommandExt;
        let _ = std::fs::create_dir_all(&self.rundir);
        let path = self.log();
        let mut opts = std::fs::OpenOptions::new();
        if std::fs::metadata(&path).is_ok_and(|m| m.len() > LOG_MAX) {
            opts.write(true).create(true).truncate(true);
        } else {
            opts.append(true).create(true);
        }
        let log = opts.open(&path)?;
        let mut cmd = Command::new(&self.bin);
        cmd.arg("daemon").stdin(Stdio::null()).stdout(log.try_clone()?).stderr(log);
        // Its own group (gsr and sleep join it) so a KILL after a stuck stop
        // reaches them; TERM if the tray dies without stopping it.
        cmd.process_group(0);
        // If the tray died between fork and prctl the signal never comes:
        // the child is then already reparented, so it exits instead.
        let parent = std::process::id() as libc::pid_t;
        // SAFETY: prctl, getppid and _exit are async-signal-safe and touch
        // no memory of ours.
        unsafe {
            cmd.pre_exec(move || {
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
                if libc::getppid() != parent {
                    libc::_exit(0);
                }
                Ok(())
            });
        }
        cmd.spawn()
    }

    fn log(&self) -> PathBuf {
        self.rundir.join("daemon.log")
    }

    fn retry(&mut self, now: Instant, why: String) {
        self.state = State::Retry { why, at: now + self.wait };
        self.wait = (self.wait * 2).min(MAX_WAIT);
    }

    /// Our daemon ended without being asked to.
    fn exited(&mut self, now: Instant, why: String, ran: Duration) {
        if self.other_daemon() {
            // e.g. frame.service won the daemon.lock.
            self.state = State::External;
            return;
        }
        if ran >= HEALTHY {
            self.wait = FIRST_WAIT;
        }
        self.retry(now, why);
    }

    /// Once a second or so: notice exits, config changes and a vanished
    /// external daemon.
    pub fn tick(&mut self, now: Instant) {
        let text = std::fs::read(&self.config).ok();
        let changed = text != self.seen;
        self.seen = text;
        let settings_changed = changed && self.values() != self.started_with;
        let (log, pidfile) = (self.log(), self.rundir.join("daemon.pid"));
        enum Do {
            Nothing,
            Restart,
            Start,
            Exited(String, Duration),
            RestartExternal,
        }
        let action = match &mut self.state {
            State::Running { child, since } => match child.try_wait() {
                Ok(None) if settings_changed => Do::Restart,
                Ok(None) => Do::Nothing,
                Ok(Some(st)) => {
                    let why = last_line(&tail(&log)).unwrap_or_else(|| describe(st));
                    Do::Exited(format!("daemon stopped: {why}"), now.saturating_duration_since(*since))
                }
                Err(e) => Do::Exited(format!("daemon lost: {e}"), Duration::ZERO),
            },
            State::External if live_pid(&self.proc_root, &pidfile).is_none() => Do::Start,
            State::External if settings_changed => Do::RestartExternal,
            State::External | State::Stopped => Do::Nothing,
            State::Retry { at, .. } if now >= *at || changed => Do::Start,
            State::BadConfig(_) if changed => Do::Start,
            State::Retry { .. } | State::BadConfig(_) => Do::Nothing,
        };
        match action {
            Do::Nothing => {}
            Do::Start => self.start(now),
            Do::Restart => {
                self.stop();
                self.wait = FIRST_WAIT;
                self.start(now);
            }
            Do::Exited(why, ran) => self.exited(now, why, ran),
            Do::RestartExternal => {
                if (self.restart_external)() {
                    self.started_with = self.values();
                }
            }
        }
    }

    /// TERM our daemon and wait for it (it stops the replay buffer and
    /// removes its pid files); KILL its group if it hangs.
    pub fn stop(&mut self) {
        if let State::Running { child, .. } = &mut self.state {
            let pid = child.id() as i32;
            // SAFETY: plain syscalls on a pid we spawned and haven't reaped.
            unsafe { libc::kill(pid, libc::SIGTERM) };
            let end = Instant::now() + STOP_WAIT;
            loop {
                match child.try_wait() {
                    Ok(Some(_)) | Err(_) => break,
                    Ok(None) if Instant::now() >= end => {
                        eprintln!("frame-gui: daemon (pid {pid}) did not stop; killing it");
                        unsafe { libc::kill(-pid, libc::SIGKILL) };
                        let _ = child.wait();
                        break;
                    }
                    Ok(None) => std::thread::sleep(Duration::from_millis(50)),
                }
            }
        }
        self.state = State::Stopped;
    }
}

fn describe(st: ExitStatus) -> String {
    use std::os::unix::process::ExitStatusExt;
    match (st.code(), st.signal()) {
        (Some(c), _) => format!("exit code {c}"),
        (_, Some(s)) => format!("killed by signal {s}"),
        _ => st.to_string(),
    }
}

/// The last non-empty line, without a leading "frame: ".
fn last_line(b: &[u8]) -> Option<String> {
    let s = String::from_utf8_lossy(b);
    let l = s.lines().rev().map(str::trim).find(|l| !l.is_empty())?;
    Some(l.strip_prefix("frame: ").unwrap_or(l).to_string())
}

/// The end of a file (the last 4 KiB), for the reason a daemon stopped.
fn tail(p: &Path) -> Vec<u8> {
    let b = std::fs::read(p).unwrap_or_default();
    b[b.len().saturating_sub(4096)..].to_vec()
}

/// frame.service runs the daemon: restart it so it picks up the change.
fn restart_service() -> bool {
    service_active()
        && Command::new("systemctl")
            .args(["--user", "restart", "frame.service"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
}

fn service_active() -> bool {
    Command::new("systemctl")
        .args(["--user", "is-active", "--quiet", "frame.service"])
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::tempdir;
    use std::os::unix::fs::PermissionsExt;

    /// A stand-in for bin/frame: `check` copies MIGRATE to a missing config
    /// (like the Clipped config copy) and exits 2 if the config holds "bad";
    /// `daemon` logs its start, writes daemon.pid (pid + start time, as
    /// bin/frame does), and exits 0 on TERM or 1 by itself if FAKE_EXIT exists.
    fn fake_frame(d: &Path) -> PathBuf {
        let bin = d.join("frame");
        let script = format!(
            r#"#!/bin/sh
D='{d}'
case "$1" in
check)
    [ -e "$D/MIGRATE" ] && [ ! -e "$D/config" ] && cp "$D/MIGRATE" "$D/config"
    grep -q bad "$D/config" 2>/dev/null && {{ echo "frame: video_fps must be 1-240" >&2; exit 2; }}; echo ok ;;
daemon)
    echo start >>"$D/starts"
    set -- $(cut -d' ' -f22 /proc/$$/stat)
    echo "$$ $1" >"$D/run/daemon.pid"
    trap 'echo term >>"$D/starts"; rm -f "$D/run/daemon.pid"; exit 0' TERM
    while :; do
        [ -e "$D/FAKE_EXIT" ] && {{ rm -f "$D/FAKE_EXIT" "$D/run/daemon.pid"; echo "frame: replay buffer gone" >&2; exit 1; }}
        sleep 0.05
    done ;;
esac
"#,
            d = d.display()
        );
        std::fs::write(&bin, script).unwrap();
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::create_dir_all(d.join("run")).unwrap();
        wait_execable(&bin);
        bin
    }

    /// A child that another test thread forked while our write fd was open
    /// holds a copy until it execs, and exec of BIN fails with ETXTBSY
    /// meanwhile (a rename wouldn't help: same inode). Our fd is closed, so
    /// no later fork can inherit it: once one exec works, all will.
    fn wait_execable(bin: &Path) {
        for _ in 0..200 {
            match Command::new(bin).stdout(Stdio::null()).status() {
                Err(e) if e.raw_os_error() == Some(libc::ETXTBSY) => std::thread::sleep(Duration::from_millis(5)),
                r => return assert!(r.is_ok_and(|s| s.success()), "{} won't run", bin.display()),
            }
        }
        panic!("{} stayed busy", bin.display());
    }

    fn sup(d: &Path) -> Supervisor {
        let mut s = Supervisor::new(fake_frame(d), d.join("run"), d.join("config"), PathBuf::from("/proc"));
        s.restart_external = Box::new(|| panic!("no external daemon in this test"));
        s
    }

    fn starts(d: &Path) -> String {
        std::fs::read_to_string(d.join("starts")).unwrap_or_default().replace('\n', " ").trim().to_string()
    }

    /// Waits until the Nth fake daemon is up (logged, pid file, TERM trap set).
    fn up(d: &Path, n: usize) {
        for _ in 0..200 {
            if starts(d).matches("start").count() >= n && d.join("run/daemon.pid").exists() {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("fake daemon {n} never started: {}", starts(d));
    }

    /// Ticks (sleeping a little each time) until F holds, or panics.
    fn until(s: &mut Supervisor, now: Instant, what: &str, f: impl Fn(&Supervisor) -> bool) {
        for _ in 0..100 {
            s.tick(now);
            if f(s) {
                return;
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        panic!("never: {what}: {:?}", s.view(now));
    }

    #[test]
    fn starts_restarts_with_backoff_and_stops() {
        let d = tempdir("sup");
        std::fs::write(d.join("config"), "replay = true\n").unwrap();
        let mut s = sup(&d);
        let t0 = Instant::now();
        s.start(t0);
        assert!(matches!(s.view(t0), View::Running { .. }), "{:?}", s.view(t0));
        assert!(s.replay_wanted());
        up(&d, 1);

        // It exits by itself: retry in 2 s (with the log's last line), then 4 s.
        std::fs::write(d.join("FAKE_EXIT"), "").unwrap();
        until(&mut s, t0, "exit noticed", |s| matches!(s.view(t0), View::Retry { .. }));
        assert_eq!(s.view(t0), View::Retry { why: "daemon stopped: replay buffer gone".into(), secs: 2 });
        s.tick(t0 + Duration::from_secs(1));
        assert!(matches!(s.view(t0), View::Retry { .. }), "too early");
        let t1 = t0 + Duration::from_secs(2);
        s.tick(t1);
        assert!(matches!(s.view(t1), View::Running { .. }));
        up(&d, 2);
        std::fs::write(d.join("FAKE_EXIT"), "").unwrap();
        until(&mut s, t1, "second exit", |s| matches!(s.view(t1), View::Retry { .. }));
        assert!(matches!(s.view(t1), View::Retry { secs: 4, .. }), "{:?}", s.view(t1));
        let t2 = t1 + Duration::from_secs(4);
        s.tick(t2);
        assert!(matches!(s.view(t2), View::Running { .. }));
        up(&d, 3);

        // Quit: TERM, waited for, pid file gone.
        s.stop();
        assert_eq!(s.view(t2), View::Stopped);
        assert_eq!(starts(&d), "start start start term");
        assert!(!d.join("run/daemon.pid").exists());
        s.tick(t2 + MAX_WAIT);
        assert_eq!(starts(&d), "start start start term", "stopped means stopped");
    }

    #[test]
    fn daemon_settings_change_restarts_others_dont() {
        let d = tempdir("sup-cfg");
        std::fs::write(d.join("config"), "replay = true\nimage_format = png\n").unwrap();
        let mut s = sup(&d);
        let t = Instant::now();
        s.start(t);
        up(&d, 1);
        s.tick(t);
        // Not a daemon key: nothing happens.
        std::fs::write(d.join("config"), "replay = true\nimage_format = jpg\nbind.all = Meta+F1\n").unwrap();
        s.tick(t);
        assert_eq!(starts(&d), "start");
        // Replay off: restarted (the new daemon runs without a buffer).
        std::fs::write(d.join("config"), "replay = false\nimage_format = jpg\n").unwrap();
        s.tick(t);
        up(&d, 2);
        assert_eq!(starts(&d), "start term start");
        assert!(!s.replay_wanted() && matches!(s.view(t), View::Running { .. }));
        // Same values, file rewritten (a comment added): no restart.
        std::fs::write(d.join("config"), "replay = false   # off\nimage_format = jpg\n").unwrap();
        s.tick(t);
        assert_eq!(starts(&d), "start term start");
        s.stop();
    }

    #[test]
    fn config_copied_by_check_is_not_a_change() {
        let d = tempdir("sup-migrate");
        std::fs::write(d.join("MIGRATE"), "replay = false\n").unwrap();
        let mut s = sup(&d);
        let t = Instant::now();
        s.start(t);
        up(&d, 1);
        assert!(d.join("config").exists() && !s.replay_wanted());
        s.tick(t);
        s.tick(t);
        assert_eq!(starts(&d), "start", "no restart for the copied config");
        s.stop();
    }

    #[test]
    fn bad_config_waits_for_a_change() {
        let d = tempdir("sup-bad");
        std::fs::write(d.join("config"), "video_fps = bad\n").unwrap();
        let mut s = sup(&d);
        let t = Instant::now();
        s.start(t);
        assert_eq!(s.view(t), View::BadConfig("video_fps must be 1-240".into()));
        s.tick(t + Duration::from_secs(600));
        assert_eq!(starts(&d), "", "no spinning on a refused config");
        std::fs::write(d.join("config"), "video_fps = 30\n").unwrap();
        s.tick(t + Duration::from_secs(601));
        assert!(matches!(s.view(t), View::Running { .. }));
        up(&d, 1);
        s.stop();
        assert_eq!(starts(&d), "start term");
    }

    #[test]
    fn someone_elses_daemon_is_left_alone() {
        use std::sync::atomic::{AtomicU32, Ordering};
        static RESTARTS: AtomicU32 = AtomicU32::new(0);
        let d = tempdir("sup-ext");
        std::fs::write(d.join("config"), "video_fps = 30\n").unwrap();
        // "frame.service": a live process in daemon.pid.
        let mut other = Command::new("sleep").arg("30").spawn().unwrap();
        let start = crate::state::proc_start(Path::new("/proc"), other.id()).unwrap();
        std::fs::create_dir_all(d.join("run")).unwrap();
        let mut s = sup(&d);
        std::fs::write(d.join("run/daemon.pid"), format!("{} {start}\n", other.id())).unwrap();
        s.restart_external = Box::new(|| {
            RESTARTS.fetch_add(1, Ordering::SeqCst);
            true
        });
        let t = Instant::now();
        s.start(t);
        assert_eq!(s.view(t), View::External);
        std::fs::write(d.join("config"), "video_fps = 60\n").unwrap();
        s.tick(t);
        assert_eq!(RESTARTS.load(Ordering::SeqCst), 1, "service asked to restart");
        s.stop(); // not ours: nothing to stop
        assert!(other.try_wait().unwrap().is_none(), "the other daemon was signalled");
        // It goes away: ours takes over.
        let mut s = sup(&d);
        s.start(t);
        other.kill().unwrap();
        other.wait().unwrap();
        s.tick(t);
        assert!(matches!(s.view(t), View::Running { .. }), "{:?}", s.view(t));
        up(&d, 1);
        s.stop();
        assert_eq!(starts(&d), "start term");
    }
}
