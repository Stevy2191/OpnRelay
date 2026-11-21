// SPDX-License-Identifier: AGPL-3.0-only
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Kind of block a step represents in a guide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StepKind {
    /// A recorded (or manually added) user action, usually with a screenshot.
    #[default]
    Action,
    /// Free text paragraph.
    Text,
    /// Section heading.
    Heading,
    /// Helpful tip callout.
    Tip,
    /// Warning / alert callout.
    Alert,
}

impl StepKind {
    pub fn as_str(self) -> &'static str {
        match self {
            StepKind::Action => "action",
            StepKind::Text => "text",
            StepKind::Heading => "heading",
            StepKind::Tip => "tip",
            StepKind::Alert => "alert",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "action" => StepKind::Action,
            "text" => StepKind::Text,
            "heading" => StepKind::Heading,
            "tip" => StepKind::Tip,
            "alert" => StepKind::Alert,
            _ => return None,
        })
    }

    /// Whether this kind is numbered as a step in viewers and exports.
    pub fn is_numbered(self) -> bool {
        matches!(self, StepKind::Action)
    }
}

/// The type of user interaction that produced an action step.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionType {
    Click,
    DoubleClick,
    RightClick,
    Type,
    KeyShortcut,
    Select,
    Navigate,
    SwitchApp,
    Scroll,
    Other,
}

/// Metadata describing a captured action. All fields optional except `type`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Action {
    #[serde(rename = "type")]
    pub action_type: ActionType,
    /// Human readable label of the element (button text, field label...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_label: Option<String>,
    /// Accessibility role of the target (button, textbox, link...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_role: Option<String>,
    /// For `select`: chosen option.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value_label: Option<String>,
    /// Typed text, only present when the user opted in for this recording.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub typed_text: Option<String>,
    /// For key shortcuts, e.g. "Ctrl+S".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keys: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Click position in screenshot pixel coordinates.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<i32>,
    /// Offset into the guide's recording video, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video_ms: Option<u64>,
}

impl Action {
    pub fn new(action_type: ActionType) -> Self {
        Self {
            action_type,
            target_label: None,
            target_role: None,
            value_label: None,
            typed_text: None,
            keys: None,
            app: None,
            window_title: None,
            url: None,
            x: None,
            y: None,
            video_ms: None,
        }
    }
}

/// Axis aligned rectangle in screenshot pixel coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

/// Vector annotations drawn over a screenshot. Burned in only at export time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Annotations {
    /// Highlight box around the click target.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub highlight: Option<Rect>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub boxes: Vec<Rect>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub arrows: Vec<Arrow>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<TextLabel>,
    /// Optional crop applied to the screenshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crop: Option<Rect>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Arrow {
    pub from: (i32, i32),
    pub to: (i32, i32),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextLabel {
    pub x: i32,
    pub y: i32,
    pub text: String,
}

/// A single step as stored and exchanged between components.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Step {
    #[serde(default = "Uuid::now_v7")]
    pub id: Uuid,
    #[serde(default)]
    pub kind: StepKind,
    #[serde(default)]
    pub title: String,
    /// Markdown (sanitised subset).
    #[serde(default)]
    pub body_md: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<Action>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_id: Option<Uuid>,
    #[serde(default)]
    pub annotations: Annotations,
}

/// Guide lifecycle state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum GuideStatus {
    #[default]
    Draft,
    Published,
    Archived,
}

impl GuideStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            GuideStatus::Draft => "draft",
            GuideStatus::Published => "published",
            GuideStatus::Archived => "archived",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "draft" => GuideStatus::Draft,
            "published" => GuideStatus::Published,
            "archived" => GuideStatus::Archived,
            _ => return None,
        })
    }
}

/// Classification label printed on guides/exports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Classification {
    #[default]
    Internal,
    Confidential,
    Restricted,
}

impl Classification {
    pub fn as_str(self) -> &'static str {
        match self {
            Classification::Internal => "internal",
            Classification::Confidential => "confidential",
            Classification::Restricted => "restricted",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "internal" => Classification::Internal,
            "confidential" => Classification::Confidential,
            "restricted" => Classification::Restricted,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_json_roundtrip_with_defaults() {
        let json = r#"{"title":"Click \"Save\"","action":{"type":"click","target_label":"Save","x":10,"y":20}}"#;
        let step: Step = serde_json::from_str(json).unwrap();
        assert_eq!(step.kind, StepKind::Action);
        assert_eq!(step.action.as_ref().unwrap().action_type, ActionType::Click);
        let back = serde_json::to_string(&step).unwrap();
        let again: Step = serde_json::from_str(&back).unwrap();
        assert_eq!(step, again);
    }

    #[test]
    fn enums_parse_and_print() {
        for k in [
            StepKind::Action,
            StepKind::Text,
            StepKind::Heading,
            StepKind::Tip,
            StepKind::Alert,
        ] {
            assert_eq!(StepKind::parse(k.as_str()), Some(k));
        }
        assert_eq!(
            GuideStatus::parse("published"),
            Some(GuideStatus::Published)
        );
        assert_eq!(Classification::parse("nope"), None);
    }
}
