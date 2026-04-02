//! Terminal UI setup and teardown (crossterm + ratatui).
//!
//! Call [`setup_terminal`] to enter the alternate screen and raw mode,
//! and [`restore_terminal`] (or the panic hook) to leave it cleanly.

pub mod app;

use anyhow::Result;
use crossterm::{
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io::{stdout, Stdout};

/// Enters the alternate screen and enables raw mode.
///
/// Installs a panic hook that restores the terminal before the
/// default handler runs, preventing garbled output on crash.
pub fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    // Save original panic hook
    let original_hook = std::panic::take_hook();

    // Hook to ensure we restore the terminal if a panic occurs
    std::panic::set_hook(Box::new(move |panic| {
        let _ = restore_terminal();
        original_hook(panic);
    }));

    enable_raw_mode()?;
    stdout().execute(EnterAlternateScreen)?;
    
    let backend = CrosstermBackend::new(stdout());
    let terminal = Terminal::new(backend)?;
    
    Ok(terminal)
}

/// Leaves the alternate screen and disables raw mode.
pub fn restore_terminal() -> Result<()> {
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}
