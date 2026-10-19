use serde_json::Value;

const MAX_MESSAGE: usize = 300;
const REDACTED: &str = "[redacted]";

fn is_sensitive(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    if n == "weighted_tokens" {
        return false;
    }
    ["token", "key", "secret", "password"]
        .iter()
        .any(|s| n.contains(s))
}

/// Replace the value of a sensitive field with `"[redacted]"`.
pub fn redact_field(name: &str, value: Value) -> Value {
    if is_sensitive(name) {
        Value::String(REDACTED.to_string())
    } else {
        value
    }
}

/// Redact the value of `--name=value` words whose name is sensitive.
pub fn redact_command(command: &str) -> String {
    command
        .split_whitespace()
        .map(
            |word| match word.strip_prefix("--").and_then(|w| w.split_once('=')) {
                Some((name, _)) if is_sensitive(name) => format!("--{name}={REDACTED}"),
                _ => word.to_string(),
            },
        )
        .collect::<Vec<_>>()
        .join(" ")
}

/// Keep the last 300 characters.
pub fn cap_message(msg: &str) -> String {
    let count = msg.chars().count();
    if count <= MAX_MESSAGE {
        return msg.to_string();
    }
    msg.chars().skip(count - MAX_MESSAGE).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn sensitive_field_names_are_redacted() {
        for n in ["api_token", "secret_key", "password", "Authorization_Key"] {
            assert_eq!(redact_field(n, json!("v")), json!("[redacted]"), "{n}");
        }
        assert_eq!(redact_field("job_id", json!("v")), json!("v"));
    }

    #[test]
    fn redaction_is_case_insensitive() {
        assert_eq!(redact_field("Password", json!(1)), json!("[redacted]"));
        assert_eq!(redact_field("API_TOKEN", json!(1)), json!("[redacted]"));
    }

    #[test]
    fn weighted_tokens_is_the_one_field_not_redacted() {
        assert_eq!(redact_field("weighted_tokens", json!(5)), json!(5));
    }

    #[test]
    fn command_value_of_a_sensitive_flag_is_redacted() {
        assert_eq!(
            redact_command("cargo run --token=abc --name=x"),
            "cargo run --token=[redacted] --name=x"
        );
    }

    #[test]
    fn bare_secret_word_in_a_command_is_kept() {
        assert_eq!(redact_command("echo hunter2"), "echo hunter2");
    }

    #[test]
    fn message_is_cut_to_300_chars() {
        let m = "a".repeat(250) + &"b".repeat(100);
        let out = cap_message(&m);
        assert_eq!(out.chars().count(), 300);
        assert!(out.ends_with('b'));
    }

    #[test]
    fn message_cut_keeps_whole_chars() {
        let m = "é".repeat(400);
        assert_eq!(cap_message(&m), "é".repeat(300));
    }
}
