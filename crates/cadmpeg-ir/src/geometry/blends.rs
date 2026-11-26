// SPDX-License-Identifier: Apache-2.0
//! Blend supports, rolling-ball, G2, and vertex blend constructions.

use super::legacy_cache::{FitTolerance, ProceduralGeometryError};
use super::lofts::LoftBridgeToken;
use super::nurbs::NurbsCurve;
use super::pcurve::PcurveGeometry;
use super::revision_forms::{RevisionCacheForm, RevisionSurfaceParameterization};
use super::variable_blends::VariableBlendSupportKind;
use crate::features::geometry::{FinitePoint3, FiniteVector3};
use crate::ids::{CurveId, SurfaceId};
use crate::math::{Point3, Vector3};
use crate::scalar::{FiniteReal, PositiveI64};
use cadmpeg_derive::DeserializeName;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One oriented support of a procedural blend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct BlendSupport {
    /// The support surface.
    pub surface: SurfaceId,
    /// Selects the opposite surface-normal side when true.
    pub reversed: bool,
}

/// One parameter station of a rolling-ball jet, with its complete value rows.
// A source states raw values; `RollingBallJetStations` holds the admitted
// station, whose scalars, vectors and points are checked.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RollingBallJetStation<R = f64, V = Vector3, P = Point3> {
    /// Native spine parameter.
    pub knot: R,
    /// Multiplicity of this parameter in the native knot vector.
    pub multiplicity: u32,
    /// Values and derivatives at this parameter.
    pub site: RollingBallJetSite<R, V, P>,
}

pub(super) const EPS_ROLLING_BALL_RADIUS: f64 = 1.0e-9;

pub(super) const ROLLING_BALL_KNOTS: &str =
    "rolling-ball jet knots must be finite and strictly increasing";

/// Refuse a site whose radii are not finite, whose first radius is not
/// positive, or whose two radii disagree beyond the relative tolerance.
pub(super) fn admit_rolling_ball_radii(
    site: &RollingBallJetSite<FiniteReal, FiniteVector3, FinitePoint3>,
) -> Result<(), &'static str> {
    let first_radius = site.first_limit.distance(site.center.get());
    let second_radius = site.second_limit.distance(site.center.get());
    if !first_radius.is_finite()
        || first_radius <= 0.0
        || !second_radius.is_finite()
        || (first_radius - second_radius).abs()
            > EPS_ROLLING_BALL_RADIUS * first_radius.max(second_radius).max(1.0)
    {
        return Err("rolling-ball jet site radii must be finite and agree within tolerance, with a positive first radius");
    }
    Ok(())
}

/// Degree and finite clamped station data of a rolling-ball jet.
#[derive(Debug, Clone, PartialEq, Deserialize, cadmpeg_derive::StructuralOrd)]
#[serde(try_from = "RollingBallJetReadWire")]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
pub struct RollingBallJetStations {
    pub(super) degree: u32,
    #[cfg_attr(feature = "schema", schemars(with = "Vec<RollingBallJetStation>"))]
    pub(super) stations: Vec<RollingBallJetStation<FiniteReal, FiniteVector3, FinitePoint3>>,
}

/// Station invariant or context-free reconstruction failure.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RollingBallJetError {
    /// A station invariant failed.
    #[error("{0}")]
    Invalid(&'static str),
    /// The admission channel could not complete.
    #[error("{0}")]
    Admission(String),
}

impl RollingBallJetStations {
    /// Admit raw station controls.
    pub fn try_new(
        degree: u32,
        stations: Vec<RollingBallJetStation>,
    ) -> Result<Self, &'static str> {
        Self::admit_controls(degree, &stations, |row| row.knot)?;
        let mut admitted = Vec::new();
        for row in stations {
            let knot = FiniteReal::new(row.knot).ok_or(ROLLING_BALL_KNOTS)?;
            let site = row.site.admit()?;
            admit_rolling_ball_radii(&site)?;
            admitted.push(RollingBallJetStation {
                knot,
                multiplicity: row.multiplicity,
                site,
            });
        }
        Ok(Self {
            degree,
            stations: admitted,
        })
    }

    /// Validate finite station rows without copying their owned storage.
    pub fn from_parts(
        degree: u32,
        stations: Vec<RollingBallJetStation<FiniteReal, FiniteVector3, FinitePoint3>>,
    ) -> Result<Self, &'static str> {
        Self::admit_controls(degree, &stations, |row| row.knot.get())?;
        for station in &stations {
            admit_rolling_ball_radii(&station.site)?;
        }
        Ok(Self { degree, stations })
    }

    pub(super) fn admit_controls<R, V, P>(
        degree: u32,
        stations: &[RollingBallJetStation<R, V, P>],
        knot: impl Fn(&RollingBallJetStation<R, V, P>) -> f64,
    ) -> Result<(), &'static str> {
        let Some(maximum) = degree.checked_add(1).filter(|_| degree != 0) else {
            return Err(
                "rolling-ball jet degree must be positive with a representable end multiplicity",
            );
        };
        if stations.len() < 2 {
            return Err("rolling-ball jet stations must contain at least two rows");
        }
        if stations[0].multiplicity != maximum
            || stations[stations.len() - 1].multiplicity != maximum
        {
            return Err(
                "rolling-ball jet multiplicities must be in 1..=degree+1 with clamped ends",
            );
        }
        for station in stations {
            if station.multiplicity == 0 || station.multiplicity > maximum {
                return Err(
                    "rolling-ball jet multiplicities must be in 1..=degree+1 with clamped ends",
                );
            }
        }
        let mut previous = None;
        for station in stations {
            let value = knot(station);
            if !value.is_finite() || previous.is_some_and(|last| value <= last) {
                return Err(ROLLING_BALL_KNOTS);
            }
            previous = Some(value);
        }
        Ok(())
    }

    /// Return the polynomial degree of each scalar channel.
    #[must_use]
    pub const fn degree(&self) -> u32 {
        self.degree
    }

    /// Return the ordered station data.
    #[must_use]
    pub fn stations(&self) -> &[RollingBallJetStation<FiniteReal, FiniteVector3, FinitePoint3>] {
        &self.stations
    }
}

#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub(super) struct RollingBallJetReadWire {
    /// Polynomial degree of each scalar channel.
    pub(super) degree: u32,
    /// Ordered station data.
    pub(super) stations: Vec<RollingBallJetStation>,
}

impl TryFrom<RollingBallJetReadWire> for RollingBallJetStations {
    type Error = RollingBallJetError;

    fn try_from(wire: RollingBallJetReadWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.degree, wire.stations).map_err(RollingBallJetError::Invalid)
    }
}

#[derive(Serialize)]
pub(super) struct RollingBallJetWriteWire<'a> {
    pub(super) degree: u32,
    pub(super) stations: &'a [RollingBallJetStation<FiniteReal, FiniteVector3, FinitePoint3>],
}

impl Serialize for RollingBallJetStations {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        RollingBallJetWriteWire {
            degree: self.degree,
            stations: &self.stations,
        }
        .serialize(serializer)
    }
}

/// One aligned knot site of an exact rolling-ball surface jet.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RollingBallJetSite<R = f64, V = Vector3, P = Point3> {
    /// First limiting point at the knot.
    pub first_limit: P,
    /// Second limiting point at the knot.
    pub second_limit: P,
    /// Rolling-ball center at the knot.
    pub center: P,
    /// Signed opening angle at the knot, in radians.
    pub angle: R,
    /// First parameter derivative of all four value channels.
    pub first_derivative: RollingBallJetDerivative<R, V>,
    /// Second parameter derivative of all four value channels.
    pub second_derivative: RollingBallJetDerivative<R, V>,
}

/// One derivative row for the four channels of a rolling-ball jet.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RollingBallJetDerivative<R = f64, V = Vector3> {
    /// Derivative of the first limiting point.
    pub first_limit: V,
    /// Derivative of the second limiting point.
    pub second_limit: V,
    /// Derivative of the rolling-ball center.
    pub center: V,
    /// Derivative of the signed opening angle.
    pub angle: R,
}

/// Cross-section family of a procedural blend.
#[derive(
    Debug, Clone, PartialEq, Eq, Serialize, DeserializeName, cadmpeg_derive::StructuralOrd,
)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum BlendCrossSection {
    /// Constant-radius circular cross-section.
    Circular,
    /// Conic (non-circular quadric) cross-section.
    Conic,
    /// Free-form polynomial cross-section.
    Polynomial,
}

/// Common carrier fields of one G2 blend side.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct G2BlendSide<V = Vector3> {
    /// Native side label.
    pub label: String,
    /// Primary support surface.
    pub surface: SurfaceId,
    /// Primary side curve.
    pub curve: CurveId,
    /// First and second ordered BS2 pcurves; each may be `nullbs`.
    pub pcurves: [Option<PcurveGeometry>; 2],
    /// Native side direction.
    pub direction: V,
}

/// Singularity-specific payload of the first G2 blend side.
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
#[serde(bound(deserialize = "R: ::serde::de::DeserializeOwned"))]
#[cfg_attr(feature = "schema", schemars(bound = "R: JsonSchema + Serialize"))]
pub enum G2BlendFirstShape<R = f64> {
    /// Full singularity with an optional BS3 support surface.
    Full {
        /// Exact BS3 support and fit tolerance, when serialized.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_support"
        )]
        support: Option<G2BlendFullSupport>,
    },
    /// Non-singular nine-scalar frame and tertiary pcurve.
    None {
        /// Ordered native frame scalars.
        coefficients: [R; 9],
        /// Native fit tolerance.
        tolerance: FitTolerance,
        /// Optional intervening native token.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_extension"
        )]
        extension: Option<LoftBridgeToken<R>>,
        /// Tertiary BS2 pcurve, absent for `nullbs`.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_pcurve"
        )]
        pcurve: Option<PcurveGeometry>,
    },
}

/// Exact support surface and fit tolerance of a full G2 first-side shape.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct G2BlendFullSupport {
    /// Exact BS3 support surface.
    pub surface: SurfaceId,
    /// Fit tolerance of the support, in document length units.
    pub tolerance: FitTolerance,
}

/// Full native G2 blend construction graph.
// A source states raw values; a `G2BlendSurfacePayload` holds the admitted
// construction, whose scalars and vectors are checked.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize")
)]
#[serde(bound(deserialize = "R: ::serde::de::DeserializeOwned, V: ::serde::de::DeserializeOwned"))]
pub struct G2BlendConstruction<R = f64, V = Vector3> {
    /// First side common fields.
    pub first: G2BlendSide<V>,
    /// Native first-side singularity enum.
    pub singularity: i64,
    /// First-side singularity payload.
    pub first_shape: G2BlendFirstShape<R>,
    /// Second side common fields.
    pub second: G2BlendSide<V>,
    /// Exact second-side spline support.
    pub second_exact_surface: SurfaceId,
    /// Center or transition curve.
    pub center_curve: CurveId,
    /// Ordered center-curve scalars.
    pub center_parameters: [R; 2],
    /// Native center tail integer.
    pub center_flag: i64,
    /// Native U and V intervals.
    pub parameter_ranges: [[R; 2]; 2],
    /// Four ordered trailing scalars.
    pub trailing_parameters: [R; 4],
    /// Three ordered ASM discontinuity arrays.
    pub discontinuities: [Vec<R>; 3],
}

/// A present rolling-ball support surface and its native UV bounds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RollingBallSupportSurface<S = SurfaceId, R = f64> {
    /// Support surface or embedded geometry.
    pub surface: S,
    /// Optional native U and V endpoints.
    pub parameter_ranges: [[Option<R>; 2]; 2],
}

/// A present rolling-ball side curve and its native parameter bounds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RollingBallSupportCurve<C = CurveId, R = f64> {
    /// Side curve or embedded geometry.
    pub curve: C,
    /// Optional native parameter endpoints.
    pub parameter_range: [Option<R>; 2],
}

/// The optional rolling-ball extension clause.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(bound(deserialize = "P: ::serde::de::DeserializeOwned"))]
#[serde(deny_unknown_fields)]
pub struct RollingBallSideExtension<P = PcurveGeometry> {
    /// Native integer introducing the clause.
    pub value: i64,
    /// Tertiary BS2 pcurve, absent for `nullbs`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_rolling_ball_side_extension_pcurve"
    )]
    pub pcurve: Option<P>,
}

/// One complete native rolling-ball support side.
// A source states raw values; a blend construction holds the admitted side,
// whose endpoints are `FiniteReal` values and whose location is a
// `FinitePoint3`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(bound(
    serialize = "S: Serialize, C: Serialize, P: Serialize, R: Serialize, L: Serialize",
    deserialize = "S: ::serde::de::DeserializeOwned, C: ::serde::de::DeserializeOwned, P: ::serde::de::DeserializeOwned, R: ::serde::de::DeserializeOwned, L: ::serde::de::DeserializeOwned"
))]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "schema",
    schemars(
        bound = "S: JsonSchema + Serialize, C: JsonSchema + Serialize, P: JsonSchema + Serialize, R: JsonSchema + Serialize, L: JsonSchema + Serialize"
    )
)]
pub struct RollingBallSide<S = SurfaceId, C = CurveId, P = PcurveGeometry, R = f64, L = Point3> {
    /// Geometry role selected by the support-side discriminator.
    pub support_kind: VariableBlendSupportKind,
    /// Primary support surface and bounds, absent for `null_surface`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_rolling_ball_side_surface"
    )]
    pub surface: Option<RollingBallSupportSurface<S, R>>,
    /// Side curve and bounds, absent for `null_curve`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_curve"
    )]
    pub curve: Option<RollingBallSupportCurve<C, R>>,
    /// Primary BS2 pcurve, absent for `nullbs`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_rolling_ball_side_pcurve"
    )]
    pub pcurve: Option<P>,
    /// Native model-space side location.
    pub location: L,
    /// ASM secondary BS2 pcurve, absent for `nullbs`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_rolling_ball_side_secondary_pcurve"
    )]
    pub secondary_pcurve: Option<P>,
    /// Native extension integer and nullable tertiary pcurve.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_rolling_ball_side_extension"
    )]
    pub extension: Option<RollingBallSideExtension<P>>,
}

/// Third support graph appended by `sss_blend_spl_sur`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct RollingBallThirdSide<V = Vector3> {
    /// Native side label.
    pub label: String,
    /// Third support surface.
    pub surface: SurfaceId,
    /// Third side curve.
    pub curve: CurveId,
    /// Primary BS2 pcurve, absent for `nullbs`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_pcurve"
    )]
    pub pcurve: Option<PcurveGeometry>,
    /// Native side vector.
    pub direction: V,
    /// ASM secondary BS2 pcurve, absent for `nullbs`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_secondary_pcurve"
    )]
    pub secondary_pcurve: Option<PcurveGeometry>,
    /// Native ASM integer following the secondary pcurve.
    pub extension: i64,
    /// ASM tertiary BS2 pcurve, absent for `nullbs`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_tertiary_pcurve"
    )]
    pub tertiary_pcurve: Option<PcurveGeometry>,
    /// Final ASM flag.
    pub flag: bool,
}

/// Native optional-radius selector in a rolling-ball construction.
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
pub enum RollingBallRadiusSelector<T = f64> {
    /// The construction states no radius.
    None {},
    /// Explicit native selector scalar.
    Value {
        /// Stored scalar value.
        value: T,
    },
}

/// Complete byte-backed rolling-ball or three-surface blend context.
// A source states raw values; a `BlendSurfacePayload` holds the admitted
// construction, whose scalars, vectors and points are checked.
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
pub struct RollingBallConstruction<R = f64, V = Vector3, P = Point3> {
    /// Positive serializer-revision integer following the subtype name.
    pub revision: PositiveI64,
    /// Two ordered primary support sides.
    pub sides: [RollingBallSide<SurfaceId, CurveId, PcurveGeometry, R, P>; 2],
    /// Stored slice or center curve.
    pub slice: CurveId,
    /// Optional native slice-curve parameter endpoints.
    pub slice_range: [Option<R>; 2],
    /// Two signed support offsets in document length units.
    pub offsets: [R; 2],
    /// Optional-radius selector field.
    pub radius_selector: RollingBallRadiusSelector<R>,
    /// Native optional U interval endpoints.
    pub u_range: [Option<R>; 2],
    /// Native optional V interval endpoints.
    pub v_range: [Option<R>; 2],
    /// Native integer preceding the trailing scalars.
    pub shape_prefix: i64,
    /// Two ordered trailing scalars.
    pub parameters: [R; 2],
    /// Native long following the trailing scalars.
    pub tail: i64,
    /// Approximation-cache form selected by the shared tail enum.
    pub cache: RevisionCacheForm<RevisionSurfaceParameterization<R>>,
    /// Six ordered ASM discontinuity arrays closing the shared tail.
    pub discontinuities: [Vec<R>; 6],
    /// Native Boolean closing the shared tail.
    pub tail_flag: bool,
    /// Third side present only for `sss_blend_spl_sur`.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_third"
    )]
    pub third: Option<Box<RollingBallThirdSide<V>>>,
    /// Three ASM integers preceding the subtype close.
    pub tail_extensions: [i64; 3],
}

/// One boundary record in a native vertex-blend patch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "schema",
    schemars(
        bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize, P: JsonSchema + Serialize"
    )
)]
#[serde(bound(
    deserialize = "R: ::serde::de::DeserializeOwned, V: ::serde::de::DeserializeOwned, P: ::serde::de::DeserializeOwned"
))]
pub struct VertexBlendBoundary<R = f64, V = Vector3, P = Point3> {
    /// Native cross flag. The wire form is a logical, so the value is the
    /// tag itself and no payload follows.
    pub boundary_type: bool,
    /// Native magic direction with finite components, stored as read. It is a
    /// direction, never a length, so it carries no unit scale.
    pub magic: V,
    /// Native U-smoothing flag, a logical on the wire.
    pub u_smoothing: bool,
    /// Native V-smoothing flag, a logical on the wire.
    pub v_smoothing: bool,
    /// Native fullness scalar.
    pub fullness: R,
    /// Structurally selected boundary geometry.
    pub geometry: VertexBlendBoundaryGeometry<R, V, P>,
}

/// Twist payload selected by a vertex-blend circle form.
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
pub enum VertexBlendTwists<P = Point3> {
    /// Native form zero: no twist entries.
    None {},
    /// Native form one: one twist entry.
    One {
        /// The one twist entry.
        twist: P,
    },
    /// Native form three: two ordered twist entries.
    Two {
        /// The two ordered twist entries.
        twists: [P; 2],
    },
}

impl<P> VertexBlendTwists<P> {
    /// Native form selected by the twist payload.
    #[must_use]
    pub const fn form(&self) -> i64 {
        match self {
            Self::None {} => 0,
            Self::One { .. } => 1,
            Self::Two { .. } => 3,
        }
    }

    /// Ordered twist entries. Their coordinate semantics depend on the layout revision.
    #[must_use]
    pub fn entries(&self) -> &[P] {
        match self {
            Self::None {} => &[],
            Self::One { twist } => std::slice::from_ref(twist),
            Self::Two { twists } => twists,
        }
    }
}

/// Type-specific geometry of a vertex-blend boundary.
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
pub enum VertexBlendBoundaryGeometry<R = f64, V = Vector3, P = Point3> {
    /// Curve boundary with a circle/ellipse/unknown twist form.
    Circle {
        /// Boundary curve.
        curve: CurveId,
        /// Optional native curve parameter endpoints stored by the
        /// revision-gated layout.
        curve_endpoints: [Option<R>; 2],
        /// Twist payload. Pre-revision layouts store model-space locations;
        /// the revision-gated layout stores unscaled twist vectors.
        twists: VertexBlendTwists<P>,
        /// Two ordered curve parameters.
        parameters: [R; 2],
        /// Native sense flag, a logical on the wire.
        sense: bool,
    },
    /// Degenerate boundary at a model-space location.
    Degenerate {
        /// Degenerate location.
        location: P,
        /// Two ordered boundary normals.
        normals: [V; 2],
    },
    /// Surface pcurve boundary.
    Pcurve {
        /// Support surface.
        surface: SurfaceId,
        /// Optional U/V bound fields stored after the support by the
        /// revision-gated layout.
        support_bounds: [Option<R>; 4],
        /// Native BS2 pcurve, absent for `nullbs`.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_pcurve"
        )]
        pcurve: Option<PcurveGeometry>,
        /// Native sense flag, a logical on the wire.
        sense: bool,
        /// Parameter-space fit tolerance.
        fit_tolerance: FitTolerance,
    },
    /// Planar boundary described by a normal and curve.
    Plane {
        /// Plane normal.
        normal: V,
        /// Two ordered plane parameters.
        parameters: [R; 2],
        /// Boundary curve.
        curve: CurveId,
        /// Optional native curve parameter endpoints stored by the
        /// revision-gated layout.
        curve_endpoints: [Option<R>; 2],
    },
}

/// Complete native vertex-blend surface construction.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "schema",
    schemars(
        bound = "R: JsonSchema + Serialize, V: JsonSchema + Serialize, P: JsonSchema + Serialize"
    )
)]
#[serde(bound(
    deserialize = "R: ::serde::de::DeserializeOwned, V: ::serde::de::DeserializeOwned, P: ::serde::de::DeserializeOwned"
))]
pub struct VertexBlendConstruction<R = f64, V = Vector3, P = Point3> {
    /// Positive serializer-revision integer selecting the revision-gated
    /// layout; absent from the pre-revision layout.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_revision"
    )]
    pub revision: Option<PositiveI64>,
    /// Ordered boundary records.
    pub boundaries: Vec<VertexBlendBoundary<R, V, P>>,
    /// Native grid-size integer.
    pub grid_size: i64,
    /// Native model-space fit tolerance.
    pub fit_tolerance: FitTolerance,
}

/// Radius law for a procedural blend.
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
pub enum BlendRadiusLaw {
    /// Constant blend radius along the whole spine.
    Constant {
        /// Signed radius, in document length units; sign selects the support offset side.
        signed_radius: crate::scalar::Length,
    },
    /// Radius varying linearly from `start` to `end` along the spine.
    Linear {
        /// Signed radius at the spine start, in document length units.
        start: crate::scalar::Length,
        /// Signed radius at the spine end, in document length units.
        end: crate::scalar::Length,
    },
    /// Radius varying along the spine per an explicit law curve.
    Law {
        /// Curve whose parameterization gives the signed radius along the spine.
        curve: NurbsCurve,
    },
}

/// The refusal of a blend radius law with a non-finite radius.
pub(super) const NON_FINITE_BLEND_RADIUS: ProceduralGeometryError =
    ProceduralGeometryError::Payload("blend radius law is not finite");

impl BlendRadiusLaw {
    /// Admit a constant law from a finite signed radius. The sign of a radius
    /// selects the support offset side, so a negative radius is admitted.
    pub fn constant(signed_radius: f64) -> Result<Self, ProceduralGeometryError> {
        Ok(Self::Constant {
            signed_radius: crate::scalar::Length::new(signed_radius)
                .ok_or(NON_FINITE_BLEND_RADIUS)?,
        })
    }

    /// Admit a linear law from finite signed radii at the spine start and
    /// end, with the refusal of [`BlendRadiusLaw::constant`].
    pub fn linear(start: f64, end: f64) -> Result<Self, ProceduralGeometryError> {
        Ok(Self::Linear {
            start: crate::scalar::Length::new(start).ok_or(NON_FINITE_BLEND_RADIUS)?,
            end: crate::scalar::Length::new(end).ok_or(NON_FINITE_BLEND_RADIUS)?,
        })
    }
}

cadmpeg_core::named_optional_field!(deserialize_pcurve, PcurveGeometry, "pcurve");

cadmpeg_core::named_optional_field!(
    deserialize_secondary_pcurve,
    PcurveGeometry,
    "secondary_pcurve"
);

cadmpeg_core::named_optional_field!(deserialize_support, G2BlendFullSupport, "support");
cadmpeg_core::named_optional_field!(deserialize_extension<R>, LoftBridgeToken<R>, "extension");
cadmpeg_core::named_optional_field!(
    deserialize_rolling_ball_side_surface<S, R>,
    RollingBallSupportSurface<S, R>,
    "surface"
);
cadmpeg_core::named_optional_field!(
    deserialize_curve<C, R>,
    RollingBallSupportCurve<C, R>,
    "curve"
);
cadmpeg_core::named_optional_field!(deserialize_rolling_ball_side_pcurve<P>, P, "pcurve");
cadmpeg_core::named_optional_field!(
    deserialize_rolling_ball_side_secondary_pcurve<P>,
    P,
    "secondary_pcurve"
);
cadmpeg_core::named_optional_field!(
    deserialize_rolling_ball_side_extension<P>,
    RollingBallSideExtension<P>,
    "extension"
);
cadmpeg_core::named_optional_field!(
    deserialize_tertiary_pcurve,
    PcurveGeometry,
    "tertiary_pcurve"
);
cadmpeg_core::named_optional_field!(deserialize_third<V>, Box<RollingBallThirdSide<V>>, "third");

cadmpeg_core::named_optional_field!(deserialize_revision, PositiveI64, "revision");

cadmpeg_core::named_optional_field!(
    deserialize_rolling_ball_side_extension_pcurve<P>,
    P,
    "pcurve"
);
