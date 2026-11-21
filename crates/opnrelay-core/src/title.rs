// SPDX-License-Identifier: AGPL-3.0-only
//! Generates human readable step titles from captured actions.

use crate::model::{Action, ActionType};

/// Maximum label length used inside a generated title.
const MAX_LABEL: usize = 60;

fn clean(label: &str) -> Option<String> {
    let collapsed: String = label.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        return None;
    }
    if collapsed.chars().count() > MAX_LABEL {
        let truncated: String = collapsed.chars().take(MAX_LABEL - 1).collect();
        Some(format!("{truncated}…"))
    } else {
        Some(collapsed)
    }
}

fn label(action: &Action) -> Option<String> {
    action.target_label.as_deref().and_then(clean)
}

/// Strips scheme, query and fragment from a URL for display: `host/path`.
pub fn display_url(url: &str) -> String {
    let without_scheme = url.split_once("://").map(|(_, r)| r).unwrap_or(url);
    let end = without_scheme
        .find(['?', '#'])
        .unwrap_or(without_scheme.len());
    without_scheme[..end].trim_end_matches('/').to_string()
}

/// Generate an English title for an action (e.g. `Click "Save"`).
pub fn generate(action: &Action) -> String {
    match action.action_type {
        ActionType::Click => match label(action) {
            Some(l) => format!("Click \"{l}\""),
            None => "Click here".to_string(),
        },
        ActionType::DoubleClick => match label(action) {
            Some(l) => format!("Double-click \"{l}\""),
            None => "Double-click here".to_string(),
        },
        ActionType::RightClick => match label(action) {
            Some(l) => format!("Right-click \"{l}\""),
            None => "Right-click here".to_string(),
        },
        ActionType::Type => {
            let text = action.typed_text.as_deref().and_then(clean);
            match (text, label(action)) {
                (Some(t), Some(l)) => format!("Type \"{t}\" in \"{l}\""),
                (None, Some(l)) => format!("Type in \"{l}\""),
                (Some(t), None) => format!("Type \"{t}\""),
                (None, None) => "Type text".to_string(),
            }
        }
        ActionType::KeyShortcut => match action.keys.as_deref().and_then(clean) {
            Some(k) => format!("Press {k}"),
            None => "Press a key".to_string(),
        },
        ActionType::Select => {
            let value = action.value_label.as_deref().and_then(clean);
            match (value, label(action)) {
                (Some(v), Some(l)) => format!("Select \"{v}\" from \"{l}\""),
                (Some(v), None) => format!("Select \"{v}\""),
                (None, Some(l)) => format!("Choose an option in \"{l}\""),
                (None, None) => "Select an option".to_string(),
            }
        }
        ActionType::Navigate => match action.url.as_deref() {
            Some(u) if !u.is_empty() => format!("Go to {}", display_url(u)),
            _ => "Go to the page".to_string(),
        },
        ActionType::SwitchApp => match action.app.as_deref().and_then(clean) {
            Some(a) => format!("Switch to {a}"),
            None => "Switch application".to_string(),
        },
        ActionType::Scroll => match label(action) {
            Some(l) => format!("Scroll to \"{l}\""),
            None => "Scroll down".to_string(),
        },
        ActionType::Other => label(action).unwrap_or_else(|| "Next step".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn act(t: ActionType) -> Action {
        Action::new(t)
    }

    #[test]
    fn click_titles() {
        let mut a = act(ActionType::Click);
        assert_eq!(generate(&a), "Click here");
        a.target_label = Some("  Save \n changes ".into());
        assert_eq!(generate(&a), "Click \"Save changes\"");
    }

    #[test]
    fn type_titles_respect_missing_text() {
        let mut a = act(ActionType::Type);
        a.target_label = Some("Employee ID".into());
        assert_eq!(generate(&a), "Type in \"Employee ID\"");
        a.typed_text = Some("12345".into());
        assert_eq!(generate(&a), "Type \"12345\" in \"Employee ID\"");
    }

    #[test]
    fn navigate_strips_query() {
        let mut a = act(ActionType::Navigate);
        a.url = Some("https://hr.example.com/onboarding/?token=secret#x".into());
        assert_eq!(generate(&a), "Go to hr.example.com/onboarding");
    }

    #[test]
    fn long_labels_truncate() {
        let mut a = act(ActionType::Click);
        a.target_label = Some("x".repeat(200));
        let t = generate(&a);
        assert!(t.chars().count() < 80);
        assert!(t.contains('…'));
    }

    #[test]
    fn select_and_keys() {
        let mut a = act(ActionType::Select);
        a.target_label = Some("Department".into());
        a.value_label = Some("EMS".into());
        assert_eq!(generate(&a), "Select \"EMS\" from \"Department\"");
        let mut k = act(ActionType::KeyShortcut);
        k.keys = Some("Ctrl+S".into());
        assert_eq!(generate(&k), "Press Ctrl+S");
    }
}
