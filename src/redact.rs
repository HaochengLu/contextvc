use regex::Regex;
use serde_json::Value;
use std::sync::LazyLock;

static SECRET_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        Regex::new(r"(?i)(api[_-]?key|secret|token|password|passwd)\s*[:=]\s*\S+").unwrap(),
        Regex::new(r"(?i)bearer\s+[a-z0-9\-_\.=]+").unwrap(),
        Regex::new(r"sk-[a-zA-Z0-9]{20,}").unwrap(),
        Regex::new(r"ghp_[a-zA-Z0-9]{20,}").unwrap(),
        Regex::new(r"AKIA[0-9A-Z]{16}").unwrap(),
    ]
});

const REDACTED: &str = "[REDACTED]";

pub fn redact_text(input: &str) -> String {
    let mut out = input.to_string();
    for pattern in SECRET_PATTERNS.iter() {
        out = pattern.replace_all(&out, REDACTED).into_owned();
    }
    out
}

pub fn redact_value(value: &Value) -> Value {
    match value {
        Value::String(s) => Value::String(redact_text(s)),
        Value::Array(items) => Value::Array(items.iter().map(redact_value).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), redact_value(v)))
                .collect(),
        ),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_api_keys() {
        let text = format!("API_KEY={}{}", "sk-", "abcdefghijklmnopqrstuvwxyz123456");
        assert!(redact_text(&text).contains(REDACTED));
    }
}
