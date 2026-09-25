//! Small typed accessors over `serde_json` objects with protocol-style errors.

use serde_json::{Map, Value};
use owo_core::ModelError;

pub fn invalid(msg: impl Into<String>) -> ModelError {
    ModelError::invalid_request(msg)
}

fn type_err(key: &str, expected: &str) -> ModelError {
    invalid(format!("`{key}` must be {expected}"))
}

pub fn take_string(obj: &mut Map<String, Value>, key: &str) -> Result<Option<String>, ModelError> {
    match obj.remove(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(type_err(key, "a string")),
    }
}

pub fn take_bool(obj: &mut Map<String, Value>, key: &str) -> Result<Option<bool>, ModelError> {
    match obj.remove(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(b)) => Ok(Some(b)),
        Some(_) => Err(type_err(key, "a boolean")),
    }
}

pub fn take_f64(obj: &mut Map<String, Value>, key: &str) -> Result<Option<f64>, ModelError> {
    match obj.remove(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => n.as_f64().map(Some).ok_or_else(|| type_err(key, "a number")),
        Some(_) => Err(type_err(key, "a number")),
    }
}

pub fn take_u32(obj: &mut Map<String, Value>, key: &str) -> Result<Option<u32>, ModelError> {
    match obj.remove(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => n
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
            .map(Some)
            .ok_or_else(|| type_err(key, "a non-negative integer")),
        Some(_) => Err(type_err(key, "a non-negative integer")),
    }
}

pub fn take_i64(obj: &mut Map<String, Value>, key: &str) -> Result<Option<i64>, ModelError> {
    match obj.remove(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(n)) => n.as_i64().map(Some).ok_or_else(|| type_err(key, "an integer")),
        Some(_) => Err(type_err(key, "an integer")),
    }
}

pub fn str_field<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

pub fn u64_field(v: &Value, path: &[&str]) -> Option<u64> {
    let mut cur = v;
    for key in path {
        cur = cur.get(key)?;
    }
    cur.as_u64()
}
