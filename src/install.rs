//! `frame install` / `frame uninstall`: put this binary under PREFIX
//! (default ~/.local) with its menu entry and icons, or take them away.
//! The AppImage installs itself this way (KWin <= 6.7 only lets a binary
//! named by a .desktop Exec take screenshots, and the AppImage's own
//! binary lives in a temporary mount); install.sh builds and runs it.
//!
//!   frame install [--force] [--no-autostart] [--from FILE]
//!   frame uninstall
//!
//! Installed: bin/frame, share/applications/frame.desktop,
//! share/icons/hicolor/{scalable,NxN}/apps/frame.{svg,png}, and the list
//! of them in share/frame/installed (uninstall removes only what that list
//! names). $DESTDIR (packaging) goes in front of every path; the files
//! still refer to $PREFIX, and nothing outside DESTDIR is touched.
//!
//! A real install (no DESTDIR) also:
//! - stops a running Frame first (its tray holds gui.lock and the old
//!   binary): the tray and settings window only, never a capture,
//!   `frame _clip` (it serves the clipboard) or `frame _toast`;
//! - removes an old Clipped install (share/clipped/installed), Frame 0.3's
//!   video service and replay/record shortcut entries, and 0.4's
//!   bin/frame-gui (its autostart entry becomes ours);
//! - on a first install turns "Start Frame when I log in" on; a login
//!   entry that runs another Frame of ours (the AppImage) is pointed at
//!   the installed one;
//! - starts the new tray if one was running or Frame starts at login.

use crate::ctx::{desktop_arg, Ctx};
use crate::state::live_pid;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

const MANIFEST: &str = "share/frame/installed";
const OLD_MANIFEST: &str = "share/clipped/installed";
const SVG: &[u8] = include_bytes!("../icons/frame.svg");
const PNGS: &[(u32, &[u8])] = &[
    (16, include_bytes!("../icons/png/frame-16.png")),
    (22, include_bytes!("../icons/png/frame-22.png")),
    (24, include_bytes!("../icons/png/frame-24.png")),
    (32, include_bytes!("../icons/png/frame-32.png")),
    (48, include_bytes!("../icons/png/frame-48.png")),
    (64, include_bytes!("../icons/png/frame-64.png")),
    (128, include_bytes!("../icons/png/frame-128.png")),
    (256, include_bytes!("../icons/png/frame-256.png")),
];
/// Variables the AppImage's AppRun and runtime set. A tray started from
/// the installed binary must not inherit them (FRAME_BIN would point its
/// shortcuts back at the AppImage).
const APPIMAGE_VARS: &[&str] = &["FRAME_BIN", "APPIMAGE", "APPDIR", "ARGV0", "OWD"];

pub const USAGE: &str = "usage: frame install [--force] [--no-autostart] [--from FILE] | frame uninstall";

/// Where things go: PREFIX as the files name it, ROOT = DESTDIR + PREFIX
/// where they are written.
pub struct Layout {
    pub prefix: PathBuf,
    pub root: PathBuf,
    /// No DESTDIR: this is the user's real install.
    pub live: bool,
}

impl Layout {
    pub fn new(ctx: &Ctx) -> Result<Layout, String> {
        let prefix = match ctx.var_os("PREFIX") {
            Some(p) => PathBuf::from(p),
            None => ctx.home().join(".local"),
        };
        if !prefix.is_absolute() {
            return Err(format!("PREFIX must be absolute (got '{}')", prefix.display()));
        }
        let prefix: PathBuf = prefix.components().collect(); // drops a trailing /
        let dest = ctx.var("DESTDIR");
        let root = if dest.is_empty() {
            prefix.clone()
        } else {
            let mut r = std::ffi::OsString::from(dest.trim_end_matches('/'));
            r.push(prefix.as_os_str());
            PathBuf::from(r)
        };
        Ok(Layout { prefix, root, live: dest.is_empty() })
    }

    /// The installed binary, as shortcuts and menu entries name it.
    pub fn bin(&self) -> PathBuf {
        self.prefix.join("bin/frame")
    }

    fn at(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    fn listed(&self, list: &str, rel: &str) -> bool {
        std::fs::read_to_string(self.at(list)).is_ok_and(|t| t.lines().any(|l| l == rel))
    }
}

/// Is frame installed under the default PREFIX (manifest lists bin/frame
/// and it is there)? The installed binary, if so.
pub fn installed(ctx: &Ctx) -> Option<PathBuf> {
    let l = Layout::new(ctx).ok()?;
    (l.listed(MANIFEST, "bin/frame") && crate::ctx::is_exec(&l.at("bin/frame"))).then(|| l.bin())
}

/// The binary a login entry should start: the installed one when this is
/// the AppImage and Frame is installed, else $APPIMAGE (never the binary
/// in its temporary /tmp/.mount_* dir), else this binary.
pub fn stable_exe(ctx: &Ctx) -> Result<PathBuf, String> {
    if let Some(app) = ctx.var_os("APPIMAGE") {
        return Ok(installed(ctx).unwrap_or_else(|| PathBuf::from(app)));
    }
    let exe = ctx.exe.canonicalize().unwrap_or_else(|_| ctx.exe.clone());
    if let Some(dir) = ctx.var_os("APPDIR") {
        if exe.starts_with(dir) {
            return Err(format!("{} is inside the AppImage's temporary mount; run the .AppImage itself", exe.display()));
        }
    }
    Ok(exe)
}

/// The menu entry. KWin <= 6.7 lets frame use ScreenShot2 only because a
/// .desktop entry names it: Exec's first word must be the absolute final
/// path (PREFIX, never DESTDIR) of the running binary. No arguments = tray
/// + settings window. (KWin 6.8 no longer checks; the key stays.)
pub fn desktop_entry(bin: &Path) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=Frame\nGenericName=Screenshot Tool\nComment=Screenshots with global shortcuts\n\
         Exec={}\nIcon=frame\nTerminal=false\nCategories=Utility;\nKeywords=screenshot;screen;capture;snip;\n\
         StartupWMClass=frame\nX-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2\n",
        desktop_arg(&bin.to_string_lossy())
    )
}

/// The login entry (also what the settings checkbox writes).
pub fn autostart_entry(exe: &str) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=Frame\nComment=Frame tray: screenshot shortcuts\nExec={} tray\nIcon=frame\n\
         Terminal=false\nX-GNOME-Autostart-enabled=true\n",
        desktop_arg(exe)
    )
}

/// Every installed path, relative to ROOT.
fn files() -> Vec<String> {
    let mut v = vec!["bin/frame".to_string(), "share/applications/frame.desktop".into(), "share/icons/hicolor/scalable/apps/frame.svg".into()];
    v.extend(PNGS.iter().map(|(s, _)| format!("share/icons/hicolor/{s}x{s}/apps/frame.png")));
    v
}

// ---------------------------------------------------------------- command

/// `frame install ...` / `frame uninstall`.
pub fn run(ctx: &Ctx, args: &[String]) -> i32 {
    let (cmd, rest) = args.split_first().map(|(c, r)| (c.as_str(), r)).unwrap_or(("", &[]));
    let mut opts = Opts { force: false, autostart: true, from: None };
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        match (cmd, a.as_str()) {
            ("install", "--force") => opts.force = true,
            ("install", "--no-autostart") => opts.autostart = false,
            ("install", "--from") => match it.next() {
                Some(f) => opts.from = Some(PathBuf::from(f)),
                None => return usage(ctx, "--from needs a FILE"),
            },
            (_, "-h" | "--help") => {
                ctx.say(USAGE);
                return 0;
            }
            _ => return usage(ctx, &format!("unknown option: {a}")),
        }
    }
    let r = Layout::new(ctx).and_then(|l| if cmd == "install" { install(ctx, &l, &opts) } else { uninstall(ctx, &l) });
    match r {
        Ok(()) => 0,
        Err(e) => {
            ctx.warn(&e);
            1
        }
    }
}

fn usage(ctx: &Ctx, msg: &str) -> i32 {
    ctx.warn(msg);
    ctx.err_raw(&format!("{USAGE}\n"));
    2
}

pub struct Opts {
    pub force: bool,
    pub autostart: bool,
    /// The binary to install; default this one (from the AppImage: the
    /// binary in its mount, which runs fine on its own).
    pub from: Option<PathBuf>,
}

pub fn install(ctx: &Ctx, l: &Layout, opts: &Opts) -> Result<(), String> {
    let src = match &opts.from {
        Some(f) => f.clone(),
        None => std::env::current_exe().map_err(|e| format!("can't find this binary: {e}"))?,
    };
    if !crate::ctx::is_exec(&src) {
        return Err(format!("{} is not an executable file", src.display()));
    }
    let first = !l.at(MANIFEST).exists();
    let bin = l.at("bin/frame");
    if bin.symlink_metadata().is_ok() && !l.listed(MANIFEST, "bin/frame") && !opts.force {
        return Err(format!(
            "{} exists and was not installed by frame (not in {}); move it away or use --force",
            bin.display(),
            l.at(MANIFEST).display()
        ));
    }
    let src = std::fs::read(&src).map_err(|e| format!("{}: {e}", src.display()))?;
    let mut tray_was_running = false;
    if l.live {
        tray_was_running = stop_running(ctx, l);
        remove_clipped(ctx, l);
        remove_video_era(ctx);
        remove_stale(ctx, l);
    }
    // New files by rename: a running old binary keeps its inode, and a
    // failed write never leaves half a file.
    put(&bin, &src, 0o755)?;
    put(&l.at("share/icons/hicolor/scalable/apps/frame.svg"), SVG, 0o644)?;
    for (s, png) in PNGS {
        put(&l.at(&format!("share/icons/hicolor/{s}x{s}/apps/frame.png")), png, 0o644)?;
    }
    put(&l.at("share/applications/frame.desktop"), desktop_entry(&l.bin()).as_bytes(), 0o644)?;
    let list: String = files().iter().map(|f| format!("{f}\n")).collect();
    put(&l.at(MANIFEST), list.as_bytes(), 0o644)?;
    if l.live {
        refresh_caches(ctx, l);
    }
    for f in files() {
        ctx.say(&format!("installed {}", l.at(&f).display()));
    }
    ctx.say(&format!("installed {}", l.at(MANIFEST).display()));
    if l.live {
        migrate_autostart(ctx, l);
        if opts.autostart && first {
            add_autostart(ctx, l);
        }
        retarget_autostart(ctx, l);
        start_tray(ctx, l, tray_was_running);
    }
    let on_path = ctx.var_os("PATH").is_some_and(|p| std::env::split_paths(p).any(|d| d == l.prefix.join("bin")));
    if !on_path {
        ctx.say(&format!("note: {} is not on PATH", l.prefix.join("bin").display()));
    }
    Ok(())
}

pub fn uninstall(ctx: &Ctx, l: &Layout) -> Result<(), String> {
    if !l.at(MANIFEST).is_file() {
        return Err(format!("{} not found: nothing installed by frame there, nothing removed", l.at(MANIFEST).display()));
    }
    if l.live {
        stop_running(ctx, l);
        remove_video_era(ctx);
        if let Err(e) = crate::binds::unbind(ctx) {
            ctx.warn(&format!("unbind failed (continuing): {e}"));
        }
        remove_autostart(ctx, l, &ctx.autostart_file(), "frame", " tray");
        remove_autostart(ctx, l, &old_autostart(ctx), "frame-gui", "");
    }
    remove_listed(ctx, l, MANIFEST);
    if l.live {
        refresh_caches(ctx, l);
    }
    Ok(())
}

/// Write DATA to PATH via a temp file and rename, with MODE.
fn put(path: &Path, data: &[u8], mode: u32) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let err = |e: std::io::Error| format!("{}: {e}", path.display());
    let dir = path.parent().ok_or_else(|| format!("{}: no parent dir", path.display()))?;
    std::fs::create_dir_all(dir).map_err(err)?;
    let name = path.file_name().unwrap_or_default().to_string_lossy();
    let tmp = dir.join(format!(".{name}.install-{}", std::process::id()));
    let _ = std::fs::remove_file(&tmp);
    let r = (|| {
        let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(mode).open(&tmp)?;
        f.set_permissions(std::fs::Permissions::from_mode(mode))?; // umask may cut bits
        f.write_all(data)?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if r.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    r.map_err(err)
}

fn refresh_caches(ctx: &Ctx, l: &Layout) {
    for (tool, args) in [
        ("update-desktop-database", vec!["-q".into(), l.at("share/applications")]),
        ("gtk-update-icon-cache", vec!["-q".into(), "-t".into(), "-f".into(), l.at("share/icons/hicolor")]),
    ] {
        if ctx.have(tool) {
            let _ = ctx.command(tool).args(args).stdout(Stdio::null()).stderr(Stdio::null()).status();
        }
    }
}

// ------------------------------------------------------- stopping Frame

/// The processes to stop before files are replaced. NAME = a binary in
/// ROOT/bin: matched by /proc/PID/exe, also when that file was replaced
/// since it started ("PATH (deleted)"), so other programs are never
/// touched. `frame` only as the tray or settings window (no argument,
/// `tray`, `settings`): never a capture, `_clip` (serving the clipboard)
/// or `_toast`. The old frame-gui / clipped-gui were GUI-only binaries.
fn gui_pids(proc_root: &Path, exe: &Path, name: &str) -> Vec<u32> {
    let me = std::process::id();
    let deleted = {
        let mut d = exe.as_os_str().to_owned();
        d.push(" (deleted)");
        PathBuf::from(d)
    };
    let mut out = Vec::new();
    for e in std::fs::read_dir(proc_root).into_iter().flatten().flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else { continue };
        if pid == me {
            continue;
        }
        let Ok(x) = std::fs::read_link(e.path().join("exe")) else { continue };
        if x != exe && x != deleted {
            continue;
        }
        let cmdline = std::fs::read(e.path().join("cmdline")).unwrap_or_default();
        let arg1 = cmdline.split(|&b| b == 0).nth(1).filter(|a| !a.is_empty());
        let gui = match name {
            "frame" => matches!(arg1, None | Some(b"tray" | b"settings")),
            _ => arg1 != Some(b"--grab"),
        };
        if gui {
            out.push(pid);
        }
    }
    out
}

/// PID is gone (or a zombie: it has exited, only its parent hasn't
/// reaped it).
fn gone(pid: u32) -> bool {
    match std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        Ok(s) => s.rfind(") ").and_then(|i| s[i + 2..].chars().next()) == Some('Z'),
        Err(_) => true,
    }
}

/// Wait up to SECS for PIDS to go; the ones still running.
fn wait_gone(secs: u64, pids: &[u32]) -> Vec<u32> {
    let end = Instant::now() + Duration::from_secs(secs);
    loop {
        let left: Vec<u32> = pids.iter().copied().filter(|&p| !gone(p)).collect();
        if left.is_empty() || Instant::now() >= end {
            return left;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn kill(pid: u32, sig: libc::c_int) {
    // SAFETY: kill has no memory effects; a stale pid only fails.
    unsafe {
        libc::kill(pid as libc::pid_t, sig);
    }
}

fn join(pids: &[u32]) -> String {
    pids.iter().map(u32::to_string).collect::<Vec<_>>().join(" ")
}

/// TERM PIDS (what LABEL names) and wait up to 12 s (an old 0.3 tray gave
/// its daemon 10 s to stop).
fn stop(ctx: &Ctx, label: &str, pids: &[u32]) {
    if pids.is_empty() {
        return;
    }
    ctx.say(&format!("stopping {label} (pid {})", join(pids)));
    for &p in pids {
        kill(p, libc::SIGTERM);
    }
    let left = wait_gone(12, pids);
    if !left.is_empty() {
        ctx.warn(&format!("{label} still running (pid {}); not killed", join(&left)));
    }
}

/// Stop our bin/NAME's tray / settings processes.
fn stop_gui(ctx: &Ctx, l: &Layout, name: &str) {
    let exe = l.at(&format!("bin/{name}"));
    let exe = exe.canonicalize().unwrap_or_else(|_| l.root.canonicalize().unwrap_or(l.root.clone()).join("bin").join(name));
    stop(ctx, name, &gui_pids(Path::new("/proc"), &exe, name));
}

/// Before files are replaced: a running tray (any version, also the
/// AppImage's, found by its tray.pid) holds gui.lock and keeps the old
/// binary running, so the new one couldn't start. True if a tray ran.
fn stop_running(ctx: &Ctx, l: &Layout) -> bool {
    let run = ctx.rundir();
    let tray = live_pid(Path::new("/proc"), &run.join("tray.pid")).filter(|&p| p != std::process::id());
    stop_gui(ctx, l, "frame");
    stop_gui(ctx, l, "frame-gui");
    if let Some(p) = tray.filter(|&p| !gone(p)) {
        stop(ctx, "the Frame tray", &[p]);
    }
    stop_video_era(ctx, &run);
    tray.is_some()
}

/// Frame 0.3 ran `frame daemon` (a bash script, so not found by exe) and
/// gpu-screen-recorder for replay / record. Stop what their pid files in
/// RUNDIR still name: INT for the recorders (so a recording is finished,
/// not cut; TERM if that doesn't stop them in 10 s), TERM for the daemon.
fn stop_video_era(ctx: &Ctx, run: &Path) {
    for (f, sig) in [("record.pid", libc::SIGINT), ("daemon.pid", libc::SIGTERM), ("replay.pid", libc::SIGINT)] {
        let file = run.join(f);
        let Some(pid) = live_pid(Path::new("/proc"), &file) else { continue };
        ctx.say(&format!("stopping Frame 0.3's {f} process (pid {pid})"));
        kill(pid, sig);
        if sig == libc::SIGINT && !wait_gone(10, &[pid]).is_empty() {
            kill(pid, libc::SIGTERM);
        }
        if !wait_gone(12, &[pid]).is_empty() {
            ctx.warn(&format!("pid {pid} still running; not killed"));
            continue;
        }
        let _ = std::fs::remove_file(file);
    }
}

// ------------------------------------------------------------- clean-up

/// Delete the files manifest LIST names (plain relative paths only), then
/// the list and its directory if empty.
fn remove_listed(ctx: &Ctx, l: &Layout, list: &str) {
    let text = std::fs::read_to_string(l.at(list)).unwrap_or_default();
    for f in text.lines() {
        let plain = f.bytes().next().is_some_and(|b| b.is_ascii_alphanumeric())
            && f.bytes().all(|b| b.is_ascii_alphanumeric() || b"._+/-".contains(&b))
            && !f.split('/').any(|c| c == ".." || c.is_empty());
        if !plain {
            ctx.warn(&format!("skipped odd manifest line: {f}"));
            continue;
        }
        let p = l.at(f);
        if p.symlink_metadata().is_ok() && std::fs::remove_file(&p).is_ok() {
            ctx.say(&format!("removed {}", p.display()));
        }
    }
    let _ = std::fs::remove_file(l.at(list));
    if let Some(d) = l.at(list).parent() {
        let _ = std::fs::remove_dir(d);
    }
}

/// Files an older install listed that this one no longer installs
/// (bin/frame-gui before 0.5).
fn remove_stale(ctx: &Ctx, l: &Layout) {
    if l.listed(MANIFEST, "bin/frame-gui") {
        let p = l.at("bin/frame-gui");
        if p.symlink_metadata().is_ok() && std::fs::remove_file(&p).is_ok() {
            ctx.say(&format!("removed {} (Frame < 0.5)", p.display()));
        }
    }
}

/// Frame replaces Clipped: its tray, service and shortcuts would fight
/// Frame's. Its config (~/.config/clipped) and captures stay; frame copies
/// the config once on first run.
fn remove_clipped(ctx: &Ctx, l: &Layout) {
    if !l.at(OLD_MANIFEST).is_file() {
        return;
    }
    ctx.say(&format!("removing the old Clipped install ({})", l.at(OLD_MANIFEST).display()));
    stop_gui(ctx, l, "clipped-gui");
    let clipped = l.at("bin/clipped");
    if l.listed(OLD_MANIFEST, "bin/clipped") && crate::ctx::is_exec(&clipped) {
        for args in [["service", "off"].as_slice(), &["unbind"]] {
            let ok = ctx.command(&clipped.to_string_lossy()).args(args).status().is_ok_and(|s| s.success());
            if !ok {
                ctx.warn(&format!("clipped {} failed (continuing)", args.join(" ")));
            }
        }
    }
    let auto = ctx.config_home().join("autostart/clipped-gui.desktop");
    remove_autostart(ctx, l, &auto, "clipped-gui", "");
    remove_listed(ctx, l, OLD_MANIFEST);
}

/// Frame < 0.4 recorded video: `frame service on` wrote a systemd user
/// unit that ran `frame daemon` (gone now), and KDE shortcuts for replay /
/// record were .desktop entries. The tray's `frame bind` also drops those
/// shortcuts from kglobalaccel.
fn remove_video_era(ctx: &Ctx) {
    let unit = ctx.config_home().join("systemd/user/frame.service");
    if unit.is_file() {
        ctx.say(&format!("removing the old video service ({})", unit.display()));
        let ok = ctx.command("systemctl").args(["--user", "disable", "--now", "frame.service"]).status().is_ok_and(|s| s.success());
        if !ok {
            ctx.warn("could not stop frame.service (continuing)");
        }
        let _ = std::fs::remove_file(&unit);
        let _ = ctx.command("systemctl").args(["--user", "daemon-reload"]).status();
    }
    for a in ["replay", "record"] {
        let d = ctx.data_home().join(format!("applications/frame-{a}.desktop"));
        if d.symlink_metadata().is_ok() && std::fs::remove_file(&d).is_ok() {
            ctx.say(&format!("removed {}", d.display()));
        }
    }
}

// ------------------------------------------------------------ autostart

fn old_autostart(ctx: &Ctx) -> PathBuf {
    ctx.config_home().join("autostart/frame-gui.desktop")
}

fn exec_line(text: &str) -> Option<&str> {
    text.lines().find_map(|l| l.strip_prefix("Exec="))
}

fn turned_off(text: &str) -> bool {
    text.lines().any(|l| l == "X-GNOME-Autostart-enabled=false" || l == "Hidden=true")
}

/// The binaries a login entry of ours may name for bin/NAME: the PREFIX
/// path, its resolved path, and for `frame` this process's own stable
/// path (the AppImage, before it was installed).
fn our_exes(ctx: &Ctx, l: &Layout, name: &str) -> Vec<String> {
    let mut v = vec![l.prefix.join("bin").join(name)];
    if let Ok(r) = l.at(&format!("bin/{name}")).canonicalize() {
        v.push(r);
    }
    if name == "frame" {
        v.push(ctx.exe.clone());
        if let Some(app) = ctx.var_os("APPIMAGE") {
            v.push(PathBuf::from(app));
        }
        if let Ok(e) = stable_exe(ctx) {
            v.push(e);
        }
    }
    v.iter().map(|p| desktop_arg(&p.to_string_lossy())).collect()
}

/// The login entry FILE runs our bin/NAME followed by ARGS.
fn autostart_ours(ctx: &Ctx, l: &Layout, file: &Path, name: &str, args: &str) -> bool {
    let Ok(text) = std::fs::read_to_string(file) else { return false };
    let Some(exec) = exec_line(&text) else { return false };
    our_exes(ctx, l, name).iter().any(|e| exec == format!("{e}{args}"))
}

fn remove_autostart(ctx: &Ctx, l: &Layout, file: &Path, name: &str, args: &str) {
    if !file.is_file() {
        return;
    }
    if autostart_ours(ctx, l, file, name, args) {
        let _ = std::fs::remove_file(file);
        ctx.say(&format!("removed {}", file.display()));
    } else {
        ctx.say(&format!("note: kept {} (it starts a different {name})", file.display()));
    }
}

fn add_autostart(ctx: &Ctx, l: &Layout) {
    let f = ctx.autostart_file();
    if f.symlink_metadata().is_ok() {
        return;
    }
    match put(&f, autostart_entry(&l.bin().to_string_lossy()).as_bytes(), 0o644) {
        Ok(()) => ctx.say(&format!("Frame starts when you log in ({}; turn off in Settings)", f.display())),
        Err(e) => ctx.warn(&e),
    }
}

/// Frame < 0.5 started bin/frame-gui at login. Our old entry is replaced
/// by the new one, unless it was turned off.
fn migrate_autostart(ctx: &Ctx, l: &Layout) {
    let old = old_autostart(ctx);
    if !autostart_ours(ctx, l, &old, "frame-gui", "") {
        return;
    }
    let off = std::fs::read_to_string(&old).is_ok_and(|t| turned_off(&t));
    let _ = std::fs::remove_file(&old);
    ctx.say(&format!("removed {} (Frame < 0.5)", old.display()));
    if !off {
        add_autostart(ctx, l);
    }
}

/// A login entry that starts another frame of ours (the AppImage that is
/// installing itself) now starts the installed one; on/off is kept.
fn retarget_autostart(ctx: &Ctx, l: &Layout) {
    let f = ctx.autostart_file();
    let want = format!("{} tray", desktop_arg(&l.bin().to_string_lossy()));
    let Ok(text) = std::fs::read_to_string(&f) else { return };
    if exec_line(&text) == Some(want.as_str()) || !autostart_ours(ctx, l, &f, "frame", " tray") {
        return;
    }
    let new: String = text.lines().map(|line| if line.starts_with("Exec=") { format!("Exec={want}\n") } else { format!("{line}\n") }).collect();
    match put(&f, new.as_bytes(), 0o644) {
        Ok(()) => ctx.say(&format!("{} now starts {}", f.display(), l.bin().display())),
        Err(e) => ctx.warn(&e),
    }
}

/// After a live install: start the new tray if one was running before, or
/// if Frame starts at login (our entry, not turned off), so the user is
/// on the new version without logging out. Detached, without the
/// AppImage's variables.
fn start_tray(ctx: &Ctx, l: &Layout, was_running: bool) {
    if !was_running {
        let f = ctx.autostart_file();
        let on = std::fs::read_to_string(&f).is_ok_and(|t| !turned_off(&t));
        if !on || !autostart_ours(ctx, l, &f, "frame", " tray") {
            return;
        }
    }
    if ctx.var_os("WAYLAND_DISPLAY").is_none() && ctx.var_os("DISPLAY").is_none() {
        ctx.say("note: no desktop session here; the Frame tray starts when you next log in");
        return;
    }
    let bin = l.at("bin/frame");
    let mut c = ctx.command(&bin.to_string_lossy());
    c.arg("tray");
    for v in APPIMAGE_VARS {
        c.env_remove(v);
    }
    match crate::ctx::spawn_detached(&mut c) {
        Ok(()) => ctx.say(&format!("started the Frame tray ({} tray)", bin.display())),
        Err(e) => ctx.warn(&format!("could not start {}: {e}; start Frame from the menu", bin.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx::testenv::T;
    use std::os::unix::fs::PermissionsExt;
    use std::process::{Child, Command};

    /// T with PREFIX in the temp dir and a logging stand-in frame to
    /// install (it records "argv | FRAME_BIN" in runs.log).
    fn setup(tag: &str) -> (T, PathBuf) {
        let mut t = T::new(tag);
        t.set("PREFIX", t.dir.join("pfx"));
        let src = t.dir.join("src-frame");
        std::fs::write(&src, format!("#!/bin/sh\necho \"$0 $* | $FRAME_BIN\" >>'{}'\n", t.dir.join("runs.log").display())).unwrap();
        std::fs::set_permissions(&src, std::fs::Permissions::from_mode(0o755)).unwrap();
        (t, src)
    }

    fn inst(t: &T, src: &Path, extra: &[&str]) -> (i32, String, String) {
        let mut a = vec!["install", "--from", src.to_str().unwrap()];
        a.extend(extra);
        t.frame(&a)
    }

    fn runs(t: &T) -> String {
        std::fs::read_to_string(t.dir.join("runs.log")).unwrap_or_default()
    }

    fn wait_log(t: &T, want: &str) -> bool {
        (0..50).any(|_| {
            let ok = runs(t).contains(want);
            if !ok {
                std::thread::sleep(Duration::from_millis(100));
            }
            ok
        })
    }

    /// A long-running stand-in for a frame process: a copy of /bin/sh at
    /// EXE run as `EXE ARG` (ARG = a script in DIR that waits on stdin) or
    /// as bare `EXE` (reads stdin). Nothing real ever runs as EXE.
    fn fake_proc(dir: &Path, exe: &Path, arg: Option<&str>) -> Child {
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        if !exe.exists() {
            std::fs::copy("/bin/sh", exe).unwrap();
        }
        let mut c = Command::new(exe);
        if let Some(a) = arg {
            std::fs::write(dir.join(a), "read x\n").unwrap();
            c.arg(a);
        }
        c.current_dir(dir).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null());
        // Another test thread may fork while EXE's write fd is open (it
        // closes at that child's exec): ETXTBSY for a moment, so retry.
        for _ in 0..50 {
            match c.spawn() {
                Err(e) if e.raw_os_error() == Some(libc::ETXTBSY) => std::thread::sleep(Duration::from_millis(20)),
                r => return r.unwrap(),
            }
        }
        panic!("{} stayed busy", exe.display());
    }

    fn exited(c: &mut Child) -> bool {
        (0..30).any(|_| {
            let done = c.try_wait().unwrap().is_some();
            if !done {
                std::thread::sleep(Duration::from_millis(100));
            }
            done
        })
    }

    fn pidline(pid: u32) -> String {
        format!("{pid} {}\n", crate::state::proc_start(Path::new("/proc"), pid).unwrap())
    }

    /// Into a temp DESTDIR: 11 files + manifest, nothing outside it, no
    /// autostart; uninstall removes exactly the list and skips odd lines.
    #[test]
    fn destdir_install_and_uninstall() {
        let (mut t, src) = setup("instdest");
        let d = t.dir.join("dest");
        t.set("DESTDIR", d.clone()).set("PREFIX", "/usr/local/");
        let (c, out, err) = inst(&t, &src, &[]);
        assert_eq!(c, 0, "{out}{err}");
        let mut n = 0;
        let mut stack = vec![d.clone()];
        while let Some(p) = stack.pop() {
            for e in std::fs::read_dir(p).unwrap().flatten() {
                if e.file_type().unwrap().is_dir() { stack.push(e.path()) } else { n += 1 }
            }
        }
        assert_eq!(n, 12);
        let root = d.join("usr/local");
        assert_eq!(std::fs::read_to_string(root.join(MANIFEST)).unwrap().lines().count(), 11);
        assert_eq!(std::fs::read(root.join("bin/frame")).unwrap(), std::fs::read(&src).unwrap());
        assert_eq!(std::fs::metadata(root.join("bin/frame")).unwrap().permissions().mode() & 0o777, 0o755);
        let de = std::fs::read_to_string(root.join("share/applications/frame.desktop")).unwrap();
        assert!(de.contains("\nExec=/usr/local/bin/frame\n") && de.contains("\nCategories=Utility;\n"), "{de}");
        assert!(de.ends_with("\nX-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2\n"), "{de}");
        assert_eq!(std::fs::read(root.join("share/icons/hicolor/256x256/apps/frame.png")).unwrap(), PNGS[7].1);
        assert!(!t.dir.join("cfg/autostart/frame.desktop").exists());
        assert!(out.contains(&format!("installed {}/bin/frame\n", root.display())), "{out}");
        assert!(out.contains("note: /usr/local/bin is not on PATH"), "{out}");
        assert!(runs(&t).is_empty());

        std::fs::write(d.join("usr/outside"), "keep").unwrap();
        let mut list = std::fs::read_to_string(root.join(MANIFEST)).unwrap();
        list.push_str("../outside\n/etc/passwd\n");
        std::fs::write(root.join(MANIFEST), list).unwrap();
        let (c, _, err) = t.frame(&["uninstall"]);
        assert_eq!(c, 0, "{err}");
        assert!(err.contains("skipped odd manifest line: ../outside") && err.contains("skipped odd manifest line: /etc/passwd"), "{err}");
        assert!(d.join("usr/outside").exists() && !root.join("bin/frame").exists() && !root.join("share/frame").exists());
        // Nothing installed: refused, nothing removed.
        let (c, _, err) = t.frame(&["uninstall"]);
        assert_eq!(c, 1);
        assert!(err.contains("nothing removed"), "{err}");
    }

    /// A real (no DESTDIR) first install turns autostart on with the PREFIX
    /// path (quoted: a space); a reinstall keeps the user's choice;
    /// --no-autostart skips it. Bad options and PREFIX are refused.
    #[test]
    fn autostart_first_install_only() {
        let (mut t, src) = setup("instauto");
        t.set("PREFIX", t.dir.join("a pfx"));
        let auto = t.dir.join("cfg/autostart/frame.desktop");
        let (c, out, err) = inst(&t, &src, &[]);
        assert_eq!(c, 0, "{err}");
        assert!(out.contains("Frame starts when you log in"), "{out}");
        assert!(out.contains("note: no desktop session here"), "{out}");
        let bin = t.dir.join("a pfx/bin/frame");
        assert_eq!(std::fs::read_to_string(&auto).unwrap(), autostart_entry(bin.to_str().unwrap()));
        assert!(std::fs::read_to_string(&auto).unwrap().contains(&format!("Exec=\"{}\" tray\n", bin.display())));
        std::fs::remove_file(&auto).unwrap();
        assert_eq!(inst(&t, &src, &[]).0, 0);
        assert!(!auto.exists(), "reinstall re-added autostart");
        assert_eq!(t.frame(&["uninstall"]).0, 0);
        t.set("PREFIX", t.dir.join("p2"));
        assert_eq!(inst(&t, &src, &["--no-autostart"]).0, 0);
        assert!(!auto.exists());
        assert_eq!(inst(&t, &src, &["--bogus"]).0, 2);
        assert_eq!(t.frame(&["uninstall", "--force"]).0, 2);
        t.set("PREFIX", "rel/dir");
        let (c, _, err) = inst(&t, &src, &[]);
        assert_eq!(c, 1);
        assert!(err.contains("PREFIX must be absolute"), "{err}");
    }

    #[test]
    fn foreign_bin_is_refused_unless_forced() {
        let (t, src) = setup("instforeign");
        let bin = t.dir.join("pfx/bin/frame");
        std::fs::create_dir_all(bin.parent().unwrap()).unwrap();
        std::fs::write(&bin, "#!/bin/sh\n# not ours\n").unwrap();
        let (c, _, err) = inst(&t, &src, &[]);
        assert_eq!(c, 1);
        assert!(err.contains("exists and was not installed by frame"), "{err}");
        assert!(t.frame(&["uninstall"]).2.contains("nothing removed"));
        assert!(std::fs::read_to_string(&bin).unwrap().contains("not ours"));
        assert_eq!(inst(&t, &src, &["--force", "--no-autostart"]).0, 0);
        assert!(!std::fs::read_to_string(&bin).unwrap().contains("not ours"));
        assert_eq!(t.frame(&["uninstall"]).0, 0);
    }

    /// Live: the installed tray and settings window are stopped (also with
    /// their binary replaced, "(deleted)"), `frame _clip` and a capture are
    /// not. The tray was running, so the new one is started, without the
    /// AppImage's FRAME_BIN.
    #[test]
    fn stops_only_tray_and_settings() {
        let (mut t, src) = setup("inststop");
        t.set("WAYLAND_DISPLAY", "wayland-test").set("FRAME_BIN", "/tmp/Frame.AppImage");
        assert_eq!(inst(&t, &src, &["--no-autostart"]).0, 0);
        let bin = t.dir.join("pfx/bin/frame");
        std::fs::remove_file(&bin).unwrap();
        let work = t.dir.join("work");
        std::fs::create_dir_all(&work).unwrap();
        let mut tray = fake_proc(&work, &bin, Some("tray"));
        let mut settings = fake_proc(&work, &bin, Some("settings"));
        let mut bare = fake_proc(&work, &bin, None);
        let mut clip = fake_proc(&work, &bin, Some("_clip"));
        let mut region = fake_proc(&work, &bin, Some("region"));
        std::fs::create_dir_all(t.dir.join("run/frame")).unwrap();
        std::fs::write(t.dir.join("run/frame/tray.pid"), pidline(tray.id())).unwrap();
        // The binary is replaced under the running processes.
        std::fs::remove_file(&bin).unwrap();
        std::fs::copy(&src, &bin).unwrap();
        let (c, out, err) = inst(&t, &src, &[]);
        assert_eq!(c, 0, "{err}");
        for p in [&mut tray, &mut settings, &mut bare] {
            assert!(exited(p), "pid {} not stopped: {out}", p.id());
        }
        for p in [&mut clip, &mut region] {
            assert!(p.try_wait().unwrap().is_none(), "pid {} was stopped: {out}", p.id());
            p.kill().unwrap();
            p.wait().unwrap();
        }
        assert!(out.contains("stopping frame (pid "), "{out}");
        assert!(out.contains(&format!("started the Frame tray ({} tray)", bin.display())), "{out}");
        assert!(wait_log(&t, &format!("{} tray | \n", bin.display())), "{}", runs(&t));
        assert_eq!(t.frame(&["uninstall"]).0, 0);
    }

    /// The AppImage installing itself: its own tray (another binary, found
    /// by tray.pid) is stopped, its login entry now starts the installed
    /// frame (still turned on), and that tray is started.
    #[test]
    fn appimage_tray_and_autostart_move_to_installed() {
        let (mut t, src) = setup("instapp");
        let app = t.dir.join("Frame.AppImage");
        std::fs::copy(&src, &app).unwrap();
        t.set("WAYLAND_DISPLAY", "wayland-test").set("APPIMAGE", app.clone()).set("FRAME_BIN", app.clone());
        let mount = t.dir.join("mount/usr/bin/frame");
        let work = t.dir.join("work");
        std::fs::create_dir_all(&work).unwrap();
        let mut tray = fake_proc(&work, &mount, Some("tray"));
        std::fs::create_dir_all(t.dir.join("run/frame")).unwrap();
        std::fs::write(t.dir.join("run/frame/tray.pid"), pidline(tray.id())).unwrap();
        let auto = t.dir.join("cfg/autostart/frame.desktop");
        std::fs::create_dir_all(auto.parent().unwrap()).unwrap();
        std::fs::write(&auto, autostart_entry(app.to_str().unwrap())).unwrap();
        // The settings window asks before: not installed, so autostart names the AppImage.
        let ctx = t.ctx();
        assert_eq!(installed(&ctx), None);
        assert_eq!(stable_exe(&ctx).unwrap(), app);
        let (c, out, err) = inst(&t, &src, &[]);
        assert_eq!(c, 0, "{err}");
        assert!(exited(&mut tray), "{out}");
        assert!(out.contains(&format!("stopping the Frame tray (pid {})", tray.id())), "{out}");
        let bin = t.dir.join("pfx/bin/frame");
        assert_eq!(std::fs::read_to_string(&auto).unwrap(), autostart_entry(bin.to_str().unwrap()));
        assert!(wait_log(&t, &format!("{} tray | \n", bin.display())), "{}", runs(&t));
        let ctx = t.ctx();
        assert_eq!(installed(&ctx), Some(bin.clone()));
        assert_eq!(stable_exe(&ctx).unwrap(), bin);
        // Uninstall removes that login entry: it starts our frame.
        let (c, out, _) = t.frame(&["uninstall"]);
        assert_eq!(c, 0);
        assert!(!auto.exists(), "{out}");
    }

    /// Never a path in the AppImage's temporary mount.
    #[test]
    fn stable_exe_never_in_the_mount() {
        let (mut t, _) = setup("stable");
        let mount = t.dir.join("mount");
        std::fs::create_dir_all(mount.join("usr/bin")).unwrap();
        t.set("APPDIR", mount.clone());
        let ctx = Ctx::captured(t.env.clone(), mount.join("usr/bin/frame"));
        assert!(stable_exe(&ctx).unwrap_err().contains("temporary mount"));
        assert_eq!(stable_exe(&t.ctx()).unwrap(), PathBuf::from("/opt/frame/bin/frame"));
    }

    /// Upgrades in the same PREFIX: a 0.4 install (bin/frame-gui listed,
    /// its login entry) and Frame 0.3's service and video shortcuts.
    #[test]
    fn upgrades_04_and_03() {
        let (t, src) = setup("instup");
        let p = t.dir.join("pfx");
        std::fs::create_dir_all(p.join("bin")).unwrap();
        std::fs::create_dir_all(p.join("share/frame")).unwrap();
        std::fs::write(p.join("bin/frame"), "#!/usr/bin/env bash\n# frame 0.4\n").unwrap();
        std::fs::write(p.join("bin/frame-gui"), "x").unwrap();
        std::fs::write(p.join(MANIFEST), "bin/frame\nbin/frame-gui\nshare/applications/frame.desktop\n").unwrap();
        let old = t.dir.join("cfg/autostart/frame-gui.desktop");
        std::fs::create_dir_all(old.parent().unwrap()).unwrap();
        std::fs::write(&old, format!("[Desktop Entry]\nExec={}\nX-GNOME-Autostart-enabled=true\n", p.join("bin/frame-gui").display())).unwrap();
        let unit = t.dir.join("cfg/systemd/user/frame.service");
        std::fs::create_dir_all(unit.parent().unwrap()).unwrap();
        std::fs::write(&unit, "[Service]\n").unwrap();
        let apps = t.dir.join("data/applications");
        std::fs::create_dir_all(&apps).unwrap();
        for a in ["replay", "record", "region"] {
            std::fs::write(apps.join(format!("frame-{a}.desktop")), "[Desktop Entry]\n").unwrap();
        }
        let log = t.dir.join("systemctl.log");
        t.fake("systemctl", &format!("echo \"$*\" >>'{}'", log.display()));
        let (c, out, err) = inst(&t, &src, &[]);
        assert_eq!(c, 0, "{err}");
        assert!(out.contains(&format!("removed {} (Frame < 0.5)", p.join("bin/frame-gui").display())), "{out}");
        assert!(out.contains(&format!("removed {} (Frame < 0.5)", old.display())), "{out}");
        assert!(!p.join("bin/frame-gui").exists() && !old.exists());
        let auto = std::fs::read_to_string(t.dir.join("cfg/autostart/frame.desktop")).unwrap();
        assert!(auto.contains(&format!("Exec={} tray\n", p.join("bin/frame").display())), "{auto}");
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "--user disable --now frame.service\n--user daemon-reload\n");
        assert!(!unit.exists() && !apps.join("frame-replay.desktop").exists() && !apps.join("frame-record.desktop").exists());
        assert!(apps.join("frame-region.desktop").exists());
        assert!(!std::fs::read_to_string(p.join(MANIFEST)).unwrap().contains("frame-gui"));
        // Reinstall: no systemctl again.
        std::fs::remove_file(&log).unwrap();
        assert_eq!(inst(&t, &src, &[]).0, 0);
        assert!(!log.exists());
        // Uninstall (live): unbinds in-process (dry-run here) and removes
        // our login entry.
        let (c, out, _) = t.frame(&["uninstall"]);
        assert_eq!(c, 0);
        assert!(out.contains("+ kglobalaccel unregister frame-region.desktop _launch"), "{out}");
        assert!(!t.dir.join("cfg/autostart/frame.desktop").exists());
    }

    /// A turned-off 0.4 login entry is removed, not replaced; someone
    /// else's frame.desktop login entry is kept on uninstall.
    #[test]
    fn autostart_off_and_foreign_kept() {
        let (t, src) = setup("instoff");
        let p = t.dir.join("pfx");
        let old = t.dir.join("cfg/autostart/frame-gui.desktop");
        std::fs::create_dir_all(old.parent().unwrap()).unwrap();
        std::fs::write(&old, format!("[Desktop Entry]\nExec={}\nHidden=true\n", p.join("bin/frame-gui").display())).unwrap();
        std::fs::create_dir_all(p.join("share/frame")).unwrap();
        std::fs::write(p.join(MANIFEST), "bin/frame-gui\n").unwrap();
        assert_eq!(inst(&t, &src, &[]).0, 0);
        let auto = t.dir.join("cfg/autostart/frame.desktop");
        assert!(!old.exists() && !auto.exists(), "off entry re-enabled");
        std::fs::write(&auto, "[Desktop Entry]\nExec=/opt/other/frame tray\n").unwrap();
        let (c, out, _) = t.frame(&["uninstall"]);
        assert_eq!(c, 0);
        assert!(auto.exists() && out.contains(&format!("note: kept {}", auto.display())), "{out}");
    }

    /// An old Clipped install in the same PREFIX: its clipped-gui is
    /// stopped, `clipped service off` + `unbind` run, its listed files,
    /// manifest and login entry go; unlisted files, config and captures stay.
    #[test]
    fn removes_old_clipped() {
        let (t, src) = setup("instclipped");
        let p = t.dir.join("pfx");
        for d in ["bin", "share/clipped", "share/applications"] {
            std::fs::create_dir_all(p.join(d)).unwrap();
        }
        let log = t.dir.join("clipped.log");
        std::fs::write(p.join("bin/clipped"), format!("#!/bin/sh\necho \"$*\" >>'{}'\n", log.display())).unwrap();
        std::fs::set_permissions(p.join("bin/clipped"), std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(p.join("share/applications/clipped.desktop"), "[Desktop Entry]\n").unwrap();
        std::fs::write(p.join("share/applications/other.desktop"), "mine").unwrap();
        std::fs::write(p.join(OLD_MANIFEST), "bin/clipped\nbin/clipped-gui\nshare/applications/clipped.desktop\n").unwrap();
        let auto = t.dir.join("cfg/autostart/clipped-gui.desktop");
        std::fs::create_dir_all(auto.parent().unwrap()).unwrap();
        std::fs::write(&auto, format!("[Desktop Entry]\nExec={}\n", p.join("bin/clipped-gui").display())).unwrap();
        std::fs::create_dir_all(t.dir.join("cfg/clipped")).unwrap();
        std::fs::write(t.dir.join("cfg/clipped/config"), "image_format = jpg\n").unwrap();
        let work = t.dir.join("work");
        std::fs::create_dir_all(&work).unwrap();
        let mut gui = fake_proc(&work, &p.join("bin/clipped-gui"), None);
        let (c, out, err) = inst(&t, &src, &["--no-autostart"]);
        assert_eq!(c, 0, "{err}");
        assert!(exited(&mut gui), "{out}");
        assert!(out.contains(&format!("stopping clipped-gui (pid {})", gui.id())), "{out}");
        assert_eq!(std::fs::read_to_string(&log).unwrap(), "service off\nunbind\n");
        for f in ["bin/clipped", "bin/clipped-gui", "share/applications/clipped.desktop", OLD_MANIFEST, "share/clipped"] {
            assert!(!p.join(f).exists(), "left {f}");
        }
        assert!(!auto.exists() && p.join("share/applications/other.desktop").exists());
        assert!(t.dir.join("cfg/clipped/config").exists());
        assert!(p.join("bin/frame").exists());
    }
}
