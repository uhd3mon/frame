//! The clip config file: same reading rules as `load_config` in bin/clip,
//! and a writer that changes only the values it has to.

use std::collections::BTreeMap;

/// Keys and defaults, in the order of `default_config()` in bin/clip.
pub const DEFAULTS: &[(&str, &str)] = &[
    ("save_dir", "~/Documents"),
    ("image_format", "png"),
    ("video_format", "mp4"),
    ("clipboard", "true"),
    ("notify", "true"),
    ("replay", "true"),
    ("replay_seconds", "auto"),
    ("video_source", "auto"),
    ("video_fps", "60"),
    ("video_quality", "very_high"),
    ("video_codec", "auto"),
    ("audio", "default_output"),
    ("bind.region", "Meta+Alt+R"),
    ("bind.window", "Meta+Alt+W"),
    ("bind.monitor", "Meta+Alt+M"),
    ("bind.all", "Meta+Alt+A"),
    ("bind.replay", "Meta+Alt+C"),
    ("bind.record", "Meta+Alt+V"),
];

pub const ACTIONS: &[&str] = &["region", "window", "monitor", "all", "replay", "record"];

pub fn is_known(key: &str) -> bool {
    DEFAULTS.iter().any(|(k, _)| *k == key)
}

/// Bash's `[[:space:]]` under `LC_ALL=C` (load_config sets it): ASCII only.
fn is_sp(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r')
}

fn trim_start(s: &[u8]) -> &[u8] {
    let n = s.iter().take_while(|b| is_sp(**b)).count();
    &s[n..]
}

fn trim_end(s: &[u8]) -> &[u8] {
    let n = s.iter().rev().take_while(|b| is_sp(**b)).count();
    &s[..s.len() - n]
}

/// One assignment line, split into byte ranges of the original line.
struct Assign {
    key: String,
    /// Start of the value (after `=` and the spaces that follow it).
    vstart: usize,
    /// End of the key = value part: where " # comment" starts, or the line end.
    cut: usize,
    /// The value as bash sees it (trailing spaces gone, one pair of quotes off).
    value: Vec<u8>,
}

/// Parse a line like bash does (bytes, C locale); None for blank, comment
/// or junk lines. Works on bytes so a file that isn't UTF-8 round-trips.
fn parse_line(line: &[u8]) -> Option<Assign> {
    let t = trim_start(line);
    if t.is_empty() || t[0] == b'#' {
        return None;
    }
    // A comment starts at the first whitespace that is followed by '#'.
    let cut = (1..line.len()).find(|&i| line[i] == b'#' && is_sp(line[i - 1])).map_or(line.len(), |i| i - 1);
    let head = &line[..cut];
    let ks = head.len() - trim_start(head).len();
    let klen = head[ks..].iter().take_while(|b| b.is_ascii_lowercase() || **b == b'_' || **b == b'.').count();
    if klen == 0 {
        return None;
    }
    let key = String::from_utf8_lossy(&head[ks..ks + klen]).into_owned();
    let eq = trim_start(&head[ks + klen..]).strip_prefix(b"=")?;
    let raw = trim_start(eq);
    let vstart = head.len() - raw.len();
    let mut value = trim_end(raw);
    if value.len() >= 2 && (value[0] == b'"' || value[0] == b'\'') && value[value.len() - 1] == value[0] {
        value = &value[1..value.len() - 1];
    }
    Some(Assign { key, vstart, cut, value: value.to_vec() })
}

/// bash `read -r` lines: split on \n, a last line without \n still counts.
fn lines(text: &[u8]) -> Vec<&[u8]> {
    let mut v: Vec<&[u8]> = text.split(|b| *b == b'\n').collect();
    if v.last().is_some_and(|l| l.is_empty()) {
        v.pop();
    }
    v
}

/// Effective settings: defaults, then every known key in the file (last
/// wins). A value that isn't UTF-8 is shown lossily; it is only written
/// back if the user edits it.
pub fn parse(text: impl AsRef<[u8]>) -> BTreeMap<String, String> {
    let mut m: BTreeMap<String, String> =
        DEFAULTS.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    for l in lines(text.as_ref()) {
        if let Some(a) = parse_line(l) {
            if is_known(&a.key) {
                m.insert(a.key, String::from_utf8_lossy(&a.value).into_owned());
            }
        }
    }
    m
}

/// Why the bash parser could not read this value back, if it couldn't.
pub fn unrepresentable(value: &str) -> Option<&'static str> {
    if value.chars().any(|c| c.is_control()) {
        return Some("contains a tab, newline or other control character");
    }
    if (1..value.len()).any(|i| value.as_bytes()[i] == b'#' && is_sp(value.as_bytes()[i - 1])) {
        return Some("contains a space followed by # (that starts a comment)");
    }
    None
}

/// The text to write after `key = ` so bash reads back exactly `value`.
/// Quotes only when needed (edge spaces, a leading #, or a quote pair).
pub fn format_value(value: &str) -> String {
    let b = value.as_bytes();
    let edge_space = b.first().is_some_and(|c| is_sp(*c)) || b.last().is_some_and(|c| is_sp(*c));
    let quote_pair = b.len() >= 2 && (b[0] == b'"' || b[0] == b'\'') && b[b.len() - 1] == b[0];
    if edge_space || quote_pair || value.starts_with('#') {
        format!("\"{value}\"")
    } else {
        value.to_string()
    }
}

/// Apply `changes` (key -> new value) to config text. Lines of changed
/// keys get the new value and keep their indent, trailing comment (at the
/// same column when it fits) and a trailing \r; everything else stays
/// byte for byte. Changed keys the file doesn't have are appended.
pub fn apply(text: &[u8], changes: &BTreeMap<String, String>) -> Result<Vec<u8>, String> {
    for (k, v) in changes {
        if let Some(why) = unrepresentable(v) {
            return Err(format!("{k}: value {why}; clip can't read that back"));
        }
    }
    let mut out = Vec::with_capacity(text.len() + 64);
    let mut seen = std::collections::BTreeSet::new();
    for l in lines(text) {
        let (body, cr) = match l.strip_suffix(b"\r") {
            Some(b) => (b, true),
            None => (l, false),
        };
        match parse_line(body).filter(|a| changes.contains_key(&a.key)) {
            Some(a) => {
                let newv = format_value(&changes[&a.key]);
                let mut pre = body[..a.vstart].to_vec();
                if !pre.last().is_some_and(|b| is_sp(*b)) {
                    pre.push(b' ');
                }
                let comment = &body[a.cut..];
                if newv.is_empty() && comment.is_empty() {
                    pre.truncate(trim_end(&pre).len()); // "key =", no trailing space
                }
                out.extend_from_slice(&pre);
                out.extend_from_slice(newv.as_bytes());
                if !comment.is_empty() {
                    let old_w = String::from_utf8_lossy(&body[a.vstart..a.cut]).chars().count();
                    let new_w = newv.chars().count();
                    out.extend(std::iter::repeat_n(b' ', old_w.saturating_sub(new_w)));
                    out.extend_from_slice(comment);
                }
                if cr {
                    out.push(b'\r');
                }
                seen.insert(a.key);
            }
            None => out.extend_from_slice(l),
        }
        out.push(b'\n');
    }
    for (k, _) in DEFAULTS {
        if let Some(v) = changes.get(*k) {
            if !seen.contains(*k) {
                let line = format!("{k} = {}", format_value(v));
                out.extend_from_slice(line.trim_end().as_bytes());
                out.push(b'\n');
            }
        }
    }
    Ok(out)
}

/// Same rule as `parse_shortcut` in bin/clip: modifiers plus one key.
pub fn valid_shortcut(s: &str) -> bool {
    let s: String = s.chars().filter(|c| !c.is_ascii() || !is_sp(*c as u8)).collect();
    if s.is_empty() || s.ends_with('+') {
        return false;
    }
    let mut key = false;
    for part in s.split('+') {
        // ASCII only, like ${1,,} under LC_ALL=C (the Kelvin sign is not k).
        match part.to_ascii_lowercase().as_str() {
            "meta" | "super" | "win" | "ctrl" | "control" | "alt" | "shift" => {}
            p => {
                if key || !known_key(p) {
                    return false;
                }
                key = true;
            }
        }
    }
    key
}

fn known_key(k: &str) -> bool {
    const NAMED: &[&str] = &[
        "esc", "escape", "tab", "backspace", "return", "enter", "ins", "insert", "del", "delete",
        "pause", "print", "home", "end", "left", "up", "right", "down", "pgup", "pageup",
        "pgdown", "pagedown", "space",
    ];
    if NAMED.contains(&k) {
        return true;
    }
    if k.len() == 1 {
        return k.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    }
    if let Some(n) = k.strip_prefix('f') {
        if !n.starts_with('0') {
            if let Ok(n) = n.parse::<u32>() {
                return (1..=35).contains(&n);
            }
        }
    }
    false
}

/// Would `validate_config` in bin/clip accept V for KEY on its own?
/// (Free-text keys and the webm/codec pair aren't checked here.)
pub fn valid_value(key: &str, v: &str) -> bool {
    let int_in = |lo: u64, hi: u64| {
        !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit()) && v.parse::<u64>().is_ok_and(|n| (lo..=hi).contains(&n))
    };
    match key {
        "image_format" => matches!(v, "png" | "jpg" | "webp"),
        "video_format" => matches!(v, "mp4" | "webm" | "mkv"),
        "clipboard" | "notify" | "replay" => {
            matches!(v.to_ascii_lowercase().as_str(), "true" | "false" | "yes" | "no" | "on" | "off" | "1" | "0")
        }
        "replay_seconds" => v == "auto" || int_in(5, 1800),
        "video_fps" => int_in(1, 240),
        "video_quality" => matches!(v, "medium" | "high" | "very_high" | "ultra"),
        "video_codec" => matches!(
            v,
            "auto" | "h264" | "hevc" | "av1" | "vp8" | "vp9" | "hevc_hdr" | "av1_hdr" | "hevc_10bit" | "av1_10bit"
        ),
        _ => true,
    }
}

/// Same as bin/clip `yes_`.
pub fn yes(v: &str) -> bool {
    matches!(v.to_ascii_lowercase().as_str(), "true" | "yes" | "on" | "1")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    /// apply() on UTF-8 text, result as a String.
    fn ap(text: &str, pairs: &[(&str, &str)]) -> String {
        String::from_utf8(apply(text.as_bytes(), &ch(pairs)).unwrap()).unwrap()
    }

    #[test]
    fn parse_like_bash() {
        let t = "# c\n  # indented\nsave_dir = \"/x/my#docs\"   # real\nvideo_source = 'DP-1'#x\n\
                 image_format=jpg#no-space\nvideo_codec = 'av1'\nfoo.bar = 1\njunk line\n\
                 audio =   none  \t\nbind.window =\nvideo_fps = 30";
        let m = parse(t);
        assert_eq!(m["save_dir"], "/x/my#docs");
        assert_eq!(m["video_source"], "'DP-1'#x");
        assert_eq!(m["image_format"], "jpg#no-space");
        assert_eq!(m["video_codec"], "av1");
        assert_eq!(m["audio"], "none");
        assert_eq!(m["bind.window"], "");
        assert_eq!(m["video_fps"], "30"); // last line without \n
        assert_eq!(m["notify"], "true"); // default
        assert!(!m.contains_key("foo.bar"));
    }

    #[test]
    fn quote_pair_only_one() {
        assert_eq!(parse("audio = \"\"x\"\"\n")["audio"], "\"x\"");
        assert_eq!(parse("audio = \"x'\n")["audio"], "\"x'");
        assert_eq!(parse("audio = \"\n")["audio"], "\"");
    }

    /// Only ASCII whitespace counts (bash runs load_config under LC_ALL=C):
    /// U+3000 before # is part of the value, and no edge trimming of it.
    #[test]
    fn unicode_space_is_not_space() {
        assert_eq!(parse("audio = x\u{3000}#c\n")["audio"], "x\u{3000}#c");
        assert_eq!(parse("audio =\u{3000}x\u{3000}\n")["audio"], "\u{3000}x\u{3000}");
        assert!(unrepresentable("a\u{3000}#b").is_none());
        assert_eq!(ap("audio = y\n", &[("audio", "a\u{3000}#b")]), "audio = a\u{3000}#b\n");
    }

    #[test]
    fn write_keeps_comments_order_unknown() {
        let t = "# head\n\nsave_dir = ~/Documents     # files go here\nfoo.bar = keep me\n\
                 image_format = png   # fmt\n  # tail comment\nnot a pair\n";
        let out = ap(t, &[("image_format", "webp"), ("save_dir", "/a")]);
        assert_eq!(
            out,
            "# head\n\nsave_dir = /a              # files go here\nfoo.bar = keep me\n\
             image_format = webp  # fmt\n  # tail comment\nnot a pair\n"
        );
        let m = parse(&out);
        assert_eq!(m["image_format"], "webp");
        assert_eq!(m["save_dir"], "/a");
    }

    #[test]
    fn write_long_value_keeps_one_space_before_comment() {
        let out = ap("audio = none # c\n", &[("audio", "alsa_output.pci-0000_00_1f.3")]);
        assert_eq!(out, "audio = alsa_output.pci-0000_00_1f.3 # c\n");
    }

    #[test]
    fn write_appends_missing_keys_in_default_order() {
        let out = ap("save_dir = /x", &[("bind.record", ""), ("video_fps", "30")]);
        assert_eq!(out, "save_dir = /x\nvideo_fps = 30\nbind.record =\n");
        let m = parse(&out);
        assert_eq!(m["bind.record"], "");
        assert_eq!(m["video_fps"], "30");
    }

    #[test]
    fn write_every_duplicate_and_no_space_after_eq() {
        assert_eq!(ap("audio=a\naudio = b\n", &[("audio", "none")]), "audio= none\naudio = none\n");
    }

    #[test]
    fn unchanged_text_is_identical() {
        let t = "x\r\n save_dir=~/a  #c\n\n";
        assert_eq!(apply(t.as_bytes(), &BTreeMap::new()).unwrap(), t.as_bytes());
    }

    #[test]
    fn edited_crlf_lines_keep_cr() {
        let t = "audio = x\r\nvideo_fps = 30  # c\r\nbind.all = Meta+A\r\nnotify = true\n";
        let out = ap(t, &[("audio", "none"), ("video_fps", "45"), ("bind.all", ""), ("notify", "false")]);
        assert_eq!(out, "audio = none\r\nvideo_fps = 45  # c\r\nbind.all =\r\nnotify = false\n");
        let m = parse(&out);
        assert_eq!((m["audio"].as_str(), m["video_fps"].as_str(), m["bind.all"].as_str()), ("none", "45", ""));
    }

    /// Bytes that aren't UTF-8 (a Latin-1 comment, an unknown key's value)
    /// come back exactly; only the edited line changes.
    #[test]
    fn non_utf8_bytes_round_trip() {
        let t: &[u8] = b"# caf\xe9 \xff\nfoo.bar = \xfe\xfd\nvideo_fps = 30 # \xe9t\xe9\naudio = a\xe9b\n";
        assert_eq!(apply(t, &BTreeMap::new()).unwrap(), t);
        let m = parse(t);
        assert_eq!(m["video_fps"], "30");
        assert_eq!(m["audio"], "a\u{fffd}b");
        let out = apply(t, &ch(&[("video_fps", "45")])).unwrap();
        assert_eq!(out, b"# caf\xe9 \xff\nfoo.bar = \xfe\xfd\nvideo_fps = 45 # \xe9t\xe9\naudio = a\xe9b\n");
    }

    #[test]
    fn tricky_values_round_trip() {
        for v in ["/my dir/a#b", " lead", "trail ", "\"q\"", "'q'", "#hash", "a\"b", "", "Дé x", "\"x"] {
            let out = ap("audio = x # c\n", &[("audio", v)]);
            assert_eq!(parse(&out)["audio"], v, "value {v:?} -> {out:?}");
        }
    }

    #[test]
    fn refuses_unrepresentable() {
        for v in ["a #b", "a\t#b", "line\nbreak", "tab\there"] {
            assert!(unrepresentable(v).is_some(), "{v:?}");
            assert!(apply(b"", &ch(&[("audio", v)])).is_err(), "{v:?}");
        }
        assert!(unrepresentable("a#b").is_none());
        assert!(unrepresentable("/my dir/x").is_none());
    }

    #[test]
    fn shortcuts_match_bash() {
        for ok in ["Meta+Alt+R", " ctrl + shift + print ", "Meta+F35", "Alt+PgUp", "Super+5", "Space"] {
            assert!(valid_shortcut(ok), "{ok}");
        }
        // \u{212a} is the Kelvin sign: Unicode lowercases it to k, bash under C doesn't.
        for bad in ["Meta+Foo+R", "Meta+Alt+", "Meta+R+T", "Meta+Alt", "Meta+,", "Meta+F36", "Meta+é", "", "Meta+F0", "Meta+F01", "Meta+\u{212a}", "Meta+\u{3000}R"] {
            assert!(!valid_shortcut(bad), "{bad}");
        }
    }

    #[test]
    fn valid_values_like_validate_config() {
        for (k, v) in DEFAULTS {
            assert!(valid_value(k, v), "default {k}={v}");
        }
        for (k, v) in [("video_fps", "fast"), ("video_fps", "0"), ("video_fps", "241"), ("video_fps", "+5"), ("video_fps", ""),
                       ("replay_seconds", "4"), ("replay_seconds", "Auto"), ("notify", "maybe"), ("image_format", "PNG"),
                       ("video_codec", "x264"), ("video_quality", "low")] {
            assert!(!valid_value(k, v), "{k}={v}");
        }
        for (k, v) in [("video_fps", "060"), ("notify", "OFF"), ("replay_seconds", "1800"), ("audio", "anything")] {
            assert!(valid_value(k, v), "{k}={v}");
        }
    }

    /// The bash parser itself agrees with the writer (skipped without bash).
    /// Run under a UTF-8 locale: load_config must still treat only ASCII
    /// as space.
    #[test]
    fn bash_reads_what_we_write() {
        let clip = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../bin/clip");
        if !clip.exists() || std::process::Command::new("bash").arg("-c").arg("true").status().is_err() {
            return;
        }
        let dir = crate::testutil::tempdir("bashrt");
        let cfg = dir.join("config");
        let read_back = |text: &[u8]| {
            std::fs::write(&cfg, text).unwrap();
            let out = std::process::Command::new("bash")
                .arg("-c")
                .arg(r#"source "$1"; load_config 2>/dev/null; printf %s "${CFG[audio]}""#)
                .arg("_")
                .arg(&clip)
                .env("CLIP_CONFIG", &cfg)
                .env("HOME", &*dir)
                .env("LC_ALL", "en_US.UTF-8")
                .output()
                .unwrap();
            assert!(out.status.success(), "bash failed for {:?}", String::from_utf8_lossy(text));
            out.stdout
        };
        for v in ["/my dir/a#b", " lead", "trail ", "\"q\"", "'q'", "#hash", "a\"b", "", "Дé x", "\"x", "a'b c", "a\u{3000}#b", "\u{3000}x"] {
            let text = apply(b"audio = x   # c\nfoo.bar = 1\n", &ch(&[("audio", v)])).unwrap();
            assert_eq!(String::from_utf8_lossy(&read_back(&text)), v, "bash read back {v:?}");
        }
        for line in ["audio = x\u{3000}#c\n", "audio =\u{3000}x\u{3000}\n", "audio = 'a b' # c\r\n"] {
            assert_eq!(String::from_utf8_lossy(&read_back(line.as_bytes())), parse(line)["audio"], "{line:?}");
        }
    }
}
