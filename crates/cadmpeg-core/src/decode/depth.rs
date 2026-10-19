// SPDX-License-Identifier: Apache-2.0
//! The classes a caller requests from a decoder.

use std::fmt;

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The content a caller requests from a decoder.
///
/// The delivered document declares its independent content scope. Missing or
/// partially decoded content is described by the decode report.
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
    /// The classes a decoder must deliver to satisfy this request.
    pub const fn required_scope(self) -> super::ContentScope {
        match self {
            Self::Container => super::ContentScope::container(),
            Self::Geometry => super::ContentScope::geometry(),
            Self::Full => super::ContentScope::full(),
        }
    }

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
    fn requests_name_required_classes_without_constraining_delivery() {
        use crate::decode::ContentScope;
        assert_eq!(
            DecodeDepth::Container.required_scope(),
            ContentScope::container()
        );
        assert_eq!(
            DecodeDepth::Geometry.required_scope(),
            ContentScope::geometry()
        );
        assert_eq!(DecodeDepth::Full.required_scope(), ContentScope::full());
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
