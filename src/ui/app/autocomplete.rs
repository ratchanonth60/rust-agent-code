use std::collections::HashSet;

use super::{
    App, AutocompleteItem, AutocompleteKind, AUTOCOMPLETE_MAX_ITEMS, FILE_SCAN_MAX,
    FILE_SUGGEST_DEBOUNCE,
};

impl App {
    /// Determine whether autocomplete UI should be shown.
    ///
    /// # Returns
    ///
    /// `true` when no blocking prompt is active and there are candidates to render.
    pub(super) fn is_autocomplete_visible(&self) -> bool {
        self.pending_permission.is_none()
            && self.pending_question.is_none()
            && !self.autocomplete_items.is_empty()
    }

    /// Mark autocomplete cache as stale after input state changes.
    ///
    /// # Behavior
    ///
    /// Also clears the dismissal token when the input differs from the string that
    /// previously dismissed suggestions.
    pub(super) fn mark_autocomplete_dirty(&mut self) {
        self.autocomplete_dirty = true;
        if self
            .autocomplete_dismissed_input
            .as_ref()
            .map(|s| s != &self.input)
            .unwrap_or(false)
        {
            self.autocomplete_dismissed_input = None;
        }
    }

    /// Clear autocomplete items and reset all related selection/dismissal state.
    pub(super) fn clear_autocomplete(&mut self) {
        self.autocomplete_items.clear();
        self.autocomplete_selected = 0;
        self.autocomplete_dismissed_input = None;
        self.autocomplete_dirty = false;
    }

    /// Refresh autocomplete candidates when marked dirty.
    ///
    /// # Behavior
    ///
    /// Stops early when autocomplete is blocked (permission/question prompt) or
    /// still debounced for file lookup, otherwise rebuilds command/file candidates.
    pub(super) fn maybe_refresh_autocomplete(&mut self) {
        if !self.autocomplete_dirty {
            return;
        }

        if self.pending_permission.is_some() || self.pending_question.is_some() {
            self.autocomplete_items.clear();
            self.autocomplete_selected = 0;
            self.autocomplete_dirty = false;
            return;
        }

        if self
            .autocomplete_dismissed_input
            .as_ref()
            .map(|s| s == &self.input)
            .unwrap_or(false)
        {
            self.autocomplete_items.clear();
            self.autocomplete_selected = 0;
            self.autocomplete_dirty = false;
            return;
        }

        let mut next_items = self.build_command_suggestions();
        if next_items.is_empty() {
            if let Some(query) = self.extract_file_query() {
                let is_debounced = self
                    .last_file_refresh
                    .map(|t| t.elapsed() < FILE_SUGGEST_DEBOUNCE)
                    .unwrap_or(false);
                if is_debounced {
                    return;
                }
                next_items = self.build_file_suggestions(&query);
                self.last_file_refresh = Some(std::time::Instant::now());
            }
        }

        self.autocomplete_items = next_items;
        if self.autocomplete_selected >= self.autocomplete_items.len() {
            self.autocomplete_selected = 0;
        }
        self.autocomplete_dirty = false;
    }

    /// Build slash-command suggestions from command names and aliases.
    ///
    /// # Returns
    ///
    /// A sorted list of command candidates capped by `AUTOCOMPLETE_MAX_ITEMS`.
    fn build_command_suggestions(&self) -> Vec<AutocompleteItem> {
        let token = self.input.split_whitespace().next().unwrap_or("");
        if !token.starts_with('/') {
            return Vec::new();
        }

        let needle = token.trim_start_matches('/').to_lowercase();
        let mut seen = HashSet::new();
        let mut scored: Vec<(usize, String)> = Vec::new();

        for command in self.command_registry.list() {
            let mut names = vec![command.name().to_string()];
            names.extend(command.aliases().into_iter().map(|alias| alias.to_string()));
            for name in names {
                if !seen.insert(name.clone()) {
                    continue;
                }
                let lower = name.to_lowercase();
                if needle.is_empty() || lower.contains(&needle) {
                    let score = if lower.starts_with(&needle) { 0 } else { 1 };
                    scored.push((score, name));
                }
            }
        }

        scored.sort();
        scored
            .into_iter()
            .take(AUTOCOMPLETE_MAX_ITEMS)
            .map(|(_, name)| AutocompleteItem {
                display: format!("/{}", name),
                insert: format!("/{} ", name),
                kind: AutocompleteKind::Command,
            })
            .collect()
    }

    /// Extract a lowercase file-query from the last `@token` in input.
    ///
    /// # Returns
    ///
    /// `Some(query)` when the trailing token begins with `@`, else `None`.
    fn extract_file_query(&self) -> Option<String> {
        let token = self.input.split_whitespace().last()?;
        if token.starts_with('@') {
            Some(token.trim_start_matches('@').to_lowercase())
        } else {
            None
        }
    }

    /// Build file suggestions from cached workspace paths.
    ///
    /// # Parameters
    ///
    /// - `query`: Lowercase search fragment extracted from user input.
    ///
    /// # Returns
    ///
    /// A sorted file candidate list capped by `AUTOCOMPLETE_MAX_ITEMS`.
    fn build_file_suggestions(&mut self, query: &str) -> Vec<AutocompleteItem> {
        if self.file_index.is_none() {
            self.file_index = Some(self.scan_workspace_files());
        }

        let files = self.file_index.clone().unwrap_or_default();
        let mut scored: Vec<(usize, String)> = files
            .into_iter()
            .filter_map(|path| {
                let lower = path.to_lowercase();
                if query.is_empty() || lower.contains(query) {
                    let score = if lower.starts_with(query) { 0 } else { 1 };
                    Some((score, path))
                } else {
                    None
                }
            })
            .collect();

        scored.sort();
        scored
            .into_iter()
            .take(AUTOCOMPLETE_MAX_ITEMS)
            .map(|(_, path)| AutocompleteItem {
                display: format!("@{}", path),
                insert: format!("@{}", path),
                kind: AutocompleteKind::File,
            })
            .collect()
    }

    /// Scan the workspace recursively and collect relative file paths.
    ///
    /// # Returns
    ///
    /// A list of relative paths, skipping common heavy directories like `.git`.
    fn scan_workspace_files(&self) -> Vec<String> {
        /// Recursively walk a directory tree and append relative file paths.
        ///
        /// # Parameters
        ///
        /// - `dir`: Current directory being traversed.
        /// - `root`: Workspace root used to compute relative path output.
        /// - `out`: Mutable output list receiving discovered file paths.
        fn walk(dir: &std::path::Path, root: &std::path::Path, out: &mut Vec<String>) {
            if out.len() >= FILE_SCAN_MAX {
                return;
            }
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };

            for entry in entries.flatten() {
                if out.len() >= FILE_SCAN_MAX {
                    return;
                }
                let path = entry.path();
                let name = entry.file_name();
                if let Some(name_str) = name.to_str() {
                    if matches!(name_str, ".git" | "target" | "node_modules") {
                        continue;
                    }
                }
                if path.is_dir() {
                    walk(&path, root, out);
                } else if let Ok(rel) = path.strip_prefix(root) {
                    out.push(rel.to_string_lossy().to_string());
                }
            }
        }

        let root = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let mut out = Vec::new();
        walk(&root, &root, &mut out);
        out
    }

    /// Apply the currently selected autocomplete candidate to `self.input`.
    ///
    /// # Behavior
    ///
    /// Command candidates replace the first token, while file candidates replace
    /// the trailing token. After insertion, the dropdown is reset.
    pub(super) fn accept_autocomplete(&mut self) {
        if self.autocomplete_items.is_empty() {
            return;
        }

        let idx = self
            .autocomplete_selected
            .min(self.autocomplete_items.len().saturating_sub(1));
        let item = self.autocomplete_items[idx].clone();

        match item.kind {
            AutocompleteKind::Command => {
                let mut parts = self.input.splitn(2, ' ');
                let _ = parts.next();
                if let Some(rest) = parts.next() {
                    self.input = format!("{}{}", item.insert, rest.trim_start());
                } else {
                    self.input = item.insert;
                }
            }
            AutocompleteKind::File => {
                self.input = Self::replace_last_token(&self.input, &item.insert);
            }
        }

        self.autocomplete_items.clear();
        self.autocomplete_selected = 0;
        self.autocomplete_dismissed_input = None;
        self.autocomplete_dirty = true;
    }

    /// Dismiss autocomplete for the current input snapshot.
    ///
    /// # Behavior
    ///
    /// Records the current input to prevent immediate reappearance until the user
    /// changes the text.
    pub(super) fn dismiss_autocomplete(&mut self) {
        self.autocomplete_dismissed_input = Some(self.input.clone());
        self.autocomplete_items.clear();
        self.autocomplete_selected = 0;
        self.autocomplete_dirty = false;
    }

    /// Move selection to the previous autocomplete candidate with wrap-around.
    pub(super) fn autocomplete_previous(&mut self) {
        if self.autocomplete_items.is_empty() {
            return;
        }
        if self.autocomplete_selected == 0 {
            self.autocomplete_selected = self.autocomplete_items.len() - 1;
        } else {
            self.autocomplete_selected -= 1;
        }
    }

    /// Move selection to the next autocomplete candidate with wrap-around.
    pub(super) fn autocomplete_next(&mut self) {
        if self.autocomplete_items.is_empty() {
            return;
        }
        self.autocomplete_selected =
            (self.autocomplete_selected + 1) % self.autocomplete_items.len();
    }

    /// Replace the last whitespace-delimited token with a replacement string.
    ///
    /// # Parameters
    ///
    /// - `input`: Full input buffer to transform.
    /// - `replacement`: New token value to put at the end.
    ///
    /// # Returns
    ///
    /// A new string where the final token is replaced, preserving prefix spacing.
    fn replace_last_token(input: &str, replacement: &str) -> String {
        if let Some((idx, _)) = input
            .char_indices()
            .rev()
            .find(|(_, ch)| ch.is_whitespace())
        {
            let prefix = &input[..=idx];
            format!("{}{}", prefix, replacement)
        } else {
            replacement.to_string()
        }
    }
}
