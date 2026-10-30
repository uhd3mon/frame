//! The tray icon (StatusNotifierItem via ksni): a menu for screenshots,
//! the save folder and settings. On start it runs `frame bind` once:
//! Hyprland and Sway forget runtime binds when the session ends, so the
//! tray (started at login) puts them back. Menu items run frame detached.

use crate::{config, frame_detached, icons, state};
use ksni::blocking::TrayMethods;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

/// Set by Quit and by SIGTERM / SIGINT / SIGHUP; the main loop then exits.
static QUIT: AtomicBool = AtomicBool::new(false);

extern "C" fn on_signal(_: libc::c_int) {
    QUIT.store(true, Ordering::SeqCst);
}

struct ClipTray {
    icon: Vec<ksni::Icon>,
}

fn item(label: &str, f: impl Fn(&mut ClipTray) + Send + 'static) -> ksni::MenuItem<ClipTray> {
    ksni::menu::StandardItem { label: label.into(), activate: Box::new(f), ..Default::default() }.into()
}

fn shot(label: &str, mode: &'static str) -> ksni::MenuItem<ClipTray> {
    item(label, move |_| frame_detached(&[mode]))
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
        self.icon.clone()
    }
    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip { title: "Frame".into(), icon_pixmap: self.icon.clone(), ..Default::default() }
    }
    fn activate(&mut self, _x: i32, _y: i32) {
        crate::spawn_settings();
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        vec![
            shot("Region", "region"),
            shot("Window", "window"),
            shot("Monitor", "monitor"),
            shot("All monitors", "all"),
            ksni::MenuItem::Separator,
            item("Open folder", |_| open_folder()),
            item("Settings…", |_| crate::spawn_settings()),
            ksni::MenuItem::Separator,
            item("Quit", |_| QUIT.store(true, Ordering::SeqCst)),
        ]
    }
}

/// <save_dir>/frame/images, where screenshots are saved.
fn open_folder() {
    let text = std::fs::read(state::config_path()).unwrap_or_default();
    let dir = state::expand_dir(&config::parse(&text)["save_dir"]).join("frame/images");
    let _ = std::fs::create_dir_all(&dir);
    let _ = crate::ctx::spawn_detached(Command::new("xdg-open").arg(dir));
}

/// Run the tray until Quit or a signal.
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
            eprintln!("frame: {e}");
            return 1;
        }
    };
    let tray = ClipTray { icon: icons::pixmaps(icons::IDLE) };
    let handle = match tray.assume_sni_available(true).spawn() {
        Ok(h) => h,
        Err(e) => {
            eprintln!("frame: cannot show the tray icon: {e}");
            return 1;
        }
    };
    let pidfile = state::tray_pid_file(&rundir);
    if let Err(e) = state::write_own_pid(&pidfile) {
        eprintln!("frame: {}: {e}", pidfile.display());
    }
    frame_detached(&["bind"]);
    while !QUIT.load(Ordering::SeqCst) && !handle.is_closed() {
        std::thread::sleep(Duration::from_millis(200));
    }
    let _ = std::fs::remove_file(&pidfile);
    handle.shutdown().wait();
    0
}
