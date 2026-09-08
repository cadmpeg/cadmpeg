// SPDX-License-Identifier: Apache-2.0
//! Families retained by structured type-81 value relations.

use serde::{Deserialize, Serialize};

use super::ParasolidAttributeFieldValueKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum StructuredValueKind {
    Points,
    Vectors,
    Directions,
    Axes,
    Tags,
    Unicode,
}

impl From<StructuredValueKind> for ParasolidAttributeFieldValueKind {
    fn from(kind: StructuredValueKind) -> Self {
        match kind {
            StructuredValueKind::Points => Self::Points,
            StructuredValueKind::Vectors => Self::Vectors,
            StructuredValueKind::Directions => Self::Directions,
            StructuredValueKind::Axes => Self::Axes,
            StructuredValueKind::Tags => Self::Tags,
            StructuredValueKind::Unicode => Self::Unicode,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::StructuredValueKind;

    #[test]
    fn structured_kinds_preserve_wire_and_reject_numeric_or_string_families() {
        for (kind, wire) in [
            (StructuredValueKind::Points, "\"points\""),
            (StructuredValueKind::Vectors, "\"vectors\""),
            (StructuredValueKind::Directions, "\"directions\""),
            (StructuredValueKind::Axes, "\"axes\""),
            (StructuredValueKind::Tags, "\"tags\""),
            (StructuredValueKind::Unicode, "\"unicode\""),
        ] {
            assert_eq!(serde_json::to_string(&kind).unwrap(), wire);
            assert_eq!(
                serde_json::from_str::<StructuredValueKind>(wire).unwrap(),
                kind
            );
        }
        for wire in ["\"unsigned_integers\"", "\"doubles\"", "\"string\""] {
            assert!(serde_json::from_str::<StructuredValueKind>(wire).is_err());
        }
    }
}
