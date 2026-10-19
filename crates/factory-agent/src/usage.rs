use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Token counts of one message or one run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub cache_read_input_tokens: u64,
    pub output_tokens: u64,
    /// The part of `cache_creation_input_tokens` written with the 1 hour lifetime.
    #[serde(default)]
    pub cache_creation_1h_input_tokens: u64,
}

fn field(v: &Value, key: &str) -> u64 {
    v.get(key).and_then(Value::as_u64).unwrap_or(0)
}

impl Usage {
    /// Reads a `usage` JSON object. Missing fields count as zero.
    pub fn from_value(v: &Value) -> Usage {
        Usage {
            input_tokens: field(v, "input_tokens"),
            cache_creation_input_tokens: field(v, "cache_creation_input_tokens"),
            cache_read_input_tokens: field(v, "cache_read_input_tokens"),
            output_tokens: field(v, "output_tokens"),
            cache_creation_1h_input_tokens: v
                .get("cache_creation")
                .map_or(0, |c| field(c, "ephemeral_1h_input_tokens")),
        }
    }

    pub fn add(&self, other: &Usage) -> Usage {
        Usage {
            input_tokens: self.input_tokens.saturating_add(other.input_tokens),
            cache_creation_input_tokens: self
                .cache_creation_input_tokens
                .saturating_add(other.cache_creation_input_tokens),
            cache_read_input_tokens: self
                .cache_read_input_tokens
                .saturating_add(other.cache_read_input_tokens),
            output_tokens: self.output_tokens.saturating_add(other.output_tokens),
            cache_creation_1h_input_tokens: self
                .cache_creation_1h_input_tokens
                .saturating_add(other.cache_creation_1h_input_tokens),
        }
    }

    /// Field-wise maximum. A streaming message only ever grows, so this is the latest value.
    pub fn max_with(&self, other: &Usage) -> Usage {
        Usage {
            input_tokens: self.input_tokens.max(other.input_tokens),
            cache_creation_input_tokens: self
                .cache_creation_input_tokens
                .max(other.cache_creation_input_tokens),
            cache_read_input_tokens: self
                .cache_read_input_tokens
                .max(other.cache_read_input_tokens),
            output_tokens: self.output_tokens.max(other.output_tokens),
            cache_creation_1h_input_tokens: self
                .cache_creation_1h_input_tokens
                .max(other.cache_creation_1h_input_tokens),
        }
    }

    /// Weighted tokens by the budget formula in `factory-core`.
    pub fn weighted(&self) -> u64 {
        factory_core::weighted_tokens(
            self.input_tokens,
            self.cache_creation_input_tokens,
            self.output_tokens,
            self.cache_read_input_tokens,
        )
    }

    pub fn is_zero(&self) -> bool {
        *self == Usage::default()
    }
}
