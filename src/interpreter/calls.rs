use super::{
    check_value_type, coroutines::compile_coroutine_body, expect_arity, expect_int, tasks,
    Interpreter,
};
use crate::error::{IcooError, IcooResult};
use crate::lexer::token::Span;
use crate::native_modules;
use crate::runtime::env::{BindingKind, Environment};
use crate::runtime::value::*;
use std::cell::RefCell;
use std::rc::Rc;

impl Interpreter {
    pub fn call_global_main(&mut self) -> IcooResult<()> {
        let span = Span::new(1, 1, 0, 0);
        let main =
            self.env.borrow().get("main", span).map_err(|_| {
                IcooError::runtime("project entry must define fn main()", Some(span))
            })?;
        self.call_value(main, Vec::new(), span).map(|_| ())
    }

    pub(super) fn call_value(
        &mut self,
        callee: Value,
        args: Vec<Value>,
        span: Span,
    ) -> IcooResult<Value> {
        match callee {
            Value::Function(function) => self.call_function(function, args, span),
            Value::NativeFunction(function) => self.call_native_function(&function, args, span),
            Value::NativeMethod(method) => self.call_native_method(&method, args, span),
            Value::NativeModuleMethod(method) => {
                self.call_native_module_method(&method, args, span)
            }
            Value::Class(class) => self.call_class(class, args, span),
            _ => Err(IcooError::runtime(
                format!("type '{}' is not callable", callee.type_name()),
                Some(span),
            )),
        }
    }

    pub(super) fn call_function(
        &mut self,
        function: Rc<IcooFunction>,
        args: Vec<Value>,
        span: Span,
    ) -> IcooResult<Value> {
        self.check_timeout(span)?;
        if self.call_depth >= crate::runtime::limits::MAX_CALL_DEPTH {
            return Err(IcooError::runtime(
                format!(
                    "maximum call stack depth exceeded ({})",
                    crate::runtime::limits::MAX_CALL_DEPTH
                ),
                Some(span),
            ));
        }
        self.call_depth += 1;

        let bound_offset = usize::from(function.bound_self.is_some());
        let expected = function.decl.params.len().saturating_sub(bound_offset);
        if args.len() != expected {
            self.call_depth -= 1;
            return Err(IcooError::runtime(
                format!("expected {} arguments but got {}", expected, args.len()),
                Some(span),
            ));
        }

        let env = Environment::child(function.closure.clone());
        let mut arg_index = 0;
        for (param_index, param) in function.decl.params.iter().enumerate() {
            if param_index == 0 {
                if let Some(receiver) = &function.bound_self {
                    if let Some(type_hint) = &param.type_hint {
                        if let Err(err) = check_value_type(
                            receiver,
                            type_hint,
                            &format!("parameter '{}'", param.name.name),
                            span,
                        ) {
                            self.call_depth -= 1;
                            return Err(err);
                        }
                    }
                    env.borrow_mut().define(
                        param.name.name.clone(),
                        receiver.clone(),
                        true,
                        BindingKind::Const,
                    );
                    continue;
                }
            }
            let arg = args[arg_index].clone();
            if let Some(type_hint) = &param.type_hint {
                if let Err(err) = check_value_type(
                    &arg,
                    type_hint,
                    &format!("parameter '{}'", param.name.name),
                    span,
                ) {
                    self.call_depth -= 1;
                    return Err(err);
                }
            }
            env.borrow_mut()
                .define(param.name.name.clone(), arg, true, BindingKind::Mutable);
            arg_index += 1;
        }

        if function.decl.is_coroutine {
            self.call_depth -= 1;
            return Ok(Value::Coroutine(Rc::new(RefCell::new(IcooCoroutine {
                name: function.decl.name.name.clone(),
                return_type: function.decl.return_type.clone(),
                env,
                instructions: compile_coroutine_body(&function.decl.body),
                pc: 0,
                owner_task: None,
            }))));
        }

        let result = self.execute_block(&function.decl.body, env);
        let result = match result {
            Err(IcooError::Return(value)) => {
                if function.is_initializer {
                    Ok(function.bound_self.clone().unwrap_or(Value::Nil))
                } else {
                    self.check_function_return(&function, &value, span)?;
                    Ok(value)
                }
            }
            Err(mut err) => {
                err.push_frame(
                    function.decl.name.name.clone(),
                    function.decl.name.span,
                    crate::error::StackFrameKind::Function,
                );
                Err(err)
            }
            Ok(()) if function.is_initializer => {
                Ok(function.bound_self.clone().unwrap_or(Value::Nil))
            }
            Ok(()) => {
                self.check_function_return(&function, &Value::Nil, span)?;
                Ok(Value::Nil)
            }
        };
        self.call_depth -= 1;
        result
    }

    fn check_function_return(
        &self,
        function: &IcooFunction,
        value: &Value,
        span: Span,
    ) -> IcooResult<()> {
        if let Some(return_type) = &function.decl.return_type {
            check_value_type(
                value,
                return_type,
                &format!("return value of '{}'", function.decl.name.name),
                span,
            )?;
        }
        Ok(())
    }

    fn call_native_function(
        &mut self,
        function: &NativeFunction,
        args: Vec<Value>,
        span: Span,
    ) -> IcooResult<Value> {
        let valid_arity = if function.name == "EventLoop" {
            args.len() <= 1
        } else {
            args.len() == function.arity
        };
        if !valid_arity {
            return Err(IcooError::runtime(
                format!(
                    "expected {} arguments but got {}",
                    function.arity,
                    args.len()
                ),
                Some(span),
            ));
        }
        let name = function.name.clone();
        let result = self.dispatch_native_function(&function, args, span);
        match result {
            Err(mut err) if !matches!(err, IcooError::Return(_)) => {
                err.push_frame(name, span, crate::error::StackFrameKind::NativeFunction);
                Err(err)
            }
            other => other,
        }
    }

    fn dispatch_native_function(
        &mut self,
        function: &NativeFunction,
        args: Vec<Value>,
        span: Span,
    ) -> IcooResult<Value> {
        match function.name.as_str() {
            "print" => {
                (self.output)(args[0].display());
                Ok(Value::Nil)
            }
            "len" => self.call_native_method(
                &NativeMethod {
                    name: "len".to_string(),
                    receiver: args[0].clone(),
                },
                Vec::new(),
                span,
            ),
            "str" => Ok(Value::String(args[0].display())),
            "type" => Ok(Value::String(args[0].type_name())),
            "EventLoop" => {
                let workers = if args.is_empty() {
                    std::thread::available_parallelism()
                        .map(|count| count.get())
                        .unwrap_or(1)
                } else {
                    let workers = expect_int(&args[0], span)?;
                    if workers <= 0 {
                        return Err(IcooError::runtime(
                            "EventLoop() worker count must be positive",
                            Some(span),
                        ));
                    }
                    workers as usize
                };
                let event_loop = IcooEventLoop::new_tokio(workers)
                    .map_err(|message| IcooError::runtime(message, Some(span)))?;
                Ok(Value::EventLoop(Rc::new(RefCell::new(event_loop))))
            }
            "current_loop" => self
                .current_loop
                .as_ref()
                .cloned()
                .map(Value::EventLoop)
                .ok_or_else(|| {
                    IcooError::runtime(
                        "current_loop() can only be used inside a running task",
                        Some(span),
                    )
                }),
            "sleep" => {
                let millis = expect_int(&args[0], span)?;
                if millis < 0 {
                    return Err(IcooError::runtime(
                        "sleep() expects non-negative milliseconds",
                        Some(span),
                    ));
                }
                let Some(loop_ref) = self.current_loop.clone() else {
                    return Err(IcooError::runtime(
                        "sleep() can only be used inside a running task",
                        Some(span),
                    ));
                };
                Ok(Value::Task(tasks::schedule_sleep_task(
                    loop_ref,
                    millis as u64,
                )))
            }
            "int" => match &args[0] {
                Value::Int(value) => Ok(Value::Int(*value)),
                Value::Float(value) => Ok(Value::Int(*value as i64)),
                Value::Bool(value) => Ok(Value::Int(i64::from(*value))),
                Value::String(value) => value.parse::<i64>().map(Value::Int).map_err(|_| {
                    IcooError::runtime(format!("cannot convert '{}' to Int", value), Some(span))
                }),
                _ => Err(IcooError::runtime(
                    "value cannot be converted to Int",
                    Some(span),
                )),
            },
            "float" => match &args[0] {
                Value::Float(value) => Ok(Value::Float(*value)),
                Value::Int(value) => Ok(Value::Float(*value as f64)),
                Value::String(value) => value.parse::<f64>().map(Value::Float).map_err(|_| {
                    IcooError::runtime(format!("cannot convert '{}' to Float", value), Some(span))
                }),
                _ => Err(IcooError::runtime(
                    "value cannot be converted to Float",
                    Some(span),
                )),
            },
            _ => Err(IcooError::runtime("unknown native function", Some(span))),
        }
    }

    fn call_native_module_method(
        &mut self,
        method: &NativeModuleMethod,
        args: Vec<Value>,
        span: Span,
    ) -> IcooResult<Value> {
        let kind = native_modules::kind(&method.module);
        native_modules::call(self, kind, &method.name, args, span).unwrap_or_else(|| {
            Err(IcooError::runtime(
                format!(
                    "unknown native module method '{}.{}'",
                    method.module, method.name
                ),
                Some(span),
            ))
        })
    }

    fn call_native_method(
        &mut self,
        method: &NativeMethod,
        args: Vec<Value>,
        span: Span,
    ) -> IcooResult<Value> {
        match method.name.as_str() {
            "to_string" if !matches!(method.receiver, Value::Bytes(_)) => {
                expect_arity(&args, 0, span)?;
                return Ok(Value::String(method.receiver.display()));
            }
            "type_name" => {
                expect_arity(&args, 0, span)?;
                return Ok(Value::String(method.receiver.type_name()));
            }
            _ => {}
        }

        match &method.receiver {
            Value::String(value) => self.string_method(value, &method.name, args, span),
            Value::Bytes(bytes) => self.bytes_method(bytes.clone(), &method.name, args, span),
            Value::Buffer(buffer) => self.buffer_method(buffer.clone(), &method.name, args, span),
            Value::Array(values) => self.array_method(values.clone(), &method.name, args, span),
            Value::Map(values) => self.map_method(values.clone(), &method.name, args, span),
            Value::EventLoop(loop_ref) => {
                self.event_loop_method(loop_ref.clone(), &method.name, args, span)
            }
            Value::WebInoApp(app) => self.web_ino_app_method(app.clone(), &method.name, args, span),
            Value::WebInoResponse(response) => {
                self.web_ino_response_method(response.clone(), &method.name, args, span)
            }
            Value::Task(task) => self.task_method(task.clone(), &method.name, args, span),
            Value::Bool(value) if method.name == "to_int" => {
                expect_arity(&args, 0, span)?;
                Ok(Value::Int(i64::from(*value)))
            }
            Value::Int(value) if method.name == "to_float" => {
                expect_arity(&args, 0, span)?;
                Ok(Value::Float(*value as f64))
            }
            Value::Int(value) if method.name == "abs" => {
                expect_arity(&args, 0, span)?;
                Ok(Value::Int(value.abs()))
            }
            Value::Float(value) if method.name == "to_int" => {
                expect_arity(&args, 0, span)?;
                Ok(Value::Int(*value as i64))
            }
            Value::Float(value) if method.name == "abs" => {
                expect_arity(&args, 0, span)?;
                Ok(Value::Float(value.abs()))
            }
            _ => Err(IcooError::runtime(
                format!(
                    "type '{}' has no native method '{}'",
                    method.receiver.type_name(),
                    method.name
                ),
                Some(span),
            )),
        }
    }
}
