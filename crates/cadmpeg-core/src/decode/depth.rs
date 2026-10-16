// SPDX-License-Identifier: Apache-2.0
//! How much of a source document a decode transfers.

use std::fmt;

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// How much of the source document a decode transfers.
///
/// Depths are ordered: each one transfers everything a shallower depth
/// transfers. A caller requests a depth through the decode context, and the
/// codec records the depth it delivered on the decoded document.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum DecodeDepth {
    /// The container layer only: framing, identity, and native container
    /// records, without entity transfer.
    Container,
    /// Everything an exchange output consumes: B-rep, tessellation, product
    /// structure, placements, appearance, PMI, and attributes. Every design
    /// arena (features, sketches, parameters, configurations, spreadsheets,
    /// and their topology bindings) is empty.
    Geometry,
    /// Everything the codec transfers, including design intent.
    #[default]
    Full,
}

impl DecodeDepth {
    /// The wire name of this depth.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Container => "container",
            Self::Geometry => "geometry",
            Self::Full => "full",
        }
    }
}

impl fmt::Display for DecodeDepth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.pad(self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::DecodeDepth;

    #[test]
    fn depths_order_by_what_they_transfer() {
        assert!(DecodeDepth::Container < DecodeDepth::Geometry);
        assert!(DecodeDepth::Geometry < DecodeDepth::Full);
        assert_eq!(DecodeDepth::default(), DecodeDepth::Full);
    }

    #[test]
    fn wire_names_match_display() {
        for depth in [
            DecodeDepth::Container,
            DecodeDepth::Geometry,
            DecodeDepth::Full,
        ] {
            assert_eq!(
                serde_json::to_value(depth).expect("depth serializes"),
                serde_json::Value::String(depth.to_string())
            );
        }
    }
}
