//! The decision whether a fix round runs on a stronger builder model.

/// Returns the model to store on the job, or `None` when the builder stays as it is.
/// A job with its own `--model` never escalates, and an escalated job stays escalated.
pub(crate) fn escalation_for(
    fix_rounds_spent: u32,
    after: u32,
    escalation_model: Option<&str>,
    job_model: Option<&str>,
    already: Option<&str>,
) -> Option<String> {
    if job_model.is_some() {
        return None;
    }
    if let Some(model) = already {
        return Some(model.to_string());
    }
    let model = escalation_model?;
    (fix_rounds_spent >= after).then(|| model.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_escalation_before_the_threshold() {
        assert_eq!(escalation_for(0, 1, Some("opus"), None, None), None);
    }

    #[test]
    fn escalates_at_the_threshold() {
        assert_eq!(
            escalation_for(1, 1, Some("opus"), None, None),
            Some("opus".to_string())
        );
    }

    #[test]
    fn zero_escalates_the_first_fix_round() {
        assert_eq!(
            escalation_for(0, 0, Some("opus"), None, None),
            Some("opus".to_string())
        );
    }

    #[test]
    fn a_job_model_never_escalates() {
        assert_eq!(escalation_for(5, 1, Some("opus"), Some("x"), None), None);
    }

    #[test]
    fn off_without_a_model() {
        assert_eq!(escalation_for(5, 1, None, None, None), None);
    }

    #[test]
    fn an_escalated_job_stays_escalated() {
        assert_eq!(
            escalation_for(5, 1, Some("opus"), None, Some("opus")),
            Some("opus".to_string())
        );
    }
}
