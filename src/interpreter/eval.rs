use super::{check_value_type, modules, value_equal, Interpreter};
use crate::error::{IcooError, IcooResult};
use crate::lexer::token::Span;
use crate::parser::ast::*;
use crate::runtime::env::{BindingKind, EnvRef, Environment};
use crate::runtime::value::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

impl Interpreter {
    pub(super) fn execute(&mut self, stmt: &Stmt) -> IcooResult<()> {
        match stmt {
            Stmt::ImportModule {
                source,
                alias,
                span,
            } => {
                let module = self.load_import_value(source, *span)?;
                self.env
                    .borrow_mut()
                    .define(alias.name.clone(), module, true, BindingKind::Const);
                Ok(())
            }
            Stmt::ImportNames {
                source,
                items,
                span,
            } => {
                let module = self.load_import_value(source, *span)?;
                for item in items {
                    let local = item.alias.as_ref().unwrap_or(&item.name);
                    let value = modules::imported_member(&module, &item.name.name, item.name.span)?;
                    self.env.borrow_mut().define(
                        local.name.clone(),
                        value,
                        true,
                        BindingKind::Const,
                    );
                }
                Ok(())
            }
            Stmt::ExportDecl(inner) => self.execute(inner),
            Stmt::Let(decl) => self.define_binding(decl, BindingKind::Mutable),
            Stmt::Const(decl) => self.define_binding(decl, BindingKind::Const),
            Stmt::Final(decl) => self.define_binding(decl, BindingKind::Final),
            Stmt::Function(decl) => {
                let function = IcooFunction {
                    decl: decl.clone(),
                    closure: self.env.clone(),
                    bound_self: None,
                    is_initializer: false,
                };
                self.env.borrow_mut().define(
                    decl.name.name.clone(),
                    Value::Function(Rc::new(function)),
                    true,
                    BindingKind::Const,
                );
                Ok(())
            }
            Stmt::Class(decl) => self.execute_class(decl),
            Stmt::TryCatch {
                try_block,
                catch_name,
                catch_block,
            } => match self.execute_block(try_block, Environment::child(self.env.clone())) {
                Ok(()) => Ok(()),
                Err(IcooError::Runtime { message, span }) => {
                    let catch_env = Environment::child(self.env.clone());
                    let error_text = IcooError::Runtime { message, span }.to_string();
                    catch_env.borrow_mut().define(
                        catch_name.name.clone(),
                        Value::String(error_text),
                        true,
                        BindingKind::Const,
                    );
                    self.execute_block(catch_block, catch_env)
                }
                Err(err) => Err(err),
            },
            Stmt::If {
                condition,
                then_branch,
                elifs,
                else_branch,
            } => {
                if self.eval(condition)?.truthy() {
                    self.execute_block(then_branch, Environment::child(self.env.clone()))
                } else {
                    for (condition, body) in elifs {
                        if self.eval(condition)?.truthy() {
                            return self.execute_block(body, Environment::child(self.env.clone()));
                        }
                    }
                    if let Some(body) = else_branch {
                        self.execute_block(body, Environment::child(self.env.clone()))
                    } else {
                        Ok(())
                    }
                }
            }
            Stmt::While { condition, body } => {
                while self.eval(condition)?.truthy() {
                    self.check_timeout(condition.span())?;
                    match self.execute_block(body, Environment::child(self.env.clone())) {
                        Err(IcooError::Break) => break,
                        Err(IcooError::Continue) => continue,
                        other => other?,
                    }
                }
                Ok(())
            }
            Stmt::Return { value, .. } => {
                let value = if let Some(value) = value {
                    self.eval(value)?
                } else {
                    Value::Nil
                };
                Err(IcooError::Return(value))
            }
            Stmt::Yield { span, .. } => Err(IcooError::runtime(
                "yield can only be used inside an async fn",
                Some(*span),
            )),
            Stmt::Break(_) => Err(IcooError::Break),
            Stmt::Continue(_) => Err(IcooError::Continue),
            Stmt::Expr(expr) => {
                self.eval(expr)?;
                Ok(())
            }
        }
    }

    fn define_binding(&mut self, decl: &BindingDecl, kind: BindingKind) -> IcooResult<()> {
        let (value, initialized) = if let Some(initializer) = &decl.initializer {
            let value = self.eval(initializer)?;
            if let Some(type_hint) = &decl.type_hint {
                check_value_type(
                    &value,
                    type_hint,
                    &format!("binding '{}'", decl.name.name),
                    decl.name.span,
                )?;
            }
            (value, true)
        } else {
            (Value::Nil, false)
        };
        self.env
            .borrow_mut()
            .define(decl.name.name.clone(), value, initialized, kind);
        Ok(())
    }

    pub(super) fn execute_block(&mut self, statements: &[Stmt], env: EnvRef) -> IcooResult<()> {
        let previous = self.env.clone();
        self.env = env;
        let mut result = Ok(());
        for stmt in statements {
            if let Err(err) = self.execute(stmt) {
                result = Err(err);
                break;
            }
        }
        self.env = previous;
        result
    }

    pub(super) fn eval(&mut self, expr: &Expr) -> IcooResult<Value> {
        match expr {
            Expr::Literal(literal, _) => Ok(match literal {
                Literal::Nil => Value::Nil,
                Literal::Bool(value) => Value::Bool(*value),
                Literal::Int(value) => Value::Int(*value),
                Literal::Float(value) => Value::Float(*value),
                Literal::String(value) => Value::String(value.clone()),
            }),
            Expr::Variable(name) => self.env.borrow().get(&name.name, name.span),
            Expr::Self_(span) => self.env.borrow().get("self", *span),
            Expr::Super(span) => self.env.borrow().get("super", *span),
            Expr::Array(values, _) => {
                let mut result = Vec::new();
                for value in values {
                    result.push(self.eval(value)?);
                }
                Ok(Value::Array(Rc::new(RefCell::new(result))))
            }
            Expr::Map(entries, _) => {
                let mut result = HashMap::new();
                for (key, value) in entries {
                    result.insert(key.clone(), self.eval(value)?);
                }
                Ok(Value::Map(Rc::new(RefCell::new(result))))
            }
            Expr::Template(parts, _) => {
                let mut result = String::new();
                for part in parts {
                    match part {
                        TemplatePart::Text(text) => result.push_str(text),
                        TemplatePart::Expr(expr) => result.push_str(&self.eval(expr)?.display()),
                    }
                }
                Ok(Value::String(result))
            }
            Expr::Unary { op, right, span } => {
                let right = self.eval(right)?;
                match op {
                    UnaryOp::Not => Ok(Value::Bool(!right.truthy())),
                    UnaryOp::Negate => match right {
                        Value::Int(value) => value
                            .checked_neg()
                            .map(Value::Int)
                            .ok_or_else(|| {
                                IcooError::runtime("integer overflow in negation", Some(*span))
                            }),
                        Value::Float(value) => Ok(Value::Float(-value)),
                        _ => Err(IcooError::runtime("operand must be a number", Some(*span))),
                    },
                }
            }
            Expr::Binary {
                left,
                op,
                right,
                span,
            } => {
                let left = self.eval(left)?;
                let right = self.eval(right)?;
                self.eval_binary(left, *op, right, *span)
            }
            Expr::Logical {
                left, op, right, ..
            } => {
                let left = self.eval(left)?;
                match op {
                    LogicalOp::Or if left.truthy() => Ok(left),
                    LogicalOp::And if !left.truthy() => Ok(left),
                    _ => self.eval(right),
                }
            }
            Expr::Assign {
                target,
                value,
                span,
            } => {
                let value = self.eval(value)?;
                match target.as_ref() {
                    Expr::Variable(name) => {
                        self.env
                            .borrow_mut()
                            .assign(&name.name, value.clone(), *span)?;
                        Ok(value)
                    }
                    Expr::Get { object, name, .. } => {
                        let object = self.eval(object)?;
                        self.set_property(object, &name.name, value.clone(), *span)?;
                        Ok(value)
                    }
                    _ => Err(IcooError::runtime("invalid assignment target", Some(*span))),
                }
            }
            Expr::Get { object, name, span } => {
                if matches!(object.as_ref(), Expr::Super(_)) {
                    return self.eval_super_get(&name.name, *span);
                }
                let object = self.eval(object)?;
                self.get_property(object, &name.name, *span)
            }
            Expr::Call { callee, args, span } => {
                let callee = self.eval(callee)?;
                let mut values = Vec::new();
                for arg in args {
                    values.push(self.eval(arg)?);
                }
                self.call_value(callee, values, *span)
            }
            Expr::Await { task, span } => {
                if self.current_task.is_none() {
                    return Err(IcooError::runtime(
                        "await can only be used inside an async fn",
                        Some(*span),
                    ));
                }
                let value = self.eval(task)?;
                let Value::Task(task) = value else {
                    return Err(IcooError::runtime("await expects a Task", Some(*span)));
                };
                if let Some(loop_ref) = &self.current_loop {
                    if task.borrow().loop_id != loop_ref.borrow().id {
                        return Err(IcooError::runtime(
                            "cannot await task from a different EventLoop",
                            Some(*span),
                        ));
                    }
                }
                let state = task.borrow().state;
                match state {
                    TaskState::Done => {
                        let result = task.borrow().result.clone().unwrap_or(Value::Nil);
                        Ok(result)
                    }
                    TaskState::Failed => {
                        let error = task
                            .borrow()
                            .error
                            .clone()
                            .unwrap_or_else(|| "task failed".to_string());
                        Err(IcooError::runtime(error, Some(*span)))
                    }
                    TaskState::Cancelled => {
                        Err(IcooError::runtime("task was cancelled", Some(*span)))
                    }
                    _ => Err(IcooError::Await(Value::Task(task))),
                }
            }
        }
    }

    fn eval_binary(
        &self,
        left: Value,
        op: BinaryOp,
        right: Value,
        span: Span,
    ) -> IcooResult<Value> {
        match op {
            BinaryOp::Add => match (left, right) {
                (Value::Int(a), Value::Int(b)) => a
                    .checked_add(b)
                    .map(Value::Int)
                    .ok_or_else(|| {
                        IcooError::runtime("integer overflow in addition", Some(span))
                    }),
                (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
                (Value::Int(a), Value::Float(b)) => Ok(Value::Float(a as f64 + b)),
                (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a + b as f64)),
                (Value::String(a), Value::String(b)) => Ok(Value::String(a + &b)),
                (Value::String(a), b) => Ok(Value::String(a + &b.display())),
                (a, Value::String(b)) => Ok(Value::String(a.display() + &b)),
                _ => Err(IcooError::runtime(
                    "operands must be numbers or strings",
                    Some(span),
                )),
            },
            BinaryOp::Subtract => numeric_checked(left, right, span, |a, b| {
                a.checked_sub(b).ok_or("integer overflow in subtraction")
            }, |a, b| Ok(a - b)),
            BinaryOp::Multiply => numeric_checked(left, right, span, |a, b| {
                a.checked_mul(b).ok_or("integer overflow in multiplication")
            }, |a, b| Ok(a * b)),
            BinaryOp::Divide => numeric_float(left, right, span, |a, b| a / b),
            BinaryOp::Remainder => numeric_checked(left, right, span, |a, b| {
                a.checked_rem(b).ok_or("integer remainder with zero divisor")
            }, |a, b| Ok(a % b)),
            BinaryOp::Equal => Ok(Value::Bool(value_equal(&left, &right))),
            BinaryOp::NotEqual => Ok(Value::Bool(!value_equal(&left, &right))),
            BinaryOp::Less => compare(left, right, span, |a, b| a < b),
            BinaryOp::LessEqual => compare(left, right, span, |a, b| a <= b),
            BinaryOp::Greater => compare(left, right, span, |a, b| a > b),
            BinaryOp::GreaterEqual => compare(left, right, span, |a, b| a >= b),
        }
    }
}

fn numeric_checked(
    left: Value,
    right: Value,
    span: Span,
    int_op: impl Fn(i64, i64) -> Result<i64, &'static str>,
    float_op: impl Fn(f64, f64) -> Result<f64, &'static str>,
) -> IcooResult<Value> {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => int_op(a, b)
            .map(Value::Int)
            .map_err(|msg| IcooError::runtime(msg, Some(span))),
        (Value::Float(a), Value::Float(b)) => float_op(a, b)
            .map(Value::Float)
            .map_err(|msg| IcooError::runtime(msg, Some(span))),
        (Value::Int(a), Value::Float(b)) => float_op(a as f64, b)
            .map(Value::Float)
            .map_err(|msg| IcooError::runtime(msg, Some(span))),
        (Value::Float(a), Value::Int(b)) => float_op(a, b as f64)
            .map(Value::Float)
            .map_err(|msg| IcooError::runtime(msg, Some(span))),
        _ => Err(IcooError::runtime("operands must be numbers", Some(span))),
    }
}

fn numeric_float(
    left: Value,
    right: Value,
    span: Span,
    float_op: impl Fn(f64, f64) -> f64,
) -> IcooResult<Value> {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => Ok(Value::Float(float_op(a as f64, b as f64))),
        (Value::Float(a), Value::Float(b)) => Ok(Value::Float(float_op(a, b))),
        (Value::Int(a), Value::Float(b)) => Ok(Value::Float(float_op(a as f64, b))),
        (Value::Float(a), Value::Int(b)) => Ok(Value::Float(float_op(a, b as f64))),
        _ => Err(IcooError::runtime("operands must be numbers", Some(span))),
    }
}

fn compare(
    left: Value,
    right: Value,
    span: Span,
    op: impl Fn(f64, f64) -> bool,
) -> IcooResult<Value> {
    let left = number_as_f64(left, span)?;
    let right = number_as_f64(right, span)?;
    Ok(Value::Bool(op(left, right)))
}

fn number_as_f64(value: Value, span: Span) -> IcooResult<f64> {
    match value {
        Value::Int(value) => Ok(value as f64),
        Value::Float(value) => Ok(value),
        _ => Err(IcooError::runtime("operand must be a number", Some(span))),
    }
}
