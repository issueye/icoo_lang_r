use crate::error::{IcooError, IcooResult};
use crate::lexer::token::Span;
use crate::parser::ast::TypeRef;
use crate::runtime::value::Value;
use std::rc::Rc;

pub(crate) fn check_value_type(
    value: &Value,
    type_hint: &TypeRef,
    context: &str,
    span: Span,
) -> IcooResult<()> {
    if value_matches_type(value, &type_hint.name) {
        Ok(())
    } else {
        Err(IcooError::runtime(
            format!(
                "expected {} for {} but got {}",
                type_hint.display_name(),
                context,
                value.type_name()
            ),
            Some(span),
        ))
    }
}

fn value_matches_type(value: &Value, type_name: &str) -> bool {
    match type_name {
        "Any" => true,
        "Nil" => matches!(value, Value::Nil),
        "Bool" => matches!(value, Value::Bool(_)),
        "Int" => matches!(value, Value::Int(_)),
        "Float" => matches!(value, Value::Float(_)),
        "String" => matches!(value, Value::String(_)),
        "Bytes" => matches!(value, Value::Bytes(_)),
        "Buffer" => matches!(value, Value::Buffer(_)),
        "Array" => matches!(value, Value::Array(_)),
        "Map" => matches!(value, Value::Map(_)),
        "Function" => matches!(
            value,
            Value::Function(_) | Value::NativeFunction(_) | Value::NativeMethod(_)
        ),
        "Coroutine" => matches!(value, Value::Coroutine(_)),
        "Task" => matches!(value, Value::Task(_)),
        "EventLoop" => matches!(value, Value::EventLoop(_)),
        "WebInoApp" => matches!(value, Value::WebInoApp(_)),
        "WebInoResponse" => matches!(value, Value::WebInoResponse(_)),
        class_name => matches_instance_type(value, class_name),
    }
}

pub(crate) fn is_callable(value: &Value) -> bool {
    matches!(
        value,
        Value::Function(_)
            | Value::NativeFunction(_)
            | Value::NativeMethod(_)
            | Value::NativeModuleMethod(_)
    )
}

fn matches_instance_type(value: &Value, class_name: &str) -> bool {
    let Value::Instance(instance) = value else {
        return false;
    };
    let mut class = Some(instance.borrow().class.clone());
    while let Some(current) = class {
        if current.name == class_name {
            return true;
        }
        class = current.superclass.clone();
    }
    false
}

pub(crate) fn value_equal(left: &Value, right: &Value) -> bool {
    match (left, right) {
        (Value::Nil, Value::Nil) => true,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (Value::Int(a), Value::Int(b)) => a == b,
        (Value::Float(a), Value::Float(b)) => a == b,
        (Value::Int(a), Value::Float(b)) => (*a as f64) == *b,
        (Value::Float(a), Value::Int(b)) => *a == (*b as f64),
        (Value::String(a), Value::String(b)) => a == b,
        (Value::Bytes(a), Value::Bytes(b)) => a == b,
        (Value::Buffer(a), Value::Buffer(b)) => Rc::ptr_eq(a, b),
        (Value::Array(a), Value::Array(b)) => Rc::ptr_eq(a, b),
        (Value::Map(a), Value::Map(b)) => Rc::ptr_eq(a, b),
        (Value::Coroutine(a), Value::Coroutine(b)) => Rc::ptr_eq(a, b),
        (Value::Task(a), Value::Task(b)) => Rc::ptr_eq(a, b),
        (Value::EventLoop(a), Value::EventLoop(b)) => Rc::ptr_eq(a, b),
        (Value::WebInoApp(a), Value::WebInoApp(b)) => Rc::ptr_eq(a, b),
        (Value::WebInoResponse(a), Value::WebInoResponse(b)) => Rc::ptr_eq(a, b),
        (Value::Instance(a), Value::Instance(b)) => Rc::ptr_eq(a, b),
        (Value::Class(a), Value::Class(b)) => Rc::ptr_eq(a, b),
        _ => false,
    }
}
