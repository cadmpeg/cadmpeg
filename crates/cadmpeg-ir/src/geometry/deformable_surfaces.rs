// SPDX-License-Identifier: Apache-2.0
//! Deformable surface data and frames.

use super::legacy_cache::CacheContract;
use super::revision_forms::RevisionSurfaceForm;
use crate::ids::{CurveId, SurfaceId};
use crate::math::{Point3, Vector3};
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Structurally selected deformable-surface payload.
// A source states raw values; a `DeformableSurfacePayload` holds the admitted
// payload, whose scalars, vectors and points are checked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DeformableSurfaceData<R = f64, V = Vector3, P = Point3> {
    /// Mode-6 full embedded deformation payload.
    Full {
        /// Four leading deformation vectors.
        leading_vectors: [V; 4],
        /// Leading deformation scalar.
        leading_parameter: R,
        /// Three leading flags.
        leading_flags: [bool; 3],
        /// Native selector before the secondary support.
        selector: i64,
        /// Secondary embedded support surface.
        surface: SurfaceId,
        /// Native long after the support.
        native_id: i64,
        /// Native support-side flag.
        flag: bool,
        /// First scalar after the flag.
        first_parameter: R,
        /// Version-gated ASM long when present.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_version_value"
        )]
        version_value: Option<i64>,
        /// Second scalar after the optional long.
        second_parameter: R,
        /// Embedded deformation curve.
        curve: CurveId,
        /// Two ordered full vector frames.
        frames: Box<[DeformableVectorFrame<R, V>; 2]>,
        /// Native trailing long.
        trailing_value: i64,
    },
    /// Mode-5 surface-and-curve deformation payload.
    SurfaceCurve {
        /// Secondary embedded support surface.
        surface: SurfaceId,
        /// Native long identifier.
        native_id: i64,
        /// Native leading flag.
        flag: bool,
        /// First native scalar.
        first_parameter: R,
        /// Native selector integer.
        selector: i64,
        /// Second native scalar.
        second_parameter: R,
        /// Embedded deformation curve.
        curve: CurveId,
        /// Four ordered deformation vectors.
        vectors: [V; 4],
        /// Frame scalar after the vectors.
        frame_parameter: R,
        /// Three frame flags.
        flags: [bool; 3],
        /// Counted ordered scalar triples.
        parameter_triples: Vec<[R; 3]>,
    },
    /// Mode-1 deformation frame with counted parameter triples.
    Plain {
        /// Shared full deformation frame.
        frame: Box<DeformableSurfaceFrame<R, V, P>>,
        /// Ordered native scalar triples.
        parameter_triples: Vec<[R; 3]>,
    },
    /// Mode-3 deformation frame with a guide scalar.
    Guided {
        /// Shared full deformation frame.
        frame: Box<DeformableSurfaceFrame<R, V, P>>,
        /// Native guide selector.
        selector: i64,
        /// Native guide scalar.
        guide_parameter: R,
    },
    /// Mode-8 minimal four-vector scaffold.
    Minimal {
        /// Four ordered deformation vectors.
        vectors: [V; 4],
        /// Native trailing selector.
        selector: i64,
    },
    /// Revision-gated mode-3 deformation payload.
    RevisionMode3 {
        /// Four leading deformation vectors.
        leading_vectors: [V; 4],
        /// Scalar following the leading vectors.
        leading_parameter: R,
        /// Three flags following the leading scalar.
        leading_flags: [bool; 3],
        /// Position anchoring the trailing frame.
        trailing_point: P,
        /// Two vectors following the trailing point.
        trailing_vectors: [V; 2],
        /// Scalar following the trailing vectors.
        frame_parameter: R,
        /// Two flags following the trailing frame scalar.
        frame_flags: [bool; 2],
        /// Three ordered scalar parameters following the trailing frame.
        parameters: [R; 3],
        /// Five flags following the ordered scalar parameters.
        trailing_flags: [bool; 5],
        /// Scalar preceding the payload's final integer.
        trailing_parameter: R,
        /// Integer closing the revision mode-3 payload.
        trailing_value: i64,
    },
}

/// Four-vector frame used by full deformable surfaces.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DeformableVectorFrame<R = f64, V = Vector3> {
    /// Four ordered vectors.
    pub vectors: [V; 4],
    /// Frame scalar.
    pub parameter: R,
    /// Three ordered flags.
    pub flags: [bool; 3],
}

/// Shared frame payload of deformable-surface modes 1 and 3.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct DeformableSurfaceFrame<R = f64, V = Vector3, P = Point3> {
    /// Four leading deformation vectors.
    pub leading_vectors: [V; 4],
    /// Leading frame scalar.
    pub leading_parameter: R,
    /// Three leading frame flags.
    pub leading_flags: [bool; 3],
    /// Three secondary deformation vectors.
    pub secondary_vectors: [V; 3],
    /// Secondary frame scalar.
    pub secondary_parameter: R,
    /// Two secondary frame flags.
    pub secondary_flags: [bool; 2],
    /// Native model-space frame point.
    pub point: P,
    /// Five trailing frame flags.
    pub trailing_flags: [bool; 5],
}

/// Complete native deformable-surface construction.
// A source states raw values; a `DeformableSurfacePayload` holds the admitted
// construction, whose scalars, vectors and points are checked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[serde(bound(deserialize = "R: Deserialize<'de>, V: Deserialize<'de>, P: Deserialize<'de>"))]
#[cfg_attr(
    feature = "schema",
    schemars(
        bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize, P: JsonSchema + Serialize"
    )
)]
pub struct DeformableSurfaceConstruction<R = f64, V = Vector3, P = Point3> {
    /// Surface being deformed.
    pub support: SurfaceId,
    /// Discriminator-selected deformation data.
    pub data: DeformableSurfaceData<R, V, P>,
    /// Cache contract: the revision-gated fields surrounding the support and
    /// shared surface tail, or the legacy solved-cache tolerance this
    /// construction states instead.
    pub cache: CacheContract<RevisionSurfaceForm<Vec<bool>, R>>,
    /// Six ordered solved-surface discontinuity arrays.
    pub discontinuities: [Vec<R>; 6],
    /// Native discontinuity tail flag.
    pub discontinuity_flag: bool,
}

cadmpeg_core::named_optional_field!(deserialize_version_value, i64, "version_value");
