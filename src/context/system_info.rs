use std::path::Path;

/// Get system information for the context prompt.
pub fn get_system_info(cwd: &Path) -> String {
    let mut parts = Vec::new();

    parts.push(format!("Working directory: {}", cwd.display()));
    parts.push(format!("OS: {}", std::env::consts::OS));
    parts.push(format!("Arch: {}", std::env::consts::ARCH));

    if let Ok(shell) = std::env::var("SHELL") {
        parts.push(format!("Shell: {}", shell));
    }

    // Current date
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| {
            let secs = d.as_secs();
            // Simple date formatting without chrono
            let days = secs / 86400;
            // Approximate year/month/day (good enough for context)
            let year = 1970 + (days / 365);
            format!("Date: ~{}", year)
        })
        .unwrap_or_default();
    if !now.is_empty() {
        parts.push(now);
    }

    parts.join("\n")
}
