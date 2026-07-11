use super::terminal;
use indicatif::{ProgressBar, ProgressStyle};
use std::time::Duration;

/// RAII spinner on stderr; no-op when progress is disabled.
pub struct ProgressGuard {
    bar: Option<ProgressBar>,
}

impl ProgressGuard {
    pub fn new(message: &str) -> Self {
        if !terminal::progress_enabled() {
            eprintln!("{message}");
            return Self { bar: None };
        }

        let bar = ProgressBar::new_spinner();
        bar.enable_steady_tick(Duration::from_millis(80));
        let style = ProgressStyle::with_template("{spinner:.green} {msg} [{elapsed_precise}]")
            .unwrap_or_else(|_| ProgressStyle::default_spinner());
        bar.set_style(style);
        bar.set_message(message.to_string());
        Self { bar: Some(bar) }
    }

    pub fn set_message(&self, message: &str) {
        if let Some(bar) = &self.bar {
            bar.set_message(message.to_string());
        }
    }
}

impl Drop for ProgressGuard {
    fn drop(&mut self) {
        if let Some(bar) = self.bar.take() {
            bar.finish_and_clear();
        }
    }
}