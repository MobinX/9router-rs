//! Differential test helpers: normalize nondeterministic fields then compare.
use serde_json::Value;

const VOLATILE_KEYS: &[&str] = &[
    "id",
    "created",
    "timestamp",
    "request_id",
    "requestId",
    "etag",
    "syncedAt",
    "session_id",
    "sessionId",
    "nonce",
    "state",
    "code_verifier",
    "traceId",
];
const VOLATILE_SUBSTR: &[&str] = &["req_", "resp_", "chatcmpl-", "msg_", "call_"];

pub fn normalize(mut v: Value) -> Value {
    normalize_inner(&mut v);
    v
}

fn norm_str(s: &mut String) {
    if VOLATILE_SUBSTR.iter().any(|p| s.contains(p)) {
        *s = "<NORM>".to_string();
    }
}

fn normalize_inner(v: &mut Value) {
    match v {
        Value::Object(m) => {
            for (k, child) in m.iter_mut() {
                if VOLATILE_KEYS.contains(&k.as_str()) {
                    *child = Value::String("<NORM>".into());
                } else {
                    normalize_inner(child);
                    if let Value::String(s) = child {
                        norm_str(s);
                    }
                }
            }
        }
        Value::Array(a) => a.iter_mut().for_each(normalize_inner),
        Value::String(s) => norm_str(s),
        _ => {}
    }
}

/// Parse SSE stream into (event, normalized-data) pairs.
pub fn parse_sse(body: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut event = "message".to_string();
    for block in body.split("\n\n") {
        let mut data = String::new();
        for line in block.lines() {
            if let Some(e) = line.strip_prefix("event:") {
                event = e.trim().to_string();
            } else if let Some(d) = line.strip_prefix("data:") {
                if !data.is_empty() {
                    data.push('\n');
                }
                data.push_str(d.trim());
            }
        }
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        let norm = match serde_json::from_str::<Value>(&data) {
            Ok(v) => serde_json::to_string(&normalize(v)).unwrap_or(data.clone()),
            Err(_) => data.clone(),
        };
        out.push((event.clone(), norm));
        event = "message".to_string();
    }
    out
}

/// Structural shape of a JSON value: sorted key tree with scalar types.
pub fn shape(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<_> = m.keys().collect();
            keys.sort();
            Value::Object(
                keys.into_iter()
                    .map(|k| (k.clone(), shape(&m[k])))
                    .collect(),
            )
        }
        Value::Array(a) => Value::Array(a.iter().map(shape).collect()),
        Value::String(_) => Value::String("string".into()),
        Value::Number(_) => Value::String("number".into()),
        Value::Bool(_) => Value::String("bool".into()),
        Value::Null => Value::String("null".into()),
    }
}

/// Event-type sequence of an SSE body, e.g. ["message","message","[DONE]"].
pub fn sse_event_types(body: &str) -> Vec<String> {
    body.split("\n\n")
        .filter_map(|block| {
            let mut event: Option<String> = None;
            let mut has_data = false;
            for line in block.lines() {
                if let Some(e) = line.strip_prefix("event:") {
                    event = Some(e.trim().to_string());
                } else if let Some(d) = line.strip_prefix("data:") {
                    has_data = d.trim() == "[DONE]" || !d.trim().is_empty();
                    if d.trim() == "[DONE]" {
                        return Some("[DONE]".to_string());
                    }
                }
            }
            if has_data {
                Some(event.unwrap_or_else(|| "message".to_string()))
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_and_timestamps_normalized() {
        let v = serde_json::json!({"id":"chatcmpl-abc123","created":1700000000,"model":"gpt-4o"});
        let n = normalize(v);
        assert_eq!(n["id"], "<NORM>");
        assert_eq!(n["created"], "<NORM>");
        assert_eq!(n["model"], "gpt-4o");
    }

    #[test]
    fn nested_volatile_strings() {
        let v = serde_json::json!({"choices":[{"message":{"id":"msg_1"}}]});
        let n = normalize(v);
        assert_eq!(n["choices"][0]["message"]["id"], "<NORM>");
    }

    #[test]
    fn sse_done_skipped() {
        let body = "data: {\"a\":1}\n\ndata: [DONE]\n\n";
        assert_eq!(parse_sse(body).len(), 1);
    }

    #[test]
    fn shape_sorts_keys() {
        let v = serde_json::json!({"b":1,"a":"x"});
        assert_eq!(shape(&v), serde_json::json!({"a":"string","b":"number"}));
    }

    #[test]
    fn event_types_sequence() {
        let body = "event: a\ndata: 1\n\ndata: [DONE]\n\n";
        assert_eq!(sse_event_types(body), vec!["a", "[DONE]"]);
    }
}
