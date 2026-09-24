const fn style(s: &str) -> &str {
    if cfg!(test) { "" } else { s }
}

pub const MUTED: &str = style("\x1b[30m");
pub const RESET: &str = style("\x1b[0m");
pub const HIGHLIGHT: &str = style("\x1b[40m");
