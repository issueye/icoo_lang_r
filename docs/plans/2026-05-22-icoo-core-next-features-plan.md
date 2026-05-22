# Icoo 核心功能下一阶段计划

**Goal:** 暂停 Web 框架主线，把下一阶段开发重心转向语言核心、运行时安全、开发工具和标准库一致性。

**Architecture:** 保持当前 AST backend 稳定运行；不继续扩展 WebIno 应用层能力，Web 只保留安全和回归修复。优先补齐权限、CLI、错误处理和标准库元数据，让语言本身更像一个可长期维护的脚本语言。

**Tech Stack:** Rust 2021、当前 AST interpreter、现有 resolver/typechecker/interpreter pipeline、Cargo integration tests。

---

## 1. 当前评估

当前没有阻塞性大问题。

已完成的关键基础：

- 语言核心已有变量、常量、`final`、函数、类、继承、显式属性、类型标注、数组/Map、字符串模板、多行字符串。
- 模块系统已具备本地文件导入、导出、循环依赖拒绝和 `std.*` 内置库导入。
- async/EventLoop 已修正 `run_until`、取消唤醒、跨 loop 校验和复杂表达式 await 限制。
- HTTP/WebIno 已拆出独立 runtime 文件，WebIno 作为 MVP 足够使用。
- 已有权限模型骨架、VM 设计文档和 backend equivalence 测试骨架。

当前主要风险：

- 权限模型只接入了解释器构造，还没有真正拦截 `fs/env/os/net`。
- `NativeModuleSpec` 只有方法名，类型检查器和文档仍需要手工同步。
- CLI 只有 `icoo <file.icoo>`，缺少 `check`、`run`、诊断输出等开发工具能力。
- 错误处理还是运行时错误为主，用户脚本缺少 `try/catch` 或显式错误值模型。
- VM 还只是设计，不能投入主路径；短期不应强推迁移。

结论：下一阶段不要继续在 Web 框架上投入功能开发。WebIno 只处理安全、性能回归和必要兼容；新功能优先进入语言、标准库、工具链。

## 2. 明确暂停的 Web 工作

暂缓以下 WebIno 功能：

- middleware
- router group
- 自定义 404/500 handler
- 更完整 Express 风格 API
- 长连接/SSE 框架封装
- Web 框架层性能优化

仅保留：

- 安全修复，例如权限绕过、路径穿越、请求大小限制。
- 已有 API 回归测试。
- 明确阻塞其他核心功能的拆分或接口调整。

## 3. 下一阶段优先级

### P0：权限接入和运行时安全

**Why:** 已有 `RuntimePermissions` 骨架，但还没有真正生效。脚本语言如果要嵌入或运行不可信脚本，必须先有安全边界。

**Scope:**

- `std.io.fs`
- `std.env`
- `std.os`
- `std.net.http.client`
- `std.net.http.server`
- `std.web.ino` 的 `listen` 和 `download` 仅作为权限边界补丁处理，不扩展 Web 功能。

**Expected behavior:**

- 默认 `RuntimePermissions::allow_all()` 保持现有兼容行为。
- `RuntimePermissions::deny_all()` 能稳定拒绝文件、环境、OS 信息、网络 connect/listen。
- 错误信息明确，例如：

```text
permission denied: fs.read
permission denied: net.listen
```

**Files:**

- Modify: `src/runtime/permissions.rs`
- Modify: `src/interpreter/mod.rs`
- Modify: `src/interpreter/http_client.rs`
- Modify: `src/interpreter/http_server.rs`
- Modify: `src/interpreter/web_ino.rs`
- Modify: `src/native_modules/io_fs.rs`
- Modify: `src/native_modules/env.rs`
- Modify: `src/native_modules/os.rs`
- Modify: `src/native_modules/net_http_client.rs`
- Modify: `src/native_modules/net_http_server.rs`
- Test: `tests/permissions_matrix.rs`

**Verification:**

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
git commit -m "Enforce runtime permissions for native capabilities"
```

### P1：CLI 工具链

**Why:** 当前 CLI 只能直接运行文件。下一阶段要让语言可用、可检查、可嵌入，需要最小工具链。

**MVP CLI:**

```text
icoo run <file.icoo>
icoo check <file.icoo>
icoo --version
icoo --help
```

兼容旧用法：

```text
icoo <file.icoo>
```

等价于：

```text
icoo run <file.icoo>
```

**Behavior:**

- `run`：完整执行。
- `check`：只执行 lex/parse/resolve/typecheck，不运行解释器。
- `--help`：列出命令和示例。
- `--version`：输出 Cargo package version。

**Files:**

- Modify: `src/main.rs`
- Modify: `src/lib.rs`
- Test: `tests/cli.rs`

**Library API:**

```rust
pub fn check_source(source: &str) -> Result<(), IcooError>
pub fn check_file(path: impl AsRef<Path>) -> Result<(), IcooError>
```

**Verification:**

```text
cargo fmt
cargo test --test cli
cargo test
```

**Commit:**

```text
git commit -m "Add check and run CLI commands"
```

### P2：标准库签名元数据收敛

**Why:** 目前 native module 的方法列表、类型检查签名、文档说明分散维护，后续会持续漂移。

**Scope:**

- 扩展 `NativeModuleSpec`
- 增加 `NativeMethodSpec`
- 先迁移高变动模块：
  - `std.io.fs`
  - `std.net.http.client`
  - `std.web.ino`
  - `std.os`

**Proposed structures:**

```rust
pub struct NativeMethodSpec {
    pub name: &'static str,
    pub arity: NativeAritySpec,
    pub params: &'static [&'static str],
    pub return_type: &'static str,
}

pub enum NativeAritySpec {
    Exact(usize),
    Range { min: usize, max: usize },
    AtLeast(usize),
}
```

**Expected outcome:**

- `native_modules::has_method` 读取方法 spec。
- `typechecker.rs` 优先读取 spec。
- 未迁移模块允许临时保留旧 match。
- `tests/native_modules_matrix.rs` 校验每个模块方法都有签名元数据。

**Files:**

- Modify: `src/native_modules/mod.rs`
- Modify: `src/native_modules/*.rs`
- Modify: `src/typechecker.rs`
- Modify: `tests/native_modules_matrix.rs`

**Verification:**

```text
cargo fmt
cargo test --test native_modules_matrix
cargo test typechecker_rejects_native_method_argument_type_mismatches
cargo test
```

**Commit:**

```text
git commit -m "Add native module signature metadata"
```

### P3：脚本错误处理模型

**Why:** 当前脚本只能被运行时错误打断，用户代码不能优雅处理失败。作为脚本语言，需要基本错误处理。

**Decision to make first:**

选择一种模型：

1. `try/catch` 语句模型。
2. `Result<T>` 值模型。
3. 两者都支持，但先实现一个。

**Recommended MVP:** `try/catch`，因为当前解释器已经有内部错误传播机制，落地成本更低。

**Syntax proposal:**

```python
try:
    risky()
catch err:
    print(err.to_string())
```

**Optional later:**

```python
raise "message"
```

**Files:**

- Modify: `src/lexer/token.rs`
- Modify: `src/lexer/mod.rs`
- Modify: `src/parser/ast.rs`
- Modify: `src/parser/mod.rs`
- Modify: `src/resolver.rs`
- Modify: `src/typechecker.rs`
- Modify: `src/interpreter/mod.rs`
- Test: `tests/error_handling.rs`

**Verification:**

```text
cargo fmt
cargo test --test error_handling
cargo test
```

**Commit:**

```text
git commit -m "Add try catch error handling"
```

### P4：二进制 I/O 和 Buffer 类型设计

**Why:** 当前文件、HTTP、上传/下载大量内容都以字符串为主，不适合二进制脚本场景。

**Do not implement immediately. First design.**

**Design questions:**

- 是否新增 `Bytes`/`Buffer` 类型？
- `String` 与 `Bytes` 如何转换？
- `std.io.fs.read_bytes()` 返回什么？
- HTTP client body 是否支持 bytes？
- `to_string()` 对 bytes 如何展示？

**Files:**

- Create: `docs/plans/2026-05-22-icoo-bytes-buffer-design.md`
- Optional Test Skeleton: `tests/bytes_design_placeholder.rs`

**Commit:**

```text
git commit -m "Document bytes buffer design"
```

### P5：VM 原型，只做同步子集

**Why:** VM 是长期方向，但现在不能替换主执行路径。

**Scope:**

- 新建 `src/vm/`
- 只支持：
  - literal
  - arithmetic
  - let binding
  - if/while
  - function call 的最小子集
- 不支持：
  - class
  - module
  - async
  - native module
  - WebIno

**Files:**

- Create: `src/vm/mod.rs`
- Create: `src/vm/instruction.rs`
- Create: `src/vm/compiler.rs`
- Create: `src/vm/interpreter.rs`
- Modify: `src/lib.rs`
- Extend: `tests/backend_equivalence.rs`

**Verification:**

```text
cargo fmt
cargo test --test backend_equivalence
cargo test
```

**Commit:**

```text
git commit -m "Prototype bytecode VM for sync subset"
```

## 4. 推荐执行顺序

第一批：

1. P0 权限接入和运行时安全。
2. P1 CLI `run/check/help/version`。

第二批：

3. P2 标准库签名元数据收敛。
4. P3 错误处理模型。

第三批：

5. P4 Bytes/Buffer 设计。
6. P5 VM 同步子集原型。

## 5. 暂不做

- 不继续扩展 WebIno middleware/router。
- 不做包管理器。
- 不做完整 formatter。
- 不重写全部 VM。
- 不把运行时全面改成 `Arc<Mutex>`。
- 不加入大型新标准库。

## 6. 最小下一步

下一步建议直接做 P0 和 P1：

- 一个 worker 做权限接入。
- 一个 worker 做 CLI `run/check`。
- 主线程负责审查错误信息和兼容性。

这两项完成后，语言会从“能跑脚本”进到“可以更安全地跑脚本、可以先检查脚本”的阶段，收益比继续加 Web 框架 API 更大。
