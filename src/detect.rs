//! Which desktop we are on, and which tool does each job there. Picked at
//! runtime; never assume one desktop.

use crate::ctx::Ctx;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Desktop {
    Hyprland,
    Sway,
    Kde,
    Gnome,
    Other,
}

pub fn session(ctx: &Ctx) -> String {
    let s = ctx.var("XDG_SESSION_TYPE");
    if s.is_empty() {
        "unknown".into()
    } else {
        s
    }
}

pub fn desktop(ctx: &Ctx) -> Desktop {
    let d = ctx.var("XDG_CURRENT_DESKTOP").to_ascii_lowercase();
    if ctx.var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some() || d.contains("hyprland") {
        Desktop::Hyprland
    } else if ctx.var_os("SWAYSOCK").is_some() || d.contains("sway") {
        Desktop::Sway
    } else if d.contains("kde") {
        Desktop::Kde
    } else if d.contains("gnome") {
        Desktop::Gnome
    } else {
        Desktop::Other
    }
}

/// The desktop's name for --detect (the raw value when we don't know it).
pub fn desktop_name(ctx: &Ctx) -> String {
    match desktop(ctx) {
        Desktop::Hyprland => "hyprland".into(),
        Desktop::Sway => "sway".into(),
        Desktop::Kde => "kde".into(),
        Desktop::Gnome => "gnome".into(),
        Desktop::Other => {
            let d = ctx.var("XDG_CURRENT_DESKTOP").to_ascii_lowercase();
            if d.is_empty() {
                "unknown".into()
            } else {
                d
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Capture {
    /// KWin's ScreenShot2, in this process (KDE Wayland).
    Kwin,
    /// ext-image-copy-capture / wlr-screencopy (Sway, Hyprland, other
    /// wlroots; falls back to the portal when the compositor has neither).
    Screencopy,
    /// org.freedesktop.portal.Screenshot (GNOME: Mutter lets no other
    /// client copy the screen).
    Portal,
    /// GetImage on the root window.
    X11,
    None,
}

impl Capture {
    pub fn name(self) -> &'static str {
        match self {
            Capture::Kwin => "kwin",
            Capture::Screencopy => "screencopy",
            Capture::Portal => "portal",
            Capture::X11 => "x11",
            Capture::None => "none",
        }
    }
}

/// Wayland or X11: XDG_SESSION_TYPE, else whichever display is set.
fn display_kind(ctx: &Ctx) -> Option<&'static str> {
    match session(ctx).as_str() {
        "wayland" => Some("wayland"),
        "x11" => Some("x11"),
        _ if ctx.var_os("WAYLAND_DISPLAY").is_some() => Some("wayland"),
        _ if ctx.var_os("DISPLAY").is_some() => Some("x11"),
        _ => None,
    }
}

/// All built in; picked by desktop and session. grim-style screencopy
/// does not work on KWin or Mutter, so those get their own APIs.
pub fn capture(ctx: &Ctx) -> Capture {
    match (display_kind(ctx), desktop(ctx)) {
        (Some("wayland"), Desktop::Kde) => Capture::Kwin,
        (Some("wayland"), Desktop::Gnome) => Capture::Portal,
        (Some("wayland"), _) => Capture::Screencopy,
        (Some(_), _) => Capture::X11,
        (None, _) => Capture::None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Clipboard {
    /// wlr data-control (KWin, Sway, Hyprland, other wlroots).
    DataControl,
    /// GNOME: the core wl_data_device from a briefly focused window.
    Focus,
    /// Our own CLIPBOARD selection owner.
    X11,
    None,
}

impl Clipboard {
    /// Also the method argument of `frame _clip FILE MIME HOW`.
    pub fn name(self) -> &'static str {
        match self {
            Clipboard::DataControl => "data-control",
            Clipboard::Focus => "focus",
            Clipboard::X11 => "x11",
            Clipboard::None => "none",
        }
    }
}

/// Mutter (GNOME) has no data-control protocol, hence Focus there.
pub fn clipboard(ctx: &Ctx) -> Clipboard {
    match (display_kind(ctx), desktop(ctx)) {
        (Some("wayland"), Desktop::Gnome) => Clipboard::Focus,
        (Some("wayland"), _) => Clipboard::DataControl,
        (Some(_), _) => Clipboard::X11,
        (None, _) => Clipboard::None,
    }
}

/// Shortcut backend: a desktop we can bind on, or None (manual).
pub fn binds(ctx: &Ctx) -> Option<Desktop> {
    Some(desktop(ctx)).filter(|d| *d != Desktop::Other)
}

pub fn binds_name(ctx: &Ctx) -> &'static str {
    match binds(ctx) {
        Some(Desktop::Hyprland) => "hyprland",
        Some(Desktop::Sway) => "sway",
        Some(Desktop::Kde) => "kde",
        Some(Desktop::Gnome) => "gnome",
        _ => "manual",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ctx::testenv::T;

    #[test]
    fn backends() {
        let mut t = T::new("detect");
        assert_eq!(capture(&t.ctx()), Capture::Kwin);
        assert_eq!(clipboard(&t.ctx()), Clipboard::DataControl);
        t.set("XDG_SESSION_TYPE", "x11");
        assert_eq!((capture(&t.ctx()), clipboard(&t.ctx())), (Capture::X11, Clipboard::X11));
        t.set("XDG_SESSION_TYPE", "wayland").set("XDG_CURRENT_DESKTOP", "GNOME");
        assert_eq!((capture(&t.ctx()), clipboard(&t.ctx())), (Capture::Portal, Clipboard::Focus));
        t.set("XDG_CURRENT_DESKTOP", "frame-test");
        assert_eq!((capture(&t.ctx()), clipboard(&t.ctx())), (Capture::Screencopy, Clipboard::DataControl));
        assert_eq!(binds_name(&t.ctx()), "manual");
        assert_eq!(desktop_name(&t.ctx()), "frame-test");
        // No session type: whichever display is set; none = nothing.
        t.unset("XDG_SESSION_TYPE");
        assert_eq!(capture(&t.ctx()), Capture::None);
        t.set("DISPLAY", ":5");
        assert_eq!(capture(&t.ctx()), Capture::X11);
        t.set("WAYLAND_DISPLAY", "wayland-9");
        assert_eq!(capture(&t.ctx()), Capture::Screencopy);
        t.set("SWAYSOCK", "/x");
        assert_eq!(desktop(&t.ctx()), Desktop::Sway);
        t.set("HYPRLAND_INSTANCE_SIGNATURE", "y");
        assert_eq!(binds_name(&t.ctx()), "hyprland");
    }
}
