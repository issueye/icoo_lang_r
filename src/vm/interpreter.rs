use crate::error::{IcooError, IcooResult};
use crate::parser::ast::{BinaryOp, UnaryOp};
use crate::runtime::env::{EnvRef, Environment};
use crate::runtime::value::Value;
use crate::vm::compiler::CompiledProgram;
use crate::vm::instruction::Instruction;
use std::rc::Rc;

pub fn run_program_with_output<F>(program: &CompiledProgram, output: F) -> IcooResult<()>
where
    F: FnMut(String),
{
    VmInterpreter::new(output).run(program)
}

struct VmInterpreter<F>
where
    F: FnMut(String),
{
    stack: Vec<Value>,
    env: EnvRef,
    scopes: Vec<EnvRef>,
    output: F,
}

impl<F> VmInterpreter<F>
where
    F: FnMut(String),
{
    fn new(output: F) -> Self {
        Self {
            stack: Vec::new(),
            env: Environment::new(),
            scopes: Vec::new(),
            output,
        }
    }

    fn run(&mut self, program: &CompiledProgram) -> IcooResult<()> {
        let mut pc = 0;
        while pc < program.instructions.len() {
            match &program.instructions[pc] {
                Instruction::Constant(value) => self.stack.push(value.clone()),
                Instruction::Nil => self.stack.push(Value::Nil),
                Instruction::Load(name) => {
                    let value = self.env.borrow().get(name, default_span())?;
                    self.stack.push(value);
                }
                Instruction::Define {
                    name,
                    kind,
                    initialized,
                } => {
                    let value = self.pop_stack("define binding")?;
                    self.env
                        .borrow_mut()
                        .define(name.clone(), value, *initialized, *kind);
                }
                Instruction::Store(name) => {
                    let value = self.pop_stack("assign binding")?;
                    self.env
                        .borrow_mut()
                        .assign(name, value.clone(), default_span())?;
                    self.stack.push(value);
                }
                Instruction::Pop => {
                    self.pop_stack("discard expression result")?;
                }
                Instruction::Unary(op) => {
                    let value = self.pop_stack("evaluate unary operator")?;
                    self.stack.push(eval_unary(*op, value)?);
                }
                Instruction::Binary(op) => {
                    let right = self.pop_stack("evaluate binary operator")?;
                    let left = self.pop_stack("evaluate binary operator")?;
                    self.stack.push(eval_binary(left, *op, right)?);
                }
                Instruction::Jump(target) => {
                    pc = *target;
                    continue;
                }
                Instruction::JumpIfFalse(target) => {
                    let value = self.pop_stack("evaluate branch condition")?;
                    if !value.truthy() {
                        pc = *target;
                        continue;
                    }
                }
                Instruction::JumpIfFalseKeep(target) => {
                    let value = self.peek_stack("evaluate branch condition")?;
                    if !value.truthy() {
                        pc = *target;
                        continue;
                    }
                }
                Instruction::JumpIfTrueKeep(target) => {
                    let value = self.peek_stack("evaluate branch condition")?;
                    if value.truthy() {
                        pc = *target;
                        continue;
                    }
                }
                Instruction::EnterScope => {
                    self.scopes.push(self.env.clone());
                    self.env = Environment::child(self.env.clone());
                }
                Instruction::ExitScope => {
                    self.env = self
                        .scopes
                        .pop()
                        .ok_or_else(|| runtime_error("VM scope stack underflow"))?;
                }
                Instruction::Print => {
                    let value = self.pop_stack("print value")?;
                    (self.output)(value.display());
                    self.stack.push(Value::Nil);
                }
                Instruction::ToString => {
                    let value = self.pop_stack("convert to string")?;
                    self.stack.push(Value::String(value.display()));
                }
            }
            pc += 1;
        }
        Ok(())
    }

    fn pop_stack(&mut self, action: &str) -> IcooResult<Value> {
        self.stack
            .pop()
            .ok_or_else(|| runtime_error(format!("VM stack underflow while trying to {}", action)))
    }

    fn peek_stack(&self, action: &str) -> IcooResult<&Value> {
        self.stack
            .last()
            .ok_or_else(|| runtime_error(format!("VM stack underflow while trying to {}", action)))
    }
}

fn eval_unary(op: UnaryOp, value: Value) -> IcooResult<Value> {
    match op {
        UnaryOp::Not => Ok(Value::Bool(!value.truthy())),
        UnaryOp::Negate => match value {
            Value::Int(value) => Ok(Value::Int(-value)),
            Value::Float(value) => Ok(Value::Float(-value)),
            _ => Err(runtime_error("operand must be a number")),
        },
    }
}

fn eval_binary(left: Value, op: BinaryOp, right: Value) -> IcooResult<Value> {
    match op {
        BinaryOp::Add => match (left, right) {
            (Value::Int(a), Value::Int(b)) => Ok(Value::Int(a + b)),
            (Value::Float(a), Value::Float(b)) => Ok(Value::Float(a + b)),
            (Value::Int(a), Value::Float(b)) => Ok(Value::Float(a as f64 + b)),
            (Value::Float(a), Value::Int(b)) => Ok(Value::Float(a + b as f64)),
            (Value::String(a), Value::String(b)) => Ok(Value::String(a + &b)),
            (Value::String(a), b) => Ok(Value::String(a + &b.display())),
            (a, Value::String(b)) => Ok(Value::String(a.display() + &b)),
            _ => Err(runtime_error("operands must be numbers or strings")),
        },
        BinaryOp::Subtract => numeric(left, right, |a, b| a - b, |a, b| a - b),
        BinaryOp::Multiply => numeric(left, right, |a, b| a * b, |a, b| a * b),
        BinaryOp::Divide => numeric_float(left, right, |a, b| a / b),
        BinaryOp::Remainder => numeric(left, right, |a, b| a % b, |a, b| a % b),
        BinaryOp::Equal => Ok(Value::Bool(value_equal(&left, &right))),
        BinaryOp::NotEqual => Ok(Value::Bool(!value_equal(&left, &right))),
        BinaryOp::Less => compare(left, right, |a, b| a < b),
        BinaryOp::LessEqual => compare(left, right, |a, b| a <= b),
        BinaryOp::Greater => compare(left, right, |a, b| a > b),
        BinaryOp::GreaterEqual => compare(left, right, |a, b| a >= b),
    }
}

fn numeric(
    left: Value,
    right: Value,
    int_op: impl Fn(i64, i64) -> i64,
    float_op: impl Fn(f64, f64) -> f64,
) -> IcooResult<Value> {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => Ok(Value::Int(int_op(a, b))),
        (Value::Float(a), Value::Float(b)) => Ok(Value::Float(float_op(a, b))),
        (Value::Int(a), Value::Float(b)) => Ok(Value::Float(float_op(a as f64, b))),
        (Value::Float(a), Value::Int(b)) => Ok(Value::Float(float_op(a, b as f64))),
        _ => Err(runtime_error("operands must be numbers")),
    }
}

fn numeric_float(
    left: Value,
    right: Value,
    float_op: impl Fn(f64, f64) -> f64,
) -> IcooResult<Value> {
    match (left, right) {
        (Value::Int(a), Value::Int(b)) => Ok(Value::Float(float_op(a as f64, b as f64))),
        (Value::Float(a), Value::Float(b)) => Ok(Value::Float(float_op(a, b))),
        (Value::Int(a), Value::Float(b)) => Ok(Value::Float(float_op(a as f64, b))),
        (Value::Float(a), Value::Int(b)) => Ok(Value::Float(float_op(a, b as f64))),
        _ => Err(runtime_error("operands must be numbers")),
    }
}

fn compare(left: Value, right: Value, op: impl Fn(f64, f64) -> bool) -> IcooResult<Value> {
    Ok(Value::Bool(op(number_as_f64(left)?, number_as_f64(right)?)))
}

fn number_as_f64(value: Value) -> IcooResult<f64> {
    match value {
        Value::Int(value) => Ok(value as f64),
        Value::Float(value) => Ok(value),
        _ => Err(runtime_error("operand must be a number")),
    }
}

fn value_equal(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Nil, Value::Nil) => true,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Int(a), Value::Int(b)) => a == b,
        (Value::Float(a), Value::Float(b)) => a == b,
        (Value::Int(a), Value::Float(b)) => (*a as f64) == *b,
        (Value::Float(a), Value::Int(b)) => *a == (*b as f64),
        (Value::String(a), Value::String(b)) => a == b,
        (Value::Array(a), Value::Array(b)) => Rc::ptr_eq(a, b),
        (Value::Map(a), Value::Map(b)) => Rc::ptr_eq(a, b),
        _ => false,
    }
}

fn runtime_error(message: impl Into<String>) -> IcooError {
    IcooError::runtime(message, None)
}

fn default_span() -> crate::lexer::token::Span {
    crate::lexer::token::Span::new(0, 0, 0, 0)
}
