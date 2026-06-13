# Personal Assistant

基于 AutoAgents 的 Linux 服务器个人助理，通过飞书 Bot 交互。

## 前置要求

### 飞书开放平台配置

1. 前往 [飞书开放平台](https://open.feishu.cn/) 创建企业自建应用
2. 在"应用功能"中启用 **机器人** 能力
3. 在"事件订阅"中配置回调地址: `http://<你的1037U地址>:8080/feishu/event`
4. 订阅事件: `im.message.receive_v1`
5. 获取 App ID、App Secret、Verification Token
6. 填写到 `deploy/config.yaml` 中

### API Key 环境变量

在 1037U 上设置:

```bash
echo 'MINIMAX_KEY=your-minimax-key' | sudo tee -a /opt/personal-assistant/.env
echo 'DEEPSEEK_KEY=your-deepseek-key' | sudo tee -a /opt/personal-assistant/.env
echo 'GLM_KEY=your-glm-key' | sudo tee -a /opt/personal-assistant/.env
```

### 系统依赖：bubblewrap（沙箱）

Coding Agent 的 shell 命令在 `bubblewrap` 沙箱中执行——无网络、根文件系统只读、只有工作目录可写，`/opt/personal-assistant` 和 `~/.ssh` 等对沙箱不可见。这样即使危险度分析被绕过，也读不到密钥、发不出网络。服务端默认 `SandboxPolicy::Required`，**未安装 bwrap 时 shell 工具一律拒绝执行（fail-closed）**。

在 1037U 上安装并验证：

```bash
sudo apt update && sudo apt install -y bubblewrap
bwrap --version
```

> REPL（本地 `--repl`）默认 `Auto`：有 bwrap 就沙箱，没有就退化为普通 shell 并打日志，方便在开发机上使用。

## 编译和部署

在 MacBook 上:

```bash
# 安装 cross 交叉编译工具
cargo install cross

# 一键编译 + 部署到 1037U
bash deploy/build_and_deploy.sh
```

首次部署需要根据你的 1037U IP 修改 `deploy/build_and_deploy.sh` 中的 `TARGET_HOST`。

## 项目结构

```
crates/
  autoagents-tool-auth/    # 工具权限框架 (Safe/Write/System/Destructive)
  autoagents-memory/       # SQLite 持久化 + 心跳调度
  autoagents-experts/      # 专家 Agents (编程、运维)
  autoagents-supervisor/   # 主管 Agent (意图路由、任务调度、长期记忆)
  autoagents-server/       # HTTP 服务 + 飞书 Bot 接入

deploy/
  config.yaml              # 应用配置
  personal-assistant.service  # systemd unit
  build_and_deploy.sh      # 编译部署脚本
```

## 使用方式

在飞书 App 中找到你的 Bot:

- 发消息直接对话，Bot 会自动理解意图并分配专家处理
- `/status` - 查看系统状态
- `/tasks` - 查看当前任务列表
- `/help` - 查看帮助
- `/reload` - 重新加载配置

## 容量规划 (1037U)

- 内存预算: < 300MB
- 并发任务上限: 2 (可配置)
- 向量库: InMemoryVectorStore + mmap，< 200MB
- SQLite: WAL 模式，< 50MB
- 飞书文件上传限制: 20MB
