//! KDE-style shortcut names (Meta+Alt+R) and what each desktop calls them.

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mod {
    Meta,
    Ctrl,
    Alt,
    Shift,
}

#[derive(Debug, PartialEq)]
pub struct Shortcut {
    /// In the order written (duplicates kept, as the bash version did).
    pub mods: Vec<Mod>,
    /// Qt key code and XKB keysym name of the one key.
    pub qt: u32,
    pub sym: String,
}

/// ASCII whitespace only, whatever the locale.
fn is_sp(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r')
}

/// "Meta+Alt+R" -> Shortcut. Every part must be a modifier or a known key,
/// with exactly one key. This is also what keeps config text out of the
/// commands we build.
pub fn parse(s: &str) -> Option<Shortcut> {
    let s: String = s.chars().filter(|c| !is_sp(*c)).collect();
    if s.is_empty() || s.ends_with('+') {
        return None;
    }
    let (mut mods, mut key) = (Vec::new(), None);
    for part in s.split('+') {
        // ASCII lowercase only: the Kelvin sign is not k.
        match part.to_ascii_lowercase().as_str() {
            "meta" | "super" | "win" => mods.push(Mod::Meta),
            "ctrl" | "control" => mods.push(Mod::Ctrl),
            "alt" => mods.push(Mod::Alt),
            "shift" => mods.push(Mod::Shift),
            p => {
                if key.is_some() {
                    return None;
                }
                key = Some(key_info(p)?);
            }
        }
    }
    let (qt, sym) = key?;
    Some(Shortcut { mods, qt, sym })
}

/// Lower-case key name -> (Qt key code, XKB keysym).
fn key_info(k: &str) -> Option<(u32, String)> {
    let named = |q: u32, s: &str| Some((0x0100_0000 | q, s.to_string()));
    match k {
        "esc" | "escape" => named(0x00, "Escape"),
        "tab" => named(0x01, "Tab"),
        "backspace" => named(0x03, "BackSpace"),
        "return" | "enter" => named(0x04, "Return"),
        "ins" | "insert" => named(0x06, "Insert"),
        "del" | "delete" => named(0x07, "Delete"),
        "pause" => named(0x08, "Pause"),
        "print" => named(0x09, "Print"),
        "home" => named(0x10, "Home"),
        "end" => named(0x11, "End"),
        "left" => named(0x12, "Left"),
        "up" => named(0x13, "Up"),
        "right" => named(0x14, "Right"),
        "down" => named(0x15, "Down"),
        "pgup" | "pageup" => named(0x16, "Prior"),
        "pgdown" | "pagedown" => named(0x17, "Next"),
        "space" => Some((32, "space".into())),
        _ => {
            let b = k.as_bytes();
            if b.len() == 1 && (b[0].is_ascii_lowercase() || b[0].is_ascii_digit()) {
                return Some((b[0].to_ascii_uppercase() as u32, k.to_string()));
            }
            let n = k.strip_prefix('f').filter(|n| !n.starts_with('0') && n.bytes().all(|c| c.is_ascii_digit()))?;
            let n: u32 = n.parse().ok().filter(|n| (1..=35).contains(n))?;
            Some((0x0100_0030 + n - 1, format!("F{n}")))
        }
    }
}

/// Qt key code for KDE, e.g. Meta+Alt+F12 -> 419430459.
pub fn qt_keyint(sc: &Shortcut) -> i32 {
    let mods = sc.mods.iter().fold(0u32, |m, x| {
        m | match x {
            Mod::Meta => 0x1000_0000,
            Mod::Ctrl => 0x0400_0000,
            Mod::Alt => 0x0800_0000,
            Mod::Shift => 0x0200_0000,
        }
    });
    (mods | sc.qt) as i32
}

#[derive(Clone, Copy)]
pub enum Style {
    Hypr,
    Sway,
    Gnome,
}

/// The shortcut in that desktop's syntax.
pub fn combo(style: Style, sc: &Shortcut) -> String {
    let m: Vec<&str> = sc
        .mods
        .iter()
        .map(|x| match (style, x) {
            (Style::Hypr, Mod::Meta) => "SUPER",
            (Style::Hypr, Mod::Ctrl) => "CTRL",
            (Style::Hypr, Mod::Alt) => "ALT",
            (Style::Hypr, Mod::Shift) => "SHIFT",
            (Style::Sway, Mod::Meta) => "Mod4",
            (Style::Sway, Mod::Ctrl) => "Control",
            (Style::Sway, Mod::Alt) => "Mod1",
            (Style::Sway, Mod::Shift) => "Shift",
            (Style::Gnome, Mod::Meta) => "<Super>",
            (Style::Gnome, Mod::Ctrl) => "<Ctrl>",
            (Style::Gnome, Mod::Alt) => "<Alt>",
            (Style::Gnome, Mod::Shift) => "<Shift>",
        })
        .collect();
    match style {
        Style::Hypr => format!("{}, {}", m.join(" "), sc.sym),
        Style::Sway if m.is_empty() => sc.sym.clone(),
        Style::Sway => format!("{}+{}", m.join("+"), sc.sym),
        Style::Gnome => format!("{}{}", m.concat(), sc.sym),
    }
}

/// Hyprland's modmask bits for the modifiers.
pub fn hypr_mask(sc: &Shortcut) -> u64 {
    sc.mods.iter().fold(0, |m, x| {
        m | match x {
            Mod::Shift => 1,
            Mod::Ctrl => 4,
            Mod::Alt => 8,
            Mod::Meta => 64,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> Shortcut {
        parse(s).unwrap_or_else(|| panic!("{s}"))
    }

    #[test]
    fn keyints() {
        assert_eq!(qt_keyint(&p("Meta+Alt+F12")), 419430459);
        assert_eq!(qt_keyint(&p("Meta+Alt+R")), 0x1800_0052);
        assert_eq!(qt_keyint(&p("Ctrl+Shift+Print")), 0x0600_0000 | 0x0100_0009);
        assert_eq!(qt_keyint(&p(" ctrl + shift + print ")), 0x0600_0000 | 0x0100_0009);
        assert_eq!(qt_keyint(&p("Meta+5")), 0x1000_0035);
    }

    #[test]
    fn desktop_names() {
        assert_eq!(combo(Style::Sway, &p("Meta+PgUp")), "Mod4+Prior");
        assert_eq!(combo(Style::Hypr, &p("Alt+backspace")), "ALT, BackSpace");
        assert_eq!(combo(Style::Gnome, &p("Ctrl+Ins")), "<Ctrl>Insert");
        assert_eq!(combo(Style::Hypr, &p("Meta+Alt+R")), "SUPER ALT, r");
        assert_eq!(combo(Style::Sway, &p("F9")), "F9");
        assert_eq!(hypr_mask(&p("Meta+Alt+A")), 72);
    }

    #[test]
    fn rejects() {
        for bad in ["Meta+Foo+R", "Meta+Alt+", "Meta+R+T", "Meta+Alt", "Meta+,", "Meta+F36", "Meta+é", "", "Meta+F0", "Meta+F01", "Meta+\u{212a}", "Meta+\u{3000}R", "Meta+Alt+R; touch /tmp/x"] {
            assert!(parse(bad).is_none(), "{bad}");
        }
    }
}
