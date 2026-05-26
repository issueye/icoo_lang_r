# Icoo 核心功能下一阶段计划

**Status:** 2026-05-22 校准版。

**Goal:** 在现有 AST backend 稳定运行的基础上，把开发重心从“补 MVP 能力”切到“降低解释器复杂度、补齐二进制/权限边界、沉淀工具链和 VM 迁移路径”。

**Architecture:** 主路径继续使用 AST interpreter；VM 只作为同步子集原型和未来迁移验证工具。WebIno 保持 MVP 能力，除安全、二进制兼容和回归修复外，不继续扩展框架层 API。

**Tech Stack:** Rust 2021、AST interpreter、Tokio timer backend、`std.*` native modules、Cargo integration tests。

---

## 1. 当前评估

当前没有阻塞性大问题，完整 `cargo test` 通过；`web_ino_perf` 仍是手动 ignored 性能测试。

已完成的关键基础：

- 语言核心已有变量、常量、`final`、函数、闭包、类、继承、显式属性、类型标注、数组/Map、字符串模板、多行字符串。
- 模块系统已具备本地文件导入、导出、循环依赖拒绝和 `std.*` 内置库导入。
- async/EventLoop 已修正 `run_until`、取消唤醒、跨 loop 校验和复杂表达式 `await` 限制。
- HTTP client/server 和 WebIno runtime 已从解释器主体拆到独立文件。
- 模块加载、相对导入、export 收集已从解释器主体拆到 `src/interpreter/modules.rs`。
- 函数调用、native function/module/method 调用和返回值检查已从解释器主体拆到 `src/interpreter/calls.rs`。
- 类声明、实例创建、字段访问、字段赋值和 `super` 查找已从解释器主体拆到 `src/interpreter/classes.rs`。
- EventLoop/Task 方法、运行循环、timer/awaiter 唤醒和协程恢复执行已从解释器主体拆到 `src/interpreter/tasks.rs`。
- String/Bytes/Array/Map 内置方法和 native method 可用性判断已从解释器主体拆到 `src/interpreter/methods.rs`。
- 核心语句执行、表达式求值、赋值、二元运算和控制流已从解释器主体拆到 `src/interpreter/eval.rs`。
- 协程函数体到 `CoroutineInstr` 的编译逻辑已从解释器主体拆到 `src/interpreter/coroutines.rs`。
- 运行时类型检查和值比较 helper 已拆到 `src/interpreter/types.rs`。
- 参数提取、arity、索引和时间 helper 已拆到 `src/interpreter/args.rs`。
- JSON/TOML/YAML 值转换 helper 已拆到 `src/interpreter/formats.rs`。
- `RuntimePermissions` 已接入 `std.io.fs`、`std.env`、`std.os`、`std.net.http.*`、WebIno listen/download 等宿主能力边界。
- CLI 已支持 `icoo run`、`icoo check`、旧式 `icoo <file.icoo>`、`--help` 和 `--version`。
- `NativeModuleSpec`/`NativeMethodSpec` 已用于标准库方法元数据和类型检查。
- 脚本已支持 `try/catch`。
- `Bytes` 已有第一阶段实现，覆盖字符串编码、基础方法、Base64/Hex 构造、`std.io.fs` 二进制读写和 HTTP client bytes 场景。
- `Buffer` 已有最小可变构建器实现，覆盖 `new/from_bytes/from_string/append/append_string/slice/to_bytes/clear` 和快照语义。
- VM 已有同步子集原型和 backend equivalence 测试。

当前主要风险：

- `src/interpreter/mod.rs` 已收敛为解释器构造、native 安装和模块接线，后续风险主要转向功能边界而不是单文件膨胀。
- `Bytes`/`Buffer` 尚未完成设计文档中的全部范围：HTTP client bytes streaming、Bytes、HTTP response/stream chunk 和 WebIno request 大小限制已落地；WebIno bytes streaming 已有 response 侧能力，request 侧 streaming file API 仍待设计。
- 权限模型仍是 coarse-grained allow/deny；还没有路径白名单、host/port 白名单或运行时资源限制。
- VM 只覆盖同步小子集，不能承载函数、类、模块、native 标准库和 async 主路径。
- 文档和语言设计总览需要继续同步，避免设计文档落后于代码。

结论：下一阶段优先做结构减负和运行时边界，不继续横向扩 Web 框架 API。

## 2. 暂停的 Web 工作

暂缓以下 WebIno 功能：

- middleware
- router group
- 自定义 404/500 handler
- 更完整 Express 风格 API
- 长连接/SSE 框架封装
- Web 框架层性能优化

仅保留：

- 安全修复，例如权限绕过、路径穿越、请求大小限制。
- 二进制兼容，例如 `body_bytes`、`content_bytes`、`send_bytes`、`write_bytes`。
- 已有 API 回归测试。
- 明确阻塞其他核心功能的拆分或接口调整。

## 3. 下一阶段优先级

### P0：解释器主体拆分完成后的守护

**Why:** `src/interpreter/mod.rs` 已从最大维护风险降为入口文件。后续需要保持这个边界，新增 runtime 能力时优先落在对应子模块。

**Scope:**

- 新功能优先进入职责明确的子模块。
- 避免把 HTTP/WebIno/async/native method 逻辑重新塞回 `mod.rs`。
- 继续用完整测试保护结构重排。

**Expected behavior:**

- 不改变语言行为。
- 每次只拆一个边界，并保留完整测试。
- `src/interpreter/mod.rs` 保持为解释器入口、native 安装和模块接线。

**Verification:**

```text
cargo fmt
cargo test
```

### P1：Bytes/Buffer 后续实现

**Why:** 二进制数据已经进入语言，但目前只完成 `Bytes` 第一阶段。文件、HTTP、WebIno 上传下载和 streaming 仍需要统一二进制语义。

**Scope:**

- 已完成：`Buffer` 类型和基础方法。
- 已完成：`Bytes.from_base64()`、`Bytes.to_base64()`。
- 已完成：WebIno `req["body_bytes"]`、上传文件 `content_bytes`。
- 已完成：`res.send_bytes()`、`res.write_bytes()`。
- 已完成：HTTP client `stream_get_bytes`、`stream_post_bytes`、`stream_put_bytes`，handler 接收 `Bytes` chunk。
- 已完成：Bytes、HTTP response/stream chunk 和 WebIno request 最大体积限制。

**Verification:**

```text
cargo fmt
cargo test --test bytes
cargo test --test http_client_bytes
cargo test --test web_ino_response_headers
cargo test
```

### P2：权限模型细化

**Why:** 当前权限模型已经能阻止大类宿主能力，但还不能表达嵌入场景常见需求，例如“只允许读某个目录”或“只允许访问某个 host”。

**Scope:**

- 路径白名单或沙箱根目录。
- 网络 connect/listen 的 host/port 规则。
- 环境变量 key 白名单。
- 错误信息中包含被拒绝的资源摘要。

**Compatibility:** 默认仍保持 allow-all，避免破坏现有脚本。

### P3：开发工具链

**Why:** `run/check/help/version` 已完成，下一步是提高诊断和开发体验。

**Scope:**

- `icoo check` 支持更清晰的多错误或导入链诊断。
- 可选 `icoo ast <file.icoo>` 或 debug dump，帮助语言开发。
- 最小 formatter/linter 先做设计，不急于实现。

### P4：VM 扩展但不替换主路径

**Why:** VM 是长期方向，但当前价值在于验证调用协议和 async 恢复点，而不是抢主路径。

**Scope:**

- 扩展同步子集到函数调用和局部 frame。
- 继续用 backend equivalence 测试约束行为。
- 暂不迁移 class/module/async/native。

## 4. 推荐执行顺序

第一批（已完成）：

1. 拆核心执行/eval 边界。
2. 拆协程指令编译边界。

第二批（进行中）：

3. 补 `Buffer` 和 Base64。
4. 补 WebIno bytes request/response。

第三批：

5. 权限规则细化。
6. VM 函数调用子集。

## 5. 暂不做

- 不继续扩展 WebIno middleware/router。
- 不做包管理器。
- 不做完整 formatter。
- 不把 VM 切为默认 backend。
- 不把运行时全面改成 `Arc<Mutex>`。
- 不加入大型新标准库。

## 6. 最小下一步

下一步建议继续评估是否需要把限制做成可配置项，并补 WebIno request 侧 streaming 设计：

- 保持默认权限和现有 `Bytes` API 兼容。
