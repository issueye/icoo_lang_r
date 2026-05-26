# Icoo Lang R

Icoo Lang R is a Rust implementation of the Icoo scripting language. Icoo uses brace-delimited block syntax and provides dynamic execution, optional type annotations, classes with single inheritance, one-time assignment bindings, coroutine event loops, local modules, and built-in standard library modules.

This repository provides:

- The `icoo` CLI for running and checking `.icoo` scripts.
- The `icoo_lang_r` Rust library for embedding Icoo in Rust applications.
- An AST interpreter as the primary runtime, with bytecode VM work kept in the codebase and tests.
- Standard library modules for JSON/YAML/TOML, filesystem, OS/environment access, HTTP client/server, WebIno routing, byte data, and more.

## Quick Start

### Requirements

- Rust stable
- Cargo

The repository includes `rust-toolchain.toml` and uses the stable toolchain by default.

### Build

```bash
cargo build
```

### Run an Example

```bash
cargo run -- run examples/demo.icoo
```

The `run` subcommand can also be omitted:

```bash
cargo run -- examples/demo.icoo
```

### Initialize a Project

```bash
cargo run -- init my_app
cd my_app
icoo run
```

`init` creates `pkg.toml` and `src/main.icoo`. When running a project directory or `pkg.toml`, the runtime reads `run.entry`, loads that entry file, and calls `main()`.

### Check a Script

`check` runs lexing, parsing, name resolution, and type checking without executing the script:

```bash
cargo run -- check examples/demo.icoo
```

### Help and Version

```bash
cargo run -- --help
cargo run -- --version
```

### Tests

```bash
cargo test
```

## CLI Usage

```text
icoo init [dir]
icoo run [file.icoo|project_dir|pkg.toml]
icoo check <file.icoo>
icoo <file.icoo|project_dir|pkg.toml>
icoo --help
icoo --version
```

After a release build, use the generated `icoo` executable directly:

```bash
cargo build --release
target/release/icoo run examples/demo.icoo
```

## Language Example

```python
const PI: Float = 3.14159

let count = 10
final runtime_id: String
runtime_id = "icoo-" + count.to_string()

fn add(a: Int, b: Int) -> Int {
    return a + b

}
class Animal {
    let name: String

    fn init(self, name: String) {
        self.name = name

    }
    fn speak(self) {
        print("...")

    }
}
class Dog <- Animal {
    let breed: String
    final owner_id: String
    const KIND: String = "dog"

    fn init(self, name: String, breed: String, owner_id: String) {
        super.init(name)
        self.breed = breed
        self.owner_id = owner_id

    }
    fn speak(self) {
        print(self.name + " says woof")

    }
    fn to_string(self) -> String {
        return "Dog(" + self.name + ", " + self.breed + ")"

    }
}
let dog = Dog("Lucky", "Border Collie", "U001")
dog.speak()
print(dog.to_string())
print(runtime_id)
print(add(2, 3).to_string())
```

More examples:

- `examples/demo.icoo`
- `examples/coroutines.icoo`
- `examples/modules/main.icoo`

## Core Features

- Brace-delimited blocks: functions, classes, conditions, loops, and error handling use `{ ... }`.
- Bindings with `let`, `const`, and `final`.
- Optional type annotations, including `Int`, `Float`, `String`, `Array<T>`, `Map<K, V>`, and `Task<T>`.
- Functions, return checking, and closures.
- Classes with declared fields, methods, constructors, single inheritance, and `super`.
- Built-in methods such as `to_string()` and `type_name()`, plus collection and byte helpers.
- String templates with `f"hello {name}"` and multiline template strings.
- Coroutines with `async fn`, `await`, `EventLoop`, `Task`, and `sleep(0)`.
- Local modules with `export`, `import "./file.icoo" as name`, and `from "./file.icoo" import name`.
- Error handling with `try/catch` and source-positioned errors.
- Runtime permissions for filesystem, environment, OS information, and network access in embedded use.

## Modules

`math_extra.icoo`:

```python
export const VERSION: String = "modules-1"

export fn add(a: Int, b: Int) -> Int {
    return a + b

}
export class User {
    let name: String

    fn init(self, name: String) {
        self.name = name

    }
    fn to_string(self) -> String {
        return "User(" + self.name + ")"
    }
}
```

`main.icoo`:

```python
import "./math_extra.icoo" as extra
from "./math_extra.icoo" import add, User as AppUser

print(extra.VERSION)
print(extra.add(1, 2).to_string())
print(add(3, 4).to_string())

let user = AppUser("Tom")
print(user.to_string())
```

Current module-system boundaries:

- Local module paths use `./` or `../`.
- The `.icoo` extension must be written explicitly.
- Top-level bindings are private by default; only `export` declarations are public.
- Cyclic dependencies are rejected.

## Coroutines and Event Loops

```python
async fn worker(name: String) -> String {
    print(name + ": start")
    let delay = sleep(0)
    await delay
    print(name + ": end")
    return name

}
async fn main() -> String {
    let loop = current_loop()
    let a = loop.spawn(worker("A"))
    let b = loop.spawn(worker("B"))
    let av = await a
    let bv = await b
    return av + "+" + bv

}
let loop = EventLoop(2)
let task = loop.spawn(main())
print(loop.backend_name())
print(loop.worker_threads().to_string())
print(loop.run_until(task))
```

The event loop is exposed as an Icoo language abstraction; the underlying runtime implementation is not exposed to script code.

## Standard Library

| Module | Purpose |
| --- | --- |
| `Bytes` | Byte constructors: `empty`, `from_hex`, `from_base64`, `from_string` |
| `Buffer` | Mutable byte buffer: `new`, `from_bytes`, `from_string` |
| `std.math` | Math functions: `abs`, `floor`, `ceil`, `round`, `min`, `max`, `random` |
| `std.time` | Time functions: `now_ms`, `now_sec` |
| `std.json` | JSON encode/decode: `stringify`, `parse` |
| `std.yaml` | YAML encode/decode: `stringify`, `parse` |
| `std.toml` | TOML encode/decode: `stringify`, `parse` |
| `std.env` | Current directory, CLI args, and environment variable reads |
| `std.io` | Output via `print` |
| `std.io.fs` | Text/byte file read, write, append, existence checks, and directory listing |
| `std.os` | OS name, family, architecture, process ID, executable path, and environment variables |
| `std.process` | Permission-controlled local shell command execution via `exec` |
| `std.net.http.client` | HTTP requests, byte requests, and streaming receive |
| `std.net.http.server` | Lightweight HTTP server with `serve_once` |
| `std.web.ino` | Express-style web routing with `App` and `create` |

Example:

```python
import "std.json" as json
import "std.io.fs" as fs

let text = json.stringify({"name": "Icoo", "items": [1, 2]})
let data = json.parse(text)
print(data.get("name"))

fs.write_text("target/hello.txt", "hello")
print(fs.read_text("target/hello.txt"))
```

Local command tool example:

```python
import "std.process" as process

let result = process.exec("echo icoo", {
    "timeout_ms": 1000,
    "max_output_bytes": 4096
})
print(result.get("success").to_string())
print(result.get("stdout"))
```

Legacy global modules `math`, `time`, `json`, and `env` are still available. New code should prefer the `std.*` import form.

## WebIno Example

```python
import "std.web.ino" as ino

let app = ino.App()

fn home(req: Map<String, Any>, res: WebInoResponse) {
    res.send("hello")

}
fn user(req: Map<String, Any>, res: WebInoResponse) {
    let params = req.get("params")
    res.send("user=" + params.get("id"))

}
app.get("/", home)
app.get("/users/:id", user)
app.listen("127.0.0.1", 3000, 4)
```

WebIno supports basic routes, route params, query params, response headers, downloads, uploads, and streaming responses. See `tests/web_ino_*.rs` and `tests/language.rs` for detailed behavior.

## Embedding in Rust

The library entry point is `src/lib.rs`. Common APIs:

```rust
icoo_lang_r::check_source(source)?;
icoo_lang_r::check_file(path)?;
icoo_lang_r::run_source(source)?;
icoo_lang_r::run_file(path)?;
icoo_lang_r::run_source_with_output(source, |line| {
    println!("{line}");
})?;
```

Permission control example:

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
    process_exec: PermissionRule::DenyAll,
};

icoo_lang_r::run_source_with_permissions(source, permissions)?;
```

The default permission set is `RuntimePermissions::allow_all()`. Embedded hosts can use `deny_all()` or configure capabilities individually.

## Repository Layout

```text
src/
  main.rs              # icoo CLI
  lib.rs               # Rust library entry point
  lexer/               # Lexing
  parser/              # AST and parsing
  resolver.rs          # Name resolution and semantic constraints
  typechecker.rs       # Type checking
  interpreter/         # AST interpreter and runtime execution logic
  runtime/             # Value model, environment, permissions
  native_modules/      # Native standard library modules
  vm/                  # Bytecode VM-related implementation
examples/              # Icoo example scripts
tests/                 # End-to-end and module tests
docs/plans/            # Design documents and implementation plans
```

## Development Commands

```bash
cargo fmt
cargo test
cargo test native_modules_matrix
cargo test web_ino
```

Some HTTP/WebIno tests temporarily listen on local loopback ports.

## Design Documents

See `docs/plans/` for the fuller language design, standard library planning, module-system design, and VM notes.

## License

No license is declared in this repository yet. Add a license before using or distributing the project.
