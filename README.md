# Personal Assistant — 本地 Linux 服务器个人助理

基于 [AutoAgents](https://github.com/liquidos-ai/AutoAgents)（MIT / Apache 2.0）构建的本地个人助理，运行在你的 Linux 服务器上，通过飞书 Bot 与手机交互。

像跟真人实习生聊天一样——发消息、发文件，它帮你写代码、管服务器、处理文档。

## 功能

- **编程**：写脚本、执行命令、Git 操作、代码搜索
- **运维**：系统监控、服务管理、日志分析、定时任务
- **文档**：读写 PDF/DOCX/XLSX/Markdown，格式转换
- **知识库**：本地文档索引、语义搜索
- **信息检索**：网页搜索、API 调用

## 架构

```
飞书 App → 飞书开放平台 → HTTP 回调 → 主管 Agent → 编程/运维/文档/信息/知识库 Agent
```

一个主管协调所有专家，长期记住你的偏好和习惯，越用越聪明。

## 硬件要求

- Linux x86_64 服务器（1 核 4GB 即可）
- 不跑本地 LLM，需要 MiniMax / DeepSeek / GLM API Key

## 快速开始

### 1. 飞书开放平台

创建企业自建应用 → 启用机器人 → 订阅 `im.message.receive_v1` → 记下 **App ID / App Secret / Verification Token / Encrypt Key**（在"事件订阅"页开启"加密策略"以获得 Encrypt Key）。

回调地址用 HTTPS（例如 `https://你的域名/feishu/event`），由 nginx/Caddy 反代到服务器的 `127.0.0.1:8080`。**不要**直接把 `0.0.0.0:8080` 暴露到公网。

### 2. 编译部署

```bash
# 在 MacBook 上（需要 zig + cargo-zigbuild）
rustup target add x86_64-unknown-linux-gnu
export TARGET_HOST=root@你的服务器IP
bash deploy/build_and_deploy.sh
```

部署脚本会创建非 root 的 `personal-assistant` 系统账户、以该账户运行服务，并只在 `config.yaml` 不存在时用 `config.example.yaml` 建一个（**不会**覆盖你已填好的真实配置）。

### 3. 配置

编辑 `/opt/personal-assistant/config.yaml`（权限自动收紧为 0600）：

- `app_id` / `app_secret` / `verification_token`：飞书凭据。
- `encrypt_key`：**强烈建议填写**。配置后每个回调都会带 `X-Lark-Signature`，服务用 SHA256 校验并 AES 解密，能防伪造/篡改/重放。留空则退回仅校验 `verification_token` 的弱模式。
- `allowed_sender_ids`：**发送者白名单**（你的 `open_id`）。留空 = 拒绝所有人。先随便发一条消息，从日志里看到自己的 `open_id` 后填进来。
- LLM 的 API Key 通过环境变量（`MINIMAX_KEY` / `DEEPSEEK_KEY` / `GLM_KEY`）注入。

> ⚠️ 默认是 fail-closed：`encrypt_key`/`verification_token` 与 `allowed_sender_ids` 任意一项未配置，回调都会被拒绝。这是有意的——这个 Bot 能在你服务器上执行命令。

## 安全

- 回调先验签/校验 token + 时间戳新鲜度，再做任何业务（[security.rs](crates/autoagents-server/src/feishu/security.rs)）。
- 发送者白名单：只有指定的 `open_id` 能驱动命令执行。
- 文件工具（读写/日志）有路径收敛 + 敏感目录黑名单（[path_policy.rs](crates/autoagents-experts/src/path_policy.rs)），防读取 `config.yaml`/`~/.ssh` 等。
- 服务以非 root 账户运行，systemd 沙箱（`ProtectSystem=strict`、`NoNewPrivileges`）。
- 配置与审计日志权限 0600。
- ⚠️ 仍待加固：`coding` agent 的 `shell_execute` 走 `sh -c`（见 [coding.rs](crates/autoagents-experts/src/coding.rs)），权限守卫是正则黑名单（易绕过）。当前依赖入口鉴权（验签 + 白名单）兜底；若要给 coding agent 开放任意 shell，建议放进容器/`bwrap` 隔离。

## 项目结构

```
crates/
  autoagents-server/       HTTP 服务 + 飞书 Bot 接入
  autoagents-supervisor/   主管 Agent（意图路由、任务调度）
  autoagents-experts/      专家 Agent（编程、运维等）
  autoagents-tool-auth/    工具权限框架（四级权限 + Shell 安全分析）
  autoagents-memory/       SQLite 持久化 + 心跳调度
  autoagents-*/            原 AutoAgents 框架（不动）
```

## 开源协议

本项目基于 [AutoAgents](https://github.com/liquidos-ai/AutoAgents) 构建，保留原 MIT / Apache 2.0 双许可。新增代码同样以 MIT OR Apache 2.0 发布。

`docs/` 为原 AutoAgents 框架文档，`deploy/README.md` 为本项目使用说明。
