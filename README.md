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

创建企业自建应用 → 启用机器人 → 订阅 `im.message.receive_v1` → 配置回调地址 `http://你的服务器IP:8080/feishu/event`

### 2. 编译部署

```bash
# 在 MacBook 上（需要 zig + cargo-zigbuild）
rustup target add x86_64-unknown-linux-gnu
export TARGET_HOST=root@你的服务器IP
bash deploy/build_and_deploy.sh
```

### 3. 配置

编辑 `/opt/personal-assistant/config.yaml`，填入飞书 App ID/Secret 和 LLM API Key。

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
