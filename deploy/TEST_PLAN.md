# Personal Assistant 部署测试计划

部署后按顺序执行，每步有预期结果。哪步失败就停在那步排查。

## 部署前准备

- [ ] **feishu 开放平台**:App ID / App Secret / Verification Token 已填进 `config.yaml`
- [ ] **feishu 回调**:地址设为 `http://<1037U-IP>:8080/feishu/event`,订阅 `im.message.receive_v1`
- [ ] **bubblewrap**:`apt install -y bubblewrap`
- [ ] **API key**:`echo 'DEEPSEEK_KEY=<key>' > /opt/personal-assistant/.env`
- [ ] **部署**:`bash deploy/build_and_deploy.sh`

---

## 第一层：服务起得来

- [ ] **服务状态**

```bash
ssh root@1037u "systemctl status personal-assistant --no-pager"
```

预期:`active (running)`，`Main PID` 存在。

如果失败:

```bash
ssh root@1037u "journalctl -u personal-assistant -n 50 --no-pager"
```

---

## 第二层：飞书收得到消息

在飞书 App 中给 Bot 发消息:

- [ ] **基本对话**

> 你好

预期:回复一段真正的文字(不能是"收到你的问题，我来处理: 你好"这种回显)。

失败排查:

```bash
ssh root@1037u "journalctl -u personal-assistant --no-pager | grep -i 'callback\|event\|supervisor'"
```

---

## 第三层：Agent 真能干活

- [ ] **coding agent 写文件**

> 在 /tmp/personal-assistant-workspace 下创建一个文件 test.txt 内容是 hello world

预期:Agent 回复确认，文件确实创建了。

验证:

```bash
ssh root@1037u "cat /tmp/personal-assistant-workspace/test.txt"
# 预期: hello world
```

- [ ] **ops agent 读系统**

> 查看 CPU 和内存使用情况

预期:回复中包含 CPU 百分比、内存容量等具体数值(不是空话)。

- [ ] **脱敏生效**

> 读一下 /opt/personal-assistant/.env

预期:要么报错(沙箱文件不可见)，要么返回值全部显示为 `[REDACTED]`。**绝对不能出现原始 API key**。

---

## 第四层：沙箱真的拦住了

- [ ] **密钥目录不可见**

> 用 cat 命令看一下 /opt/personal-assistant/config.yaml

预期:命令执行失败或输出为空——bubblewrap 沙箱把 `/opt/personal-assistant` 屏蔽了。

- [ ] **网络外发被拦截**

> 用 curl 访问 http://www.baidu.com

预期:shell 分析器判定 `curl` 为 Destructive → 拒绝执行("Command blocked")。即使绕过分析器，沙箱的 `--unshare-net` 也会让网络不可用 → curl 连接超时。

- [ ] **审计日志有记录**

```bash
ssh root@1037u "tail -30 /opt/personal-assistant/audit/audit.log"
```

预期:有 `shell_execute` / `write_file` / `system_status` 的操作记录，含时间戳和 `allowed` / `blocked` 标记。

---

## 常见失败排查

| 现象 | 最可能原因 | 检查 |
|---|---|---|
| 服务起不来 | bubblewrap 没装(coding agent shell fail-closed → 启动失败) | `apt install -y bubblewrap` |
| 飞书不回消息 | 回调地址配错 / 没订阅事件 | 飞书开放平台 → 事件订阅 |
| Agent 不调工具 | DEEPSEEK_KEY 没填进 `.env` 或 key 不对 | `cat /opt/personal-assistant/.env` |
| 沙箱不生效 | bubblewrap 没装(退化为 unsandboxed) | `which bwrap` |
