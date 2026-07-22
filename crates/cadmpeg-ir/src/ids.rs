// SPDX-License-Identifier: Apache-2.0
//! Typed string identifiers for the ID-referenced IR graph.
//!
//! Each entity kind wraps a string in a distinct newtype, preventing references
//! between incompatible arenas. IDs must be stable and globally unique within
//! a document.

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
    /// Identifies a [`crate::topology::Body`].
    BodyId
);
id_type!(
    /// Identifies one feature-input topology state.
    FeatureInputTopologyId
);
id_type!(
    /// Identifies a body within one feature-input topology state.
    HistoricalBodyId
);
id_type!(
    /// Identifies a face within one feature-input topology state.
    HistoricalFaceId
);
id_type!(
    /// Identifies an edge within one feature-input topology state.
    HistoricalEdgeId
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
    /// Identifies a linked [`crate::attributes::SourceAttribute`] record.
    AttributeId
);
id_type!(
    /// Identifies a reusable [`crate::product::Product`] prototype.
    ProductId
);
id_type!(
    /// Identifies a placed [`crate::product::ProductOccurrence`].
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
