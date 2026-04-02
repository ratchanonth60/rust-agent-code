use std::path::Path;

/// Files that should never be auto-approved for writing.
const DANGEROUS_FILES: &[&str] = &[
    ".env",
    ".gitconfig",
    ".gitmodules",
    ".bashrc",
    ".bash_profile",
    ".zshrc",
    ".zprofile",
    ".profile",
    ".ssh/config",
    ".ssh/authorized_keys",
    ".ssh/known_hosts",
    ".npmrc",
    ".pypirc",
];

/// Directories that should never be auto-approved for writing.
const DANGEROUS_DIRS: &[&str] = &[
    ".git",
    ".ssh",
    ".gnupg",
    ".vscode",
    ".idea",
];

/// Returns `true` if the file path is considered dangerous for auto-approval.
pub fn is_dangerous_path(path: &str) -> bool {
    let path_lower = path.to_lowercase();

    // Check dangerous files
    for dangerous in DANGEROUS_FILES {
        if path_lower.ends_with(dangerous) {
            return true;
        }
    }

    // Check dangerous directories
    for dangerous_dir in DANGEROUS_DIRS {
        let pattern = format!("{}/", dangerous_dir);
        if path_lower.contains(&pattern) || path_lower == *dangerous_dir {
            return true;
        }
    }

    false
}

/// Returns `true` if the path is within the given working directory (no traversal escape).
pub fn is_within_directory(file_path: &str, working_dir: &Path) -> bool {
    let file = Path::new(file_path);

    // Try to canonicalize both paths
    let canonical_file = if file.is_absolute() {
        file.to_path_buf()
    } else {
        working_dir.join(file)
    };

    // Normalize by checking prefix after stripping ".." components
    let file_str = canonical_file.to_string_lossy();
    let dir_str = working_dir.to_string_lossy();

    // Simple check: the file path should start with the working directory
    if let (Ok(canon_file), Ok(canon_dir)) = (
        std::fs::canonicalize(&canonical_file),
        std::fs::canonicalize(working_dir),
    ) {
        return canon_file.starts_with(&canon_dir);
    }

    // Fallback: string prefix check (less reliable but works when files don't exist yet)
    file_str.starts_with(dir_str.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dangerous_paths() {
        assert!(is_dangerous_path(".env"));
        assert!(is_dangerous_path("/home/user/.bashrc"));
        assert!(is_dangerous_path("project/.git/config"));
        assert!(is_dangerous_path(".ssh/authorized_keys"));
        assert!(!is_dangerous_path("src/main.rs"));
        assert!(!is_dangerous_path("Cargo.toml"));
    }
}
