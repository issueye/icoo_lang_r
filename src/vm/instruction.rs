use crate::parser::ast::{BinaryOp, UnaryOp};
use crate::runtime::env::BindingKind;
use crate::runtime::value::Value;

#[derive(Debug, Clone)]
pub enum Instruction {
    Constant(Value),
    Nil,
    Load(String),
    Define {
        name: String,
        kind: BindingKind,
        initialized: bool,
    },
    Store(String),
    Pop,
    Unary(UnaryOp),
    Binary(BinaryOp),
    Jump(usize),
    JumpIfFalse(usize),
    JumpIfFalseKeep(usize),
    JumpIfTrueKeep(usize),
    EnterScope,
    ExitScope,
    Print,
    ToString,
}
