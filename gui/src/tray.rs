//! The tray icon (StatusNotifierItem via ksni). "Frame is running" means
//! this process: it keeps `frame daemon` (shortcuts + replay buffer) alive
//! through daemon::Supervisor and stops it on Quit or SIGTERM. State comes
//! from bin/frame's pid files; menu items run frame detached.

use crate::daemon::{Supervisor, View};
use crate::state::{self, Status};
use crate::{clip_detached, config, icons};
use ksni::blocking::TrayMethods;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Set by Quit and by SIGTERM / SIGINT / SIGHUP; the main loop then stops
/// the daemon and exits.
static QUIT: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_: libc::c_int) {
    QUIT.store(true, Ordering::SeqCst);
}

struct Icons {
    idle: Vec<ksni::Icon>,
    replay: Vec<ksni::Icon>,
    recording: Vec<ksni::Icon>,
}

struct ClipTray {
    status: Status,
    daemon: View,
    replay_wanted: bool,
    icons: Arc<Icons>,
}

impl ClipTray {
    fn icon(&self) -> Vec<ksni::Icon> {
        if self.status.recording.is_some() {
            self.icons.recording.clone()
        } else if self.status.replay.is_some() {
            self.icons.replay.clone()
        } else {
            self.icons.idle.clone()
        }
    }
}

/// The menu's replay line ("Replay buffer: on, 60 s") and a problem with
/// the daemon, if there is one (menu line + tooltip text).
fn replay_line(status: &Status, daemon: &View, replay_wanted: bool) -> (String, Option<String>) {
    if let Some(s) = &status.replay {
        let s = if s.is_empty() { String::new() } else { format!(", {s} s") };
        return (format!("Replay buffer: on{s}"), None);
    }
    let off = || "Replay buffer: off".to_string();
    match daemon {
        View::BadConfig(m) => ("Replay buffer: off (config error)".into(), Some(format!("Config error: {m}"))),
        View::Retry { why, secs } => (off(), Some(format!("{why}; trying again in {secs} s"))),
        View::Stopped => (off(), Some("Frame is stopping".into())),
        _ if !replay_wanted => (off(), None),
        View::Running { up } if *up < Duration::from_secs(10) => ("Replay buffer: starting…".into(), None),
        View::External if !status.daemon => ("Replay buffer: starting…".into(), None),
        // The daemon retries the recorder itself; its log says why it fails.
        _ => (
            "Replay buffer: not running".into(),
            Some(format!("The recorder isn't running; see {}", state::rundir().join("daemon.log").display())),
        ),
    }
}

fn item(label: &str, enabled: bool, f: impl Fn(&mut ClipTray) + Send + 'static) -> ksni::MenuItem<ClipTray> {
    ksni::menu::StandardItem { label: label.into(), enabled, activate: Box::new(f), ..Default::default() }
        .into()
}

fn shot(label: &str, mode: &'static str) -> ksni::MenuItem<ClipTray> {
    item(label, true, move |_| clip_detached(&[mode]))
}

impl ksni::Tray for ClipTray {
    fn id(&self) -> String {
        "frame".into()
    }
    fn title(&self) -> String {
        "Frame".into()
    }
    fn category(&self) -> ksni::Category {
        ksni::Category::ApplicationStatus
    }
    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        self.icon()
    }
    fn tool_tip(&self) -> ksni::ToolTip {
        let (_, problem) = replay_line(&self.status, &self.daemon, self.replay_wanted);
        ksni::ToolTip {
            title: self.status.tooltip(),
            description: problem.unwrap_or_default(),
            icon_pixmap: self.icon(),
            ..Default::default()
        }
    }
    fn activate(&mut self, _x: i32, _y: i32) {
        crate::spawn_settings();
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::SubMenu;
        let recording = self.status.recording.is_some();
        let (replay, problem) = replay_line(&self.status, &self.daemon, self.replay_wanted);
        let mut m = vec![item(&replay, false, |_| {})];
        if let Some(p) = problem {
            m.push(item(&p, false, |_| {}));
        }
        m.extend([
            ksni::MenuItem::Separator,
            item(if recording { "Stop recording" } else { "Start recording" }, true, |t| {
                clip_detached(&["record"]);
                // Show the change now; the next poll confirms it.
                if t.status.recording.is_some() {
                    t.status.recording = None;
                }
            }),
            item("Save replay", self.status.replay.is_some(), |_| clip_detached(&["replay"])),
            SubMenu {
                label: "Screenshot".into(),
                submenu: vec![
                    shot("Region", "region"),
                    shot("Window", "window"),
                    shot("Monitor", "monitor"),
                    shot("All monitors", "all"),
                ],
                ..Default::default()
            }
            .into(),
            item("Open Frame folder", true, |_| open_folder()),
            ksni::MenuItem::Separator,
            item("Settings…", true, |_| crate::spawn_settings()),
            item("Quit", true, |_| QUIT.store(true, Ordering::SeqCst)),
        ]);
        m
    }
}

fn open_folder() {
    let text = std::fs::read(state::config_path()).unwrap_or_default();
    let dir = state::expand_dir(&config::parse(&text)["save_dir"]).join("frame");
    let _ = std::fs::create_dir_all(&dir);
    crate::spawn_detached(Command::new("xdg-open").arg(dir));
}

/// Run the tray until Quit or a signal, keeping the daemon alive. Polls on
/// this thread, which also spawns the daemon (PR_SET_PDEATHSIG follows the
/// spawning thread); ksni has its own.
pub fn run() -> i32 {
    // SAFETY: the handler only stores to an atomic.
    unsafe {
        for s in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
            libc::signal(s, on_signal as *const () as libc::sighandler_t);
        }
    }
    let rundir = match state::ensure_rundir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("frame-gui: {e}");
            return 1;
        }
    };
    let icons = Arc::new(Icons {
        idle: icons::pixmaps(icons::IDLE),
        replay: icons::pixmaps(icons::REPLAY),
        recording: icons::pixmaps(icons::RECORDING),
    });
    let proc_root = Path::new("/proc");
    let mut sup = Supervisor::new(crate::clip_bin(), rundir.clone(), state::config_path(), proc_root.into());
    let tray = ClipTray { status: Status::read(proc_root, &rundir), daemon: View::Stopped, replay_wanted: true, icons };
    let handle = match tray.assume_sni_available(true).spawn() {
        Ok(h) => h,
        Err(e) => {
            eprintln!("frame-gui: cannot show the tray icon: {e}");
            return 1;
        }
    };
    let pidfile = state::tray_pid_file(&rundir);
    if let Err(e) = state::write_own_pid(&pidfile) {
        eprintln!("frame-gui: {}: {e}", pidfile.display());
    }
    sup.start(Instant::now());
    // ksni compares the properties after an update and only signals changes.
    let mut tick = 0u32;
    while !QUIT.load(Ordering::SeqCst) && !handle.is_closed() {
        std::thread::sleep(Duration::from_millis(200));
        tick += 1;
        if !tick.is_multiple_of(5) {
            continue;
        }
        let now = Instant::now();
        sup.tick(now);
        let (status, daemon, wanted) = (Status::read(proc_root, &rundir), sup.view(now), sup.replay_wanted());
        handle.update(|t| {
            t.status = status;
            t.daemon = daemon;
            t.replay_wanted = wanted;
        });
    }
    handle.update(|t| t.daemon = View::Stopped);
    sup.stop();
    let _ = std::fs::remove_file(&pidfile);
    handle.shutdown().wait();
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_lines() {
        let on = Status { replay: Some("60".into()), daemon: true, ..Default::default() };
        let running = View::Running { up: Duration::from_secs(30) };
        assert_eq!(replay_line(&on, &running, true), ("Replay buffer: on, 60 s".into(), None));
        assert_eq!(replay_line(&on, &View::External, true).0, "Replay buffer: on, 60 s");

        let idle = Status { daemon: true, ..Default::default() };
        assert_eq!(replay_line(&idle, &running, false), ("Replay buffer: off".into(), None));
        let fresh = View::Running { up: Duration::from_secs(1) };
        assert_eq!(replay_line(&idle, &fresh, true).0, "Replay buffer: starting…");
        let (l, p) = replay_line(&idle, &running, true);
        assert!(l == "Replay buffer: not running" && p.unwrap().contains("daemon.log"));

        let none = Status::default();
        let bad = View::BadConfig("video_fps must be 1-240".into());
        assert_eq!(
            replay_line(&none, &bad, true),
            ("Replay buffer: off (config error)".into(), Some("Config error: video_fps must be 1-240".into()))
        );
        let retry = View::Retry { why: "daemon stopped: exit code 1".into(), secs: 4 };
        assert_eq!(replay_line(&none, &retry, true).1.as_deref(), Some("daemon stopped: exit code 1; trying again in 4 s"));
    }
}
