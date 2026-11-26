// SPDX-License-Identifier: Apache-2.0
//! Variable-radius blend values, cross sections, and constructions.

use super::blends::{RollingBallSide, RollingBallSupportCurve};
use super::pcurve::PcurveGeometry;
use super::revision_forms::VariableBlendCache;
use crate::ids::{CurveId, SurfaceId};
use crate::math::{Point3, Vector3};
use crate::scalar::PositiveI64;
use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Geometry role selected by a variable-blend support-side discriminator.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName, cadmpeg_derive::StructuralOrd,
)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum VariableBlendSupportKind {
    /// Support defined by a cosine curve.
    CosineCurve,
    /// Support defined by a general curve.
    Curve,
    /// Support defined by a point curve.
    PointCurve,
    /// Support defined by a surface.
    Surface,
    /// Support defined by a zero curve.
    ZeroCurve,
}

/// Convexity selected for a variable-radius blend surface.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName, cadmpeg_derive::StructuralOrd,
)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum VariableBlendConvexity {
    /// The blend bends toward the support intersection.
    Convex,
    /// The blend bends away from the support intersection.
    Concave,
}

/// Solved-surface representation selected for a variable-radius blend.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName, cadmpeg_derive::StructuralOrd,
)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum VariableBlendRenderMode {
    /// The solved surface is the rolling-ball envelope.
    RollingBallEnvelope,
    /// The solved surface is a rolling-ball snapshot.
    RollingBallSnapshot,
}

/// One interpolation control point in a variable blend-value law.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct VariableBlendInterpolationPoint<R = f64, V = Vector3, P = Point3> {
    /// Law parameter.
    pub parameter: R,
    /// Radius in document length units.
    pub radius: R,
    /// Optional first and second derivative scalars.
    pub tangents: [Option<R>; 2],
    /// Model-space control location.
    pub location: P,
    /// Control normal.
    pub normal: V,
}

/// Native edge-offset blend-value sub-discriminator.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd,
)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "i64", into = "i64")]
pub enum EdgeOffsetDiscriminator {
    /// Explicit zero sub-discriminator.
    Zero,
    /// One sub-discriminator, including the elided default.
    One,
}

impl EdgeOffsetDiscriminator {
    /// Admit the two native edge-offset codes.
    pub const fn from_code(code: i64) -> Option<Self> {
        match code {
            0 => Some(Self::Zero),
            1 => Some(Self::One),
            _ => None,
        }
    }

    /// Native integer code.
    pub const fn code(self) -> i64 {
        match self {
            Self::Zero => 0,
            Self::One => 1,
        }
    }
}

/// An edge-offset blend value carries sub-discriminator 0 or 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("edge-offset discriminator must be 0 or 1")]
pub struct EdgeOffsetDiscriminatorCode;

impl TryFrom<i64> for EdgeOffsetDiscriminator {
    type Error = EdgeOffsetDiscriminatorCode;

    fn try_from(code: i64) -> Result<Self, Self::Error> {
        Self::from_code(code).ok_or(EdgeOffsetDiscriminatorCode)
    }
}

impl From<EdgeOffsetDiscriminator> for i64 {
    fn from(discriminator: EdgeOffsetDiscriminator) -> Self {
        discriminator.code()
    }
}

/// Numeric or symbolic terminal of a functional blend-value law.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Serialize,
    cadmpeg_derive::DeserializeTagged,
    cadmpeg_derive::StructuralOrd,
)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum VariableBlendTerminal<R = f64> {
    /// Native double token.
    Double(R),
    /// Native string token.
    Text(String),
}

/// Complete recursive native `getBlendValues` payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[serde(bound(
    deserialize = "R: ::serde::de::DeserializeOwned, V: ::serde::de::DeserializeOwned, P: ::serde::de::DeserializeOwned"
))]
pub struct VariableBlendValue<R = f64, V = Vector3, P = Point3> {
    /// Native Boolean following the calibrated enum.
    pub modern_flag: bool,
    /// Native calibrated enum.
    pub calibrated: i64,
    /// Type-specific payload with its native sub-discriminator.
    pub payload: VariableBlendValuePayload<R, V, P>,
}

/// Type-specific payload of a variable blend value.
///
/// Each arm carries the native sub-discriminator it admits: the edge-offset
/// arm is one of two codes, so an edge-offset value with any other
/// sub-discriminator has no spelling.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Serialize,
    cadmpeg_derive::DeserializeTagged,
    cadmpeg_derive::StructuralOrd,
)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum VariableBlendValuePayload<R = f64, V = Vector3, P = Point3> {
    /// Law-domain parameter range and two endpoint radii.
    TwoEnds {
        /// Native sub-discriminator preceding the calibrated enum.
        discriminator: i64,
        /// Law-domain parameter range (lower, upper).
        parameters: [R; 2],
        /// Endpoint radii in document length units.
        radii: [R; 2],
    },
    /// Fixed-width branch: the parameter-range bounds and the chamfer width
    /// scalar, stored unscaled.
    FixedWidth {
        /// Native sub-discriminator preceding the calibrated enum.
        discriminator: i64,
        /// Parameter-range lower and upper bounds.
        parameters: [R; 2],
        /// Chamfer width.
        width: R,
    },
    /// Edge-offset branch.
    EdgeOffset {
        /// Native sub-discriminator preceding the calibrated enum.
        discriminator: EdgeOffsetDiscriminator,
        /// Ordered native scalar payload.
        scalars: [R; 2],
        /// Ordered length payload in document units.
        lengths: [R; 1],
    },
    /// Functional radius law carried by a BS2 pcurve.
    Functional {
        /// Native sub-discriminator preceding the calibrated enum.
        discriminator: i64,
        /// Leading scalar.
        parameter: R,
        /// Leading length in document units.
        radius: R,
        /// Scalar function whose first coordinate is radius in document units.
        function: PcurveGeometry,
        /// Numeric or symbolic terminal value.
        terminal: VariableBlendTerminal<R>,
    },
    /// Constant law followed by a recursive chamfer value.
    Constant {
        /// Native sub-discriminator preceding the calibrated enum.
        discriminator: i64,
        /// Ordered native scalars.
        parameters: [R; 2],
        /// Radius in document length units.
        radius: R,
        /// Native variable-chamfer enum.
        variable_chamfer: i64,
        /// Native chamfer-type enum.
        chamfer_type: i64,
        /// Recursively nested blend value.
        nested: Box<VariableBlendValue<R, V, P>>,
    },
    /// Interpolated radius law.
    Interpolated {
        /// Native sub-discriminator preceding the calibrated enum.
        discriminator: i64,
        /// Leading scalar.
        parameter: R,
        /// Leading radius in document length units.
        radius: R,
        /// Scalar function whose first coordinate is radius in document units.
        function: PcurveGeometry,
        /// Native extension enum, stored ahead of the radius-point count. It
        /// gates nothing; the payload ends at the last radius point.
        enum_count: i64,
        /// Whether the extension enum is stored as a `0x15` enum token
        /// (revision-gated streams) rather than a `0x04` integer.
        enum_tagged: bool,
        /// Counted radius-point array: each control carries a parameter,
        /// radius, two derivative scalars, a position, and a vector.
        points: Vec<VariableBlendInterpolationPoint<R, V, P>>,
    },
}

impl<R, V, P> VariableBlendValuePayload<R, V, P> {
    /// Native sub-discriminator preceding the calibrated enum.
    pub const fn discriminator(&self) -> i64 {
        match self {
            Self::EdgeOffset { discriminator, .. } => discriminator.code(),
            Self::TwoEnds { discriminator, .. }
            | Self::FixedWidth { discriminator, .. }
            | Self::Functional { discriminator, .. }
            | Self::Constant { discriminator, .. }
            | Self::Interpolated { discriminator, .. } => *discriminator,
        }
    }

    /// Native type name introducing this payload.
    pub const fn native_name(&self) -> &'static str {
        match self {
            Self::TwoEnds { .. } => "two_ends",
            Self::FixedWidth { .. } => "fixed_width",
            Self::EdgeOffset { .. } => "edge_offset",
            Self::Functional { .. } => "functional",
            Self::Constant { .. } => "const",
            Self::Interpolated { .. } => "interp",
        }
    }
}

/// Radius-law payloads of a variable blend.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Serialize,
    cadmpeg_derive::DeserializeTagged,
    cadmpeg_derive::StructuralOrd,
)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum VariableBlendRadii<R = f64, V = Vector3, P = Point3> {
    /// One radius law controls both support sides.
    Single {
        /// Shared radius law.
        value: VariableBlendValue<R, V, P>,
    },
    /// Each support side has an independent radius law.
    Two {
        /// First support-side radius law.
        first: VariableBlendValue<R, V, P>,
        /// Second support-side radius law.
        second: VariableBlendValue<R, V, P>,
    },
}

impl<R, V, P> VariableBlendRadii<R, V, P> {
    /// First radius law in native order.
    #[must_use]
    pub const fn first(&self) -> &VariableBlendValue<R, V, P> {
        match self {
            Self::Single { value } | Self::Two { first: value, .. } => value,
        }
    }

    /// Second radius law when both sides are controlled independently.
    #[must_use]
    pub const fn second(&self) -> Option<&VariableBlendValue<R, V, P>> {
        match self {
            Self::Single { .. } => None,
            Self::Two { second, .. } => Some(second),
        }
    }

    /// Whether one radius law controls both sides.
    #[must_use]
    pub const fn is_single(&self) -> bool {
        matches!(self, Self::Single { .. })
    }
}

/// Cross-section clause following the variable-radius laws.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Serialize,
    cadmpeg_derive::DeserializeTagged,
    cadmpeg_derive::StructuralOrd,
)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
#[serde(bound(
    deserialize = "R: ::serde::de::DeserializeOwned, V: ::serde::de::DeserializeOwned, P: ::serde::de::DeserializeOwned"
))]
#[cfg_attr(
    feature = "schema",
    schemars(
        bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize, P: JsonSchema + Serialize"
    )
)]
pub enum VariableBlendCrossSection<R = f64, V = Vector3, P = Point3> {
    /// Circular section with no additional parameters.
    Circular {},
    /// Thumbweight-controlled section with two ordered shape parameters.
    Thumbweights {
        /// Ordered native shape parameters.
        parameters: [R; 2],
    },
    /// Rounded chamfer with an optional independent rounding-radius law.
    RoundedChamfer {
        /// Rounding-radius law; absent when the clause stores `no_radius`.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_radius"
        )]
        radius: Option<Box<VariableBlendValue<R, V, P>>>,
    },
    /// Curvature-continuous round with two ordered shape parameters.
    G2Round {
        /// Ordered native shape parameters.
        parameters: [R; 2],
    },
    /// A zero-width native selector whose record framing is known but whose
    /// geometric cross-section law is not classified.
    UnclassifiedBare {
        /// Exact numeric selector retained from the native record.
        selector: VariableBlendBareCrossSection,
    },
}

/// Native zero-width variable-blend cross-section selectors whose framing is
/// established while their geometric laws remain unclassified.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, DeserializeName, cadmpeg_derive::StructuralOrd,
)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[repr(i64)]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum VariableBlendBareCrossSection {
    /// Native selector `2`.
    Selector2 = 2,
    /// Native selector `4`.
    Selector4 = 4,
    /// Native selector `5`.
    Selector5 = 5,
    /// Native selector `6`.
    Selector6 = 6,
}

impl VariableBlendBareCrossSection {
    /// Numeric selector stored in the native variable-blend record.
    pub const fn native_selector(self) -> i64 {
        match self {
            Self::Selector2 => 2,
            Self::Selector4 => 4,
            Self::Selector5 => 5,
            Self::Selector6 => 6,
        }
    }
}

impl TryFrom<i64> for VariableBlendBareCrossSection {
    type Error = ();

    fn try_from(value: i64) -> Result<Self, Self::Error> {
        match value {
            2 => Ok(Self::Selector2),
            4 => Ok(Self::Selector4),
            5 => Ok(Self::Selector5),
            6 => Ok(Self::Selector6),
            _ => Err(()),
        }
    }
}

/// Native variable-radius blend surface subtype.
#[derive(
    Debug,
    Default,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Serialize,
    DeserializeName,
    cadmpeg_derive::StructuralOrd,
)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum VariableBlendSurfaceSubtype {
    /// General variable-blend surface.
    #[default]
    VariableBlend,
    /// Surface-to-surface variable blend.
    SurfaceSurface,
    /// Curve-to-curve variable blend.
    CurveCurve,
    /// Curve-to-surface variable blend.
    CurveSurface,
    /// Free surface-curve variable blend.
    SurfaceCurveFree,
}

/// Complete native variable-radius blend construction graph.
// A source states raw values; a `VariableBlendSurfacePayload` holds the
// admitted construction, whose scalars, vectors and points are checked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[serde(bound(
    deserialize = "R: ::serde::de::DeserializeOwned, V: ::serde::de::DeserializeOwned, P: ::serde::de::DeserializeOwned"
))]
#[cfg_attr(
    feature = "schema",
    schemars(
        bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize, P: JsonSchema + Serialize"
    )
)]
pub struct VariableBlendConstruction<R = f64, V = Vector3, P = Point3> {
    /// Native surface subtype selecting the variable-blend behavior class.
    pub subtype: VariableBlendSurfaceSubtype,
    /// Positive serializer-revision integer following the subtype name.
    pub revision: PositiveI64,
    /// Two ordered support-side graphs in the rolling-ball side layout.
    pub sides: [RollingBallSide<SurfaceId, CurveId, PcurveGeometry, R, P>; 2],
    /// Stored slice curve.
    pub slice: CurveId,
    /// Optional native slice-curve parameter endpoints.
    pub slice_range: [Option<R>; 2],
    /// Two signed support offsets in document length units.
    pub offsets: [R; 2],
    /// Structurally selected radius-control payloads.
    pub radii: VariableBlendRadii<R, V, P>,
    /// Cross-section clause following the complete radius-law sequence.
    /// Absence denotes an elided default circular section; an explicit
    /// circular clause remains distinct.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_cross_section"
    )]
    pub cross_section: Option<VariableBlendCrossSection<R, V, P>>,
    /// Support-side parameter interval `(T0, T1)`; both bounds present in
    /// every instance.
    pub u_range: [R; 2],
    /// Second interval: a lower bound with an unbounded-above marker,
    /// encoded as `(T lo, F)` and decoding to `[Some(lo), None]`. The `F`
    /// upper-bound marker is an interval bound, not a standalone Boolean.
    #[serde(
        rename = "v_lower",
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_v_lower"
    )]
    pub v_lower: Option<R>,
    /// Requested fit tolerance for the surface cache.
    pub shape_parameter: R,
    /// Achieved fit tolerance for the surface cache, in document units.
    pub shape_length: R,
    /// Native integer immediately before the shared tail's enum.
    pub shape_tail: i64,
    /// Approximation-cache form selected by the shared tail enum.
    pub cache: VariableBlendCache<R>,
    /// Six ordered ASM discontinuity arrays closing the shared tail.
    pub discontinuities: [Vec<R>; 6],
    /// Native Boolean following the discontinuity arrays.
    pub tail_flag: bool,
    /// Three ASM integers following the tail Boolean.
    pub tail_extensions: [i64; 3],
    /// Secondary curve and its bounds, absent for `null_curve`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_secondary_curve"
    )]
    pub secondary_curve: Option<RollingBallSupportCurve<CurveId, R>>,
    /// Blend convexity.
    pub convexity: VariableBlendConvexity,
    /// Solved-surface representation.
    pub render_mode: VariableBlendRenderMode,
    /// Native optional post-shape interval endpoints.
    pub post_range: [Option<R>; 2],
    /// Native post-shape BS3 curve, absent for `nullbs`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_post_curve"
    )]
    pub post_curve: Option<CurveId>,
    /// Native post-shape BS2 pcurve, absent for `nullbs`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_post_pcurve"
    )]
    pub post_pcurve: Option<PcurveGeometry>,
}

cadmpeg_core::named_optional_field!(
    deserialize_radius<R, V, P>,
    Box<VariableBlendValue<R, V, P>>,
    "radius"
);
cadmpeg_core::named_optional_field!(
    deserialize_cross_section<R, V, P>,
    VariableBlendCrossSection<R, V, P>,
    "cross_section"
);
cadmpeg_core::named_optional_field!(deserialize_v_lower<R>, R, "v_lower");
cadmpeg_core::named_optional_field!(
    deserialize_secondary_curve<R>,
    RollingBallSupportCurve<CurveId, R>,
    "secondary_curve"
);
cadmpeg_core::named_optional_field!(deserialize_post_curve, CurveId, "post_curve");
cadmpeg_core::named_optional_field!(deserialize_post_pcurve, PcurveGeometry, "post_pcurve");
