//! Intent classification for the supervisor agent.
//!
//! Uses a lightweight heuristic approach first, with LLM fallback for ambiguity.

use serde::{Deserialize, Serialize};

use autoagents_memory::TaskRecord;

/// Classified user intent.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Intent {
    /// A new task to be dispatched to an expert agent.
    NewTask {
        task_type: String,
        description: String,
        priority: i32,
    },
    /// A follow-up message to an existing task.
    FollowUp { task_id: String, message: String },
    /// A slash command (e.g. /status, /reload).
    Command { command: String },
    /// A general question or request for information.
    Query { question: String },
}

/// Intent classifier using heuristics first, with LLM fallback.
pub struct IntentClassifier;

impl IntentClassifier {
    /// Classify a user message into an Intent.
    pub async fn classify(
        message: &str,
        active_task: Option<&TaskRecord>,
    ) -> Result<Intent, String> {
        let trimmed = message.trim();

        // 1. Check for slash commands
        if trimmed.starts_with('/') {
            return Ok(Intent::Command {
                command: trimmed.to_string(),
            });
        }

        // 2. Check for task ID references in follow-ups
        if let Some(task) = active_task {
            // Short message referencing the ongoing task
            if is_likely_follow_up(trimmed, task) {
                return Ok(Intent::FollowUp {
                    task_id: task.id.clone(),
                    message: trimmed.to_string(),
                });
            }
        }

        // 3. Heuristic task type detection
        let task_type = detect_task_type(trimmed);
        if task_type != "query" {
            return Ok(Intent::NewTask {
                task_type,
                description: trimmed.to_string(),
                priority: detect_priority(trimmed),
            });
        }

        // 4. Default: general query
        Ok(Intent::Query {
            question: trimmed.to_string(),
        })
    }
}

/// Quick heuristic to detect if this is a follow-up to an active task.
fn is_likely_follow_up(message: &str, task: &TaskRecord) -> bool {
    let msg_lower = message.to_lowercase();
    let task_desc_lower = task.description.to_lowercase();

    // Very short messages are likely follow-ups
    if message.len() < 30 {
        return true;
    }

    // Contains keywords suggesting continuation
    let follow_keywords = [
        "再加",
        "改成",
        "修改",
        "还是",
        "换成",
        "不要",
        "另外",
        "还有",
        "继续",
        "刚才",
        "刚刚",
        "刚刚那个",
        "这个任务",
        "instead",
        "also",
        "change",
        "update",
        "继续做",
    ];
    for kw in &follow_keywords {
        if msg_lower.contains(kw) {
            return true;
        }
    }

    // References the same topic
    let task_words: Vec<&str> = task_desc_lower.split_whitespace().collect();
    let message_words: Vec<&str> = msg_lower.split_whitespace().collect();
    let common = task_words
        .iter()
        .filter(|w| message_words.contains(w))
        .count();
    if common >= 3 {
        return true;
    }

    false
}

/// Detect the task type based on keywords.
fn detect_task_type(message: &str) -> String {
    let msg = message.to_lowercase();

    // Coding keywords
    let coding_kw = [
        "写",
        "代码",
        "脚本",
        "编程",
        "程序",
        "函数",
        "debug",
        "编译",
        "运行",
        "开发",
        "重构",
        "git",
        "commit",
        "测试",
        "bug",
        "报错",
        "错误",
        "修复",
        "实现",
        "接口",
        "api",
        "code",
        "script",
        "function",
        "implement",
        "fix",
    ];
    for kw in &coding_kw {
        if msg.contains(kw) {
            return "coding".to_string();
        }
    }

    // Ops keywords
    let ops_kw = [
        "服务",
        "部署",
        "监控",
        "日志",
        "重启",
        "备份",
        "磁盘",
        "内存",
        "cpu",
        "进程",
        "网络",
        "端口",
        "安装",
        "配置",
        "防火墙",
        "nginx",
        "docker",
        "数据库",
        "mysql",
        "postgres",
        "定时",
        "crontab",
        "计划任务",
        "启动",
        "停止",
        "server",
        "deploy",
        "monitor",
        "restart",
        "disk",
    ];
    for kw in &ops_kw {
        if msg.contains(kw) {
            return "ops".to_string();
        }
    }

    // Document keywords
    let doc_kw = [
        "文档",
        "pdf",
        "docx",
        "excel",
        "ppt",
        "表格",
        "文件",
        "翻译",
        "摘要",
        "总结",
        "报告",
        "生成",
        "格式",
        "document",
        "translate",
        "summarize",
        "report",
    ];
    for kw in &doc_kw {
        if msg.contains(kw) {
            return "document".to_string();
        }
    }

    // Search / info keywords
    let info_kw = [
        "搜索",
        "查询",
        "天气",
        "新闻",
        "搜索一下",
        "查一下",
        "帮我查",
        "找一下",
        "search",
        "find",
        "weather",
        "news",
    ];
    for kw in &info_kw {
        if msg.contains(kw) {
            return "information".to_string();
        }
    }

    // Default: general query
    "query".to_string()
}

/// Detect task priority from message content.
fn detect_priority(message: &str) -> i32 {
    let msg = message.to_lowercase();
    let high_kw = [
        "紧急",
        "马上",
        "立刻",
        "立即",
        "尽快",
        "快",
        "重要",
        "urgent",
        "asap",
        "critical",
        "important",
        "now",
    ];
    for kw in &high_kw {
        if msg.contains(kw) {
            return 2; // High priority
        }
    }
    1 // Normal
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_detection() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let intent = rt
            .block_on(IntentClassifier::classify("/status", None))
            .unwrap();
        assert!(matches!(intent, Intent::Command { .. }));
    }

    #[test]
    fn test_new_task_coding() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let intent = rt
            .block_on(IntentClassifier::classify(
                "帮我写个Python脚本处理日志",
                None,
            ))
            .unwrap();
        match intent {
            Intent::NewTask { task_type, .. } => assert_eq!(task_type, "coding"),
            other => panic!("Expected NewTask(coding), got {:?}", other),
        }
    }

    #[test]
    fn test_new_task_ops() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let intent = rt
            .block_on(IntentClassifier::classify("检查一下磁盘使用率", None))
            .unwrap();
        match intent {
            Intent::NewTask { task_type, .. } => assert_eq!(task_type, "ops"),
            other => panic!("Expected NewTask(ops), got {:?}", other),
        }
    }

    #[test]
    fn test_new_task_document() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let intent = rt
            .block_on(IntentClassifier::classify("把这份PDF翻译成英文", None))
            .unwrap();
        match intent {
            Intent::NewTask { task_type, .. } => assert_eq!(task_type, "document"),
            other => panic!("Expected NewTask(document), got {:?}", other),
        }
    }

    #[test]
    fn test_new_task_info() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let intent = rt
            .block_on(IntentClassifier::classify("帮我查一下深圳天气", None))
            .unwrap();
        match intent {
            Intent::NewTask { task_type, .. } => assert_eq!(task_type, "information"),
            other => panic!("Expected NewTask(information), got {:?}", other),
        }
    }

    #[test]
    fn test_follow_up_detection() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let task = TaskRecord::new("abc123", "写一个监控CPU温度的脚本");
        let intent = rt
            .block_on(IntentClassifier::classify("再加个内存监控吧", Some(&task)))
            .unwrap();
        assert!(matches!(intent, Intent::FollowUp { .. }));
    }

    #[test]
    fn test_high_priority_detection() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let intent = rt
            .block_on(IntentClassifier::classify("紧急！服务器挂了快看看", None))
            .unwrap();
        match intent {
            Intent::NewTask { priority, .. } => assert_eq!(priority, 2),
            _ => panic!("Expected NewTask with priority"),
        }
    }

    #[test]
    fn test_normal_priority() {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let intent = rt
            .block_on(IntentClassifier::classify("帮我写个脚本", None))
            .unwrap();
        match intent {
            Intent::NewTask { priority, .. } => assert_eq!(priority, 1),
            _ => panic!("Expected NewTask with normal priority"),
        }
    }
}
