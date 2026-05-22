# Icoo 下一阶段并行开发计划

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 把 Icoo 从“功能持续堆叠的 MVP”推进到“结构可维护、权限边界清晰、Web/async 语义更稳定”的下一阶段。

**Architecture:** 先做低风险结构拆分，保持现有语言行为不变；再在拆分后的边界上补 WebIno、权限模型和 async 语义。短期继续使用当前 AST 解释器和协程指令层，不立即切换到字节码 VM，但同步输出 VM 设计草案。

**Tech Stack:** Rust 2021、现有 AST interpreter、Tokio timer backend、标准库 `std.*` native modules、Cargo integration tests。

---

## 1. 当前基线

- 最新提交：`feb7078 Add WebIno params and HTTP header support`
- 仓库状态：计划生成前 `git status --short` 为空
- 已通过测试：
  - `cargo test`
  - `cargo test --test web_ino_perf -- --ignored --nocapture`
- 当前重点风险：
  - `src/interpreter/mod.rs` 同时承载核心解释执行、HTTP client、WebIno、EventLoop/Task 调度和大量 helper。
  - 标准库实现、类型签名和文档 API 清单仍存在重复维护。
  - `std.io.fs`、`std.os`、`std.env`、网络和 `res.download()` 暂无统一权限边界。
  - 当前 Tokio 后端主要用于阻塞 sleep，并不代表 Icoo 用户代码已经多线程执行。

## 2. 架构决策

### ADR-001：先拆解释器内部模块，不先拆 `Value`

**Decision:** 下一阶段先把 `src/interpreter/mod.rs` 内的 HTTP/WebIno/EventLoop helper 拆到 `src/interpreter/*.rs`，暂不移动 `src/runtime/value.rs` 中的 `Value` enum 和相关结构体。

**Reason:** `Value` 是全局运行时协议，影响 parser/typechecker/interpreter/native modules。先拆解释器内部文件可以明显降低冲突，并保持行为不变。

**Consequence:** 第一阶段主要是结构重排，不应引入用户可见行为变化。

### ADR-002：权限模型默认兼容现状

**Decision:** 引入 `RuntimePermissions` 或 `HostCapabilities` 时，默认配置保持全允许；新增受限模式专门用于测试和嵌入场景。

**Reason:** 当前测试和脚本依赖文件、网络、环境读取能力。直接默认拒绝会破坏现有行为。

**Consequence:** 权限能力先作为运行时配置进入，不改变 `run_source()`、`run_file()` 默认体验。

### ADR-003：短期不重写 VM

**Decision:** async/await 短期继续基于现有 AST 解释器 + `CoroutineInstr` 调度，先修任务生命周期和语义边界；字节码 VM 只做设计文档和小原型。

**Reason:** 当前协程已有 `instructions + pc` 基础，足够支撑 MVP。过早切 VM 会把类、闭包、模块、native method、错误 span 全部重新落地，风险过大。

**Consequence:** 需要明确当前 `await` 支持子集，避免复杂表达式恢复时重复执行副作用。

## 3. 并行协作规则

- 每个 worker 只拥有明确文件范围，避免互相改同一片代码。
- `src/interpreter/mod.rs` 是高冲突文件，由主线程或集成 worker 统一接线。
- 每个 worker 先写测试，再实现，再跑自己的专项测试。
- 每个 worker 完成后只提交自己的范围；集成提交单独做。
- 每轮合并后跑：
  - `cargo fmt`
  - worker 专项测试
  - `cargo test`

## 4. 今日/下一轮 Worker 分工

### Worker A：HTTP client/server 解释器拆分

**Goal:** 把 HTTP client/server 的传输、解析和流式读取逻辑从 `src/interpreter/mod.rs` 拆出，不改变行为。

**Files:**
- Create: `src/interpreter/http_common.rs`
- Create: `src/interpreter/http_client.rs`
- Create: `src/interpreter/http_server.rs`
- Modify: `src/interpreter/mod.rs`
- Read-only reference: `src/native_modules/net_http_client.rs`
- Read-only reference: `src/native_modules/net_http_server.rs`
- Test: `tests/http_client_headers.rs`
- Test: `tests/language.rs`

**Steps:**

1. 写保护性测试，不新增功能：
   - HTTP 默认端口和自定义端口。
   - chunked malformed response 报错。
   - stream `Content-Length` 响应。

2. 运行测试确认当前行为：

```text
cargo test --test http_client_headers
cargo test supports_imported_net_http_client_and_server_modules
cargo test supports_net_http_client_stream_receive
```

3. 创建 `http_common.rs`：
   - 移入 `find_http_body_start`
   - 移入 `http_content_length`
   - 移入 `http_status_text`
   - 移入通用 header CR/LF 校验函数

4. 创建 `http_client.rs`：
   - 移入 `ParsedHttpUrl`
   - 移入 `ParsedHttpResponseHead`
   - 移入 `HttpClientHeaders`
   - 移入 `http_client_request`
   - 移入 `http_stream_method_name`
   - 移入 stream chunk/content-length/until-close 读取函数
   - 保留 `impl Interpreter { http_client_stream_request(...) }`

5. 创建 `http_server.rs`：
   - 移入 `http_server_serve_once`
   - 保持 `src/native_modules/net_http_server.rs` 调用面不变

6. 在 `src/interpreter/mod.rs` 只做模块声明和 re-export：

```rust
mod http_client;
mod http_common;
mod http_server;

pub(crate) use http_client::{http_client_request, http_stream_method_name, HttpClientHeaders};
pub(crate) use http_server::http_server_serve_once;
```

7. 验证：

```text
cargo fmt
cargo test --test http_client_headers
cargo test supports_imported_net_http_client_and_server_modules
cargo test supports_net_http_client_stream_receive
cargo test
```

**Commit:**

```text
git add src/interpreter/mod.rs src/interpreter/http_common.rs src/interpreter/http_client.rs src/interpreter/http_server.rs tests/http_client_headers.rs
git commit -m "Refactor HTTP runtime helpers out of interpreter"
```

### Worker B：WebIno 拆分和路由/响应测试补强

**Goal:** 把 WebIno 请求解析、路由、响应序列化和 server listen 逻辑拆到独立文件，并补关键行为测试。

**Files:**
- Create: `src/interpreter/web_ino.rs`
- Modify: `src/interpreter/mod.rs`
- Test: `tests/web_ino_routes.rs`
- Test: `tests/web_ino_response_headers.rs`
- Test: `tests/language.rs`
- Test: `tests/web_ino_perf.rs`

**Dependency:** 建议在 Worker A 完成后开始，避免同时移动 `http_status_text`、header helper 和 `mod.rs` 接线。

**Steps:**

1. 先补测试：
   - 404 返回 `Not Found`。
   - 精确路由优先于参数路由。
   - `%xx` 和 `+` query 解码。
   - stream 开始后禁止 `res.status`、`res.header`、`res.content_type`、`res.send`、`res.json`。
   - `res.download()` 文件名引号转义。

2. 运行当前 WebIno 测试：

```text
cargo test --test web_ino_routes
cargo test --test web_ino_response_headers
cargo test supports_std_web_ino
```

3. 创建 `src/interpreter/web_ino.rs`，迁移：
   - `web_ino_app_method`
   - `web_ino_response_method`
   - `web_ino_listen_once`
   - `web_ino_listen`
   - `web_ino_handle_request`
   - `read_web_ino_request_text`
   - `parse_web_ino_request`
   - multipart/form/query/params helper
   - stream/write/end/download helper

4. `mod.rs` 只保留分发调用：

```rust
mod web_ino;
```

5. 验证：

```text
cargo fmt
cargo test --test web_ino_routes
cargo test --test web_ino_response_headers
cargo test --test web_ino_perf -- --ignored --nocapture
cargo test
```

**Commit:**

```text
git add src/interpreter/mod.rs src/interpreter/web_ino.rs tests/web_ino_routes.rs tests/web_ino_response_headers.rs
git commit -m "Refactor WebIno runtime into interpreter module"
```

### Worker C：权限模型骨架

**Goal:** 引入运行时权限配置，默认兼容现有行为，并提供受限模式测试入口。

**Files:**
- Create: `src/runtime/permissions.rs`
- Modify: `src/runtime/mod.rs`
- Modify: `src/interpreter/mod.rs`
- Modify: `src/lib.rs`
- Test: `tests/permissions_matrix.rs`

**Dependency:** 可以和 Worker A 并行起草，但接线应在 A/B 的结构拆分后合并。

**Initial API:**

```rust
#[derive(Debug, Clone)]
pub struct RuntimePermissions {
    pub fs_read: PermissionRule,
    pub fs_write: PermissionRule,
    pub fs_list: PermissionRule,
    pub env_read: PermissionRule,
    pub os_info: PermissionRule,
    pub net_connect: PermissionRule,
    pub net_listen: PermissionRule,
}

#[derive(Debug, Clone)]
pub enum PermissionRule {
    AllowAll,
    DenyAll,
}
```

**Steps:**

1. 新增 `RuntimePermissions::allow_all()` 和 `RuntimePermissions::deny_all()`。

2. 给 `Interpreter` 增加字段：

```rust
permissions: RuntimePermissions
```

3. 增加构造方法：

```rust
pub fn with_permissions(permissions: RuntimePermissions) -> Self
```

4. `src/lib.rs` 增加受限运行入口：

```rust
pub fn run_source_with_permissions(source: &str, permissions: RuntimePermissions) -> Result<(), IcooError>
```

5. 先不拦截所有模块，只让测试能构造受限解释器，确保兼容默认不变。

6. 验证：

```text
cargo fmt
cargo test --test permissions_matrix
cargo test
```

**Commit:**

```text
git add src/runtime/permissions.rs src/runtime/mod.rs src/interpreter/mod.rs src/lib.rs tests/permissions_matrix.rs
git commit -m "Add runtime permissions skeleton"
```

### Worker D：权限接入 std.io.fs/std.env/std.os/net/WebIno download

**Goal:** 把敏感宿主能力接入权限模型，默认允许，受限模式拒绝。

**Files:**
- Modify: `src/native_modules/io_fs.rs`
- Modify: `src/native_modules/env.rs`
- Modify: `src/native_modules/os.rs`
- Modify: `src/native_modules/net_http_client.rs`
- Modify: `src/native_modules/net_http_server.rs`
- Modify: `src/interpreter/http_client.rs`
- Modify: `src/interpreter/http_server.rs`
- Modify: `src/interpreter/web_ino.rs`
- Test: `tests/permissions_matrix.rs`

**Dependency:** 必须在 Worker C 完成后开始。

**Steps:**

1. `std.io.fs`：
   - `read_text` 检查 `fs_read`
   - `write_text`/`append_text` 检查 `fs_write`
   - `list_dir` 检查 `fs_list`
   - `exists`/`is_file`/`is_dir` 暂时按 `fs_read` 处理

2. `std.env`：
   - `get`/`has` 检查 `env_read`

3. `std.os`：
   - `name`/`family`/`arch`/`pid`/`cwd`/`args`/`exe_path` 检查 `os_info`
   - `get_env`/`has_env` 检查 `env_read`

4. 网络：
   - `std.net.http.client` connect 前检查 `net_connect`
   - `std.net.http.server` bind 前检查 `net_listen`
   - `std.web.ino` listen/listen_once/listen_with_workers bind 前检查 `net_listen`

5. WebIno download：
   - `res.download(path)` 读取文件前检查 `fs_read`

6. 验证：

```text
cargo fmt
cargo test --test permissions_matrix
cargo test --test native_modules_matrix
cargo test --test http_client_headers
cargo test --test web_ino_response_headers
cargo test
```

**Commit:**

```text
git add src/native_modules src/interpreter tests/permissions_matrix.rs
git commit -m "Enforce runtime permissions for native modules"
```

### Worker E：async/EventLoop 语义加固

**Goal:** 稳住当前 async/await 语义，不扩展新异步 I/O。

**Files:**
- Modify: `src/interpreter/mod.rs`
- Modify: `src/runtime/value.rs`
- Modify: `src/typechecker.rs`
- Modify: `src/resolver.rs`
- Test: `tests/language.rs`
- Optionally create: `tests/async_event_loop.rs`

**Dependency:** 可以与 Worker C 并行，但不要和解释器拆分同时改同一代码段。建议等 A 完成后再做。

**Steps:**

1. 补测试：`run_until()` 只等待目标 task。

```python
async fn fast() -> String:
    return "done"

async fn slow() -> String:
    let delay = sleep(5000)
    await delay
    return "slow"

let loop = EventLoop(2)
let fast_task = loop.spawn(fast())
let slow_task = loop.spawn(slow())
print(loop.run_until(fast_task))
```

Expected：快速返回 `done`，不等待 `slow`。

2. 修改 `run_until(task)`：
   - 循环执行 ready/timer，直到目标 task 状态为 Done/Failed/Cancelled。
   - 目标完成即返回，不 drain 整个 loop。

3. 补测试：取消任务唤醒 awaiter。

4. 修改 `Task.cancel()`：
   - 设置 `Cancelled`
   - 唤醒 awaiters
   - `await cancelled_task` 和 `task.result()` 都返回稳定错误

5. 明确 `await` 子集：
   - 短期在 resolver 或 typechecker 中禁止复杂表达式位置的 `await`，只允许：
     - `let value = await task`
     - `await task`
     - `return await task`
   - 或者明确测试当前允许行为，保证副作用不重复。推荐先限制。

6. 验证：

```text
cargo fmt
cargo test supports_event_loop_async_functions_and_await
cargo test supports_awaiting_tasks_inside_coroutines
cargo test typechecker_tracks_async_task_result_types
cargo test
```

**Commit:**

```text
git add src/interpreter/mod.rs src/runtime/value.rs src/typechecker.rs src/resolver.rs tests/language.rs tests/async_event_loop.rs
git commit -m "Tighten async task lifecycle semantics"
```

### Worker F：WebIno 应用层能力设计和第一批实现

**Goal:** 在 WebIno 拆分完成后，补应用层框架能力：middleware、统一错误处理、404/500 handler、请求限制。

**Files:**
- Modify: `src/runtime/value.rs`
- Modify: `src/interpreter/web_ino.rs`
- Modify: `src/typechecker.rs`
- Test: `tests/web_ino_middleware.rs`
- Test: `tests/web_ino_limits.rs`
- Docs: `docs/plans/2026-05-21-icoo-language-design.md`

**Dependency:** 必须在 Worker B 完成后开始。

**MVP API Proposal:**

```python
app.use(handler)
app.not_found(handler)
app.error(handler)
app.limit_body(bytes)
```

**Handler shape:**

```python
fn middleware(req: Map<String, Any>, res: WebInoResponse):
    req.get("headers")

fn not_found(req: Map<String, Any>, res: WebInoResponse):
    res.status(404)
    res.send("custom 404")

fn error(req: Map<String, Any>, res: WebInoResponse, err: String):
    res.status(500)
    res.send(err)
```

**Steps:**

1. 先写测试：
   - middleware 在 route 前执行。
   - middleware 可以直接 `res.send()` 短路 route。
   - 未命中路由调用自定义 `not_found`。
   - handler 报错调用自定义 `error`。
   - body 超过限制返回 413。

2. 扩展 `WebInoApp`：

```rust
pub middlewares: Vec<Value>,
pub not_found_handler: Option<Value>,
pub error_handler: Option<Value>,
pub max_body_bytes: Option<usize>,
```

3. 扩展 app 方法分发和 typechecker 签名。

4. 修改 request 读取逻辑，基于 `Content-Length` 和 `max_body_bytes` 拒绝过大请求。

5. 验证：

```text
cargo fmt
cargo test --test web_ino_middleware
cargo test --test web_ino_limits
cargo test --test web_ino_perf -- --ignored --nocapture
cargo test
```

**Commit:**

```text
git add src/runtime/value.rs src/interpreter/web_ino.rs src/typechecker.rs tests/web_ino_middleware.rs tests/web_ino_limits.rs docs/plans/2026-05-21-icoo-language-design.md
git commit -m "Add WebIno middleware and request limits"
```

### Worker G：标准库签名元数据收敛

**Goal:** 减少 native module 实现、typechecker、文档之间的 API 漂移。

**Files:**
- Modify: `src/native_modules/mod.rs`
- Modify: `src/typechecker.rs`
- Modify: `tests/native_modules_matrix.rs`
- Docs: `docs/plans/2026-05-21-icoo-language-design.md`

**Dependency:** 建议在权限第一阶段后开始，避免同时改标准库注册表。

**Steps:**

1. 扩展 `NativeModuleSpec`，加入方法签名元数据：

```rust
pub struct NativeMethodSpec {
    pub name: &'static str,
    pub arity: NativeAritySpec,
    pub params: &'static [&'static str],
    pub return_type: &'static str,
}
```

2. 先只把 `std.net.http.client`、`std.web.ino`、`std.io.fs` 三个高变动模块接入。

3. `typechecker.rs` 优先从 `NativeModuleSpec` 读取签名；未迁移模块保留旧 match 逻辑。

4. `tests/native_modules_matrix.rs` 增加：
   - 每个注册方法有签名。
   - 文档表中的模块名和注册表一致。

5. 验证：

```text
cargo fmt
cargo test --test native_modules_matrix
cargo test typechecker_rejects_native_method_argument_type_mismatches
cargo test
```

**Commit:**

```text
git add src/native_modules/mod.rs src/native_modules src/typechecker.rs tests/native_modules_matrix.rs docs/plans/2026-05-21-icoo-language-design.md
git commit -m "Add native module signature metadata"
```

### Worker H：VM 设计文档和等价测试准备

**Goal:** 为未来字节码 VM 做设计准备，不改变主执行路径。

**Files:**
- Create: `docs/plans/2026-05-22-icoo-bytecode-vm-design.md`
- Create: `tests/backend_equivalence.rs`
- No production code changes unless只加入测试 helper

**Steps:**

1. 设计文档包含：
   - bytecode instruction set 草案
   - frame layout：`ip + stack + locals + env + wait_source`
   - function/class/module/native call 边界
   - async suspension/resume 模型
   - 与当前 AST backend 的迁移策略

2. 新增等价测试骨架：
   - 暂时只运行 AST backend。
   - 测试样例按同步语言子集组织，未来 VM 接入后同一批样例跑双 backend。

3. 验证：

```text
cargo fmt
cargo test --test backend_equivalence
```

**Commit:**

```text
git add docs/plans/2026-05-22-icoo-bytecode-vm-design.md tests/backend_equivalence.rs
git commit -m "Document bytecode VM migration path"
```

## 5. 推荐执行顺序

### 第一批：降低冲突和风险

可以并行：

- Worker A：HTTP 拆分
- Worker C：权限骨架
- Worker H：VM 设计文档

主线程职责：

- 只处理 `src/interpreter/mod.rs` 接线冲突。
- 每个 worker 合并后跑 `cargo test`。

### 第二批：WebIno 和 async 语义

Worker A 完成后：

- Worker B：WebIno 拆分
- Worker E：async/EventLoop 语义加固

注意：B 和 E 都会碰 `src/interpreter/mod.rs`，如果 A 已把代码拆开，冲突会小很多；否则不要并行执行。

### 第三批：能力增强

Worker B/C 完成后：

- Worker D：权限接入各标准库
- Worker F：WebIno middleware/错误处理/请求限制
- Worker G：标准库签名元数据收敛

## 6. 每日验收标准

每天结束前必须满足：

- `git status --short` 清晰，不能混入未知 worker 的半成品。
- 至少一个可独立回滚的提交。
- `cargo fmt` 通过。
- `cargo test` 通过。
- 如果改到 WebIno listen/stream/download：

```text
cargo test --test web_ino_perf -- --ignored --nocapture
```

- 文档同步：
  - 新 API 写入 `docs/plans/2026-05-21-icoo-language-design.md`
  - 架构路线写入对应 `docs/plans/2026-05-22-*.md`

## 7. 当前不做的事

- 不立即重写字节码 VM。
- 不把 `Value` 从 `Rc<RefCell>` 全面改成 `Arc<Mutex>`。
- 不宣称 WebIno handler 已经多线程并行执行。
- 不默认开启权限拒绝，避免破坏现有脚本。
- 不继续横向添加大量新标准库，先补结构、权限和测试边界。

## 8. 最小下一步

建议下一轮直接启动三名 worker：

1. Worker A：HTTP runtime 拆分。
2. Worker C：权限模型骨架。
3. Worker H：VM 设计文档和 backend equivalence 测试骨架。

主线程同时负责 WebIno middleware/limits 的测试草案，不立刻实现。第一批完成后再启动 Worker B/E，避免 `src/interpreter/mod.rs` 出现大冲突。
