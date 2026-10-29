//! The settings window: edits the frame config, checks it with `frame check`
//! before it replaces the real file, and re-applies shortcuts.

use crate::config::{self, ACTIONS};
use crate::state;
use crate::theme;
use eframe::egui::{self, Color32, RichText};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::mpsc::{channel, Receiver, Sender};

/// Free-text fields: edge spaces are trimmed on Save.
fn is_text_key(k: &str) -> bool {
    k == "save_dir" || k.starts_with("bind.")
}

enum Msg {
    Folder(PathBuf),
    /// Result of `frame bind`; it replaces "Updating shortcuts…" in the status.
    Rebind(String, bool),
}

/// self_shot's frame count once the screenshot is asked for.
const SHOT_SENT: u32 = u32::MAX;

const UPDATING: &str = "Updating shortcuts…";

const ACTION_NAMES: [&str; 4] = ["Region", "Window", "Monitor", "All monitors"];

/// The keys the editor offers (bin/frame key_info takes them all, plus
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
/// frame doesn't know it.
fn canonical_key(part: &str) -> Option<String> {
    let l = part.to_ascii_lowercase();
    let l = match l.as_str() {
        "escape" => "esc", "enter" => "return", "insert" => "ins", "delete" => "del",
        "pageup" => "pgup", "pagedown" => "pgdown", x => x,
    };
    if let Some(k) = key_choices().into_iter().find(|k| k.to_ascii_lowercase() == l) {
        return Some(k);
    }
    // F25–F35: frame takes them, the list stops at F24.
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

    /// "Meta+Ctrl+Alt+Shift+Key", the order frame's defaults use.
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
    /// A key frame can't bind (comma, AltGr, …) was pressed.
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
                    // frame takes one key: a second one replaces the first.
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
    /// The Cancel button (last frame): Enter on it cancels instead of Set.
    cancel: Option<egui::Id>,
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
    /// A `frame bind` is in flight; Save waits for it.
    rebinding: bool,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    ctx: egui::Context,
    /// FRAME_GUI_SHOT=FILE.png: save this window's own frame and quit (docs, review).
    shot: Option<(PathBuf, u32)>,
    /// FRAME_GUI_SHOT_VIEW=end (with FRAME_GUI_SHOT): scroll to the bottom first.
    shot_end: bool,
    /// Header pill: is the tray running.
    live: bool,
    live_at: Option<std::time::Instant>,
    icon: Option<egui::TextureHandle>,
}

pub fn run() -> i32 {
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Frame settings")
        .with_app_id("frame")
        .with_inner_size([480.0, 700.0])
        .with_min_inner_size([440.0, 520.0]);
    if let Some((w, h, rgba)) = crate::icons::rgba(crate::icons::APP_64) {
        viewport = viewport.with_icon(egui::IconData { rgba, width: w, height: h });
    }
    let opts = eframe::NativeOptions { viewport, ..Default::default() };
    // run_native, but on our own event loop so KeyTap sees winit's keys.
    let event_loop = match winit::event_loop::EventLoop::<eframe::UserEvent>::with_user_event().build() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("frame-gui: {e}");
            return 1;
        }
    };
    let keys = KeyQueue::default();
    let app_keys = keys.clone();
    let creator: eframe::AppCreator = Box::new(move |cc| {
        theme::apply(&cc.egui_ctx);
        let mut app = App::new(&cc.egui_ctx);
        app.keys = Some(app_keys);
        // FRAME_GUI_SHOT_VIEW=<action>: the editor opens listening, as a row click does.
        if let Some(ed) = app.editor.as_mut() {
            ed.rec = Some(Recorder::default());
        }
        Ok(Box::new(app))
    });
    let mut tap = KeyTap { inner: eframe::create_native("Frame", opts, creator, &event_loop), keys };
    match event_loop.run_app(&mut tap) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("frame-gui: {e}");
            1
        }
    }
}

fn run_clip(args: &[&str], config: Option<&Path>) -> Result<Output, String> {
    let bin = crate::clip_bin();
    let mut c = Command::new(&bin);
    c.args(args);
    if let Some(p) = config {
        c.env("FRAME_CONFIG", p);
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

/// Escape a path for a desktop entry Exec= line (same as desktop_arg in bin/frame).
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
        "[Desktop Entry]\nType=Application\nName=Frame\nComment=Frame tray: screenshot shortcuts\nExec={}\nIcon=frame\n\
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
/// one (same mode, synced), `frame check` on it, then rename. The real
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
        let tmp = dir.join(format!(".{name}.frame-gui-{}-{i}.tmp", std::process::id()));
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

/// The text `frame config` would write, made in a temp path (not the real one).
fn template(dir: &Path) -> String {
    let t = dir.join(format!(".frame-gui-template-{}", std::process::id()));
    let _ = std::fs::create_dir_all(dir);
    let _ = run_clip(&["config"], Some(&t));
    let text = std::fs::read_to_string(&t).unwrap_or_default();
    let _ = std::fs::remove_file(&t);
    text
}

impl App {
    fn new(ctx: &egui::Context) -> App {
        let (tx, rx) = channel();
        let path = state::config_path();
        // A first start after the rename: `frame check` copies the Clipped
        // config to the new place before we read it.
        if !cfg!(test) && matches!(read_config(&path), Ok(None)) {
            let _ = run_clip(&["check"], None);
        }
        let mut app = App {
            path,
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
            tx,
            rx,
            ctx: ctx.clone(),
            shot: std::env::var_os("FRAME_GUI_SHOT").map(|p| (PathBuf::from(p), 0)),
            shot_end: false,
            live: Default::default(),
            live_at: None,
            icon: crate::icons::rgba(HEADER_ICON).map(|(w, h, rgba)| {
                let img = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
                ctx.load_texture("frame-icon", img, egui::TextureOptions::LINEAR)
            }),
        };
        app.load();
        // FRAME_GUI_SHOT_VIEW: "end" scrolls down, an action opens its shortcut editor.
        if app.shot.is_some() {
            let view = std::env::var("FRAME_GUI_SHOT_VIEW").unwrap_or_default();
            app.shot_end = view == "end";
            if let Some(a) = ACTIONS.iter().find(|a| **a == view) {
                app.editor = Some(Editor { field: a, sc: Shortcut::parse(&app.vals[&format!("bind.{a}")]), rec: None, cancel: None });
            }
        }
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

    /// First problem with the edited values, checked before frame sees them.
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
                Ok(()) => notes.push(if self.autostart { "Frame starts when you log in." } else { "Frame no longer starts at login." }.into()),
                Err(e) => errors.push(format!("Start at login not changed: {e}.")),
            }
        }
        self.load();
        // Never bound: a first Save turns shortcuts on.
        if changes.keys().any(|k| k.starts_with("bind.")) || !state::binds_state().exists() {
            notes.push(UPDATING.into());
            self.rebind();
        }
        let error = !errors.is_empty();
        notes.extend(errors);
        self.set_status(notes.join(" "), error);
    }

    /// frame never bound anything and Save won't: offer "Turn on shortcuts".
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

    /// `frame bind` (it removes what it bound last time first), off the UI
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

    fn browse(&self) {
        let start = state::expand_dir(&self.vals["save_dir"]);
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let mut d = rfd::FileDialog::new().set_title("Frame: save folder");
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
                    self.vals.insert("save_dir".into(), tilde(&p));
                }
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

    /// Keys in the shortcut editor. Only Record captures keys; otherwise
    /// egui keeps them (Tab moves focus, Space presses the focused button,
    /// Esc cancels via the modal) and Enter means Set, unless Cancel has
    /// focus or a key list is open.
    fn edit_keys(&mut self, ctx: &egui::Context) {
        self.record_keys(ctx);
        let Some(ed) = self.editor.as_ref() else { return };
        if ed.rec.is_some() || egui::Popup::is_any_open(ctx) {
            return;
        }
        let focus = ctx.memory(|m| m.focused());
        if focus.is_some() && focus == ed.cancel {
            return;
        }
        if !ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter)) {
            return;
        }
        if !ed.sc.key.is_empty() {
            self.vals.insert(format!("bind.{}", ed.field), ed.sc.text());
            self.editor = None;
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
        let Some((path, frames)) = &mut self.shot else { return };
        if *frames < SHOT_SENT {
            *frames += 1;
            if *frames >= 5 {
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
            eprintln!("frame-gui: {}: {e}", path.display());
        }
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }

    fn val(&mut self, k: &str) -> &mut String {
        self.vals.entry(k.to_string()).or_default()
    }
}

/// A config value and the text the window shows for it.
type Choice = (&'static str, &'static str);

const IMAGE_FORMATS: &[Choice] = &[("png", "PNG"), ("jpg", "JPG"), ("webp", "WebP")];

/// The header icon (drawn at 34 pt, so 128 px stays sharp at 2× and up).
const HEADER_ICON: &[u8] = include_bytes!("../icons/png/frame-128.png");

const SHORTCUT_ROWS: [theme::Glyph; 4] = [theme::Glyph::Region, theme::Glyph::Window, theme::Glyph::Monitor, theme::Glyph::AllMonitors];

/// The file's value when frame would refuse it, so the widget's fallback
/// (off, no segment, …) isn't taken for what the file says.
fn raw_note(ui: &mut egui::Ui, key: &str, v: &str) {
    if !config::valid_value(key, v) {
        ui.label(RichText::new(format!("config has '{v}'")).size(12.5).color(theme::RED))
            .on_hover_text("frame refuses this value; pick a valid one before saving");
    }
}

/// An on/off setting as a list row with a toggle (writes true/false).
fn toggle_row(ui: &mut egui::Ui, divider: bool, key: &str, v: &mut String, title: &str, sub: Option<&str>) {
    theme::list_row(ui, divider, None, title, sub, false, |ui| {
        let mut b = config::yes(v);
        if theme::toggle(ui, &mut b).changed() {
            *v = b.to_string();
        }
        raw_note(ui, key, v);
    });
}

/// A small filled circle in line with text.
fn dot(ui: &mut egui::Ui, color: Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 8.0), egui::Sense::hover());
    ui.painter().circle_filled(rect.center(), 4.0, color);
}

/// The header pill: whether the tray (shortcuts at login) runs.
fn pill(tray: bool) -> (Color32, String) {
    if tray {
        (theme::GREEN, "Running".into())
    } else {
        (theme::TEXT_FAINT, "Not running".into())
    }
}

fn action_name(field: &str) -> &'static str {
    ACTIONS.iter().zip(ACTION_NAMES).find(|(a, _)| **a == field).map_or("", |p| p.1)
}

/// A path as the window shows it: `~` for $HOME.
fn tilde(p: &Path) -> String {
    match p.strip_prefix(state::home()) {
        Ok(r) if r.as_os_str().is_empty() => "~".into(),
        Ok(r) => format!("~/{}", r.display()),
        Err(_) => p.display().to_string(),
    }
}

impl App {
    /// Re-read the tray's pid file once a second for the header pill.
    fn poll_live(&mut self, ctx: &egui::Context) {
        let due = self.live_at.is_none_or(|t| t.elapsed() >= std::time::Duration::from_secs(1));
        if due {
            self.live = state::tray_running(Path::new("/proc"), &state::rundir());
            self.live_at = Some(std::time::Instant::now());
        }
        ctx.request_repaint_after(std::time::Duration::from_secs(1));
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if let Some(tex) = &self.icon {
                ui.add(egui::Image::new(tex).fit_to_exact_size(egui::vec2(34.0, 34.0)));
            }
            ui.add_space(4.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(RichText::new("Frame").size(18.0).strong().color(theme::TEXT));
                ui.label(RichText::new("Screenshots on a shortcut").size(12.0).color(theme::TEXT_FAINT));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (color, text) = pill(self.live);
                theme::status_pill(ui, color, &text);
            });
        });
    }

    fn bottom_bar(&mut self, ui: &mut egui::Ui) {
        theme::top_hairline(ui);
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add_enabled_ui(self.can_save(), |ui| theme::primary_button(ui, "Save")).inner.clicked() {
                    self.save();
                }
                let reload = self.read_error.is_some();
                let label = if reload { "Reload" } else { "Revert" };
                if ui.add_enabled_ui(reload || self.dirty(), |ui| theme::secondary_button(ui, label)).inner.clicked() {
                    self.load();
                }
                if self.rebinding {
                    ui.spinner();
                }
                // What's left, left to right: the state of the edits, or the
                // last result (an error stays until it's fixed or saved).
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    let config = format!("Config: {}", tilde(&self.path));
                    if self.dirty() && !self.error {
                        dot(ui, theme::AMBER);
                        ui.label(RichText::new("Unsaved changes").size(13.0).color(theme::TEXT_MUTED)).on_hover_text(config);
                    } else if !self.status.is_empty() {
                        let color = match () {
                            _ if self.error => theme::RED,
                            _ if self.status.starts_with("Saved.") => theme::GREEN,
                            _ => theme::TEXT_MUTED,
                        };
                        ui.add(egui::Label::new(RichText::new(&self.status).size(13.0).color(color)).wrap()).on_hover_text(config);
                    } else {
                        ui.label(RichText::new("All changes saved").size(13.0).color(theme::TEXT_FAINT)).on_hover_text(config);
                    }
                });
            });
        });
    }

    fn shortcuts_group(&mut self, ui: &mut egui::Ui) {
        let can_record = self.keys.is_some();
        theme::group_title(ui, "Shortcuts");
        let mut bad = None;
        theme::list_card(ui, |ui| {
            let notice = self.shortcuts_inactive();
            if notice {
                theme::notice(ui, "Shortcuts aren't turned on yet", |ui| {
                    if theme::primary_button(ui, "Turn on").on_hover_text("Bind the shortcuts below now").clicked() {
                        self.set_status(UPDATING.into(), false);
                        self.rebind();
                    }
                });
            }
            for (i, ((a, name), glyph)) in ACTIONS.iter().zip(ACTION_NAMES).zip(SHORTCUT_ROWS).enumerate() {
                let v = self.vals.get(&format!("bind.{a}")).cloned().unwrap_or_default();
                let (r, ()) = theme::list_row(ui, i > 0 || notice, Some(glyph), name, None, true, |ui| {
                    if v.is_empty() || config::valid_shortcut(&v) {
                        theme::keycaps_compact(ui, &v);
                    } else {
                        ui.add(egui::Label::new(RichText::new(format!("'{v}'")).color(theme::RED)).truncate());
                    }
                });
                if !v.is_empty() && !config::valid_shortcut(&v) {
                    bad.get_or_insert(format!("{name}: config has '{v}', not a shortcut."));
                }
                if r.on_hover_text("Press to record a shortcut").clicked() {
                    // Opens already listening (Record needs KeyTap's keys).
                    self.editor = Some(Editor { field: a, sc: Shortcut::parse(&v), rec: can_record.then(Recorder::default), cancel: None });
                }
            }
        });
        if let Some(b) = bad {
            ui.label(RichText::new(b).size(12.5).color(theme::RED));
        }
    }

    fn saving_group(&mut self, ui: &mut egui::Ui) {
        theme::group_title(ui, "Saving");
        let dir = state::expand_dir(&self.vals["save_dir"]);
        theme::list_card(ui, |ui| {
            let sub = tilde(&dir.join("frame/images"));
            theme::list_row(ui, false, None, "Save to", Some(&sub), false, |ui| {
                if theme::compact_button(ui, "Change…").on_hover_text("Pick the folder Frame saves into").clicked() {
                    self.browse();
                }
                let can_open = dir.is_absolute();
                if ui.add_enabled_ui(can_open, |ui| theme::compact_button(ui, "Open")).inner.on_hover_text(sub.clone()).clicked() {
                    open_folder(dir.join("frame/images"));
                }
            });
            theme::list_row(ui, true, None, "Format", None, false, |ui| {
                theme::segmented(ui, self.val("image_format"), IMAGE_FORMATS);
            });
        });
        if !dir.is_absolute() {
            ui.label(RichText::new("Save to must be an absolute path or start with ~/").size(12.5).color(theme::RED));
        }
        let f = &self.vals["image_format"];
        if !config::valid_value("image_format", f) {
            ui.label(RichText::new(format!("Format: config has '{f}'; pick PNG, JPG or WebP.")).size(12.5).color(theme::RED));
        }
    }

    fn behaviour_group(&mut self, ui: &mut egui::Ui) {
        theme::group_title(ui, "Behaviour");
        theme::list_card(ui, |ui| {
            toggle_row(ui, false, "clipboard", self.val("clipboard"), "Copy to clipboard", None);
            toggle_row(ui, true, "notify", self.val("notify"), "Show a notification", Some("With a preview of the picture"));
            theme::list_row(ui, true, None, "Start Frame at login", Some("Lives in the tray, keeps shortcuts on"), false, |ui| {
                theme::toggle(ui, &mut self.autostart).on_hover_text("Starts the tray, which turns the shortcuts on at login");
            });
        });
    }

    /// The shortcut editor as a modal dialog: Record (KeyTap, up to 3
    /// keys, Stop / Esc), or Meta/Ctrl/Alt/Shift + a key from the list; Set
    /// writes the field, Clear empties it, Cancel (or a click outside)
    /// leaves it.
    fn editor_dialog(&mut self, ctx: &egui::Context) {
        let can_record = self.keys.is_some();
        let Some(ed) = self.editor.as_mut() else { return };
        let (mut set, mut close) = (None, false);
        let frame = egui::Frame::new()
            .fill(theme::CARD)
            .stroke(egui::Stroke::new(1.0_f32, theme::BORDER_STRONG))
            .corner_radius(egui::CornerRadius::same(theme::CARD_RADIUS))
            .inner_margin(egui::Margin::same(20))
            .shadow(ctx.style().visuals.window_shadow);
        // No fade-in: Area's fade multiplies the whole layer's opacity, so for
        // ~80 ms the CARD fill (and the backdrop) let the rows show through.
        let id = egui::Id::new("shortcut-editor");
        let modal = egui::Modal::new(id)
            .area(egui::Modal::default_area(id).fade_in(false))
            .frame(frame)
            .backdrop_color(Color32::from_black_alpha(150))
            .show(ctx, |ui| {
                ui.set_width(380.0);
                ui.label(RichText::new(format!("Shortcut for {}", action_name(ed.field))).size(17.0).strong().color(theme::TEXT));
                theme::hint(ui, "Press it on the keyboard, or build it below.");
                ui.add_space(6.0);

                // The shortcut so far, as keycaps.
                egui::Frame::new()
                    .fill(theme::FIELD)
                    .stroke(egui::Stroke::new(1.0_f32, if ed.rec.is_some() { theme::ACCENT } else { theme::BORDER }))
                    .corner_radius(egui::CornerRadius::same(theme::RADIUS))
                    .inner_margin(egui::Margin::symmetric(12, 10))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| match &ed.rec {
                            Some(rec) => {
                                ui.spinner();
                                if rec.combo.is_empty() {
                                    ui.label(RichText::new("Press up to 3 keys…").color(theme::TEXT_MUTED));
                                } else {
                                    theme::keycaps_compact(ui, &rec.shortcut().text());
                                }
                            }
                            None if ed.sc.key.is_empty() => {
                                ui.label(RichText::new("Pick a key").italics().color(theme::TEXT_FAINT));
                            }
                            None => {
                                theme::keycaps_compact(ui, &ed.sc.text());
                            }
                        });
                    });
                if ed.rec.as_ref().is_some_and(|r| r.refused) {
                    ui.label(RichText::new("That key can't be a shortcut.").size(12.5).color(theme::RED));
                }
                ui.add_space(2.0);
                ui.horizontal(|ui| match &ed.rec {
                    Some(rec) => {
                        let got = (!rec.combo.is_empty()).then(|| rec.shortcut());
                        if theme::primary_button(ui, "Stop").clicked() {
                            ed.sc = got.unwrap_or(std::mem::take(&mut ed.sc));
                            ed.rec = None;
                        }
                        theme::hint(ui, "Esc cancels");
                    }
                    None => {
                        let r = ui
                            .add_enabled_ui(can_record, |ui| theme::secondary_button(ui, "⏺ Record"))
                            .inner
                            .on_hover_text("Press the shortcut on the keyboard")
                            .on_disabled_hover_text("Key capture isn't available here; use the keys below");
                        if r.clicked() {
                            ed.rec = Some(Recorder::default());
                            r.surrender_focus();
                        }
                    }
                });

                ui.add_space(8.0);
                theme::section_header(ui, "Or pick the keys");
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    for (on, name) in [(&mut ed.sc.meta, "Meta"), (&mut ed.sc.ctrl, "Ctrl"), (&mut ed.sc.alt, "Alt"), (&mut ed.sc.shift, "Shift")] {
                        if ui.add(egui::Button::new(name).selected(*on).min_size(egui::vec2(52.0, 30.0))).clicked() {
                            *on = !*on;
                        }
                    }
                    ui.label(RichText::new("+").color(theme::TEXT_MUTED));
                    let shown = if ed.sc.key.is_empty() { "key…" } else { ed.sc.key.as_str() };
                    egui::ComboBox::from_id_salt("shortcut-key").width(84.0).selected_text(shown.to_string()).show_ui(ui, |ui| {
                        for k in key_choices() {
                            ui.selectable_value(&mut ed.sc.key, k.clone(), k);
                        }
                    });
                });
                ui.add_space(4.0);
                theme::hint(
                    ui,
                    "Record takes up to 3 keys and stops when you let go. Keys KDE already uses go to KDE \
                     first: press Stop and pick them above.",
                );
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if theme::danger_link(ui, "Clear shortcut").on_hover_text("No shortcut for this action").clicked() {
                        set = Some(String::new());
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let text = ed.sc.text();
                        if ui.add_enabled_ui(!ed.sc.key.is_empty(), |ui| theme::primary_button(ui, "Set")).inner.clicked() {
                            set = Some(text);
                        }
                        let r = theme::secondary_button(ui, "Cancel");
                        ed.cancel = Some(r.id);
                        if r.clicked() {
                            close = true;
                        }
                    });
                });
            });
        // A click outside (or Esc, also handled in edit_keys) cancels, but
        // never while Record listens.
        if ed.rec.is_none() && modal.should_close() {
            close = true;
        }
        if let Some(v) = set {
            self.vals.insert(format!("bind.{}", ed.field), v);
            close = true;
        }
        if close {
            self.editor = None;
        }
    }

    /// The whole window (update minus the per-frame polling).
    fn ui(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("header")
            .frame(egui::Frame::new().fill(theme::BG).inner_margin(egui::Margin { left: 20, right: 20, top: 16, bottom: 4 }))
            .show_separator_line(false)
            .show(ctx, |ui| self.header(ui));

        egui::TopBottomPanel::bottom("buttons")
            .frame(theme::bottom_bar_frame())
            .show_separator_line(false)
            .show(ctx, |ui| self.bottom_bar(ui));

        egui::CentralPanel::default().frame(theme::page_frame()).show(ctx, |ui| {
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                self.shortcuts_group(ui);
                ui.add_space(6.0);
                self.saving_group(ui);
                ui.add_space(6.0);
                self.behaviour_group(ui);
                if self.shot_end {
                    ui.scroll_to_cursor(Some(egui::Align::BOTTOM));
                }
            });
        });

        self.editor_dialog(ctx);
    }
}

/// xdg-open FOLDER (made first, so a new save folder opens too); waited
/// for off the UI thread so it leaves no zombie.
fn open_folder(dir: PathBuf) {
    std::thread::spawn(move || {
        let _ = std::fs::create_dir_all(&dir);
        if let Err(e) = Command::new("xdg-open").arg(&dir).status() {
            eprintln!("frame-gui: xdg-open: {e}");
        }
    });
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll_messages();
        self.poll_live(ctx);
        self.edit_keys(ctx);
        self.self_shot(ctx);
        self.ui(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exec_quoting_matches_bash() {
        assert_eq!(desktop_exec_arg("/home/u/.local/bin/frame-gui"), "/home/u/.local/bin/frame-gui");
        // Same expectation as the "quoting helpers" check in tests/smoke.sh.
        assert_eq!(desktop_exec_arg("/a b/$x%\"q\\"), "\"/a b/\\\\$x%%\\\\\"q\\\\\\\\\"");
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

    /// Every key the editor offers is one frame accepts, Print first.
    #[test]
    fn key_choices_match_frame() {
        let keys = key_choices();
        assert_eq!(keys[0], "Print");
        assert!(keys.contains(&"F24".into()) && keys.contains(&"Z".into()) && keys.contains(&"9".into()));
        for k in &keys {
            assert!(config::valid_shortcut(&format!("Meta+{k}")), "{k}");
            assert_eq!(canonical_key(&k.to_ascii_lowercase()).as_deref(), Some(k.as_str()));
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

    /// The combos Record finishes on (each also checked by `frame check`
    /// in record_output_passes_frame_check).
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
            // A second key replaces the first (frame takes one).
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
        // A key frame can't bind is flagged and not taken.
        let (text, r) = record(&[Down(Ctrl), Other]);
        assert_eq!(text, "…Ctrl");
        assert!(r.refused);
        assert!(!record(&[Down(Ctrl), Other, Up(Ctrl), Down(Alt)]).1.refused);
    }

    #[test]
    fn winit_keys_map_to_frame_names() {
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

    /// Everything Record can finish with is a value `frame check` accepts.
    #[test]
    fn record_output_passes_frame_check() {
        let Some(frame) = clip_script() else { return };
        let _env = crate::testutil::env_lock();
        let d = crate::testutil::tempdir("rec");
        test_env(&d, &frame);
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

    /// Outside Record, keys never change the shortcut; Enter means Set
    /// (not while Cancel has focus).
    #[test]
    fn editor_keys_dont_overwrite() {
        let _env = crate::testutil::env_lock();
        let d = crate::testutil::tempdir("edkeys");
        test_env(&d, Path::new("/nonexistent"));
        let ctx = egui::Context::default();
        let mut app = App::new(&ctx);
        app.path = d.join("config");
        app.load();
        let press = |key| egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() };
        let frame = |app: &mut App, events: Vec<egui::Event>, focus: bool| {
            let input = egui::RawInput { events, ..Default::default() };
            let _ = ctx.run(input, |ctx| {
                if focus {
                    if let Some(id) = app.editor.as_ref().and_then(|e| e.cancel) {
                        ctx.memory_mut(|m| m.request_focus(id));
                    }
                }
                app.edit_keys(ctx);
                app.editor_dialog(ctx);
            });
        };
        let open = |app: &mut App| app.editor = Some(Editor { field: "region", sc: Shortcut::parse("Meta+Alt+R"), rec: None, cancel: None });
        open(&mut app);
        frame(&mut app, vec![press(egui::Key::Tab), press(egui::Key::ArrowDown), press(egui::Key::F5)], false);
        assert_eq!(app.editor.as_ref().map(|e| e.sc.text()).as_deref(), Some("Meta+Alt+R"));
        // Enter while Cancel has focus: no Set.
        frame(&mut app, vec![], true);
        frame(&mut app, vec![press(egui::Key::Enter)], false);
        assert_eq!(app.vals["bind.region"], config::DEFAULTS.iter().find(|d| d.0 == "bind.region").unwrap().1);
        // Enter with nothing focused: Set, key unchanged.
        open(&mut app);
        frame(&mut app, vec![], false);
        frame(&mut app, vec![press(egui::Key::Enter)], false);
        assert!(app.editor.is_none());
        assert_eq!(app.vals["bind.region"], "Meta+Alt+R");
        // Esc cancels.
        open(&mut app);
        app.vals.insert("bind.region".into(), String::new());
        frame(&mut app, vec![press(egui::Key::Escape)], false);
        frame(&mut app, vec![], false);
        assert!(app.editor.is_none());
        assert_eq!(app.vals["bind.region"], "");
    }

    #[test]
    fn autostart_entry_validates() {
        let d = crate::testutil::tempdir("auto");
        let f = d.join("frame-gui.desktop");
        std::fs::write(&f, autostart_entry("/opt/my frame/frame-gui")).unwrap();
        let text = std::fs::read_to_string(&f).unwrap();
        assert!(text.contains("Exec=\"/opt/my frame/frame-gui\"\n"), "{text}");
        assert!(text.contains("X-GNOME-Autostart-enabled=true"));
        // install.sh writes the same text (tests/smoke.sh checks its copy).
        assert_eq!(
            autostart_entry("/p/frame-gui"),
            "[Desktop Entry]\nType=Application\nName=Frame\nComment=Frame tray: screenshot shortcuts\n\
             Exec=/p/frame-gui\nIcon=frame\nTerminal=false\nX-GNOME-Autostart-enabled=true\n"
        );
        if let Ok(o) = Command::new("desktop-file-validate").arg(&f).output() {
            assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stdout));
        }
    }

    #[test]
    fn friendly_labels_keep_config_values() {
        let labels: Vec<_> = IMAGE_FORMATS.iter().map(|c| c.1).collect();
        assert_eq!(labels, ["PNG", "JPG", "WebP"]);
        for (v, _) in IMAGE_FORMATS {
            assert!(config::valid_value("image_format", v), "image_format = {v}");
        }
    }

    #[test]
    fn header_pill() {
        assert_eq!(pill(true), (theme::GREEN, "Running".into()));
        assert_eq!(pill(false).1, "Not running");
    }

    fn clip_script() -> Option<PathBuf> {
        let frame = Path::new(env!("CARGO_MANIFEST_DIR")).join("../bin/frame");
        frame.exists().then_some(frame)
    }

    /// Temp XDG dirs, dry-run frame and a desktop with no shortcut backend,
    /// so nothing here can touch the real session. FRAME_BIN logs each call
    /// to D/calls, then runs bin/frame.
    fn test_env(d: &Path, frame: &Path) {
        for (k, sub) in [("XDG_CONFIG_HOME", "cfg"), ("XDG_STATE_HOME", "state"), ("XDG_RUNTIME_DIR", "run"), ("XDG_CACHE_HOME", "cache")] {
            std::fs::create_dir_all(d.join(sub)).unwrap();
            std::env::set_var(k, d.join(sub));
        }
        let wrap = d.join("frame-wrap");
        std::fs::write(&wrap, format!("#!/bin/sh\necho \"$*\" >>'{}/calls'\nexec '{}' \"$@\"\n", d.display(), frame.display())).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&wrap, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::env::set_var("FRAME_BIN", &wrap);
        std::env::set_var("FRAME_DRY_RUN", "1");
        std::env::set_var("XDG_CURRENT_DESKTOP", "frame-test");
        std::env::remove_var("FRAME_CONFIG");
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
    /// would run under the next test's FRAME_BIN.
    fn wait_rebind(app: &mut App) {
        for _ in 0..200 {
            app.poll_messages();
            if !app.rebinding {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        panic!("frame bind never reported: {}", app.status);
    }

    /// Nova #1: a config we can't read (EACCES, or unreadable by the time
    /// Save re-reads it) must never be replaced by the template.
    #[test]
    fn unreadable_config_is_never_replaced() {
        use std::os::unix::fs::PermissionsExt;
        let Some(frame) = clip_script() else { return };
        let _env = crate::testutil::env_lock();
        let d = crate::testutil::tempdir("unread");
        test_env(&d, &frame);
        let cfg = d.join("cfg/frame/config");
        std::fs::create_dir_all(cfg.parent().unwrap()).unwrap();
        let mine = "# mine\nnotify = false\n";
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
        assert!(app.read_error.is_none() && app.vals["notify"] == "false");
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
        let Some(frame) = clip_script() else { return };
        let _env = crate::testutil::env_lock();
        let d = crate::testutil::tempdir("latin1");
        test_env(&d, &frame);
        let cfg = d.join("config");
        std::fs::write(&cfg, b"# caf\xe9\r\nnotify = false   # \xe9\nimage_format = png\n").unwrap();
        let mut app = app_at(&cfg);
        let (load_status, notify) = (app.status.clone(), app.vals["notify"].clone());
        app.vals.insert("image_format".into(), "jpg".into());
        app.save();
        let after = std::fs::read(&cfg).unwrap();
        assert_eq!(String::from_utf8_lossy(&after), "# caf\u{fffd}\r\nnotify = false   # \u{fffd}\nimage_format = jpg\n");
        assert_eq!(after, b"# caf\xe9\r\nnotify = false   # \xe9\nimage_format = jpg\n");
        assert!(load_status.is_empty() && notify == "false", "{load_status}");
        assert!(!app.error, "{}", app.status);
        wait_rebind(&mut app);
    }

    /// The Save button end to end, headless, in temp XDG dirs.
    #[test]
    fn save_flow() {
        let Some(frame) = clip_script() else { return };
        let _env = crate::testutil::env_lock();
        let d = crate::testutil::tempdir("save");
        test_env(&d, &frame);
        let mut app = app_at(&d.join("cfg/frame/config"));
        assert!(app.status.contains("No config file yet"));

        // New file: frame's template plus the changes, then `frame bind`.
        // Text fields are trimmed.
        app.vals.insert("image_format".into(), "jpg".into());
        app.vals.insert("bind.window".into(), String::new());
        app.vals.insert("bind.region".into(), "  Meta+F9 ".into());
        app.save();
        assert!(!app.error, "{}", app.status);
        assert!(app.status.contains(UPDATING), "{}", app.status);
        wait_rebind(&mut app);
        assert!(app.status.starts_with("Saved. Shortcuts updated"), "{}", app.status);
        let text = std::fs::read_to_string(&app.path).unwrap();
        assert!(text.starts_with("# frame config"), "{text}");
        assert!(text.contains("\nimage_format = jpg         # png | jpg | webp\n"), "{text}");
        assert!(text.contains("\nbind.window =\n"), "{text}");
        assert!(text.contains("\nbind.region = Meta+F9\n"), "{text}");
        assert!(!text.contains("video") && !text.contains("replay"), "{text}");
        assert!(!app.dirty());

        // frame check refuses: file untouched, error shown.
        app.vals.insert("save_dir".into(), "relative/dir".into());
        app.save();
        assert!(app.error && app.status.contains("save_dir must be an absolute path"), "{}", app.status);
        assert_eq!(std::fs::read_to_string(&app.path).unwrap(), text);

        // Refused before frame sees it.
        app.load();
        app.vals.insert("save_dir".into(), "/a #b".into());
        app.save();
        assert!(app.error && app.status.contains("save_dir:"), "{}", app.status);
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
        let auto = d.join("cfg/autostart/frame-gui.desktop");
        assert!(std::fs::read_to_string(&auto).unwrap().contains("X-GNOME-Autostart-enabled=true"));
        app.autostart = false;
        app.save();
        wait_rebind(&mut app);
        assert!(!auto.exists() && !app.error, "{}", app.status);
    }

    /// Kevin's case: installed, never bound. Any Save runs `frame bind`;
    /// once frame has bound, a Save without bind.* changes doesn't.
    #[test]
    fn first_save_binds() {
        let Some(frame) = clip_script() else { return };
        let _env = crate::testutil::env_lock();
        let d = crate::testutil::tempdir("firstbind");
        test_env(&d, &frame);
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

        std::fs::create_dir_all(d.join("state/frame")).unwrap();
        std::fs::write(d.join("state/frame/binds"), "kde region Meta+Print\n").unwrap();
        assert!(!app.shortcuts_inactive());
        app.vals.insert("image_format".into(), "webp".into());
        app.save();
        assert!(!app.rebinding && app.status == "Saved.", "{}", app.status);
        let calls = std::fs::read_to_string(d.join("calls")).unwrap();
        let runs: Vec<&str> = calls.lines().filter(|l| *l != "check" && *l != "config").collect();
        assert_eq!(runs, ["bind"], "{calls}");
    }

    /// A config from Frame < 0.4 (video keys, replay/record shortcuts)
    /// loads without an error, and Save keeps those lines as they are.
    #[test]
    fn old_video_config_still_saves() {
        let Some(frame) = clip_script() else { return };
        let _env = crate::testutil::env_lock();
        let d = crate::testutil::tempdir("oldcfg");
        test_env(&d, &frame);
        let cfg = d.join("config");
        let old = "image_format = png\nvideo_format = mp4\nreplay = false\nreplay_seconds = auto\nvideo_source = DP-2\n\
                   video_fps = 60\nvideo_quality = very_high\nvideo_codec = auto\naudio = default_output\n\
                   bind.replay = Meta+Alt+C\nbind.record = Meta+Alt+V\n";
        std::fs::write(&cfg, old).unwrap();
        let mut app = app_at(&cfg);
        assert!(!app.error && app.status.is_empty(), "{}", app.status);
        assert!(!app.vals.contains_key("replay") && !app.vals.contains_key("bind.record"));
        app.vals.insert("image_format".into(), "jpg".into());
        app.save();
        assert!(!app.error, "{}", app.status);
        wait_rebind(&mut app);
        assert_eq!(std::fs::read_to_string(&cfg).unwrap(), old.replace("image_format = png", "image_format = jpg"));
    }

    /// A value frame refuses is shown as the file has it (not as the
    /// widget's fallback), and is what Save keeps.
    #[test]
    fn unparseable_value_is_kept_raw() {
        let Some(frame) = clip_script() else { return };
        let _env = crate::testutil::env_lock();
        let d = crate::testutil::tempdir("raw");
        test_env(&d, &frame);
        let cfg = d.join("config");
        // Every image format the window offers is a value frame check takes.
        for (v, _) in IMAGE_FORMATS {
            std::fs::write(&cfg, format!("image_format = {v}\n")).unwrap();
            let o = run_clip(&["check"], Some(&cfg)).unwrap();
            assert!(o.status.success(), "{v}: {}", stderr_of(&o));
        }
        std::fs::write(&cfg, "image_format = gif\nnotify = maybe\n").unwrap();
        let mut app = app_at(&cfg);
        assert_eq!((app.vals["image_format"].as_str(), app.vals["notify"].as_str()), ("gif", "maybe"));
        assert!(!config::valid_value("image_format", &app.vals["image_format"]));
        app.vals.insert("clipboard".into(), "false".into());
        app.save();
        assert!(app.error && app.status.contains("image_format must be png, jpg or webp"), "{}", app.status);
        app.vals.insert("image_format".into(), "jpg".into());
        app.vals.insert("notify".into(), "false".into());
        app.save();
        assert!(!app.error, "{}", app.status);
        wait_rebind(&mut app);
        assert_eq!(std::fs::read_to_string(&cfg).unwrap(), "image_format = jpg\nnotify = false\nclipboard = false\n");
    }

    /// Nova #4 + #8: Save runs only `frame bind` (no unbind), one at a time;
    /// a failed autostart write after the config was saved still reloads
    /// and rebinds, and the rebind result is added to the status.
    #[test]
    fn rebind_once_and_errors_collected() {
        let Some(frame) = clip_script() else { return };
        let _env = crate::testutil::env_lock();
        let d = crate::testutil::tempdir("rebind");
        test_env(&d, &frame);
        std::fs::create_dir_all(d.join("state/frame")).unwrap();
        std::fs::write(d.join("state/frame/binds"), "manual\n").unwrap(); // frame has bound before
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
        let Some(frame) = clip_script() else { return };
        let _env = crate::testutil::env_lock();
        let d = crate::testutil::tempdir("link");
        test_env(&d, &frame);
        std::fs::create_dir_all(d.join("dots")).unwrap();
        std::fs::create_dir_all(d.join("cfg/frame")).unwrap();
        let link = d.join("cfg/frame/config");
        std::os::unix::fs::symlink("../../dots/frame.conf", &link).unwrap();
        assert!(resolve_link(&link).unwrap().ends_with("cfg/frame/../../dots/frame.conf"));
        let mut app = app_at(&link);
        assert!(app.read_error.is_none() && app.status.contains("No config file yet"), "{}", app.status);
        app.vals.insert("image_format".into(), "webp".into());
        app.save();
        assert!(!app.error, "{}", app.status);
        wait_rebind(&mut app);
        assert!(std::fs::symlink_metadata(&link).unwrap().file_type().is_symlink(), "link was replaced");
        let target = std::fs::read_to_string(d.join("dots/frame.conf")).unwrap();
        assert!(target.starts_with("# frame config") && target.contains("\nimage_format = webp "), "{target}");
    }

    /// Nova #6: the temp file is new (a planted one isn't reused), keeps
    /// the config's mode, and a new config gets 0644 minus the umask.
    #[test]
    fn write_checked_modes() {
        use std::os::unix::fs::PermissionsExt;
        let Some(frame) = clip_script() else { return };
        let _env = crate::testutil::env_lock();
        let d = crate::testutil::tempdir("mode");
        test_env(&d, &frame);
        let cfg = d.join("config");
        std::fs::write(&cfg, "notify = true\n").unwrap();
        std::fs::set_permissions(&cfg, std::fs::Permissions::from_mode(0o600)).unwrap();
        let planted = d.join(format!(".config.frame-gui-{}-0.tmp", std::process::id()));
        std::fs::write(&planted, "planted").unwrap();
        write_checked(&cfg, b"notify = false\n").unwrap();
        assert_eq!(std::fs::metadata(&cfg).unwrap().permissions().mode() & 0o7777, 0o600);
        assert_eq!(std::fs::read_to_string(&planted).unwrap(), "planted");
        assert_eq!(std::fs::read_to_string(&cfg).unwrap(), "notify = false\n");

        let umask = std::fs::read_to_string("/proc/self/status")
            .unwrap()
            .lines()
            .find_map(|l| l.strip_prefix("Umask:").map(|v| u32::from_str_radix(v.trim(), 8).unwrap()))
            .unwrap();
        let new = d.join("new/config");
        write_checked(&new, b"notify = false\n").unwrap();
        assert_eq!(std::fs::metadata(&new).unwrap().permissions().mode() & 0o7777, 0o644 & !umask);
    }

    /// write_checked against the real bin/frame: a bad value leaves the file
    /// alone, a good one replaces it (skipped without bash).
    #[test]
    fn write_checked_uses_clip_check() {
        let Some(frame) = clip_script() else { return };
        let _env = crate::testutil::env_lock();
        let d = crate::testutil::tempdir("wc");
        test_env(&d, &frame);
        let cfg = d.join("config");
        std::fs::write(&cfg, "image_format = png # mine\n").unwrap();
        let err = write_checked(&cfg, b"image_format = gif # mine\n").unwrap_err();
        assert!(err.contains("image_format must be png, jpg or webp"), "{err}");
        assert_eq!(std::fs::read_to_string(&cfg).unwrap(), "image_format = png # mine\n");
        write_checked(&cfg, b"image_format = webp # mine\n").unwrap();
        assert_eq!(std::fs::read_to_string(&cfg).unwrap(), "image_format = webp # mine\n");
        let mut left: Vec<_> = std::fs::read_dir(&*d).unwrap().filter_map(|e| e.ok()).map(|e| e.file_name()).collect();
        left.retain(|n| n.to_string_lossy().starts_with(".config"));
        assert!(left.is_empty(), "temp files left behind: {left:?}");

        let t = template(&d);
        assert!(t.contains("bind.all = Meta+Alt+A") && t.contains("# frame config") && !t.contains("video"));
        let defaults = config::parse(&t);
        for (k, v) in config::DEFAULTS {
            assert_eq!(defaults[*k], *v, "default_config() and DEFAULTS disagree on {k}");
        }
    }
}
