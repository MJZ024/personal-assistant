//! Bubblewrap (`bwrap`) confinement for the coding agent's shell tool.
//!
//! The danger analyzer ([`autoagents_tool_auth::ShellAnalyzer`]) is a
//! denylist and therefore best-effort: a command it lets through can still
//! read a secret or phone home. The sandbox is the hard backstop — it makes
//! those two worst-case chains physically impossible regardless of what the
//! analyzer classifies:
//!
//! - `--unshare-net` ⟹ no network ⟹ `curl`/`wget` exfiltration fails.
//! - read-only root, only the working directory is writable.
//! - `--tmpfs /opt/personal-assistant` ⟹ the `.env` / `config.yaml` secrets
//!   are masked out, even though `/` is bound read-only.
//! - `--tmpfs $HOME` ⟹ `~/.ssh`, `~/.aws`, … are masked (when HOME is known).
//!
//! The argv is built by a pure function so the confinement policy is fully
//! unit-testable without `bwrap` installed (e.g. on the macOS dev machine).

use std::path::{Component, Path};

/// Absolute path of the directory whose contents (config/secrets) must never
/// be visible inside the sandbox, regardless of the working directory.
const SECRET_DIR: &str = "/opt/personal-assistant";

/// Build the full `bwrap` argv for running `inner` (e.g. `["sh","-c",cmd]`)
/// confined to `working_dir`.
///
/// `home` is the host `$HOME` to mask, when known. If it equals or sits
/// beneath [`SECRET_DIR`] it is not double-masked.
pub fn build_sandbox_argv(working_dir: &str, home: Option<&str>, inner: &[String]) -> Vec<String> {
    let mut argv: Vec<String> = vec![
        "bwrap".into(),
        "--die-with-parent".into(),
        "--unshare-net".into(),
        // Read-only view of the whole root. Specific paths are overridden
        // below (dev/proc/tmpfs/bind) — later operations win in bwrap.
        "--ro-bind".into(),
        "/".into(),
        "/".into(),
        "--dev".into(),
        "/dev".into(),
        "--proc".into(),
        "/proc".into(),
        "--tmpfs".into(),
        "/tmp".into(),
        // Mask the app's secrets (.env, config.yaml) even under ro-bind /.
        "--tmpfs".into(),
        SECRET_DIR.into(),
    ];

    // Mask HOME (covers ~/.ssh, ~/.aws, ~/.gnupg, …) unless it is already
    // covered by the SECRET_DIR mask, to avoid a redundant entry.
    if let Some(h) = home {
        if !h.is_empty() && !is_within(SECRET_DIR, h) {
            argv.push("--tmpfs".into());
            argv.push(h.into());
        }
    }

    // Re-expose the working directory as the sole writable location. This
    // MUST come after any --tmpfs that covers it (e.g. HOME) so the bind wins.
    argv.push("--bind".into());
    argv.push(working_dir.into());
    argv.push(working_dir.into());

    argv.push("--".into());
    argv.extend(inner.iter().cloned());
    argv
}

/// Whether a usable `bwrap` binary is on `$PATH`.
pub fn bwrap_available() -> bool {
    std::process::Command::new("bwrap")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// True if `child` is the same as, or lives beneath, `parent` (lexical).
fn is_within(parent: &str, child: &str) -> bool {
    if parent == child {
        return true;
    }
    let parent = Path::new(parent);
    let child = Path::new(child);
    child.starts_with(parent) && matches!(parent.components().next(), Some(Component::RootDir))
}

/// How strictly the coding agent's shell must be confined.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxPolicy {
    /// Refuse to run shell commands unless bwrap is available. Use on the
    /// production server, where bubblewrap is installed and the threat model
    /// includes prompt injection.
    Required,
    /// Use bwrap when available; otherwise fall back to an unsandboxed shell
    /// (logged). Reasonable for a trusted local REPL on a dev machine.
    Auto,
    /// Never sandbox — shell commands run directly. Only for fully trusted
    /// contexts where the caller accepts full responsibility.
    Off,
}

/// Pure core of [`resolve_argv`]: decides the spawn argv given an explicit
/// `available` flag so the policy logic is unit-testable without bwrap.
fn decide(
    policy: SandboxPolicy,
    available: bool,
    working_dir: &str,
    home: Option<&str>,
    command: &str,
) -> Result<Vec<String>, &'static str> {
    let inner: Vec<String> = vec!["sh".into(), "-c".into(), command.into()];
    match policy {
        SandboxPolicy::Off => Ok(inner),
        SandboxPolicy::Auto if available => {
            Ok(build_sandbox_argv(working_dir, home, &inner))
        }
        SandboxPolicy::Auto => Ok(inner),
        SandboxPolicy::Required if available => {
            Ok(build_sandbox_argv(working_dir, home, &inner))
        }
        SandboxPolicy::Required => Err("sandbox required but bwrap is not installed"),
    }
}

/// Resolve the argv to run `command` under `policy`, probing the host for
/// bwrap. On `Auto` without bwrap, logs a warning and runs unsandboxed.
pub fn resolve_argv(
    policy: SandboxPolicy,
    working_dir: &str,
    home: Option<&str>,
    command: &str,
) -> Result<Vec<String>, &'static str> {
    let available = bwrap_available();
    if matches!(policy, SandboxPolicy::Auto) && !available {
        log::warn!("bwrap unavailable; running shell unsandboxed (policy=Auto)");
    }
    decide(policy, available, working_dir, home, command)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inner() -> Vec<String> {
        vec!["sh".into(), "-c".into(), "ls -la".into()]
    }

    /// Find the value following a `flag` token, if present.
    fn value_after<'a>(argv: &'a [String], flag: &str) -> Option<&'a str> {
        let mut it = argv.iter();
        while let Some(a) = it.next() {
            if a == flag {
                return it.next().map(String::as_str);
            }
        }
        None
    }

    fn index_of(argv: &[String], flag: &str) -> Option<usize> {
        argv.iter().position(|a| a == flag)
    }

    #[test]
    fn starts_with_bwrap_and_unshares_net() {
        let argv = build_sandbox_argv("/home/me/repo", None, &inner());
        assert_eq!(argv.first().map(String::as_str), Some("bwrap"));
        assert!(argv.iter().any(|a| a == "--unshare-net"));
        assert!(argv.iter().any(|a| a == "--die-with-parent"));
    }

    #[test]
    fn root_is_read_only() {
        let argv = build_sandbox_argv("/home/me/repo", None, &inner());
        assert_eq!(value_after(&argv, "--ro-bind"), Some("/"));
        // The value after the *first* "/" bind is also "/" (src then dst).
        let root_idx = index_of(&argv, "--ro-bind").unwrap();
        assert_eq!(argv[root_idx + 1], "/");
        assert_eq!(argv[root_idx + 2], "/");
    }

    #[test]
    fn working_dir_is_the_only_writable_path() {
        let argv = build_sandbox_argv("/home/me/repo", None, &inner());
        assert_eq!(value_after(&argv, "--bind"), Some("/home/me/repo"));
        let bind_idx = index_of(&argv, "--bind").unwrap();
        assert_eq!(argv[bind_idx + 1], "/home/me/repo");
        assert_eq!(argv[bind_idx + 2], "/home/me/repo");
    }

    #[test]
    fn secret_dir_is_masked() {
        let argv = build_sandbox_argv("/home/me/repo", None, &inner());
        // A --tmpfs over SECRET_DIR hides its contents even under ro-bind /.
        let tmpfs_vals: Vec<&str> = argv
            .windows(2)
            .filter(|w| w[0] == "--tmpfs")
            .map(|w| w[1].as_str())
            .collect();
        assert!(
            tmpfs_vals.contains(&SECRET_DIR),
            "expected {SECRET_DIR} masked, got {tmpfs_vals:?}"
        );
    }

    #[test]
    fn home_is_masked_when_known_and_outside_secret_dir() {
        let argv = build_sandbox_argv("/home/me/repo", Some("/home/me"), &inner());
        let tmpfs_vals: Vec<&str> = argv
            .windows(2)
            .filter(|w| w[0] == "--tmpfs")
            .map(|w| w[1].as_str())
            .collect();
        assert!(tmpfs_vals.contains(&"/home/me"), "home should be masked: {tmpfs_vals:?}");
    }

    #[test]
    fn home_not_double_masked_when_under_secret_dir() {
        // HOME == SECRET_DIR: masking SECRET_DIR already covers it; no
        // duplicate --tmpfs for the same path.
        let argv = build_sandbox_argv("/srv/app", Some(SECRET_DIR), &inner());
        let count = argv
            .windows(2)
            .filter(|w| w[0] == "--tmpfs" && w[1] == SECRET_DIR)
            .count();
        assert_eq!(count, 1, "SECRET_DIR masked exactly once");
    }

    #[test]
    fn tmp_is_ephemeral() {
        let argv = build_sandbox_argv("/home/me/repo", None, &inner());
        assert_eq!(value_after(&argv, "--tmpfs"), Some("/tmp"));
    }

    #[test]
    fn working_dir_re_exposed_after_home_masked() {
        // When the working dir lives under HOME, the --bind must come AFTER
        // the --tmpfs HOME so the bind wins and the dir is visible+writable.
        let argv = build_sandbox_argv("/home/me/repo", Some("/home/me"), &inner());
        let mut home_tmpfs_idx = None;
        for (i, w) in argv.windows(2).enumerate() {
            if w[0] == "--tmpfs" && w[1] == "/home/me" {
                home_tmpfs_idx = Some(i);
            }
        }
        let home_tmpfs_idx = home_tmpfs_idx.expect("home tmpfs present");
        let bind_idx = index_of(&argv, "--bind").unwrap();
        assert!(
            bind_idx > home_tmpfs_idx,
            "working dir bind must come after home mask (bind={} < home_tmpfs={})",
            bind_idx,
            home_tmpfs_idx
        );
    }

    #[test]
    fn inner_command_is_appended_after_separator() {
        let inner = inner();
        let argv = build_sandbox_argv("/home/me/repo", None, &inner);
        let sep = index_of(&argv, "--").expect("separator present");
        assert_eq!(&argv[sep + 1..], inner.as_slice());
    }

    #[test]
    fn is_within_lexical() {
        assert!(is_within("/opt/personal-assistant", "/opt/personal-assistant"));
        assert!(is_within("/opt/personal-assistant", "/opt/personal-assistant/sub"));
        assert!(!is_within("/opt/personal-assistant", "/opt/personal-assistant-evil"));
        assert!(!is_within("/home", "/opt"));
    }

    // ── decide(): policy × availability matrix (pure, no bwrap needed) ──

    fn is_sandboxed(argv: &[String]) -> bool {
        argv.first().map(String::as_str) == Some("bwrap")
    }

    #[test]
    fn off_policy_never_sandboxes() {
        for avail in [true, false] {
            let argv = decide(SandboxPolicy::Off, avail, "/w", None, "ls").unwrap();
            assert!(!is_sandboxed(&argv), "Off must not sandbox (avail={avail})");
            assert_eq!(argv, ["sh", "-c", "ls"]);
        }
    }

    #[test]
    fn auto_sandboxes_when_available_falls_back_when_not() {
        let with_bwrap = decide(SandboxPolicy::Auto, true, "/w", None, "ls").unwrap();
        assert!(is_sandboxed(&with_bwrap));
        let without = decide(SandboxPolicy::Auto, false, "/w", None, "ls").unwrap();
        assert!(!is_sandboxed(&without), "Auto must fall back when bwrap absent");
    }

    #[test]
    fn required_sandboxes_when_available_refuses_when_not() {
        let with_bwrap = decide(SandboxPolicy::Required, true, "/w", None, "ls").unwrap();
        assert!(is_sandboxed(&with_bwrap));
        let refused = decide(SandboxPolicy::Required, false, "/w", None, "ls");
        assert!(refused.is_err(), "Required without bwrap must refuse (fail-closed)");
    }

    #[test]
    fn sandboxed_argv_carries_command_and_working_dir() {
        let argv = decide(SandboxPolicy::Required, true, "/home/me/repo", None, "make test").unwrap();
        // inner command is the tail after "--"
        let sep = argv.iter().position(|a| a == "--").unwrap();
        assert_eq!(&argv[sep + 1..], &["sh", "-c", "make test"]);
        // working dir bound
        let bind_idx = argv.iter().position(|a| a == "--bind").unwrap();
        assert_eq!(argv[bind_idx + 1], "/home/me/repo");
    }
}
