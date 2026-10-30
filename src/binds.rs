//! `frame bind` / `frame unbind`: the global shortcuts, per desktop.
//!
//! What we bound is recorded per backend in $XDG_STATE_HOME/frame/binds
//! as "backend action shortcut" lines, so unbind removes what we set, not
//! what the config says now.

use crate::config::{ACTIONS, OLD_ACTIONS};
use crate::ctx::{desktop_arg, quote, quote1, Ctx};
use crate::detect::{self, Desktop};
use crate::keys::{self, Style};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

type Cfg = BTreeMap<String, String>;

// ------------------------------------------------------------------ state

fn backend_name(d: Desktop) -> &'static str {
    match d {
        Desktop::Hyprland => "hyprland",
        Desktop::Sway => "sway",
        Desktop::Kde => "kde",
        Desktop::Gnome => "gnome",
        Desktop::Other => "manual",
    }
}

/// (action, shortcut) pairs saved for BACKEND.
pub fn state_get(ctx: &Ctx, backend: &str) -> Vec<(String, String)> {
    let text = std::fs::read_to_string(ctx.binds_state()).unwrap_or_default();
    text.lines()
        .filter_map(|l| {
            let mut p = l.splitn(3, ' ');
            (p.next()? == backend).then(|| (p.next().unwrap_or("").to_string(), p.next().unwrap_or("").to_string()))
        })
        .filter(|(a, _)| !a.is_empty())
        .collect()
}

fn state_save(ctx: &Ctx, backend: &str, pairs: &[(String, String)]) {
    let f = ctx.binds_state();
    if ctx.dry() {
        let list: Vec<String> = pairs.iter().map(|(a, k)| format!("{a} {k}")).collect();
        let list = if list.is_empty() { "(none)".to_string() } else { list.join(" ") };
        ctx.say(&format!("+ save {}: {backend} {list}", f.display()));
        return;
    }
    let old = std::fs::read_to_string(&f).unwrap_or_default();
    let mut out: String =
        old.lines().filter(|l| l.split(' ').next() != Some(backend)).map(|l| format!("{l}\n")).collect();
    for (a, k) in pairs {
        out.push_str(&format!("{backend} {a} {k}\n"));
    }
    let tmp = f.with_extension("tmp");
    let r = f
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&tmp, out))
        .and_then(|()| std::fs::rename(&tmp, &f));
    if let Err(e) = r {
        ctx.warn(&format!("{}: {e}", f.display()));
    }
}

// -------------------------------------------------------------------- KDE

const KGA: &str = "org.kde.kglobalaccel";

fn kde_id(a: &str) -> Vec<String> {
    vec![format!("frame-{a}.desktop"), "_launch".into(), format!("frame {a}"), format!("frame {a}")]
}

fn kde_desktop(ctx: &Ctx, a: &str) -> PathBuf {
    ctx.data_home().join(format!("applications/frame-{a}.desktop"))
}

/// The kglobalaccel D-Bus calls we make. Dry-run prints them instead.
struct Kga {
    conn: Option<zbus::blocking::Connection>,
}

/// What every call error becomes: KDE's shortcut service is out of reach,
/// which is not the same as a key someone else holds.
fn unreachable(e: impl std::fmt::Display) -> String {
    format!("can't reach kglobalaccel (KDE's shortcut service) over D-Bus: {e}")
}

impl Kga {
    /// No connection in dry-run or when NEED is false (not KDE). On KDE a
    /// bus we can't reach, or no kglobalaccel on it, is an error.
    fn new(ctx: &Ctx, need: bool) -> Result<Kga, String> {
        if ctx.dry() || !need {
            return Ok(Kga { conn: None });
        }
        let conn = ctx.session_bus().map_err(unreachable)?;
        // Ping starts kglobalaccel if it is D-Bus activated, else fails.
        zbus::blocking::Proxy::new(&conn, KGA, "/kglobalaccel", "org.freedesktop.DBus.Peer")
            .and_then(|p| p.call::<_, _, ()>("Ping", &()))
            .map_err(unreachable)?;
        Ok(Kga { conn: Some(conn) })
    }

    fn proxy(&self, path: &str, iface: &str) -> Result<zbus::blocking::Proxy<'_>, String> {
        let conn = self.conn.as_ref().ok_or_else(|| unreachable("not connected"))?;
        zbus::blocking::Proxy::new(conn, KGA, path.to_string(), iface.to_string()).map_err(unreachable)
    }

    fn kga(&self) -> Result<zbus::blocking::Proxy<'_>, String> {
        self.proxy("/kglobalaccel", "org.kde.KGlobalAccel")
    }

    fn dry(ctx: &Ctx, method: &str, args: &[String]) {
        let mut v = vec!["kglobalaccel".to_string(), method.to_string()];
        v.extend(args.iter().cloned());
        ctx.say(&format!("+ {}", quote(&v.iter().map(String::as_str).collect::<Vec<_>>())));
    }

    fn json(v: &[String]) -> String {
        let q: Vec<String> = v.iter().map(|s| format!("\"{s}\"")).collect();
        format!("[{}]", q.join(","))
    }

    fn do_register(&self, ctx: &Ctx, a: &str) -> Result<(), String> {
        if ctx.dry() {
            Kga::dry(ctx, "doRegister", &[Kga::json(&kde_id(a))]);
            return Ok(());
        }
        self.kga()?.call::<_, _, ()>("doRegister", &(kde_id(a),)).map_err(unreachable)
    }

    /// The keys KDE really set (empty when it refused).
    fn set_shortcut(&self, ctx: &Ctx, a: &str, keys: &[i32], flags: u32) -> Result<Vec<i32>, String> {
        if ctx.dry() {
            let k: Vec<String> = keys.iter().map(|k| k.to_string()).collect();
            Kga::dry(ctx, "setShortcut", &[Kga::json(&kde_id(a)), format!("[{}]", k.join(",")), flags.to_string()]);
            return Ok(keys.to_vec());
        }
        self.kga()?.call("setShortcut", &(kde_id(a), keys.to_vec(), flags)).map_err(unreachable)
    }

    fn unregister(&self, ctx: &Ctx, a: &str) -> Result<(), String> {
        let comp = format!("frame-{a}.desktop");
        if ctx.dry() {
            Kga::dry(ctx, "unregister", &[comp, "_launch".into()]);
            return Ok(());
        }
        self.kga()?.call::<_, _, bool>("unregister", &(comp.as_str(), "_launch")).map(|_| ()).map_err(unreachable)
    }

    /// No KDE shortcut holds KEY.
    fn free(&self, key: i32) -> Result<bool, String> {
        self.kga()?.call::<_, _, bool>("isGlobalShortcutAvailable", &(key, "")).map_err(unreachable)
    }

    fn active(&self, a: &str) -> bool {
        self.proxy(&format!("/component/frame_{a}_desktop"), "org.kde.kglobalaccel.Component")
            .and_then(|p| p.call::<_, _, bool>("isActive", &()).map_err(unreachable))
            .unwrap_or(false)
    }
}

/// The shortcut's launcher entry. X-KDE-DBUS-Restricted-Interfaces is
/// here too, not only in frame.desktop: KWin <= 6.7 authorizes ScreenShot2
/// by the first .desktop whose Exec names our binary, and that can be one
/// of these. (KWin 6.8 no longer checks; the key stays for older ones.)
fn kde_entry(a: &str, exec: &str) -> String {
    format!(
        "[Desktop Entry]\nType=Application\nName=frame {a}\nExec={exec}\nNoDisplay=true\nX-KDE-GlobalAccel-CommandShortcut=true\n\
         X-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2\n"
    )
}

/// Ok(false) = not bound (warned); Err = kglobalaccel is out of reach.
fn kde_bind(ctx: &Ctx, kga: &Kga, a: &str, sc: &str) -> Result<bool, String> {
    let Some(parsed) = keys::parse(sc) else { return Ok(false) };
    let want = keys::qt_keyint(&parsed);
    let d = kde_desktop(ctx, a);
    let exec = format!("{} {a}", desktop_arg(&ctx.exe.to_string_lossy()));
    if ctx.dry() {
        ctx.say(&format!("+ write {} (Exec={exec})", d.display()));
    } else if let Err(e) = d.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::write(&d, kde_entry(a, &exec))) {
        ctx.warn(&format!("{}: {e}", d.display()));
        return Ok(false);
    }
    kga.do_register(ctx, a)?;
    // Flags 6 = SetPresent|NoAutoloading. Without SetPresent KDE stores the
    // key but leaves the shortcut inactive, so pressing it does nothing.
    if ctx.dry() {
        kga.set_shortcut(ctx, a, &[want], 6)?;
        return Ok(true);
    }
    // KGlobalAccel answers with the keys it really set. A taken combo comes
    // back empty, but so does a free one while KDE hasn't yet picked up a
    // just-written .desktop entry (seen taking ~1.5 s), so retry those.
    for _ in 0..10 {
        if kga.set_shortcut(ctx, a, &[want], 6)? == [want] {
            if !kga.active(a) {
                ctx.warn(&format!("KDE stored {sc} but left it inactive ({a})"));
            }
            return Ok(true);
        }
        if !kga.free(want)? {
            break;
        }
        std::thread::sleep(Duration::from_millis(300));
        kga.do_register(ctx, a)?;
    }
    if kga.free(want)? {
        ctx.warn(&format!("KDE did not accept {sc}, not bound ({a})"));
    } else {
        ctx.warn(&format!("shortcut {sc} taken, not bound ({a})"));
    }
    kde_unbind(ctx, kga, a)?;
    Ok(false)
}

fn kde_unbind(ctx: &Ctx, kga: &Kga, a: &str) -> Result<(), String> {
    kga.set_shortcut(ctx, a, &[], 4)?;
    kga.unregister(ctx, a)?;
    let d = kde_desktop(ctx, a);
    if ctx.dry() {
        ctx.say(&format!("+ rm -f {}", quote1(&d.to_string_lossy())));
    } else {
        let _ = std::fs::remove_file(d);
    }
    Ok(())
}

// --------------------------------------------------------------- Hyprland

#[derive(PartialEq)]
enum Owner {
    Free,
    Ours,
    Taken,
}

/// Who holds SC in Hyprland (per `hyprctl binds -j`).
fn hypr_owner(ctx: &Ctx, sc: &str) -> Owner {
    let Some(p) = keys::parse(sc) else { return Owner::Free };
    let Some(v) = ctx.query(&["hyprctl", "binds", "-j"]).and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()) else {
        return Owner::Free;
    };
    let (mask, key) = (keys::hypr_mask(&p), p.sym.to_ascii_lowercase());
    let exe = ctx.exe.to_string_lossy();
    let hits: Vec<&serde_json::Value> = v
        .as_array()
        .map(|a| {
            a.iter()
                .filter(|b| b["modmask"].as_u64() == Some(mask) && b["key"].as_str().unwrap_or("").to_ascii_lowercase() == key)
                .collect()
        })
        .unwrap_or_default();
    if hits.is_empty() {
        Owner::Free
    } else if hits.iter().all(|b| b["arg"].as_str().unwrap_or("").contains(exe.as_ref())) {
        Owner::Ours
    } else {
        Owner::Taken
    }
}

fn hypr_bind(ctx: &Ctx, a: &str, sc: &str) -> bool {
    let Some(p) = keys::parse(sc) else { return false };
    let b = keys::combo(Style::Hypr, &p);
    match hypr_owner(ctx, sc) {
        Owner::Taken => {
            ctx.warn(&format!("shortcut {sc} is already bound in Hyprland, skipped ({a})"));
            return false;
        }
        Owner::Ours => {
            let _ = ctx.runq(&["hyprctl", "keyword", "unbind", &b]);
        }
        Owner::Free => {}
    }
    let cmd = format!("{b}, exec, {} {a}", quote1(&ctx.exe.to_string_lossy()));
    ctx.runq(&["hyprctl", "keyword", "bind", &cmd]).is_ok()
}

fn hypr_unbind(ctx: &Ctx, sc: &str) {
    let Some(p) = keys::parse(sc) else { return };
    if hypr_owner(ctx, sc) == Owner::Taken {
        ctx.warn(&format!("Hyprland now binds {sc} to something else; left alone"));
        return;
    }
    let _ = ctx.runq(&["hyprctl", "keyword", "unbind", &keys::combo(Style::Hypr, &p)]);
}

// ------------------------------------------------------------------- Sway

/// Lower-case, sorted combo so Mod4+Mod1+v == mod1+mod4+V.
fn sway_norm(s: &str) -> String {
    let mut v: Vec<String> =
        s.to_ascii_lowercase().split('+').map(|p| if p == "ctrl" { "control".into() } else { p.to_string() }).collect();
    v.sort();
    v.join("+")
}

/// Is SC bound in sway's config? Best effort: reads `swaymsg -t
/// get_config`, expands `set $var` lines, skips --flags.
fn sway_taken(ctx: &Ctx, sc: &str) -> bool {
    let Some(p) = keys::parse(sc) else { return false };
    let want = sway_norm(&keys::combo(Style::Sway, &p));
    let Some(v) = ctx.query(&["swaymsg", "-t", "get_config", "-r"]).and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok()) else {
        return false;
    };
    let mut texts: Vec<&str> = v["config"].as_str().into_iter().collect();
    if let Some(inc) = v["included_configs"].as_array() {
        texts.extend(inc.iter().filter_map(|i| i["raw_contents"].as_str()));
    }
    let mut vars: Vec<(String, String)> = Vec::new();
    for line in texts.iter().flat_map(|t| t.lines()).map(str::trim) {
        let w: Vec<&str> = line.split_whitespace().collect();
        match w.first() {
            Some(&"set") if w.len() >= 3 && w[1].starts_with('$') => vars.push((w[1].to_string(), w[2].to_string())),
            Some(&"bindsym") => {
                let Some(combo) = w[1..].iter().find(|x| !x.starts_with("--")) else { continue };
                let mut c = combo.to_string();
                for (k, val) in &vars {
                    c = c.replace(k.as_str(), val);
                }
                if sway_norm(&c) == want {
                    return true;
                }
            }
            _ => {}
        }
    }
    false
}

fn sway_bind(ctx: &Ctx, a: &str, sc: &str) -> bool {
    let Some(p) = keys::parse(sc) else { return false };
    if sway_taken(ctx, sc) {
        ctx.warn(&format!("shortcut {sc} is already bound in sway, skipped ({a})"));
        return false;
    }
    // One string, so the path may hold spaces.
    let cmd = format!("bindsym {} exec {} {a}", keys::combo(Style::Sway, &p), quote1(&ctx.exe.to_string_lossy()));
    ctx.runq(&["swaymsg", &cmd]).is_ok()
}

fn sway_unbind(ctx: &Ctx, sc: &str) {
    let Some(p) = keys::parse(sc) else { return };
    if sway_taken(ctx, sc) {
        ctx.warn(&format!("sway's config binds {sc}; left alone"));
        return;
    }
    let _ = ctx.runq(&["swaymsg", &format!("unbindsym {}", keys::combo(Style::Sway, &p))]);
}

// ------------------------------------------------------------------ GNOME

const GS: &str = "org.gnome.settings-daemon.plugins.media-keys";
const GP: &str = "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings";

/// GVariant string.
fn gv_str(s: &str) -> String {
    format!("'{}'", s.replace('\\', r"\\").replace('\'', r"\'"))
}

/// Current custom-keybinding paths. None unless gsettings answers with a
/// list, so we never write back a list we couldn't read.
fn gnome_paths(ctx: &Ctx) -> Option<Vec<String>> {
    let l = ctx.query(&["gsettings", "get", GS, "custom-keybindings"])?;
    let l = l.trim();
    let l = l.strip_prefix("@as ").unwrap_or(l);
    let inner = l.strip_prefix('[')?.strip_suffix(']')?;
    Some(
        inner
            .split(',')
            .map(|p| p.trim().trim_start_matches('\'').trim_end_matches('\'').to_string())
            .filter(|p| !p.is_empty())
            .collect(),
    )
}

/// Make frame's GNOME custom shortcuts exactly PAIRS.
fn gnome_apply(ctx: &Ctx, pairs: &[(String, String)]) -> bool {
    let Some(list) = gnome_paths(ctx) else {
        ctx.warn("cannot read GNOME custom shortcuts; nothing changed");
        return false;
    };
    let prefix = format!("{GP}/frame-");
    let (old, mut keep): (Vec<String>, Vec<String>) = list.into_iter().partition(|p| p.starts_with(&prefix));
    let mut mine = Vec::new();
    let exe = quote1(&ctx.exe.to_string_lossy());
    for (a, sc) in pairs {
        let Some(parsed) = keys::parse(sc) else { continue };
        let p = format!("{GP}/frame-{a}/");
        keep.push(p.clone());
        mine.push(p.clone());
        let schema = format!("{GS}.custom-keybinding:{p}");
        let _ = ctx.run(&["gsettings", "set", &schema, "name", &gv_str(&format!("frame {a}"))]);
        let _ = ctx.run(&["gsettings", "set", &schema, "command", &gv_str(&format!("{exe} {a}"))]);
        let _ = ctx.run(&["gsettings", "set", &schema, "binding", &gv_str(&keys::combo(Style::Gnome, &parsed))]);
    }
    let out: Vec<String> = keep.iter().map(|p| format!("'{p}'")).collect();
    let _ = ctx.run(&["gsettings", "set", GS, "custom-keybindings", &format!("[{}]", out.join(", "))]);
    for p in old.iter().filter(|p| !mine.contains(p)) {
        let schema = format!("{GS}.custom-keybinding:{p}");
        for k in ["name", "command", "binding"] {
            let _ = ctx.run(&["gsettings", "reset", &schema, k]);
        }
    }
    true
}

// --------------------------------------------------------------- commands

/// Remove what frame bound last time on BACKEND (Hyprland, Sway, KDE).
fn unbind_saved(ctx: &Ctx, be: Desktop, kga: &Kga) -> Result<(), String> {
    let saved = state_get(ctx, backend_name(be));
    for (a, sc) in &saved {
        match be {
            Desktop::Kde => kde_unbind(ctx, kga, a)?,
            Desktop::Hyprland if !sc.is_empty() => hypr_unbind(ctx, sc),
            Desktop::Sway if !sc.is_empty() => sway_unbind(ctx, sc),
            _ => {}
        }
    }
    if be == Desktop::Kde {
        let has = |a: &str| saved.iter().any(|(s, _)| s == a);
        // .desktop shortcuts left without a state line.
        for a in ACTIONS.iter().filter(|a| !has(a) && kde_desktop(ctx, a).symlink_metadata().is_ok()) {
            kde_unbind(ctx, kga, a)?;
        }
        // 0.3's replay / record go whether or not their .desktop entries are
        // still there: installing 0.4+ deletes those files before the tray
        // gets to unbind them.
        for a in OLD_ACTIONS.iter().filter(|a| !has(a)) {
            kde_unbind(ctx, kga, a)?;
        }
    }
    Ok(())
}

/// One bind/unbind at a time (the tray binds when it starts, the settings
/// window on Save). The lock is released when the Flock drops.
fn bind_lock(ctx: &Ctx) -> Result<crate::ctx::Flock, String> {
    let dir = ctx.ensure_rundir()?;
    let wait: u64 = ctx.var("FRAME_BIND_WAIT").parse().unwrap_or(30);
    let end = Instant::now() + Duration::from_secs(wait);
    loop {
        match crate::ctx::Flock::try_new(&dir.join("bind.lock")) {
            Ok(Some(l)) => return Ok(l),
            Ok(None) if Instant::now() < end => std::thread::sleep(Duration::from_millis(100)),
            Ok(None) => return Err("another frame bind/unbind is still running".into()),
            Err(e) => return Err(format!("bind.lock: {e}")),
        }
    }
}

pub fn bind(ctx: &Ctx, cfg: &Cfg) -> Result<(), String> {
    let _lock = bind_lock(ctx)?;
    let exe = quote1(&ctx.exe.to_string_lossy());
    let Some(be) = detect::binds(ctx) else {
        ctx.say("frame: no shortcut support for this desktop; bind these yourself:");
        for a in ACTIONS {
            let sc = &cfg[&format!("bind.{a}")];
            if !sc.is_empty() {
                ctx.say(&format!("  {sc}  ->  {exe} {a}"));
            }
        }
        return Ok(());
    };
    let wanted: Vec<(String, String)> = ACTIONS
        .iter()
        .map(|a| (a.to_string(), cfg[&format!("bind.{a}")].clone()))
        .filter(|(_, sc)| !sc.is_empty())
        .collect();
    let applied = if be == Desktop::Gnome {
        if !gnome_apply(ctx, &wanted) {
            return Err("GNOME shortcuts unchanged".into());
        }
        wanted
    } else {
        let kga = Kga::new(ctx, be == Desktop::Kde)?;
        unbind_saved(ctx, be, &kga)?;
        let mut applied = Vec::new();
        for (a, sc) in wanted {
            let ok = match be {
                Desktop::Kde => kde_bind(ctx, &kga, &a, &sc)?,
                Desktop::Hyprland => hypr_bind(ctx, &a, &sc),
                _ => sway_bind(ctx, &a, &sc),
            };
            if ok {
                applied.push((a, sc));
            }
        }
        applied
    };
    state_save(ctx, backend_name(be), &applied);
    Ok(())
}

pub fn unbind(ctx: &Ctx) -> Result<(), String> {
    let _lock = bind_lock(ctx)?;
    let Some(be) = detect::binds(ctx) else {
        ctx.say("frame: remove your manual shortcuts for frame");
        return Ok(());
    };
    if be == Desktop::Gnome {
        if !gnome_apply(ctx, &[]) {
            return Err("GNOME shortcuts unchanged".into());
        }
    } else {
        unbind_saved(ctx, be, &Kga::new(ctx, be == Desktop::Kde)?)?;
    }
    state_save(ctx, backend_name(be), &[]);
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::ctx::{quote1, testenv::T};

    fn state(t: &T, text: &str) {
        std::fs::create_dir_all(t.dir.join("state/frame")).unwrap();
        std::fs::write(t.dir.join("state/frame/binds"), text).unwrap();
    }

    const REGION_ID: &str = r#"'["frame-region.desktop","_launch","frame region","frame region"]'"#;

    #[test]
    fn kde_bind_dry() {
        let t = T::new("kdebind");
        t.conf(&["bind.window ="]);
        let (code, out, err) = t.frame(&["bind"]);
        assert_eq!(code, 0, "{err}");
        assert!(out.contains(&format!("+ kglobalaccel setShortcut {REGION_ID} '[402653266]' 6")), "{out}");
        assert!(out.contains(&format!("+ write {}/data/applications/frame-region.desktop (Exec=/opt/frame/bin/frame region)", t.dir.display())), "{out}");
        assert!(out.contains(&format!("+ save {}/state/frame/binds: kde region Meta+Alt+R monitor Meta+Alt+M all Meta+Alt+A\n", t.dir.display())), "{out}");
        // 0.3's replay / record are dropped even with no state and no files.
        for a in ["replay", "record"] {
            assert!(out.contains(&format!("+ kglobalaccel setShortcut '[\"frame-{a}.desktop\",\"_launch\",\"frame {a}\",\"frame {a}\"]' '[]' 4")), "{out}");
            assert!(out.contains(&format!("+ kglobalaccel unregister frame-{a}.desktop _launch")), "{out}");
            assert!(!out.contains(&format!("write {}/data/applications/frame-{a}", t.dir.display())));
        }
        assert!(!t.dir.join("data/applications/frame-region.desktop").exists());
        let (_, out, _) = t.frame(&["unbind"]);
        assert!(out.contains("unregister frame-replay.desktop") && out.contains("unregister frame-record.desktop"), "{out}");
    }

    /// No session bus (testenv has a temp XDG_RUNTIME_DIR and no address):
    /// a clear error and exit 1, never "taken", and no state is saved.
    #[test]
    fn kde_without_dbus() {
        let mut t = T::new("kdenobus");
        t.conf(&[]);
        t.unset("FRAME_DRY_RUN");
        for cmd in ["bind", "unbind"] {
            let (code, out, err) = t.frame(&[cmd]);
            assert_eq!(code, 1, "{cmd}: {out}{err}");
            assert!(err.contains("can't reach kglobalaccel"), "{err}");
            assert!(!err.contains("taken"), "{err}");
        }
        assert!(!t.dir.join("state/frame/binds").exists());
        assert!(!t.dir.join("data/applications/frame-region.desktop").exists());
    }

    #[test]
    fn kde_entry_authorizes_screenshots() {
        let e = super::kde_entry("region", "/home/u/.local/bin/frame region");
        assert!(e.contains("\nExec=/home/u/.local/bin/frame region\n"), "{e}");
        assert!(e.ends_with("\nX-KDE-DBUS-Restricted-Interfaces=org.kde.KWin.ScreenShot2\n"), "{e}");
    }

    #[test]
    fn kde_unbind_from_state() {
        let t = T::new("kdeunbind");
        t.conf(&[]);
        state(&t, "kde window Meta+Alt+W\nkde all Meta+Alt+A\nhyprland region Meta+Shift+R\n");
        let (code, out, _) = t.frame(&["unbind"]);
        assert_eq!(code, 0);
        assert_eq!(out.matches("unregister frame-window").count() + out.matches("unregister frame-all").count(), 2, "{out}");
        assert!(!out.contains("frame-region"), "{out}");
        assert!(out.contains("+ save ") && out.trim_end().ends_with(": kde (none)"), "{out}");
    }

    #[test]
    fn hyprland() {
        let mut t = T::new("hypr");
        t.conf(&[]);
        t.set("XDG_CURRENT_DESKTOP", "Hyprland");
        t.fake("hyprctl", r#"[ "$1" = binds ] && echo '[{"modmask":72,"key":"A","dispatcher":"exec","arg":"firefox"}]'; exit 0"#);
        let (code, out, err) = t.frame(&["bind"]);
        assert_eq!(code, 0);
        assert!(out.contains("+ hyprctl keyword bind 'SUPER ALT, r, exec, /opt/frame/bin/frame region'"), "{out}");
        assert!(err.contains("Meta+Alt+A is already bound in Hyprland, skipped (all)"), "{err}");
        assert!(!out.contains("ALT, a, exec"));
        assert!(out.contains(": hyprland region Meta+Alt+R window Meta+Alt+W monitor Meta+Alt+M\n"), "{out}");
        state(&t, "hyprland region Meta+Shift+R\nhyprland all Meta+Alt+V\nkde x Meta+Alt+R\n");
        let (_, out, _) = t.frame(&["unbind"]);
        assert!(out.contains("+ hyprctl keyword unbind 'SUPER SHIFT, r'") && out.contains("+ hyprctl keyword unbind 'SUPER ALT, v'"), "{out}");
        assert!(!out.contains("ALT, r'"), "{out}");
    }

    #[test]
    fn sway() {
        let mut t = T::new("sway");
        t.conf(&[]);
        t.set("XDG_CURRENT_DESKTOP", "sway");
        t.fake("swaymsg", r#"[ "$2" = get_config ] && printf '%s\n' '{"config":"set $mod Mod4\nbindsym --release $mod+Mod1+a exec foo\n"}'; exit 0"#);
        let (_, out, err) = t.frame(&["bind"]);
        assert!(out.contains("+ swaymsg 'bindsym Mod4+Mod1+r exec /opt/frame/bin/frame region'"), "{out}");
        assert!(err.contains("Meta+Alt+A is already bound in sway, skipped (all)"), "{err}");
    }

    #[test]
    fn gnome() {
        let mut t = T::new("gnome");
        t.conf(&["bind.window ="]);
        t.set("XDG_CURRENT_DESKTOP", "GNOME");
        let p = "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings";
        let log = t.dir.join("gs.log");
        t.fake("gsettings", &format!("echo \"$*\" >>'{}'; [ \"$1\" = get ] && echo \"['{p}/custom0/', '{p}/frame-replay/', '{p}/frame-all/']\"; exit 0", log.display()));
        let (code, out, _) = t.frame(&["bind"]);
        assert_eq!(code, 0);
        let s = "org.gnome.settings-daemon.plugins.media-keys";
        let q = |v: &str| quote1(&super::gv_str(v));
        assert!(out.contains(&format!("+ gsettings set {s}.custom-keybinding:{p}/frame-region/ binding {}\n", q("<Super><Alt>r"))), "{out}");
        assert!(out.contains(&format!(" command {}\n", q("/opt/frame/bin/frame region"))), "{out}");
        let list = format!("['{p}/custom0/', '{p}/frame-region/', '{p}/frame-monitor/', '{p}/frame-all/']");
        assert!(out.contains(&format!("+ gsettings set {s} custom-keybindings {}\n", quote1(&list))), "{out}");
        assert!(out.contains(&format!("+ gsettings reset {s}.custom-keybinding:{p}/frame-replay/ name")), "{out}");
        assert!(!out.contains(&format!("reset {s}.custom-keybinding:{p}/frame-all/")), "{out}");
        // Only the read ran for real.
        assert_eq!(std::fs::read_to_string(&log).unwrap().trim(), format!("get {s} custom-keybindings"));
        t.fake("gsettings", "echo 'No schemas installed'");
        let (code, _, err) = t.frame(&["bind"]);
        assert_eq!(code, 1);
        assert!(err.contains("cannot read GNOME custom shortcuts; nothing changed"), "{err}");
    }

    #[test]
    fn manual_and_injection() {
        let mut t = T::new("manual");
        t.conf(&[]);
        t.set("XDG_CURRENT_DESKTOP", "frame-test");
        let (code, out, _) = t.frame(&["bind"]);
        assert_eq!(code, 0);
        assert!(out.contains("  Meta+Alt+R  ->  /opt/frame/bin/frame region\n"), "{out}");
        assert!(!out.contains("+ save"), "{out}");
        t.conf(&["bind.region = Meta+Alt+R; touch PWNED"]);
        t.set("XDG_CURRENT_DESKTOP", "Hyprland");
        let (code, out, err) = t.frame(&["bind"]);
        assert_eq!(code, 2);
        assert!(!out.contains("hyprctl") && err.contains("is not a shortcut"), "{out}{err}");
    }

    #[test]
    fn lock_busy() {
        let mut t = T::new("lock");
        t.conf(&[]);
        t.set("FRAME_BIND_WAIT", "0").set("XDG_CURRENT_DESKTOP", "frame-test");
        let held = super::bind_lock(&t.ctx()).unwrap();
        let (code, _, err) = t.frame(&["bind"]);
        assert_eq!(code, 1);
        assert!(err.contains("another frame bind/unbind is still running"), "{err}");
        drop(held);
        // A child another test thread forks may hold a copy of the fd
        // until it execs (close-on-exec), so allow a moment.
        t.set("FRAME_BIND_WAIT", "2");
        let again = t.frame(&["bind"]);
        assert_eq!(again.0, 0, "{again:?}");
    }
}
