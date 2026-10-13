use serde::{Deserialize, Serialize};
use std::fmt;

/// The job stage. Serialized names match the v1 `board.json`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Stage {
    Spec,
    Build,
    Review,
    Approve,
    Merged,
    NeedsHuman,
}

impl Stage {
    pub fn as_str(self) -> &'static str {
        match self {
            Stage::Spec => "spec",
            Stage::Build => "build",
            Stage::Review => "review",
            Stage::Approve => "approve",
            Stage::Merged => "merged",
            Stage::NeedsHuman => "needs-human",
        }
    }
}

impl fmt::Display for Stage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
