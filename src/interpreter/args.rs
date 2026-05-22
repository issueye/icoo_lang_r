use crate::error::{IcooError, IcooResult};
use crate::lexer::token::Span;
use crate::runtime::value::Value;
use std::rc::Rc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(crate) fn expect_arity(args: &[Value], expected: usize, span: Span) -> IcooResult<()> {
    if args.len() == expected {
        Ok(())
    } else {
        Err(IcooError::runtime(
            format!("expected {} arguments but got {}", expected, args.len()),
            Some(span),
        ))
    }
}

pub(crate) fn arity_error(name: &str, expected: &str, got: usize, span: Span) -> IcooError {
    IcooError::runtime(
        format!(
            "method '{}' expected {} arguments but got {}",
            name, expected, got
        ),
        Some(span),
    )
}

pub(crate) fn expect_string(value: &Value, span: Span) -> IcooResult<String> {
    match value {
        Value::String(value) => Ok(value.clone()),
        _ => Err(IcooError::runtime("expected String argument", Some(span))),
    }
}

pub(crate) fn expect_bytes(value: &Value, span: Span) -> IcooResult<Rc<Vec<u8>>> {
    match value {
        Value::Bytes(value) => Ok(value.clone()),
        _ => Err(IcooError::runtime("expected Bytes argument", Some(span))),
    }
}

pub(crate) fn expect_int(value: &Value, span: Span) -> IcooResult<i64> {
    match value {
        Value::Int(value) => Ok(*value),
        _ => Err(IcooError::runtime("expected Int argument", Some(span))),
    }
}

pub(crate) fn expect_byte_index(index: i64, len: usize, span: Span) -> IcooResult<usize> {
    if index < 0 {
        return Err(IcooError::runtime(
            "byte index must be non-negative",
            Some(span),
        ));
    }
    let index = index as usize;
    if index > len {
        return Err(IcooError::runtime(
            format!("byte index {} is out of bounds for length {}", index, len),
            Some(span),
        ));
    }
    Ok(index)
}

pub(crate) fn expect_number(value: &Value, span: Span) -> IcooResult<f64> {
    match value {
        Value::Int(value) => Ok(*value as f64),
        Value::Float(value) => Ok(*value),
        _ => Err(IcooError::runtime("expected numeric argument", Some(span))),
    }
}

pub(crate) fn numeric_min_max(
    left: &Value,
    right: &Value,
    span: Span,
    op: impl Fn(f64, f64) -> f64,
) -> IcooResult<Value> {
    match (left, right) {
        (Value::Int(left), Value::Int(right)) => {
            Ok(Value::Int(op(*left as f64, *right as f64) as i64))
        }
        _ => Ok(Value::Float(op(
            expect_number(left, span)?,
            expect_number(right, span)?,
        ))),
    }
}

pub(crate) fn now_duration(span: Span) -> IcooResult<Duration> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| IcooError::runtime(format!("system time error: {}", err), Some(span)))
}

pub(crate) fn normalize_index(index: i64, len: usize) -> Option<usize> {
    let len = len as i64;
    let index = if index < 0 { len + index } else { index };
    if index < 0 || index >= len {
        None
    } else {
        Some(index as usize)
    }
}

pub(crate) fn clamp_slice_index(index: i64, len: i64) -> i64 {
    let index = if index < 0 { len + index } else { index };
    index.clamp(0, len)
}
