use std::cell::RefCell;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::rc::Rc;
use std::thread;
use std::time::Duration;

fn run(source: &str) -> Result<Vec<String>, String> {
    let output = Rc::new(RefCell::new(Vec::new()));
    let captured = output.clone();
    icoo_lang_r::run_source_with_output(source, move |line| {
        captured.borrow_mut().push(line);
    })
    .map(|_| output.borrow().clone())
    .map_err(|err| err.to_string())
}

fn run_file(path: PathBuf) -> Result<Vec<String>, String> {
    let output = Rc::new(RefCell::new(Vec::new()));
    let captured = output.clone();
    icoo_lang_r::run_file_with_output(path, move |line| {
        captured.borrow_mut().push(line);
    })
    .map(|_| output.borrow().clone())
    .map_err(|err| err.to_string())
}

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

#[test]
fn supports_final_once_assignment() {
    let output = run(r#"
final runtime_id: String
runtime_id = "abc"
print(runtime_id)
"#)
    .unwrap();
    assert_eq!(output, vec!["abc"]);

    let err = run(r#"
final runtime_id: String
runtime_id = "abc"
runtime_id = "def"
"#)
    .unwrap_err();
    assert!(err.contains("final binding 'runtime_id' can only be assigned once"));
}

#[test]
fn supports_classes_inheritance_and_declared_fields() {
    let output = run(r#"
class Animal:
    let name: String

    fn init(self, name: String):
        self.name = name

class Dog <- Animal:
    let breed: String
    final owner_id: String
    const KIND: String = "dog"

    fn init(self, name: String, breed: String, owner_id: String):
        super.init(name)
        self.breed = breed
        self.owner_id = owner_id

    fn to_string(self) -> String:
        return self.name + ":" + self.breed + ":" + self.owner_id + ":" + self.KIND

let dog = Dog("Lucky", "Collie", "U001")
print(dog.to_string())
"#)
    .unwrap();
    assert_eq!(output, vec!["Lucky:Collie:U001:dog"]);
}

#[test]
fn rejects_undeclared_fields() {
    let err = run(r#"
class User:
    let name: String

    fn init(self, name: String):
        self.name = name
        self.email = "x@test.com"

let user = User("Tom")
"#)
    .unwrap_err();
    assert!(err.contains("cannot assign undeclared field 'email'"));
}

#[test]
fn supports_array_and_map_methods() {
    let output = run(r#"
let values = [1, 2, 3]
values.push(4)
print(values.len().to_string())
print(values.index_of(3).to_string())
print(values.slice(1, 3).join("-"))

let scores = {"Tom": 95}
scores.set("Lucy", 88)
print(scores.has("Tom").to_string())
print(scores.size().to_string())
"#)
    .unwrap();
    assert_eq!(output, vec!["4", "2", "2-3", "true", "2"]);
}

#[test]
fn validates_names() {
    let err = run("const max_count = 1\n").unwrap_err();
    assert!(err.contains("constant name 'max_count'"));

    let err = run("class user:\n    let name: String\n").unwrap_err();
    assert!(err.contains("class name 'user'"));

    let err = run("class User:\n    fn getName(self):\n        return nil\n").unwrap_err();
    assert!(err.contains("method name 'getName'"));
}

#[test]
fn supports_multiline_strings_and_templates() {
    let output = run(r#"
let name = "Icoo"
let count = 3
let plain = """
hello
world
"""
print(plain.contains("world").to_string())
print(f"hello {name}, count={count + 1}")
print(f"literal braces: {{name}}")
let multi = f"""
name={name}
next={count + 1}
"""
print(multi.contains("next=4").to_string())
"#)
    .unwrap();
    assert_eq!(
        output,
        vec![
            "true",
            "hello Icoo, count=4",
            "literal braces: {name}",
            "true"
        ]
    );
}

#[test]
fn supports_event_loop_async_functions_and_await() {
    let output = run(r#"
async fn worker(name: String) -> String:
    print(name + ":start")
    let delay = sleep(0)
    await delay
    print(name + ":end")
    return name

let loop = EventLoop(2)
let a = loop.spawn(worker("A"))
let b = loop.spawn(worker("B"))
print(loop.backend_name())
print(loop.worker_threads().to_string())
loop.run()
print(a.result())
print(b.result())
"#)
    .unwrap();
    assert_eq!(
        output,
        vec!["tokio", "2", "A:start", "B:start", "A:end", "B:end", "A", "B"]
    );
}

#[test]
fn supports_awaiting_tasks_inside_coroutines() {
    let output = run(r#"
async fn child() -> String:
    let delay = sleep(1)
    await delay
    return "child_done"

async fn parent() -> String:
    let loop = current_loop()
    let task = loop.spawn(child())
    let value = await task
    print(value)
    return "parent:" + value

let loop = EventLoop(2)
let task = loop.spawn(parent())
print(loop.run_until(task))
"#)
    .unwrap();
    assert_eq!(output, vec!["child_done", "parent:child_done"]);
}

#[test]
fn resolver_rejects_invalid_control_flow() {
    let err = run("return 1\n").unwrap_err();
    assert!(err.contains("resolve error: return can only be used inside a function"));

    let err = run("break\n").unwrap_err();
    assert!(err.contains("resolve error: break can only be used inside a loop"));

    let err = run("continue\n").unwrap_err();
    assert!(err.contains("resolve error: continue can only be used inside a loop"));

    let err = run(r#"
fn bad():
    await sleep(0)
"#)
    .unwrap_err();
    assert!(err.contains("resolve error: await can only be used inside an async fn"));

    let err = run(r#"
fn bad():
    yield
"#)
    .unwrap_err();
    assert!(err.contains("resolve error: yield can only be used inside an async fn"));
}

#[test]
fn resolver_allows_loop_control_inside_async_functions() {
    let output = run(r#"
async fn count_until_two() -> Int:
    let i = 0
    while i < 10:
        i = i + 1
        if i < 2:
            continue
        break
    return i

let loop = EventLoop(2)
let task = loop.spawn(count_until_two())
print(loop.run_until(task).to_string())
"#)
    .unwrap();
    assert_eq!(output, vec!["2"]);
}

#[test]
fn checks_variable_parameter_return_and_field_types() {
    let err = run(r#"
let age: Int = "old"
"#)
    .unwrap_err();
    assert!(err.contains("type error: expected Int for binding 'age' but got String"));

    let err = run(r#"
fn add_one(value: Int) -> Int:
    return value + 1

print(add_one("x"))
"#)
    .unwrap_err();
    assert!(err.contains("type error: expected Int for argument 1 but got String"));

    let err = run(r#"
fn bad() -> Int:
    return "x"

print(bad())
"#)
    .unwrap_err();
    assert!(err.contains("type error: expected Int for return value but got String"));

    let err = run(r#"
class User:
    let age: Int

    fn init(self):
        self.age = "old"

let user = User()
"#)
    .unwrap_err();
    assert!(err.contains("type error: expected Int for field 'age' but got String"));
}

#[test]
fn checks_async_return_types_and_class_type_assignability() {
    let err = run(r#"
async fn bad() -> String:
    return 1

let loop = EventLoop(2)
let task = loop.spawn(bad())
loop.run_until(task)
"#)
    .unwrap_err();
    assert!(err.contains("type error: expected String for return value but got Int"));

    let output = run(r#"
class Animal:
    let name: String

    fn init(self, name: String):
        self.name = name

class Dog <- Animal:
    let breed: String

    fn init(self, name: String, breed: String):
        super.init(name)
        self.breed = breed

fn describe(animal: Animal) -> String:
    return animal.name

print(describe(Dog("Lucky", "Collie")))
"#)
    .unwrap();
    assert_eq!(output, vec!["Lucky"]);
}

#[test]
fn typechecker_rejects_obvious_field_initializers_and_assignments() {
    let err = run(r#"
class Config:
    const PORT: Int = "8080"
"#)
    .unwrap_err();
    assert!(err.contains("type error: expected Int for field 'PORT' but got String"));

    let err = run(r#"
let count: Int = 1
count = "two"
"#)
    .unwrap_err();
    assert!(err.contains("type error: expected Int for binding 'count' but got String"));
}

#[test]
fn typechecker_tracks_async_task_result_types() {
    let err = run(r#"
async fn name() -> String:
    return "Icoo"

async fn main() -> Nil:
    let loop = current_loop()
    let task = loop.spawn(name())
    let value: Int = await task

let loop = EventLoop(2)
loop.run_until(loop.spawn(main()))
"#)
    .unwrap_err();
    assert!(err.contains("type error: expected Int for binding 'value' but got String"));

    let err = run(r#"
async fn name() -> String:
    return "Icoo"

let loop = EventLoop(2)
let task = loop.spawn(name())
let value: Int = loop.run_until(task)
"#)
    .unwrap_err();
    assert!(err.contains("type error: expected Int for binding 'value' but got String"));

    let err = run(r#"
async fn name() -> String:
    return "Icoo"

let loop = EventLoop(2)
let task = loop.spawn(name())
loop.run()
let value: Int = task.result()
"#)
    .unwrap_err();
    assert!(err.contains("type error: expected Int for binding 'value' but got String"));

    let err = run(r#"
let loop = EventLoop(2)
loop.spawn(1)
"#)
    .unwrap_err();
    assert!(err.contains("type error: expected Coroutine for argument 1 but got Int"));
}

#[test]
fn typechecker_rejects_native_method_argument_type_mismatches() {
    let err = run(r#"
print("abc".contains(1))
"#)
    .unwrap_err();
    assert!(err.contains("type error: expected String for argument 1 but got Int"));

    let err = run(r#"
let values = [1, 2]
print(values.join(1))
"#)
    .unwrap_err();
    assert!(err.contains("type error: expected String for argument 1 but got Int"));

    let err = run(r#"
let scores = {"Tom": 95}
print(scores.get(1))
"#)
    .unwrap_err();
    assert!(err.contains("type error: expected String for argument 1 but got Int"));

    let err = run(r#"
let values = [1, 2, 3]
print(values.slice("x"))
"#)
    .unwrap_err();
    assert!(err.contains("type error: expected Int for argument 1 but got String"));

    let err = run(r#"
let scores = {"Tom": 95}
scores.set(1, 2)
"#)
    .unwrap_err();
    assert!(err.contains("type error: expected String for argument 1 but got Int"));
}

#[test]
fn typechecker_supports_generic_type_annotations() {
    let output = run(r#"
let values: Array<Int> = [1, 2, 3]
let scores: Map<String, Int> = {"Tom": 95}

async fn name() -> String:
    return "Icoo"

let loop = EventLoop(2)
let task: Task<String> = loop.spawn(name())
print(values.join("-"))
print(scores.get("Tom").to_string())
print(loop.run_until(task))
"#)
    .unwrap();
    assert_eq!(output, vec!["1-2-3", "95", "Icoo"]);

    let err = run(r#"
let values: Array<Int> = [1, "x"]
"#)
    .unwrap_err();
    assert!(err.contains("type error: expected Array<Int> for binding 'values' but got Array<Any>"));

    let err = run(r#"
let scores: Map<String, Int> = {"Tom": "A"}
"#)
    .unwrap_err();
    assert!(err.contains(
        "type error: expected Map<String, Int> for binding 'scores' but got Map<String, String>"
    ));

    let err = run(r#"
async fn name() -> String:
    return "Icoo"

let loop = EventLoop(2)
let task: Task<Int> = loop.spawn(name())
"#)
    .unwrap_err();
    assert!(err.contains("type error: expected Task<Int> for binding 'task' but got Task<String>"));
}

#[test]
fn supports_math_and_time_builtin_modules() {
    let output = run(r#"
print(math.max(1, 2).to_string())
print(math.min(1, 2.5).to_string())
print(math.floor(2.8).to_string())
print(math.ceil(2.1).to_string())
print(math.round(2.6).to_string())
print(math.abs(-3).to_string())
print((math.random() >= 0.0 and math.random() < 1.0).to_string())
print((time.now_ms() > 0).to_string())
print((time.now_sec() > 0).to_string())
"#)
    .unwrap();
    assert_eq!(
        output,
        vec!["2", "1.0", "2", "3", "3", "3", "true", "true", "true"]
    );

    let err = run(r#"
print(math.max("x", 2))
"#)
    .unwrap_err();
    assert!(err.contains("type error: expected Number for argument 1 but got String"));

    let err = run(r#"
print(time.now_ms(1))
"#)
    .unwrap_err();
    assert!(err.contains("type error: method expected 0 arguments but got 1"));
}

#[test]
fn supports_json_builtin_module() {
    let output = run(r#"
let text = json.stringify([1, "x", true, nil])
print(text)
let data = json.parse("{\"name\":\"Icoo\",\"items\":[1,2],\"active\":true}")
print(data.get("name"))
print(data.get("items").at(1).to_string())
print(data.get("active").to_string())
"#)
    .unwrap();
    assert_eq!(output, vec![r#"[1,"x",true,null]"#, "Icoo", "2", "true"]);

    let err = run(r#"
json.parse(1)
"#)
    .unwrap_err();
    assert!(err.contains("type error: expected String for argument 1 but got Int"));

    let err = run(r#"
json.parse("{bad}")
"#)
    .unwrap_err();
    assert!(err.contains("runtime error: json.parse() failed"));
}

#[test]
fn supports_imported_yaml_and_toml_builtin_modules() {
    let output = run(r#"
import "std.yaml" as yaml
import "std.toml" as toml

let yaml_text = """
name: Icoo
items:
  - 1
  - 2
active: true
missing: null
"""
let yaml_data = yaml.parse(yaml_text)
print(yaml_data.get("name"))
print(yaml_data.get("items").at(1).to_string())
print(yaml_data.get("active").to_string())
print(yaml_data.get("missing").to_string())
let yaml_map = {"name": "Icoo"}
print(yaml.stringify(yaml_map).contains("name: Icoo").to_string())

let toml_text = """
name = "Icoo"
items = [1, 2]
active = true
"""
let toml_data = toml.parse(toml_text)
print(toml_data.get("name"))
print(toml_data.get("items").at(0).to_string())
print(toml_data.get("active").to_string())
let toml_map = {"name": "Icoo"}
print(toml.stringify(toml_map).contains("name = \"Icoo\"").to_string())
"#)
    .unwrap();
    assert_eq!(
        output,
        vec!["Icoo", "2", "true", "nil", "true", "Icoo", "1", "true", "true"]
    );

    let err = run(r#"
yaml.parse("name: Icoo")
"#)
    .unwrap_err();
    assert!(err.contains("undefined variable 'yaml'"));

    let err = run(r#"
import "std.yaml" as yaml
yaml.parse(1)
"#)
    .unwrap_err();
    assert!(err.contains("type error: expected String for argument 1 but got Int"));

    let err = run(r#"
import "std.toml" as toml
toml.parse("bad =")
"#)
    .unwrap_err();
    assert!(err.contains("runtime error: toml.parse() failed"));

    let err = run(r#"
import "std.toml" as toml
toml.stringify({"missing": nil})
"#)
    .unwrap_err();
    assert!(err.contains("runtime error: Nil cannot be represented as TOML"));
}

#[test]
fn supports_env_builtin_module() {
    let output = run(r#"
print(env.cwd().contains("icoo_lang_r").to_string())
print((env.args().len() >= 1).to_string())
print(env.has("__ICOO_LANG_R_TEST_MISSING__").to_string())
print(env.get("__ICOO_LANG_R_TEST_MISSING__").to_string())
"#)
    .unwrap();
    assert_eq!(output, vec!["true", "true", "false", "nil"]);

    let err = run(r#"
env.get(1)
"#)
    .unwrap_err();
    assert!(err.contains("type error: expected String for argument 1 but got Int"));
}

#[test]
fn supports_imported_io_fs_builtin_module() {
    let output = run(r#"
import "std.io.fs" as fs

let path = "target/icoo_fs_test.txt"
fs.write_text(path, "hello fs")
fs.append_text(path, " plus")
print(fs.exists(path).to_string())
print(fs.is_file(path).to_string())
print(fs.is_dir("target").to_string())
print(fs.read_text(path))
print(fs.list_dir("target").includes("icoo_fs_test.txt").to_string())
"#)
    .unwrap();
    assert_eq!(
        output,
        vec!["true", "true", "true", "hello fs plus", "true"]
    );

    let err = run(r#"
fs.read_text("target/icoo_fs_test.txt")
"#)
    .unwrap_err();
    assert!(err.contains("undefined variable 'fs'"));

    let err = run(r#"
import "std.fs" as fs
"#)
    .unwrap_err();
    assert!(err.contains("module path must end with '.icoo'"));

    let err = run(r#"
import "std.io.fs" as fs
fs.read_text(1)
"#)
    .unwrap_err();
    assert!(err.contains("type error: expected String for argument 1 but got Int"));

    let err = run(r#"
import "std.io.fs" as fs
fs.write_text("target/icoo_fs_test.txt", 1)
"#)
    .unwrap_err();
    assert!(err.contains("type error: expected String for argument 2 but got Int"));
}

#[test]
fn supports_imported_io_and_os_builtin_modules() {
    let output = run(r#"
import "std.io" as io
import "std.os" as os

io.print("hello io")
print(os.name().len() > 0)
print(os.family().len() > 0)
print(os.arch().len() > 0)
print(os.pid() > 0)
print(os.cwd().contains("icoo_lang_r").to_string())
print((os.args().len() >= 1).to_string())
print(os.has_env("__ICOO_LANG_R_TEST_MISSING__").to_string())
print(os.get_env("__ICOO_LANG_R_TEST_MISSING__").to_string())
"#)
    .unwrap();
    assert_eq!(
        output,
        vec!["hello io", "true", "true", "true", "true", "true", "true", "false", "nil"]
    );

    let err = run(r#"
io.print("x")
"#)
    .unwrap_err();
    assert!(err.contains("undefined variable 'io'"));

    let err = run(r#"
os.name()
"#)
    .unwrap_err();
    assert!(err.contains("undefined variable 'os'"));

    let err = run(r#"
import "std.io" as io
io.read_text("target/icoo_io_test.txt")
"#)
    .unwrap_err();
    assert!(err.contains("type 'std.io' has no native method 'read_text'"));

    let err = run(r#"
import "std.os" as os
os.has_env(1)
"#)
    .unwrap_err();
    assert!(err.contains("type error: expected String for argument 1 but got Int"));
}

#[test]
fn supports_file_modules_imports_and_exports() {
    let dir = PathBuf::from("target/icoo_module_tests/basic");
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("math_extra.icoo"),
        r#"
const SECRET: String = "hidden"
export const VERSION: String = "1.0.0"

export fn add(a: Int, b: Int) -> Int:
    return a + b

export class User:
    let name: String

    fn init(self, name: String):
        self.name = name

    fn to_string(self) -> String:
        return self.name
"#,
    )
    .unwrap();
    fs::write(
        dir.join("main.icoo"),
        r#"
import "./math_extra.icoo" as extra
from "./math_extra.icoo" import add, User as AppUser

print(extra.VERSION)
print(extra.add(1, 2).to_string())
print(add(3, 4).to_string())
let user = AppUser("Tom")
print(user.to_string())
"#,
    )
    .unwrap();

    let output = run_file(dir.join("main.icoo")).unwrap();
    assert_eq!(output, vec!["1.0.0", "3", "7", "Tom"]);
}

#[test]
fn modules_reject_private_exports_and_cycles() {
    let private_dir = PathBuf::from("target/icoo_module_tests/private");
    fs::create_dir_all(&private_dir).unwrap();
    fs::write(
        private_dir.join("config.icoo"),
        r#"
const SECRET: String = "hidden"
export const NAME: String = "visible"
"#,
    )
    .unwrap();
    fs::write(
        private_dir.join("main.icoo"),
        r#"
import "./config.icoo" as config
print(config.SECRET)
"#,
    )
    .unwrap();
    let err = run_file(private_dir.join("main.icoo")).unwrap_err();
    assert!(err.contains("has no export 'SECRET'"));

    let cycle_dir = PathBuf::from("target/icoo_module_tests/cycle");
    fs::create_dir_all(&cycle_dir).unwrap();
    fs::write(
        cycle_dir.join("a.icoo"),
        r#"
import "./b.icoo" as b
export const A: String = "a"
"#,
    )
    .unwrap();
    fs::write(
        cycle_dir.join("b.icoo"),
        r#"
import "./a.icoo" as a
export const B: String = "b"
"#,
    )
    .unwrap();
    fs::write(
        cycle_dir.join("main.icoo"),
        r#"
import "./a.icoo" as a
print(a.A)
"#,
    )
    .unwrap();
    let err = run_file(cycle_dir.join("main.icoo")).unwrap_err();
    assert!(err.contains("module cycle detected"));
}

#[test]
fn supports_imported_net_http_client_and_server_modules() {
    let dir = PathBuf::from("target/icoo_module_tests/net_http");
    fs::create_dir_all(&dir).unwrap();
    let port = free_port();
    let server_path = dir.join("server.icoo");
    fs::write(
        &server_path,
        format!(
            r#"
import "std.net.http.server" as server
server.serve_once("127.0.0.1", {}, "hello from icoo")
"#,
            port
        ),
    )
    .unwrap();
    let server_handle =
        thread::spawn(move || icoo_lang_r::run_file(server_path).map_err(|err| err.to_string()));
    thread::sleep(Duration::from_millis(150));

    let client_path = dir.join("client.icoo");
    fs::write(
        &client_path,
        format!(
            r#"
import "std.net.http.client" as client
let response = client.get("http://127.0.0.1:{}/hello")
print(response.get("status").to_string())
print(response.get("body"))
"#,
            port
        ),
    )
    .unwrap();
    let output = run_file(client_path).unwrap();
    assert_eq!(output, vec!["200", "hello from icoo"]);
    server_handle.join().unwrap().unwrap();

    let err = run(r#"
net.http.client.get("http://127.0.0.1/")
"#)
    .unwrap_err();
    assert!(err.contains("undefined variable 'net'"));

    let old_import_path = dir.join("old_import.icoo");
    fs::write(
        &old_import_path,
        r#"
import "net.http.client" as client
"#,
    )
    .unwrap();
    let err = run_file(old_import_path).unwrap_err();
    assert!(err.contains("module path must end with '.icoo'"));

    let err_path = dir.join("bad.icoo");
    fs::write(
        &err_path,
        r#"
import "std.net.http.client" as client
client.get(1)
"#,
    )
    .unwrap();
    let err = run_file(err_path).unwrap_err();
    assert!(err.contains("type error: expected String for argument 1 but got Int"));

    let std_io_fs_path = dir.join("std_io_fs.icoo");
    fs::write(
        &std_io_fs_path,
        r#"
import "std.io.fs" as fs
print(fs.exists("target").to_string())
"#,
    )
    .unwrap();
    let output = run_file(std_io_fs_path).unwrap();
    assert_eq!(output, vec!["true"]);
}

#[test]
fn supports_std_web_ino_express_style_server() {
    let dir = PathBuf::from("target/icoo_module_tests/web_ino");
    fs::create_dir_all(&dir).unwrap();
    let port = free_port();
    let server_path = dir.join("server.icoo");
    fs::write(
        &server_path,
        format!(
            r#"
import "std.web.ino" as ino

let app = ino.App()

fn home(req: Map<String, Any>, res: WebInoResponse):
    res.status(201)
    res.send("hello " + req.get("path"))

app.get("/hello", home)
app.listen_once("127.0.0.1", {})
"#,
            port
        ),
    )
    .unwrap();
    let server_handle =
        thread::spawn(move || icoo_lang_r::run_file(server_path).map_err(|err| err.to_string()));
    thread::sleep(Duration::from_millis(150));

    let client_path = dir.join("client.icoo");
    fs::write(
        &client_path,
        format!(
            r#"
import "std.net.http.client" as client
let response = client.get("http://127.0.0.1:{}/hello")
print(response.get("status").to_string())
print(response.get("body"))
"#,
            port
        ),
    )
    .unwrap();
    let output = run_file(client_path).unwrap();
    assert_eq!(output, vec!["201", "hello /hello"]);
    server_handle.join().unwrap().unwrap();

    let err = run(r#"
web.ino.App()
"#)
    .unwrap_err();
    assert!(err.contains("undefined variable 'web'"));

    let err = run(r#"
import "std.web.ino" as ino
let app = ino.App()
app.get("/", 1)
"#)
    .unwrap_err();
    assert!(err.contains("type error: expected Function for argument 2 but got Int"));
}

#[test]
fn supports_std_web_ino_concurrent_request_acceptance() {
    let dir = PathBuf::from("target/icoo_module_tests/web_ino_concurrent");
    fs::create_dir_all(&dir).unwrap();
    let port = free_port();
    let server_path = dir.join("server.icoo");
    fs::write(
        &server_path,
        format!(
            r#"
import "std.web.ino" as ino

let app = ino.create()

fn echo(req: Map<String, Any>, res: WebInoResponse):
    res.send("path=" + req.get("path"))

app.get("/a", echo)
app.get("/b", echo)
app.listen("127.0.0.1", {}, 2)
"#,
            port
        ),
    )
    .unwrap();
    let server_handle =
        thread::spawn(move || icoo_lang_r::run_file(server_path).map_err(|err| err.to_string()));
    thread::sleep(Duration::from_millis(150));

    let client_a_path = dir.join("client_a.icoo");
    fs::write(
        &client_a_path,
        format!(
            r#"
import "std.net.http.client" as client
let response = client.get("http://127.0.0.1:{}/a")
print(response.get("body"))
"#,
            port
        ),
    )
    .unwrap();
    let client_b_path = dir.join("client_b.icoo");
    fs::write(
        &client_b_path,
        format!(
            r#"
import "std.net.http.client" as client
let response = client.get("http://127.0.0.1:{}/b")
print(response.get("body"))
"#,
            port
        ),
    )
    .unwrap();

    let client_a = thread::spawn(move || run_file(client_a_path));
    let client_b = thread::spawn(move || run_file(client_b_path));
    let mut output = Vec::new();
    output.extend(client_a.join().unwrap().unwrap());
    output.extend(client_b.join().unwrap().unwrap());
    output.sort();
    assert_eq!(output, vec!["path=/a", "path=/b"]);
    server_handle.join().unwrap().unwrap();

    let err = run(r#"
import "std.web.ino" as ino
let app = ino.App()
app.listen("127.0.0.1", 1, 0)
"#)
    .unwrap_err();
    assert!(err.contains("max_requests must be positive"));

    let err = run(r#"
import "std.web.ino" as ino
let app = ino.App()
app.listen_with_workers("127.0.0.1", 1, 1, 0)
"#)
    .unwrap_err();
    assert!(err.contains("workers must be positive"));
}

#[test]
fn supports_std_web_ino_file_uploads() {
    let dir = PathBuf::from("target/icoo_module_tests/web_ino_upload");
    fs::create_dir_all(&dir).unwrap();
    let port = free_port();
    let server_path = dir.join("server.icoo");
    fs::write(
        &server_path,
        format!(
            r#"
import "std.web.ino" as ino

let app = ino.App()

fn upload(req: Map<String, Any>, res: WebInoResponse):
    let form = req.get("form")
    let files = req.get("files")
    let file = files.get("avatar")
    res.json({{"title": form.get("title"), "filename": file.get("filename"), "content_type": file.get("content_type"), "content": file.get("content"), "size": file.get("size")}})

app.post("/upload", upload)
app.listen_once("127.0.0.1", {})
"#,
            port
        ),
    )
    .unwrap();
    let server_handle =
        thread::spawn(move || icoo_lang_r::run_file(server_path).map_err(|err| err.to_string()));
    thread::sleep(Duration::from_millis(150));

    let boundary = "ICOOTESTBOUNDARY";
    let body = format!(
        "--{0}\r\nContent-Disposition: form-data; name=\"title\"\r\n\r\nprofile\r\n--{0}\r\nContent-Disposition: form-data; name=\"avatar\"; filename=\"avatar.txt\"\r\nContent-Type: text/plain\r\n\r\nhello upload\r\n--{0}--\r\n",
        boundary
    );
    let request = format!(
        "POST /upload HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\nContent-Type: multipart/form-data; boundary={}\r\nContent-Length: {}\r\n\r\n{}",
        boundary,
        body.as_bytes().len(),
        body
    );
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200 OK"));
    assert!(response.contains(r#""title":"profile""#));
    assert!(response.contains(r#""filename":"avatar.txt""#));
    assert!(response.contains(r#""content_type":"text/plain""#));
    assert!(response.contains(r#""content":"hello upload""#));
    assert!(response.contains(r#""size":12"#));
    server_handle.join().unwrap().unwrap();
}

#[test]
fn supports_std_web_ino_streaming_responses() {
    let dir = PathBuf::from("target/icoo_module_tests/web_ino_stream");
    fs::create_dir_all(&dir).unwrap();
    let port = free_port();
    let server_path = dir.join("server.icoo");
    fs::write(
        &server_path,
        format!(
            r#"
import "std.web.ino" as ino

let app = ino.App()

fn stream(req: Map<String, Any>, res: WebInoResponse):
    res.write("hello")
    res.write(" ")
    res.write(req.get("path"))
    res.end()

app.get("/stream", stream)
app.listen_once("127.0.0.1", {})
"#,
            port
        ),
    )
    .unwrap();
    let server_handle =
        thread::spawn(move || icoo_lang_r::run_file(server_path).map_err(|err| err.to_string()));
    thread::sleep(Duration::from_millis(150));

    let client_path = dir.join("client.icoo");
    fs::write(
        &client_path,
        format!(
            r#"
import "std.net.http.client" as client
let response = client.get("http://127.0.0.1:{}/stream")
print(response.get("headers").get("transfer-encoding"))
print(response.get("body"))
"#,
            port
        ),
    )
    .unwrap();
    let output = run_file(client_path).unwrap();
    assert_eq!(output, vec!["chunked", "hello /stream"]);
    server_handle.join().unwrap().unwrap();
}

#[test]
fn supports_std_web_ino_file_downloads() {
    let dir = PathBuf::from("target/icoo_module_tests/web_ino_download");
    fs::create_dir_all(&dir).unwrap();
    let port = free_port();
    let file_path = dir.join("report.bin");
    let file_bytes = vec![0_u8, 1, 2, 3, 254, 255];
    fs::write(&file_path, &file_bytes).unwrap();
    let script_file_path = file_path.to_string_lossy().replace('\\', "/");
    let server_path = dir.join("server.icoo");
    fs::write(
        &server_path,
        format!(
            r#"
import "std.web.ino" as ino

let app = ino.App()

fn download(req: Map<String, Any>, res: WebInoResponse):
    res.download("{}", "export.bin")

app.get("/download", download)
app.listen_once("127.0.0.1", {})
"#,
            script_file_path, port
        ),
    )
    .unwrap();
    let server_handle =
        thread::spawn(move || icoo_lang_r::run_file(server_path).map_err(|err| err.to_string()));
    thread::sleep(Duration::from_millis(150));

    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .write_all(b"GET /download HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).unwrap();
    let split = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .unwrap();
    let headers = String::from_utf8_lossy(&response[..split]);
    let body = &response[split + 4..];
    assert!(headers.starts_with("HTTP/1.1 200 OK"));
    assert!(headers.contains("Content-Length: 6"));
    assert!(headers.contains("Content-Type: application/octet-stream"));
    assert!(headers.contains("Content-Disposition: attachment; filename=\"export.bin\""));
    assert_eq!(body, file_bytes.as_slice());
    server_handle.join().unwrap().unwrap();
}
