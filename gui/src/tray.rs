//! The tray icon (StatusNotifierItem via ksni). State comes from bin/clipped's
//! pid files, polled once a second; menu items run clipped detached.

use crate::state::{self, Status};
use crate::{clip_detached, config, icons};
use ksni::blocking::TrayMethods;
use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

struct Icons {
    idle: Vec<ksni::Icon>,
    replay: Vec<ksni::Icon>,
    recording: Vec<ksni::Icon>,
}

struct ClipTray {
    status: Status,
    service: bool,
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

fn item(label: &str, enabled: bool, f: impl Fn(&mut ClipTray) + Send + 'static) -> ksni::MenuItem<ClipTray> {
    ksni::menu::StandardItem { label: label.into(), enabled, activate: Box::new(f), ..Default::default() }
        .into()
}

fn shot(label: &str, mode: &'static str) -> ksni::MenuItem<ClipTray> {
    item(label, true, move |_| clip_detached(&[mode]))
}

impl ksni::Tray for ClipTray {
    fn id(&self) -> String {
        "clipped".into()
    }
    fn title(&self) -> String {
        "Clipped".into()
    }
    fn category(&self) -> ksni::Category {
        ksni::Category::ApplicationStatus
    }
    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        self.icon()
    }
    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip { title: self.status.tooltip(), icon_pixmap: self.icon(), ..Default::default() }
    }
    fn activate(&mut self, _x: i32, _y: i32) {
        crate::spawn_settings();
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::{CheckmarkItem, SubMenu};
        let recording = self.status.recording.is_some();
        vec![
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
            item("Open Clipped folder", true, |_| open_folder()),
            ksni::MenuItem::Separator,
            CheckmarkItem {
                label: "Background service".into(),
                checked: self.service,
                activate: Box::new(|t: &mut Self| {
                    clip_detached(&["service", if t.service { "off" } else { "on" }]);
                    t.service = !t.service;
                }),
                ..Default::default()
            }
            .into(),
            item("Settings…", true, |_| crate::spawn_settings()),
            item("Quit", true, |_| std::process::exit(0)),
        ]
    }
}

fn open_folder() {
    let text = std::fs::read_to_string(state::config_path()).unwrap_or_default();
    let dir = state::expand_dir(&config::parse(&text)["save_dir"]).join("clipped");
    let _ = std::fs::create_dir_all(&dir);
    crate::spawn_detached(Command::new("xdg-open").arg(dir));
}

pub fn service_active() -> bool {
    Command::new("systemctl")
        .args(["--user", "is-active", "--quiet", "clipped.service"])
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Run the tray until Quit. Polls on this thread; ksni has its own.
pub fn run() -> i32 {
    let icons = Arc::new(Icons {
        idle: icons::pixmaps(icons::IDLE),
        replay: icons::pixmaps(icons::REPLAY),
        recording: icons::pixmaps(icons::RECORDING),
    });
    let rundir = state::rundir();
    let proc_root = Path::new("/proc");
    let tray = ClipTray { status: Status::read(proc_root, &rundir), service: service_active(), icons };
    let handle = match tray.assume_sni_available(true).spawn() {
        Ok(h) => h,
        Err(e) => {
            eprintln!("clipped-gui: cannot show the tray icon: {e}");
            return 1;
        }
    };
    // ksni compares the properties after an update and only signals changes.
    let mut tick = 0u32;
    while !handle.is_closed() {
        std::thread::sleep(Duration::from_secs(1));
        tick += 1;
        let status = Status::read(proc_root, &rundir);
        let service = if tick.is_multiple_of(5) { Some(service_active()) } else { None };
        handle.update(|t| {
            t.status = status;
            if let Some(s) = service {
                t.service = s;
            }
        });
    }
    0
}
