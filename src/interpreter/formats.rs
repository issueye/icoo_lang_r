use crate::error::{IcooError, IcooResult};
use crate::lexer::token::Span;
use crate::runtime::value::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

pub(crate) fn value_to_json(value: &Value, span: Span) -> IcooResult<serde_json::Value> {
    match value {
        Value::Nil => Ok(serde_json::Value::Null),
        Value::Bool(value) => Ok(serde_json::Value::Bool(*value)),
        Value::Int(value) => Ok(serde_json::Value::Number((*value).into())),
        Value::Float(value) => serde_json::Number::from_f64(*value)
            .map(serde_json::Value::Number)
            .ok_or_else(|| {
                IcooError::runtime("Float value cannot be represented as JSON", Some(span))
            }),
        Value::String(value) => Ok(serde_json::Value::String(value.clone())),
        Value::Bytes(_) => Err(IcooError::runtime(
            "Bytes cannot be represented as JSON",
            Some(span),
        )),
        Value::Array(values) => values
            .borrow()
            .iter()
            .map(|value| value_to_json(value, span))
            .collect::<IcooResult<Vec<_>>>()
            .map(serde_json::Value::Array),
        Value::Map(values) => {
            let mut object = serde_json::Map::new();
            let values_ref = values.borrow();
            for (key, value) in values_ref.iter() {
                object.insert(key.clone(), value_to_json(value, span)?);
            }
            Ok(serde_json::Value::Object(object))
        }
        _ => Err(IcooError::runtime(
            format!("type '{}' cannot be represented as JSON", value.type_name()),
            Some(span),
        )),
    }
}

pub(crate) fn json_to_value(value: serde_json::Value, span: Span) -> IcooResult<Value> {
    match value {
        serde_json::Value::Null => Ok(Value::Nil),
        serde_json::Value::Bool(value) => Ok(Value::Bool(value)),
        serde_json::Value::Number(value) => {
            if let Some(value) = value.as_i64() {
                Ok(Value::Int(value))
            } else if let Some(value) = value.as_f64() {
                Ok(Value::Float(value))
            } else {
                Err(IcooError::runtime(
                    "JSON number cannot be represented as Int or Float",
                    Some(span),
                ))
            }
        }
        serde_json::Value::String(value) => Ok(Value::String(value)),
        serde_json::Value::Array(values) => values
            .into_iter()
            .map(|value| json_to_value(value, span))
            .collect::<IcooResult<Vec<_>>>()
            .map(|values| Value::Array(Rc::new(RefCell::new(values)))),
        serde_json::Value::Object(values) => {
            let mut result = HashMap::new();
            for (key, value) in values {
                result.insert(key, json_to_value(value, span)?);
            }
            Ok(Value::Map(Rc::new(RefCell::new(result))))
        }
    }
}

pub(crate) fn value_to_toml(value: &Value, span: Span) -> IcooResult<toml::Value> {
    match value {
        Value::Bool(value) => Ok(toml::Value::Boolean(*value)),
        Value::Int(value) => Ok(toml::Value::Integer(*value)),
        Value::Float(value) if value.is_finite() => Ok(toml::Value::Float(*value)),
        Value::Float(_) => Err(IcooError::runtime(
            "Float value cannot be represented as TOML",
            Some(span),
        )),
        Value::String(value) => Ok(toml::Value::String(value.clone())),
        Value::Bytes(_) => Err(IcooError::runtime(
            "Bytes cannot be represented as TOML",
            Some(span),
        )),
        Value::Array(values) => values
            .borrow()
            .iter()
            .map(|value| value_to_toml(value, span))
            .collect::<IcooResult<Vec<_>>>()
            .map(toml::Value::Array),
        Value::Map(values) => {
            let mut table = toml::map::Map::new();
            let values_ref = values.borrow();
            for (key, value) in values_ref.iter() {
                table.insert(key.clone(), value_to_toml(value, span)?);
            }
            Ok(toml::Value::Table(table))
        }
        Value::Nil => Err(IcooError::runtime(
            "Nil cannot be represented as TOML",
            Some(span),
        )),
        _ => Err(IcooError::runtime(
            format!("type '{}' cannot be represented as TOML", value.type_name()),
            Some(span),
        )),
    }
}

pub(crate) fn toml_to_value(value: toml::Value, span: Span) -> IcooResult<Value> {
    match value {
        toml::Value::String(value) => Ok(Value::String(value)),
        toml::Value::Integer(value) => Ok(Value::Int(value)),
        toml::Value::Float(value) => Ok(Value::Float(value)),
        toml::Value::Boolean(value) => Ok(Value::Bool(value)),
        toml::Value::Datetime(value) => Ok(Value::String(value.to_string())),
        toml::Value::Array(values) => values
            .into_iter()
            .map(|value| toml_to_value(value, span))
            .collect::<IcooResult<Vec<_>>>()
            .map(|values| Value::Array(Rc::new(RefCell::new(values)))),
        toml::Value::Table(values) => {
            let mut result = HashMap::new();
            for (key, value) in values {
                result.insert(key, toml_to_value(value, span)?);
            }
            Ok(Value::Map(Rc::new(RefCell::new(result))))
        }
    }
}
