//! What a frame command reads from its surroundings: environment, paths,
//! the dry-run switch, where output goes. Commands take a `Ctx` instead of
//! reading the process environment, so tests can run them against temp
//! dirs and fake tools in parallel.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub struct Ctx {
    env: BTreeMap<OsString, OsString>,
    /// The frame binary that shortcuts and toasts run.
    pub exe: PathBuf,
    /// Tests: stdout and stderr go here instead of the terminal.
    cap: Option<RefCell<(String, String)>>,
}

impl Ctx {
    /// The real process: its environment and its own binary.
    pub fn process() -> Ctx {
        let exe = std::env::var_os("FRAME_BIN")
            .filter(|p| !p.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::current_exe().ok())
            .unwrap_or_else(|| PathBuf::from("frame"));
        Ctx { env: std::env::vars_os().collect(), exe, cap: None }
    }

    /// A context with exactly ENV whose output is captured (tests, and the
    /// settings window, which shows `frame bind`'s warnings).
    pub fn captured(env: BTreeMap<OsString, OsString>, exe: PathBuf) -> Ctx {
        Ctx { env, exe, cap: Some(RefCell::new((String::new(), String::new()))) }
    }

    /// The process context, with output captured.
    pub fn process_captured() -> Ctx {
        let p = Ctx::process();
        Ctx::captured(p.env, p.exe)
    }

    /// (stdout, stderr) captured so far.
    pub fn output(&self) -> (String, String) {
        self.cap.as_ref().map(|c| c.borrow().clone()).unwrap_or_default()
    }

    /// A non-empty environment variable.
    pub fn var_os(&self, k: &str) -> Option<&OsStr> {
        self.env.get(OsStr::new(k)).map(OsString::as_os_str).filter(|v| !v.is_empty())
    }

    pub fn var(&self, k: &str) -> String {
        self.var_os(k).map(|v| v.to_string_lossy().into_owned()).unwrap_or_default()
    }

    fn path_var(&self, k: &str) -> Option<PathBuf> {
        self.var_os(k).map(PathBuf::from)
    }

    pub fn dry(&self) -> bool {
        self.var("FRAME_DRY_RUN") == "1"
    }

    /// A line on stdout.
    pub fn say(&self, s: &str) {
        match &self.cap {
            Some(c) => {
                let o = &mut c.borrow_mut().0;
                o.push_str(s);
                o.push('\n');
            }
            None => println!("{s}"),
        }
    }

    /// "frame: MSG" on stderr.
    pub fn warn(&self, s: &str) {
        match &self.cap {
            Some(c) => {
                let e = &mut c.borrow_mut().1;
                e.push_str("frame: ");
                e.push_str(s);
                e.push('\n');
            }
            None => eprintln!("frame: {s}"),
        }
    }

    /// Raw text on stderr (no "frame: " prefix), e.g. the usage.
    pub fn err_raw(&self, s: &str) {
        match &self.cap {
            Some(c) => c.borrow_mut().1.push_str(s),
            None => eprint!("{s}"),
        }
    }

    /// The session bus named by our environment: $DBUS_SESSION_BUS_ADDRESS,
    /// else $XDG_RUNTIME_DIR/bus. Never the process's own environment, so a
    /// test Ctx (temp XDG_RUNTIME_DIR, no address) can't reach the real bus.
    pub fn session_bus(&self) -> Result<zbus::blocking::Connection, String> {
        let addr = match self.var_os("DBUS_SESSION_BUS_ADDRESS") {
            Some(a) => a.to_string_lossy().into_owned(),
            None => match self.path_var("XDG_RUNTIME_DIR") {
                Some(d) => format!("unix:path={}", d.join("bus").display()),
                None => return Err("no D-Bus session (DBUS_SESSION_BUS_ADDRESS and XDG_RUNTIME_DIR unset)".into()),
            },
        };
        zbus::blocking::connection::Builder::address(addr.as_str())
            .and_then(|b| b.build())
            .map_err(|e| format!("D-Bus session: {e}"))
    }

    // ------------------------------------------------------------ paths

    pub fn home(&self) -> PathBuf {
        self.path_var("HOME").unwrap_or_else(|| PathBuf::from("/"))
    }

    pub fn config_home(&self) -> PathBuf {
        self.path_var("XDG_CONFIG_HOME").unwrap_or_else(|| self.home().join(".config"))
    }

    pub fn data_home(&self) -> PathBuf {
        self.path_var("XDG_DATA_HOME").unwrap_or_else(|| self.home().join(".local/share"))
    }

    /// $FRAME_CONFIG, else ~/.config/frame/config.
    pub fn config_path(&self) -> PathBuf {
        self.path_var("FRAME_CONFIG").unwrap_or_else(|| self.config_home().join("frame/config"))
    }

    /// Shortcuts we bound, per backend ("backend action shortcut" lines).
    /// KDE and GNOME binds survive a reboot, hence not the runtime dir.
    pub fn binds_state(&self) -> PathBuf {
        self.path_var("XDG_STATE_HOME").unwrap_or_else(|| self.home().join(".local/state")).join("frame/binds")
    }

    pub fn autostart_file(&self) -> PathBuf {
        self.config_home().join("autostart/frame.desktop")
    }

    /// ${XDG_RUNTIME_DIR:-${TMPDIR:-/tmp}/frame-$UID}/frame: pid file and locks.
    pub fn rundir(&self) -> PathBuf {
        match self.path_var("XDG_RUNTIME_DIR") {
            Some(d) => d.join("frame"),
            None => self.tmp_dir().join(format!("frame-{}", uid())).join("frame"),
        }
    }

    /// $XDG_RUNTIME_DIR (where the Wayland socket lives).
    pub fn runtime_dir(&self) -> Option<PathBuf> {
        self.path_var("XDG_RUNTIME_DIR")
    }

    pub fn tmp_dir(&self) -> PathBuf {
        self.path_var("TMPDIR").unwrap_or_else(|| PathBuf::from("/tmp"))
    }

    /// Create the runtime dir. Without XDG_RUNTIME_DIR it is under /tmp,
    /// so refuse a base someone else owns or planted (a symlink).
    pub fn ensure_rundir(&self) -> Result<PathBuf, String> {
        use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
        let dir = self.rundir();
        if self.var_os("XDG_RUNTIME_DIR").is_none() {
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

    /// save_dir with ~ and ~/ expanded (only those; not ~user, not $VARS).
    pub fn expand_dir(&self, d: &str) -> PathBuf {
        if d == "~" {
            self.home()
        } else if let Some(rest) = d.strip_prefix("~/") {
            self.home().join(rest)
        } else {
            PathBuf::from(d)
        }
    }

    // --------------------------------------------------------- programs

    /// PROG on our PATH, if it is there and executable.
    pub fn which(&self, prog: &str) -> Option<PathBuf> {
        if prog.contains('/') {
            return is_exec(Path::new(prog)).then(|| PathBuf::from(prog));
        }
        std::env::split_paths(self.var_os("PATH")?).map(|d| d.join(prog)).find(|c| is_exec(c))
    }

    pub fn have(&self, prog: &str) -> bool {
        self.which(prog).is_some()
    }

    /// A Command for PROG that sees exactly our environment.
    pub fn command(&self, prog: &str) -> Command {
        let mut c = Command::new(self.which(prog).unwrap_or_else(|| PathBuf::from(prog)));
        c.env_clear().envs(&self.env).stdin(Stdio::null());
        c
    }

    /// Run ARGV (stdout shown), or print it in dry-run.
    pub fn run(&self, argv: &[&str]) -> Result<(), String> {
        self.run_with(argv, false)
    }

    /// Like run, with stdout hidden.
    pub fn runq(&self, argv: &[&str]) -> Result<(), String> {
        self.run_with(argv, true)
    }

    fn run_with(&self, argv: &[&str], quiet: bool) -> Result<(), String> {
        if self.dry() {
            self.say(&format!("+ {}", quote(argv)));
            return Ok(());
        }
        let mut c = self.command(argv[0]);
        c.args(&argv[1..]);
        if quiet {
            c.stdout(Stdio::null());
        }
        match c.status() {
            Ok(s) if s.success() => Ok(()),
            Ok(s) => Err(format!("{} failed ({s})", argv[0])),
            Err(e) => Err(format!("cannot run {}: {e}", argv[0])),
        }
    }

    /// Run a query (also in dry-run) and return its stdout if it succeeded.
    pub fn query(&self, argv: &[&str]) -> Option<String> {
        let o = self.command(argv[0]).args(&argv[1..]).stderr(Stdio::null()).output().ok()?;
        o.status.success().then(|| String::from_utf8_lossy(&o.stdout).into_owned())
    }
}

pub fn uid() -> u32 {
    // SAFETY: getuid has no preconditions and cannot fail.
    unsafe { libc::getuid() }
}

pub fn is_exec(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.metadata().map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0).unwrap_or(false)
}

/// Shell-quote only when needed, so dry-run output stays readable.
pub fn quote(argv: &[&str]) -> String {
    argv.iter().map(|a| quote1(a)).collect::<Vec<_>>().join(" ")
}

pub fn quote1(a: &str) -> String {
    let plain = !a.is_empty() && a.bytes().all(|b| b.is_ascii_alphanumeric() || b"_./:=+@%,-".contains(&b));
    if plain {
        a.to_string()
    } else {
        format!("'{}'", a.replace('\'', r"'\''"))
    }
}

/// Percent-encode a path byte by byte (UTF-8 names come out right).
pub fn file_uri(p: &Path) -> String {
    let mut s = String::from("file://");
    for &b in p.as_os_str().as_bytes() {
        if b.is_ascii_alphanumeric() || b"/._~-".contains(&b) {
            s.push(b as char);
        } else {
            s.push_str(&format!("%{b:02X}"));
        }
    }
    s
}

/// A desktop-entry Exec= argument: \ " ` $ escaped inside double quotes,
/// then the key-file \ doubling, and % as %%. Plain paths stay bare.
pub fn desktop_arg(s: &str) -> String {
    if !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"/._+-".contains(&b)) {
        return s.to_string();
    }
    let mut q = String::from("\"");
    for c in s.chars() {
        if matches!(c, '\\' | '"' | '`' | '$') {
            q.push('\\');
        }
        q.push(c);
    }
    q.push('"');
    q.replace('\\', r"\\").replace('%', "%%")
}

/// An exclusive flock on a file, unlocked explicitly when dropped. A flock
/// belongs to the open file, not the fd: any spawn in this process (another
/// thread's, too) copies our fd into the child until it execs, and merely
/// closing ours would leave the lock held by that copy for a moment.
/// LOCK_UN releases it for every copy.
pub struct Flock(std::fs::File);

impl Flock {
    /// Ok(None) if someone else holds the lock.
    pub fn try_new(f: &Path) -> std::io::Result<Option<Flock>> {
        let file = std::fs::File::create(f)?;
        match file.try_lock() {
            Ok(()) => Ok(Some(Flock(file))),
            Err(std::fs::TryLockError::WouldBlock) => Ok(None),
            Err(std::fs::TryLockError::Error(e)) => Err(e),
        }
    }
}

impl Drop for Flock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

/// Start a command in its own session, without waiting for it; a thread
/// reaps it so no zombie is left (the thread dies with us otherwise).
pub fn spawn_detached(cmd: &mut Command) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    // SAFETY: setsid is async-signal-safe; nothing else runs between fork and exec.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let mut child = cmd.spawn()?;
    std::thread::spawn(move || child.wait());
    Ok(())
}

#[cfg(test)]
pub mod testenv {
    //! A dry-run Ctx on temp dirs, with fake tools first on PATH.
    use super::*;
    use crate::testutil::{tempdir, TempDir};

    pub struct T {
        pub dir: TempDir,
        pub env: BTreeMap<OsString, OsString>,
    }

    impl T {
        pub fn new(tag: &str) -> T {
            let dir = tempdir(tag);
            let mut env = BTreeMap::new();
            for sub in ["run", "fake", "cfg", "state", "data", "cache", "tmp", "home"] {
                std::fs::create_dir_all(dir.join(sub)).unwrap();
            }
            let p = |s: &str| dir.join(s).into_os_string();
            let fake = dir.join("fake");
            for (k, v) in [
                ("HOME", p("home")),
                ("FRAME_CONFIG", p("config")),
                ("XDG_RUNTIME_DIR", p("run")),
                ("XDG_CONFIG_HOME", p("cfg")),
                ("XDG_STATE_HOME", p("state")),
                ("XDG_DATA_HOME", p("data")),
                ("XDG_CACHE_HOME", p("cache")),
                ("TMPDIR", p("tmp")),
                ("FRAME_DRY_RUN", "1".into()),
                ("XDG_SESSION_TYPE", "wayland".into()),
                ("XDG_CURRENT_DESKTOP", "KDE".into()),
                // Only the fakes: no real screenshot, clipboard or shortcut
                // tool can ever run. Fakes call other programs by full path.
                ("PATH", fake.into_os_string()),
            ] {
                env.insert(OsString::from(k), v);
            }
            T { dir, env }
        }

        pub fn set(&mut self, k: &str, v: impl Into<OsString>) -> &mut Self {
            self.env.insert(k.into(), v.into());
            self
        }

        pub fn unset(&mut self, k: &str) -> &mut Self {
            self.env.remove(OsStr::new(k));
            self
        }

        pub fn ctx(&self) -> Ctx {
            Ctx::captured(self.env.clone(), PathBuf::from("/opt/frame/bin/frame"))
        }

        /// Config = save_dir in the temp dir plus LINES.
        pub fn conf(&self, lines: &[&str]) {
            let mut t = format!("save_dir = {}/docs\n", self.dir.display());
            for l in lines {
                t.push_str(l);
                t.push('\n');
            }
            std::fs::write(self.dir.join("config"), t).unwrap();
        }

        /// A fake tool: a shell script with BODY.
        pub fn fake(&self, name: &str, body: &str) {
            use std::os::unix::fs::PermissionsExt;
            let f = self.dir.join("fake").join(name);
            std::fs::write(&f, format!("#!/bin/sh\n{body}\n")).unwrap();
            std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        /// Run `frame ARGS` in this context: (exit code, stdout, stderr).
        pub fn frame(&self, args: &[&str]) -> (i32, String, String) {
            let ctx = self.ctx();
            let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
            let code = crate::cli::run(&ctx, &args);
            let (o, e) = ctx.output();
            (code, o, e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting() {
        assert_eq!(quote(&["a", "b c", "it's", "", "x=y,z"]), r"a 'b c' 'it'\''s' '' x=y,z");
    }

    #[test]
    fn uri_encodes_utf8_bytes() {
        assert_eq!(file_uri(Path::new("/h/é Д")), "file:///h/%C3%A9%20%D0%94");
    }

    #[test]
    fn exec_args() {
        assert_eq!(desktop_arg("/usr/bin/frame"), "/usr/bin/frame");
        assert_eq!(desktop_arg("/opt/my frame/frame"), "\"/opt/my frame/frame\"");
        assert_eq!(desktop_arg("/a$b\\c%d"), r#""/a\\$b\\\\c%%d""#);
    }

    #[test]
    fn rundir_refuses_planted_tmp_dir() {
        let mut t = testenv::T::new("rundir");
        let tp = t.dir.join("tp");
        std::fs::create_dir_all(&tp).unwrap();
        std::fs::create_dir_all(t.dir.join("elsewhere")).unwrap();
        std::os::unix::fs::symlink(t.dir.join("elsewhere"), tp.join(format!("frame-{}", uid()))).unwrap();
        t.unset("XDG_RUNTIME_DIR").set("TMPDIR", tp.clone());
        let e = t.ctx().ensure_rundir().unwrap_err();
        assert!(e.contains("not a directory owned by you"), "{e}");

        use std::os::unix::fs::PermissionsExt;
        let tp2 = t.dir.join("tp2");
        std::fs::create_dir_all(&tp2).unwrap();
        t.set("TMPDIR", tp2.clone());
        t.ctx().ensure_rundir().unwrap();
        let m = std::fs::metadata(tp2.join(format!("frame-{}", uid()))).unwrap();
        assert_eq!(m.permissions().mode() & 0o777, 0o700);
    }
}
