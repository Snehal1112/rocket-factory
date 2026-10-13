use crate::error::CoreError;
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

impl Stage {
    fn rank(self) -> u8 {
        match self {
            Stage::Spec => 0,
            Stage::Build => 1,
            Stage::Review => 2,
            Stage::Approve => 3,
            Stage::Merged => 4,
            Stage::NeedsHuman => 5,
        }
    }

    /// No backward move and no move out of merged, unless forced.
    /// Any live stage may park in needs-human, and needs-human may resume at any stage after spec.
    pub fn check_move(from: Stage, to: Stage, force: bool) -> Result<(), CoreError> {
        let bad = CoreError::StageMove { from, to };
        if force || from == to {
            return Ok(());
        }
        let allowed = match (from, to) {
            (Stage::Merged, _) => false,
            (_, Stage::NeedsHuman) => true,
            (Stage::NeedsHuman, Stage::Spec) => false,
            (Stage::NeedsHuman, _) => true,
            _ => to.rank() >= from.rank(),
        };
        if allowed {
            Ok(())
        } else {
            Err(bad)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Stage::*;

    #[test]
    fn forward_and_same_moves_are_allowed() {
        for (from, to) in [(Spec, Build), (Build, Review), (Review, Approve), (Approve, Merged), (Spec, Merged), (Build, Build)] {
            assert!(Stage::check_move(from, to, false).is_ok(), "{from} -> {to}");
        }
    }

    #[test]
    fn backward_moves_are_refused_unless_forced() {
        assert!(Stage::check_move(Review, Build, false).is_err());
        assert!(Stage::check_move(Approve, Spec, false).is_err());
        assert!(Stage::check_move(Review, Build, true).is_ok());
    }

    #[test]
    fn merged_is_final_unless_forced() {
        for to in [Spec, Build, Review, Approve, NeedsHuman] {
            assert!(Stage::check_move(Merged, to, false).is_err(), "merged -> {to}");
        }
        assert!(Stage::check_move(Merged, Build, true).is_ok());
    }

    #[test]
    fn needs_human_is_a_parking_stage() {
        for from in [Spec, Build, Review, Approve] {
            assert!(Stage::check_move(from, NeedsHuman, false).is_ok());
        }
        for to in [Build, Review, Approve, Merged] {
            assert!(Stage::check_move(NeedsHuman, to, false).is_ok(), "needs-human -> {to}");
        }
        assert!(Stage::check_move(NeedsHuman, Spec, false).is_err());
    }

    #[test]
    fn serde_names_match_the_v1_board() {
        let names: Vec<String> = [Spec, Build, Review, Approve, Merged, NeedsHuman]
            .iter()
            .map(|s| serde_json::to_string(s).expect("serialize"))
            .collect();
        assert_eq!(names, vec!["\"spec\"", "\"build\"", "\"review\"", "\"approve\"", "\"merged\"", "\"needs-human\""]);
    }
}
