# Pi Agent 对标与 Icoo 语言优化记录

日期：2026-05-26

## 目标

目标项目：`https://github.com/earendil-works/pi.git`

本次分析的目标不是移植 Pi 的 TypeScript 代码，而是提炼“用 Icoo 写出同类 AI Agent”需要的语言和标准库能力，并优先补齐最短路径上的缺口。

## Pi 的核心结构

Pi 是一个 monorepo，关键包分为四层：

- `packages/ai`：统一多提供商 LLM API，覆盖 OpenAI、Anthropic、Google、Bedrock、OpenRouter 等，负责消息转换、流式事件解析、工具调用 payload 和 token/cost 统计。
- `packages/agent`：通用 Agent loop，维护上下文、消息、工具调用、工具结果、turn 生命周期和并行/串行工具执行。
- `packages/coding-agent`：面向代码任务的会话层，负责 session 存储、压缩、模型选择、系统提示、技能/扩展加载、文件工具、bash 工具、RPC/交互模式。
- `packages/tui`：终端 UI 和渲染系统，不是 Agent 能力的核心依赖，但决定交互体验。

Pi 的最小可复刻 Agent 能力可以压缩为：LLM 调用、消息上下文、工具 schema、工具执行、会话持久化、文件/命令工具、流式输出、权限控制。

## Icoo 当前能力映射

Icoo 已具备的基础：

- JSON/YAML/TOML 编解码，可以承载消息、工具 schema 和配置。
- HTTP client 支持普通请求、字节请求和流式接收，可以作为 LLM API 的底层。
- 文件系统模块覆盖读写、追加、目录列举、元数据、原子写入等能力。
- 环境变量和 OS 信息模块可读取 API key、cwd、args 等运行上下文。
- Map/Array/String 基础方法可以表达消息结构和工具结果。
- async/event loop 已存在，可以承载后续的流式 Agent loop。
- Runtime permissions 已覆盖 fs/env/os/net，适合嵌入场景。

主要缺口：

- 缺少本地进程执行能力，无法实现 Pi 的 `bash` 工具。
- 多行集合字面量和函数调用参数解析不够友好，不利于书写复杂配置、消息和工具 schema。
- 尚无一等的 LLM provider 模块，当前需要用通用 HTTP+JSON 直接拼请求。
- 尚无 session/compaction 标准库封装，需要用 Icoo 代码基于 fs/json 实现。
- 尚无流式 SSE/Responses 事件解析辅助库，虽然 HTTP streaming 底层已经具备。

## 本次优化

1. 新增 `std.process.exec(command, options?)`。

   返回结构化 Map：

   - `exit_code`: `Int | nil`
   - `success`: `Bool`
   - `timed_out`: `Bool`
   - `stdout`: `String`
   - `stderr`: `String`
   - `stdout_truncated`: `Bool`
   - `stderr_truncated`: `Bool`

   支持 options：

   - `cwd`: `String`
   - `timeout_ms`: `Int`
   - `env`: `Map<String, String>`
   - `max_output_bytes`: `Int`

2. 权限系统新增 `process_exec`，并提供：

   - `can_exec_process()`
   - `check_process_exec()`

   默认 `allow_all()` 允许执行；`deny_all()` 拒绝执行。

3. 解析器支持多行：

   - 函数调用参数
   - Array 字面量
   - Map 字面量
   - 嵌套 Map/Array 配置

这让 Icoo 可以自然表达 Agent 配置和工具 schema，例如：

```python
let tool = {
    "name": "bash",
    "limits": {
        "max_output_bytes": 4096,
    },
}
```

## 下一步建议

优先级 1：实现 `std.ai.openai.responses` 或通用 `std.ai.llm` 模块，封装消息、工具 schema 和流式响应解析。

优先级 2：在 Icoo 层实现 Agent loop 示例：`messages -> LLM -> tool_calls -> tool_results -> next turn`。

优先级 3：补 `std.text` 或扩展 String 方法，例如 `split`、`trim`、`starts_with`、`replace`，降低 SSE 和 transcript 处理成本。

优先级 4：补 session 示例库，用 `std.io.fs` + `std.json` 存储消息、分支摘要和压缩结果。

优先级 5：在 `examples/agent/` 中落地一个最小 coding agent：支持 read/write/list/exec 四个工具，先用 OpenAI-compatible HTTP API。
