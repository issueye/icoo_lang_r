# Icoo Lang R

Icoo Lang R 是一个使用 Rust 实现的 Icoo 脚本语言解释器。Icoo 采用接近 Python 的缩进语法，提供动态执行、可选类型标注、类与单继承、一次赋值绑定、协程事件循环、本地模块系统和一组内置标准库。

当前项目同时提供：

- `icoo` 命令行工具，用于运行和检查 `.icoo` 脚本。
- `icoo_lang_r` Rust 库，用于在 Rust 程序中嵌入 Icoo。
- AST 解释器为主的运行时，并保留字节码 VM 方向的设计与测试。
- 标准库模块，覆盖 JSON/YAML/TOML、文件系统、OS/环境、HTTP client/server、WebIno 路由、字节数据等能力。

## 快速开始

### 环境要求

- Rust stable
- Cargo

项目包含 `rust-toolchain.toml`，默认使用 stable 工具链。

### 构建

```bash
cargo build
```

### 运行示例

```bash
cargo run -- run examples/demo.icoo
```

也可以省略 `run`：

```bash
cargo run -- examples/demo.icoo
```

### 检查脚本

`check` 会执行词法分析、语法分析、名称解析和类型检查，但不会运行脚本：

```bash
cargo run -- check examples/demo.icoo
```

### 查看帮助和版本

```bash
cargo run -- --help
cargo run -- --version
```

### 运行测试

```bash
cargo test
```

## CLI 用法

```text
icoo run <file.icoo>
icoo check <file.icoo>
icoo <file.icoo>
icoo --help
icoo --version
```

构建 release 版本后可直接使用生成的 `icoo` 可执行文件：

```bash
cargo build --release
target/release/icoo run examples/demo.icoo
```

## 语言示例

```python
const PI: Float = 3.14159

let count = 10
final runtime_id: String
runtime_id = "icoo-" + count.to_string()

fn add(a: Int, b: Int) -> Int:
    return a + b

class Animal:
    let name: String

    fn init(self, name: String):
        self.name = name

    fn speak(self):
        print("...")

class Dog <- Animal:
    let breed: String
    final owner_id: String
    const KIND: String = "dog"

    fn init(self, name: String, breed: String, owner_id: String):
        super.init(name)
        self.breed = breed
        self.owner_id = owner_id

    fn speak(self):
        print(self.name + " says woof")

    fn to_string(self) -> String:
        return "Dog(" + self.name + ", " + self.breed + ")"

let dog = Dog("Lucky", "Border Collie", "U001")
dog.speak()
print(dog.to_string())
print(runtime_id)
print(add(2, 3).to_string())
```

更多示例见：

- `examples/demo.icoo`
- `examples/coroutines.icoo`
- `examples/modules/main.icoo`

## 核心特性

- 缩进代码块：使用缩进表达块结构。
- 绑定模型：支持 `let`、`const`、`final`。
- 可选类型标注：支持 `Int`、`Float`、`String`、`Array<T>`、`Map<K, V>`、`Task<T>` 等标注。
- 函数与闭包：支持函数声明、返回值检查和闭包捕获。
- 类系统：支持显式字段声明、方法、构造函数、单继承和 `super`。
- 内置方法：基础值支持 `to_string()`、`type_name()`，集合和字节类型提供常用方法。
- 字符串模板：支持 `f"hello {name}"` 和多行模板字符串。
- 协程：支持 `async fn`、`await`、`EventLoop`、`Task` 和 `sleep(0)`。
- 本地模块：支持 `export`、`import "./file.icoo" as name`、`from "./file.icoo" import name`。
- 错误处理：支持 `try/catch`，错误信息包含源码位置。
- 权限模型：嵌入运行时可限制文件系统、环境变量、OS 信息和网络能力。

## 模块系统

`math_extra.icoo`：

```python
export const VERSION: String = "modules-1"

export fn add(a: Int, b: Int) -> Int:
    return a + b

export class User:
    let name: String

    fn init(self, name: String):
        self.name = name

    fn to_string(self) -> String:
        return "User(" + self.name + ")"
```

`main.icoo`：

```python
import "./math_extra.icoo" as extra
from "./math_extra.icoo" import add, User as AppUser

print(extra.VERSION)
print(extra.add(1, 2).to_string())
print(add(3, 4).to_string())

let user = AppUser("Tom")
print(user.to_string())
```

第一版模块系统的边界：

- 本地模块路径使用 `./` 或 `../`。
- 文件扩展名需要显式写出 `.icoo`。
- 顶层绑定默认私有，只有 `export` 声明会暴露。
- 循环依赖会被拒绝。

## 协程和事件循环

```python
async fn worker(name: String) -> String:
    print(name + ": start")
    let delay = sleep(0)
    await delay
    print(name + ": end")
    return name

async fn main() -> String:
    let loop = current_loop()
    let a = loop.spawn(worker("A"))
    let b = loop.spawn(worker("B"))
    let av = await a
    let bv = await b
    return av + "+" + bv

let loop = EventLoop(2)
let task = loop.spawn(main())
print(loop.backend_name())
print(loop.worker_threads().to_string())
print(loop.run_until(task))
```

事件循环对外暴露的是 Icoo 语言层抽象，底层运行时实现细节不暴露给脚本代码。

## 标准库

| 模块 | 用途 |
| --- | --- |
| `Bytes` | 字节数据构造：`empty`、`from_hex`、`from_base64`、`from_string` |
| `Buffer` | 可变字节缓冲区：`new`、`from_bytes`、`from_string` |
| `std.math` | 数学函数：`abs`、`floor`、`ceil`、`round`、`min`、`max`、`random` |
| `std.time` | 时间函数：`now_ms`、`now_sec` |
| `std.json` | JSON 编码和解析：`stringify`、`parse` |
| `std.yaml` | YAML 编码和解析：`stringify`、`parse` |
| `std.toml` | TOML 编码和解析：`stringify`、`parse` |
| `std.env` | 工作目录、命令行参数和环境变量读取 |
| `std.io` | 输出：`print` |
| `std.io.fs` | 文本/字节文件读写、追加、存在性检查、目录列表 |
| `std.os` | OS 名称、平台族、架构、进程 ID、可执行路径、环境变量 |
| `std.net.http.client` | HTTP 请求、字节请求和流式接收 |
| `std.net.http.server` | 轻量 HTTP server：`serve_once` |
| `std.web.ino` | Express 风格 Web 路由：`App`、`create` |

示例：

```python
import "std.json" as json
import "std.io.fs" as fs

let text = json.stringify({"name": "Icoo", "items": [1, 2]})
let data = json.parse(text)
print(data.get("name"))

fs.write_text("target/hello.txt", "hello")
print(fs.read_text("target/hello.txt"))
```

历史全局模块 `math`、`time`、`json`、`env` 仍可用；新代码建议使用 `std.*` 导入形式。

## WebIno 示例

```python
import "std.web.ino" as ino

let app = ino.App()

fn home(req: Map<String, Any>, res: WebInoResponse):
    res.send("hello")

fn user(req: Map<String, Any>, res: WebInoResponse):
    let params = req.get("params")
    res.send("user=" + params.get("id"))

app.get("/", home)
app.get("/users/:id", user)
app.listen("127.0.0.1", 3000, 4)
```

WebIno 支持基础路由、路径参数、查询参数、响应头、文件下载、上传和流式响应，详细行为可参考 `tests/web_ino_*.rs` 与 `tests/language.rs`。

## 在 Rust 中嵌入

库入口位于 `src/lib.rs`，常用 API：

```rust
icoo_lang_r::check_source(source)?;
icoo_lang_r::check_file(path)?;
icoo_lang_r::run_source(source)?;
icoo_lang_r::run_file(path)?;
icoo_lang_r::run_source_with_output(source, |line| {
    println!("{line}");
})?;
```

权限控制示例：

```rust
use icoo_lang_r::{RuntimePermissions, PermissionRule};

let permissions = RuntimePermissions {
    fs_read: PermissionRule::AllowAll,
    fs_write: PermissionRule::DenyAll,
    fs_list: PermissionRule::AllowAll,
    env_read: PermissionRule::DenyAll,
    os_info: PermissionRule::AllowAll,
    net_connect: PermissionRule::DenyAll,
    net_listen: PermissionRule::DenyAll,
};

icoo_lang_r::run_source_with_permissions(source, permissions)?;
```

默认权限为 `RuntimePermissions::allow_all()`，嵌入场景可以根据需要改为 `deny_all()` 或细粒度配置。

## 项目结构

```text
src/
  main.rs              # icoo CLI
  lib.rs               # Rust 库入口
  lexer/               # 词法分析
  parser/              # AST 和语法分析
  resolver.rs          # 名称解析和语义约束
  typechecker.rs       # 类型检查
  interpreter/         # AST 解释器和运行时执行逻辑
  runtime/             # 值模型、环境、权限
  native_modules/      # 标准库原生模块
  vm/                  # 字节码 VM 相关实现
examples/              # Icoo 示例脚本
tests/                 # 端到端和模块测试
docs/plans/            # 设计文档和阶段计划
```

## 开发命令

```bash
cargo fmt
cargo test
cargo test native_modules_matrix
cargo test web_ino
```

部分 HTTP/WebIno 测试会在本机回环地址上临时监听端口。

## 设计文档

更完整的语言设计、标准库规划、模块系统和 VM 设计见 `docs/plans/`。

## 许可证

当前仓库未声明许可证。使用或分发前请先补充许可证信息。
