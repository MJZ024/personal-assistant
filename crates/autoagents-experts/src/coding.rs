//! Coding Agent — handles programming tasks.

use serde_json::Value;
use std::sync::Arc;

use autoagents_core::agent::{AgentDeriveT, AgentHooks};
use autoagents_core::tool::{ToolCallError, ToolRuntime, ToolT};

use autoagents_tool_auth::{PermissionLevel, ShellDangerLevel};

use super::sandbox::{resolve_argv, SandboxPolicy};
use super::ExpertAgent;

// ── Agent Definition ──

#[derive(Debug, Clone)]
pub struct CodingAgent {
    auth: Option<Arc<autoagents_tool_auth::ToolAuthInterceptor>>,
    working_dir: String,
    sandbox: SandboxPolicy,
}

impl CodingAgent {
    pub fn new() -> Self {
        Self {
            auth: None,
            working_dir: String::new(),
            sandbox: SandboxPolicy::Required,
        }
    }

    pub fn name() -> &'static str {
        "coding"
    }
    pub fn description() -> &'static str {
        "Expert coding agent — writes, tests, debugs code. Has shell, file, git, and code search tools."
    }
}

#[async_trait::async_trait]
impl AgentDeriveT for CodingAgent {
    type Output = String;

    fn description(&self) -> &str {
        Self::description()
    }
    fn output_schema(&self) -> Option<Value> {
        None
    }
    fn name(&self) -> &str {
        Self::name()
    }

    fn tools(&self) -> Vec<Box<dyn ToolT>> {
        vec![
            Box::new(ShellExecuteTool::new(
                self.auth.clone(),
                self.working_dir.clone(),
                self.sandbox,
            )),
            Box::new(ReadFileTool::new(self.working_dir.clone())),
            Box::new(WriteFileTool::new(
                self.auth.clone(),
                self.working_dir.clone(),
            )),
            Box::new(GitOperationTool::new(self.auth.clone())),
            Box::new(CodeSearchTool::new(self.working_dir.clone())),
        ]
    }
}

// ReActAgent requires this; all methods carry sensible defaults.
impl AgentHooks for CodingAgent {}

#[async_trait::async_trait]
impl ExpertAgent for CodingAgent {
    fn agent_type(&self) -> &'static str {
        "coding"
    }
    fn max_permission_level(&self) -> PermissionLevel {
        PermissionLevel::System
    }
    async fn init(&mut self, ctx: Arc<super::ExpertContext>) {
        self.auth = Some(ctx.auth.clone());
        self.working_dir = ctx.working_dir.clone();
        self.sandbox = ctx.sandbox;
    }
}

// ── Shell Execute Tool ──

#[derive(Debug, Clone)]
pub struct ShellExecuteTool {
    auth: Option<Arc<autoagents_tool_auth::ToolAuthInterceptor>>,
    working_dir: String,
    sandbox: SandboxPolicy,
}

impl ShellExecuteTool {
    pub fn new(
        auth: Option<Arc<autoagents_tool_auth::ToolAuthInterceptor>>,
        working_dir: String,
        sandbox: SandboxPolicy,
    ) -> Self {
        Self {
            auth,
            working_dir,
            sandbox,
        }
    }
}

impl ToolT for ShellExecuteTool {
    fn name(&self) -> &str {
        "shell_execute"
    }
    fn description(&self) -> &str {
        "Execute a shell command. Destructive commands require user confirmation."
    }
    fn args_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {"type": "string"},
                "working_dir": {"type": "string"},
                "timeout_secs": {"type": "integer"}
            },
            "required": ["command"]
        })
    }
}

#[async_trait::async_trait]
impl ToolRuntime for ShellExecuteTool {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let command = args["command"]
            .as_str()
            .ok_or(ToolCallError::RuntimeError("command required".into()))?;

        // Fail-closed danger gate: with no auth interceptor configured we
        // refuse to run anything rather than running unchecked.
        let auth = self
            .auth
            .as_ref()
            .ok_or(ToolCallError::RuntimeError("auth not configured".into()))?;
        let (level, warning) = auth.analyze_shell_command(command);
        if level == ShellDangerLevel::Unknown || level >= ShellDangerLevel::System {
            return Err(ToolCallError::RuntimeError(
                format!(
                    "Command blocked (level: {:?}): {}. User confirmation required.",
                    level,
                    warning.unwrap_or_default()
                )
                .into(),
            ));
        }

        // Confine execution to the agent's working directory under the
        // configured sandbox policy. The per-call `working_dir` arg is
        // intentionally ignored so the LLM cannot relocate execution outside
        // the sandboxed bind.
        let workdir = if self.working_dir.is_empty() {
            "/tmp".to_string()
        } else {
            self.working_dir.clone()
        };
        let home = std::env::var("HOME").ok();
        let argv = resolve_argv(self.sandbox, &workdir, home.as_deref(), command)
            .map_err(|e| ToolCallError::RuntimeError(e.into()))?;

        let output = tokio::process::Command::new(&argv[0])
            .args(&argv[1..])
            .current_dir(&workdir)
            .output()
            .await
            .map_err(|e| ToolCallError::RuntimeError(e.to_string().into()))?;

        Ok(serde_json::json!({
            "stdout": String::from_utf8_lossy(&output.stdout),
            "stderr": String::from_utf8_lossy(&output.stderr),
            "exit_code": output.status.code(),
            "success": output.status.success(),
        }))
    }
}

// ── Read File Tool ──

#[derive(Debug, Clone)]
pub struct ReadFileTool {
    working_dir: String,
}

impl ReadFileTool {
    pub fn new(working_dir: String) -> Self {
        Self { working_dir }
    }
}

impl ToolT for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }
    fn description(&self) -> &str {
        "Read the contents of a file."
    }
    fn args_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "max_lines": {"type": "integer"}
            },
            "required": ["path"]
        })
    }
}

#[async_trait::async_trait]
impl ToolRuntime for ReadFileTool {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let path = args["path"]
            .as_str()
            .ok_or(ToolCallError::RuntimeError("path required".into()))?;
        let max_lines = args["max_lines"].as_u64().unwrap_or(5000) as usize;

        // Confine reads to the allow-root and deny sensitive locations.
        // Symlink-resolved so a planted symlink can't exfiltrate a secret.
        let policy = super::path_policy::PathPolicy::for_coding(Some(&self.working_dir));
        let safe_path = policy.validate_resolved(path).map_err(|e| {
            ToolCallError::RuntimeError(format!("path '{path}' rejected: {e:?}").into())
        })?;

        let contents = tokio::fs::read_to_string(&safe_path)
            .await
            .map_err(|e| ToolCallError::RuntimeError(e.to_string().into()))?;
        let lines: Vec<&str> = contents.lines().take(max_lines).collect();

        Ok(serde_json::json!({ "path": path, "lines": lines.len(), "content": lines.join("\n") }))
    }
}

// ── Write File Tool ──

#[derive(Debug, Clone)]
pub struct WriteFileTool {
    auth: Option<Arc<autoagents_tool_auth::ToolAuthInterceptor>>,
    working_dir: String,
}

impl WriteFileTool {
    pub fn new(
        auth: Option<Arc<autoagents_tool_auth::ToolAuthInterceptor>>,
        working_dir: String,
    ) -> Self {
        Self { auth, working_dir }
    }
}

impl ToolT for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }
    fn description(&self) -> &str {
        "Write content to a file. Creates parent directories if needed."
    }
    fn args_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {"type": "string"},
                "content": {"type": "string"}
            },
            "required": ["path", "content"]
        })
    }
}

#[async_trait::async_trait]
impl ToolRuntime for WriteFileTool {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let path = args["path"]
            .as_str()
            .ok_or(ToolCallError::RuntimeError("path required".into()))?;
        let content = args["content"]
            .as_str()
            .ok_or(ToolCallError::RuntimeError("content required".into()))?;

        // Confine writes to the allow-root and deny sensitive locations.
        let policy = super::path_policy::PathPolicy::for_coding(Some(&self.working_dir));
        let safe_path = policy.validate(path).map_err(|e| {
            ToolCallError::RuntimeError(format!("path '{path}' rejected: {e:?}").into())
        })?;

        if let Some(parent) = safe_path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| ToolCallError::RuntimeError(e.to_string().into()))?;
        }

        tokio::fs::write(&safe_path, content)
            .await
            .map_err(|e| ToolCallError::RuntimeError(e.to_string().into()))?;

        Ok(serde_json::json!({ "path": path, "written": true }))
    }
}

// ── Git Operation Tool ──

#[derive(Debug, Clone)]
pub struct GitOperationTool {
    auth: Option<Arc<autoagents_tool_auth::ToolAuthInterceptor>>,
}

impl GitOperationTool {
    pub fn new(auth: Option<Arc<autoagents_tool_auth::ToolAuthInterceptor>>) -> Self {
        Self { auth }
    }
}

impl ToolT for GitOperationTool {
    fn name(&self) -> &str {
        "git_operation"
    }
    fn description(&self) -> &str {
        "Execute git: status, add, commit, push, log, diff, branch."
    }
    fn args_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "operation": {"type": "string", "enum": ["status","add","commit","push","log","diff","branch"]},
                "repo_path": {"type": "string"},
                "message": {"type": "string"}
            },
            "required": ["operation", "repo_path"]
        })
    }
}

#[async_trait::async_trait]
impl ToolRuntime for GitOperationTool {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let operation = args["operation"]
            .as_str()
            .ok_or(ToolCallError::RuntimeError("operation required".into()))?;
        let repo_path = args["repo_path"]
            .as_str()
            .ok_or(ToolCallError::RuntimeError("repo_path required".into()))?;

        // `push` exfiltrates code and mutates the remote — gate it, and fail
        // closed when no auth interceptor is configured.
        if operation == "push" {
            let auth = self
                .auth
                .as_ref()
                .ok_or(ToolCallError::RuntimeError("auth not configured".into()))?;
            let check = auth.check("coding", "git_push", PermissionLevel::System);
            if check.needs_confirmation() {
                return Err(ToolCallError::RuntimeError(
                    "git push requires user confirmation.".into(),
                ));
            }
        }

        let mut cmd = tokio::process::Command::new("git");
        cmd.arg(operation).current_dir(repo_path);

        if operation == "commit" {
            if let Some(msg) = args["message"].as_str() {
                cmd.args(["-m", msg]);
            }
        }

        let output = cmd
            .output()
            .await
            .map_err(|e| ToolCallError::RuntimeError(e.to_string().into()))?;

        Ok(serde_json::json!({
            "operation": operation,
            "stdout": String::from_utf8_lossy(&output.stdout),
            "stderr": String::from_utf8_lossy(&output.stderr),
            "success": output.status.success(),
        }))
    }
}

// ── Code Search Tool ──

#[derive(Debug, Clone)]
pub struct CodeSearchTool {
    working_dir: String,
}

impl CodeSearchTool {
    pub fn new(working_dir: String) -> Self {
        Self { working_dir }
    }
}

impl ToolT for CodeSearchTool {
    fn name(&self) -> &str {
        "code_search"
    }
    fn description(&self) -> &str {
        "Search code using grep with regex patterns."
    }
    fn args_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": {"type": "string"},
                "directory": {"type": "string"},
                "file_pattern": {"type": "string"}
            },
            "required": ["pattern", "directory"]
        })
    }
}

#[async_trait::async_trait]
impl ToolRuntime for CodeSearchTool {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let pattern = args["pattern"]
            .as_str()
            .ok_or(ToolCallError::RuntimeError("pattern required".into()))?;
        let directory = args["directory"]
            .as_str()
            .ok_or(ToolCallError::RuntimeError("directory required".into()))?;
        let file_pattern = args["file_pattern"].as_str();

        // Confine the search root the same way read_file/write_file are, so
        // the LLM cannot grep across secret-bearing directories.
        let policy = super::path_policy::PathPolicy::for_coding(Some(&self.working_dir));
        let safe_dir = policy.validate_resolved(directory).map_err(|e| {
            ToolCallError::RuntimeError(format!("directory '{directory}' rejected: {e:?}").into())
        })?;

        let mut cmd = tokio::process::Command::new("grep");
        cmd.args(["-rn", "-E", pattern]);
        cmd.arg(safe_dir.as_os_str());
        if let Some(fp) = file_pattern {
            cmd.args(["--include", fp]);
        }

        let output = cmd
            .output()
            .await
            .map_err(|e| ToolCallError::RuntimeError(e.to_string().into()))?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let matches: Vec<&str> = stdout.lines().take(100).collect();

        Ok(serde_json::json!({
            "matches": matches,
            "total": stdout.lines().count(),
            "truncated": stdout.lines().count() > 100,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use autoagents_tool_auth::{ToolAuthConfig, ToolAuthInterceptor};
    use std::sync::Arc;

    fn test_auth() -> Arc<ToolAuthInterceptor> {
        // AuditLogger opens its log path, so point it somewhere writable.
        let mut cfg = ToolAuthConfig::default();
        cfg.audit_log_path = "/tmp/pa-test-audit.log".into();
        Arc::new(ToolAuthInterceptor::new(cfg).expect("interceptor"))
    }

    #[tokio::test]
    async fn git_push_fails_closed_without_auth() {
        let tool = GitOperationTool::new(None);
        let res = tool
            .execute(serde_json::json!({"operation":"push","repo_path":"/tmp"}))
            .await;
        assert!(res.is_err(), "git push must refuse when auth is None");
    }

    #[tokio::test]
    async fn git_push_blocked_by_gate_with_auth() {
        // coding agent cap is System; push is gated at System, which the
        // (currently dead-letter) confirmation turns into a refusal.
        let tool = GitOperationTool::new(Some(test_auth()));
        let res = tool
            .execute(serde_json::json!({"operation":"push","repo_path":"/tmp"}))
            .await;
        assert!(res.is_err(), "git push must pass through the auth gate");
    }

    #[tokio::test]
    async fn code_search_rejects_secret_directory() {
        let tool = CodeSearchTool::new("/home/me/repo".into());
        let res = tool
            .execute(serde_json::json!({"pattern":"KEY","directory":"/opt/personal-assistant"}))
            .await;
        assert!(res.is_err(), "code_search must refuse the secrets dir");
    }

    #[tokio::test]
    async fn code_search_rejects_path_traversal() {
        let tool = CodeSearchTool::new("/home/me/repo".into());
        let res = tool
            .execute(serde_json::json!({"pattern":"x","directory":"../../../etc"}))
            .await;
        assert!(res.is_err(), "code_search must reject traversal outside root");
    }
}
