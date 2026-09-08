// SPDX-License-Identifier: Apache-2.0
//! Shared local, external, and explicit-null reference targets.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Identity form of a drawing or semantic-annotation reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "ReferenceTargetWire", into = "ReferenceTargetWire")]
pub enum ReferenceTarget {
    /// Explicit null reference.
    Null,
    /// Identity within the current document.
    Local(String),
    /// Identity within another document.
    External {
        /// External document token.
        document: String,
        /// Stable object token within the external document.
        object: String,
    },
}

impl ReferenceTarget {
    /// Returns the local identity, when this target is local.
    #[must_use]
    pub fn local(&self) -> Option<&str> {
        match self {
            Self::Local(target) => Some(target),
            Self::Null | Self::External { .. } => None,
        }
    }

    /// Whether this is an explicit null reference.
    #[must_use]
    pub const fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }
}

/// One reference target and its ordered model-subelement selectors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct ReferenceSelection {
    /// Local, external, or explicit-null target identity.
    #[serde(flatten)]
    pub target: ReferenceTarget,
    /// Ordered model subelement selectors.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subelements: Vec<String>,
}

impl ReferenceSelection {
    /// Creates a target with its ordered subelement selectors.
    #[must_use]
    pub fn new(target: ReferenceTarget, subelements: Vec<String>) -> Self {
        Self {
            target,
            subelements,
        }
    }

    /// Returns the local identity, when this selection targets this document.
    #[must_use]
    pub fn local_target(&self) -> Option<&str> {
        self.target.local()
    }

    /// Whether this selection is an explicit null reference.
    #[must_use]
    pub const fn is_null(&self) -> bool {
        self.target.is_null()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
struct ReferenceTargetWire {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    external_document: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    external_object: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    is_null: bool,
}

impl TryFrom<ReferenceTargetWire> for ReferenceTarget {
    type Error = &'static str;

    fn try_from(wire: ReferenceTargetWire) -> Result<Self, Self::Error> {
        match (
            wire.is_null,
            wire.target,
            wire.external_document,
            wire.external_object,
        ) {
            (true, None, None, None) => Ok(Self::Null),
            (false, Some(target), None, None) => Ok(Self::Local(target)),
            (false, None, Some(document), Some(object)) => {
                Ok(Self::External { document, object })
            }
            _ => Err(
                "reference target requires exactly is_null, target, or external_document with external_object",
            ),
        }
    }
}

impl From<ReferenceTarget> for ReferenceTargetWire {
    fn from(target: ReferenceTarget) -> Self {
        match target {
            ReferenceTarget::Null => Self {
                target: None,
                external_document: None,
                external_object: None,
                is_null: true,
            },
            ReferenceTarget::Local(target) => Self {
                target: Some(target),
                external_document: None,
                external_object: None,
                is_null: false,
            },
            ReferenceTarget::External { document, object } => Self {
                target: None,
                external_document: Some(document),
                external_object: Some(object),
                is_null: false,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ReferenceSelection, ReferenceTarget};

    #[test]
    fn reference_targets_preserve_the_flat_wire_fields() {
        let cases = [
            (
                ReferenceTarget::Null,
                serde_json::json!({ "is_null": true }),
            ),
            (
                ReferenceTarget::Local("local-id".into()),
                serde_json::json!({ "target": "local-id" }),
            ),
            (
                ReferenceTarget::External {
                    document: "document".into(),
                    object: "object".into(),
                },
                serde_json::json!({
                    "external_document": "document",
                    "external_object": "object"
                }),
            ),
        ];
        for (target, wire) in cases {
            let selection = ReferenceSelection::new(target.clone(), Vec::new());
            assert_eq!(serde_json::to_value(selection).unwrap(), wire);
            let decoded: ReferenceSelection = serde_json::from_value(wire).unwrap();
            assert_eq!(decoded.target, target);
        }

        let selection = ReferenceSelection::new(
            ReferenceTarget::Local("local-id".into()),
            vec!["Face1".into()],
        );
        assert_eq!(
            serde_json::to_value(selection).unwrap(),
            serde_json::json!({ "target": "local-id", "subelements": ["Face1"] })
        );
    }

    #[test]
    fn reference_targets_reject_mixed_wire_forms() {
        for wire in [
            serde_json::json!({}),
            serde_json::json!({ "target": "local", "is_null": true }),
            serde_json::json!({ "external_document": "document" }),
            serde_json::json!({
                "target": "local",
                "external_document": "document",
                "external_object": "object"
            }),
        ] {
            assert!(serde_json::from_value::<ReferenceSelection>(wire).is_err());
        }
    }
}
