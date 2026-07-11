use std::io::IsTerminal;

pub fn use_color() -> bool {
    std::env::var_os("NO_COLOR").is_none()
}

pub fn progress_enabled() -> bool {
    use_color()
        && std::io::stderr().is_terminal()
        && std::env::var("CTX_NO_PROGRESS").is_err()
}