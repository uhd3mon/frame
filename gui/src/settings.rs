//! The settings window: edits the clipped config, checks it with `clipped check`
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
    Monitors(Vec<Monitor>),
    Status(String, bool),
    /// Result of `clipped bind`; it replaces "Updating shortcuts…" in the status.
    Rebind(String, bool),
}

/// self_shot's frame count once the screenshot is asked for.
const SHOT_SENT: u32 = u32::MAX;

const UPDATING: &str = "Updating shortcuts…";

const ACTION_NAMES: [&str; 6] = ["Region", "Window", "Monitor", "All monitors", "Save replay", "Record"];

/// The keys the editor offers (bin/clipped key_info takes them all, plus
/// F25–F35 and spelling variants).
fn key_choices() -> Vec<String> {
    let mut v: Vec<String> = [
        "Print", "Pause", "Ins", "Del", "Home", "End", "PgUp", "PgDown", "Left", "Right", "Up", "Down", "Space",
        "Tab", "Esc", "Backspace", "Return",
    ]
    .map(String::from)
    .to_vec();
    v.extend((1..=24).map(|n| format!("F{n}")));
    v.extend(('A'..='Z').map(String::from));
    v.extend(('0'..='9').map(String::from));
    v
}

/// The editor's name for a key part ("print", "PageUp", "f30"), None if
/// clipped doesn't know it.
fn canonical_key(part: &str) -> Option<String> {
    let l = part.to_ascii_lowercase();
    let l = match l.as_str() {
        "escape" => "esc", "enter" => "return", "insert" => "ins", "delete" => "del",
        "pageup" => "pgup", "pagedown" => "pgdown", x => x,
    };
    if let Some(k) = key_choices().into_iter().find(|k| k.to_ascii_lowercase() == l) {
        return Some(k);
    }
    // F25–F35: clipped takes them, the list stops at F24.
    (l.starts_with('f') && config::valid_shortcut(l)).then(|| l.to_ascii_uppercase())
}

/// A shortcut as the editor's checkboxes and key list see it.
#[derive(Debug, Default, PartialEq)]
struct Shortcut {
    meta: bool,
    ctrl: bool,
    alt: bool,
    shift: bool,
    key: String,
}

impl Shortcut {
    /// Prefill from a field: parts in any case, Super/Win = Meta, unknown
    /// keys dropped. An empty field starts with Meta on.
    fn parse(s: &str) -> Shortcut {
        let s: String = s.chars().filter(|c| !c.is_whitespace()).collect();
        if s.is_empty() {
            return Shortcut { meta: true, ..Default::default() };
        }
        let mut sc = Shortcut::default();
        for part in s.split('+') {
            match part.to_ascii_lowercase().as_str() {
                "meta" | "super" | "win" => sc.meta = true,
                "ctrl" | "control" => sc.ctrl = true,
                "alt" => sc.alt = true,
                "shift" => sc.shift = true,
                _ if sc.key.is_empty() => sc.key = canonical_key(part).unwrap_or_default(),
                _ => {}
            }
        }
        sc
    }

    /// "Meta+Ctrl+Alt+Shift+Key", the order clipped's defaults use.
    fn text(&self) -> String {
        let mods = [(self.meta, "Meta"), (self.ctrl, "Ctrl"), (self.alt, "Alt"), (self.shift, "Shift")];
        let mut parts: Vec<&str> = mods.iter().filter(|m| m.0).map(|m| m.1).collect();
        if !self.key.is_empty() {
            parts.push(&self.key);
        }
        parts.join("+")
    }
}

/// One key as Record sees it.
#[derive(Clone, Debug, PartialEq)]
enum Part {
    Meta,
    Ctrl,
    Alt,
    Shift,
    Key(String),
}

/// A key event for Record, from winit (see KeyTap).
#[derive(Clone, Debug, PartialEq)]
enum KeyEv {
    Down(Part),
    Up(Part),
    /// A key clipped can't bind (comma, AltGr, …) was pressed.
    Other,
    /// The window lost focus: releases may never come.
    Reset,
}

enum Rec {
    Going,
    Done(Shortcut),
    Cancel,
}

/// Record: collects up to 3 keys of one combo. It ends when 3 are held,
/// or when a key of a combo with a non-modifier key goes up. A combo of
/// modifiers only stays shown (for Stop) until the next press starts over.
#[derive(Debug, Default)]
struct Recorder {
    held: Vec<Part>,
    combo: Vec<Part>,
    refused: bool,
}

impl Recorder {
    fn has_key(&self) -> bool {
        self.combo.iter().any(|p| matches!(p, Part::Key(_)))
    }

    fn feed(&mut self, ev: KeyEv) -> Rec {
        match ev {
            KeyEv::Down(p) => {
                if self.held.is_empty() {
                    self.combo.clear();
                    self.refused = false;
                }
                if p == Part::Key("Esc".into()) && self.held.is_empty() {
                    return Rec::Cancel;
                }
                if self.held.contains(&p) {
                    return Rec::Going;
                }
                self.held.push(p.clone());
                if self.combo.len() < 3 && !self.combo.contains(&p) {
                    // clipped takes one key: a second one replaces the first.
                    if matches!(p, Part::Key(_)) {
                        self.combo.retain(|q| !matches!(q, Part::Key(_)));
                    }
                    self.combo.push(p);
                }
                if self.combo.len() == 3 {
                    return Rec::Done(self.shortcut());
                }
            }
            KeyEv::Up(p) => {
                // Keys already down when Record started don't count.
                if !self.held.contains(&p) {
                    return Rec::Going;
                }
                self.held.retain(|q| *q != p);
                if self.has_key() {
                    return Rec::Done(self.shortcut());
                }
            }
            KeyEv::Other => self.refused = true,
            KeyEv::Reset => self.held.clear(),
        }
        Rec::Going
    }

    /// What Stop takes: the combo so far (maybe no key yet).
    fn shortcut(&self) -> Shortcut {
        let has = |p: Part| self.combo.contains(&p);
        let key = self.combo.iter().find_map(|p| match p {
            Part::Key(k) => Some(k.clone()),
            _ => None,
        });
        Shortcut { meta: has(Part::Meta), ctrl: has(Part::Ctrl), alt: has(Part::Alt), shift: has(Part::Shift), key: key.unwrap_or_default() }
    }
}

/// Record's name for a winit key (layout-aware, Shift not applied).
fn winit_part(k: &winit::keyboard::Key) -> Option<Part> {
    use winit::keyboard::{Key, NamedKey};
    match k {
        Key::Named(NamedKey::Super | NamedKey::Meta) => Some(Part::Meta),
        Key::Named(NamedKey::Control) => Some(Part::Ctrl),
        Key::Named(NamedKey::Alt) => Some(Part::Alt),
        Key::Named(NamedKey::Shift) => Some(Part::Shift),
        Key::Named(n) => {
            let name = format!("{n:?}");
            let name = match name.as_str() {
                "PrintScreen" => "Print",
                n => n.strip_prefix("Arrow").unwrap_or(n),
            };
            canonical_key(name).map(Part::Key)
        }
        Key::Character(c) if c.len() == 1 => canonical_key(c).map(Part::Key),
        _ => None,
    }
}

type KeyQueue = std::sync::Arc<std::sync::Mutex<Vec<KeyEv>>>;

/// Wraps eframe's winit handler and copies key events into a queue before
/// egui sees them: egui-winit 0.33 has no Print key and reports Super only
/// on macOS, but winit has both.
struct KeyTap<'a> {
    inner: eframe::EframeWinitApplication<'a>,
    keys: KeyQueue,
}

impl winit::application::ApplicationHandler<eframe::UserEvent> for KeyTap<'_> {
    fn resumed(&mut self, el: &winit::event_loop::ActiveEventLoop) {
        self.inner.resumed(el);
    }

    fn window_event(&mut self, el: &winit::event_loop::ActiveEventLoop, id: winit::window::WindowId, event: winit::event::WindowEvent) {
        use winit::event::{ElementState, WindowEvent};
        use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
        let ev = match &event {
            WindowEvent::KeyboardInput { event: k, is_synthetic: false, .. } if !k.repeat => {
                let pressed = k.state == ElementState::Pressed;
                match (winit_part(&k.key_without_modifiers()), pressed) {
                    (Some(p), true) => Some(KeyEv::Down(p)),
                    (Some(p), false) => Some(KeyEv::Up(p)),
                    (None, true) => Some(KeyEv::Other),
                    (None, false) => None,
                }
            }
            WindowEvent::Focused(false) => Some(KeyEv::Reset),
            _ => None,
        };
        if let (Some(ev), Ok(mut q)) = (ev, self.keys.lock()) {
            if q.len() < 64 {
                q.push(ev);
            }
        }
        self.inner.window_event(el, id, event);
    }

    fn new_events(&mut self, el: &winit::event_loop::ActiveEventLoop, cause: winit::event::StartCause) {
        self.inner.new_events(el, cause);
    }

    fn user_event(&mut self, el: &winit::event_loop::ActiveEventLoop, event: eframe::UserEvent) {
        self.inner.user_event(el, event);
    }

    fn device_event(&mut self, el: &winit::event_loop::ActiveEventLoop, id: winit::event::DeviceId, event: winit::event::DeviceEvent) {
        self.inner.device_event(el, id, event);
    }

    fn about_to_wait(&mut self, el: &winit::event_loop::ActiveEventLoop) {
        self.inner.about_to_wait(el);
    }

    fn suspended(&mut self, el: &winit::event_loop::ActiveEventLoop) {
        self.inner.suspended(el);
    }

    fn exiting(&mut self, el: &winit::event_loop::ActiveEventLoop) {
        self.inner.exiting(el);
    }

    fn memory_warning(&mut self, el: &winit::event_loop::ActiveEventLoop) {
        self.inner.memory_warning(el);
    }
}

/// The open shortcut editor: which bind.* it edits and the value so far.
struct Editor {
    field: &'static str,
    sc: Shortcut,
    /// Record is listening.
    rec: Option<Recorder>,
}

struct App {
    path: PathBuf,
    loaded: BTreeMap<String, String>,
    vals: BTreeMap<String, String>,
    autostart_loaded: bool,
    autostart: bool,
    editor: Option<Editor>,
    /// Key events from KeyTap; None in tests (Record is off then).
    keys: Option<KeyQueue>,
    status: String,
    error: bool,
    /// Set when the config exists but can't be read: Save is off, so the
    /// template can never replace a file we failed to read.
    read_error: Option<String>,
    /// A `clipped bind` is in flight; Save waits for it.
    rebinding: bool,
    offer_restart: bool,
    /// From `clipped monitors`; None until it answers.
    monitors: Option<Vec<Monitor>>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    ctx: egui::Context,
    /// CLIPPED_GUI_SHOT=FILE.png: save this window's own frame and quit (docs, review).
    shot: Option<(PathBuf, u32)>,
}

pub fn run() -> i32 {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Clipped settings")
        .with_app_id("clipped")
        .with_inner_size([560.0, 720.0])
        .with_min_inner_size([440.0, 360.0]);
    if let Some((w, h, rgba)) = crate::icons::rgba(crate::icons::APP_64) {
        viewport = viewport.with_icon(egui::IconData { rgba, width: w, height: h });
    }
    let opts = eframe::NativeOptions { viewport, ..Default::default() };
    // run_native, but on our own event loop so KeyTap sees winit's keys.
    let event_loop = match winit::event_loop::EventLoop::<eframe::UserEvent>::with_user_event().build() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("clipped-gui: {e}");
            return 1;
        }
    };
    let keys = KeyQueue::default();
    let app_keys = keys.clone();
    let creator: eframe::AppCreator = Box::new(move |cc| {
        let mut app = App::new(&cc.egui_ctx);
        app.keys = Some(app_keys);
        Ok(Box::new(app))
    });
    let mut tap = KeyTap { inner: eframe::create_native("Clipped", opts, creator, &event_loop), keys };
    match event_loop.run_app(&mut tap) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("clipped-gui: {e}");
            1
        }
    }
}

fn run_clip(args: &[&str], config: Option<&Path>) -> Result<Output, String> {
    let bin = crate::clip_bin();
    let mut c = Command::new(&bin);
    c.args(args);
    if let Some(p) = config {
        c.env("CLIPPED_CONFIG", p);
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

/// Escape a path for a desktop entry Exec= line (same as desktop_arg in bin/clipped).
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
        "[Desktop Entry]\nType=Application\nName=Clipped\nComment=clipped tray icon\nExec={}\nIcon=clipped\n\
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
/// one (same mode, synced), `clipped check` on it, then rename. The real
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
        let tmp = dir.join(format!(".{name}.clipped-gui-{}-{i}.tmp", std::process::id()));
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

/// The text `clipped config` would write, made in a temp path (not the real one).
fn template(dir: &Path) -> String {
    let t = dir.join(format!(".clipped-gui-template-{}", std::process::id()));
    let _ = std::fs::create_dir_all(dir);
    let _ = run_clip(&["config"], Some(&t));
    let text = std::fs::read_to_string(&t).unwrap_or_default();
    let _ = std::fs::remove_file(&t);
    text
}

/// clipped's name for an egui key (see key_info in bin/clipped).
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
            editor: None,
            keys: None,
            status: String::new(),
            error: false,
            read_error: None,
            rebinding: false,
            offer_restart: false,
            monitors: None,
            tx,
            rx,
            ctx: ctx.clone(),
            shot: std::env::var_os("CLIPPED_GUI_SHOT").map(|p| (PathBuf::from(p), 0)),
        };
        app.load();
        if cfg!(test) {
            return app;
        }
        let (tx, ctx) = (app.tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let list = run_clip(&["monitors"], None).map(|o| parse_monitors(&o.stdout)).unwrap_or_default();
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
        self.editor = None;
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

    /// First problem with the edited values, checked before clipped sees them.
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
        // Never bound: a first Save turns shortcuts on (clipped bind needs no daemon).
        if changes.keys().any(|k| k.starts_with("bind.")) || !state::binds_state().exists() {
            notes.push(UPDATING.into());
            self.rebind();
        }
        if daemon && needs_daemon_restart(&changes) {
            if crate::tray::service_active() {
                self.offer_restart = true;
                notes.push("Restart the service to use the new video settings.".into());
            } else {
                notes.push("Restart `clipped daemon` to use the new video settings.".into());
            }
        }
        let error = !errors.is_empty();
        notes.extend(errors);
        self.set_status(notes.join(" "), error);
    }

    /// clipped never bound anything and Save won't: offer "Turn on shortcuts".
    fn shortcuts_inactive(&self) -> bool {
        !self.dirty() && !self.rebinding && self.read_error.is_none() && !state::binds_state().exists()
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

    /// `clipped bind` (it removes what it bound last time first), off the UI
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
            let r = Command::new("systemctl").args(["--user", "restart", "clipped.service"]).output();
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
            let mut d = rfd::FileDialog::new().set_title("Clipped: save folder");
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
                Msg::Monitors(l) => self.monitors = Some(l),
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

    /// While the editor is open but not recording, a key press egui sees
    /// fills the key and Ctrl/Alt/Shift (egui on Linux sees neither Meta nor
    /// Print, so the Meta box is left alone; Record sees both). Esc alone
    /// cancels.
    fn edit_keys(&mut self, ctx: &egui::Context) {
        self.record_keys(ctx);
        if self.editor.is_none() || ctx.wants_keyboard_input() {
            return;
        }
        let events = ctx.input(|i| i.events.clone());
        for e in events {
            if let egui::Event::Key { key, pressed: true, modifiers, .. } = e {
                if key == egui::Key::Escape && !modifiers.any() {
                    self.editor = None;
                    return;
                }
                let (Some(ed), Some(name)) = (self.editor.as_mut(), key_name(key)) else { continue };
                ed.sc.key = name;
                ed.sc.ctrl = modifiers.ctrl;
                ed.sc.alt = modifiers.alt;
                ed.sc.shift = modifiers.shift;
            }
        }
    }

    /// Feeds KeyTap's events to Record. While it listens egui gets no keys,
    /// so Space/Return can't press a button and Tab can't move focus.
    fn record_keys(&mut self, ctx: &egui::Context) {
        let evs = match &self.keys {
            Some(q) => q.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default(),
            None => return,
        };
        let Some(ed) = self.editor.as_mut() else { return };
        let Some(rec) = ed.rec.as_mut() else { return };
        ctx.input_mut(|i| i.events.retain(|e| !matches!(e, egui::Event::Key { .. } | egui::Event::Text(_))));
        for ev in evs {
            match rec.feed(ev) {
                Rec::Going => {}
                Rec::Done(sc) => {
                    ed.sc = sc;
                    ed.rec = None;
                    return;
                }
                Rec::Cancel => {
                    ed.rec = None;
                    return;
                }
            }
        }
    }

    fn self_shot(&mut self, ctx: &egui::Context) {
        // Wait for the monitor list (or ~300 frames) so the Source row is final.
        let ready = self.monitors.is_some();
        let Some((path, frames)) = &mut self.shot else { return };
        if *frames < SHOT_SENT {
            *frames += 1;
            if *frames >= 5 && (ready || *frames >= 300) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
                *frames = SHOT_SENT;
            }
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
            eprintln!("clipped-gui: {}: {e}", path.display());
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

/// The file's value when clipped would refuse it, so the widget's fallback
/// (unchecked, 60, …) isn't taken for what the file says.
fn raw_note(ui: &mut egui::Ui, key: &str, v: &str) {
    if !config::valid_value(key, v) {
        ui.colored_label(Color32::from_rgb(0xe0, 0x50, 0x50), format!("config has '{v}'"))
            .on_hover_text("clipped refuses this value; pick a valid one before saving");
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

/// One monitor from `clipped monitors` (NAME|WxH[|active]).
#[derive(Clone, Debug, PartialEq)]
struct Monitor {
    name: String,
    size: String,
}

fn parse_monitors(out: &[u8]) -> Vec<Monitor> {
    String::from_utf8_lossy(out)
        .lines()
        .filter_map(|l| {
            let mut f = l.trim().split('|');
            let name = f.next().filter(|n| !n.is_empty() && !n.contains(char::is_whitespace))?;
            Some(Monitor { name: name.to_string(), size: f.next().unwrap_or("").to_string() })
        })
        .collect()
}

/// Source choices as (config value, label): auto, each detected monitor,
/// portal, screen on X11, and the configured value if it is none of those,
/// so picking nothing never drops it. `monitors` is None while still looking.
fn source_options(current: &str, monitors: Option<&[Monitor]>, x11: bool) -> Vec<(String, String)> {
    let mut v = vec![("auto".to_string(), "Monitor you're on (auto)".to_string())];
    v.extend(monitors.unwrap_or(&[]).iter().map(|m| (m.name.clone(), format!("{}  {}", m.name, m.size).trim_end().to_string())));
    v.push(("portal".into(), "Pick each time (portal)".into()));
    if x11 {
        v.push(("screen".into(), "All monitors (X11 screen)".into()));
    }
    if !v.iter().any(|(val, _)| val == current) {
        let label = match (current.is_empty(), monitors) {
            (true, _) => "(empty)".to_string(),
            (false, None) => current.to_string(),
            (false, Some(_)) => format!("{current} (not detected)"),
        };
        v.push((current.to_string(), label));
    }
    v
}

fn source_combo(ui: &mut egui::Ui, v: &mut String, monitors: Option<&[Monitor]>, x11: bool) {
    let opts = source_options(v, monitors, x11);
    let shown = opts.iter().find(|(val, _)| val == v).map_or(v.clone(), |o| o.1.clone());
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("video_source").width(220.0).selected_text(shown).show_ui(ui, |ui| {
            for (val, label) in &opts {
                ui.selectable_value(v, val.clone(), label);
            }
            let hint = match monitors {
                None => Some("(looking for monitors…)"),
                Some([]) => Some("(no monitors found)"),
                Some(_) => None,
            };
            if let Some(h) = hint {
                ui.label(RichText::new(h).weak());
            }
        });
        if monitors.is_some_and(|m| m.is_empty()) {
            ui.label(RichText::new("no monitors found").weak())
                .on_hover_text("`clipped monitors` listed none: is gpu-screen-recorder installed?");
        }
    });
}

/// The daemon reads these only at start (the replay buffer keeps its monitor).
fn needs_daemon_restart(changes: &BTreeMap<String, String>) -> bool {
    changes.keys().any(|k| DAEMON_KEYS.contains(&k.as_str()))
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
        self.edit_keys(ctx);
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
                    let x11 = std::env::var("XDG_SESSION_TYPE").is_ok_and(|s| s == "x11");
                    let monitors = self.monitors.clone();
                    source_combo(ui, self.val("video_source"), monitors.as_deref(), x11);
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
                    for (a, name) in ACTIONS.iter().zip(ACTION_NAMES) {
                        let key = format!("bind.{a}");
                        ui.label(name);
                        ui.horizontal(|ui| {
                            let v = self.vals.entry(key.clone()).or_default();
                            ui.add(egui::TextEdit::singleline(v).desired_width(150.0).hint_text("none"));
                            let open = self.editor.as_ref().is_some_and(|e| e.field == *a);
                            let label = if v.is_empty() { "Set…" } else { "Change" };
                            let r = ui.add(egui::Button::new(label).selected(open));
                            if r.clicked() {
                                self.editor = (!open).then(|| Editor { field: a, sc: Shortcut::parse(v), rec: None });
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
                let can_record = self.keys.is_some();
                if let Some(ed) = self.editor.as_mut() {
                    let (mut set, mut close) = (None, false);
                    ui.add_space(4.0);
                    ui.group(|ui| {
                        let name = ACTIONS.iter().zip(ACTION_NAMES).find(|(a, _)| **a == ed.field).map_or("", |p| p.1);
                        ui.label(RichText::new(format!("Shortcut for {name}")).strong());
                        ui.horizontal(|ui| match &ed.rec {
                            Some(rec) => {
                                let so_far = rec.shortcut().text();
                                let shown = if rec.combo.is_empty() { "Press up to 3 keys…".to_string() } else { so_far };
                                ui.spinner();
                                ui.label(RichText::new(shown).monospace().strong());
                                let refused = rec.refused;
                                let got = (!rec.combo.is_empty()).then(|| rec.shortcut());
                                if ui.button("Stop").clicked() {
                                    ed.sc = got.unwrap_or(std::mem::take(&mut ed.sc));
                                    ed.rec = None;
                                }
                                if refused {
                                    ui.colored_label(red, "that key can't be a shortcut");
                                }
                            }
                            None => {
                                let r = ui
                                    .add_enabled(can_record, egui::Button::new("⏺ Record"))
                                    .on_hover_text("Press the shortcut on the keyboard")
                                    .on_disabled_hover_text("Key capture isn't available here; use the boxes below");
                                if r.clicked() {
                                    ed.rec = Some(Recorder::default());
                                    r.surrender_focus();
                                }
                            }
                        });
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut ed.sc.meta, "Meta");
                            ui.checkbox(&mut ed.sc.ctrl, "Ctrl");
                            ui.checkbox(&mut ed.sc.alt, "Alt");
                            ui.checkbox(&mut ed.sc.shift, "Shift");
                            ui.label("+");
                            let shown = if ed.sc.key.is_empty() { "key…" } else { ed.sc.key.as_str() };
                            egui::ComboBox::from_id_salt("shortcut-key").selected_text(shown.to_string()).show_ui(ui, |ui| {
                                for k in key_choices() {
                                    ui.selectable_value(&mut ed.sc.key, k.clone(), k);
                                }
                            });
                        });
                        ui.horizontal(|ui| {
                            let text = ed.sc.text();
                            if ed.sc.key.is_empty() {
                                ui.label(RichText::new("Pick a key").weak());
                            } else {
                                ui.label(RichText::new(&text).monospace().strong());
                            }
                            if ui.add_enabled(!ed.sc.key.is_empty(), egui::Button::new("Set")).clicked() {
                                set = Some(text);
                            }
                            if ui.button("Cancel").clicked() {
                                close = true;
                            }
                        });
                        ui.label(
                            RichText::new(
                                "Record takes up to 3 keys and stops when you let go (Stop ends it sooner; Esc \
                                 cancels). Combos KDE already uses, like the current Meta+Print, go to KDE \
                                 first: press Stop and pick the key above. 4-key shortcuts: use the boxes.",
                            )
                            .small()
                            .weak(),
                        );
                    });
                    if let Some(v) = set {
                        self.vals.insert(format!("bind.{}", ed.field), v);
                        close = true;
                    }
                    if close {
                        self.editor = None;
                    }
                }
                ui.label(
                    RichText::new("Meta, Ctrl, Alt, Shift + one key (A–Z, 0–9, F1–F35, Print, Space, Home…).")
                        .small()
                        .weak(),
                );
                if self.shortcuts_inactive() {
                    ui.horizontal(|ui| {
                        ui.colored_label(red, "Shortcuts are not active.");
                        if ui.button("Turn on shortcuts").clicked() {
                            self.set_status(UPDATING.into(), false);
                            self.rebind();
                        }
                    });
                }

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
        assert_eq!(desktop_exec_arg("/home/u/.local/bin/clipped-gui"), "/home/u/.local/bin/clipped-gui");
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
    fn shortcut_parse_and_text() {
        let sc = |m, c, a, s, k: &str| Shortcut { meta: m, ctrl: c, alt: a, shift: s, key: k.into() };
        assert_eq!(Shortcut::parse("Meta+Print"), sc(true, false, false, false, "Print"));
        assert_eq!(Shortcut::parse("super+print").text(), "Meta+Print");
        assert_eq!(Shortcut::parse("WIN + pageup").text(), "Meta+PgUp");
        assert_eq!(Shortcut::parse("shift+CONTROL+alt+escape").text(), "Ctrl+Alt+Shift+Esc");
        assert_eq!(Shortcut::parse("Meta+Alt+r"), sc(true, false, true, false, "R"));
        assert_eq!(Shortcut::parse("Ctrl+f30").text(), "Ctrl+F30");
        // Empty field: Meta on, no key yet.
        assert_eq!(Shortcut::parse(""), sc(true, false, false, false, ""));
        assert_eq!(Shortcut::parse("  ").text(), "Meta");
        // Unknown or second keys are dropped; modifiers are kept.
        assert_eq!(Shortcut::parse("Meta+Nope"), sc(true, false, false, false, ""));
        assert_eq!(Shortcut::parse("Alt+A+B").text(), "Alt+A");
        assert_eq!(sc(false, false, false, false, "Print").text(), "Print");
    }

    /// Every key the editor offers is one clipped accepts, Print first.
    #[test]
    fn key_choices_match_clipped() {
        let keys = key_choices();
        assert_eq!(keys[0], "Print");
        assert!(keys.contains(&"F24".into()) && keys.contains(&"Z".into()) && keys.contains(&"9".into()));
        for k in &keys {
            assert!(config::valid_shortcut(&format!("Meta+{k}")), "{k}");
            assert_eq!(canonical_key(&k.to_ascii_lowercase()).as_deref(), Some(k.as_str()));
        }
        // Keys egui reports land on an entry of the list.
        for k in [egui::Key::Escape, egui::Key::Enter, egui::Key::Insert, egui::Key::PageDown, egui::Key::F12, egui::Key::Num0] {
            assert!(keys.contains(&key_name(k).unwrap()), "{k:?}");
        }
    }

    /// Feeds events to a fresh Recorder; the text it ends with ("-" if it
    /// cancelled, "…+" prefix if still going) and the Recorder.
    fn record(evs: &[KeyEv]) -> (String, Recorder) {
        let mut r = Recorder::default();
        for ev in evs {
            match r.feed(ev.clone()) {
                Rec::Going => {}
                Rec::Done(sc) => return (sc.text(), r),
                Rec::Cancel => return ("-".into(), r),
            }
        }
        (format!("…{}", r.shortcut().text()), r)
    }

    /// The combos Record finishes on (each also checked by `clipped check`
    /// in record_output_passes_clipped_check).
    fn recorded_combos() -> Vec<(Vec<KeyEv>, &'static str)> {
        use KeyEv::{Down, Up};
        use Part::*;
        let k = |s: &str| Key(s.into());
        vec![
            // Meta+Print: two keys, ends when Print goes up.
            (vec![Down(Meta), Down(k("Print")), Up(k("Print"))], "Meta+Print"),
            // Ends at 3 held keys, before anything is released.
            (vec![Down(Ctrl), Down(Alt), Down(k("R"))], "Ctrl+Alt+R"),
            // A single key.
            (vec![Down(k("Print")), Up(k("Print"))], "Print"),
            // Order is always Meta+Ctrl+Alt+Shift+Key, whatever was pressed first.
            (vec![Down(Shift), Down(k("F12")), Down(Meta)], "Meta+Shift+F12"),
            (vec![Down(Alt), Down(Ctrl), Up(Alt)], "…Ctrl+Alt"),
            // Letting go of a modifier first also ends it.
            (vec![Down(Alt), Down(k("S")), Up(Alt)], "Alt+S"),
            // A second key replaces the first (clipped takes one).
            (vec![Down(Ctrl), Down(k("A")), Down(k("B")), Up(k("B"))], "Ctrl+B"),
            // Esc with a modifier is a shortcut; alone it cancels.
            (vec![Down(Ctrl), Down(k("Esc")), Up(k("Esc"))], "Ctrl+Esc"),
            (vec![Down(k("Esc"))], "-"),
        ]
    }

    #[test]
    fn record_capture_rules() {
        use KeyEv::{Down, Other, Reset, Up};
        use Part::*;
        for (evs, want) in recorded_combos() {
            assert_eq!(record(&evs).0, want, "{evs:?}");
        }
        // Max 3 keys: three modifiers end it with no key (pick one from the list).
        let (text, _) = record(&[Down(Meta), Down(Ctrl), Down(Alt), Down(Key("R".into()))]);
        assert_eq!(text, "Meta+Ctrl+Alt");
        // Modifiers alone, let go: still listening, shown for Stop.
        let (text, r) = record(&[Down(Meta), Up(Meta)]);
        assert_eq!(text, "…Meta");
        assert_eq!(r.shortcut(), Shortcut { meta: true, ..Default::default() });
        // …and the next press starts over (no stale Meta).
        assert_eq!(record(&[Down(Meta), Up(Meta), Down(Ctrl), Down(Key("R".into())), Up(Key("R".into()))]).0, "Ctrl+R");
        // Keys already down when Record started: their release is ignored.
        assert_eq!(record(&[Up(Key("Space".into())), Up(Ctrl), Down(Meta)]).0, "…Meta");
        // A held key repeating (winit repeats are dropped, but be safe).
        assert_eq!(record(&[Down(Meta), Down(Meta), Down(Key("V".into())), Up(Key("V".into()))]).0, "Meta+V");
        // Focus lost: held keys are forgotten, the next press starts over.
        assert_eq!(record(&[Down(Meta), Reset, Down(Key("P".into())), Up(Key("P".into()))]).0, "P");
        // A key clipped can't bind is flagged and not taken.
        let (text, r) = record(&[Down(Ctrl), Other]);
        assert_eq!(text, "…Ctrl");
        assert!(r.refused);
        assert!(!record(&[Down(Ctrl), Other, Up(Ctrl), Down(Alt)]).1.refused);
    }

    #[test]
    fn winit_keys_map_to_clipped_names() {
        use winit::keyboard::{Key, NamedKey as N};
        let name = |k: Key| match winit_part(&k) {
            Some(Part::Key(s)) => s,
            Some(p) => format!("{p:?}"),
            None => "-".into(),
        };
        for (k, want) in [
            (N::Super, "Meta"), (N::Meta, "Meta"), (N::Control, "Ctrl"), (N::Alt, "Alt"), (N::Shift, "Shift"),
            (N::PrintScreen, "Print"), (N::Pause, "Pause"), (N::Escape, "Esc"), (N::Enter, "Return"),
            (N::Insert, "Ins"), (N::Delete, "Del"), (N::PageUp, "PgUp"), (N::PageDown, "PgDown"),
            (N::ArrowLeft, "Left"), (N::ArrowDown, "Down"), (N::Space, "Space"), (N::Tab, "Tab"),
            (N::Backspace, "Backspace"), (N::F1, "F1"), (N::F24, "F24"), (N::F35, "F35"),
            (N::AltGraph, "-"), (N::CapsLock, "-"), (N::AudioVolumeUp, "-"),
        ] {
            assert_eq!(name(Key::Named(k)), want, "{k:?}");
        }
        for (c, want) in [("r", "R"), ("R", "R"), ("7", "7"), (",", "-"), ("é", "-"), ("ab", "-")] {
            assert_eq!(name(Key::Character(c.into())), want, "{c}");
        }
    }

    /// Everything Record can finish with is a value `clipped check` accepts.
    #[test]
    fn record_output_passes_clipped_check() {
        let Some(clipped) = clip_script() else { return };
        let _env = crate::testutil::env_lock();
        let d = crate::testutil::tempdir("rec");
        test_env(&d, &clipped);
        let cfg = d.join("config");
        for (evs, want) in recorded_combos() {
            if want.starts_with(['-', '…']) {
                continue;
            }
            let text = record(&evs).0;
            assert!(config::valid_shortcut(&text), "{text}");
            std::fs::write(&cfg, format!("bind.region = {text}\n")).unwrap();
            let o = run_clip(&["check"], Some(&cfg)).unwrap();
            assert!(o.status.success(), "{text}: {}", stderr_of(&o));
        }
    }

    #[test]
    fn autostart_entry_validates() {
        let d = crate::testutil::tempdir("auto");
        let f = d.join("clipped-gui.desktop");
        std::fs::write(&f, autostart_entry("/opt/my clipped/clipped-gui")).unwrap();
        let text = std::fs::read_to_string(&f).unwrap();
        assert!(text.contains("Exec=\"/opt/my clipped/clipped-gui\"\n"), "{text}");
        assert!(text.contains("X-GNOME-Autostart-enabled=true"));
        if let Ok(o) = Command::new("desktop-file-validate").arg(&f).output() {
            assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stdout));
        }
    }

    #[test]
    fn monitors_parse() {
        let m = parse_monitors(b"DP-1|2560x1440\nDP-2|1920x1080|active\n\nbad line\n|1x1\nHDMI-A-1\n");
        let names: Vec<_> = m.iter().map(|m| (m.name.as_str(), m.size.as_str())).collect();
        assert_eq!(names, [("DP-1", "2560x1440"), ("DP-2", "1920x1080"), ("HDMI-A-1", "")]);
    }

    #[test]
    fn source_choices() {
        let m = parse_monitors(b"DP-1|2560x1440\nDP-2|2560x1440|active\n");
        let vals = |cur: &str, x11| source_options(cur, Some(&m), x11).into_iter().map(|o| o.0).collect::<Vec<_>>();
        assert_eq!(vals("auto", false), ["auto", "DP-1", "DP-2", "portal"]);
        assert_eq!(vals("DP-2", false), ["auto", "DP-1", "DP-2", "portal"]);
        assert_eq!(vals("auto", true), ["auto", "DP-1", "DP-2", "portal", "screen"]);
        // Unknown values stay selectable, never dropped or duplicated.
        assert_eq!(vals("DP-9", false), ["auto", "DP-1", "DP-2", "portal", "DP-9"]);
        assert_eq!(vals("screen", false), ["auto", "DP-1", "DP-2", "portal", "screen"]);
        assert_eq!(vals("screen", true), ["auto", "DP-1", "DP-2", "portal", "screen"]);
        let o = source_options("DP-9", Some(&m), false);
        assert_eq!(o[0].1, "Monitor you're on (auto)");
        assert_eq!(o[1].1, "DP-1  2560x1440");
        assert_eq!(o[3].1, "Pick each time (portal)");
        assert_eq!(o[4].1, "DP-9 (not detected)");
        assert_eq!(source_options("DP-9", None, false)[2].1, "DP-9");
        assert_eq!(source_options("", Some(&[]), false).into_iter().map(|o| o.0).collect::<Vec<_>>(), ["auto", "portal", ""]);
    }

    #[test]
    fn video_source_change_offers_restart() {
        let ch = |k: &str| BTreeMap::from([(k.to_string(), "x".to_string())]);
        assert!(needs_daemon_restart(&ch("video_source")));
        assert!(needs_daemon_restart(&ch("replay_seconds")));
        assert!(!needs_daemon_restart(&ch("image_format")));
        assert!(!needs_daemon_restart(&ch("bind.region")));
    }

    fn clip_script() -> Option<PathBuf> {
        let clipped = Path::new(env!("CARGO_MANIFEST_DIR")).join("../bin/clipped");
        clipped.exists().then_some(clipped)
    }

    /// Temp XDG dirs, dry-run clipped and a desktop with no shortcut backend,
    /// so nothing here can touch the real session. CLIPPED_BIN logs each call
    /// to D/calls, then runs bin/clipped.
    fn test_env(d: &Path, clipped: &Path) {
        for (k, sub) in [("XDG_CONFIG_HOME", "cfg"), ("XDG_STATE_HOME", "state"), ("XDG_RUNTIME_DIR", "run"), ("XDG_CACHE_HOME", "cache")] {
            std::fs::create_dir_all(d.join(sub)).unwrap();
            std::env::set_var(k, d.join(sub));
        }
        let wrap = d.join("clipped-wrap");
        std::fs::write(&wrap, format!("#!/bin/sh\necho \"$*\" >>'{}/calls'\nexec '{}' \"$@\"\n", d.display(), clipped.display())).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&wrap, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::env::set_var("CLIPPED_BIN", &wrap);
        std::env::set_var("CLIPPED_DRY_RUN", "1");
        std::env::set_var("XDG_CURRENT_DESKTOP", "clipped-test");
        std::env::remove_var("CLIPPED_CONFIG");
        std::env::remove_var("HYPRLAND_INSTANCE_SIGNATURE");
        std::env::remove_var("SWAYSOCK");
    }

    fn app_at(path: &Path) -> App {
        let mut app = App::new(&egui::Context::default());
        app.path = path.to_path_buf();
        app.load();
        app
    }

    /// Also called before a test ends: a bind thread outliving env_lock
    /// would run under the next test's CLIPPED_BIN.
    fn wait_rebind(app: &mut App) {
        for _ in 0..200 {
            app.poll_messages();
            if !app.rebinding {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        panic!("clipped bind never reported: {}", app.status);
    }

    /// Nova #1: a config we can't read (EACCES, or unreadable by the time
    /// Save re-reads it) must never be replaced by the template.
    #[test]
    fn unreadable_config_is_never_replaced() {
        use std::os::unix::fs::PermissionsExt;
        let Some(clipped) = clip_script() else { return };
        let _env = crate::testutil::env_lock();
        let d = crate::testutil::tempdir("unread");
        test_env(&d, &clipped);
        let cfg = d.join("cfg/clipped/config");
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
        let Some(clipped) = clip_script() else { return };
        let _env = crate::testutil::env_lock();
        let d = crate::testutil::tempdir("latin1");
        test_env(&d, &clipped);
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
        wait_rebind(&mut app);
    }

    /// The Save button end to end, headless, in temp XDG dirs.
    #[test]
    fn save_flow() {
        let Some(clipped) = clip_script() else { return };
        let _env = crate::testutil::env_lock();
        let d = crate::testutil::tempdir("save");
        test_env(&d, &clipped);
        let mut app = app_at(&d.join("cfg/clipped/config"));
        assert!(app.status.contains("No config file yet"));

        // New file: clipped's template plus the changes, then `clipped bind`.
        // Text fields are trimmed.
        app.vals.insert("image_format".into(), "jpg".into());
        app.vals.insert("bind.window".into(), String::new());
        app.vals.insert("video_source".into(), "  DP-1 ".into());
        app.save();
        assert!(!app.error, "{}", app.status);
        assert!(app.status.contains(UPDATING), "{}", app.status);
        wait_rebind(&mut app);
        assert!(app.status.starts_with("Saved. Shortcuts updated"), "{}", app.status);
        let text = std::fs::read_to_string(&app.path).unwrap();
        assert!(text.starts_with("# clipped config"), "{text}");
        assert!(text.contains("\nimage_format = jpg         # png | jpg | webp\n"), "{text}");
        assert!(text.contains("\nbind.window =\n"), "{text}");
        assert!(text.contains("\nvideo_source = DP-1        # auto"), "{text}");
        assert!(!app.dirty());

        // clipped check refuses: file untouched, error shown.
        app.vals.insert("video_fps".into(), "999".into());
        app.save();
        assert!(app.error && app.status.contains("video_fps must be 1-240"), "{}", app.status);
        assert_eq!(std::fs::read_to_string(&app.path).unwrap(), text);

        // Refused before clipped sees it.
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
        wait_rebind(&mut app);
        let auto = d.join("cfg/autostart/clipped-gui.desktop");
        assert!(std::fs::read_to_string(&auto).unwrap().contains("X-GNOME-Autostart-enabled=true"));
        app.autostart = false;
        app.save();
        wait_rebind(&mut app);
        assert!(!auto.exists() && !app.error, "{}", app.status);
    }

    /// Kevin's case: installed, never bound. Any Save runs `clipped bind`;
    /// once clipped has bound, a Save without bind.* changes doesn't.
    #[test]
    fn first_save_binds() {
        let Some(clipped) = clip_script() else { return };
        let _env = crate::testutil::env_lock();
        let d = crate::testutil::tempdir("firstbind");
        test_env(&d, &clipped);
        let cfg = d.join("config");
        std::fs::write(&cfg, "image_format = png\n").unwrap();
        let mut app = app_at(&cfg);
        assert!(app.shortcuts_inactive());
        app.vals.insert("image_format".into(), "jpg".into());
        assert!(!app.shortcuts_inactive(), "Save binds; no second offer while dirty");
        app.save();
        assert!(app.status.starts_with("Saved. Updating shortcuts…") && app.rebinding, "{}", app.status);
        wait_rebind(&mut app);
        assert!(!app.error && app.status.starts_with("Saved. Shortcuts updated"), "{}", app.status);

        std::fs::create_dir_all(d.join("state/clipped")).unwrap();
        std::fs::write(d.join("state/clipped/binds"), "kde region Meta+Print\n").unwrap();
        assert!(!app.shortcuts_inactive());
        app.vals.insert("image_format".into(), "webp".into());
        app.save();
        assert!(!app.rebinding && app.status == "Saved.", "{}", app.status);
        let calls = std::fs::read_to_string(d.join("calls")).unwrap();
        let runs: Vec<&str> = calls.lines().filter(|l| *l != "check" && *l != "config").collect();
        assert_eq!(runs, ["bind"], "{calls}");
    }

    /// A value clipped refuses is shown as the file has it (not as the
    /// widget's fallback), and is what Save keeps.
    #[test]
    fn unparseable_value_is_kept_raw() {
        let Some(clipped) = clip_script() else { return };
        let _env = crate::testutil::env_lock();
        let d = crate::testutil::tempdir("raw");
        test_env(&d, &clipped);
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
        wait_rebind(&mut app);
        assert_eq!(std::fs::read_to_string(&cfg).unwrap(), "video_fps = 30\nnotify = false\nimage_format = jpg\n");
    }

    /// Nova #4 + #8: Save runs only `clipped bind` (no unbind), one at a time;
    /// a failed autostart write after the config was saved still reloads
    /// and rebinds, and the rebind result is added to the status.
    #[test]
    fn rebind_once_and_errors_collected() {
        let Some(clipped) = clip_script() else { return };
        let _env = crate::testutil::env_lock();
        let d = crate::testutil::tempdir("rebind");
        test_env(&d, &clipped);
        std::fs::create_dir_all(d.join("state/clipped")).unwrap();
        std::fs::write(d.join("state/clipped/binds"), "manual\n").unwrap(); // clipped has bound before
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
        let Some(clipped) = clip_script() else { return };
        let _env = crate::testutil::env_lock();
        let d = crate::testutil::tempdir("link");
        test_env(&d, &clipped);
        std::fs::create_dir_all(d.join("dots")).unwrap();
        std::fs::create_dir_all(d.join("cfg/clipped")).unwrap();
        let link = d.join("cfg/clipped/config");
        std::os::unix::fs::symlink("../../dots/clipped.conf", &link).unwrap();
        assert!(resolve_link(&link).unwrap().ends_with("cfg/clipped/../../dots/clipped.conf"));
        let mut app = app_at(&link);
        assert!(app.read_error.is_none() && app.status.contains("No config file yet"), "{}", app.status);
        app.vals.insert("video_fps".into(), "30".into());
        app.save();
        assert!(!app.error, "{}", app.status);
        wait_rebind(&mut app);
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink(), "link was replaced");
        let target = std::fs::read_to_string(d.join("dots/clipped.conf")).unwrap();
        assert!(target.starts_with("# clipped config") && target.contains("\nvideo_fps = 30 "), "{target}");
    }

    /// Nova #6: the temp file is new (a planted one isn't reused), keeps
    /// the config's mode, and a new config gets 0644 minus the umask.
    #[test]
    fn write_checked_modes() {
        use std::os::unix::fs::PermissionsExt;
        let Some(clipped) = clip_script() else { return };
        let _env = crate::testutil::env_lock();
        let d = crate::testutil::tempdir("mode");
        test_env(&d, &clipped);
        let cfg = d.join("config");
        std::fs::write(&cfg, "video_fps = 30\n").unwrap();
        std::fs::set_permissions(&cfg, std::fs::Permissions::from_mode(0o600)).unwrap();
        let planted = d.join(format!(".config.clipped-gui-{}-0.tmp", std::process::id()));
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

    /// write_checked against the real bin/clipped: a bad value leaves the file
    /// alone, a good one replaces it (skipped without bash).
    #[test]
    fn write_checked_uses_clip_check() {
        let Some(clipped) = clip_script() else { return };
        let _env = crate::testutil::env_lock();
        let d = crate::testutil::tempdir("wc");
        test_env(&d, &clipped);
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
        assert!(t.contains("bind.record = Meta+Alt+V") && t.contains("# clipped config"));
        let defaults = config::parse(&t);
        for (k, v) in config::DEFAULTS {
            assert_eq!(defaults[*k], *v, "default_config() and DEFAULTS disagree on {k}");
        }
    }
}
