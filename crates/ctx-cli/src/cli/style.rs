use clap::builder::styling::{AnsiColor, Color, Effects, Style, Styles};

const fn color(c: AnsiColor) -> Style {
    Style::new().fg_color(Some(Color::Ansi(c)))
}

const fn bold_color(c: AnsiColor) -> Style {
    color(c).effects(Effects::BOLD)
}

/// Tokyo Night–inspired clap help styling (nearest ANSI palette).
pub const CLAP_STYLING: Styles = Styles::styled()
    .header(bold_color(AnsiColor::BrightBlue))
    .usage(bold_color(AnsiColor::BrightCyan))
    .literal(color(AnsiColor::Green))
    .placeholder(color(AnsiColor::Yellow))
    .context(color(AnsiColor::BrightBlack))
    .context_value(color(AnsiColor::White))
    .valid(color(AnsiColor::Green))
    .invalid(color(AnsiColor::Red))
    .error(bold_color(AnsiColor::Red));

pub const HELP_TEMPLATE: &str = "\
{before-help}{name} {version}
{about-with-newline}
{usage-heading} {usage}

{all-args}{after-help}";