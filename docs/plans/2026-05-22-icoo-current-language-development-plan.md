# Icoo 当前语言开发计划

**Status:** 2026-05-22 执行版。

**Goal:** 在现有 AST interpreter 稳定通过完整测试的基础上，把 Icoo 从“核心能力可用”推进到“运行时边界清晰、二进制/权限/工具链更稳、VM 迁移路径可验证”的阶段。

**Current baseline:** `cargo fmt --check` 与 `cargo test` 已通过；`web_ino_perf` 仍是手动 ignored 性能测试。

---

## 1. 当前语言状态

已完成：

- 核心语言：变量、常量、`final`、函数、闭包、类、继承、字段、类型标注、数组、Map、字符串模板、多行字符串、`try/catch`。
- 模块系统：本地文件导入、命名导入、导出、循环依赖拒绝、`std.*` 标准库导入。
- async：`EventLoop`、`Task`、协程指令层、`await` 子集、取消唤醒、跨 loop 校验、复杂表达式 await 限制。
- 解释器结构：`src/interpreter/mod.rs` 已收敛为入口文件；模块加载、调用、类、任务、方法、求值、协程编译、类型 helper、参数 helper、格式转换均已拆出。
- 标准库：math、time、json、yaml、toml、env、io、io.fs、os、net.http.client/server、net.ws.client/server、net.sse.client/server、net.socket.client/server、web.ino。
- 二进制能力：`Bytes`、`Buffer`、Hex/Base64、文件 bytes 读写、HTTP bytes request/response、HTTP bytes streaming、WebIno `body_bytes` / `content_bytes` / `send_bytes` / `write_bytes`。
- 资源边界：WebIno 请求 body 默认限制为 16 MiB；HTTP client 普通响应体默认限制为 16 MiB，流式单 chunk 默认限制为 64 KiB。
- 权限模型：已有 coarse-grained allow/deny，并支持 fs 路径白名单、env key 白名单、net host/port 白名单；覆盖 fs/env/os/net/WebIno listen/download。
- 网络协议 MVP：WebSocket、SSE 和裸 TCP Socket 已有单连接 client/server 模块；WebSocket 暂只支持 `ws://` 和单帧消息，SSE 暂只支持 `http://`。
- VM：已有同步子集原型和 backend equivalence 测试。

主要风险：

- 权限白名单已接入主要宿主能力；后续风险在于配置入口、通配/域名规则和更完整的嵌入 API 设计。
- VM 只覆盖同步小子集，不能承载函数、类、模块、native 和 async 主路径。
- 语言设计总览需要继续同步已落地行为，防止文档落后。

## 2. 开发原则

- 主执行路径继续使用 AST interpreter；VM 只做实验后端和行为等价验证。
- 新 runtime 能力进入职责明确的子模块，不把逻辑堆回 `src/interpreter/mod.rs`。
- WebIno 暂停横向框架扩张，只做安全、二进制兼容、资源边界和回归修复。
- 默认权限保持 allow-all，避免破坏现有脚本；受限权限通过配置和测试覆盖。
- 每个阶段必须有专项测试，再跑完整 `cargo test`。

## 3. 阶段计划

### Phase 1：资源边界与二进制 streaming（已完成）

**目标：** 让 HTTP/WebIno 在二进制能力可用的基础上具备更安全的资源边界。

已完成：

- 为 WebIno 请求读取增加最大 body 体积限制。
- 为 multipart 文件和普通 body 增加大小拒绝测试。
- 为 HTTP client 响应体和 streaming chunk 增加最大体积限制。
- 增加 `stream_get_bytes`、`stream_post_bytes`、`stream_put_bytes`、`stream_delete_bytes`、`stream_options_bytes`，handler 接收 `Bytes`。
- 确认旧 `stream_get` / `stream_post` 文本 API 保持兼容。

建议测试：

```text
cargo fmt --check
cargo test --test bytes
cargo test --test http_client_bytes
cargo test --test web_ino_response_headers
cargo test --test web_ino_routes
cargo test
```

验收：

- 超限请求返回清晰错误或 400 响应，不造成无限读。
- bytes streaming 不损坏 `0x00`、`0xff` 等非 UTF-8 数据。
- 旧文本 streaming 测试继续通过。

### Phase 2：权限模型细化（已完成）

**目标：** 从大类 allow/deny 进入可嵌入的细粒度权限模型。

已完成：

- `fs_read` / `fs_write` 支持路径白名单或沙箱根目录。
- `net_connect` / `net_listen` 支持 host/port 规则。
- `env_read` 支持 key 白名单。
- 错误消息包含被拒绝资源摘要，例如路径、host、port 或 env key。
- 保持默认 allow-all。

建议测试：

```text
cargo fmt --check
cargo test --test permissions_matrix
cargo test --test language
cargo test
```

验收：

- 受限模式能允许一部分资源、拒绝另一部分资源。
- 默认配置不改变现有脚本行为。
- WebIno download/listen 与 HTTP client/server 权限仍受控。

### Phase 3：开发工具链与诊断

**目标：** 提高语言开发和用户调试体验。

任务：

- 改进 `icoo check` 的错误定位和导入链诊断。
- 设计 `icoo ast <file.icoo>` 或 `icoo debug ast <file.icoo>`。
- 设计最小 formatter/linter 范围，暂不急于完整实现。
- 为 typechecker/native method 错误补更清楚的参数位置说明。

建议测试：

```text
cargo fmt --check
cargo test --test cli
cargo test --test error_handling
cargo test --test language
cargo test
```

验收：

- CLI 行为兼容已有 `run/check/help/version`。
- 新诊断不牺牲已有错误测试稳定性。

### Phase 4：VM 同步子集扩展

**目标：** 让 VM 从表达式/控制流子集扩展到函数调用和局部 frame，继续作为等价验证后端。

任务：

- 设计 VM frame、call/return 指令和局部变量布局。
- 支持普通函数调用和返回值。
- 支持闭包前先明确限制或拒绝。
- 扩展 backend equivalence 测试。
- 不迁移 class/module/native/async 主路径。

建议测试：

```text
cargo fmt --check
cargo test --test backend_equivalence
cargo test --test language
cargo test
```

验收：

- AST backend 与 VM backend 在同步函数子集上结果一致。
- VM 遇到未支持特性继续给出稳定 unsupported 信息。

### Phase 5：语言设计文档同步

**目标：** 把已经实现的行为变成可维护的语言规范。

任务：

- 更新 `docs/plans/2026-05-21-icoo-language-design.md` 中的 Bytes/Buffer、WebIno bytes、权限和 async 限制。
- 给 `Bytes` / `Buffer` 写稳定 API 表。
- 标注当前保留、实验、暂停的功能。
- 补错误语义和兼容性说明。

验收：

- 文档中的 API 名称与代码一致，例如 `Buffer.from_bytes()`。
- 文档不宣称未完成的 VM、权限或 WebIno 框架能力。

## 4. 推荐执行顺序

1. CLI 诊断增强。
2. VM 函数调用和 frame。
3. 语言设计文档总同步。

## 5. 暂不做

- 不继续扩展 WebIno middleware/router group/custom error handler。
- 不把 VM 切为默认 backend。
- 不做包管理器。
- 不做完整 formatter。
- 不把运行时全面改成 `Arc<Mutex>`。
- 不新增大型标准库。

## 6. 每轮完成标准

每一轮开发结束前至少满足：

- `cargo fmt --check` 通过。
- 相关专项测试通过。
- `cargo test` 通过。
- 文档同步到对应计划或设计文档。
- `git status --short --untracked-files=all` 清楚，新增文件都属于本轮目标。
