// SPDX-License-Identifier: Apache-2.0
//! Typed string identifiers for the id-referenced IR graph.
//!
//! The IR is stored as flat arenas (see [`crate::document`]); entities refer to
//! one another by id rather than by nested ownership. Each entity kind gets its
//! own newtype so the compiler rejects, say, passing a [`FaceId`] where an
//! [`EdgeId`] is expected. Every id wraps a [`String`]: decoders mint ids that
//! encode provenance (for example `f3d:smbh#42` for `RecordTable` index 42), and
//! hand-built IR can use any stable unique string.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

macro_rules! id_type {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
        )]
        #[serde(transparent)]
        pub struct $name(pub String);

        impl $name {
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
                $name(value.into())
            }
        }
    };
}

id_type!(
    /// Identifies a [`crate::topology::Body`] — the top-level solid/sheet entity.
    BodyId
);
id_type!(
    /// Identifies a [`crate::topology::Lump`] — a connected region of a body.
    LumpId
);
id_type!(
    /// Identifies a [`crate::topology::Shell`] — an oriented boundary of a lump.
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
    /// Identifies a [`crate::geometry::ProceduralSurfaceV1`] construction.
    ProceduralSurfaceId
);
id_type!(
    /// Identifies a [`crate::geometry::ProceduralCurveV1`] construction.
    ProceduralCurveId
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
    /// Identifies a linked [`crate::attributes::SourceAttribute`] record.
    AttributeId
);
