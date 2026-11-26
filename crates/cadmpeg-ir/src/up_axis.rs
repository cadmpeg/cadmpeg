// SPDX-License-Identifier: Apache-2.0
//! The document up axis.

use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::Serialize;

/// The model-space axis that points up in the source document's own view of
/// it: `Y` for a Y-up document, `Z` for a Z-up document. Stored geometry is
/// not rotated by it. `CadIr::up_axis` is `None` when the source states no up
/// axis.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, DeserializeName)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum UpAxis {
    /// The +Y axis points up.
    Y,
    /// The +Z axis points up.
    Z,
}

#[cfg(test)]
mod tests {
    use super::UpAxis;

    #[test]
    fn wire_form_is_lowercase_axis_name() {
        assert_eq!(serde_json::to_string(&UpAxis::Y).unwrap(), "\"y\"");
        assert_eq!(serde_json::to_string(&UpAxis::Z).unwrap(), "\"z\"");
        assert_eq!(serde_json::from_str::<UpAxis>("\"y\"").unwrap(), UpAxis::Y);
        assert_eq!(serde_json::from_str::<UpAxis>("\"z\"").unwrap(), UpAxis::Z);
        assert!(serde_json::from_str::<UpAxis>("\"x\"").is_err());
        assert!(serde_json::from_str::<UpAxis>("\"Y\"").is_err());
    }
}
