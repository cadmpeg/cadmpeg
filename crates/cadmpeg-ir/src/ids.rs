// SPDX-License-Identifier: Apache-2.0
//! Typed string identifiers for the IR graph.
//!
//! Each identity kind wraps a string in a distinct newtype, preventing references
//! between incompatible entity arenas and state-local member sets. Entity IDs
//! must be stable and globally unique within a document. State-local IDs need
//! only be unique within their owning state.
//!
//! Entity IDs follow `<format>:<scope>:<kind>#<key>` (exactly three colon
//! components before `#`). Use [`is_valid_identity`] / [`format_identity`] at
//! mint time; validation repeats the same grammar.

#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

fn deserialize_entity_id<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<String, D::Error> {
    let value = String::deserialize(deserializer)?;
    if is_valid_identity(&value) {
        Ok(value)
    } else {
        Err(serde::de::Error::custom(IdentityError::InvalidId { value }))
    }
}

fn deserialize_local_id<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<String, D::Error> {
    let value = String::deserialize(deserializer)?;
    if value.is_empty() || value.chars().any(char::is_whitespace) {
        Err(serde::de::Error::custom(IdentityError::InvalidId { value }))
    } else {
        Ok(value)
    }
}
use std::fmt::{self, Display};

/// True when `id` matches `<format>:<scope>:<kind>#<key>`.
///
/// The key is non-empty, contains no `#`, and the whole id has no whitespace.
#[must_use]
pub fn is_valid_identity(id: &str) -> bool {
    let Some((namespace, key)) = id.split_once('#') else {
        return false;
    };
    if key.is_empty() || key.contains('#') || id.chars().any(char::is_whitespace) {
        return false;
    }
    let mut components = namespace.split(':');
    components.next().is_some_and(|value| !value.is_empty())
        && components.next().is_some_and(|value| !value.is_empty())
        && components.next().is_some_and(|value| !value.is_empty())
        && components.next().is_none()
}

/// Format a three-component identity and reject grammar violations.
///
/// # Errors
///
/// Returns [`IdentityError`] when any component is empty, contains `:`, `#`, or
/// whitespace, or when the composed string fails [`is_valid_identity`].
pub fn format_identity(
    format: &str,
    scope: &str,
    kind: &str,
    key: impl Display,
) -> Result<String, IdentityError> {
    for (label, part) in [("format", format), ("scope", scope), ("kind", kind)] {
        if part.is_empty()
            || part.contains(':')
            || part.contains('#')
            || part.chars().any(char::is_whitespace)
        {
            return Err(IdentityError::InvalidComponent {
                label,
                value: part.to_owned(),
            });
        }
    }
    let key = key.to_string();
    if key.is_empty() || key.contains('#') || key.chars().any(char::is_whitespace) {
        return Err(IdentityError::InvalidKey { value: key });
    }
    let id = format!("{format}:{scope}:{kind}#{key}");
    if !is_valid_identity(&id) {
        return Err(IdentityError::InvalidId { value: id });
    }
    Ok(id)
}

/// Failure to mint an entity identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentityError {
    /// A colon-separated namespace component is empty or contains separators.
    InvalidComponent {
        /// Component name (`format`, `scope`, or `kind`).
        label: &'static str,
        /// Rejected value.
        value: String,
    },
    /// The `#` key is empty or contains `#` / whitespace.
    InvalidKey {
        /// Rejected key.
        value: String,
    },
    /// Composed id failed [`is_valid_identity`].
    InvalidId {
        /// Rejected id.
        value: String,
    },
}

impl fmt::Display for IdentityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidComponent { label, value } => {
                write!(f, "identity {label} component is invalid: {value:?}")
            }
            Self::InvalidKey { value } => write!(f, "identity key is invalid: {value:?}"),
            Self::InvalidId { value } => write!(f, "identity is invalid: {value:?}"),
        }
    }
}

impl std::error::Error for IdentityError {}

macro_rules! id_type {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
        #[cfg_attr(feature = "schema", derive(JsonSchema))]
        #[serde(transparent)]
        pub struct $name(#[serde(deserialize_with = "deserialize_entity_id")] pub String);

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                crate::schema::serialize_reference_id(&self.0, serializer)
            }
        }

        impl $name {
            /// Mint an identity that matches `<format>:<scope>:<kind>#<key>`.
            pub fn mint(value: impl Into<String>) -> Result<Self, IdentityError> {
                let value = value.into();
                if !is_valid_identity(&value) {
                    return Err(IdentityError::InvalidId { value });
                }
                Ok(Self(value))
            }

            /// Borrow the underlying id string.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl<S: Into<String>> From<S> for $name {
            fn from(value: S) -> Self {
                Self::mint(value.into()).unwrap_or_else(|error| panic!("{error}"))
            }
        }
    };
}

macro_rules! local_id_type {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[cfg_attr(feature = "schema", derive(JsonSchema))]
        #[serde(transparent)]
        pub struct $name(#[serde(deserialize_with = "deserialize_local_id")] pub String);

        impl $name {
            /// Mint a non-empty state-local identity.
            pub fn mint(value: impl Into<String>) -> Result<Self, IdentityError> {
                let value = value.into();
                if value.is_empty() || value.chars().any(char::is_whitespace) {
                    return Err(IdentityError::InvalidId { value });
                }
                Ok(Self(value))
            }

            /// Borrow the underlying id string.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl<S: Into<String>> From<S> for $name {
            fn from(value: S) -> Self {
                Self::mint(value.into()).unwrap_or_else(|error| panic!("{error}"))
            }
        }
    };
}

id_type!(
    /// Identifies a [`crate::topology::Body`].
    BodyId
);
id_type!(
    /// Identifies one feature-input topology state.
    FeatureInputTopologyId
);
id_type!(
    /// Identifies one feature-result topology state.
    FeatureResultTopologyId
);
local_id_type!(
    /// Identifies a body within one feature-input topology state.
    HistoricalBodyId
);
local_id_type!(
    /// Identifies a face within one feature-input topology state.
    HistoricalFaceId
);
local_id_type!(
    /// Identifies an edge within one feature-input topology state.
    HistoricalEdgeId
);
local_id_type!(
    /// Identifies a vertex within one feature-input topology state.
    HistoricalVertexId
);
id_type!(
    /// Identifies a [`crate::topology::Region`].
    RegionId
);
id_type!(
    /// Identifies a [`crate::topology::Shell`].
    ShellId
);
id_type!(
    /// Identifies a [`crate::topology::Face`].
    FaceId
);
id_type!(
    /// Identifies a [`crate::topology::Loop`].
    LoopId
);
id_type!(
    /// Identifies a [`crate::topology::Coedge`].
    CoedgeId
);
id_type!(
    /// Identifies a [`crate::topology::Edge`].
    EdgeId
);
id_type!(
    /// Identifies a [`crate::topology::Vertex`].
    VertexId
);
id_type!(
    /// Identifies a [`crate::geometry::Surface`] carrier.
    SurfaceId
);
id_type!(
    /// Identifies a [`crate::geometry::Curve`] carrier.
    CurveId
);
id_type!(
    /// Identifies a [`crate::geometry::Pcurve`] carrier.
    PcurveId
);
id_type!(
    /// Identifies a [`crate::geometry::ProceduralSurface`] construction.
    ProceduralSurfaceId
);
id_type!(
    /// Identifies a [`crate::geometry::ProceduralCurve`] construction.
    ProceduralCurveId
);
id_type!(
    /// Identifies a [`crate::subd::SubdSurface`] carrier.
    SubdId
);
id_type!(
    /// Identifies a [`crate::topology::Point`] carrier (a vertex position).
    PointId
);
id_type!(
    /// Identifies a passthrough [`crate::unknown::UnknownRecord`].
    UnknownId
);
id_type!(
    /// Identifies a decoded [`crate::appearance::Appearance`] asset.
    AppearanceId
);
id_type!(
    /// Identifies an [`crate::appearance::AppearanceBinding`] assignment.
    AppearanceBindingId
);
id_type!(
    /// Identifies a linked [`crate::attributes::SourceAttribute`] record.
    AttributeId
);
id_type!(
    /// Identifies a canonical [`crate::products::ProductDefinition`].
    ProductDefinitionId
);
id_type!(
    /// Identifies a placed [`crate::products::Occurrence`].
    OccurrenceId
);
id_type!(
    /// Identifies a document-level [`crate::pmi::PmiAnnotation`].
    PmiId
);
id_type!(
    /// Identifies a [`crate::presentation::PresentationLayer`].
    LayerId
);

#[cfg(test)]
mod tests;
