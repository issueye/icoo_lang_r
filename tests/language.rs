use std::cell::RefCell;
use std::rc::Rc;

fn run(source: &str) -> Result<Vec<String>, String> {
    let output = Rc::new(RefCell::new(Vec::new()));
    let captured = output.clone();
    icoo_lang_r::run_source_with_output(source, move |line| {
        captured.borrow_mut().push(line);
    })
    .map(|_| output.borrow().clone())
    .map_err(|err| err.to_string())
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
fn supports_fs_builtin_module() {
    let output = run(r#"
let path = "target/icoo_fs_test.txt"
fs.write_text(path, "hello fs")
print(fs.exists(path).to_string())
print(fs.is_file(path).to_string())
print(fs.is_dir("target").to_string())
print(fs.read_text(path))
print(fs.list_dir("target").includes("icoo_fs_test.txt").to_string())
"#)
    .unwrap();
    assert_eq!(output, vec!["true", "true", "true", "hello fs", "true"]);

    let err = run(r#"
fs.read_text(1)
"#)
    .unwrap_err();
    assert!(err.contains("type error: expected String for argument 1 but got Int"));

    let err = run(r#"
fs.write_text("target/icoo_fs_test.txt", 1)
"#)
    .unwrap_err();
    assert!(err.contains("type error: expected String for argument 2 but got Int"));
}
