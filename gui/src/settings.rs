//! The settings window: edits the clip config, checks it with `clip check`
//! before it replaces the real file, and re-applies shortcuts.

use crate::config::{self, ACTIONS};
use crate::state::{self, Status};
use eframe::egui::{self, Color32, RichText};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::mpsc::{channel, Receiver, Sender};

/// Keys the daemon reads only when it starts (replay buffer, file names).
const DAEMON_KEYS: &[&str] = &[
    "save_dir", "video_format", "replay", "replay_seconds", "video_source", "video_fps",
    "video_quality", "video_codec", "audio",
];

/// Free-text fields: edge spaces are trimmed on Save.
fn is_text_key(k: &str) -> bool {
    matches!(k, "save_dir" | "video_source" | "audio") || k.starts_with("bind.")
}

enum Msg {
    Folder(PathBuf),
    Monitors(Vec<String>),
    Status(String, bool),
    /// Result of `clip bind`; it replaces "Updating shortcuts…" in the status.
    Rebind(String, bool),
}

const UPDATING: &str = "Updating shortcuts…";

struct App {
    path: PathBuf,
    loaded: BTreeMap<String, String>,
    vals: BTreeMap<String, String>,
    autostart_loaded: bool,
    autostart: bool,
    capture: Option<&'static str>,
    capture_meta: bool,
    status: String,
    error: bool,
    /// Set when the config exists but can't be read: Save is off, so the
    /// template can never replace a file we failed to read.
    read_error: Option<String>,
    /// A `clip bind` is in flight; Save waits for it.
    rebinding: bool,
    offer_restart: bool,
    monitors: Vec<String>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    ctx: egui::Context,
    /// CLIP_GUI_SHOT=FILE.png: save this window's own frame and quit (docs, review).
    shot: Option<(PathBuf, u32)>,
}

pub fn run() -> i32 {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("clip settings")
        .with_app_id("clip")
        .with_inner_size([560.0, 720.0])
        .with_min_inner_size([440.0, 360.0]);
    if let Some((w, h, rgba)) = crate::icons::rgba(crate::icons::APP_64) {
        viewport = viewport.with_icon(egui::IconData { rgba, width: w, height: h });
    }
    let opts = eframe::NativeOptions { viewport, ..Default::default() };
    match eframe::run_native("clip", opts, Box::new(|cc| Ok(Box::new(App::new(&cc.egui_ctx))))) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("clip-gui: {e}");
            1
        }
    }
}

fn run_clip(args: &[&str], config: Option<&Path>) -> Result<Output, String> {
    let bin = crate::clip_bin();
    let mut c = Command::new(&bin);
    c.args(args);
    if let Some(p) = config {
        c.env("CLIP_CONFIG", p);
    }
    c.output().map_err(|e| format!("cannot run {}: {e}", bin.display()))
}

fn stderr_of(o: &Output) -> String {
    let e = String::from_utf8_lossy(&o.stderr).trim().to_string();
    if e.is_empty() {
        format!("exit code {}", o.status.code().unwrap_or(-1))
    } else {
        e
    }
}

/// Escape a path for a desktop entry Exec= line (same as desktop_arg in bin/clip).
pub fn desktop_exec_arg(p: &str) -> String {
    if p.chars().all(|c| c.is_ascii_alphanumeric() || "/._+-".contains(c)) {
        return p.to_string();
    }
    let mut s = String::from("\"");
    for c in p.chars() {
        if matches!(c, '\\' | '"' | '`' | '$') {
            s.push('\\');
        }
        s.push(c);
    }
    s.push('"');
    s.replace('\\', "\\\\").replace('%', "%%")
}

pub fn autostart_entry(exe: &str) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=Clip\nComment=clip tray icon\nExec={}\nIcon=clip\n\
         Terminal=false\nX-GNOME-Autostart-enabled=true\n",
        desktop_exec_arg(exe)
    )
}

/// The config bytes, None if there is no file (or only a dangling
/// symlink: Save then creates its target). Any other error is an error.
pub fn read_config(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match std::fs::read(path) {
        Ok(b) => Ok(Some(b)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("Can't read {}: {e}", path.display())),
    }
}

/// Follow PATH's symlinks by hand (read_link, relative to the link's
/// directory), so a dangling link resolves to the file it names.
pub fn resolve_link(path: &Path) -> Result<PathBuf, String> {
    let mut p = path.to_path_buf();
    for _ in 0..40 {
        match std::fs::symlink_metadata(&p) {
            Ok(m) if m.file_type().is_symlink() => {
                let t = std::fs::read_link(&p).map_err(|e| format!("{}: {e}", p.display()))?;
                p = p.parent().unwrap_or(Path::new(".")).join(t);
            }
            _ => return Ok(p),
        }
    }
    Err(format!("{}: too many levels of symbolic links", path.display()))
}

/// Write TEXT as the config at PATH: a new temp file next to the real
/// one (same mode, synced), `clip check` on it, then rename. The real
/// file (or a symlink's target, never the link) is untouched on failure.
pub fn write_checked(path: &Path, text: &[u8]) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let real = resolve_link(path)?;
    let dir = real.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let name = real.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mode = std::fs::metadata(&real).ok().map(|m| m.permissions().mode() & 0o7777);
    let mut opened = None;
    for i in 0..100 {
        let tmp = dir.join(format!(".{name}.clip-gui-{}-{i}.tmp", std::process::id()));
        match std::fs::OpenOptions::new().write(true).create_new(true).mode(mode.unwrap_or(0o644)).open(&tmp) {
            Ok(f) => {
                opened = Some((tmp, f));
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(format!("{}: {e}", tmp.display())),
        }
    }
    let Some((tmp, mut f)) = opened else { return Err(format!("{}: no free temp file name", dir.display())) };
    let written = (|| {
        if let Some(m) = mode {
            f.set_permissions(std::fs::Permissions::from_mode(m))?; // umask may have cut bits
        }
        f.write_all(text)?;
        f.sync_all()
    })();
    drop(f);
    let checked = match written {
        Err(e) => Err(format!("Not saved: {}: {e}", tmp.display())),
        Ok(()) => match run_clip(&["check"], Some(&tmp)) {
            Ok(o) if o.status.success() => Ok(()),
            Ok(o) => Err(format!("Not saved: {}", stderr_of(&o))),
            Err(e) => Err(format!("Not saved: {e}")),
        },
    };
    if let Err(e) = checked {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    std::fs::rename(&tmp, &real).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("{}: {e}", real.display())
    })?;
    if let Ok(d) = std::fs::File::open(dir) {
        let _ = d.sync_all();
    }
    Ok(())
}

/// The text `clip config` would write, made in a temp path (not the real one).
fn template(dir: &Path) -> String {
    let t = dir.join(format!(".clip-gui-template-{}", std::process::id()));
    let _ = std::fs::create_dir_all(dir);
    let _ = run_clip(&["config"], Some(&t));
    let text = std::fs::read_to_string(&t).unwrap_or_default();
    let _ = std::fs::remove_file(&t);
    text
}

/// clip's name for an egui key (see key_info in bin/clip).
fn key_name(k: egui::Key) -> Option<String> {
    use egui::Key::*;
    let s = match k {
        Escape => "Esc", Tab => "Tab", Backspace => "Backspace", Enter => "Return",
        Insert => "Ins", Delete => "Del", Home => "Home", End => "End", PageUp => "PgUp",
        PageDown => "PgDown", ArrowLeft => "Left", ArrowRight => "Right", ArrowUp => "Up",
        ArrowDown => "Down", Space => "Space",
        _ => {
            let n = k.name();
            let ok = (n.len() == 1 && n.chars().all(|c| c.is_ascii_alphanumeric()))
                || (n.starts_with('F') && config::valid_shortcut(n));
            return ok.then(|| n.to_string());
        }
    };
    Some(s.to_string())
}

impl App {
    fn new(ctx: &egui::Context) -> App {
        let (tx, rx) = channel();
        let mut app = App {
            path: state::config_path(),
            loaded: BTreeMap::new(),
            vals: BTreeMap::new(),
            autostart_loaded: false,
            autostart: false,
            capture: None,
            capture_meta: true,
            status: String::new(),
            error: false,
            read_error: None,
            rebinding: false,
            offer_restart: false,
            monitors: Vec::new(),
            tx,
            rx,
            ctx: ctx.clone(),
            shot: std::env::var_os("CLIP_GUI_SHOT").map(|p| (PathBuf::from(p), 0)),
        };
        app.load();
        if cfg!(test) {
            return app;
        }
        let (tx, ctx) = (app.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let out = Command::new("gpu-screen-recorder").arg("--list-monitors").output();
            let list = out
                .map(|o| {
                    String::from_utf8_lossy(&o.stdout)
                        .lines()
                        .filter_map(|l| l.split('|').next().filter(|s| !s.is_empty()).map(str::to_string))
                        .collect()
                })
                .unwrap_or_default();
            let _ = tx.send(Msg::Monitors(list));
            ctx.request_repaint();
        });
        app
    }

    fn load(&mut self) {
        let (text, status, read_error) = match read_config(&self.path) {
            Ok(Some(t)) => (t, String::new(), None),
            Ok(None) => (Vec::new(), "No config file yet: showing defaults. Save creates it.".into(), None),
            Err(e) => (Vec::new(), format!("{e}. Save is off; fix that and press Reload."), Some(e)),
        };
        self.loaded = config::parse(&text);
        self.vals = self.loaded.clone();
        self.autostart_loaded = state::autostart_file().exists();
        self.autostart = self.autostart_loaded;
        self.capture = None;
        self.offer_restart = false;
        let error = read_error.is_some();
        self.read_error = read_error;
        self.set_status(status, error);
    }

    fn can_save(&self) -> bool {
        self.dirty() && self.read_error.is_none() && !self.rebinding
    }

    fn set_status(&mut self, s: String, error: bool) {
        self.status = s;
        self.error = error;
    }

    fn changes(&self) -> BTreeMap<String, String> {
        self.vals.iter().filter(|(k, v)| self.loaded.get(*k) != Some(v)).map(|(k, v)| (k.clone(), v.clone())).collect()
    }

    fn dirty(&self) -> bool {
        !self.changes().is_empty() || self.autostart != self.autostart_loaded
    }

    /// First problem with the edited values, checked before clip sees them.
    fn problem(&self, changes: &BTreeMap<String, String>) -> Option<String> {
        for (k, v) in changes {
            if let Some(why) = config::unrepresentable(v) {
                return Some(format!("{k}: the value {why}, which the config file can't hold."));
            }
            if k.starts_with("bind.") && !v.is_empty() && !config::valid_shortcut(v) {
                return Some(format!("{k}: '{v}' is not a shortcut (example: Meta+Alt+R)."));
            }
        }
        None
    }

    fn save(&mut self) {
        if self.read_error.is_some() || self.rebinding {
            return;
        }
        for (k, v) in self.vals.iter_mut() {
            if is_text_key(k) && v.trim() != v {
                *v = v.trim().to_string();
            }
        }
        let changes = self.changes();
        if let Some(p) = self.problem(&changes) {
            return self.set_status(p, true);
        }
        let (mut notes, mut errors) = (Vec::new(), Vec::new());
        if !changes.is_empty() {
            // Read again: the file may have changed (or become unreadable)
            // since load. Only a missing file means "start from the template".
            let base = match read_config(&self.path) {
                Ok(Some(t)) => t,
                Ok(None) => template(self.path.parent().unwrap_or(Path::new("."))).into_bytes(),
                Err(e) => return self.set_status(format!("Not saved: {e}"), true),
            };
            let result = config::apply(&base, &changes).and_then(|text| write_checked(&self.path, &text));
            if let Err(e) = result {
                return self.set_status(e, true);
            }
            notes.push("Saved.".to_string());
        }
        if self.autostart != self.autostart_loaded {
            match self.write_autostart() {
                Ok(()) => notes.push(if self.autostart { "Tray starts at login." } else { "Tray no longer starts at login." }.into()),
                Err(e) => errors.push(format!("Start at login not changed: {e}.")),
            }
        }
        self.load();
        let daemon = Status::read(Path::new("/proc"), &state::rundir()).daemon;
        if changes.keys().any(|k| k.starts_with("bind.")) {
            if daemon || state::binds_state().exists() {
                notes.push(UPDATING.into());
                self.rebind();
            } else {
                notes.push("Shortcuts apply when clip starts.".into());
            }
        }
        if daemon && changes.keys().any(|k| DAEMON_KEYS.contains(&k.as_str())) {
            if crate::tray::service_active() {
                self.offer_restart = true;
                notes.push("Restart the service to use the new video settings.".into());
            } else {
                notes.push("Restart `clip daemon` to use the new video settings.".into());
            }
        }
        let error = !errors.is_empty();
        notes.extend(errors);
        self.set_status(notes.join(" "), error);
    }

    fn write_autostart(&self) -> Result<(), String> {
        let f = state::autostart_file();
        if !self.autostart {
            return std::fs::remove_file(&f).or_else(|e| if f.exists() { Err(e) } else { Ok(()) }).map_err(|e| format!("{}: {e}", f.display()));
        }
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let exe = exe.canonicalize().unwrap_or(exe);
        if let Some(d) = f.parent() {
            std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
        }
        std::fs::write(&f, autostart_entry(&exe.to_string_lossy())).map_err(|e| format!("{}: {e}", f.display()))
    }

    /// `clip bind` (it removes what it bound last time first), off the UI
    /// thread. One at a time: Save is off until it reports.
    fn rebind(&mut self) {
        self.rebinding = true;
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let msg = match run_clip(&["bind"], None) {
                Ok(o) if o.status.success() && o.stderr.is_empty() => ("Shortcuts updated.".into(), false),
                Ok(o) if o.status.success() => (format!("Shortcuts updated; {}", stderr_of(&o)), false),
                Ok(o) => (format!("Shortcuts failed: {}", stderr_of(&o)), true),
                Err(e) => (format!("Shortcuts failed: {e}"), true),
            };
            let _ = tx.send(Msg::Rebind(msg.0, msg.1));
            ctx.request_repaint();
        });
    }

    fn restart_service(&mut self) {
        self.offer_restart = false;
        self.set_status("Restarting the service…".into(), false);
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let r = Command::new("systemctl").args(["--user", "restart", "clip.service"]).output();
            let msg = match r {
                Ok(o) if o.status.success() => ("Service restarted.".into(), false),
                Ok(o) => (format!("Restart failed: {}", stderr_of(&o)), true),
                Err(e) => (format!("Restart failed: {e}"), true),
            };
            let _ = tx.send(Msg::Status(msg.0, msg.1));
            ctx.request_repaint();
        });
    }

    fn browse(&self) {
        let start = state::expand_dir(&self.vals["save_dir"]);
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let mut d = rfd::FileDialog::new().set_title("clip: save folder");
            if start.is_dir() {
                d = d.set_directory(start);
            }
            if let Some(p) = d.pick_folder() {
                let _ = tx.send(Msg::Folder(p));
                ctx.request_repaint();
            }
        });
    }

    fn poll_messages(&mut self) {
        while let Ok(m) = self.rx.try_recv() {
            match m {
                Msg::Folder(p) => {
                    let home = state::home();
                    let v = match p.strip_prefix(&home) {
                        Ok(r) if r.as_os_str().is_empty() => "~".to_string(),
                        Ok(r) => format!("~/{}", r.display()),
                        Err(_) => p.display().to_string(),
                    };
                    self.vals.insert("save_dir".into(), v);
                }
                Msg::Monitors(l) => self.monitors = l,
                Msg::Status(s, e) => self.set_status(s, e),
                Msg::Rebind(s, e) => {
                    self.rebinding = false;
                    self.status = if self.status.contains(UPDATING) {
                        self.status.replace(UPDATING, &s)
                    } else {
                        format!("{} {s}", self.status).trim_start().to_string()
                    };
                    self.error |= e;
                }
            }
        }
    }

    /// While a shortcut field records, the next key press (with Ctrl/Alt/
    /// Shift) becomes its value. egui can't see Meta on Linux, hence the toggle.
    fn capture_keys(&mut self, ctx: &egui::Context) {
        let Some(field) = self.capture else { return };
        let events = ctx.input(|i| i.events.clone());
        for e in events {
            if let egui::Event::Key { key, pressed: true, modifiers, .. } = e {
                if key == egui::Key::Escape && !modifiers.any() {
                    self.capture = None;
                    return;
                }
                let Some(name) = key_name(key) else { continue };
                let mut parts = Vec::new();
                if self.capture_meta {
                    parts.push("Meta");
                }
                if modifiers.ctrl {
                    parts.push("Ctrl");
                }
                if modifiers.alt {
                    parts.push("Alt");
                }
                if modifiers.shift {
                    parts.push("Shift");
                }
                parts.push(&name);
                self.vals.insert(format!("bind.{field}"), parts.join("+"));
                self.capture = None;
                return;
            }
        }
    }

    fn self_shot(&mut self, ctx: &egui::Context) {
        let Some((path, frames)) = &mut self.shot else { return };
        *frames += 1;
        if *frames == 5 {
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        ctx.request_repaint();
        let image = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Screenshot { image, .. } => Some(image.clone()),
                _ => None,
            })
        });
        let Some(img) = image else { return };
        let r = std::fs::File::create(&*path).map_err(|e| e.to_string()).and_then(|f| {
            let mut enc = png::Encoder::new(std::io::BufWriter::new(f), img.width() as u32, img.height() as u32);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut w = enc.write_header().map_err(|e| e.to_string())?;
            w.write_image_data(img.as_raw()).map_err(|e| e.to_string())
        });
        if let Err(e) = r {
            eprintln!("clip-gui: {}: {e}", path.display());
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn val(&mut self, k: &str) -> &mut String {
        self.vals.entry(k.to_string()).or_default()
    }
}

fn combo(ui: &mut egui::Ui, id: &str, v: &mut String, options: &[&str]) {
    egui::ComboBox::from_id_salt(id).selected_text(v.as_str()).show_ui(ui, |ui| {
        for o in options {
            ui.selectable_value(v, o.to_string(), *o);
        }
    });
}

/// The file's value when clip would refuse it, so the widget's fallback
/// (unchecked, 60, …) isn't taken for what the file says.
fn raw_note(ui: &mut egui::Ui, key: &str, v: &str) {
    if !config::valid_value(key, v) {
        ui.colored_label(Color32::from_rgb(0xe0, 0x50, 0x50), format!("config has '{v}'"))
            .on_hover_text("clip refuses this value; pick a valid one before saving");
    }
}

fn combo_row(ui: &mut egui::Ui, key: &str, v: &mut String, options: &[&str]) {
    ui.horizontal(|ui| {
        combo(ui, key, v, options);
        raw_note(ui, key, v);
    });
}

fn checkbox(ui: &mut egui::Ui, key: &str, v: &mut String, label: &str) {
    ui.horizontal(|ui| {
        let mut b = config::yes(v);
        if ui.checkbox(&mut b, label).changed() {
            *v = b.to_string();
        }
        raw_note(ui, key, v);
    });
}

fn number(ui: &mut egui::Ui, key: &str, v: &mut String, range: std::ops::RangeInclusive<u32>, fallback: u32, suffix: &str) {
    let mut n: u32 = v.parse().ok().filter(|n| range.contains(n)).unwrap_or(fallback);
    if ui.add(egui::DragValue::new(&mut n).range(range).suffix(suffix)).changed() {
        *v = n.to_string();
    }
    raw_note(ui, key, v);
}

fn text_with_menu(ui: &mut egui::Ui, v: &mut String, width: f32, options: &[String]) {
    ui.add(egui::TextEdit::singleline(v).desired_width(width));
    ui.menu_button("⏷", |ui| {
        for o in options {
            if ui.button(o).clicked() {
                *v = o.clone();
                ui.close();
            }
        }
    });
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(8.0);
    ui.label(RichText::new(title).strong().size(15.0));
    ui.separator();
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_messages();
        self.capture_keys(ctx);
        self.self_shot(ctx);
        let red = Color32::from_rgb(0xe0, 0x50, 0x50);

        egui::TopBottomPanel::bottom("buttons").show(ctx, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.add_enabled(self.can_save(), egui::Button::new("Save")).clicked() {
                    self.save();
                }
                let reload = self.read_error.is_some();
                let label = if reload { "Reload" } else { "Revert" };
                if ui.add_enabled(reload || self.dirty(), egui::Button::new(label)).clicked() {
                    self.load();
                }
                if self.rebinding {
                    ui.spinner();
                }
                if self.offer_restart && ui.button("Restart service").clicked() {
                    self.restart_service();
                }
            });
            let text = RichText::new(&self.status);
            ui.add(egui::Label::new(if self.error { text.color(red) } else { text }).wrap());
            ui.add_space(4.0);
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                ui.label(RichText::new(format!("Config: {}", self.path.display())).small().weak());

                section(ui, "Saving");
                egui::Grid::new("saving").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    ui.label("Save folder");
                    ui.horizontal(|ui| {
                        ui.add(egui::TextEdit::singleline(self.val("save_dir")).desired_width(260.0));
                        if ui.button("Browse…").clicked() {
                            self.browse();
                        }
                    });
                    ui.end_row();
                    let dir = self.vals["save_dir"].clone();
                    if !state::expand_dir(&dir).is_absolute() {
                        ui.label("");
                        ui.colored_label(red, "Must be an absolute path or start with ~/");
                        ui.end_row();
                    }
                    ui.label("Image format");
                    combo_row(ui, "image_format", self.val("image_format"), &["png", "jpg", "webp"]);
                    ui.end_row();
                    ui.label("Video format");
                    combo_row(ui, "video_format", self.val("video_format"), &["mp4", "webm", "mkv"]);
                    ui.end_row();
                    ui.label("");
                    ui.vertical(|ui| {
                        checkbox(ui, "clipboard", self.val("clipboard"), "Copy to clipboard");
                        checkbox(ui, "notify", self.val("notify"), "Notification with preview");
                    });
                    ui.end_row();
                });

                section(ui, "Video");
                egui::Grid::new("video").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    ui.label("");
                    checkbox(ui, "replay", self.val("replay"), "Keep a replay buffer running");
                    ui.end_row();
                    ui.label("Replay length");
                    ui.horizontal(|ui| {
                        let v = self.val("replay_seconds");
                        let mut auto = v == "auto";
                        if ui.checkbox(&mut auto, "Auto").changed() {
                            *v = if auto { "auto".into() } else { "60".into() };
                        }
                        if !auto {
                            number(ui, "replay_seconds", v, 5..=1800, 60, " s");
                        } else {
                            ui.label(RichText::new("60 s with 8 GiB free RAM, else 30 s").weak());
                        }
                    });
                    ui.end_row();
                    ui.label("Source");
                    let mut opts: Vec<String> = ["auto", "portal", "screen"].map(String::from).to_vec();
                    opts.extend(self.monitors.iter().cloned());
                    text_with_menu(ui, self.val("video_source"), 180.0, &opts);
                    ui.end_row();
                    ui.label("Frame rate");
                    ui.horizontal(|ui| number(ui, "video_fps", self.val("video_fps"), 1..=240, 60, " fps"));
                    ui.end_row();
                    ui.label("Quality");
                    combo_row(ui, "video_quality", self.val("video_quality"), &["medium", "high", "very_high", "ultra"]);
                    ui.end_row();
                    ui.label("Codec");
                    combo_row(ui, "video_codec", self.val("video_codec"), &[
                        "auto", "h264", "hevc", "av1", "vp8", "vp9", "hevc_hdr", "av1_hdr", "hevc_10bit", "av1_10bit",
                    ]);
                    ui.end_row();
                    ui.label("Audio");
                    let opts = ["default_output", "default_input", "none"].map(String::from);
                    text_with_menu(ui, self.val("audio"), 180.0, &opts);
                    ui.end_row();
                });

                section(ui, "Shortcuts");
                egui::Grid::new("binds").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    let names = ["Region", "Window", "Monitor", "All monitors", "Save replay", "Record"];
                    for (a, name) in ACTIONS.iter().zip(names) {
                        let key = format!("bind.{a}");
                        ui.label(name);
                        ui.horizontal(|ui| {
                            let v = self.vals.entry(key.clone()).or_default();
                            ui.add(egui::TextEdit::singleline(v).desired_width(150.0).hint_text("none"));
                            let recording = self.capture == Some(*a);
                            let label = if recording { "Press keys…" } else { "Record" };
                            let r = ui.add(egui::Button::new(label).selected(recording));
                            if r.clicked() {
                                self.capture = if recording { None } else { Some(*a) };
                                r.surrender_focus();
                            }
                            if ui.button("Clear").clicked() {
                                self.vals.insert(key.clone(), String::new());
                            }
                            let v = &self.vals[&key];
                            if !v.is_empty() && !config::valid_shortcut(v) {
                                ui.colored_label(red, "not a shortcut");
                            }
                        });
                        ui.end_row();
                    }
                });
                if self.capture.is_some() {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Press the key with Ctrl/Alt/Shift; Esc cancels.").weak());
                        ui.checkbox(&mut self.capture_meta, "add Meta");
                    });
                }
                ui.label(
                    RichText::new("Meta, Ctrl, Alt, Shift + one key (A–Z, 0–9, F1–F35, Print, Space, Home…).")
                        .small()
                        .weak(),
                );

                section(ui, "Tray");
                ui.checkbox(&mut self.autostart, "Start tray at login");
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_quoting_matches_bash() {
        assert_eq!(desktop_exec_arg("/home/u/.local/bin/clip-gui"), "/home/u/.local/bin/clip-gui");
        // Same expectation as the "quoting helpers" check in tests/smoke.sh.
        assert_eq!(desktop_exec_arg("/a b/$x%\"q\\"), "\"/a b/\\\\$x%%\\\\\"q\\\\\\\\\"");
    }

    #[test]
    fn key_names() {
        assert_eq!(key_name(egui::Key::R).as_deref(), Some("R"));
        assert_eq!(key_name(egui::Key::Num5).as_deref(), Some("5"));
        assert_eq!(key_name(egui::Key::F12).as_deref(), Some("F12"));
        assert_eq!(key_name(egui::Key::PageUp).as_deref(), Some("PgUp"));
        assert_eq!(key_name(egui::Key::Comma), None);
        for k in ["R", "5", "F12", "PgUp", "Space", "Return", "Del"] {
            assert!(config::valid_shortcut(&format!("Meta+{k}")), "{k}");
        }
    }

    #[test]
    fn autostart_entry_validates() {
        let d = crate::testutil::tempdir("auto");
        let f = d.join("clip-gui.desktop");
        std::fs::write(&f, autostart_entry("/opt/my clip/clip-gui")).unwrap();
        let text = std::fs::read_to_string(&f).unwrap();
        assert!(text.contains("Exec=\"/opt/my clip/clip-gui\"\n"), "{text}");
        assert!(text.contains("X-GNOME-Autostart-enabled=true"));
        if let Ok(o) = Command::new("desktop-file-validate").arg(&f).output() {
            assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stdout));
        }
    }

    fn clip_script() -> Option<PathBuf> {
        let clip = Path::new(env!("CARGO_MANIFEST_DIR")).join("../bin/clip");
        clip.exists().then_some(clip)
    }

    /// Temp XDG dirs, dry-run clip and a desktop with no shortcut backend,
    /// so nothing here can touch the real session. CLIP_BIN logs each call
    /// to D/calls, then runs bin/clip.
    fn test_env(d: &Path, clip: &Path) {
        for (k, sub) in [("XDG_CONFIG_HOME", "cfg"), ("XDG_STATE_HOME", "state"), ("XDG_RUNTIME_DIR", "run"), ("XDG_CACHE_HOME", "cache")] {
            std::fs::create_dir_all(d.join(sub)).unwrap();
            std::env::set_var(k, d.join(sub));
        }
        let wrap = d.join("clip-wrap");
        std::fs::write(&wrap, format!("#!/bin/sh\necho \"$*\" >>'{}/calls'\nexec '{}' \"$@\"\n", d.display(), clip.display())).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&wrap, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::env::set_var("CLIP_BIN", &wrap);
        std::env::set_var("CLIP_DRY_RUN", "1");
        std::env::set_var("XDG_CURRENT_DESKTOP", "clip-test");
        std::env::remove_var("CLIP_CONFIG");
        std::env::remove_var("HYPRLAND_INSTANCE_SIGNATURE");
        std::env::remove_var("SWAYSOCK");
    }

    fn app_at(path: &Path) -> App {
        let mut app = App::new(&egui::Context::default());
        app.path = path.to_path_buf();
        app.load();
        app
    }

    fn wait_rebind(app: &mut App) {
        for _ in 0..200 {
            app.poll_messages();
            if !app.rebinding {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        panic!("clip bind never reported: {}", app.status);
    }

    /// Nova #1: a config we can't read (EACCES, or unreadable by the time
    /// Save re-reads it) must never be replaced by the template.
    #[test]
    fn unreadable_config_is_never_replaced() {
        use std::os::unix::fs::PermissionsExt;
        let Some(clip) = clip_script() else { return };
        let _env = crate::testutil::env_lock();
        let d = crate::testutil::tempdir("unread");
        test_env(&d, &clip);
        let cfg = d.join("cfg/clip/config");
        std::fs::create_dir_all(cfg.parent().unwrap()).unwrap();
        let mine = "# mine\nvideo_fps = 30\n";
        std::fs::write(&cfg, mine).unwrap();
        let mode = |m| std::fs::set_permissions(&cfg, std::fs::Permissions::from_mode(m)).unwrap();
        mode(0o000);
        if std::fs::read(&cfg).is_ok() {
            return mode(0o644); // root reads anything; nothing to test
        }

        // Unreadable at load: error shown, Save off, save() writes nothing.
        let mut app = app_at(&cfg);
        assert!(app.error && app.read_error.is_some() && app.status.contains("Permission denied"), "{}", app.status);
        app.vals.insert("image_format".into(), "jpg".into());
        assert!(app.dirty() && !app.can_save());
        app.save();
        mode(0o644);
        assert_eq!(std::fs::read_to_string(&cfg).unwrap(), mine);

        // Readable at load, unreadable when Save re-reads it.
        let mut app = app_at(&cfg);
        assert!(app.read_error.is_none() && app.vals["video_fps"] == "30");
        app.vals.insert("image_format".into(), "jpg".into());
        mode(0o000);
        app.save();
        mode(0o644);
        assert_eq!(std::fs::read_to_string(&cfg).unwrap(), mine, "template replaced the user's config");
        assert!(app.error && app.status.starts_with("Not saved: Can't read"), "{}", app.status);
    }

    /// A Latin-1 comment doesn't make the file "missing" (it used to), and
    /// Save keeps every other byte.
    #[test]
    fn non_utf8_config_is_edited_in_place() {
        let Some(clip) = clip_script() else { return };
        let _env = crate::testutil::env_lock();
        let d = crate::testutil::tempdir("latin1");
        test_env(&d, &clip);
        let cfg = d.join("config");
        std::fs::write(&cfg, b"# caf\xe9\r\nvideo_fps = 30   # \xe9\nimage_format = png\n").unwrap();
        let mut app = app_at(&cfg);
        let (load_status, fps) = (app.status.clone(), app.vals["video_fps"].clone());
        app.vals.insert("image_format".into(), "jpg".into());
        app.save();
        let after = std::fs::read(&cfg).unwrap();
        assert_eq!(String::from_utf8_lossy(&after), "# caf\u{fffd}\r\nvideo_fps = 30   # \u{fffd}\nimage_format = jpg\n");
        assert_eq!(after, b"# caf\xe9\r\nvideo_fps = 30   # \xe9\nimage_format = jpg\n");
        assert!(load_status.is_empty() && fps == "30", "{load_status}");
        assert!(!app.error, "{}", app.status);
    }

    /// The Save button end to end, headless, in temp XDG dirs.
    #[test]
    fn save_flow() {
        let Some(clip) = clip_script() else { return };
        let _env = crate::testutil::env_lock();
        let d = crate::testutil::tempdir("save");
        test_env(&d, &clip);
        let mut app = app_at(&d.join("cfg/clip/config"));
        assert!(app.status.contains("No config file yet"));

        // New file: clip's template plus the changes; binds wait for clip.
        // Text fields are trimmed.
        app.vals.insert("image_format".into(), "jpg".into());
        app.vals.insert("bind.window".into(), String::new());
        app.vals.insert("video_source".into(), "  DP-1 ".into());
        app.save();
        assert!(!app.error, "{}", app.status);
        assert!(app.status.contains("Shortcuts apply when clip starts"), "{}", app.status);
        let text = std::fs::read_to_string(&app.path).unwrap();
        assert!(text.starts_with("# clip config"), "{text}");
        assert!(text.contains("\nimage_format = jpg         # png | jpg | webp\n"), "{text}");
        assert!(text.contains("\nbind.window =\n"), "{text}");
        assert!(text.contains("\nvideo_source = DP-1        # auto"), "{text}");
        assert!(!app.dirty());

        // clip check refuses: file untouched, error shown.
        app.vals.insert("video_fps".into(), "999".into());
        app.save();
        assert!(app.error && app.status.contains("video_fps must be 1-240"), "{}", app.status);
        assert_eq!(std::fs::read_to_string(&app.path).unwrap(), text);

        // Refused before clip sees it.
        app.load();
        app.vals.insert("audio".into(), "a #b".into());
        app.save();
        assert!(app.error && app.status.contains("audio:"), "{}", app.status);
        app.load();
        app.vals.insert("bind.all".into(), "Meta+Nope".into());
        app.save();
        assert!(app.error && app.status.contains("not a shortcut"), "{}", app.status);
        assert_eq!(std::fs::read_to_string(&app.path).unwrap(), text);

        // Autostart on, then off.
        app.load();
        app.autostart = true;
        app.save();
        let auto = d.join("cfg/autostart/clip-gui.desktop");
        assert!(std::fs::read_to_string(&auto).unwrap().contains("X-GNOME-Autostart-enabled=true"));
        app.autostart = false;
        app.save();
        assert!(!auto.exists() && !app.error, "{}", app.status);
    }

    /// A value clip refuses is shown as the file has it (not as the
    /// widget's fallback), and is what Save keeps.
    #[test]
    fn unparseable_value_is_kept_raw() {
        let Some(clip) = clip_script() else { return };
        let _env = crate::testutil::env_lock();
        let d = crate::testutil::tempdir("raw");
        test_env(&d, &clip);
        let cfg = d.join("config");
        std::fs::write(&cfg, "video_fps = fast\nnotify = maybe\n").unwrap();
        let mut app = app_at(&cfg);
        assert_eq!((app.vals["video_fps"].as_str(), app.vals["notify"].as_str()), ("fast", "maybe"));
        assert!(!config::valid_value("video_fps", &app.vals["video_fps"]));
        app.vals.insert("image_format".into(), "jpg".into());
        app.save();
        assert!(app.error && app.status.contains("notify must be true or false"), "{}", app.status);
        app.vals.insert("video_fps".into(), "30".into());
        app.vals.insert("notify".into(), "false".into());
        app.save();
        assert!(!app.error, "{}", app.status);
        assert_eq!(std::fs::read_to_string(&cfg).unwrap(), "video_fps = 30\nnotify = false\nimage_format = jpg\n");
    }

    /// Nova #4 + #8: Save runs only `clip bind` (no unbind), one at a time;
    /// a failed autostart write after the config was saved still reloads
    /// and rebinds, and the rebind result is added to the status.
    #[test]
    fn rebind_once_and_errors_collected() {
        let Some(clip) = clip_script() else { return };
        let _env = crate::testutil::env_lock();
        let d = crate::testutil::tempdir("rebind");
        test_env(&d, &clip);
        std::fs::create_dir_all(d.join("state/clip")).unwrap();
        std::fs::write(d.join("state/clip/binds"), "manual\n").unwrap(); // clip has bound before
        std::fs::write(d.join("cfg/autostart"), "a file, so the autostart dir can't be made").unwrap();
        let cfg = d.join("config");
        std::fs::write(&cfg, "bind.region = Meta+Alt+R\n").unwrap();
        let mut app = app_at(&cfg);
        app.vals.insert("bind.region".into(), "Meta+Alt+F9".into());
        app.autostart = true;
        app.save();
        assert_eq!(std::fs::read_to_string(&cfg).unwrap(), "bind.region = Meta+Alt+F9\n");
        assert!(app.error && app.status.starts_with("Saved. Updating shortcuts… Start at login not changed:"), "{}", app.status);
        assert!(app.rebinding && !app.can_save());
        // A second Save while it runs does nothing.
        app.vals.insert("bind.region".into(), "Meta+Alt+F10".into());
        app.save();
        assert_eq!(std::fs::read_to_string(&cfg).unwrap(), "bind.region = Meta+Alt+F9\n");
        wait_rebind(&mut app);
        assert!(app.status.starts_with("Saved. Shortcuts updated"), "{}", app.status);
        assert!(app.status.contains("Start at login not changed:") && app.error, "{}", app.status);
        let calls = std::fs::read_to_string(d.join("calls")).unwrap();
        let runs: Vec<&str> = calls.lines().filter(|l| *l != "check" && *l != "config").collect();
        assert_eq!(runs, ["bind"], "{calls}");
    }

    /// Nova #5: a dangling symlinked config gets its target written; the
    /// link stays a link.
    #[test]
    fn symlinked_config_writes_target() {
        let Some(clip) = clip_script() else { return };
        let _env = crate::testutil::env_lock();
        let d = crate::testutil::tempdir("link");
        test_env(&d, &clip);
        std::fs::create_dir_all(d.join("dots")).unwrap();
        std::fs::create_dir_all(d.join("cfg/clip")).unwrap();
        let link = d.join("cfg/clip/config");
        std::os::unix::fs::symlink("../../dots/clip.conf", &link).unwrap();
        assert!(resolve_link(&link).unwrap().ends_with("cfg/clip/../../dots/clip.conf"));
        let mut app = app_at(&link);
        assert!(app.read_error.is_none() && app.status.contains("No config file yet"), "{}", app.status);
        app.vals.insert("video_fps".into(), "30".into());
        app.save();
        assert!(!app.error, "{}", app.status);
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink(), "link was replaced");
        let target = std::fs::read_to_string(d.join("dots/clip.conf")).unwrap();
        assert!(target.starts_with("# clip config") && target.contains("\nvideo_fps = 30 "), "{target}");
    }

    /// Nova #6: the temp file is new (a planted one isn't reused), keeps
    /// the config's mode, and a new config gets 0644 minus the umask.
    #[test]
    fn write_checked_modes() {
        use std::os::unix::fs::PermissionsExt;
        let Some(clip) = clip_script() else { return };
        let _env = crate::testutil::env_lock();
        let d = crate::testutil::tempdir("mode");
        test_env(&d, &clip);
        let cfg = d.join("config");
        std::fs::write(&cfg, "video_fps = 30\n").unwrap();
        std::fs::set_permissions(&cfg, std::fs::Permissions::from_mode(0o600)).unwrap();
        let planted = d.join(format!(".config.clip-gui-{}-0.tmp", std::process::id()));
        std::fs::write(&planted, "planted").unwrap();
        write_checked(&cfg, b"video_fps = 45\n").unwrap();
        assert_eq!(std::fs::metadata(&cfg).unwrap().permissions().mode() & 0o7777, 0o600);
        assert_eq!(std::fs::read_to_string(&planted).unwrap(), "planted");
        assert_eq!(std::fs::read_to_string(&cfg).unwrap(), "video_fps = 45\n");

        let umask = std::fs::read_to_string("/proc/self/status")
            .unwrap()
            .lines()
            .find_map(|l| l.strip_prefix("Umask:").map(|v| u32::from_str_radix(v.trim(), 8).unwrap()))
            .unwrap();
        let new = d.join("new/config");
        write_checked(&new, b"video_fps = 45\n").unwrap();
        assert_eq!(std::fs::metadata(&new).unwrap().permissions().mode() & 0o7777, 0o644 & !umask);
    }

    /// write_checked against the real bin/clip: a bad value leaves the file
    /// alone, a good one replaces it (skipped without bash).
    #[test]
    fn write_checked_uses_clip_check() {
        let Some(clip) = clip_script() else { return };
        let _env = crate::testutil::env_lock();
        let d = crate::testutil::tempdir("wc");
        test_env(&d, &clip);
        let cfg = d.join("config");
        std::fs::write(&cfg, "video_fps = 30 # mine\n").unwrap();
        let err = write_checked(&cfg, b"video_fps = fast # mine\n").unwrap_err();
        assert!(err.contains("video_fps must be 1-240"), "{err}");
        assert_eq!(std::fs::read_to_string(&cfg).unwrap(), "video_fps = 30 # mine\n");
        write_checked(&cfg, b"video_fps = 120 # mine\n").unwrap();
        assert_eq!(std::fs::read_to_string(&cfg).unwrap(), "video_fps = 120 # mine\n");
        let mut left: Vec<_> = std::fs::read_dir(&*d).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name()).collect();
        left.retain(|n| n.to_string_lossy().starts_with(".config"));
        assert!(left.is_empty(), "temp files left behind: {left:?}");

        let t = template(&d);
        assert!(t.contains("bind.record = Meta+Alt+V") && t.contains("# clip config"));
        let defaults = config::parse(&t);
        for (k, v) in config::DEFAULTS {
            assert_eq!(defaults[*k], *v, "default_config() and DEFAULTS disagree on {k}");
        }
    }
}
