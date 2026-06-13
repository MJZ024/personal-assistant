//! Path confinement for file-touching expert tools.
//!
//! The coding and ops agents operate on paths supplied by the LLM, which in
//! turn are influenced by chat input. Without confinement a single message can
//! read `/etc/shadow`, the assistant's own `config.yaml` (containing
//! `app_secret` / `verification_token`), or `~/.ssh/authorized_keys`.
//!
//! [`PathPolicy`] enforces two layers:
//! 1. **Containment** — when an allow-root is configured, every requested path
//!    must resolve to a location beneath it (after lexical `..` normalisation
//!    and, for reads, symlink resolution).
//! 2. **Blocklist** — sensitive system and secret-bearing locations are denied
//!    regardless of the allow-root, as defence in depth.

use std::path::{Component, Path, PathBuf};

/// Why a path was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathError {
    /// Empty path.
    Empty,
    /// Escaped the allow-root via `..`, an absolute path, or a symlink.
    EscapeRoot,
    /// Points at a sensitive location.
    Blocked(String),
}

#[derive(Debug, Clone)]
pub struct PathPolicy {
    allow_root: Option<PathBuf>,
    blocked: Vec<PathBuf>,
}

impl PathPolicy {
    /// Build a policy from an optional allow-root and a blocklist.
    pub fn new(allow_root: Option<PathBuf>, blocked: Vec<PathBuf>) -> Self {
        let allow_root = allow_root.map(|r| lexically_normalize(&r));
        Self {
            allow_root,
            blocked,
        }
    }

    /// Coding-agent policy: confined to `working_dir` (when set), with a
    /// standard sensitive-path blocklist applied in all cases.
    pub fn for_coding(working_dir: Option<&str>) -> Self {
        let allow_root = working_dir
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(PathBuf::from);
        Self::new(allow_root, default_blocked())
    }

    /// Ops log-viewing policy: no single root (logs live under `/var/log` and
    /// custom locations), but secret-bearing locations stay blocked.
    pub fn for_logs() -> Self {
        Self::new(None, default_blocked())
    }

    /// Validate a path requested by the agent, returning the lexically
    /// normalised path to use. Does not touch the filesystem, so it is safe
    /// for write targets that do not yet exist.
    pub fn validate(&self, requested: &str) -> Result<PathBuf, PathError> {
        let requested = requested.trim();
        if requested.is_empty() {
            return Err(PathError::Empty);
        }
        let raw = Path::new(requested);

        let candidate: PathBuf = match &self.allow_root {
            Some(root) if !raw.is_absolute() => root.join(raw),
            _ => raw.to_path_buf(),
        };
        let normalized = lexically_normalize(&candidate);

        if let Some(root) = &self.allow_root {
            if !normalized.starts_with(root) {
                return Err(PathError::EscapeRoot);
            }
        }
        self.check_blocked(&normalized)?;
        Ok(normalized)
    }

    /// Like [`validate`](Self::validate), but additionally resolves symlinks
    /// (for paths that already exist) and re-checks confinement on the
    /// canonical form. Use this for **reads** so a symlink planted inside the
    /// allow-root cannot exfiltrate a secret outside it.
    pub fn validate_resolved(&self, requested: &str) -> Result<PathBuf, PathError> {
        let lexical = self.validate(requested)?;
        match std::fs::canonicalize(&lexical) {
            Ok(canon) => {
                if let Some(root) = &self.allow_root {
                    if !canon.starts_with(root) {
                        return Err(PathError::EscapeRoot);
                    }
                }
                self.check_blocked(&canon)?;
                Ok(canon)
            }
            // Path does not exist yet (e.g. a write target) — trust the
            // lexical check.
            Err(_) => Ok(lexical),
        }
    }

    fn check_blocked(&self, path: &Path) -> Result<(), PathError> {
        for b in &self.blocked {
            if path.starts_with(b) {
                return Err(PathError::Blocked(path.display().to_string()));
            }
        }
        Ok(())
    }
}

/// Lexically normalise `.` and `..` components without touching the
/// filesystem, so `..` traversal can be evaluated against an allow-root.
fn lexically_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                let pop = matches!(out.components().next_back(), Some(Component::Normal(_)));
                if pop {
                    out.pop();
                } else if !out.has_root() {
                    // Preserve leading `..` for relative paths.
                    out.push("..");
                }
                // A `..` after the root would escape — drop it (root stays).
            }
            other => out.push(other.as_os_str()),
        }
    }
    if out.as_os_str().is_empty() {
        out.push(".");
    }
    out
}

/// Sensitive locations denied to file tools regardless of allow-root.
fn default_blocked() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = [
        "/etc",
        "/root",
        "/boot",
        "/proc",
        "/sys",
        "/dev",
        "/usr",
        "/var/lib",
        "/opt/personal-assistant",
    ]
    .iter()
    .map(PathBuf::from)
    .collect();

    // Home-relative secrets, when HOME is resolvable.
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        for sub in [".ssh", ".llm", ".aws", ".gnupg", ".config"] {
            v.push(home.join(sub));
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn coding(root: &str) -> PathPolicy {
        PathPolicy::for_coding(Some(root))
    }

    #[test]
    fn relative_path_under_root_is_joined_and_allowed() {
        let p = coding("/home/me/repo").validate("src/main.rs").unwrap();
        assert_eq!(p, PathBuf::from("/home/me/repo/src/main.rs"));
    }

    #[test]
    fn dotdot_escape_is_rejected() {
        assert_eq!(
            coding("/home/me/repo")
                .validate("../../etc/shadow")
                .unwrap_err(),
            PathError::EscapeRoot,
        );
    }

    #[test]
    fn absolute_path_outside_root_is_rejected() {
        assert_eq!(
            coding("/home/me/repo").validate("/etc/shadow").unwrap_err(),
            PathError::EscapeRoot,
        );
    }

    #[test]
    fn absolute_path_inside_root_is_allowed() {
        let p = coding("/home/me/repo")
            .validate("/home/me/repo/a.txt")
            .unwrap();
        assert_eq!(p, PathBuf::from("/home/me/repo/a.txt"));
    }

    #[test]
    fn dotdot_within_root_is_allowed_and_normalized() {
        let p = coding("/home/me/repo").validate("a/../b.rs").unwrap();
        assert_eq!(p, PathBuf::from("/home/me/repo/b.rs"));
    }

    #[test]
    fn empty_path_is_rejected() {
        assert_eq!(coding("/r").validate("   ").unwrap_err(), PathError::Empty);
    }

    #[test]
    fn blocked_path_rejected_even_inside_root() {
        // If someone mis-configures the allow-root to be the app dir, the
        // blocklist must still protect the secrets it holds.
        let policy = PathPolicy::new(
            Some(PathBuf::from("/opt/personal-assistant")),
            default_blocked(),
        );
        assert!(matches!(
            policy.validate("/opt/personal-assistant/config.yaml"),
            Err(PathError::Blocked(_)),
        ));
    }

    #[test]
    fn no_root_still_blocks_sensitive_absolute_paths() {
        let policy = PathPolicy::for_coding(None);
        assert!(matches!(
            policy.validate("/etc/shadow"),
            Err(PathError::Blocked(_)),
        ));
    }

    #[test]
    fn no_root_allows_innocuous_relative_path() {
        let policy = PathPolicy::for_coding(None);
        assert!(policy.validate("notes.txt").is_ok());
    }

    #[test]
    fn logs_policy_blocks_secrets_but_allows_var_log() {
        let policy = PathPolicy::for_logs();
        assert!(policy.validate("/var/log/syslog").is_ok());
        assert!(matches!(
            policy.validate("/opt/personal-assistant/config.yaml"),
            Err(PathError::Blocked(_)),
        ));
    }

    #[test]
    fn lexically_normalize_strips_curdir_and_parentdir() {
        assert_eq!(
            lexically_normalize(Path::new("/a/./b/../c")),
            PathBuf::from("/a/c")
        );
        assert_eq!(
            lexically_normalize(Path::new("/a/b/../../c")),
            PathBuf::from("/c")
        );
    }
}
