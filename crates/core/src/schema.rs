//! Tool parameter schema normalization for providers that require a plain object root.

use serde_json::{Map, Value};

const COMBINATORS: [&str; 3] = ["allOf", "anyOf", "oneOf"];

fn string_set(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect())
        .unwrap_or_default()
}

/// Adds `schema` for `name`, combining it with a different existing schema as `anyOf`.
fn merge_property(props: &mut Map<String, Value>, name: &str, schema: &Value) {
    match props.get_mut(name) {
        None => {
            props.insert(name.to_string(), schema.clone());
        }
        Some(existing) if existing == schema => {}
        Some(existing) => {
            if let Some(list) = existing.get_mut("anyOf").and_then(Value::as_array_mut) {
                if !list.contains(schema) {
                    list.push(schema.clone());
                }
            } else {
                let prev = existing.take();
                *existing = serde_json::json!({ "anyOf": [prev, schema] });
            }
        }
    }
}

/// Returns a schema whose root is `{"type": "object", "properties": {...}}` with no
/// root-level `allOf`/`anyOf`/`oneOf`. Nested schemas are left untouched.
///
/// - `allOf` branches are merged; their `required` lists are unioned.
/// - `anyOf`/`oneOf` branches are merged; only fields required by *every* branch stay required.
/// - A property that branches define differently becomes `{"anyOf": [...]}`.
pub fn object_root(schema: &Value) -> Value {
    let mut root = match schema {
        Value::Object(m) => m.clone(),
        _ => Map::new(),
    };
    let has_combinator = COMBINATORS.iter().any(|k| root.get(*k).is_some_and(Value::is_array));
    if !has_combinator {
        root.insert("type".into(), Value::String("object".into()));
        root.entry("properties").or_insert_with(|| Value::Object(Map::new()));
        return Value::Object(root);
    }

    let mut props = match root.remove("properties") {
        Some(Value::Object(p)) => p,
        _ => Map::new(),
    };
    let mut required = string_set(root.get("required"));

    if let Some(Value::Array(branches)) = root.remove("allOf") {
        for b in &branches {
            if let Some(p) = b.get("properties").and_then(Value::as_object) {
                for (name, s) in p {
                    merge_property(&mut props, name, s);
                }
            }
            for r in string_set(b.get("required")) {
                if !required.contains(&r) {
                    required.push(r);
                }
            }
        }
    }
    for key in ["anyOf", "oneOf"] {
        let Some(Value::Array(branches)) = root.remove(key) else { continue };
        let mut common: Option<Vec<String>> = None;
        for b in &branches {
            if let Some(p) = b.get("properties").and_then(Value::as_object) {
                for (name, s) in p {
                    merge_property(&mut props, name, s);
                }
            }
            let req = string_set(b.get("required"));
            common = Some(match common {
                None => req,
                Some(c) => c.into_iter().filter(|r| req.contains(r)).collect(),
            });
        }
        for r in common.unwrap_or_default() {
            if !required.contains(&r) {
                required.push(r);
            }
        }
    }

    root.insert("type".into(), Value::String("object".into()));
    root.insert("properties".into(), Value::Object(props));
    if required.is_empty() {
        root.remove("required");
    } else {
        root.insert("required".into(), Value::Array(required.into_iter().map(Value::String).collect()));
    }
    Value::Object(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn plain_objects_get_type_and_properties() {
        assert_eq!(object_root(&json!({})), json!({"type": "object", "properties": {}}));
        let s = json!({"type": "object", "properties": {"a": {"type": "string"}}, "required": ["a"]});
        assert_eq!(object_root(&s), s);
    }

    #[test]
    fn one_of_branches_are_merged_with_common_required() {
        let s = json!({
            "description": "Navigate or click",
            "oneOf": [
                {"type": "object", "properties": {"action": {"const": "goto"}, "url": {"type": "string"}}, "required": ["action", "url"]},
                {"type": "object", "properties": {"action": {"const": "click"}, "selector": {"type": "string"}}, "required": ["action", "selector"]}
            ]
        });
        let out = object_root(&s);
        assert_eq!(out["type"], "object");
        assert!(out.get("oneOf").is_none());
        assert_eq!(out["description"], "Navigate or click");
        assert_eq!(out["required"], json!(["action"]));
        assert_eq!(out["properties"]["action"], json!({"anyOf": [{"const": "goto"}, {"const": "click"}]}));
        assert_eq!(out["properties"]["url"], json!({"type": "string"}));
        assert_eq!(out["properties"]["selector"], json!({"type": "string"}));
    }

    #[test]
    fn all_of_unions_required_and_keeps_root_fields() {
        let s = json!({
            "properties": {"base": {"type": "number"}},
            "required": ["base"],
            "allOf": [
                {"properties": {"x": {"type": "string"}}, "required": ["x"]},
                {"properties": {"y": {"type": "string"}}}
            ],
            "additionalProperties": false
        });
        let out = object_root(&s);
        assert_eq!(out["required"], json!(["base", "x"]));
        assert_eq!(out["properties"].as_object().unwrap().len(), 3);
        assert_eq!(out["additionalProperties"], false);
    }

    #[test]
    fn nested_combinators_are_untouched() {
        let s = json!({"type": "object", "properties": {"v": {"anyOf": [{"type": "string"}, {"type": "null"}]}}});
        assert_eq!(object_root(&s), s);
    }
}
