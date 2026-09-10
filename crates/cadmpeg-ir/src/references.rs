// SPDX-License-Identifier: Apache-2.0
//! Shared local, external, and explicit-null reference targets.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Identity form of a drawing or semantic-annotation reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(from = "ReferenceTargetWire", into = "ReferenceTargetWire")]
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
#[serde(deny_unknown_fields)]
pub struct ReferenceSelection {
    /// Local, external, or explicit-null target identity.
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
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum ReferenceTargetWire {
    Null {},
    Local {
        target: String,
    },
    External {
        document: String,
        object: String,
    },
}

impl From<ReferenceTargetWire> for ReferenceTarget {
    fn from(wire: ReferenceTargetWire) -> Self {
        match wire {
            ReferenceTargetWire::Null {} => Self::Null,
            ReferenceTargetWire::Local { target } => Self::Local(target),
            ReferenceTargetWire::External { document, object } => {
                Self::External { document, object }
            }
        }
    }
}

impl From<ReferenceTarget> for ReferenceTargetWire {
    fn from(target: ReferenceTarget) -> Self {
        match target {
            ReferenceTarget::Null => Self::Null {},
            ReferenceTarget::Local(target) => Self::Local { target },
            ReferenceTarget::External { document, object } => Self::External { document, object },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ReferenceSelection, ReferenceTarget};

    #[test]
    fn a_reference_target_is_one_nested_tagged_object() {
        let cases = [
            (
                ReferenceTarget::Null,
                serde_json::json!({ "target": { "kind": "null" } }),
            ),
            (
                ReferenceTarget::Local("local-id".into()),
                serde_json::json!({ "target": { "kind": "local", "target": "local-id" } }),
            ),
            (
                ReferenceTarget::External {
                    document: "document".into(),
                    object: "object".into(),
                },
                serde_json::json!({
                    "target": {
                        "kind": "external",
                        "document": "document",
                        "object": "object"
                    }
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
            serde_json::json!({
                "target": { "kind": "local", "target": "local-id" },
                "subelements": ["Face1"]
            })
        );
    }

    #[test]
    fn a_reference_target_carries_no_key_of_another_form() {
        for wire in [
            serde_json::json!({ "target": {} }),
            serde_json::json!({ "target": { "kind": "null", "target": "local" } }),
            serde_json::json!({ "target": { "kind": "external", "document": "document" } }),
            serde_json::json!({
                "target": {
                    "kind": "local",
                    "target": "local",
                    "document": "document",
                    "object": "object"
                }
            }),
            serde_json::json!({ "is_null": true }),
            serde_json::json!({ "target": "local-id" }),
        ] {
            assert!(
                serde_json::from_value::<ReferenceSelection>(wire.clone()).is_err(),
                "{wire}"
            );
        }

        let bogus = serde_json::json!({
            "target": { "kind": "local", "target": "local-id" },
            "zz_bogus": 1
        });
        let error = serde_json::from_value::<ReferenceSelection>(bogus)
            .unwrap_err()
            .to_string();
        assert!(error.contains("zz_bogus"), "{error}");
    }
}
