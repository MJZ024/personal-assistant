//! Operations Agent — server management, monitoring, maintenance.

use serde_json::Value;
use std::sync::Arc;

use autoagents_core::agent::{AgentDeriveT, AgentHooks};
use autoagents_core::tool::{ToolCallError, ToolRuntime, ToolT};

use autoagents_tool_auth::PermissionLevel;

use super::ExpertAgent;

// ── Agent Definition ──

#[derive(Debug, Clone)]
pub struct OpsAgent {
    auth: Option<Arc<autoagents_tool_auth::ToolAuthInterceptor>>,
}

impl OpsAgent {
    pub fn new() -> Self {
        Self { auth: None }
    }
    pub fn name() -> &'static str {
        "ops"
    }
    pub fn description() -> &'static str {
        "Expert ops agent — server monitoring, service control, log analysis, cron tasks, process management."
    }
}

#[async_trait::async_trait]
impl AgentDeriveT for OpsAgent {
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
            Box::new(SystemStatusTool),
            Box::new(ServiceControlTool::new(self.auth.clone())),
            Box::new(LogViewTool),
            Box::new(CronTaskTool::new(self.auth.clone())),
            Box::new(ProcessManageTool::new(self.auth.clone())),
        ]
    }
}

// ReActAgent requires this; all methods carry sensible defaults.
impl AgentHooks for OpsAgent {}

#[async_trait::async_trait]
impl ExpertAgent for OpsAgent {
    fn agent_type(&self) -> &'static str {
        "ops"
    }
    fn max_permission_level(&self) -> PermissionLevel {
        PermissionLevel::Destructive
    }
    async fn init(&mut self, ctx: Arc<super::ExpertContext>) {
        self.auth = Some(ctx.auth.clone());
    }
}

// ── System Status Tool ──

#[derive(Debug, Clone)]
pub struct SystemStatusTool;

impl ToolT for SystemStatusTool {
    fn name(&self) -> &str {
        "system_status"
    }
    fn description(&self) -> &str {
        "Query CPU, memory, disk, load average, and optionally top processes."
    }
    fn args_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "include_processes": {"type": "boolean"}
            }
        })
    }
}

#[async_trait::async_trait]
impl ToolRuntime for SystemStatusTool {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let include_procs = args
            .get("include_processes")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let mut result = serde_json::json!({});

        // CPU / Memory summary
        if let Ok(output) = tokio::process::Command::new("top")
            .args(["-bn1"])
            .output()
            .await
        {
            let stdout = String::from_utf8_lossy(&output.stdout);
            result["top_summary"] =
                serde_json::Value::String(stdout.lines().take(5).collect::<Vec<_>>().join("\n"));
        }

        // Disk
        if let Ok(output) = tokio::process::Command::new("df")
            .args(["-h", "/"])
            .output()
            .await
        {
            result["disk_root"] =
                serde_json::Value::String(String::from_utf8_lossy(&output.stdout).to_string());
        }

        // Load average
        if let Ok(load) = tokio::fs::read_to_string("/proc/loadavg").await {
            result["load_average"] = serde_json::Value::String(load.trim().to_string());
        }

        // Memory
        if let Ok(meminfo) = tokio::fs::read_to_string("/proc/meminfo").await {
            result["memory"] =
                serde_json::Value::String(meminfo.lines().take(3).collect::<Vec<_>>().join("\n"));
        }

        // Processes
        if include_procs {
            if let Ok(output) = tokio::process::Command::new("ps")
                .args(["--no-headers", "-eo", "pid,comm,%cpu,%mem", "--sort=-%cpu"])
                .output()
                .await
            {
                let stdout = String::from_utf8_lossy(&output.stdout);
                result["top_processes"] = serde_json::Value::String(
                    stdout.lines().take(10).collect::<Vec<_>>().join("\n"),
                );
            }
        }

        Ok(result)
    }
}

// ── Service Control Tool ──

#[derive(Debug, Clone)]
pub struct ServiceControlTool {
    auth: Option<Arc<autoagents_tool_auth::ToolAuthInterceptor>>,
}

impl ServiceControlTool {
    pub fn new(auth: Option<Arc<autoagents_tool_auth::ToolAuthInterceptor>>) -> Self {
        Self { auth }
    }
}

impl ToolT for ServiceControlTool {
    fn name(&self) -> &str {
        "service_control"
    }
    fn description(&self) -> &str {
        "Control systemd services: status, start, stop, restart, enable, disable."
    }
    fn args_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {"type": "string", "enum": ["status","start","stop","restart","enable","disable"]},
                "service_name": {"type": "string"}
            },
            "required": ["action", "service_name"]
        })
    }
}

#[async_trait::async_trait]
impl ToolRuntime for ServiceControlTool {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let action = args["action"]
            .as_str()
            .ok_or(ToolCallError::RuntimeError("action required".into()))?;
        let service = args["service_name"]
            .as_str()
            .ok_or(ToolCallError::RuntimeError("service_name required".into()))?;

        if action != "status" {
            if let Some(ref auth) = self.auth {
                let check = auth.check("ops", "service_control", PermissionLevel::System);
                if check.needs_confirmation() {
                    return Err(ToolCallError::RuntimeError(
                        "Service control requires user confirmation.".into(),
                    ));
                }
            }
        }

        let output = tokio::process::Command::new("systemctl")
            .arg(action)
            .arg(service)
            .output()
            .await
            .map_err(|e| ToolCallError::RuntimeError(e.to_string().into()))?;

        Ok(serde_json::json!({
            "action": action, "service": service,
            "stdout": String::from_utf8_lossy(&output.stdout),
            "success": output.status.success(),
        }))
    }
}

// ── Log View Tool ──

#[derive(Debug, Clone)]
pub struct LogViewTool;

impl ToolT for LogViewTool {
    fn name(&self) -> &str {
        "log_view"
    }
    fn description(&self) -> &str {
        "View system or application logs via journalctl or tail."
    }
    fn args_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "source": {"type": "string", "description": "journalctl or file path"},
                "service": {"type": "string"},
                "lines": {"type": "integer", "description": "Number of lines (default 50)"}
            },
            "required": ["source"]
        })
    }
}

#[async_trait::async_trait]
impl ToolRuntime for LogViewTool {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let source = args["source"]
            .as_str()
            .ok_or(ToolCallError::RuntimeError("source required".into()))?;
        let lines = args["lines"].as_u64().unwrap_or(50).to_string();

        let output = if source == "journalctl" {
            let service = args["service"].as_str().unwrap_or("");
            let mut cmd = tokio::process::Command::new("journalctl");
            cmd.args(["-n", &lines, "--no-pager"]);
            if !service.is_empty() {
                cmd.args(["-u", service]);
            }
            cmd.output().await
        } else {
            // Confine log reads: block secret-bearing paths (config.yaml,
            // ~/.ssh, /etc/shadow, …) even though /var/log stays open.
            let policy = super::path_policy::PathPolicy::for_logs();
            let safe = match policy.validate_resolved(source) {
                Ok(p) => p,
                Err(e) => {
                    return Err(ToolCallError::RuntimeError(
                        format!("log source '{source}' rejected: {e:?}").into(),
                    ));
                }
            };
            tokio::process::Command::new("tail")
                .args(["-n", &lines])
                .arg(safe.as_os_str())
                .output()
                .await
        };

        let output = output.map_err(|e| ToolCallError::RuntimeError(e.to_string().into()))?;
        let log_text = String::from_utf8_lossy(&output.stdout);
        Ok(serde_json::json!({ "log": log_text, "line_count": log_text.lines().count() }))
    }
}

// ── Cron Task Tool ──

#[derive(Debug, Clone)]
pub struct CronTaskTool {
    auth: Option<Arc<autoagents_tool_auth::ToolAuthInterceptor>>,
}

impl CronTaskTool {
    pub fn new(auth: Option<Arc<autoagents_tool_auth::ToolAuthInterceptor>>) -> Self {
        Self { auth }
    }
}

impl ToolT for CronTaskTool {
    fn name(&self) -> &str {
        "cron_task"
    }
    fn description(&self) -> &str {
        "View (list) cron scheduled tasks."
    }
    fn args_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {"type": "string", "enum": ["list"]}
            },
            "required": ["action"]
        })
    }
}

#[async_trait::async_trait]
impl ToolRuntime for CronTaskTool {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let action = args["action"]
            .as_str()
            .ok_or(ToolCallError::RuntimeError("action required".into()))?;
        match action {
            "list" => {
                let output = tokio::process::Command::new("crontab")
                    .arg("-l")
                    .output()
                    .await
                    .map_err(|e| ToolCallError::RuntimeError(e.to_string().into()))?;
                Ok(serde_json::json!({ "crontab": String::from_utf8_lossy(&output.stdout) }))
            }
            _ => Err(ToolCallError::RuntimeError(
                "Only 'list' is supported via tool.".into(),
            )),
        }
    }
}

// ── Process Manage Tool ──

#[derive(Debug, Clone)]
pub struct ProcessManageTool {
    auth: Option<Arc<autoagents_tool_auth::ToolAuthInterceptor>>,
}

impl ProcessManageTool {
    pub fn new(auth: Option<Arc<autoagents_tool_auth::ToolAuthInterceptor>>) -> Self {
        Self { auth }
    }
}

impl ToolT for ProcessManageTool {
    fn name(&self) -> &str {
        "process_manage"
    }
    fn description(&self) -> &str {
        "List processes or kill by PID."
    }
    fn args_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {"type": "string", "enum": ["list", "kill"]},
                "pid": {"type": "integer"},
                "signal": {"type": "string", "description": "SIGTERM (default) or SIGKILL"}
            },
            "required": ["action"]
        })
    }
}

#[async_trait::async_trait]
impl ToolRuntime for ProcessManageTool {
    async fn execute(&self, args: Value) -> Result<Value, ToolCallError> {
        let action = args["action"]
            .as_str()
            .ok_or(ToolCallError::RuntimeError("action required".into()))?;
        match action {
            "list" => {
                let output = tokio::process::Command::new("ps")
                    .args([
                        "--no-headers",
                        "-eo",
                        "pid,user,%cpu,%mem,comm",
                        "--sort=-%cpu",
                    ])
                    .output()
                    .await
                    .map_err(|e| ToolCallError::RuntimeError(e.to_string().into()))?;

                let stdout = String::from_utf8_lossy(&output.stdout);
                let procs: Vec<&str> = stdout.lines().take(20).collect();
                Ok(serde_json::json!({ "processes": procs }))
            }
            "kill" => {
                if let Some(ref auth) = self.auth {
                    let check = auth.check("ops", "process_manage", PermissionLevel::System);
                    if check.needs_confirmation() {
                        return Err(ToolCallError::RuntimeError(
                            "Killing processes requires user confirmation.".into(),
                        ));
                    }
                }
                let pid = args["pid"]
                    .as_u64()
                    .ok_or(ToolCallError::RuntimeError("pid required".into()))?;
                let signal = args["signal"].as_str().unwrap_or("SIGTERM");

                let output = tokio::process::Command::new("kill")
                    .args(["-s", signal, &pid.to_string()])
                    .output()
                    .await
                    .map_err(|e| ToolCallError::RuntimeError(e.to_string().into()))?;

                Ok(
                    serde_json::json!({ "pid": pid, "signal": signal, "success": output.status.success() }),
                )
            }
            _ => Err(ToolCallError::RuntimeError("Invalid action".into())),
        }
    }
}
