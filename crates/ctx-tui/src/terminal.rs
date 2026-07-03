use crate::app::TuiApp;
use crate::events::run_app;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;
use std::path::{Path, PathBuf};

pub(crate) fn open_file<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    path: &Path,
    is_text: bool,
) -> Result<(), crate::error::TuiError> {
    if is_text {
        // Suspend TUI
        disable_raw_mode()?;
        execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture)?;

        let editor = std::env::var("EDITOR").unwrap_or_else(|_| "nvim".to_string());

        let mut child = std::process::Command::new(editor).arg(path).spawn()?;

        child.wait()?;

        // Restore TUI
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
        terminal.clear()?;
    } else {
        // Open using default system app (background)
        #[cfg(target_os = "macos")]
        std::process::Command::new("open").arg(path).spawn()?;
        #[cfg(target_os = "windows")]
        std::process::Command::new("cmd")
            .args(["/C", "start"])
            .arg(path)
            .spawn()?;
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        std::process::Command::new("xdg-open").arg(path).spawn()?;
    }
    Ok(())
}

struct TerminalGuard;

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            LeaveAlternateScreen,
            DisableMouseCapture,
            crossterm::cursor::Show
        );
    }
}

pub fn run_interactive(path: PathBuf) -> Result<(), crate::error::TuiError> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    let _guard = TerminalGuard;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = TuiApp::new(path)?;

    run_app(&mut terminal, &mut app)?;

    Ok(())
}

pub fn run_default_interactive_menu() -> Result<(), crate::error::TuiError> {
    run_interactive(PathBuf::from("."))
}
