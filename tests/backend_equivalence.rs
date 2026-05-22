use std::cell::RefCell;
use std::rc::Rc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Backend {
    Ast,
}

#[derive(Debug)]
struct Case {
    name: &'static str,
    source: &'static str,
    expected: &'static [&'static str],
}

fn run_backend(backend: Backend, source: &str) -> Result<Vec<String>, String> {
    match backend {
        Backend::Ast => run_ast(source),
    }
}

fn run_ast(source: &str) -> Result<Vec<String>, String> {
    let output = Rc::new(RefCell::new(Vec::new()));
    let captured = output.clone();
    icoo_lang_r::run_source_with_output(source, move |line| {
        captured.borrow_mut().push(line);
    })
    .map(|_| output.borrow().clone())
    .map_err(|err| err.to_string())
}

fn assert_sync_subset_cases(cases: &[Case]) {
    let backends = [Backend::Ast];

    for case in cases {
        for backend in backends {
            let output = run_backend(backend, case.source).unwrap_or_else(|err| {
                panic!("case '{}' failed on {:?}: {}", case.name, backend, err)
            });
            assert_eq!(
                output, case.expected,
                "case '{}' output mismatch on {:?}",
                case.name, backend
            );
        }
    }
}

#[test]
fn sync_literals_operators_and_bindings_match() {
    assert_sync_subset_cases(&[
        Case {
            name: "numeric_bool_string_ops",
            source: r#"
print((1 + 2 * 3).to_string())
print(((10 - 4) / 2).to_string())
print((7 % 4).to_string())
print((1 < 2 and not false).to_string())
print(("ic" + "oo").to_string())
"#,
            expected: &["7", "3.0", "3", "true", "icoo"],
        },
        Case {
            name: "let_const_final_assignment",
            source: r#"
let count = 1
count = count + 2
const NAME: String = "Icoo"
final tag: String
tag = NAME + ":" + count.to_string()
print(tag)
"#,
            expected: &["Icoo:3"],
        },
    ]);
}

#[test]
fn sync_control_flow_matches() {
    assert_sync_subset_cases(&[
        Case {
            name: "if_elif_else",
            source: r#"
let value = 7
if value < 3:
    print("small")
elif value < 10:
    print("medium")
else:
    print("large")
"#,
            expected: &["medium"],
        },
        Case {
            name: "while_break_continue",
            source: r#"
let i = 0
let values = []
while i < 6:
    i = i + 1
    if i == 2:
        continue
    if i == 5:
        break
    values.push(i)
print(values.join(","))
"#,
            expected: &["1,3,4"],
        },
    ]);
}

#[test]
fn sync_functions_and_closures_match() {
    assert_sync_subset_cases(&[
        Case {
            name: "function_call_and_return",
            source: r#"
fn add(a: Int, b: Int) -> Int:
    return a + b

fn describe(value: Int) -> String:
    return "value=" + value.to_string()

print(describe(add(2, 5)))
"#,
            expected: &["value=7"],
        },
        Case {
            name: "recursive_function",
            source: r#"
fn fact(n: Int) -> Int:
    if n <= 1:
        return 1
    return n * fact(n - 1)

print(fact(5).to_string())
"#,
            expected: &["120"],
        },
        Case {
            name: "closure_captures_outer_binding",
            source: r#"
fn make_prefixer(prefix: String) -> Function:
    fn apply(value: String) -> String:
        return prefix + value
    return apply

let add_id = make_prefixer("id:")
print(add_id("42"))
"#,
            expected: &["id:42"],
        },
    ]);
}

#[test]
fn sync_collections_and_templates_match() {
    assert_sync_subset_cases(&[
        Case {
            name: "array_methods",
            source: r#"
let values = [1, 2]
values.push(3)
values.unshift(0)
print(values.join("-"))
print(values.at(-1).to_string())
print(values.slice(1, 3).join(","))
"#,
            expected: &["0-1-2-3", "3", "1,2"],
        },
        Case {
            name: "map_methods",
            source: r#"
let scores = {"Tom": 95}
scores.set("Lucy", 88)
print(scores.has("Tom").to_string())
print(scores.get("Lucy").to_string())
print(scores.keys().includes("Tom").to_string())
"#,
            expected: &["true", "88", "true"],
        },
        Case {
            name: "template_strings",
            source: r#"
let name = "Icoo"
let count = 2
print(f"{name}:{count + 1}")
"#,
            expected: &["Icoo:3"],
        },
    ]);
}

#[test]
fn sync_classes_and_inheritance_match() {
    assert_sync_subset_cases(&[Case {
        name: "class_fields_methods_super",
        source: r#"
class Animal:
    let name: String

    fn init(self, name: String):
        self.name = name

    fn label(self) -> String:
        return "animal:" + self.name

class Dog <- Animal:
    let breed: String
    final owner_id: String

    fn init(self, name: String, breed: String, owner_id: String):
        super.init(name)
        self.breed = breed
        self.owner_id = owner_id

    fn label(self) -> String:
        return super.label() + ":" + self.breed + ":" + self.owner_id

let dog = Dog("Lucky", "Collie", "U001")
print(dog.label())
print(dog.type_name())
"#,
        expected: &["animal:Lucky:Collie:U001", "Dog"],
    }]);
}
