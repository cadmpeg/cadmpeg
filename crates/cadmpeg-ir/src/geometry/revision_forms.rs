// SPDX-License-Identifier: Apache-2.0
//! Revision surface forms, caches, and revision blend and loft constructions.

use super::blends::{RollingBallRadiusSelector, RollingBallSide};
use super::legacy_cache::{
    CacheContractError, FitTolerance, ProceduralGeometryError, RevisionCacheWrite, ToleranceWrite,
};
use super::lofts::{CompoundLoftDirection, LoftPath, LoftProfileMember, LoftSectionEntry};
use super::pcurve::PcurveGeometry;
use crate::features::geometry::{FinitePoint3, FiniteVector3};
use crate::ids::{CurveId, SurfaceId};
use crate::scalar::{FiniteReal, PositiveI64};
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::num::NonZeroI64;

/// Shared fields of a revision-gated spline-surface form: the revision
/// integer, optional support bounds and reference-curve endpoints, a
/// carrier-specific boolean run, and the shared tail enum, discontinuity
/// arrays, tail boolean, and post-tail boolean run.
// A source states raw scalars; a surface construction holds the admitted
// form, whose scalars are `FiniteReal` values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[serde(bound(deserialize = "F: Deserialize<'de> + Default, R: Deserialize<'de>"))]
#[cfg_attr(
    feature = "schema",
    schemars(bound = "F: JsonSchema + Default, R: JsonSchema + Serialize")
)]
pub struct RevisionSurfaceForm<F: Default = Vec<bool>, R = f64> {
    /// Positive serializer-revision integer following the subtype name.
    pub revision: PositiveI64,
    /// Optional U/V bound fields following the support surface.
    pub support_bounds: [Option<R>; 4],
    /// Optional parameter endpoints following the embedded reference curve.
    pub reference_endpoints: [Option<R>; 2],
    /// Optional parameter endpoints following a second embedded curve, used
    /// by two-curve carriers such as `sum_spl_sur`.
    pub second_endpoints: [Option<R>; 2],
    /// Carrier-specific boolean run preceding the shared tail.
    pub flags: F,
    /// Approximation-cache form selected by the shared tail enum.
    pub cache: RevisionCacheForm<RevisionSurfaceParameterization<R>>,
    /// Six ordered discontinuity arrays following the fit tolerance.
    pub discontinuities: [Vec<R>; 6],
    /// Boolean terminating the shared tail.
    pub tail_flag: bool,
    /// Boolean run following the shared tail.
    pub trailing_flags: Vec<bool>,
}

/// Mutually exclusive payloads of a revision-gated approximation cache.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RevisionCacheForm<P = RevisionSurfaceParameterization> {
    /// A solved cache followed by its carrier-specific cache contract.
    SolvedCache {
        /// Carrier-specific solved-cache contract.
        fit_tolerance: FitTolerance,
    },
    /// Parameterization stored in place of a solved cache.
    Parameterization(P),
}

impl<P> RevisionCacheForm<P> {
    /// State the fit tolerance this form carries for its solved cache.
    pub(super) fn write_fit_tolerance(&mut self, write: ToleranceWrite) -> RevisionCacheWrite {
        match self {
            Self::SolvedCache { fit_tolerance } => {
                match write {
                    ToleranceWrite::State(value) => *fit_tolerance = value,
                    ToleranceWrite::Raise(value) => {
                        if value.get() > fit_tolerance.get() {
                            *fit_tolerance = value;
                        }
                    }
                    ToleranceWrite::Scale(scale) => {
                        let Some(scaled) = fit_tolerance.scaled(scale) else {
                            return RevisionCacheWrite::InvalidValue(
                                fit_tolerance.get() * scale.get(),
                            );
                        };
                        *fit_tolerance = scaled;
                    }
                }
                RevisionCacheWrite::Written
            }
            Self::Parameterization(_) => RevisionCacheWrite::Parameterized,
        }
    }

    /// Parameterization carried in place of a solved cache.
    #[must_use]
    pub const fn parameterization(&self) -> Option<&P> {
        match self {
            Self::SolvedCache { .. } => None,
            Self::Parameterization(parameterization) => Some(parameterization),
        }
    }

    /// Fit tolerance carried by a solved cache.
    #[must_use]
    pub const fn fit_tolerance(&self) -> Option<FitTolerance> {
        match self {
            Self::SolvedCache { fit_tolerance } => Some(*fit_tolerance),
            Self::Parameterization(_) => None,
        }
    }
}

/// Approximation state and its dependent fit contract for a variable blend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[cfg_attr(feature = "schema", schemars(bound = "R: JsonSchema + Serialize"))]
pub enum VariableBlendCache<R = f64> {
    /// A nonzero approximation-current flag with an active fit contract.
    Current {
        /// Native approximation-current flag.
        shape_prefix: NonZeroI64,
        /// Fit tolerance in document length units.
        fit_tolerance: FitTolerance,
    },
    /// A zero approximation-current flag without an active fit contract.
    Stale {},
    /// Parameterization in place of a solved cache.
    Parameterization {
        /// Native approximation-current flag, independent of the parameterization.
        shape_prefix: i64,
        /// Surface parameterization.
        parameterization: RevisionSurfaceParameterization<R>,
    },
}

impl<R> VariableBlendCache<R> {
    /// Scale the tolerance only when this cache carries a solved one.
    pub(super) fn scale_fit_tolerance(
        &mut self,
        scale: crate::scalar::PositiveReal,
    ) -> Result<(), CacheContractError> {
        if let Self::Current { fit_tolerance, .. } = self {
            let value = *fit_tolerance;
            *fit_tolerance =
                value
                    .scaled(scale)
                    .ok_or_else(|| CacheContractError::InvalidValue {
                        value: value.get() * scale.get(),
                    })?;
        }
        Ok(())
    }

    /// Native approximation-current flag.
    #[must_use]
    pub const fn shape_prefix(&self) -> i64 {
        match self {
            Self::Current { shape_prefix, .. } => shape_prefix.get(),
            Self::Stale {} => 0,
            Self::Parameterization { shape_prefix, .. } => *shape_prefix,
        }
    }

    /// Parameterization carried in place of a solved cache.
    #[must_use]
    pub const fn parameterization(&self) -> Option<&RevisionSurfaceParameterization<R>> {
        match self {
            Self::Parameterization {
                parameterization, ..
            } => Some(parameterization),
            _ => None,
        }
    }

    /// Active fit tolerance, absent for a stale or parameterized approximation.
    #[must_use]
    pub const fn fit_tolerance(&self) -> Option<FitTolerance> {
        match self {
            Self::Current { fit_tolerance, .. } => Some(*fit_tolerance),
            _ => None,
        }
    }
}

/// Parameterization carried by tail-enum form `2` of the shared revision-gated
/// spline-surface tail. This form stores no approximation cache and no fit
/// tolerance; it stores the two parameter intervals followed by four enums, in
/// the order the fields appear below.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
#[serde(bound(deserialize = "R: Deserialize<'de>"))]
#[cfg_attr(feature = "schema", schemars(bound = "R: JsonSchema + Serialize"))]
pub struct RevisionSurfaceParameterization<R = f64> {
    /// U parameter interval, an ordered `[lo, hi]` pair of optional bounds.
    /// `None` is a false bound-presence flag.
    pub u_interval: [Option<R>; 2],
    /// V parameter interval, an ordered `[lo, hi]` pair of optional bounds.
    /// `None` is a false bound-presence flag.
    pub v_interval: [Option<R>; 2],
    /// U closure enum.
    pub u_closure: i64,
    /// V closure enum.
    pub v_closure: i64,
    /// U singularity enum.
    pub u_singularity: i64,
    /// V singularity enum.
    pub v_singularity: i64,
}

impl<R> Default for RevisionSurfaceParameterization<R> {
    /// Absent interval bounds and zero enums.
    fn default() -> Self {
        Self {
            u_interval: [None, None],
            v_interval: [None, None],
            u_closure: 0,
            v_closure: 0,
            u_singularity: 0,
            v_singularity: 0,
        }
    }
}

/// Complete native revision-gated `g2_blend_spl_sur` construction. The
/// revision layout stores the two support sides in the variable-blend side
/// layout and ends with the shared revision-gated surface tail.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "RevisionG2BlendConstructionWire")]
pub struct RevisionG2BlendConstruction {
    pub(super) revision: PositiveI64,
    pub(super) leading_parameters: [FiniteReal; 2],
    #[cfg_attr(feature = "schema", schemars(with = "Box<[RollingBallSide; 2]>"))]
    pub(super) sides:
        [RollingBallSide<SurfaceId, CurveId, PcurveGeometry, FiniteReal, FinitePoint3>; 2],
    pub(super) center: CurveId,
    #[serde(default)]
    pub(super) center_range: [Option<FiniteReal>; 2],
    pub(super) radii: [FiniteReal; 2],
    pub(super) radius_selector: RollingBallRadiusSelector<PositiveI64>,
    pub(super) u_range: [Option<FiniteReal>; 2],
    pub(super) v_range: [Option<FiniteReal>; 2],
    pub(super) shape_prefix: i64,
    pub(super) shape_parameter: FiniteReal,
    pub(super) shape_length: FiniteReal,
    pub(super) shape_tail: i64,
    #[cfg_attr(feature = "schema", schemars(with = "RevisionCacheForm"))]
    pub(super) cache: RevisionCacheForm<RevisionSurfaceParameterization<FiniteReal>>,
    #[serde(default)]
    pub(super) discontinuities: [Vec<FiniteReal>; 6],
    pub(super) tail_flag: bool,
    pub(super) tail_extensions: [i64; 3],
}

/// Stored fields of a revision-gated `g2_blend_spl_sur` construction before
/// admission. This is the wire shape the deserializer reads and the only
/// input `RevisionG2BlendConstruction::admit` accepts.
#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(feature = "schema", schemars(rename = "RevisionG2BlendConstruction"))]
#[serde(deny_unknown_fields)]
pub struct RevisionG2BlendConstructionWire {
    /// Positive serializer-revision integer following the subtype name.
    pub revision: PositiveI64,
    /// Two native scalars following the revision integer.
    pub leading_parameters: [f64; 2],
    /// Two ordered support-side graphs in the variable-blend side layout.
    pub sides: Box<[RollingBallSide; 2]>,
    /// Stored center curve.
    pub center: CurveId,
    /// Optional native center-curve parameter endpoints.
    pub center_range: [Option<f64>; 2],
    /// Two signed blend radii in document length units.
    pub radii: [f64; 2],
    /// Integer-valued optional-radius selector following the radii.
    pub radius_selector: RollingBallRadiusSelector<PositiveI64>,
    /// Native optional U interval endpoints.
    pub u_range: [Option<f64>; 2],
    /// Native optional V interval endpoints.
    pub v_range: [Option<f64>; 2],
    /// Native integer before the solved shape.
    pub shape_prefix: i64,
    /// Native scalar before the solved shape.
    pub shape_parameter: f64,
    /// Native length before the solved shape, in document units.
    pub shape_length: f64,
    /// Native integer immediately before the shared tail.
    pub shape_tail: i64,
    /// Approximation-cache form selected by the shared tail enum.
    pub cache: RevisionCacheForm,
    /// Six ordered discontinuity arrays following the fit tolerance.
    pub discontinuities: [Vec<f64>; 6],
    /// Boolean terminating the shared tail.
    pub tail_flag: bool,
    /// Three ASM integers following the shared tail.
    pub tail_extensions: [i64; 3],
}

impl RevisionG2BlendConstruction {
    /// Admit stored fields whose scalars are all finite. This is the
    /// admission of the `revision_g2_blend` procedural surface: a decoder
    /// that reads the fields admits them here, and the deserializer runs the
    /// same walk. The fields are private, so this and the deserializer are
    /// the only routes to a value.
    pub fn admit(wire: RevisionG2BlendConstructionWire) -> Result<Self, ProceduralGeometryError> {
        let invalid = || {
            ProceduralGeometryError::Payload("revision g2 blend construction payload is invalid")
        };
        let [first, second] = *wire.sides;
        let (
            Some(leading_parameters),
            Some(center_range),
            Some(radii),
            Some(u_range),
            Some(v_range),
            Some([shape_parameter, shape_length]),
            Some(discontinuities),
            Some(first),
            Some(second),
            Some(cache),
        ) = (
            FiniteReal::array(wire.leading_parameters),
            FiniteReal::optional(wire.center_range),
            FiniteReal::array(wire.radii),
            FiniteReal::optional(wire.u_range),
            FiniteReal::optional(wire.v_range),
            FiniteReal::array([wire.shape_parameter, wire.shape_length]),
            FiniteReal::lanes(wire.discontinuities),
            first.admit(),
            second.admit(),
            wire.cache.admit(),
        )
        else {
            return Err(invalid());
        };
        Ok(Self {
            revision: wire.revision,
            leading_parameters,
            sides: [first, second],
            center: wire.center,
            center_range,
            radii,
            radius_selector: wire.radius_selector,
            u_range,
            v_range,
            shape_prefix: wire.shape_prefix,
            shape_parameter,
            shape_length,
            shape_tail: wire.shape_tail,
            cache,
            discontinuities,
            tail_flag: wire.tail_flag,
            tail_extensions: wire.tail_extensions,
        })
    }

    /// Return the positive serializer-revision integer.
    #[must_use]
    pub const fn revision(&self) -> PositiveI64 {
        self.revision
    }

    /// Return the two native scalars following the revision integer.
    #[must_use]
    pub const fn leading_parameters(&self) -> [FiniteReal; 2] {
        self.leading_parameters
    }

    /// Return the two ordered support-side graphs.
    #[must_use]
    pub const fn sides(
        &self,
    ) -> &[RollingBallSide<SurfaceId, CurveId, PcurveGeometry, FiniteReal, FinitePoint3>; 2] {
        &self.sides
    }

    /// Return the stored center curve.
    #[must_use]
    pub const fn center(&self) -> &CurveId {
        &self.center
    }

    /// Return the optional center-curve parameter endpoints.
    #[must_use]
    pub const fn center_range(&self) -> [Option<FiniteReal>; 2] {
        self.center_range
    }

    /// Return the two signed blend radii.
    #[must_use]
    pub const fn radii(&self) -> [FiniteReal; 2] {
        self.radii
    }

    /// Return the optional-radius selector.
    #[must_use]
    pub const fn radius_selector(&self) -> &RollingBallRadiusSelector<PositiveI64> {
        &self.radius_selector
    }

    /// Return the optional U interval endpoints.
    #[must_use]
    pub const fn u_range(&self) -> [Option<FiniteReal>; 2] {
        self.u_range
    }

    /// Return the optional V interval endpoints.
    #[must_use]
    pub const fn v_range(&self) -> [Option<FiniteReal>; 2] {
        self.v_range
    }

    /// Return the integer before the solved shape.
    #[must_use]
    pub const fn shape_prefix(&self) -> i64 {
        self.shape_prefix
    }

    /// Return the scalar before the solved shape.
    #[must_use]
    pub const fn shape_parameter(&self) -> FiniteReal {
        self.shape_parameter
    }

    /// Return the length before the solved shape.
    #[must_use]
    pub const fn shape_length(&self) -> FiniteReal {
        self.shape_length
    }

    /// Return the integer immediately before the shared tail.
    #[must_use]
    pub const fn shape_tail(&self) -> i64 {
        self.shape_tail
    }

    /// Return the approximation-cache form.
    #[must_use]
    pub const fn cache(&self) -> &RevisionCacheForm<RevisionSurfaceParameterization<FiniteReal>> {
        &self.cache
    }

    /// Return the six ordered discontinuity arrays.
    #[must_use]
    pub const fn discontinuities(&self) -> &[Vec<FiniteReal>; 6] {
        &self.discontinuities
    }

    /// Return the Boolean terminating the shared tail.
    #[must_use]
    pub const fn tail_flag(&self) -> bool {
        self.tail_flag
    }

    /// Return the three ASM integers following the shared tail.
    #[must_use]
    pub const fn tail_extensions(&self) -> [i64; 3] {
        self.tail_extensions
    }
}

impl TryFrom<RevisionG2BlendConstructionWire> for RevisionG2BlendConstruction {
    type Error = ProceduralGeometryError;

    fn try_from(wire: RevisionG2BlendConstructionWire) -> Result<Self, Self::Error> {
        Self::admit(wire)
    }
}

/// Complete native revision-gated `cl_loft_spl_sur` construction. The
/// revision layout is cache-first: the revision integer and shared
/// revision-gated surface tail precede the construction fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "RevisionCompoundLoftConstructionWire")]
pub struct RevisionCompoundLoftConstruction {
    pub(super) revision: PositiveI64,
    #[cfg_attr(feature = "schema", schemars(with = "RevisionCacheForm"))]
    pub(super) cache: RevisionCacheForm<RevisionSurfaceParameterization<FiniteReal>>,
    #[serde(default)]
    pub(super) discontinuities: [Vec<FiniteReal>; 6],
    pub(super) tail_flag: bool,
    #[cfg_attr(feature = "schema", schemars(with = "Vec<LoftProfileMember>"))]
    pub(super) base_profile: Vec<LoftProfileMember<FiniteReal, FiniteVector3>>,
    #[cfg_attr(feature = "schema", schemars(with = "LoftPath"))]
    pub(super) base_path: LoftPath<FiniteReal>,
    #[cfg_attr(feature = "schema", schemars(with = "Vec<LoftSectionEntry>"))]
    pub(super) entries: Vec<LoftSectionEntry<FiniteReal, FiniteVector3>>,
    pub(super) flags: [bool; 2],
    pub(super) kind_flags: [bool; 2],
    #[cfg_attr(feature = "schema", schemars(with = "CompoundLoftDirection"))]
    pub(super) direction: CompoundLoftDirection<FiniteVector3>,
    pub(super) tail: RevisionCompoundLoftTail<CurveId, FiniteReal>,
}

/// Stored fields of a revision-gated `cl_loft_spl_sur` construction before
/// admission. This is the wire shape the deserializer reads and the only
/// input `RevisionCompoundLoftConstruction::admit` accepts.
#[derive(Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(rename = "RevisionCompoundLoftConstruction")
)]
#[serde(deny_unknown_fields)]
pub struct RevisionCompoundLoftConstructionWire {
    /// Positive serializer-revision integer following the subtype name.
    pub revision: PositiveI64,
    /// Approximation-cache form selected by the shared tail enum.
    pub cache: RevisionCacheForm,
    /// Six ordered discontinuity arrays following the fit tolerance.
    pub discontinuities: [Vec<f64>; 6],
    /// Boolean terminating the shared tail.
    pub tail_flag: bool,
    /// Leading unparameterized scale block: ordered profile members and path.
    pub base_profile: Vec<LoftProfileMember>,
    /// Path data of the leading scale block.
    pub base_path: LoftPath,
    /// Counted parameterized entries; the native parameter trails each
    /// entry's fields.
    pub entries: Vec<LoftSectionEntry>,
    /// Two flags following the entries.
    pub flags: [bool; 2],
    /// Two flags opening the kind-zero payload.
    pub kind_flags: [bool; 2],
    /// Direction carrier selected by the kind-zero direction tag.
    pub direction: CompoundLoftDirection,
    /// Trailing bounds and their dependent BS3 curve.
    pub tail: RevisionCompoundLoftTail<CurveId>,
}

impl RevisionCompoundLoftConstruction {
    /// Admit stored fields whose scalars are all finite. This is the
    /// admission of the `revision_compound_loft` procedural surface: a
    /// decoder that reads the fields admits them here, and the deserializer
    /// runs the same walk. The fields are private, so this and the
    /// deserializer are the only routes to a value.
    pub fn admit(
        wire: RevisionCompoundLoftConstructionWire,
    ) -> Result<Self, ProceduralGeometryError> {
        let invalid = || {
            ProceduralGeometryError::Payload(
                "revision compound loft construction payload is invalid",
            )
        };
        let (
            Some(discontinuities),
            Some(tail),
            Some(cache),
            Some(base_profile),
            Some(base_path),
            Some(entries),
            Some(direction),
        ) = (
            FiniteReal::lanes(wire.discontinuities),
            wire.tail.admit(),
            wire.cache.admit(),
            wire.base_profile
                .into_iter()
                .map(LoftProfileMember::admit)
                .collect::<Option<Vec<_>>>(),
            wire.base_path.admit(),
            wire.entries
                .into_iter()
                .map(LoftSectionEntry::admit)
                .collect::<Option<Vec<_>>>(),
            wire.direction.admit(),
        )
        else {
            return Err(invalid());
        };
        Ok(Self {
            revision: wire.revision,
            cache,
            discontinuities,
            tail_flag: wire.tail_flag,
            base_profile,
            base_path,
            entries,
            flags: wire.flags,
            kind_flags: wire.kind_flags,
            direction,
            tail,
        })
    }

    /// Return the positive serializer-revision integer.
    #[must_use]
    pub const fn revision(&self) -> PositiveI64 {
        self.revision
    }

    /// Return the approximation-cache form.
    #[must_use]
    pub const fn cache(&self) -> &RevisionCacheForm<RevisionSurfaceParameterization<FiniteReal>> {
        &self.cache
    }

    /// Return the six ordered discontinuity arrays.
    #[must_use]
    pub const fn discontinuities(&self) -> &[Vec<FiniteReal>; 6] {
        &self.discontinuities
    }

    /// Return the Boolean terminating the shared tail.
    #[must_use]
    pub const fn tail_flag(&self) -> bool {
        self.tail_flag
    }

    /// Return the profile members of the leading scale block.
    #[must_use]
    pub fn base_profile(&self) -> &[LoftProfileMember<FiniteReal, FiniteVector3>] {
        &self.base_profile
    }

    /// Return the path data of the leading scale block.
    #[must_use]
    pub const fn base_path(&self) -> &LoftPath<FiniteReal> {
        &self.base_path
    }

    /// Return the counted parameterized entries.
    #[must_use]
    pub fn entries(&self) -> &[LoftSectionEntry<FiniteReal, FiniteVector3>] {
        &self.entries
    }

    /// Return the two flags following the entries.
    #[must_use]
    pub const fn flags(&self) -> [bool; 2] {
        self.flags
    }

    /// Return the two flags opening the kind-zero payload.
    #[must_use]
    pub const fn kind_flags(&self) -> [bool; 2] {
        self.kind_flags
    }

    /// Return the direction carrier of the kind-zero payload.
    #[must_use]
    pub const fn direction(&self) -> &CompoundLoftDirection<FiniteVector3> {
        &self.direction
    }

    /// Return the trailing bounds and their dependent BS3 curve.
    #[must_use]
    pub const fn tail(&self) -> &RevisionCompoundLoftTail<CurveId, FiniteReal> {
        &self.tail
    }
}

impl TryFrom<RevisionCompoundLoftConstructionWire> for RevisionCompoundLoftConstruction {
    type Error = ProceduralGeometryError;

    fn try_from(wire: RevisionCompoundLoftConstructionWire) -> Result<Self, Self::Error> {
        Self::admit(wire)
    }
}

/// Trailing parameter bounds of a revision compound loft.
/// Both bounds select a trailing curve; all other forms contain no curve.
// A source states raw bounds; a `RevisionCompoundLoftConstruction` holds the
// admitted tail, whose bounds are `FiniteReal` values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    deny_unknown_fields,
    bound(
        serialize = "T: Serialize, S: Serialize",
        deserialize = "T: Deserialize<'de>, S: Deserialize<'de>"
    )
)]
pub enum RevisionCompoundLoftTail<T, S = f64> {
    /// Neither parameter bound is present.
    Unbounded {},
    /// Only the lower parameter bound is present.
    LowerBound {
        /// Lower parameter bound.
        lower: S,
    },
    /// Only the upper parameter bound is present.
    UpperBound {
        /// Upper parameter bound.
        upper: S,
    },
    /// Both parameter bounds and their selected curve.
    Curve {
        /// Ordered lower and upper parameter bounds.
        interval: [S; 2],
        /// Curve selected by the complete parameter pair.
        curve: T,
    },
}

impl<T> RevisionCompoundLoftTail<T> {
    /// The tail with admitted bounds, absent when a stored bound is not
    /// finite.
    pub(super) fn admit(self) -> Option<RevisionCompoundLoftTail<T, FiniteReal>> {
        Some(match self {
            Self::Unbounded {} => RevisionCompoundLoftTail::Unbounded {},
            Self::LowerBound { lower } => RevisionCompoundLoftTail::LowerBound {
                lower: FiniteReal::new(lower)?,
            },
            Self::UpperBound { upper } => RevisionCompoundLoftTail::UpperBound {
                upper: FiniteReal::new(upper)?,
            },
            Self::Curve { interval, curve } => RevisionCompoundLoftTail::Curve {
                interval: FiniteReal::array(interval)?,
                curve,
            },
        })
    }
}

impl<T, S: Copy> RevisionCompoundLoftTail<T, S> {
    /// Optional bounds in native order.
    #[must_use]
    pub const fn interval(&self) -> [Option<S>; 2] {
        match self {
            Self::Unbounded {} => [None, None],
            Self::LowerBound { lower } => [Some(*lower), None],
            Self::UpperBound { upper } => [None, Some(*upper)],
            Self::Curve { interval, .. } => [Some(interval[0]), Some(interval[1])],
        }
    }

    /// Curve selected by a complete parameter pair.
    #[must_use]
    pub const fn curve(&self) -> Option<&T> {
        match self {
            Self::Curve { curve, .. } => Some(curve),
            _ => None,
        }
    }
}
