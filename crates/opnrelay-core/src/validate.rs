// SPDX-License-Identifier: AGPL-3.0-only
//! Input validation shared by server and clients.

use crate::model::Step;

pub const MAX_TITLE_LEN: usize = 300;
pub const MAX_BODY_LEN: usize = 20_000;
pub const MAX_STEPS: usize = 1_000;
pub const MAX_GUIDE_TITLE_LEN: usize = 200;
pub const MAX_DESCRIPTION_LEN: usize = 5_000;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ValidationError {
    #[error("{field} must not be empty")]
    Empty { field: &'static str },
    #[error("{field} is too long (max {max})")]
    TooLong { field: &'static str, max: usize },
    #[error("too many steps (max {MAX_STEPS})")]
    TooManySteps,
    #[error("duplicate step id {0}")]
    DuplicateStep(uuid::Uuid),
}

pub fn guide_title(title: &str) -> Result<String, ValidationError> {
    let t = title.trim();
    if t.is_empty() {
        return Err(ValidationError::Empty { field: "title" });
    }
    if t.chars().count() > MAX_GUIDE_TITLE_LEN {
        return Err(ValidationError::TooLong {
            field: "title",
            max: MAX_GUIDE_TITLE_LEN,
        });
    }
    Ok(t.to_string())
}

pub fn description(d: &str) -> Result<String, ValidationError> {
    if d.chars().count() > MAX_DESCRIPTION_LEN {
        return Err(ValidationError::TooLong {
            field: "description",
            max: MAX_DESCRIPTION_LEN,
        });
    }
    Ok(d.trim().to_string())
}

pub fn steps(steps: &[Step]) -> Result<(), ValidationError> {
    if steps.len() > MAX_STEPS {
        return Err(ValidationError::TooManySteps);
    }
    let mut seen = std::collections::HashSet::with_capacity(steps.len());
    for s in steps {
        if !seen.insert(s.id) {
            return Err(ValidationError::DuplicateStep(s.id));
        }
        if s.title.chars().count() > MAX_TITLE_LEN {
            return Err(ValidationError::TooLong {
                field: "step title",
                max: MAX_TITLE_LEN,
            });
        }
        if s.body_md.chars().count() > MAX_BODY_LEN {
            return Err(ValidationError::TooLong {
                field: "step body",
                max: MAX_BODY_LEN,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Annotations, StepKind};

    fn step() -> Step {
        Step {
            id: uuid::Uuid::now_v7(),
            kind: StepKind::Text,
            title: "a".into(),
            body_md: String::new(),
            action: None,
            media_id: None,
            annotations: Annotations::default(),
        }
    }

    #[test]
    fn rejects_duplicates_and_long_titles() {
        let s = step();
        assert!(matches!(
            steps(&[s.clone(), s]),
            Err(ValidationError::DuplicateStep(_))
        ));
        let mut long = step();
        long.title = "x".repeat(MAX_TITLE_LEN + 1);
        assert!(steps(&[long]).is_err());
    }

    #[test]
    fn guide_title_trims() {
        assert_eq!(guide_title("  Hi ").unwrap(), "Hi");
        assert!(guide_title("   ").is_err());
    }
}
