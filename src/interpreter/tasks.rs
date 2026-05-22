use super::{check_value_type, expect_arity, Interpreter};
use crate::error::{IcooError, IcooResult};
use crate::lexer::token::Span;
use crate::parser::ast::Stmt;
use crate::runtime::env::Environment;
use crate::runtime::value::*;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

impl Interpreter {
    pub(super) fn event_loop_method(
        &mut self,
        loop_ref: Rc<RefCell<IcooEventLoop>>,
        name: &str,
        args: Vec<Value>,
        span: Span,
    ) -> IcooResult<Value> {
        match name {
            "spawn" => {
                expect_arity(&args, 1, span)?;
                let Value::Coroutine(coroutine) = &args[0] else {
                    return Err(IcooError::runtime(
                        "spawn() expects a Coroutine",
                        Some(span),
                    ));
                };
                if coroutine.borrow().owner_task.is_some() {
                    return Err(IcooError::runtime(
                        "coroutine has already been spawned",
                        Some(span),
                    ));
                }
                let task = {
                    let mut event_loop = loop_ref.borrow_mut();
                    let id = event_loop.next_task_id;
                    event_loop.next_task_id += 1;
                    let task = Rc::new(RefCell::new(IcooTask {
                        id,
                        loop_id: event_loop.id,
                        event_loop: Rc::downgrade(&loop_ref),
                        coroutine: coroutine.clone(),
                        state: TaskState::Queued,
                        result: None,
                        error: None,
                        awaiters: Vec::new(),
                    }));
                    coroutine.borrow_mut().owner_task = Some(id);
                    event_loop.ready.push_back(task.clone());
                    task
                };
                Ok(Value::Task(task))
            }
            "run" => {
                expect_arity(&args, 0, span)?;
                self.run_event_loop(loop_ref, span)?;
                Ok(Value::Nil)
            }
            "run_until" => {
                expect_arity(&args, 1, span)?;
                let Value::Task(task) = &args[0] else {
                    return Err(IcooError::runtime("run_until() expects a Task", Some(span)));
                };
                if task.borrow().loop_id != loop_ref.borrow().id {
                    return Err(IcooError::runtime(
                        "task belongs to a different EventLoop",
                        Some(span),
                    ));
                }
                self.run_event_loop_until(loop_ref, task.clone(), span)?;
                task_result(task.clone(), span)
            }
            "stop" => {
                expect_arity(&args, 0, span)?;
                loop_ref.borrow_mut().stopped = true;
                Ok(Value::Nil)
            }
            "is_stopped" => {
                expect_arity(&args, 0, span)?;
                Ok(Value::Bool(loop_ref.borrow().stopped))
            }
            "backend_name" => {
                expect_arity(&args, 0, span)?;
                Ok(Value::String(loop_ref.borrow().backend.name().to_string()))
            }
            "worker_threads" => {
                expect_arity(&args, 0, span)?;
                Ok(Value::Int(loop_ref.borrow().backend.worker_threads() as i64))
            }
            _ => Err(IcooError::runtime("unknown EventLoop method", Some(span))),
        }
    }

    pub(super) fn task_method(
        &mut self,
        task: Rc<RefCell<IcooTask>>,
        name: &str,
        args: Vec<Value>,
        span: Span,
    ) -> IcooResult<Value> {
        match name {
            "is_done" => {
                expect_arity(&args, 0, span)?;
                Ok(Value::Bool(matches!(
                    task.borrow().state,
                    TaskState::Done | TaskState::Failed | TaskState::Cancelled
                )))
            }
            "is_failed" => {
                expect_arity(&args, 0, span)?;
                Ok(Value::Bool(task.borrow().state == TaskState::Failed))
            }
            "result" => {
                expect_arity(&args, 0, span)?;
                task_result(task, span)
            }
            "cancel" => {
                expect_arity(&args, 0, span)?;
                cancel_task(task, span)?;
                Ok(Value::Nil)
            }
            _ => Err(IcooError::runtime("unknown Task method", Some(span))),
        }
    }

    fn run_event_loop(
        &mut self,
        loop_ref: Rc<RefCell<IcooEventLoop>>,
        span: Span,
    ) -> IcooResult<()> {
        loop {
            if loop_ref.borrow().stopped {
                break;
            }
            enqueue_due_timers(&loop_ref);
            let Some(task) = loop_ref.borrow_mut().ready.pop_front() else {
                if wait_for_next_timer(&loop_ref) {
                    continue;
                } else {
                    break;
                }
            };
            self.run_task(loop_ref.clone(), task, span)?;
        }
        Ok(())
    }

    fn run_event_loop_until(
        &mut self,
        loop_ref: Rc<RefCell<IcooEventLoop>>,
        target: Rc<RefCell<IcooTask>>,
        span: Span,
    ) -> IcooResult<()> {
        loop {
            if loop_ref.borrow().stopped || is_task_terminal(&target) {
                break;
            }
            enqueue_due_timers(&loop_ref);
            if is_task_terminal(&target) {
                break;
            }
            let Some(task) = loop_ref.borrow_mut().ready.pop_front() else {
                if wait_for_next_timer(&loop_ref) {
                    continue;
                } else {
                    break;
                }
            };
            self.run_task(loop_ref.clone(), task, span)?;
        }
        Ok(())
    }

    fn run_task(
        &mut self,
        loop_ref: Rc<RefCell<IcooEventLoop>>,
        task: Rc<RefCell<IcooTask>>,
        span: Span,
    ) -> IcooResult<()> {
        if matches!(
            task.borrow().state,
            TaskState::Done | TaskState::Failed | TaskState::Cancelled
        ) {
            return Ok(());
        }
        task.borrow_mut().state = TaskState::Running;

        let previous_loop = self.current_loop.replace(loop_ref.clone());
        let previous_task = self.current_task.replace(task.clone());
        let result = self.run_coroutine_until_pause(task.borrow().coroutine.clone());
        self.current_loop = previous_loop;
        self.current_task = previous_task;

        match result {
            Ok(CoroutineStep::Yielded) => {
                task.borrow_mut().state = TaskState::Queued;
                loop_ref.borrow_mut().ready.push_back(task);
            }
            Ok(CoroutineStep::Done(value)) => {
                complete_task(task, TaskState::Done, Some(value), None, &loop_ref);
            }
            Err(IcooError::Await(Value::Task(waiting_on))) => {
                if waiting_on.borrow().loop_id != loop_ref.borrow().id {
                    complete_task(
                        task,
                        TaskState::Failed,
                        None,
                        Some("cannot await task from a different EventLoop".to_string()),
                        &loop_ref,
                    );
                    return Ok(());
                }
                task.borrow_mut().state = TaskState::Waiting;
                waiting_on.borrow_mut().awaiters.push(task);
            }
            Err(err) => {
                complete_task(
                    task,
                    TaskState::Failed,
                    None,
                    Some(err.to_string()),
                    &loop_ref,
                );
            }
        }
        let _ = span;
        Ok(())
    }

    fn run_coroutine_until_pause(
        &mut self,
        coroutine: Rc<RefCell<IcooCoroutine>>,
    ) -> IcooResult<CoroutineStep> {
        let previous_env = self.env.clone();
        self.env = coroutine.borrow().env.clone();
        let result = loop {
            let instr = {
                let coroutine_ref = coroutine.borrow();
                if coroutine_ref.pc >= coroutine_ref.instructions.len() {
                    if let Some(return_type) = &coroutine_ref.return_type {
                        check_value_type(
                            &Value::Nil,
                            return_type,
                            &format!("return value of '{}'", coroutine_ref.name),
                            return_type.span,
                        )?;
                    }
                    break Ok(CoroutineStep::Done(Value::Nil));
                }
                coroutine_ref.instructions[coroutine_ref.pc].clone()
            };

            match instr {
                CoroutineInstr::Stmt(stmt) => match self.execute(&stmt) {
                    Ok(()) => coroutine.borrow_mut().pc += 1,
                    Err(IcooError::Return(value)) => {
                        if let Some(return_type) = &coroutine.borrow().return_type {
                            let span = match &stmt {
                                Stmt::Return { span, .. } => *span,
                                _ => return_type.span,
                            };
                            check_value_type(
                                &value,
                                return_type,
                                &format!("return value of '{}'", coroutine.borrow().name),
                                span,
                            )?;
                        }
                        break Ok(CoroutineStep::Done(value));
                    }
                    Err(err) => break Err(err),
                },
                CoroutineInstr::JumpIfFalse { condition, target } => {
                    if self.eval(&condition)?.truthy() {
                        coroutine.borrow_mut().pc += 1;
                    } else {
                        coroutine.borrow_mut().pc = target;
                    }
                }
                CoroutineInstr::Jump { target } => {
                    coroutine.borrow_mut().pc = target;
                }
                CoroutineInstr::Yield(value) => {
                    let value = if let Some(value) = value {
                        self.eval(&value)?
                    } else {
                        Value::Nil
                    };
                    coroutine.borrow_mut().pc += 1;
                    let _ = value;
                    break Ok(CoroutineStep::Yielded);
                }
            }
        };
        self.env = previous_env;
        result
    }
}

enum CoroutineStep {
    Yielded,
    Done(Value),
}

fn is_task_terminal(task: &Rc<RefCell<IcooTask>>) -> bool {
    matches!(
        task.borrow().state,
        TaskState::Done | TaskState::Failed | TaskState::Cancelled
    )
}

fn task_result(task: Rc<RefCell<IcooTask>>, span: Span) -> IcooResult<Value> {
    match task.borrow().state {
        TaskState::Done => Ok(task.borrow().result.clone().unwrap_or(Value::Nil)),
        TaskState::Failed => Err(IcooError::runtime(
            task.borrow()
                .error
                .clone()
                .unwrap_or_else(|| "task failed".to_string()),
            Some(span),
        )),
        TaskState::Cancelled => Err(IcooError::runtime("task was cancelled", Some(span))),
        _ => Err(IcooError::runtime("task is not done", Some(span))),
    }
}

fn cancel_task(task: Rc<RefCell<IcooTask>>, span: Span) -> IcooResult<()> {
    if is_task_terminal(&task) {
        return Ok(());
    }
    let Some(loop_ref) = task.borrow().event_loop.upgrade() else {
        return Err(IcooError::runtime(
            "task EventLoop is no longer available",
            Some(span),
        ));
    };
    complete_task(task, TaskState::Cancelled, None, None, &loop_ref);
    Ok(())
}

pub(super) fn schedule_sleep_task(
    loop_ref: Rc<RefCell<IcooEventLoop>>,
    millis: u64,
) -> Rc<RefCell<IcooTask>> {
    let env = Environment::new();
    let coroutine = Rc::new(RefCell::new(IcooCoroutine {
        name: "sleep".to_string(),
        return_type: None,
        env,
        instructions: Vec::new(),
        pc: 0,
        owner_task: None,
    }));
    let mut event_loop = loop_ref.borrow_mut();
    let id = event_loop.next_task_id;
    event_loop.next_task_id += 1;
    coroutine.borrow_mut().owner_task = Some(id);
    let task = Rc::new(RefCell::new(IcooTask {
        id,
        loop_id: event_loop.id,
        event_loop: Rc::downgrade(&loop_ref),
        coroutine,
        state: TaskState::Queued,
        result: None,
        error: None,
        awaiters: Vec::new(),
    }));
    event_loop.timers.push(SleepTimer {
        due: Instant::now() + Duration::from_millis(millis),
        task: task.clone(),
    });
    task
}

fn enqueue_due_timers(loop_ref: &Rc<RefCell<IcooEventLoop>>) {
    let now = Instant::now();
    let mut ready_timers = Vec::new();
    {
        let mut event_loop = loop_ref.borrow_mut();
        let mut index = 0;
        while index < event_loop.timers.len() {
            if event_loop.timers[index].due <= now {
                ready_timers.push(event_loop.timers.swap_remove(index));
            } else {
                index += 1;
            }
        }
    }
    if !ready_timers.is_empty() {
        let mut event_loop = loop_ref.borrow_mut();
        for timer in ready_timers {
            event_loop.ready.push_back(timer.task);
        }
    }
}

fn wait_for_next_timer(loop_ref: &Rc<RefCell<IcooEventLoop>>) -> bool {
    let Some((due, backend)) = ({
        let event_loop = loop_ref.borrow();
        event_loop
            .timers
            .iter()
            .map(|timer| timer.due)
            .min()
            .map(|due| (due, event_loop.backend.clone()))
    }) else {
        return false;
    };
    let now = Instant::now();
    if due > now {
        backend.sleep_blocking(due.duration_since(now));
    }
    enqueue_due_timers(loop_ref);
    true
}

fn complete_task(
    task: Rc<RefCell<IcooTask>>,
    state: TaskState,
    result: Option<Value>,
    error: Option<String>,
    loop_ref: &Rc<RefCell<IcooEventLoop>>,
) {
    let awaiters = {
        let mut task_ref = task.borrow_mut();
        task_ref.state = state;
        task_ref.result = result;
        task_ref.error = error;
        std::mem::take(&mut task_ref.awaiters)
    };
    let mut event_loop = loop_ref.borrow_mut();
    for awaiter in awaiters {
        if awaiter.borrow().state == TaskState::Waiting {
            awaiter.borrow_mut().state = TaskState::Queued;
            event_loop.ready.push_back(awaiter);
        }
    }
}
