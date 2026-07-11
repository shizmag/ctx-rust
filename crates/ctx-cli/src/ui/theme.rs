//! Tokyo Night palette shared with ctx-render and ctx-tui.

#[derive(Clone, Copy, Debug)]
pub struct Rgb(pub u8, pub u8, pub u8);

pub const FG: Rgb = Rgb(192, 202, 245);
pub const COMMENT: Rgb = Rgb(86, 95, 137);
pub const BLUE: Rgb = Rgb(122, 162, 247);
pub const CYAN: Rgb = Rgb(125, 207, 255);
pub const GREEN: Rgb = Rgb(158, 206, 106);
pub const YELLOW: Rgb = Rgb(224, 175, 104);
pub const MAGENTA: Rgb = Rgb(187, 154, 247);
pub const RED: Rgb = Rgb(247, 118, 142);

pub fn ansi_fg(color: Rgb) -> String {
    format!("\x1b[38;2;{};{};{}m", color.0, color.1, color.2)
}

pub fn ansi_fg_bold(color: Rgb) -> String {
    format!("\x1b[1;38;2;{};{};{}m", color.0, color.1, color.2)
}

pub fn ansi_reset() -> &'static str {
    "\x1b[0m"
}

pub fn styled(text: &str, color: Rgb) -> String {
    format!("{}{}{}", ansi_fg(color), text, ansi_reset())
}

pub fn styled_bold(text: &str, color: Rgb) -> String {
    format!("{}{}{}", ansi_fg_bold(color), text, ansi_reset())
}

pub fn success(text: &str) -> String {
    styled_bold(text, GREEN)
}

pub fn error_label(text: &str) -> String {
    styled_bold(text, RED)
}

pub fn info(text: &str) -> String {
    styled(text, CYAN)
}

pub fn muted(text: &str) -> String {
    styled(text, COMMENT)
}