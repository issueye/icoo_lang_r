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
fn run_until_only_waits_for_target_task() {
    let output = run(r#"
async fn fast() -> String:
    return "done"

async fn slow() -> String:
    let delay = sleep(50)
    await delay
    print("slow")
    return "slow"

let loop = EventLoop(2)
let fast_task = loop.spawn(fast())
let slow_task = loop.spawn(slow())
print(loop.run_until(fast_task))
print(slow_task.is_done().to_string())
"#)
    .unwrap();

    assert_eq!(output, vec!["done", "false"]);
}

#[test]
fn cancelling_task_wakes_awaiters_with_stable_error() {
    let err = run(r#"
async fn child() -> String:
    let delay = sleep(50)
    await delay
    return "child"

let loop = EventLoop(2)
let child_task = loop.spawn(child())

async fn waiter() -> String:
    let value = await child_task
    return value

async fn canceller() -> Nil:
    child_task.cancel()

let waiter_task = loop.spawn(waiter())
loop.spawn(canceller())
loop.run_until(waiter_task)
"#)
    .unwrap_err();

    assert!(err.contains("task was cancelled"), "{err}");
}

#[test]
fn run_until_rejects_task_from_another_event_loop() {
    let err = run(r#"
async fn done() -> String:
    return "done"

let first = EventLoop(2)
let second = EventLoop(2)
let task = second.spawn(done())
first.run_until(task)
"#)
    .unwrap_err();

    assert!(
        err.contains("task belongs to a different EventLoop"),
        "{err}"
    );
}

#[test]
fn spawn_rejects_reusing_the_same_coroutine() {
    let err = run(r#"
async fn done() -> String:
    return "done"

let loop = EventLoop(2)
let coroutine = done()
loop.spawn(coroutine)
loop.spawn(coroutine)
"#)
    .unwrap_err();

    assert!(err.contains("coroutine has already been spawned"), "{err}");
}

#[test]
fn resolver_rejects_await_inside_complex_expressions() {
    let err = run(r#"
async fn value() -> String:
    return "value"

async fn main() -> String:
    let loop = current_loop()
    let task = loop.spawn(value())
    return "got:" + await task

let loop = EventLoop(2)
loop.run_until(loop.spawn(main()))
"#)
    .unwrap_err();

    assert!(
        err.contains("await can only be used as a standalone expression, binding initializer, or return value"),
        "{err}"
    );
}
