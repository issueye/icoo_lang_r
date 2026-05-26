use super::{
    arity_error, clamp_slice_index, expect_arity, expect_byte_index, expect_bytes, expect_int,
    expect_string, normalize_index, value_equal, Interpreter,
};
use crate::error::{IcooError, IcooResult};
use crate::lexer::token::Span;
use crate::runtime::limits::{check_bytes_len, checked_bytes_total};
use crate::runtime::value::{bytes_to_base64, bytes_to_hex, Value};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

impl Interpreter {
    pub(super) fn has_native_method(&self, receiver: &Value, name: &str) -> bool {
        if matches!(name, "to_string" | "type_name") {
            return true;
        }
        match receiver {
            Value::String(_) => matches!(name, "len" | "is_empty" | "contains" | "to_bytes"),
            Value::Bytes(_) => matches!(
                name,
                "len" | "is_empty" | "slice" | "concat" | "equals" | "to_hex" | "to_base64"
            ),
            Value::Buffer(_) => matches!(
                name,
                "len"
                    | "is_empty"
                    | "append"
                    | "append_string"
                    | "slice"
                    | "to_bytes"
                    | "clear"
                    | "equals"
                    | "to_hex"
                    | "to_base64"
            ),
            Value::Array(_) => matches!(
                name,
                "len"
                    | "is_empty"
                    | "push"
                    | "pop"
                    | "shift"
                    | "unshift"
                    | "at"
                    | "includes"
                    | "index_of"
                    | "slice"
                    | "splice"
                    | "join"
                    | "reverse"
                    | "for_each"
                    | "map"
                    | "filter"
                    | "reduce"
                    | "find"
                    | "find_index"
                    | "some"
                    | "every"
            ),
            Value::Map(_) => matches!(
                name,
                "len"
                    | "is_empty"
                    | "size"
                    | "has"
                    | "get"
                    | "set"
                    | "delete"
                    | "clear"
                    | "keys"
                    | "values"
                    | "entries"
                    | "for_each"
            ),
            Value::EventLoop(_) => matches!(
                name,
                "spawn"
                    | "run"
                    | "run_until"
                    | "stop"
                    | "is_stopped"
                    | "backend_name"
                    | "worker_threads"
            ),
            Value::WebInoApp(_) => {
                matches!(
                    name,
                    "get"
                        | "post"
                        | "put"
                        | "delete"
                        | "options"
                        | "listen_once"
                        | "listen"
                        | "listen_with_workers"
                )
            }
            Value::WebInoResponse(_) => {
                matches!(
                    name,
                    "status"
                        | "header"
                        | "content_type"
                        | "send"
                        | "send_bytes"
                        | "json"
                        | "write"
                        | "write_bytes"
                        | "end"
                        | "download"
                )
            }
            Value::Task(_) => matches!(name, "is_done" | "is_failed" | "result" | "cancel"),
            Value::Bool(_) => matches!(name, "to_int"),
            Value::Int(_) => matches!(name, "to_float" | "abs"),
            Value::Float(_) => matches!(name, "to_int" | "abs"),
            Value::Instance(_)
            | Value::Nil
            | Value::Function(_)
            | Value::Coroutine(_)
            | Value::Class(_)
            | Value::Module(_)
            | Value::NativeModule(_) => true,
            Value::NativeFunction(_) | Value::NativeMethod(_) | Value::NativeModuleMethod(_) => {
                true
            }
        }
    }

    pub(super) fn string_method(
        &self,
        value: &str,
        name: &str,
        args: Vec<Value>,
        span: Span,
    ) -> IcooResult<Value> {
        match name {
            "len" => {
                expect_arity(&args, 0, span)?;
                Ok(Value::Int(value.chars().count() as i64))
            }
            "is_empty" => {
                expect_arity(&args, 0, span)?;
                Ok(Value::Bool(value.is_empty()))
            }
            "contains" => {
                expect_arity(&args, 1, span)?;
                let needle = expect_string(&args[0], span)?;
                Ok(Value::Bool(value.contains(&needle)))
            }
            "to_bytes" => {
                expect_arity(&args, 0, span)?;
                check_bytes_len(value.len(), span)?;
                Ok(Value::Bytes(Rc::new(value.as_bytes().to_vec())))
            }
            _ => Err(IcooError::runtime("unknown String method", Some(span))),
        }
    }

    pub(super) fn bytes_method(
        &self,
        bytes: Rc<Vec<u8>>,
        name: &str,
        args: Vec<Value>,
        span: Span,
    ) -> IcooResult<Value> {
        match name {
            "to_string" => {
                if args.len() > 1 {
                    return Err(arity_error("to_string", "0 or 1", args.len(), span));
                }
                let mode = if args.is_empty() {
                    "strict".to_string()
                } else {
                    expect_string(&args[0], span)?
                };
                match mode.as_str() {
                    "strict" => String::from_utf8(bytes.as_ref().clone())
                        .map(Value::String)
                        .map_err(|err| {
                            IcooError::runtime(
                                format!("Bytes.to_string() failed: {}", err),
                                Some(span),
                            )
                        }),
                    "lossy" => Ok(Value::String(String::from_utf8_lossy(&bytes).into_owned())),
                    "hex" => Ok(Value::String(bytes_to_hex(&bytes))),
                    _ => Err(IcooError::runtime(
                        "Bytes.to_string() mode must be 'strict', 'lossy', or 'hex'",
                        Some(span),
                    )),
                }
            }
            "len" => {
                expect_arity(&args, 0, span)?;
                Ok(Value::Int(bytes.len() as i64))
            }
            "is_empty" => {
                expect_arity(&args, 0, span)?;
                Ok(Value::Bool(bytes.is_empty()))
            }
            "slice" => {
                if args.is_empty() || args.len() > 2 {
                    return Err(arity_error("slice", "1 or 2", args.len(), span));
                }
                let start = expect_byte_index(expect_int(&args[0], span)?, bytes.len(), span)?;
                let end = if args.len() == 2 {
                    expect_byte_index(expect_int(&args[1], span)?, bytes.len(), span)?
                } else {
                    bytes.len()
                };
                if end < start {
                    return Err(IcooError::runtime(
                        "Bytes.slice() end must be greater than or equal to start",
                        Some(span),
                    ));
                }
                Ok(Value::Bytes(Rc::new(bytes[start..end].to_vec())))
            }
            "concat" => {
                expect_arity(&args, 1, span)?;
                let other = expect_bytes(&args[0], span)?;
                let total = checked_bytes_total(bytes.len(), other.len(), span)?;
                let mut result = Vec::with_capacity(total);
                result.extend_from_slice(&bytes);
                result.extend_from_slice(&other);
                Ok(Value::Bytes(Rc::new(result)))
            }
            "equals" => {
                expect_arity(&args, 1, span)?;
                let other = expect_bytes(&args[0], span)?;
                Ok(Value::Bool(bytes.as_slice() == other.as_slice()))
            }
            "to_hex" => {
                expect_arity(&args, 0, span)?;
                Ok(Value::String(bytes_to_hex(&bytes)))
            }
            "to_base64" => {
                expect_arity(&args, 0, span)?;
                Ok(Value::String(bytes_to_base64(&bytes)))
            }
            _ => Err(IcooError::runtime("unknown Bytes method", Some(span))),
        }
    }

    pub(super) fn buffer_method(
        &self,
        buffer: Rc<RefCell<Vec<u8>>>,
        name: &str,
        args: Vec<Value>,
        span: Span,
    ) -> IcooResult<Value> {
        match name {
            "to_string" => {
                if args.len() > 1 {
                    return Err(arity_error("to_string", "0 or 1", args.len(), span));
                }
                let mode = if args.is_empty() {
                    "strict".to_string()
                } else {
                    expect_string(&args[0], span)?
                };
                let bytes = buffer.borrow().clone();
                match mode.as_str() {
                    "strict" => String::from_utf8(bytes).map(Value::String).map_err(|err| {
                        IcooError::runtime(
                            format!("Buffer.to_string() failed: {}", err),
                            Some(span),
                        )
                    }),
                    "lossy" => Ok(Value::String(String::from_utf8_lossy(&bytes).into_owned())),
                    "hex" => Ok(Value::String(bytes_to_hex(&bytes))),
                    _ => Err(IcooError::runtime(
                        "Buffer.to_string() mode must be 'strict', 'lossy', or 'hex'",
                        Some(span),
                    )),
                }
            }
            "len" => {
                expect_arity(&args, 0, span)?;
                Ok(Value::Int(buffer.borrow().len() as i64))
            }
            "is_empty" => {
                expect_arity(&args, 0, span)?;
                Ok(Value::Bool(buffer.borrow().is_empty()))
            }
            "append" => {
                expect_arity(&args, 1, span)?;
                let bytes = expect_bytes(&args[0], span)?;
                checked_bytes_total(buffer.borrow().len(), bytes.len(), span)?;
                buffer.borrow_mut().extend_from_slice(&bytes);
                Ok(Value::Buffer(buffer))
            }
            "append_string" => {
                expect_arity(&args, 1, span)?;
                let value = expect_string(&args[0], span)?;
                checked_bytes_total(buffer.borrow().len(), value.len(), span)?;
                buffer.borrow_mut().extend_from_slice(value.as_bytes());
                Ok(Value::Buffer(buffer))
            }
            "slice" => {
                if args.is_empty() || args.len() > 2 {
                    return Err(arity_error("slice", "1 or 2", args.len(), span));
                }
                let bytes = buffer.borrow();
                let start = expect_byte_index(expect_int(&args[0], span)?, bytes.len(), span)?;
                let end = if args.len() == 2 {
                    expect_byte_index(expect_int(&args[1], span)?, bytes.len(), span)?
                } else {
                    bytes.len()
                };
                if end < start {
                    return Err(IcooError::runtime(
                        "Buffer.slice() end must be greater than or equal to start",
                        Some(span),
                    ));
                }
                Ok(Value::Bytes(Rc::new(bytes[start..end].to_vec())))
            }
            "to_bytes" => {
                expect_arity(&args, 0, span)?;
                check_bytes_len(buffer.borrow().len(), span)?;
                Ok(Value::Bytes(Rc::new(buffer.borrow().clone())))
            }
            "clear" => {
                expect_arity(&args, 0, span)?;
                buffer.borrow_mut().clear();
                Ok(Value::Nil)
            }
            "equals" => {
                expect_arity(&args, 1, span)?;
                let bytes = buffer.borrow();
                match &args[0] {
                    Value::Bytes(other) => Ok(Value::Bool(bytes.as_slice() == other.as_slice())),
                    Value::Buffer(other) => {
                        Ok(Value::Bool(bytes.as_slice() == other.borrow().as_slice()))
                    }
                    _ => Err(IcooError::runtime("expected Bytes argument", Some(span))),
                }
            }
            "to_hex" => {
                expect_arity(&args, 0, span)?;
                Ok(Value::String(bytes_to_hex(&buffer.borrow())))
            }
            "to_base64" => {
                expect_arity(&args, 0, span)?;
                Ok(Value::String(bytes_to_base64(&buffer.borrow())))
            }
            _ => Err(IcooError::runtime("unknown Buffer method", Some(span))),
        }
    }

    pub(super) fn array_method(
        &mut self,
        values: Rc<RefCell<Vec<Value>>>,
        name: &str,
        args: Vec<Value>,
        span: Span,
    ) -> IcooResult<Value> {
        match name {
            "len" => {
                expect_arity(&args, 0, span)?;
                Ok(Value::Int(values.borrow().len() as i64))
            }
            "is_empty" => {
                expect_arity(&args, 0, span)?;
                Ok(Value::Bool(values.borrow().is_empty()))
            }
            "push" => {
                expect_arity(&args, 1, span)?;
                values.borrow_mut().push(args[0].clone());
                Ok(Value::Nil)
            }
            "pop" => {
                expect_arity(&args, 0, span)?;
                Ok(values.borrow_mut().pop().unwrap_or(Value::Nil))
            }
            "shift" => {
                expect_arity(&args, 0, span)?;
                let mut values = values.borrow_mut();
                if values.is_empty() {
                    Ok(Value::Nil)
                } else {
                    Ok(values.remove(0))
                }
            }
            "unshift" => {
                expect_arity(&args, 1, span)?;
                let mut values_ref = values.borrow_mut();
                values_ref.insert(0, args[0].clone());
                Ok(Value::Int(values_ref.len() as i64))
            }
            "at" => {
                expect_arity(&args, 1, span)?;
                let index = normalize_index(expect_int(&args[0], span)?, values.borrow().len());
                Ok(index
                    .and_then(|index| values.borrow().get(index).cloned())
                    .unwrap_or(Value::Nil))
            }
            "includes" => {
                expect_arity(&args, 1, span)?;
                Ok(Value::Bool(
                    values
                        .borrow()
                        .iter()
                        .any(|value| value_equal(value, &args[0])),
                ))
            }
            "index_of" => {
                expect_arity(&args, 1, span)?;
                let index = values
                    .borrow()
                    .iter()
                    .position(|value| value_equal(value, &args[0]))
                    .map(|index| index as i64)
                    .unwrap_or(-1);
                Ok(Value::Int(index))
            }
            "slice" => {
                if args.is_empty() || args.len() > 2 {
                    return Err(arity_error("slice", "1 or 2", args.len(), span));
                }
                let values_ref = values.borrow();
                let len = values_ref.len() as i64;
                let start = clamp_slice_index(expect_int(&args[0], span)?, len);
                let end = if args.len() == 2 {
                    clamp_slice_index(expect_int(&args[1], span)?, len)
                } else {
                    len
                };
                let result = if end < start {
                    Vec::new()
                } else {
                    values_ref[start as usize..end as usize].to_vec()
                };
                Ok(Value::Array(Rc::new(RefCell::new(result))))
            }
            "splice" => {
                if args.len() < 2 {
                    return Err(arity_error("splice", "at least 2", args.len(), span));
                }
                let mut values_ref = values.borrow_mut();
                let len = values_ref.len() as i64;
                let start = clamp_slice_index(expect_int(&args[0], span)?, len) as usize;
                let delete_count = expect_int(&args[1], span)?.max(0) as usize;
                let delete_count = delete_count.min(values_ref.len().saturating_sub(start));
                let removed: Vec<Value> = values_ref
                    .splice(start..start + delete_count, args.iter().skip(2).cloned())
                    .collect();
                Ok(Value::Array(Rc::new(RefCell::new(removed))))
            }
            "join" => {
                if args.len() > 1 {
                    return Err(arity_error("join", "0 or 1", args.len(), span));
                }
                let separator = if args.is_empty() {
                    ",".to_string()
                } else {
                    expect_string(&args[0], span)?
                };
                let parts: Vec<String> = values.borrow().iter().map(Value::display).collect();
                Ok(Value::String(parts.join(&separator)))
            }
            "reverse" => {
                expect_arity(&args, 0, span)?;
                values.borrow_mut().reverse();
                Ok(Value::Array(values))
            }
            "for_each" => {
                expect_arity(&args, 1, span)?;
                let snapshot = values.borrow().clone();
                for (index, value) in snapshot.into_iter().enumerate() {
                    self.call_value(args[0].clone(), vec![value, Value::Int(index as i64)], span)?;
                }
                Ok(Value::Nil)
            }
            "map" => {
                expect_arity(&args, 1, span)?;
                let snapshot = values.borrow().clone();
                let mut result = Vec::new();
                for (index, value) in snapshot.into_iter().enumerate() {
                    result.push(self.call_value(
                        args[0].clone(),
                        vec![value, Value::Int(index as i64)],
                        span,
                    )?);
                }
                Ok(Value::Array(Rc::new(RefCell::new(result))))
            }
            "filter" => {
                expect_arity(&args, 1, span)?;
                let snapshot = values.borrow().clone();
                let mut result = Vec::new();
                for (index, value) in snapshot.into_iter().enumerate() {
                    if self
                        .call_value(
                            args[0].clone(),
                            vec![value.clone(), Value::Int(index as i64)],
                            span,
                        )?
                        .truthy()
                    {
                        result.push(value);
                    }
                }
                Ok(Value::Array(Rc::new(RefCell::new(result))))
            }
            "reduce" => {
                if args.is_empty() || args.len() > 2 {
                    return Err(arity_error("reduce", "1 or 2", args.len(), span));
                }
                let snapshot = values.borrow().clone();
                let mut iter = snapshot.into_iter().enumerate();
                let mut acc = if args.len() == 2 {
                    args[1].clone()
                } else if let Some((_, first)) = iter.next() {
                    first
                } else {
                    return Ok(Value::Nil);
                };
                for (index, value) in iter {
                    acc = self.call_value(
                        args[0].clone(),
                        vec![acc, value, Value::Int(index as i64)],
                        span,
                    )?;
                }
                Ok(acc)
            }
            "find" => {
                expect_arity(&args, 1, span)?;
                for (index, value) in values.borrow().clone().into_iter().enumerate() {
                    if self
                        .call_value(
                            args[0].clone(),
                            vec![value.clone(), Value::Int(index as i64)],
                            span,
                        )?
                        .truthy()
                    {
                        return Ok(value);
                    }
                }
                Ok(Value::Nil)
            }
            "find_index" => {
                expect_arity(&args, 1, span)?;
                for (index, value) in values.borrow().clone().into_iter().enumerate() {
                    if self
                        .call_value(args[0].clone(), vec![value, Value::Int(index as i64)], span)?
                        .truthy()
                    {
                        return Ok(Value::Int(index as i64));
                    }
                }
                Ok(Value::Int(-1))
            }
            "some" => {
                expect_arity(&args, 1, span)?;
                for (index, value) in values.borrow().clone().into_iter().enumerate() {
                    if self
                        .call_value(args[0].clone(), vec![value, Value::Int(index as i64)], span)?
                        .truthy()
                    {
                        return Ok(Value::Bool(true));
                    }
                }
                Ok(Value::Bool(false))
            }
            "every" => {
                expect_arity(&args, 1, span)?;
                for (index, value) in values.borrow().clone().into_iter().enumerate() {
                    if !self
                        .call_value(args[0].clone(), vec![value, Value::Int(index as i64)], span)?
                        .truthy()
                    {
                        return Ok(Value::Bool(false));
                    }
                }
                Ok(Value::Bool(true))
            }
            _ => Err(IcooError::runtime("unknown Array method", Some(span))),
        }
    }

    pub(super) fn map_method(
        &mut self,
        values: Rc<RefCell<HashMap<String, Value>>>,
        name: &str,
        args: Vec<Value>,
        span: Span,
    ) -> IcooResult<Value> {
        match name {
            "len" | "size" => {
                expect_arity(&args, 0, span)?;
                Ok(Value::Int(values.borrow().len() as i64))
            }
            "is_empty" => {
                expect_arity(&args, 0, span)?;
                Ok(Value::Bool(values.borrow().is_empty()))
            }
            "has" => {
                expect_arity(&args, 1, span)?;
                Ok(Value::Bool(
                    values
                        .borrow()
                        .contains_key(&expect_string(&args[0], span)?),
                ))
            }
            "get" => {
                expect_arity(&args, 1, span)?;
                Ok(values
                    .borrow()
                    .get(&expect_string(&args[0], span)?)
                    .cloned()
                    .unwrap_or(Value::Nil))
            }
            "set" => {
                expect_arity(&args, 2, span)?;
                values
                    .borrow_mut()
                    .insert(expect_string(&args[0], span)?, args[1].clone());
                Ok(Value::Map(values))
            }
            "delete" => {
                expect_arity(&args, 1, span)?;
                Ok(Value::Bool(
                    values
                        .borrow_mut()
                        .remove(&expect_string(&args[0], span)?)
                        .is_some(),
                ))
            }
            "clear" => {
                expect_arity(&args, 0, span)?;
                values.borrow_mut().clear();
                Ok(Value::Nil)
            }
            "keys" => {
                expect_arity(&args, 0, span)?;
                let result = values.borrow().keys().cloned().map(Value::String).collect();
                Ok(Value::Array(Rc::new(RefCell::new(result))))
            }
            "values" => {
                expect_arity(&args, 0, span)?;
                Ok(Value::Array(Rc::new(RefCell::new(
                    values.borrow().values().cloned().collect(),
                ))))
            }
            "entries" => {
                expect_arity(&args, 0, span)?;
                let result = values
                    .borrow()
                    .iter()
                    .map(|(key, value)| {
                        Value::Array(Rc::new(RefCell::new(vec![
                            Value::String(key.clone()),
                            value.clone(),
                        ])))
                    })
                    .collect();
                Ok(Value::Array(Rc::new(RefCell::new(result))))
            }
            "for_each" => {
                expect_arity(&args, 1, span)?;
                let snapshot: Vec<(String, Value)> = values
                    .borrow()
                    .iter()
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect();
                for (key, value) in snapshot {
                    self.call_value(args[0].clone(), vec![value, Value::String(key)], span)?;
                }
                Ok(Value::Nil)
            }
            _ => Err(IcooError::runtime("unknown Map method", Some(span))),
        }
    }
}
