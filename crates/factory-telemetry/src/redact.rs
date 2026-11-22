use serde_json::Value;

const MAX_MESSAGE: usize = 300;
const REDACTED: &str = "[redacted]";

const MARKERS: [&str; 10] = [
    "token",
    "key",
    "secret",
    "password",
    "passwd",
    "passphrase",
    "credential",
    "auth",
    "bearer",
    "cookie",
];

/// A name is sensitive when it contains a marker (any case) or has `pat` or
/// `pass` as a whole part split on `-` and `_`. `weighted_tokens` is the one
/// exception.
fn is_sensitive(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    if n == "weighted_tokens" {
        return false;
    }
    MARKERS.iter().any(|s| n.contains(s)) || n.split(['-', '_']).any(|p| p == "pat" || p == "pass")
}

/// Replace the value of a sensitive field with `"[redacted]"`.
pub fn redact_field(name: &str, value: Value) -> Value {
    if is_sensitive(name) {
        Value::String(REDACTED.to_string())
    } else {
        value
    }
}

/// Redact the values of sensitive long flags in a command. The value of
/// `--name=value` is redacted, and so is the word after a bare `--name`,
/// whatever it starts with. A hidden word that is itself a bare sensitive flag
/// hides the next word too. Short flags are never redacted.
pub fn redact_command(command: &str) -> String {
    let mut hide_next = false;
    let words: Vec<String> = command
        .split_whitespace()
        .map(|word| {
            let flag = word.strip_prefix("--");
            let bare_sensitive = flag.is_some_and(|n| !n.contains('=') && is_sensitive(n));
            let hidden = std::mem::replace(&mut hide_next, bare_sensitive);
            if hidden {
                return REDACTED.to_string();
            }
            match flag.and_then(|w| w.split_once('=')) {
                Some((name, _)) if is_sensitive(name) => format!("--{name}={REDACTED}"),
                _ => word.to_string(),
            }
        })
        .collect();
    words.join(" ")
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
    fn spaced_value_of_a_sensitive_flag_is_redacted() {
        assert_eq!(
            redact_command("cargo run --token abc --name x"),
            "cargo run --token [redacted] --name x"
        );
    }

    #[test]
    fn both_flag_forms_are_redacted_together() {
        assert_eq!(
            redact_command("tool --api-key abc --token=xyz"),
            "tool --api-key [redacted] --token=[redacted]"
        );
    }

    #[test]
    fn a_value_starting_with_a_dash_is_redacted() {
        assert_eq!(
            redact_command("tool --token -abc123"),
            "tool --token [redacted]"
        );
    }

    #[test]
    fn a_flag_after_a_sensitive_flag_is_hidden() {
        assert_eq!(
            redact_command("tool --token-stdin --check"),
            "tool --token-stdin [redacted]"
        );
    }

    #[test]
    fn two_sensitive_flags_hide_the_value() {
        assert_eq!(
            redact_command("tool --token --api-key abc"),
            "tool --token [redacted] [redacted]"
        );
    }

    #[test]
    fn a_sensitive_flag_at_the_end_is_kept() {
        assert_eq!(redact_command("tool --password"), "tool --password");
    }

    #[test]
    fn short_flags_and_paths_are_never_redacted() {
        let cmd = "cargo test -j4 -p factory-engine --manifest-path a/Cargo.toml";
        assert_eq!(redact_command(cmd), cmd);
    }

    #[test]
    fn a_bare_double_dash_is_not_sensitive() {
        assert_eq!(
            redact_command("tool -- --token abc"),
            "tool -- --token [redacted]"
        );
    }

    #[test]
    fn new_names_are_sensitive() {
        for name in [
            "auth",
            "authorization",
            "bearer",
            "credentials",
            "passwd",
            "passphrase",
            "cookie",
            "pat",
            "github_pat",
            "pass",
            "db-pass",
        ] {
            assert_eq!(
                redact_command(&format!("tool --{name} v")),
                format!("tool --{name} [redacted]"),
                "{name}"
            );
        }
    }

    #[test]
    fn names_that_only_contain_pat_or_pass_are_not_sensitive() {
        for name in [
            "path",
            "manifest_path",
            "passed",
            "bypass",
            "patch",
            "session_id",
        ] {
            assert_eq!(redact_field(name, json!("v")), json!("v"), "{name}");
        }
    }

    #[test]
    fn new_field_names_are_redacted() {
        for name in ["auth_header", "bearer", "db_pass", "github_pat"] {
            assert_eq!(
                redact_field(name, json!("v")),
                json!("[redacted]"),
                "{name}"
            );
        }
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
