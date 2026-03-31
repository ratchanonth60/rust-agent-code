pub mod app;

use anyhow::Result;
use crossterm::{
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::io::{stdout, Stdout};

/// Setup the terminal for a TUI application
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

/// Restore the terminal back to its original state
pub fn restore_terminal() -> Result<()> {
    disable_raw_mode()?;
    stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}
