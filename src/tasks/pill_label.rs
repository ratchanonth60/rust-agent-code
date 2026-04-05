//! Background task pill label for the TUI status bar.
//!
//! Generates a compact string like `" 3 tasks "` displayed in the status
//! line when background tasks are running.

use super::SharedTaskRegistry;

/// Returns a short pill string for the TUI status line.
///
/// - `""` when no tasks are running
/// - `" 1 task "` for a single running task
/// - `" N tasks "` for multiple running tasks
///
/// # Examples
///
/// ```ignore
/// let pill = pill_label(&registry);
/// // pill == "" or " 1 task " or " 3 tasks "
/// ```
pub fn pill_label(registry: &SharedTaskRegistry) -> String {
    let count = match registry.lock() {
        Ok(reg) => reg.running_count(),
        Err(_) => return String::new(),
    };

    match count {
        0 => String::new(),
        1 => " 1 task ".to_string(),
        n => format!(" {} tasks ", n),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tasks::{new_shared_registry, shell};

    #[test]
    fn test_pill_empty() {
        let registry = new_shared_registry();
        assert_eq!(pill_label(&registry), "");
    }

    #[test]
    fn test_pill_singular() {
        let registry = new_shared_registry();
        shell::spawn(&registry, "sleep 60", "task 1", None).unwrap();

        assert_eq!(pill_label(&registry), " 1 task ");

        // Clean up
        let id = {
            let reg = registry.lock().unwrap();
            reg.list()[0].id().to_string()
        };
        shell::kill(&registry, &id);
    }

    #[test]
    fn test_pill_plural() {
        let registry = new_shared_registry();
        shell::spawn(&registry, "sleep 60", "task 1", None).unwrap();
        shell::spawn(&registry, "sleep 60", "task 2", None).unwrap();
        shell::spawn(&registry, "sleep 60", "task 3", None).unwrap();

        assert_eq!(pill_label(&registry), " 3 tasks ");

        // Clean up
        let ids: Vec<String> = {
            let reg = registry.lock().unwrap();
            reg.list().iter().map(|t| t.id().to_string()).collect()
        };
        for id in ids {
            shell::kill(&registry, &id);
        }
    }
}
