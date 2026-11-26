// SPDX-License-Identifier: Apache-2.0
//! Procedural curve carriers, compound curves, and curve definitions.

use super::helix::HelixCurveConstruction;
use super::intersection_curves::{
    IntcurveSupportContext, IntcurveSupportSide, LawCurveVersionForm,
    TolerantIntersectionConstruction, TolerantIntersectionParameterization,
};
use super::laws::FiniteLawFormula;
#[cfg(feature = "schema")]
use super::laws::LawFormula;
use super::legacy_cache::{
    clear_revision_form_tolerance, CacheContractError, FitTolerance, LegacyCache,
    RevisionCacheWrite, ToleranceWrite, NO_LEGACY_SLOT, PARAMETERIZED_NO_SOLVED_CACHE,
};
use super::pcurve::PcurveGeometry;
use super::revision_forms::RevisionCacheForm;
use super::surface_curves::{CacheFirstCurveParameterization, SurfaceCurveFamily};
use crate::ids::{CurveId, SurfaceId, UnknownId};
use crate::math::{Point3, Vector3};
use crate::scalar::FiniteReal;
use crate::transform::Transform;
#[cfg(feature = "schema")]
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One component and its native construction scalar.
// A source states a raw scalar; a compound construction holds the admitted
// component, whose scalar is a `FiniteReal`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct CompoundComponent<T, R = f64> {
    /// Scalar paired with this component.
    pub parameter: R,
    /// Component geometry or its resolved identity.
    pub component: T,
}

/// A non-empty compound curve with finite construction parameters.
#[derive(Debug, Clone, PartialEq, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(try_from = "CompoundCurveConstructionWire")]
pub struct CompoundCurveConstruction {
    pub(super) parameters: Vec<FiniteReal>,
    #[cfg_attr(feature = "schema", schemars(with = "Vec<CompoundComponent<CurveId>>"))]
    pub(super) components: Vec<CompoundComponent<CurveId, FiniteReal>>,
    /// Solved-cache fit contract this construction states itself.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_cache"
    )]
    pub(super) cache: Option<LegacyCache>,
}

#[derive(Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub(super) struct CompoundCurveConstructionWire {
    /// Finite leading and per-component construction parameters.
    pub(super) parameters: Vec<f64>,
    /// Component curves in construction order.
    pub(super) components: Vec<CompoundComponent<CurveId>>,
    /// Solved-cache fit contract this construction states itself.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_cache"
    )]
    pub(super) cache: Option<LegacyCache>,
}

impl TryFrom<CompoundCurveConstructionWire> for CompoundCurveConstruction {
    type Error = &'static str;
    fn try_from(wire: CompoundCurveConstructionWire) -> Result<Self, Self::Error> {
        Self::try_new(wire.parameters, wire.components, wire.cache)
    }
}

impl CompoundCurveConstruction {
    /// Admit at least one component and finite leading and component parameters.
    pub fn try_new(
        parameters: Vec<f64>,
        components: Vec<CompoundComponent<CurveId>>,
        cache: Option<LegacyCache>,
    ) -> Result<Self, &'static str> {
        const INVALID: &str = "compound curve parameters must be finite";
        if components.is_empty() {
            return Err("compound curve components must not be empty");
        }
        let [parameters] = FiniteReal::lanes([parameters]).ok_or(INVALID)?;
        let components = components
            .into_iter()
            .map(CompoundComponent::admit)
            .collect::<Option<Vec<_>>>()
            .ok_or(INVALID)?;
        Ok(Self {
            parameters,
            components,
            cache,
        })
    }

    /// Return the parameters.
    #[must_use]
    pub fn parameters(&self) -> &[FiniteReal] {
        &self.parameters
    }

    /// Return the components.
    #[must_use]
    pub fn components(&self) -> &[CompoundComponent<CurveId, FiniteReal>] {
        &self.components
    }
}

impl ProceduralCurveDefinition {
    /// Solved-cache fit contract this construction states outside a
    /// revision-gated cache form.
    #[must_use]
    pub fn legacy_cache(&self) -> Option<LegacyCache> {
        match self {
            Self::Compound(construction) => construction.legacy_cache(),
            Self::Helix(construction) => construction.legacy_cache(),
            Self::Subset(payload) => payload.legacy_cache(),
            Self::VectorOffset(payload) => payload.legacy_cache(),
            Self::TwoSidedOffset(payload) => payload.legacy_cache(),
            Self::Spring(payload) => payload.legacy_cache(),
            Self::SurfaceOffset(payload) => payload.legacy_cache(),
            Self::Exact { cache }
            | Self::Law { cache, .. }
            | Self::Intersection { cache, .. }
            | Self::TolerantIntersection { cache, .. }
            | Self::Unknown { cache, .. } => *cache,
            Self::ThreeSurfaceIntersection(_)
            | Self::SurfaceCurve { .. }
            | Self::Silhouette(_)
            | Self::Deformable(_)
            | Self::Projection(_)
            | Self::Offset(_)
            | Self::SpatialOffset(_)
            | Self::Replica { .. }
            | Self::BlendSpine { .. } => None,
        }
    }

    /// Mutable legacy solved-cache slot of this construction, absent when the
    /// layout states none.
    ///
    /// The one write route to the legacy cache: the setter, the clear and the
    /// raise go through it, so a construction cannot answer them differently.
    /// No curve construction holds a cache it cannot lose, so the slot is a
    /// plain `&mut Option<LegacyCache>` and `LegacyCacheSlot` is surface-only.
    pub(super) fn legacy_cache_slot_mut(&mut self) -> Option<&mut Option<LegacyCache>> {
        match self {
            Self::Compound(construction) => Some(construction.legacy_cache_slot_mut()),
            Self::Helix(construction) => Some(construction.legacy_cache_slot_mut()),
            Self::Subset(payload) => Some(payload.legacy_cache_slot_mut()),
            Self::VectorOffset(payload) => Some(payload.legacy_cache_slot_mut()),
            Self::TwoSidedOffset(payload) => Some(payload.legacy_cache_slot_mut()),
            Self::Spring(payload) => payload.legacy_cache_slot_mut(),
            Self::SurfaceOffset(payload) => payload.legacy_cache_slot_mut(),
            Self::Exact { cache }
            | Self::Law { cache, .. }
            | Self::Intersection { cache, .. }
            | Self::TolerantIntersection { cache, .. }
            | Self::Unknown { cache, .. } => Some(cache),
            Self::ThreeSurfaceIntersection(_)
            | Self::SurfaceCurve { .. }
            | Self::Silhouette(_)
            | Self::Deformable(_)
            | Self::Projection(_)
            | Self::Offset(_)
            | Self::SpatialOffset(_)
            | Self::Replica { .. }
            | Self::BlendSpine { .. } => None,
        }
    }

    /// State the legacy solved-cache fit contract. A construction whose
    /// layout states none refuses a contract.
    pub fn set_legacy_cache(&mut self, cache: LegacyCache) -> Result<(), CacheContractError> {
        match self.legacy_cache_slot_mut() {
            Some(slot) => {
                *slot = Some(cache);
                Ok(())
            }
            None => Err(CacheContractError::Layout(NO_LEGACY_SLOT)),
        }
    }

    /// Clear the legacy solved-cache fit contract.
    ///
    /// Total: a construction with a slot loses the contract it states, and a
    /// construction with no slot has none to lose. No curve construction holds
    /// a cache it cannot lose, so this refuses nothing.
    pub fn clear_legacy_cache(&mut self) {
        if let Some(slot) = self.legacy_cache_slot_mut() {
            *slot = None;
        }
    }

    /// Effective fit tolerance of the solved cache.
    #[must_use]
    pub fn cache_fit_tolerance(&self) -> Option<FitTolerance> {
        match self.revision_cache() {
            Some(RevisionCacheForm::SolvedCache { fit_tolerance }) => Some(*fit_tolerance),
            Some(RevisionCacheForm::Parameterization(_)) => None,
            None => self.legacy_cache().map(|cache| cache.fit_tolerance),
        }
    }

    /// Change the effective fit tolerance without permitting a parameterized
    /// cache to acquire one or a solved cache to lose it.
    pub fn set_cache_fit_tolerance(
        &mut self,
        value: Option<FitTolerance>,
    ) -> Result<(), CacheContractError> {
        if self.owns_revision_cache() {
            return match value {
                Some(value) => self
                    .write_revision_fit_tolerance(ToleranceWrite::State(value))
                    .into_set_result(),
                None => clear_revision_form_tolerance(self.revision_cache()),
            };
        }
        match value {
            Some(value) => self.set_legacy_cache(LegacyCache::new(value)),
            None => {
                self.clear_legacy_cache();
                Ok(())
            }
        }
    }

    /// Scale only the fit tolerance already carried by this construction.
    pub(super) fn scale_cache_fit_tolerance(
        &mut self,
        scale: crate::scalar::PositiveReal,
    ) -> Result<(), CacheContractError> {
        match self.write_revision_fit_tolerance(ToleranceWrite::Scale(scale)) {
            RevisionCacheWrite::Written | RevisionCacheWrite::Parameterized => Ok(()),
            RevisionCacheWrite::NoForm => {
                if let Some(Some(cache)) = self.legacy_cache_slot_mut() {
                    let value = cache.fit_tolerance;
                    cache.fit_tolerance =
                        value
                            .scaled(scale)
                            .ok_or_else(|| CacheContractError::InvalidValue {
                                value: value.get() * scale.get(),
                            })?;
                }
                Ok(())
            }
            RevisionCacheWrite::InvalidValue(value) => {
                Err(CacheContractError::InvalidValue { value })
            }
        }
    }

    /// State that a solved carrier of this construction was fitted to `value`,
    /// raising an existing contract rather than lowering it.
    ///
    /// Answers every layout. A construction whose legacy slot is empty takes
    /// the contract; one that already carries a cache raises it; a solved
    /// revision cache raises its own tolerance. A parameterized form and a
    /// layout that states no slot have no solved cache to state, so they
    /// refuse: a caller that asked for a solved cache never gets a silent
    /// nothing. The write goes through `legacy_cache_slot_mut`, the one write
    /// route to the slot, so a construction cannot answer it differently from
    /// the setter and the clear.
    pub fn require_cache_fit_tolerance(
        &mut self,
        value: FitTolerance,
    ) -> Result<(), CacheContractError> {
        match self.write_revision_fit_tolerance(ToleranceWrite::Raise(value)) {
            RevisionCacheWrite::Written => Ok(()),
            RevisionCacheWrite::Parameterized => {
                Err(CacheContractError::Layout(PARAMETERIZED_NO_SOLVED_CACHE))
            }
            RevisionCacheWrite::NoForm => match self.legacy_cache_slot_mut() {
                Some(slot @ None) => {
                    *slot = Some(LegacyCache::new(value));
                    Ok(())
                }
                Some(Some(cache)) => {
                    if value.get() > cache.fit_tolerance.get() {
                        cache.fit_tolerance = value;
                    }
                    Ok(())
                }
                None => Err(CacheContractError::Layout(NO_LEGACY_SLOT)),
            },
            RevisionCacheWrite::InvalidValue(value) => {
                Err(CacheContractError::InvalidValue { value })
            }
        }
    }
}

/// A parameter-space support curve and its optional affine parameter map.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct SupportPcurve {
    /// UV curve carried by the support side.
    pub geometry: PcurveGeometry,
    /// Directed pcurve interval corresponding to the solved interval.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_parameter_range"
    )]
    pub parameter_range: Option<DirectedParameterRange>,
}

impl SupportPcurve {
    /// Pair a pcurve geometry with an optional checked parameter interval.
    #[must_use]
    pub const fn new(
        geometry: PcurveGeometry,
        parameter_range: Option<DirectedParameterRange>,
    ) -> Self {
        Self {
            geometry,
            parameter_range,
        }
    }
}

impl From<PcurveGeometry> for SupportPcurve {
    fn from(geometry: PcurveGeometry) -> Self {
        Self::new(geometry, None)
    }
}

/// A finite, non-zero-width directed parameter interval.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(transparent)]
pub struct DirectedParameterRange([f64; 2]);

/// Error returned when a directed parameter range cannot be admitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParameterRangeError;

impl std::fmt::Display for ParameterRangeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("parameter range endpoints must be finite and distinct")
    }
}

impl std::error::Error for ParameterRangeError {}

impl DirectedParameterRange {
    /// Build from finite angle endpoints, checking only that they differ.
    pub fn from_angle_endpoints(
        values: [crate::scalar::Angle; 2],
    ) -> Result<Self, ParameterRangeError> {
        (values[0] != values[1])
            .then_some(Self(values.map(crate::scalar::Angle::get)))
            .ok_or(ParameterRangeError)
    }

    /// Build from finite endpoints, checking only that they differ.
    pub fn from_finite_endpoints(
        values: [crate::scalar::FiniteReal; 2],
    ) -> Result<Self, ParameterRangeError> {
        (values[0] != values[1])
            .then_some(Self(values.map(crate::scalar::FiniteReal::get)))
            .ok_or(ParameterRangeError)
    }

    /// Construct an interval. Either ascending or descending direction is valid.
    pub fn new(values: [f64; 2]) -> Result<Self, ParameterRangeError> {
        let [first, second] = values.map(FiniteReal::new);
        let [Some(first), Some(second)] = [first, second] else {
            return Err(ParameterRangeError);
        };
        Self::from_finite_endpoints([first, second])
    }

    /// Return the directed endpoints.
    #[must_use]
    pub const fn endpoints(self) -> [f64; 2] {
        self.0
    }
}

impl<'de> Deserialize<'de> for DirectedParameterRange {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        Self::new(<[f64; 2]>::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

/// Refuse a zero-width interval under a support side with an explicit pcurve
/// mapping.
pub(super) fn require_mapped_interval(
    sides: &[IntcurveSupportSide; 2],
    parameter_range: crate::topology::ParameterInterval,
) -> Result<(), &'static str> {
    let [start, end] = parameter_range.endpoints();
    if start == end
        && sides.iter().any(|side| {
            side.pcurve
                .as_ref()
                .is_some_and(|pcurve| pcurve.parameter_range.is_some())
        })
    {
        return Err(
            "support context parameter_range must be nonzero for an explicit pcurve mapping",
        );
    }
    Ok(())
}

/// Discriminator-specific payload of a deformable native intcurve.
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
pub enum DeformableCurveData<R = f64, V = Vector3, P = Point3> {
    /// Mode 8 vector field followed by ordered scalar pairs.
    VectorField {
        /// Four ordered native vectors.
        vectors: [V; 4],
        /// Ordered pairs from the mode-8 scalar table.
        parameter_pairs: Vec<[R; 2]>,
    },
    /// Mode 3 fixed deformation payload.
    Mode3 {
        /// Four vectors at the start of the payload.
        leading_vectors: [V; 4],
        /// Scalar following the leading vectors.
        leading_parameter: R,
        /// Three flags following the leading scalar.
        leading_flags: [bool; 3],
        /// Position following the leading flags.
        trailing_point: P,
        /// Two vectors following the position.
        trailing_vectors: [V; 2],
        /// Scalar following the trailing frame.
        frame_parameter: R,
        /// Two flags following the frame scalar.
        frame_flags: [bool; 2],
        /// Three ordered scalars following the frame flags.
        parameters: [R; 3],
        /// Five flags following the ordered scalars.
        trailing_flags: [bool; 5],
        /// Final scalar before the trailing integer.
        trailing_parameter: R,
        /// Integer closing the mode-3 payload.
        trailing_value: i64,
    },
}

/// Source slot of a deformable native intcurve.
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
pub enum DeformableCurveSource {
    /// Source geometry resolved to a neutral curve carrier.
    Curve {
        /// Curve being deformed.
        curve: CurveId,
    },
    /// Native intcurve reference whose target is absent from the active subtype table.
    NativeReference {
        /// Boolean stored before the reference scope.
        flag: bool,
        /// Integer stored by the native `ref` subtype.
        index: i64,
    },
}

/// Neutral semantics for a procedural curve.
#[derive(Debug, Clone, PartialEq, Serialize, cadmpeg_derive::StructuralOrd)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ProceduralCurveDefinition {
    /// An exact native intcurve whose solved NURBS cache is authoritative.
    Exact {
        /// Solved-cache fit contract this construction states itself.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache: Option<LegacyCache>,
    },
    /// Curve defined by recursive native law formulas.
    Law {
        /// Shared support surfaces, UV curves, interval, and discontinuities.
        context: IntcurveSupportContext,
        /// Version-stamped serializer form, absent for the legacy layout.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        version: Option<LawCurveVersionForm>,
        /// Native ASM extension integer.
        extension: i64,
        /// Primary recursive law formula.
        #[cfg_attr(feature = "schema", schemars(with = "LawFormula"))]
        primary: FiniteLawFormula,
        /// Counted additional recursive law formulas.
        #[cfg_attr(feature = "schema", schemars(with = "Vec<LawFormula>"))]
        additional: Vec<FiniteLawFormula>,
        /// Solved-cache fit contract this construction states itself.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache: Option<LegacyCache>,
    },
    /// Ordered compound of native child curves with construction parameters.
    Compound(CompoundCurveConstruction),
    /// Circular or conical helix around an axis.
    Helix(HelixCurveConstruction),
    /// Intersection of two support surfaces.
    Intersection {
        /// Shared surfaces, UV curves, interval, and discontinuity metadata.
        context: IntcurveSupportContext,
        /// Native boolean following the discontinuity arrays.
        discontinuity_flag: bool,
        /// Solved-cache fit contract this construction states itself.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache: Option<LegacyCache>,
    },
    /// Tolerance-bounded intersection relation selected by topology endpoints.
    TolerantIntersection {
        /// Distinct supports and finite endpoint bounds.
        construction: TolerantIntersectionConstruction,
        /// Atomic neutral parameterization established by validated support charts.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        parameterization: Option<TolerantIntersectionParameterization>,
        /// Solved-cache fit contract this construction states itself.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache: Option<LegacyCache>,
    },
    /// Intersection constrained by a third ordered support surface.
    ThreeSurfaceIntersection(super::curve_payloads::ThreeSurfaceIntersectionCurvePayload),
    /// Surface-related curve whose native subtype has no tail beyond the shared prefix.
    SurfaceCurve {
        /// Native family, support context, and optional cache-first tail.
        family: SurfaceCurveFamily,
    },
    /// Silhouette of a cast surface in a light direction.
    Silhouette(super::curve_payloads::SilhouetteCurveConstruction),
    /// Curve offset relative to a surface parameterization.
    SurfaceOffset(super::curve_payloads::SurfaceOffsetCurveConstruction),
    /// Blend spring guide between two support sides.
    Spring(super::curve_payloads::SpringCurvePayload),
    /// Deformation of an embedded source curve.
    Deformable(super::curve_payloads::DeformableCurveConstruction),
    /// Projection of a source curve onto a support surface.
    Projection(super::curve_payloads::ProjectionCurvePayload),
    /// Offset from a source curve.
    Offset(super::curve_payloads::OffsetCurveConstruction),
    /// Free-space 3D offset using a reference direction.
    SpatialOffset(super::curve_payloads::SpatialOffsetCurveConstruction),
    /// Intersection of two surfaces after applying independent signed offsets.
    TwoSidedOffset(super::curve_payloads::TwoSidedOffsetCurveConstruction),
    /// Free-space vector offset of a source curve over a parameter interval.
    VectorOffset(super::curve_payloads::VectorOffsetCurveConstruction),
    /// A parameter sub-range of a parent curve.
    Subset(super::curve_payloads::SubsetCurveConstruction),
    /// Affine replica of a curve carrier, retaining the parent curve's
    /// parameter range and parameterization.
    Replica {
        /// Curve being replicated.
        source: CurveId,
        /// Affine map from the parent curve coordinates to this curve.
        transform: Transform,
    },
    /// Spine or center curve of a blend surface.
    BlendSpine {
        /// The blend surface this curve is the spine of, when known.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        blend_surface: Option<SurfaceId>,
    },
    /// Preserved construction without a neutral interpretation.
    Unknown {
        /// Source construction-family discriminator, when known.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        native_kind: Option<String>,
        /// Reference to the preserved raw source record, when retained.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        record: Option<UnknownId>,
        /// Solved-cache fit contract this construction states itself.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cache: Option<LegacyCache>,
    },
}

#[derive(Deserialize)]
#[serde(
    remote = "ProceduralCurveDefinition",
    tag = "kind",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub(super) enum ProceduralCurveDefinitionWire {
    Exact {
        /// Solved-cache fit contract this construction states itself.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_cache"
        )]
        cache: Option<LegacyCache>,
    },
    Law {
        context: IntcurveSupportContext,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_version"
        )]
        version: Option<LawCurveVersionForm>,
        extension: i64,
        primary: FiniteLawFormula,
        additional: Vec<FiniteLawFormula>,
        /// Solved-cache fit contract this construction states itself.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_cache"
        )]
        cache: Option<LegacyCache>,
    },
    Compound(CompoundCurveConstruction),
    Helix(HelixCurveConstruction),
    Intersection {
        context: IntcurveSupportContext,
        discontinuity_flag: bool,
        /// Solved-cache fit contract this construction states itself.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_cache"
        )]
        cache: Option<LegacyCache>,
    },
    TolerantIntersection {
        construction: TolerantIntersectionConstruction,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_parameterization"
        )]
        parameterization: Option<TolerantIntersectionParameterization>,
        /// Solved-cache fit contract this construction states itself.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_cache"
        )]
        cache: Option<LegacyCache>,
    },
    ThreeSurfaceIntersection(super::curve_payloads::ThreeSurfaceIntersectionCurvePayload),
    SurfaceCurve {
        family: SurfaceCurveFamily,
    },
    Silhouette(super::curve_payloads::SilhouetteCurveConstruction),
    SurfaceOffset(super::curve_payloads::SurfaceOffsetCurveConstruction),
    Spring(super::curve_payloads::SpringCurvePayload),
    Deformable(super::curve_payloads::DeformableCurveConstruction),
    Projection(super::curve_payloads::ProjectionCurvePayload),
    Offset(super::curve_payloads::OffsetCurveConstruction),
    SpatialOffset(super::curve_payloads::SpatialOffsetCurveConstruction),
    TwoSidedOffset(super::curve_payloads::TwoSidedOffsetCurveConstruction),
    VectorOffset(super::curve_payloads::VectorOffsetCurveConstruction),
    Subset(super::curve_payloads::SubsetCurveConstruction),
    Replica {
        source: CurveId,
        transform: Transform,
    },
    BlendSpine {
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_blend_surface"
        )]
        blend_surface: Option<SurfaceId>,
    },
    Unknown {
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_native_kind"
        )]
        native_kind: Option<String>,
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_record"
        )]
        record: Option<UnknownId>,
        /// Solved-cache fit contract this construction states itself.
        #[serde(
            default,
            skip_serializing_if = "Option::is_none",
            deserialize_with = "deserialize_cache"
        )]
        cache: Option<LegacyCache>,
    },
}

impl<'de> Deserialize<'de> for ProceduralCurveDefinition {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        ProceduralCurveDefinitionWire::deserialize(deserializer)
    }
}

/// Codes attached to the two native vector-offset roles.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, cadmpeg_derive::StructuralOrd,
)]
#[cfg_attr(feature = "schema", derive(JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct VectorOffsetRoles {
    /// Code of the source role.
    pub source: i64,
    /// Code of the offset role.
    pub offset: i64,
}

impl ProceduralCurveDefinition {
    pub(super) fn revision_cache(
        &self,
    ) -> Option<&RevisionCacheForm<CacheFirstCurveParameterization<FiniteReal>>> {
        match self {
            Self::SurfaceCurve { family } => family.revision_cache(),
            Self::SurfaceOffset(payload) => payload.cache_first().as_ref().map(|form| &form.cache),
            Self::Spring(definition_payload) => {
                let layout = definition_payload.layout();
                layout.cache_first().map(|form| &form.cache)
            }
            Self::Deformable(definition_payload) => {
                let cache_first = definition_payload.cache_first();
                Some(&cache_first.cache)
            }
            _ => None,
        }
    }

    /// Write the fit tolerance of the revision-gated solved cache.
    ///
    /// The narrow write route: the borrow of the cache form stays inside this
    /// method, so no construction lends its admitted interior for writing.
    pub(super) fn write_revision_fit_tolerance(
        &mut self,
        write: ToleranceWrite,
    ) -> RevisionCacheWrite {
        match self {
            Self::SurfaceCurve { family } => family.write_revision_fit_tolerance(write),
            Self::SurfaceOffset(payload) => payload.write_revision_fit_tolerance(write),
            Self::Spring(payload) => payload.write_revision_fit_tolerance(write),
            Self::Deformable(payload) => payload.write_revision_fit_tolerance(write),
            _ => RevisionCacheWrite::NoForm,
        }
    }
}

impl ProceduralCurveDefinition {
    /// Whether the construction owns a revision-gated cache form, which then
    /// states its solved-cache fit tolerance.
    #[must_use]
    pub fn owns_revision_cache(&self) -> bool {
        self.revision_cache().is_some()
    }
}

impl CompoundCurveConstruction {
    /// Solved-cache fit contract this construction states.
    #[must_use]
    pub const fn legacy_cache(&self) -> Option<LegacyCache> {
        self.cache
    }

    /// Mutable legacy solved-cache slot this construction states.
    pub(super) const fn legacy_cache_slot_mut(&mut self) -> &mut Option<LegacyCache> {
        &mut self.cache
    }
}

cadmpeg_core::named_optional_field!(deserialize_cache, LegacyCache, "cache");
cadmpeg_core::named_optional_field!(deserialize_record, UnknownId, "record");

cadmpeg_core::named_optional_field!(
    deserialize_parameter_range,
    DirectedParameterRange,
    "parameter_range"
);

cadmpeg_core::named_optional_field!(deserialize_version, LawCurveVersionForm, "version");
cadmpeg_core::named_optional_field!(
    deserialize_parameterization,
    TolerantIntersectionParameterization,
    "parameterization"
);
cadmpeg_core::named_optional_field!(deserialize_blend_surface, SurfaceId, "blend_surface");
cadmpeg_core::named_optional_field!(deserialize_native_kind, String, "native_kind");
