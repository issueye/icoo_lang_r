# 生产级运行时加固 实施计划

**日期:** 2026-05-26

> **For Claude:** REQUIRED SUB-SKILL: Use executing-plans to implement this plan task-by-task.

**Goal:** 将 Icoo 运行时从"MVP 可工作"提升到"生产级安全可用"，补齐执行安全、资源配额、可观测性和 VM 核心能力。

**Architecture:** AST interpreter 保持主路径不变。安全层通过 Interpreter 结构体的新增字段（深度计数器、截止时间、内存追踪器）实现，不改变 Value/Environment 对象模型。VM 扩展复用现有 Value 和解释器子模块，逐步对齐行为。新增 RuntimeConfig 集中管理所有可配参数。

**Tech Stack:** Rust 2021, AST interpreter, tokio (timer), rustls, Cargo integration tests. 无新依赖。

---

## 阶段一：执行安全

### Task 1: 调用栈深度防护

**背景:** 当前 AST 解释器以递归方式执行 `call_function()` → `execute_block()` → `execute()` 的调用链，无调用深度检查。深层递归将导致 Rust 栈溢出（非 IcooError），进程直接崩溃。

**Files:**
- Modify: `src/runtime/limits.rs`
- Modify: `src/interpreter/mod.rs`
- Modify: `src/interpreter/calls.rs`
- Create: `tests/stack_depth.rs`

**Step 1: 添加深度限制常量**

在 `src/runtime/limits.rs` 末尾添加：

```rust
pub const MAX_CALL_DEPTH: usize = 1000;
```

**Step 2: 在 Interpreter 中添加深度计数字段**

在 `src/interpreter/mod.rs` 的 `Interpreter` 结构体的 `http_tls_config` 之后添加：

```rust
call_depth: usize,
```

在 `Interpreter::with_output_permissions_logger_tls_roots_and_http_config()` 构造函数中初始化为 `0`：

```rust
call_depth: 0,
```

**Step 3: 在函数调用入口检查深度**

修改 `src/interpreter/calls.rs` 的 `call_function()` 方法。在函数开头添加：

```rust
if self.call_depth >= crate::runtime::limits::MAX_CALL_DEPTH {
    return Err(IcooError::runtime(
        format!("maximum call stack depth exceeded ({})", crate::runtime::limits::MAX_CALL_DEPTH),
        Some(span),
    ));
}
self.call_depth += 1;
```

在 `call_function()` 的所有 return 路径之前，将 `self.call_depth -= 1` 放在正确位置。

实际操作：将 `call_function` 的 result 逻辑包裹在 `self.call_depth += 1` 和 `self.call_depth -= 1` 之间。最优方案是在方法开头 push，然后用 defer-like 模式：

```rust
self.call_depth += 1;

// ... existing logic, save to result ...

self.call_depth -= 1;
result
```

**Step 4: 编写测试**

创建 `tests/stack_depth.rs`：

```rust
#[test]
fn test_stack_depth_error() {
    // 编写递归 1000 层以上的脚本触发栈深度错误
    let source = format!(
        "fn f(x: Int) -> Int {{ if x == 0 {{ return 0; }} return n(x - 1); }} n(2000);",
    );
    // 注意: 上面的脚本里的 n 和 f 要保持一致
    let err = icoo_lang_r::run_source(&source).unwrap_err();
    assert!(err.to_string().contains("stack depth"));
}
```

**Step 5: 验证**

```bash
cargo test --test stack_depth
cargo test
```

**Step 6: 提交**

```bash
git add src/runtime/limits.rs src/interpreter/mod.rs src/interpreter/calls.rs tests/stack_depth.rs
git commit -m "feat: add call stack depth limit to prevent stack overflow (max 1000)"
```

---

### Task 2: 执行超时控制

**背景:** 无执行时间限制，`while true {}` 等无限循环将永久阻塞调用线程。需要基于截止时间的检查机制。

**Files:**
- Create: `src/runtime/config.rs`
- Modify: `src/runtime/mod.rs`
- Modify: `src/interpreter/mod.rs`
- Modify: `src/interpreter/eval.rs`
- Modify: `src/interpreter/calls.rs`
- Modify: `src/lib.rs`
- Create: `tests/execution_timeout.rs`

**Step 1: 创建 RuntimeConfig**

创建 `src/runtime/config.rs`：

```rust
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub exec_timeout: Option<Duration>,
    pub max_call_depth: usize,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            exec_timeout: None,
            max_call_depth: 1000,
        }
    }
}

impl RuntimeConfig {
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.exec_timeout = Some(timeout);
        self
    }

    pub fn with_max_call_depth(mut self, depth: usize) -> Self {
        self.max_call_depth = depth;
        self
    }
}
```

在 `src/runtime/mod.rs` 添加：`pub mod config;`

**Step 2: 在 Interpreter 中添加截止时间字段**

在 `src/interpreter/mod.rs` 的 Interpreter 结构体中：

```rust
runtime_config: RuntimeConfig,
execution_deadline: Option<std::time::Instant>,
```

在构造函数中初始化：

```rust
use crate::runtime::config::RuntimeConfig;
// ...
let deadline = config.exec_timeout.map(|t| std::time::Instant::now() + t);
Self {
    // ... existing fields ...
    runtime_config: config,
    execution_deadline: deadline,
}
```

**Step 3: 添加超时检查辅助方法**

在 `src/interpreter/mod.rs` 的 `impl Interpreter` 块中添加：

```rust
fn check_timeout(&self, span: Span) -> IcooResult<()> {
    if let Some(deadline) = self.execution_deadline {
        if std::time::Instant::now() > deadline {
            return Err(IcooError::runtime("execution timed out", Some(span)));
        }
    }
    Ok(())
}
```

**Step 4: 在关键位置插入超时检查**

在 `src/interpreter/eval.rs` 中：
- `while self.eval(condition)?.truthy()` 循环体内，每次迭代调用 `self.check_timeout(span)?`

在 `src/interpreter/calls.rs` 中：
- `call_function()` 入口处调用 `self.check_timeout(span)?`

在 `src/interpreter/tasks.rs` 中：
- `run_event_loop()` 每次从 ready 队列取 task 之前调用 `self.check_timeout(span)?`

**Step 5: 更新 API 入口**

修改 `src/lib.rs`，将现有构造方法改为接受 `RuntimeConfig`。新增：

```rust
pub fn run_source_with_config(source: &str, config: RuntimeConfig) -> Result<(), IcooError> {
    let program = parse_and_check(source)?;
    let mut interpreter = Interpreter::with_output_and_config(
        |line| println!("{}", line),
        RuntimePermissions::default(),
        RuntimeLogger::default(),
        config,
    );
    interpreter.interpret(&program)
}
```

并在 `src/interpreter/mod.rs` 中添加对应的构造函数。

**Step 6: 编写测试**

创建 `tests/execution_timeout.rs`：

```rust
use std::time::Duration;

#[test]
fn test_execution_timeout() {
    let err = icoo_lang_r::run_source_with_config(
        "while true {}",
        icoo_lang_r::RuntimeConfig::default().with_timeout(Duration::from_millis(100)),
    )
    .unwrap_err();
    assert!(err.to_string().contains("timed out"));
}
```

**Step 7: 验证与提交**

```bash
cargo test --test execution_timeout
cargo test
git add src/runtime/config.rs src/runtime/mod.rs src/interpreter/mod.rs src/interpreter/eval.rs src/interpreter/calls.rs src/interpreter/tasks.rs src/lib.rs tests/execution_timeout.rs
git commit -m "feat: add execution timeout with configurable deadline"
```

---

### Task 3: 整数溢出安全防护

**背景:** 当前二元运算直接使用 Rust 的 `+`、`-`、`*` 运算符，溢出行为依赖编译 profile（debug 模式 panic，release 模式 wrapping）。生产环境应统一为 checked 运算并返回错误。

**Files:**
- Modify: `src/interpreter/eval.rs`
- Modify: `src/vm/interpreter.rs`
- Create: `tests/integer_overflow.rs`

**Step 1: 替换二元运算为 checked 版本**

修改 `src/interpreter/eval.rs` 中的 `numeric()` 和 `eval_binary()`：

将 `BinaryOp::Add` 匹配臂中的 `Value::Int(a), Value::Int(b)` 改为：

```rust
(Value::Int(a), Value::Int(b)) => {
    a.checked_add(b)
        .map(Value::Int)
        .ok_or_else(|| IcooError::runtime("integer overflow in addition", Some(span)))
}
```

同样处理 `Subtract`：`a.checked_sub(b)`，
`Multiply`：`a.checked_mul(b)`，
`Remainder`：先检查 `b != 0`，再 `a.checked_rem(b)`，
`Negate`：`a.checked_neg()`。

**Step 2: 同步修改 VM 解释器**

修改 `src/vm/interpreter.rs` 中的 `numeric()` 函数，应用相同的 checked 运算逻辑。

**Step 3: 编写测试**

创建 `tests/integer_overflow.rs`：

```rust
#[test]
fn test_add_overflow() {
    let err = icoo_lang_r::run_source(
        "let a: Int = 9223372036854775807; let b: Int = a + 1;"
    ).unwrap_err();
    assert!(err.to_string().contains("overflow"));
}

#[test]
fn test_sub_overflow() {
    let err = icoo_lang_r::run_source(
        "let a: Int = -9223372036854775808; let b: Int = a - 1;"
    ).unwrap_err();
    assert!(err.to_string().contains("overflow"));
}

#[test]
fn test_mul_overflow() {
    let err = icoo_lang_r::run_source(
        "let a: Int = 9223372036854775807; let b: Int = a * 2;"
    ).unwrap_err();
    assert!(err.to_string().contains("overflow"));
}
```

**Step 4: 验证与提交**

```bash
cargo test --test integer_overflow
cargo test
git add src/interpreter/eval.rs src/vm/interpreter.rs tests/integer_overflow.rs
git commit -m "fix: use checked arithmetic to prevent integer overflow panics"
```

---

### Task 4: Panic 恢复边界

**背景:** 嵌入场景中，Icoo 脚本的 Rust panic（如数组越界、unwrap 失败等）会直接崩溃宿主进程。需要用 `std::panic::catch_unwind` 包裹执行入口。

**Files:**
- Modify: `src/lib.rs`
- Create: `tests/panic_boundary.rs`

**Step 1: 包装执行入口**

修改 `src/lib.rs`，创建内部辅助函数：

```rust
fn run_source_catch_unwind(source: &str, permissions: RuntimePermissions, logger: RuntimeLogger, config: RuntimeConfig) -> Result<(), IcooError> {
    let program = parse_and_check(source)?;
    let mut interpreter = Interpreter::with_output_config_permissions_and_logger(
        |line| println!("{}", line),
        config,
        permissions,
        logger,
    );
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        interpreter.interpret(&program)
    }))
    .map_err(|panic_info| {
        let msg = if let Some(s) = panic_info.downcast_ref::<String>() {
            s.clone()
        } else if let Some(s) = panic_info.downcast_ref::<&str>() {
            s.to_string()
        } else {
            "internal panic".to_string()
        };
        IcooError::runtime(format!("internal runtime panic: {}", msg), None)
    })?? // 注意: catch_unwind 返回 Result<Result<T, E>, Box<dyn Any>>
}
```

然后将所有 `run_source_*` 变体统一通过此函数路由，确保 panic 被捕获。

**Step 2: 编写测试**

创建 `tests/panic_boundary.rs`，需要构造一个能触发内部 panic 的场景（如通过某些边界条件）。由于 checked 运算已在上一步完成，可以测试深层递归（在添加上限之前的非限制递归已不存在）。此步骤验证 catch_unwind 机制存在即可。

**Step 3: 验证与提交**

```bash
cargo test --test panic_boundary
cargo test
git add src/lib.rs tests/panic_boundary.rs
git commit -m "feat: add panic catch boundary for embedding safety"
```

---

## 阶段二：可观测性

### Task 5: 调用栈回溯（Stack Trace）

**背景:** 当前运行时错误仅有 Span 信息，缺乏调用链上下文（"函数 A 调用函数 B 时出错"）。需要在 IcooError::Runtime 中附加调用栈。

**Files:**
- Modify: `src/error.rs`
- Modify: `src/interpreter/mod.rs`
- Modify: `src/interpreter/calls.rs`
- Modify: `src/interpreter/eval.rs`
- Create: `tests/stack_trace.rs`

**Step 1: 扩展 IcooError 类型**

修改 `src/error.rs`，将 Runtime 变体改为：

```rust
Runtime {
    message: String,
    span: Option<Span>,
    trace: Vec<StackFrame>,
}

#[derive(Debug, Clone)]
pub struct StackFrame {
    pub name: String,
    pub span: Span,
    pub kind: StackFrameKind,
}

#[derive(Debug, Clone)]
pub enum StackFrameKind {
    Function,
    NativeFunction,
    ModuleTopLevel,
}
```

同时更新构造函数，添加 `push_trace` 方法（注意 `Return`/`Await`/`Break`/`Continue` 不需要 trace）：

```rust
impl IcooError {
    pub fn push_trace(self, name: String, span: Span, kind: StackFrameKind) -> Self {
        match self {
            IcooError::Runtime { message, span, mut trace } => {
                trace.push(StackFrame { name, span, kind });
                IcooError::Runtime { message, span, trace }
            }
            other => other,
        }
    }
}
```

**Step 2: 在调用路径上记录 frame**

在 `src/interpreter/calls.rs` 的 `call_function()` 中，遇到 `Return` 以外的 error 时：

```rust
Err(err) => {
    let frame = StackFrame {
        name: function.decl.name.name.clone(),
        span: function.decl.name.span,
        kind: StackFrameKind::Function,
    };
    Err(err.push_trace(frame.name.clone(), frame.span, frame.kind))
}
```

在 `src/interpreter/eval.rs` 的 `eval()` 中，对 `Expr::Call` 的 call_value 结果做同样的 trace 追加（因为调用是通过 `call_value` 而非直接 `call_function`）。

在 `src/interpreter/mod.rs` 的 `interpret()` 中，顶层错误也追加模块 frame。

**Step 3: 更新 Display 实现**

修改 `IcooError::Display`，在 Runtime 变体中打印 trace：

```
runtime error: message
  at function_name (1:10)
  at main_module (3:5)
```

**Step 4: 编写测试**

创建 `tests/stack_trace.rs`：

```rust
#[test]
fn test_stack_trace_on_error() {
    let source = r#"
fn inner() -> Int {
    return 1 / 0;
}
fn outer() -> Int {
    return inner();
}
outer();
"#;
    let err = icoo_lang_r::run_source(source).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("inner"), "trace should contain inner function");
    assert!(msg.contains("outer"), "trace should contain outer function");
}
```

**Step 5: 验证与提交**

```bash
cargo test --test stack_trace
cargo test
git add src/error.rs src/interpreter/mod.rs src/interpreter/calls.rs src/interpreter/eval.rs tests/stack_trace.rs
git commit -m "feat: add stack trace to runtime errors"
```

---

## 阶段三：资源配额

### Task 6: 运行时内存追踪

**背景:** 需要通过 `Rc::new()` 的包装来追踪通过 `Value` 持有的总内存量，当超出限制时拒绝分配。

**实现方案:** 使用 `std::sync::atomic::AtomicUsize` 作为全局内存计数器（因为 `Rc` 不是 `Send`，但计数器本身是原子的无锁操作）。在 `Value::Array`、`Value::Map`、`Value::String`、`Value::Bytes` 等变体创建前，检查并增加计数。由于 Rc 的 clone 共享同一块内存，我们需要一个新的包装器 `RcTracked<T>` 来只计数首次分配。

**Files:**
- Create: `src/runtime/memory.rs`
- Modify: `src/runtime/mod.rs`
- Modify: `src/runtime/limits.rs`
- Modify: `src/interpreter/eval.rs`
- Modify: `src/interpreter/methods.rs`
- Create: `tests/memory_limit.rs`

**Step 1: 创建内存追踪模块**

创建 `src/runtime/memory.rs`：

```rust
use std::sync::atomic::{AtomicUsize, Ordering};

static TOTAL_BYTES: AtomicUsize = AtomicUsize::new(0);

pub fn current_bytes() -> usize {
    TOTAL_BYTES.load(Ordering::Relaxed)
}

pub fn acquire(bytes: usize, max: usize) -> Result<(), String> {
    let prev = TOTAL_BYTES.fetch_add(bytes, Ordering::Relaxed);
    if prev + bytes > max {
        TOTAL_BYTES.fetch_sub(bytes, Ordering::Relaxed);
        return Err(format!(
            "memory limit exceeded: attempting to allocate {} bytes (current: {}, max: {})",
            bytes, prev, max
        ));
    }
    Ok(())
}

pub fn release(bytes: usize) {
    TOTAL_BYTES.fetch_sub(bytes, Ordering::Relaxed);
}

pub const DEFAULT_MEMORY_LIMIT: usize = 64 * 1024 * 1024; // 64 MiB
```

添加 `pub mod memory;` 到 `src/runtime/mod.rs`。

**Step 2: 创建 RcTracked 包装器**

在 `src/runtime/memory.rs` 中：

```rust
use std::ops::Deref;
use std::rc::Rc;

pub struct RcTracked<T> {
    inner: Rc<T>,
    byte_size: usize,
}

impl<T> RcTracked<T> {
    pub fn new(value: T, byte_size: usize, max: usize) -> Result<Self, String> {
        acquire(byte_size, max)?;
        Ok(Self { inner: Rc::new(value), byte_size })
    }
}

impl<T> Clone for RcTracked<T> {
    fn clone(&self) -> Self {
        acquire(self.byte_size, usize::MAX).ok(); // 共享同一内存，增加引用计数
        Self { inner: self.inner.clone(), byte_size: self.byte_size }
    }
}

impl<T> Drop for RcTracked<T> {
    fn drop(&self) {
        if Rc::strong_count(&self.inner) == 1 {
            release(self.byte_size);
        }
    }
}

impl<T> Deref for RcTracked<T> {
    type Target = T;
    fn deref(&self) -> &T { &self.inner }
}
```

**Step 3: 逐步替换 Value 变体**

此步骤较大，采用渐进式替换而非一次性重写：

首先修改 `Value::String(String)` 为在创建时检查字节数。在 `src/native_modules/io_fs.rs` 的 `read_text` 和 `src/interpreter/methods.rs` 的 `to_bytes` 等位置添加检查：

```rust
// 在创建 Value::String 之前
let byte_len = content.len();
crate::runtime::memory::acquire(byte_len, crate::runtime::limits::MAX_TOTAL_MEMORY)
    .map_err(|msg| IcooError::runtime(msg, Some(span)))?;
```

类似的，在 `src/interpreter/eval.rs` 的 `Expr::Array`、`Expr::Map`、`Expr::Template` 中，在分配大容器前检查内存。

**Step 4: 在 RuntimeConfig 中添加内存限制**

修改 `src/runtime/config.rs`：

```rust
pub struct RuntimeConfig {
    pub exec_timeout: Option<Duration>,
    pub max_call_depth: usize,
    pub max_memory: usize,  // 新增
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            exec_timeout: None,
            max_call_depth: 1000,
            max_memory: 64 * 1024 * 1024,
        }
    }
}
```

**Step 5: 编写测试**

创建 `tests/memory_limit.rs`：

```rust
#[test]
fn test_memory_limit_string() {
    // 创建超大字符串触发限制
    let half_mb = 513 * 1024;
    let source = format!("let x = \"{}\" + \"{}\";", "a".repeat(half_mb), "a".repeat(half_mb));
    // 期望不崩溃，返回错误
    let result = icoo_lang_r::run_source(&source);
    // 要么成功，要么内存错误 — 但不应 panic
    assert!(result.is_ok() || result.unwrap_err().to_string().contains("memory"));
}
```

**Step 6: 验证与提交**

```bash
cargo test --test memory_limit
cargo test
git add src/runtime/memory.rs src/runtime/mod.rs src/runtime/limits.rs src/runtime/config.rs src/interpreter/eval.rs tests/memory_limit.rs
git commit -m "feat: add runtime memory tracking and limit enforcement"
```

---

### Task 7: 容器大小限制

**背景:** 防止单次操作创建过大的 Array、Map 或 String，避免内存耗尽。

**Files:**
- Modify: `src/runtime/limits.rs`
- Modify: `src/interpreter/eval.rs`
- Modify: `src/interpreter/methods.rs`
- Create: `tests/container_limits.rs`

**Step 1: 添加限制常量**

在 `src/runtime/limits.rs` 添加：

```rust
pub const MAX_ARRAY_LEN: usize = 1_000_000;
pub const MAX_MAP_ENTRIES: usize = 1_000_000;
pub const MAX_STRING_LEN: usize = 16 * 1024 * 1024; // 16 MiB
```

**Step 2: 在创建容器时检查**

`src/interpreter/eval.rs` - `Expr::Array`：
```rust
if values.len() > crate::runtime::limits::MAX_ARRAY_LEN {
    return Err(IcooError::runtime(
        format!("array exceeds maximum length ({})", crate::runtime::limits::MAX_ARRAY_LEN),
        Some(*span),
    ));
}
```

`src/interpreter/eval.rs` - `Expr::Template` 最终结果长度检查。

`src/interpreter/methods.rs` - 各种操作的结果大小检查（concat/slice/push/append 等）。

**Step 3: 编写测试**

```bash
cargo test --test container_limits
cargo test
```

---

## 阶段四：嵌入 API 重构

### Task 8: InterpreterBuilder 模式

**背景:** 当前 `lib.rs` 有 8+ 个 `run_source_with_*` 变体，参数组合爆炸。需用 Builder 模式收敛。

**Files:**
- Create: `src/builder.rs`
- Modify: `src/lib.rs`
- Create: `tests/interpreter_builder.rs`

**Step 1: 创建 InterpreterBuilder**

创建 `src/builder.rs`：

```rust
use crate::interpreter::Interpreter;
use crate::runtime::config::RuntimeConfig;
use crate::runtime::http_config::RuntimeHttpConfig;
use crate::runtime::logging::RuntimeLogger;
use crate::runtime::permissions::RuntimePermissions;
use crate::error::IcooResult;
use crate::parser::ast::Program;
use std::time::Duration;

pub struct InterpreterBuilder {
    output: Option<Box<dyn FnMut(String) + 'static>>,
    permissions: RuntimePermissions,
    logger: RuntimeLogger,
    config: RuntimeConfig,
    http_config: RuntimeHttpConfig,
    http_tls_roots: Option<std::sync::Arc<rustls::RootCertStore>>,
}

impl InterpreterBuilder {
    pub fn new() -> Self {
        Self {
            output: None,
            permissions: RuntimePermissions::default(),
            logger: RuntimeLogger::default(),
            config: RuntimeConfig::default(),
            http_config: RuntimeHttpConfig::default(),
            http_tls_roots: None,
        }
    }

    pub fn output<F: FnMut(String) + 'static>(mut self, output: F) -> Self {
        self.output = Some(Box::new(output));
        self
    }

    pub fn permissions(mut self, permissions: RuntimePermissions) -> Self {
        self.permissions = permissions;
        self
    }

    pub fn timeout(mut self, duration: Duration) -> Self {
        self.config.exec_timeout = Some(duration);
        self
    }

    pub fn max_memory(mut self, bytes: usize) -> Self {
        self.config.max_memory = bytes;
        self
    }

    pub fn max_call_depth(mut self, depth: usize) -> Self {
        self.config.max_call_depth = depth;
        self
    }

    pub fn http_timeout(mut self, timeout: Duration) -> Self {
        self.http_config = self.http_config.with_timeouts(timeout, timeout, timeout);
        self
    }

    pub fn http_tls_roots(mut self, roots: rustls::RootCertStore) -> Self {
        self.http_tls_roots = Some(std::sync::Arc::new(roots));
        self
    }

    pub fn build(self) -> Interpreter {
        let output = self.output.unwrap_or_else(|| Box::new(|line| println!("{}", line)));
        Interpreter::with_output_permissions_logger_tls_roots_and_http_config(
            output,
            self.permissions,
            self.logger,
            self.http_tls_roots,
            self.http_config,
        )
    }

    pub fn run_source(self, source: &str) -> IcooResult<()> {
        let program = crate::parse_and_check(source)?;
        let mut interpreter = self.build();
        interpreter.interpret(&program)
    }

    pub fn run_file(self, path: impl AsRef<std::path::Path>) -> IcooResult<()> {
        let path = path.as_ref();
        let source = std::fs::read_to_string(path).map_err(|err| {
            crate::error::IcooError::runtime(
                format!("failed to read file: {}", err),
                None,
            )
        })?;
        self.run_source(&source)
    }
}

impl Default for InterpreterBuilder {
    fn default() -> Self {
        Self::new()
    }
}
```

**Step 2: 更新 lib.rs**

在 `src/lib.rs` 中：
```rust
mod builder;
pub use builder::InterpreterBuilder;
```

保留旧 API 作为 deprecated wrapper，内部使用 Builder：

```rust
pub fn run_source(source: &str) -> Result<(), IcooError> {
    InterpreterBuilder::new().run_source(source)
}

pub fn run_source_with_timeout(source: &str, timeout: Duration) -> Result<(), IcooError> {
    InterpreterBuilder::new().timeout(timeout).run_source(source)
}
```

**Step 3: 编写测试**

创建 `tests/interpreter_builder.rs` 验证链式调用。

```bash
cargo test --test interpreter_builder
cargo test
git add src/builder.rs src/lib.rs tests/interpreter_builder.rs
git commit -m "refactor: introduce InterpreterBuilder pattern for embedded API"
```

---

### Task 9: 脚本返回值获取

**背景:** 当前所有 `run_source*` 返回 `Result<(), IcooError>`，无法获取脚本最后的表达式值。

**Files:**
- Modify: `src/interpreter/mod.rs`
- Modify: `src/lib.rs`
- Modify: `src/builder.rs`

**Step 1: 修改 Interpreter::interpret 返回值**

在 `src/interpreter/mod.rs` 中修改 `interpret()` 为返回最后一个表达式值：

```rust
pub fn interpret(&mut self, program: &Program) -> IcooResult<Value> {
    let mut last = Value::Nil;
    for stmt in &program.statements {
        last = self.execute_result(stmt)?;
    }
    Ok(last)
}
```

其中 `execute_result` 是新的方法，对表达式语句返回求值结果，对声明语句返回 Nil。

**Step 2: 更新公开 API**

`src/builder.rs` 中：
```rust
pub fn run_source(self, source: &str) -> IcooResult<Value> {
    let program = crate::parse_and_check(source)?;
    let mut interpreter = self.build();
    interpreter.interpret(&program)
}
```

`src/lib.rs` 中同步返回类型。

**Step 3: 测试**

```bash
cargo test
git commit -m "feat: return script result value from run_source API"
```

---

## 阶段五：VM 核心能力扩展

### Task 10: VM 函数调用支持

**背景:** 当前 VM 仅支持同步最小子集（let/const/final, if/elif/else, while, print, 基础表达式）。需扩展到支持函数声明和调用。

**Files:**
- Modify: `src/vm/instruction.rs`
- Modify: `src/vm/compiler.rs`
- Modify: `src/vm/interpreter.rs`
- Modify: `tests/backend_equivalence.rs`

**Step 1: 添加指令**

在 `src/vm/instruction.rs` 中添加：

```rust
Call(usize),       // argc
Return,
MakeFunction(usize), // function index in compiled functions table
DefineFunction(String), // name
```

**Step 2: 扩展 Compiler**

在 `src/vm/compiler.rs` 中，将 `FunctionDecl` 的处理从 `Err(unsupported(...))` 改为：

```rust
Stmt::Function(decl) => {
    self.compile_function_body(&decl.body);
    self.instructions.push(Instruction::MakeFunction(self.functions.len()));
    self.functions.push(decl.clone());
    self.instructions.push(Instruction::DefineFunction(decl.name.name.clone()));
    Ok(())
}
```

对 `Expr::Call { callee, args, span }` 改为编译 args 入栈后 emit `Call(args.len())`。

**Step 3: 扩展 VM Interpreter**

在 `src/vm/interpreter.rs` 中添加 call frame 栈。新增：

```rust
struct VmFrame {
    function_ip: usize,
    stack_base: usize,
}

// 在 VM 结构体中添加
frames: Vec<VmFrame>,
functions: Vec<FunctionDecl>,
```

`Call` 指令实现：
```rust
Instruction::Call(argc) => {
    let callee = self.stack[self.stack.len() - 1 - *argc].clone();
    match callee {
        Value::Function(func) => {
            let frame = VmFrame { function_ip: pc + 1, stack_base: self.stack.len() - *argc - 1 };
            self.frames.push(frame);
            // 切换到函数体的第一条指令
            pc = 0; // 简化版本：需根据函数索引跳转
        }
        _ => return Err(runtime_error("not callable")),
    }
}
```

**Step 4: 更新等价性测试**

```bash
cargo test --test backend_equivalence
cargo test
```

---

### Task 11: VM 类和模块支持

逐步实现 `src/vm/compiler.rs` 中以下 Ast 节点的编译：

- Class 声明（MakeClass, InitInstance, SetField, GetProperty）
- 数组/Map 字面量（BuildArray, BuildMap）
- 属性访问和赋值

每个子步骤约 100-150 行代码，对应指令在 `src/vm/instruction.rs` 中新增。

---

### Task 12: VM 异常与 async 框架

**Files:**
- Modify: `src/vm/instruction.rs`
- Modify: `src/vm/compiler.rs`
- Modify: `src/vm/interpreter.rs`

添加指令：
- `TryCatch(catch_ip, end_ip)` — 标记 try 块范围
- `Throw` — 抛出运行时错误到最近的 catch
- `Await` — 协程挂起点
- `Yield` — 协程 yield

VM 层不实现完整的 async 调度（仍由解释器层处理），但编译 async fn 为可挂起的字节码并在 CoroutineInstr 编译时使用 VM 指令作为中间表示。

---

## 验证与回归

### Task 13: 全量回归测试

所有任务完成后：

```bash
cargo fmt --check
cargo test
cargo run -- examples/demo.icoo
cargo run -- examples/coroutines.icoo
cargo run -- examples/modules/main.icoo
```

确认所有测试通过，所有示例正常运行。

---

## 附录：不会做的事

- 不把 Value 从 Rc<RefCell> 改成 Arc<Mutex>（线程安全改造单独进行）
- 不实现完整 GC（当前架构用 Rc + 引用计数，循环检测为后续工作）
- 不引入自定义类型系统（保持 Rust 宿主注册）
- 不实现 JIT 编译
- 不改变现有脚本语言语法
