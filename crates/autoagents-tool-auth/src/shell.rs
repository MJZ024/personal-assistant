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
        let blacklist: Result<Vec<_>, _> = blacklist_patterns
            .iter()
            .map(|p| Regex::new(p))
            .collect();
        let whitelist: Result<Vec<_>, _> = whitelist_patterns
            .iter()
            .map(|p| Regex::new(p))
            .collect();

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

    // Destructive commands
    let destructive = [
        "rm", "mkfs", "dd", "fdisk", "parted", "shred", "wipe",
    ];
    if destructive.contains(&base.as_str()) {
        return ShellDangerLevel::Destructive;
    }

    // System commands
    let system = [
        "apt", "apt-get", "yum", "dnf", "pacman", "pip", "pip3", "npm",
        "systemctl", "service", "chkconfig", "reboot", "shutdown", "halt",
        "mount", "umount", "chown", "chmod", "useradd", "usermod", "passwd",
        "iptables", "firewall-cmd", "ufw", "docker",
    ];
    if system.contains(&base.as_str()) {
        return ShellDangerLevel::System;
    }

    // Write commands
    let write = [
        "cp", "mv", "touch", "mkdir", "rmdir", "tee", "ln",
        "git", "tar", "gzip", "zip", "unzip",
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
        assert_eq!(analyzer.analyze("echo hello > file.txt").0, ShellDangerLevel::Write);
    }

    #[test]
    fn test_system_commands() {
        let analyzer = ShellAnalyzer::new(vec![], vec![]).unwrap();
        assert_eq!(analyzer.analyze("apt install vim").0, ShellDangerLevel::System);
        assert_eq!(analyzer.analyze("systemctl restart nginx").0, ShellDangerLevel::System);
    }

    #[test]
    fn test_destructive_commands() {
        let analyzer = ShellAnalyzer::new(vec![], vec![]).unwrap();
        assert_eq!(analyzer.analyze("rm -rf /tmp/test").0, ShellDangerLevel::Destructive);
    }

    #[test]
    fn test_blacklist_pattern() {
        let analyzer = ShellAnalyzer::new(
            vec![r"rm\s+(-[rRf]+\s+)*/".into()],
            vec![],
        )
        .unwrap();
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
}
